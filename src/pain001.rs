//! ISO 20022 pain.001.003.03 — SEPA Credit Transfer (SCT) initiation.
//!
//! Builds SEPA Credit Transfer XML for outgoing payments:
//! supplier credits, refunds to customers, or any IBAN-to-IBAN EUR transfer.
//!
//! ## References
//!
//! - ISO 20022 pain.001.003.03 schema
//! - EPC SEPA Credit Transfer Rulebook (latest version)
//!
//! ## Example
//!
//! ```rust
//! use sepa::{validate_iban, Pain001Builder, CreditTransferEntry};
//!
//! let debtor_iban   = validate_iban("DE89370400440532013000").unwrap();
//! let creditor_iban = validate_iban("NL91ABNA0417164300").unwrap();
//!
//! let xml = Pain001Builder::new("Acme GmbH", &debtor_iban)
//!     .msg_id("CT-2026-07-001")
//!     .execution_date("2026-07-20")
//!     .add_entry(CreditTransferEntry::new(
//!         "Max Mustermann",
//!         creditor_iban,
//!         12000,
//!         "REFUND-2025",
//!     ).with_description("Erstattung 2025"))
//!     .build_xml();
//!
//! assert!(xml.contains("<InstdAmt Ccy=\"EUR\">120.00</InstdAmt>"));
//! assert!(xml.contains("<Nm>Max Mustermann</Nm>"));
//! ```

use crate::pain008::xml_escape;
use crate::{Bic, Iban, ct_to_eur_str};

// ── CreditTransferEntry ───────────────────────────────────────────────────────

/// A single credit transfer in a pain.001 batch.
///
/// Construct with [`CreditTransferEntry::new`] and chain optional fields.
#[derive(Debug, Clone)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CreditTransferEntry {
    /// Beneficiary name (`Cdtr/Nm`).
    pub creditor_name: String,
    /// Beneficiary IBAN (validated).
    pub creditor_iban: Iban,
    /// Beneficiary BIC. Uses `NOTPROVIDED` in XML when `None`.
    pub creditor_bic: Option<Bic>,
    /// Payment amount in **ct** (1/100 EUR). Must be positive.
    pub amount_ct: i64,
    /// Unique end-to-end reference (`EndToEndId`) visible on beneficiary's statement.
    pub end_to_end_id: String,
    /// Remittance information (Verwendungszweck), max 140 chars.
    pub description: Option<String>,
}

impl CreditTransferEntry {
    /// Create a new credit transfer entry with required fields.
    ///
    /// Chain [`with_bic`](Self::with_bic) and [`with_description`](Self::with_description).
    pub fn new(
        creditor_name: impl Into<String>,
        creditor_iban: Iban,
        amount_ct: i64,
        end_to_end_id: impl Into<String>,
    ) -> Self {
        Self {
            creditor_name: creditor_name.into(),
            creditor_iban,
            amount_ct,
            end_to_end_id: end_to_end_id.into(),
            creditor_bic: None,
            description: None,
        }
    }

    /// Set the beneficiary's BIC (optional).
    #[must_use]
    pub fn with_bic(mut self, bic: Bic) -> Self {
        self.creditor_bic = Some(bic);
        self
    }

    /// Set the remittance information (Verwendungszweck), max 140 chars.
    #[must_use]
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }
}

// ── Builder ───────────────────────────────────────────────────────────────────

/// Builder for an ISO 20022 pain.001.003.03 (SEPA Credit Transfer) XML batch.
#[derive(Debug)]
pub struct Pain001Builder {
    debtor_name: String,
    debtor_iban: Iban,
    debtor_bic: Option<Bic>,
    msg_id: String,
    execution_date: String,
    entries: Vec<CreditTransferEntry>,
}

impl Pain001Builder {
    /// Create a new builder with the debtor's (payer's) name and IBAN.
    pub fn new(debtor_name: impl Into<String>, debtor_iban: &Iban) -> Self {
        Self {
            debtor_name: debtor_name.into(),
            debtor_iban: debtor_iban.clone(),
            debtor_bic: None,
            msg_id: format!("sct-{}", crate::pain008::epoch_secs()),
            execution_date: crate::pain008::default_collection_date(),
            entries: Vec::new(),
        }
    }

    /// Override the `MsgId` (max 35 chars).
    #[must_use]
    pub fn msg_id(mut self, id: impl Into<String>) -> Self {
        self.msg_id = id.into();
        self
    }

    /// Set the requested execution date (`ReqdExctnDt`), ISO 8601 `"YYYY-MM-DD"`.
    #[must_use]
    pub fn execution_date(mut self, date: impl Into<String>) -> Self {
        self.execution_date = date.into();
        self
    }

    /// Set the debtor's BIC.
    #[must_use]
    pub fn debtor_bic(mut self, bic: Bic) -> Self {
        self.debtor_bic = Some(bic);
        self
    }

    /// Add a credit transfer entry.
    #[must_use]
    pub fn add_entry(mut self, entry: CreditTransferEntry) -> Self {
        self.entries.push(entry);
        self
    }

    /// Add multiple entries.
    #[must_use]
    pub fn add_entries(mut self, entries: impl IntoIterator<Item = CreditTransferEntry>) -> Self {
        self.entries.extend(entries);
        self
    }

    /// Number of entries in this batch.
    #[must_use]
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// Total amount in ct across all entries.
    #[must_use]
    pub fn total_ct(&self) -> i64 {
        self.entries.iter().map(|e| e.amount_ct).sum()
    }

    /// Generate the pain.001.003.03 XML string.
    #[must_use]
    pub fn build_xml(&self) -> String {
        let now = crate::pain008::iso8601_now();
        let total_eur = ct_to_eur_str(self.total_ct());
        let nb = self.entries.len();
        let debtor_bic = self.debtor_bic.as_ref().map_or("NOTPROVIDED", Bic::as_str);

        let transactions: String = self
            .entries
            .iter()
            .map(Self::render_transaction)
            .collect::<Vec<_>>()
            .join("\n");

        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<Document xmlns=\"urn:iso:std:iso:20022:tech:xsd:pain.001.003.03\">\n\
  <CstmrCdtTrfInitn>\n\
    <GrpHdr>\n\
      <MsgId>{msg_id}</MsgId>\n\
      <CreDtTm>{now}</CreDtTm>\n\
      <NbOfTxs>{nb}</NbOfTxs>\n\
      <CtrlSum>{total_eur}</CtrlSum>\n\
      <InitgPty><Nm>{debtor_name}</Nm></InitgPty>\n\
    </GrpHdr>\n\
    <PmtInf>\n\
      <PmtInfId>{msg_id}-1</PmtInfId>\n\
      <PmtMtd>TRF</PmtMtd>\n\
      <NbOfTxs>{nb}</NbOfTxs>\n\
      <CtrlSum>{total_eur}</CtrlSum>\n\
      <PmtTpInf>\n\
        <SvcLvl><Cd>SEPA</Cd></SvcLvl>\n\
      </PmtTpInf>\n\
      <ReqdExctnDt>{execution_date}</ReqdExctnDt>\n\
      <Dbtr><Nm>{debtor_name}</Nm></Dbtr>\n\
      <DbtrAcct><Id><IBAN>{debtor_iban}</IBAN></Id></DbtrAcct>\n\
      <DbtrAgt><FinInstnId><BIC>{debtor_bic}</BIC></FinInstnId></DbtrAgt>\n\
{transactions}\n\
    </PmtInf>\n\
  </CstmrCdtTrfInitn>\n\
</Document>",
            msg_id = xml_escape(&self.msg_id),
            now = now,
            nb = nb,
            total_eur = total_eur,
            debtor_name = xml_escape(&self.debtor_name),
            execution_date = self.execution_date,
            debtor_iban = self.debtor_iban.as_str(),
            debtor_bic = debtor_bic,
            transactions = transactions,
        )
    }

    fn render_transaction(e: &CreditTransferEntry) -> String {
        let amount_eur = ct_to_eur_str(e.amount_ct);
        let creditor_bic = e.creditor_bic.as_ref().map_or("NOTPROVIDED", Bic::as_str);
        let remittance = e
            .description
            .as_deref()
            .map(|d| {
                let truncated = if d.len() > 140 { &d[..140] } else { d };
                format!(
                    "\n      <RmtInf><Ustrd>{}</Ustrd></RmtInf>",
                    xml_escape(truncated)
                )
            })
            .unwrap_or_default();

        format!(
            "    <CdtTrfTxInf>\n\
      <PmtId>\n\
        <EndToEndId>{end_to_end_id}</EndToEndId>\n\
      </PmtId>\n\
      <Amt><InstdAmt Ccy=\"EUR\">{amount_eur}</InstdAmt></Amt>\n\
      <CdtrAgt><FinInstnId><BIC>{creditor_bic}</BIC></FinInstnId></CdtrAgt>\n\
      <Cdtr><Nm>{creditor_name}</Nm></Cdtr>\n\
      <CdtrAcct><Id><IBAN>{creditor_iban}</IBAN></Id></CdtrAcct>{remittance}\n\
    </CdtTrfTxInf>",
            end_to_end_id = xml_escape(&e.end_to_end_id),
            amount_eur = amount_eur,
            creditor_bic = creditor_bic,
            creditor_name = xml_escape(&e.creditor_name),
            creditor_iban = e.creditor_iban.as_str(),
            remittance = remittance,
        )
    }
}

// xml_escape is pub(crate) in pain008 — imported above

#[cfg(test)]
mod tests {
    use super::*;
    use crate::iban::validate_iban;

    fn de_iban() -> Iban {
        validate_iban("DE89370400440532013000").unwrap()
    }
    fn nl_iban() -> Iban {
        validate_iban("NL91ABNA0417164300").unwrap()
    }

    fn entry(amount_ct: i64) -> CreditTransferEntry {
        CreditTransferEntry::new("Max Mustermann", nl_iban(), amount_ct, "E2E-001")
    }

    #[test]
    fn basic_xml_structure() {
        let xml = Pain001Builder::new("Acme GmbH", &de_iban())
            .msg_id("CT-001")
            .execution_date("2026-07-20")
            .add_entry(
                CreditTransferEntry::new("Max Mustermann", nl_iban(), 12_000, "REFUND-2025")
                    .with_bic("ABNANL2A".parse::<Bic>().unwrap())
                    .with_description("Erstattung 2025"),
            )
            .build_xml();

        assert!(xml.contains("urn:iso:std:iso:20022:tech:xsd:pain.001.003.03"));
        assert!(xml.contains("<MsgId>CT-001</MsgId>"));
        assert!(xml.contains("<NbOfTxs>1</NbOfTxs>"));
        assert!(xml.contains("<InstdAmt Ccy=\"EUR\">120.00</InstdAmt>"));
        assert!(xml.contains("<Nm>Max Mustermann</Nm>"));
        assert!(xml.contains("<IBAN>NL91ABNA0417164300</IBAN>"));
        assert!(xml.contains("<ReqdExctnDt>2026-07-20</ReqdExctnDt>"));
        assert!(xml.contains("Erstattung 2025"));
        assert!(xml.contains("<PmtMtd>TRF</PmtMtd>"));
    }

    #[test]
    fn ctrl_sum_correct() {
        let xml = Pain001Builder::new("Test", &de_iban())
            .msg_id("CT-002")
            .execution_date("2026-07-20")
            .add_entry(entry(5_000))
            .add_entry(entry(7_500))
            .build_xml();
        assert!(xml.contains("<CtrlSum>125.00</CtrlSum>"));
        assert!(xml.contains("<NbOfTxs>2</NbOfTxs>"));
    }

    #[test]
    fn no_f64_rounding() {
        let builder = Pain001Builder::new("Test", &de_iban())
            .add_entry(entry(10))
            .add_entry(entry(20));
        assert_eq!(builder.total_ct(), 30);
        assert!(builder.build_xml().contains("<CtrlSum>0.30</CtrlSum>"));
    }

    #[test]
    fn xml_escaping_in_names() {
        let xml = Pain001Builder::new("Test & Co. <GmbH>", &de_iban())
            .msg_id("CT-ESC")
            .build_xml();
        assert!(xml.contains("Test &amp; Co. &lt;GmbH&gt;"));
    }

    #[test]
    fn empty_batch() {
        let xml = Pain001Builder::new("Test", &de_iban())
            .msg_id("EMPTY")
            .build_xml();
        assert!(xml.contains("<NbOfTxs>0</NbOfTxs>"));
        assert!(xml.contains("<CtrlSum>0.00</CtrlSum>"));
    }
}
