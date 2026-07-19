//! ISO 20022 camt.052 — Bank-to-Customer Account Report (intraday).
//!
//! camt.052 reports movements **during** the business day. Unlike
//! [`camt.053`](crate::camt053), its entries are provisional: an entry may be
//! `PDNG` (pending) now and booked, amended or dropped by the time the
//! end-of-day statement arrives. Reconcile against camt.053, and treat camt.052
//! as an early signal only.
//!
//! Entries use the shared [`CashEntry`] model, so the same reconciliation code
//! works across camt.052, camt.053 and camt.054.
//!
//! ## Example
//!
//! ```rust
//! use sepa::camt052::parse_camt052;
//!
//! let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
//! <Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.052.001.08">
//!   <BkToCstmrAcctRpt>
//!     <GrpHdr><MsgId>RPT-001</MsgId><CreDtTm>2026-07-14T11:00:00</CreDtTm></GrpHdr>
//!     <Rpt>
//!       <Id>INTRADAY-1</Id>
//!       <Acct><Id><IBAN>DE89370400440532013000</IBAN></Id></Acct>
//!       <Ntry>
//!         <Amt Ccy="EUR">250.00</Amt>
//!         <CdtDbtInd>CRDT</CdtDbtInd>
//!         <Sts><Cd>PDNG</Cd></Sts>
//!       </Ntry>
//!     </Rpt>
//!   </BkToCstmrAcctRpt>
//! </Document>"#;
//!
//! let doc = parse_camt052(xml)?;
//! let rpt = &doc.reports[0];
//! assert_eq!(rpt.account_iban, "DE89370400440532013000");
//! assert_eq!(rpt.entries[0].status, sepa::EntryStatus::Pending);
//! # Ok::<(), sepa::Camt052ParseError>(())
//! ```

use crate::camt::{self, CashEntry, StatementBalance};
use crate::xml::{Document, Node, XmlError};

/// Known camt.052 XML namespace URIs.
pub mod ns {
    /// Original ISO version.
    pub const CAMT052_001_02: &str = "urn:iso:std:iso:20022:tech:xsd:camt.052.001.02";
    /// Common German banking version.
    pub const CAMT052_001_06: &str = "urn:iso:std:iso:20022:tech:xsd:camt.052.001.06";
    /// 2019 maintenance release.
    pub const CAMT052_001_08: &str = "urn:iso:std:iso:20022:tech:xsd:camt.052.001.08";
}

// ── Report ────────────────────────────────────────────────────────────────────

/// A single intraday account report within a camt.052 document.
#[derive(Debug, Clone)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Camt052Report {
    /// Report identifier (`Id`).
    pub report_id: String,
    /// Electronic sequence number, when the bank numbers its reports.
    pub sequence_number: Option<u64>,
    /// Account IBAN.
    pub account_iban: String,
    /// BIC of the account servicing institution (`Acct/Svcr`), if reported.
    pub account_servicer_bic: Option<String>,
    /// Reporting period start, ISO 8601.
    pub from_date: Option<String>,
    /// Reporting period end, ISO 8601.
    pub to_date: Option<String>,
    /// Balances reported so far. Often interim (`ITBD`) rather than closing.
    pub balances: Vec<StatementBalance>,
    /// Entries reported so far — **provisional**, see the module docs.
    pub entries: Vec<CashEntry>,
}

impl Camt052Report {
    /// Net movement in ct across all entries in this report.
    #[must_use]
    pub fn net_movement_ct(&self) -> i64 {
        self.entries
            .iter()
            .fold(0i64, |acc, e| acc.saturating_add(e.signed_ct()))
    }

    /// Entries that are not yet booked (`PDNG`).
    ///
    /// These are the entries most likely to change before the camt.053 arrives.
    pub fn pending_entries(&self) -> impl Iterator<Item = &CashEntry> {
        self.entries
            .iter()
            .filter(|e| e.status == crate::EntryStatus::Pending)
    }
}

// ── Document ──────────────────────────────────────────────────────────────────

/// A parsed camt.052 Bank-to-Customer Account Report document.
#[derive(Debug, Clone)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Camt052Document {
    /// Document message ID.
    pub msg_id: String,
    /// Document creation timestamp.
    pub created_at: String,
    /// Detected XML namespace URI.
    pub namespace: Option<String>,
    /// One or more reports (typically one per account).
    pub reports: Vec<Camt052Report>,
}

/// Error returned when camt.052 XML cannot be parsed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum Camt052ParseError {
    /// The input is not well-formed XML.
    #[error(transparent)]
    Xml(#[from] XmlError),

    /// Root element `BkToCstmrAcctRpt` not found — not a camt.052 document.
    #[error("not a camt.052 document: root element <BkToCstmrAcctRpt> not found")]
    NotCamt052,
}

// ── Parser ────────────────────────────────────────────────────────────────────

/// Parse a camt.052 Bank-to-Customer Account Report XML string.
///
/// Accepts every ISO version and both the default-namespace and prefixed
/// document shapes.
///
/// # Errors
///
/// Returns [`Camt052ParseError::NotCamt052`] when the root element is missing,
/// or [`Camt052ParseError::Xml`] when the input is not well-formed.
pub fn parse_camt052(xml: &str) -> Result<Camt052Document, Camt052ParseError> {
    let doc = Document::parse(xml)?;
    let root = doc
        .root
        .child("BkToCstmrAcctRpt")
        .ok_or(Camt052ParseError::NotCamt052)?;

    let grp_hdr = root.child("GrpHdr");
    let text = |tag: &str| {
        grp_hdr
            .and_then(|h| h.text_of(tag))
            .unwrap_or_default()
            .to_owned()
    };

    Ok(Camt052Document {
        msg_id: text("MsgId"),
        created_at: text("CreDtTm"),
        namespace: doc.namespace,
        reports: root.children_named("Rpt").map(parse_report).collect(),
    })
}

fn parse_report(r: &Node) -> Camt052Report {
    let (from_date, to_date) = camt::period(r);
    Camt052Report {
        report_id: r.text_of("Id").unwrap_or_default().to_owned(),
        sequence_number: r.text_of("ElctrncSeqNb").and_then(|v| v.parse().ok()),
        account_iban: camt::account_iban(r),
        account_servicer_bic: r
            .path(&["Acct", "Svcr"])
            .and_then(camt::agent_bic)
            .map(str::to_owned),
        from_date,
        to_date,
        balances: camt::balances_of(r),
        entries: camt::entries_of(r),
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EntryStatus;
    use crate::camt054::CreditDebitIndicator;

    const REPORT: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.052.001.08">
  <BkToCstmrAcctRpt>
    <GrpHdr><MsgId>RPT-2026-07-14</MsgId><CreDtTm>2026-07-14T11:00:00</CreDtTm></GrpHdr>
    <Rpt>
      <Id>INTRADAY-1</Id>
      <ElctrncSeqNb>7</ElctrncSeqNb>
      <Acct>
        <Id><IBAN>DE89370400440532013000</IBAN></Id>
        <Svcr><FinInstnId><BICFI>COBADEFFXXX</BICFI></FinInstnId></Svcr>
      </Acct>
      <FrToDt><FrDtTm>2026-07-14T00:00:00</FrDtTm><ToDtTm>2026-07-14T11:00:00</ToDtTm></FrToDt>
      <Bal>
        <Tp><CdOrPrtry><Cd>ITBD</Cd></CdOrPrtry></Tp>
        <Amt Ccy="EUR">1000.00</Amt>
        <CdtDbtInd>CRDT</CdtDbtInd>
        <Dt><Dt>2026-07-14</Dt></Dt>
      </Bal>
      <Ntry>
        <Amt Ccy="EUR">250.00</Amt>
        <CdtDbtInd>CRDT</CdtDbtInd>
        <Sts><Cd>PDNG</Cd></Sts>
        <NtryDtls><TxDtls>
          <Refs><EndToEndId>E2E-PENDING</EndToEndId></Refs>
          <RltdPties><Dbtr><Pty><Nm>Zahler GmbH</Nm></Pty></Dbtr></RltdPties>
        </TxDtls></NtryDtls>
      </Ntry>
      <Ntry>
        <Amt Ccy="EUR">75.00</Amt>
        <CdtDbtInd>DBIT</CdtDbtInd>
        <Sts><Cd>BOOK</Cd></Sts>
      </Ntry>
    </Rpt>
  </BkToCstmrAcctRpt>
</Document>"#;

    #[test]
    fn parses_report_header_and_account() {
        let doc = parse_camt052(REPORT).unwrap();
        assert_eq!(doc.msg_id, "RPT-2026-07-14");
        assert_eq!(
            doc.namespace.as_deref(),
            Some("urn:iso:std:iso:20022:tech:xsd:camt.052.001.08")
        );
        let rpt = &doc.reports[0];
        assert_eq!(rpt.report_id, "INTRADAY-1");
        assert_eq!(rpt.sequence_number, Some(7));
        assert_eq!(rpt.account_iban, "DE89370400440532013000");
        assert_eq!(rpt.account_servicer_bic.as_deref(), Some("COBADEFFXXX"));
        assert_eq!(rpt.from_date.as_deref(), Some("2026-07-14T00:00:00"));
        assert_eq!(rpt.to_date.as_deref(), Some("2026-07-14T11:00:00"));
    }

    #[test]
    fn intraday_balance_and_entries() {
        let doc = parse_camt052(REPORT).unwrap();
        let rpt = &doc.reports[0];
        assert_eq!(rpt.balances[0].amount_ct, 100_000);
        assert_eq!(rpt.balances[0].currency, "EUR");

        assert_eq!(rpt.entries.len(), 2);
        assert_eq!(rpt.entries[0].signed_ct(), 25_000);
        assert_eq!(rpt.entries[0].status, EntryStatus::Pending);
        assert_eq!(rpt.entries[0].end_to_end_id(), Some("E2E-PENDING"));
        assert_eq!(rpt.entries[0].counterparty_name(), Some("Zahler GmbH"));
        assert_eq!(rpt.entries[1].indicator, CreditDebitIndicator::Debit);
        // 250.00 credit − 75.00 debit
        assert_eq!(rpt.net_movement_ct(), 17_500);
    }

    #[test]
    fn pending_entries_are_separable_from_booked() {
        let doc = parse_camt052(REPORT).unwrap();
        let pending: Vec<_> = doc.reports[0].pending_entries().collect();
        assert_eq!(pending.len(), 1, "only the PDNG entry is provisional");
        assert_eq!(pending[0].amount_ct, 25_000);
    }

    #[test]
    fn rejects_other_message_types() {
        let stmt = r#"<Document xmlns="urn:x"><BkToCstmrStmt/></Document>"#;
        assert_eq!(
            parse_camt052(stmt).unwrap_err(),
            Camt052ParseError::NotCamt052
        );
    }

    #[test]
    fn legacy_bare_status_code_is_accepted() {
        // camt.052.001.02 writes <Sts>BOOK</Sts> rather than the Cd choice.
        let xml = REPORT
            .replace("<Sts><Cd>PDNG</Cd></Sts>", "<Sts>PDNG</Sts>")
            .replace("camt.052.001.08", "camt.052.001.02");
        let doc = parse_camt052(&xml).unwrap();
        assert_eq!(doc.reports[0].entries[0].status, EntryStatus::Pending);
    }
}
