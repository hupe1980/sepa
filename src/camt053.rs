//! ISO 20022 camt.053 — Bank-to-Customer Statement parser.
//!
//! Parses end-of-day electronic bank statements (the SEPA equivalent of MT940).
//! Supports all common DK/EPC namespace variants.
//!
//! ## Difference between camt.053 and camt.054
//!
//! | | camt.053 | camt.054 |
//! |---|---|---|
//! | Type | End-of-day statement | Intraday notification |
//! | Content | All booked entries + balances | Specific debit/credit events |
//! | Frequency | Daily (T+1) | Real-time or batch |
//! | Use case | Ledger reconciliation | Cash management alerts |
//!
//! ## Supported namespaces
//!
//! | Schema | Used by |
//! |---|---|
//! | `camt.053.001.02` | Deutsche Kreditwirtschaft V2 |
//! | `camt.053.001.06` | Deutsche Kreditwirtschaft V6 (current) |
//! | `camt.053.001.08` | Newer EPC/ISO version |
//!
//! ## Example
//!
//! ```rust
//! use sepa::camt053::{parse_camt053, BalanceType};
//!
//! let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
//! <Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.053.001.06">
//!   <BkToCstmrStmt>
//!     <GrpHdr><MsgId>STMT-001</MsgId><CreDtTm>2026-07-14T23:59:00</CreDtTm></GrpHdr>
//!     <Stmt>
//!       <Id>2026-07-14</Id>
//!       <Acct><Id><IBAN>DE89370400440532013000</IBAN></Id></Acct>
//!       <Bal>
//!         <Tp><CdOrPrtry><Cd>OPBD</Cd></CdOrPrtry></Tp>
//!         <Amt Ccy="EUR">1000.00</Amt>
//!         <CdtDbtInd>CRDT</CdtDbtInd>
//!         <Dt><Dt>2026-07-13</Dt></Dt>
//!       </Bal>
//!       <Bal>
//!         <Tp><CdOrPrtry><Cd>CLBD</Cd></CdOrPrtry></Tp>
//!         <Amt Ccy="EUR">1155.42</Amt>
//!         <CdtDbtInd>CRDT</CdtDbtInd>
//!         <Dt><Dt>2026-07-14</Dt></Dt>
//!       </Bal>
//!     </Stmt>
//!   </BkToCstmrStmt>
//! </Document>"#;
//!
//! let doc = parse_camt053(xml).unwrap();
//! let stmt = &doc.statements[0];
//! assert_eq!(stmt.account_iban, "DE89370400440532013000");
//! let closing = stmt.closing_balance().unwrap();
//! assert_eq!(closing.amount_ct, 115_542);
//! ```

use crate::camt054::CreditDebitIndicator;
use crate::xml_util::{normalize_ns, xml_detect_ns, xml_each_iter, xml_inner, xml_text};

// ── known namespaces ──────────────────────────────────────────────────────────

/// Known camt.053 XML namespace URIs.
pub mod ns {
    /// Deutsche Kreditwirtschaft V2.
    pub const CAMT053_001_02: &str = "urn:iso:std:iso:20022:tech:xsd:camt.053.001.02";
    /// Deutsche Kreditwirtschaft V6 — most common in Germany (current).
    pub const CAMT053_001_06: &str = "urn:iso:std:iso:20022:tech:xsd:camt.053.001.06";
    /// Newer ISO 20022 version.
    pub const CAMT053_001_08: &str = "urn:iso:std:iso:20022:tech:xsd:camt.053.001.08";
}

// ── BalanceType ───────────────────────────────────────────────────────────────

/// Type of a balance in a camt.053 statement.
///
/// Appears as `Bal/Tp/CdOrPrtry/Cd`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum BalanceType {
    /// `OPBD` — Opening Booked: balance at start of statement period.
    OpeningBooked,
    /// `CLBD` — Closing Booked: balance at end of statement period.
    ClosingBooked,
    /// `ITBD` — Intraday Booked: intermediate booked balance.
    IntradayBooked,
    /// `CLAV` — Closing Available: available funds at end of period.
    ClosingAvailable,
    /// `OPAV` — Opening Available: available funds at start of period.
    OpeningAvailable,
    /// `FWAV` — Forward Available: future available balance.
    ForwardAvailable,
    /// Any other balance type code.
    Other(String),
}

impl BalanceType {
    /// ISO 20022 code string.
    #[must_use]
    pub fn as_code(&self) -> &str {
        match self {
            Self::OpeningBooked => "OPBD",
            Self::ClosingBooked => "CLBD",
            Self::IntradayBooked => "ITBD",
            Self::ClosingAvailable => "CLAV",
            Self::OpeningAvailable => "OPAV",
            Self::ForwardAvailable => "FWAV",
            Self::Other(s) => s,
        }
    }

    fn from_code(s: &str) -> Self {
        match s.trim().to_ascii_uppercase().as_str() {
            "OPBD" => Self::OpeningBooked,
            "CLBD" => Self::ClosingBooked,
            "ITBD" => Self::IntradayBooked,
            "CLAV" => Self::ClosingAvailable,
            "OPAV" => Self::OpeningAvailable,
            "FWAV" => Self::ForwardAvailable,
            other => Self::Other(other.to_owned()),
        }
    }
}

// ── EntryStatus ───────────────────────────────────────────────────────────────

/// Booking status of a camt.053 entry (`Ntry/Sts`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum EntryStatus {
    /// `BOOK` — Booked / settled.
    Booked,
    /// `PDNG` — Pending (not yet settled).
    Pending,
    /// `INFO` — Informational only.
    Info,
    /// `FUTR` — Future-dated entry.
    Future,
    /// Any other status code.
    Other(String),
}

impl EntryStatus {
    fn from_code(s: &str) -> Self {
        match s.trim().to_ascii_uppercase().as_str() {
            "BOOK" => Self::Booked,
            "PDNG" => Self::Pending,
            "INFO" => Self::Info,
            "FUTR" => Self::Future,
            other => Self::Other(other.to_owned()),
        }
    }
}

// ── StatementBalance ──────────────────────────────────────────────────────────

/// A balance entry within a camt.053 statement.
#[derive(Debug, Clone)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct StatementBalance {
    /// Balance type (opening booked, closing booked, …).
    pub balance_type: BalanceType,
    /// Amount in **ct** (1/100 EUR). Always positive.
    pub amount_ct: i64,
    /// Whether the balance is a credit (positive) or debit (negative) balance.
    pub indicator: CreditDebitIndicator,
    /// Balance date, ISO 8601 `"YYYY-MM-DD"`.
    pub date: String,
}

impl StatementBalance {
    /// Balance as signed ct value (+credit, −debit).
    #[inline]
    #[must_use]
    pub fn signed_ct(&self) -> i64 {
        match self.indicator {
            CreditDebitIndicator::Credit => self.amount_ct,
            CreditDebitIndicator::Debit => -self.amount_ct,
        }
    }
}

// ── Camt053Entry ──────────────────────────────────────────────────────────────

/// A single booked or pending entry in a camt.053 statement.
#[derive(Debug, Clone)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Camt053Entry {
    /// Amount in **ct** (1/100 EUR). Always positive — see `indicator`.
    pub amount_ct: i64,
    /// Credit (incoming) or Debit (outgoing).
    pub indicator: CreditDebitIndicator,
    /// Booking status.
    pub status: EntryStatus,
    /// Booking date (`BookgDt/Dt`), ISO 8601.
    pub booking_date: Option<String>,
    /// Value date (`ValDt/Dt`), ISO 8601.
    pub value_date: Option<String>,
    /// Bank's internal transaction reference (`AcctSvcrRef`).
    pub account_servicer_ref: Option<String>,
    /// Bank transaction code string (raw `BkTxCd` inner content).
    pub bank_tx_code: Option<String>,
    /// End-to-end reference from original payment instruction.
    pub end_to_end_id: Option<String>,
    /// Mandate reference (for direct debits, `MndtId`).
    pub mandate_id: Option<String>,
    /// SEPA Creditor Identifier (`CdtrId`).
    pub creditor_id: Option<String>,
    /// Remittance information / payment reference (`RmtInf/Ustrd`).
    pub reference: Option<String>,
    /// Counterparty name (debtor for credits; creditor for debits).
    pub counterparty_name: Option<String>,
    /// Counterparty IBAN.
    pub counterparty_iban: Option<String>,
    /// ISO 20022 return reason code, if this entry is a return.
    pub return_reason_code: Option<String>,
}

impl Camt053Entry {
    /// Signed ledger amount: credit is positive (balance increase),
    /// debit is negative (balance decrease).
    #[inline]
    #[must_use]
    pub fn signed_ct(&self) -> i64 {
        match self.indicator {
            CreditDebitIndicator::Credit => self.amount_ct,
            CreditDebitIndicator::Debit => -self.amount_ct,
        }
    }

    /// Returns `true` if this entry is a SEPA return / Rückbuchung.
    #[inline]
    #[must_use]
    pub fn is_return(&self) -> bool {
        self.return_reason_code.is_some()
    }
}

// ── Camt053Statement ──────────────────────────────────────────────────────────

/// A single account statement within a camt.053 document.
///
/// Typically one statement per account per day.
#[derive(Debug, Clone)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Camt053Statement {
    /// Statement ID (`Id`).
    pub stmt_id: String,
    /// Electronic sequence number.
    pub sequence_number: Option<u64>,
    /// Account IBAN.
    pub account_iban: String,
    /// Statement period start, ISO 8601.
    pub from_date: Option<String>,
    /// Statement period end, ISO 8601.
    pub to_date: Option<String>,
    /// All balances in this statement.
    pub balances: Vec<StatementBalance>,
    /// All entries (booked and pending transactions).
    pub entries: Vec<Camt053Entry>,
}

impl Camt053Statement {
    /// Opening booked balance (`OPBD`), if present.
    #[must_use]
    pub fn opening_balance(&self) -> Option<&StatementBalance> {
        self.balances
            .iter()
            .find(|b| b.balance_type == BalanceType::OpeningBooked)
    }

    /// Closing booked balance (`CLBD`), if present.
    #[must_use]
    pub fn closing_balance(&self) -> Option<&StatementBalance> {
        self.balances
            .iter()
            .find(|b| b.balance_type == BalanceType::ClosingBooked)
    }

    /// Net movement in ct for this statement (sum of signed entry amounts).
    #[must_use]
    pub fn net_movement_ct(&self) -> i64 {
        self.entries.iter().map(|e| e.signed_ct()).sum()
    }
}

// ── Camt053Document ───────────────────────────────────────────────────────────

/// A parsed camt.053 Bank-to-Customer Statement document.
///
/// Produced by [`parse_camt053`].
#[derive(Debug, Clone)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Camt053Document {
    /// Document message ID.
    pub msg_id: String,
    /// Document creation timestamp.
    pub created_at: String,
    /// Detected XML namespace URI.
    pub namespace: Option<String>,
    /// One or more statements (one per account, typically one per document).
    pub statements: Vec<Camt053Statement>,
}

// ── Error ─────────────────────────────────────────────────────────────────────

/// Error returned when camt.053 XML cannot be parsed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum Camt053ParseError {
    /// Root element `BkToCstmrStmt` not found — not a camt.053 document.
    #[error("not a camt.053 document: root element <BkToCstmrStmt> not found")]
    NotCamt053,
    /// A required XML element was absent.
    #[error("missing required camt.053 element: <{tag}>")]
    MissingElement {
        /// Name of the missing element.
        tag: &'static str,
    },
}

// ── Parser ────────────────────────────────────────────────────────────────────

/// Parse a camt.053 Bank-to-Customer Statement XML string.
///
/// Accepts all common DK/EPC namespace variants and prefixed documents.
///
/// # Errors
///
/// Returns [`Camt053ParseError::NotCamt053`] when the root element is missing.
pub fn parse_camt053(xml: &str) -> Result<Camt053Document, Camt053ParseError> {
    let namespace = xml_detect_ns(xml);
    let xml = normalize_ns(xml);

    let root = xml_inner(&xml, "BkToCstmrStmt").ok_or(Camt053ParseError::NotCamt053)?;

    let grp_hdr = xml_inner(root, "GrpHdr");
    let msg_id = grp_hdr
        .and_then(|h| xml_text(h, "MsgId"))
        .unwrap_or("")
        .to_owned();
    let created_at = grp_hdr
        .and_then(|h| xml_text(h, "CreDtTm"))
        .unwrap_or("")
        .to_owned();

    let statements = xml_each_iter(root, "Stmt")
        .map(parse_statement)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Camt053Document {
        msg_id,
        created_at,
        namespace,
        statements,
    })
}

fn parse_statement(s: &str) -> Result<Camt053Statement, Camt053ParseError> {
    let stmt_id = xml_text(s, "Id").unwrap_or("").to_owned();

    let sequence_number = xml_text(s, "ElctrncSeqNb").and_then(|v| v.parse::<u64>().ok());

    let account_iban = xml_inner(s, "Acct")
        .and_then(|a| xml_text(a, "IBAN"))
        .unwrap_or("")
        .to_owned();

    // Period: try FrToDt first (FrDtTm or FrDt child elements).
    // Call xml_inner once; Option<&str> is Copy so we reuse for from/to.
    let fr_to_dt = xml_inner(s, "FrToDt");
    let from_date = fr_to_dt.and_then(|d| {
        xml_text(d, "FrDtTm")
            .or_else(|| xml_text(d, "FrDt"))
            .map(str::to_owned)
    });
    let to_date = fr_to_dt.and_then(|d| {
        xml_text(d, "ToDtTm")
            .or_else(|| xml_text(d, "ToDt"))
            .map(str::to_owned)
    });

    let balances = xml_each_iter(s, "Bal").filter_map(parse_balance).collect();

    let entries = xml_each_iter(s, "Ntry").filter_map(parse_entry).collect();

    Ok(Camt053Statement {
        stmt_id,
        sequence_number,
        account_iban,
        from_date,
        to_date,
        balances,
        entries,
    })
}

fn parse_balance(b: &str) -> Option<StatementBalance> {
    // Balance type: Tp/CdOrPrtry/Cd
    let balance_type = xml_inner(b, "Tp")
        .and_then(|tp| xml_inner(tp, "CdOrPrtry"))
        .and_then(|cp| xml_text(cp, "Cd"))
        .map(BalanceType::from_code)
        .unwrap_or(BalanceType::Other(String::new()));

    let amount_ct = xml_text(b, "Amt").and_then(crate::ct_from_eur_str)?;

    let indicator = xml_text(b, "CdtDbtInd")
        .and_then(|s| s.parse::<CreditDebitIndicator>().ok())
        .unwrap_or(CreditDebitIndicator::Credit);

    // Date: Dt/Dt or Dt/DtTm
    let date = xml_inner(b, "Dt")
        .and_then(|d| {
            xml_text(d, "Dt")
                .or_else(|| xml_text(d, "DtTm"))
                .map(str::to_owned)
        })
        .unwrap_or_default();

    Some(StatementBalance {
        balance_type,
        amount_ct,
        indicator,
        date,
    })
}

fn parse_entry(e: &str) -> Option<Camt053Entry> {
    let amount_ct = xml_text(e, "Amt").and_then(crate::ct_from_eur_str)?;

    let indicator = xml_text(e, "CdtDbtInd")
        .and_then(|s| s.parse::<CreditDebitIndicator>().ok())
        .unwrap_or(CreditDebitIndicator::Credit);

    let status = xml_text(e, "Sts")
        .map(EntryStatus::from_code)
        .unwrap_or(EntryStatus::Booked);

    let booking_date = xml_inner(e, "BookgDt").and_then(|d| xml_text(d, "Dt").map(str::to_owned));

    let value_date = xml_inner(e, "ValDt").and_then(|d| xml_text(d, "Dt").map(str::to_owned));

    let account_servicer_ref = xml_text(e, "AcctSvcrRef").map(str::to_owned);

    let bank_tx_code = xml_inner(e, "BkTxCd").map(str::to_owned);

    // Transaction details (NtryDtls/TxDtls — take the first one)
    let tx_dtls = xml_inner(e, "NtryDtls")
        .and_then(|nd| xml_inner(nd, "TxDtls"))
        .or_else(|| xml_inner(e, "TxDtls"));

    let refs = tx_dtls.and_then(|td| xml_inner(td, "Refs"));

    let end_to_end_id = refs
        .and_then(|r| xml_text(r, "EndToEndId"))
        .map(str::to_owned);

    let mandate_id = refs.and_then(|r| xml_text(r, "MndtId")).map(str::to_owned);

    let creditor_id = refs.and_then(|r| xml_text(r, "CdtrId")).map(str::to_owned);

    let reference = tx_dtls
        .and_then(|td| xml_inner(td, "RmtInf"))
        .and_then(|ri| xml_text(ri, "Ustrd"))
        .map(str::to_owned);

    // Counterparty: for credits the payer is the debtor; for debits the payee is creditor
    let rltd_pties = tx_dtls.and_then(|td| xml_inner(td, "RltdPties"));
    let (counterparty_name, counterparty_iban) = match indicator {
        CreditDebitIndicator::Credit => {
            let name = rltd_pties
                .and_then(|r| xml_inner(r, "Dbtr"))
                .and_then(|d| xml_text(d, "Nm"))
                .map(str::to_owned);
            let iban = rltd_pties
                .and_then(|r| xml_inner(r, "DbtrAcct"))
                .and_then(|a| xml_text(a, "IBAN"))
                .map(str::to_owned);
            (name, iban)
        }
        CreditDebitIndicator::Debit => {
            let name = rltd_pties
                .and_then(|r| xml_inner(r, "Cdtr"))
                .and_then(|c| xml_text(c, "Nm"))
                .map(str::to_owned);
            let iban = rltd_pties
                .and_then(|r| xml_inner(r, "CdtrAcct"))
                .and_then(|a| xml_text(a, "IBAN"))
                .map(str::to_owned);
            (name, iban)
        }
    };

    // Return reason: RtrInf/Rsn/Cd
    let return_reason_code = tx_dtls
        .and_then(|td| xml_inner(td, "RtrInf"))
        .and_then(|ri| xml_inner(ri, "Rsn"))
        .and_then(|r| xml_text(r, "Cd"))
        .map(str::to_owned);

    Some(Camt053Entry {
        amount_ct,
        indicator,
        status,
        booking_date,
        value_date,
        account_servicer_ref,
        bank_tx_code,
        end_to_end_id,
        mandate_id,
        creditor_id,
        reference,
        counterparty_name,
        counterparty_iban,
        return_reason_code,
    })
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const CAMT053_EXAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.053.001.06">
  <BkToCstmrStmt>
    <GrpHdr>
      <MsgId>STMT-2026-07-14-001</MsgId>
      <CreDtTm>2026-07-14T23:59:00</CreDtTm>
    </GrpHdr>
    <Stmt>
      <Id>2026-07-14</Id>
      <ElctrncSeqNb>42</ElctrncSeqNb>
      <Acct><Id><IBAN>DE89370400440532013000</IBAN></Id></Acct>
      <Bal>
        <Tp><CdOrPrtry><Cd>OPBD</Cd></CdOrPrtry></Tp>
        <Amt Ccy="EUR">1000.00</Amt>
        <CdtDbtInd>CRDT</CdtDbtInd>
        <Dt><Dt>2026-07-13</Dt></Dt>
      </Bal>
      <Bal>
        <Tp><CdOrPrtry><Cd>CLBD</Cd></CdOrPrtry></Tp>
        <Amt Ccy="EUR">1155.42</Amt>
        <CdtDbtInd>CRDT</CdtDbtInd>
        <Dt><Dt>2026-07-14</Dt></Dt>
      </Bal>
      <Ntry>
        <Amt Ccy="EUR">155.42</Amt>
        <CdtDbtInd>CRDT</CdtDbtInd>
        <Sts>BOOK</Sts>
        <BookgDt><Dt>2026-07-14</Dt></BookgDt>
        <ValDt><Dt>2026-07-14</Dt></ValDt>
        <AcctSvcrRef>SVCRREF-001</AcctSvcrRef>
        <NtryDtls>
          <TxDtls>
            <Refs>
              <EndToEndId>E2E-INV-2026</EndToEndId>
            </Refs>
            <RmtInf><Ustrd>Invoice 2026-07-001</Ustrd></RmtInf>
            <RltdPties>
              <Dbtr><Nm>Max Mustermann GmbH</Nm></Dbtr>
              <DbtrAcct><Id><IBAN>NL91ABNA0417164300</IBAN></Id></DbtrAcct>
            </RltdPties>
          </TxDtls>
        </NtryDtls>
      </Ntry>
    </Stmt>
  </BkToCstmrStmt>
</Document>"#;

    #[test]
    fn parse_basic_statement() {
        let doc = parse_camt053(CAMT053_EXAMPLE).unwrap();
        assert_eq!(doc.msg_id, "STMT-2026-07-14-001");
        assert_eq!(
            doc.namespace.as_deref(),
            Some("urn:iso:std:iso:20022:tech:xsd:camt.053.001.06")
        );

        assert_eq!(doc.statements.len(), 1);
        let stmt = &doc.statements[0];
        assert_eq!(stmt.stmt_id, "2026-07-14");
        assert_eq!(stmt.sequence_number, Some(42));
        assert_eq!(stmt.account_iban, "DE89370400440532013000");
    }

    #[test]
    fn balances_parsed() {
        let doc = parse_camt053(CAMT053_EXAMPLE).unwrap();
        let stmt = &doc.statements[0];

        let opening = stmt.opening_balance().unwrap();
        assert_eq!(opening.amount_ct, 100_000);
        assert_eq!(opening.date, "2026-07-13");

        let closing = stmt.closing_balance().unwrap();
        assert_eq!(closing.amount_ct, 115_542);
        assert_eq!(closing.date, "2026-07-14");
    }

    #[test]
    fn entry_parsed() {
        let doc = parse_camt053(CAMT053_EXAMPLE).unwrap();
        let stmt = &doc.statements[0];
        assert_eq!(stmt.entries.len(), 1);

        let e = &stmt.entries[0];
        assert_eq!(e.amount_ct, 15_542);
        assert_eq!(e.indicator, CreditDebitIndicator::Credit);
        assert_eq!(e.status, EntryStatus::Booked);
        assert_eq!(e.booking_date.as_deref(), Some("2026-07-14"));
        assert_eq!(e.account_servicer_ref.as_deref(), Some("SVCRREF-001"));
        assert_eq!(e.end_to_end_id.as_deref(), Some("E2E-INV-2026"));
        assert_eq!(e.reference.as_deref(), Some("Invoice 2026-07-001"));
        assert_eq!(e.counterparty_name.as_deref(), Some("Max Mustermann GmbH"));
        assert_eq!(e.counterparty_iban.as_deref(), Some("NL91ABNA0417164300"));
        assert!(!e.is_return());
        assert_eq!(e.signed_ct(), 15_542); // credit = positive
    }

    #[test]
    fn net_movement() {
        let doc = parse_camt053(CAMT053_EXAMPLE).unwrap();
        assert_eq!(doc.statements[0].net_movement_ct(), 15_542);
    }

    #[test]
    fn not_camt053() {
        let err = parse_camt053("<Document><Other/></Document>").unwrap_err();
        assert_eq!(err, Camt053ParseError::NotCamt053);
    }

    #[test]
    fn balance_type_codes() {
        assert_eq!(BalanceType::OpeningBooked.as_code(), "OPBD");
        assert_eq!(BalanceType::ClosingBooked.as_code(), "CLBD");
        assert_eq!(BalanceType::from_code("CLBD"), BalanceType::ClosingBooked);
    }

    #[test]
    fn debit_entry_counterparty_is_creditor() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.053.001.06">
  <BkToCstmrStmt>
    <GrpHdr><MsgId>STMT-DEBIT</MsgId><CreDtTm>2026-07-14T23:59:00</CreDtTm></GrpHdr>
    <Stmt>
      <Id>2026-07-14-D</Id>
      <Acct><Id><IBAN>DE89370400440532013000</IBAN></Id></Acct>
      <Ntry>
        <Amt Ccy="EUR">500.00</Amt>
        <CdtDbtInd>DBIT</CdtDbtInd>
        <Sts>BOOK</Sts>
        <BookgDt><Dt>2026-07-14</Dt></BookgDt>
        <NtryDtls>
          <TxDtls>
            <Refs><EndToEndId>PAY-OUT-001</EndToEndId></Refs>
            <RmtInf><Ustrd>Supplier invoice July</Ustrd></RmtInf>
            <RltdPties>
              <Cdtr><Nm>Supplier AG</Nm></Cdtr>
              <CdtrAcct><Id><IBAN>NL91ABNA0417164300</IBAN></Id></CdtrAcct>
            </RltdPties>
          </TxDtls>
        </NtryDtls>
      </Ntry>
    </Stmt>
  </BkToCstmrStmt>
</Document>"#;
        let doc = parse_camt053(xml).unwrap();
        let e = &doc.statements[0].entries[0];
        assert_eq!(e.amount_ct, 50_000);
        assert_eq!(e.indicator, CreditDebitIndicator::Debit);
        assert_eq!(e.signed_ct(), -50_000); // debit = negative
        assert_eq!(e.counterparty_name.as_deref(), Some("Supplier AG"));
        assert_eq!(e.counterparty_iban.as_deref(), Some("NL91ABNA0417164300"));
        assert_eq!(e.reference.as_deref(), Some("Supplier invoice July"));
    }

    #[test]
    fn return_entry_detected() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.053.001.06">
  <BkToCstmrStmt>
    <GrpHdr><MsgId>STMT-RTN</MsgId><CreDtTm>2026-07-14T23:59:00</CreDtTm></GrpHdr>
    <Stmt>
      <Id>2026-07-14-R</Id>
      <Acct><Id><IBAN>DE89370400440532013000</IBAN></Id></Acct>
      <Ntry>
        <Amt Ccy="EUR">75.00</Amt>
        <CdtDbtInd>DBIT</CdtDbtInd>
        <Sts>BOOK</Sts>
        <NtryDtls>
          <TxDtls>
            <Refs><EndToEndId>MND-001</EndToEndId></Refs>
            <RtrInf>
              <Rsn><Cd>MD01</Cd></Rsn>
              <AddtlInf>No mandate found</AddtlInf>
            </RtrInf>
          </TxDtls>
        </NtryDtls>
      </Ntry>
    </Stmt>
  </BkToCstmrStmt>
</Document>"#;
        let doc = parse_camt053(xml).unwrap();
        let e = &doc.statements[0].entries[0];
        assert!(e.is_return());
        assert_eq!(e.return_reason_code.as_deref(), Some("MD01"));
    }

    #[test]
    fn no_from_date_fallback_to_creation_time() {
        // FrToDt absent → from_date must be None, NOT the document CreDtTm
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.053.001.06">
  <BkToCstmrStmt>
    <GrpHdr><MsgId>STMT-NO-PERIOD</MsgId><CreDtTm>2026-07-14T23:59:00</CreDtTm></GrpHdr>
    <Stmt>
      <Id>2026-07-14</Id>
      <Acct><Id><IBAN>DE89370400440532013000</IBAN></Id></Acct>
    </Stmt>
  </BkToCstmrStmt>
</Document>"#;
        let doc = parse_camt053(xml).unwrap();
        assert_eq!(
            doc.statements[0].from_date, None,
            "from_date must be None when FrToDt is absent"
        );
        assert_eq!(doc.statements[0].to_date, None);
    }
}
