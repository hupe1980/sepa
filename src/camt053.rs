//! ISO 20022 camt.053 — Bank-to-Customer Statement parser.
//!
//! Parses end-of-day electronic bank statements (the SEPA equivalent of MT940).
//!
//! ## The camt.05x family
//!
//! | Message | Content | Module |
//! |---|---|---|
//! | camt.052 | Intraday account report, provisional | [`camt052`](crate::camt052) |
//! | camt.053 | End-of-day statement, booked and final | this module |
//! | camt.054 | Debit/credit notification of specific events | [`camt054`](crate::camt054) |
//!
//! All three share the entry model in [`crate::camt`], so reconciliation code
//! can treat them uniformly. Every ISO version from `.001.02` to `.001.13` is
//! accepted, including the `.001.07` reshaping of `Ntry/Sts` and of the party
//! elements.
//!
//! ## Example
//!
//! ```rust
//! use sepa::camt053::parse_camt053;
//!
//! let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
//! <Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.053.001.08">
//!   <BkToCstmrStmt>
//!     <GrpHdr><MsgId>STMT-001</MsgId><CreDtTm>2026-07-14T23:59:00</CreDtTm></GrpHdr>
//!     <Stmt>
//!       <Id>2026-07-14</Id>
//!       <Acct><Id><IBAN>DE89370400440532013000</IBAN></Id></Acct>
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
//! let doc = parse_camt053(xml)?;
//! let stmt = &doc.statements[0];
//! assert_eq!(stmt.account.iban.as_deref(), Some("DE89370400440532013000"));
//! assert_eq!(stmt.closing_balance().unwrap().amount_ct, 115_542);
//! # Ok::<(), sepa::Camt053ParseError>(())
//! ```

use crate::camt::{self, AccountRef, BalanceType, CashEntry, StatementBalance};
use crate::xml::{Document, Node, XmlError};

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
    /// The account this covers — IBAN or proprietary identifier, currency and
    /// servicing institution. See [`AccountRef`].
    pub account: AccountRef,
    /// Statement period start, ISO 8601.
    pub from_date: Option<String>,
    /// Statement period end, ISO 8601.
    pub to_date: Option<String>,
    /// All balances in this statement.
    pub balances: Vec<StatementBalance>,
    /// All entries (booked and pending transactions).
    pub entries: Vec<CashEntry>,
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
        self.entries.iter().map(CashEntry::signed_ct).sum()
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
    /// The input is not well-formed XML.
    #[error(transparent)]
    Xml(#[from] XmlError),

    /// Root element `BkToCstmrStmt` not found — not a camt.053 document.
    ///
    /// Everything below the root is optional to this parser: a statement with
    /// no entries, no balances or no account identifier is unusual but not
    /// malformed, and refusing to read the rest of a real bank file over it
    /// would help nobody.
    #[error("not a camt.053 document: root element <BkToCstmrStmt> not found")]
    NotCamt053,
}

// ── Parser ────────────────────────────────────────────────────────────────────

/// Parse a camt.053 Bank-to-Customer Statement XML string.
///
/// Accepts every ISO version and both the default-namespace and prefixed
/// document shapes.
///
/// # Errors
///
/// Returns [`Camt053ParseError::NotCamt053`] when the root element is missing,
/// or [`Camt053ParseError::Xml`] when the input is not well-formed.
pub fn parse_camt053(xml: &str) -> Result<Camt053Document, Camt053ParseError> {
    let doc = Document::parse(xml)?;
    let root = doc
        .root
        .child("BkToCstmrStmt")
        .ok_or(Camt053ParseError::NotCamt053)?;

    let grp_hdr = root.child("GrpHdr");
    let text = |tag: &str| {
        grp_hdr
            .and_then(|h| h.text_of(tag))
            .unwrap_or_default()
            .to_owned()
    };

    Ok(Camt053Document {
        msg_id: text("MsgId"),
        created_at: text("CreDtTm"),
        namespace: doc.namespace,
        statements: root.children_named("Stmt").map(parse_statement).collect(),
    })
}

fn parse_statement(s: &Node) -> Camt053Statement {
    // `Id` must be a direct child: `Stmt/Acct/Id` is a different element, and a
    // descendant search would pick it up whenever the statement carries no `Id`.
    let (from_date, to_date) = camt::period(s);
    Camt053Statement {
        stmt_id: s.text_of("Id").unwrap_or_default().to_owned(),
        sequence_number: s.text_of("ElctrncSeqNb").and_then(|v| v.parse().ok()),
        account: camt::account_of(s),
        from_date,
        to_date,
        balances: camt::balances_of(s),
        entries: camt::entries_of(s),
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::IsoDate;
    use crate::camt054::CreditDebitIndicator;
    use crate::{BalanceType, EntryStatus};

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
        assert_eq!(stmt.account.iban.as_deref(), Some("DE89370400440532013000"));
    }

    #[test]
    fn balances_parsed() {
        let doc = parse_camt053(CAMT053_EXAMPLE).unwrap();
        let stmt = &doc.statements[0];

        let opening = stmt.opening_balance().unwrap();
        assert_eq!(opening.amount_ct, 100_000);
        assert_eq!(opening.date_raw, "2026-07-13");
        assert_eq!(opening.date(), Some(IsoDate::new(2026, 7, 13).unwrap()));

        let closing = stmt.closing_balance().unwrap();
        assert_eq!(closing.amount_ct, 115_542);
        assert_eq!(closing.date(), Some(IsoDate::new(2026, 7, 14).unwrap()));
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
        assert_eq!(e.booking_date(), Some(IsoDate::new(2026, 7, 14).unwrap()));
        assert_eq!(e.account_servicer_ref.as_deref(), Some("SVCRREF-001"));
        assert_eq!(e.end_to_end_id(), Some("E2E-INV-2026"));
        assert_eq!(e.reference(), Some("Invoice 2026-07-001"));
        assert_eq!(e.counterparty_name(), Some("Max Mustermann GmbH"));
        assert_eq!(e.counterparty_iban(), Some("NL91ABNA0417164300"));
        assert!(!e.is_return());
        assert_eq!(e.signed_ct(), 15_542); // credit = positive
    }

    #[test]
    fn net_movement() {
        let doc = parse_camt053(CAMT053_EXAMPLE).unwrap();
        assert_eq!(doc.statements[0].net_movement_ct(), 15_542);
    }

    #[test]
    fn a_non_iban_account_is_reported_rather_than_lost() {
        // `Acct/Id` is a choice: IBAN *or* a proprietary identifier. Collapsing
        // it to one string made a non-IBAN account read as "" — indistinguishable
        // from an account with no identifier at all.
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.053.001.08">
  <BkToCstmrStmt>
    <GrpHdr><MsgId>M</MsgId><CreDtTm>T</CreDtTm></GrpHdr>
    <Stmt>
      <Id>S1</Id>
      <Acct>
        <Id><Othr><Id>1234567890</Id></Othr></Id>
        <Ccy>CHF</Ccy>
        <Svcr><FinInstnId><BICFI>POFICHBEXXX</BICFI></FinInstnId></Svcr>
      </Acct>
    </Stmt>
  </BkToCstmrStmt>
</Document>"#;
        let acct = &parse_camt053(xml).unwrap().statements[0].account;
        assert_eq!(acct.iban, None);
        assert_eq!(acct.other_id.as_deref(), Some("1234567890"));
        assert_eq!(acct.currency.as_deref(), Some("CHF"));
        assert_eq!(acct.servicer_bic.as_deref(), Some("POFICHBEXXX"));
        assert_eq!(acct.any_id(), Some("1234567890"));

        // An IBAN account still reports the IBAN, and `any_id` prefers it.
        let acct = &parse_camt053(CAMT053_EXAMPLE).unwrap().statements[0].account;
        assert_eq!(acct.any_id(), Some("DE89370400440532013000"));

        // No Acct block at all is empty, not a panic or an invented value.
        let bare = "<Document><BkToCstmrStmt><Stmt><Id>S</Id></Stmt></BkToCstmrStmt></Document>";
        let acct = &parse_camt053(bare).unwrap().statements[0].account;
        assert_eq!(acct.any_id(), None);
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
        assert_eq!(e.counterparty_name(), Some("Supplier AG"));
        assert_eq!(e.counterparty_iban(), Some("NL91ABNA0417164300"));
        assert_eq!(e.reference(), Some("Supplier invoice July"));
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
        assert_eq!(e.return_reason_code(), Some("MD01"));
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
