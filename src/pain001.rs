//! ISO 20022 pain.001 — SEPA Credit Transfer (SCT) initiation.
//!
//! Builds SEPA Credit Transfer XML for outgoing payments:
//! supplier credits, refunds to customers, or any IBAN-to-IBAN EUR transfer.
//! Supports both the DK legacy schema and the current EPC/SCT Inst schema.
//!
//! ## Schema versions
//!
//! | Schema | Namespace | Use |
//! |---|---|---|
//! | `pain.001.003.03` | `urn:iso:std:iso:20022:tech:xsd:pain.001.003.03` | DK V2.7, German banking default |
//! | `pain.001.001.09` | `urn:iso:std:iso:20022:tech:xsd:pain.001.001.09` | EPC Rulebook 2021+, required for SCT Inst |
//!
//! ## References
//!
//! - ISO 20022 pain.001.003.03 / pain.001.001.09 schemas
//! - EPC SEPA Credit Transfer Rulebook (SCT)
//! - EPC SEPA Instant Credit Transfer Rulebook (SCT Inst)
//! - Deutsche Kreditwirtschaft DFÜ-Abkommen V2.7
//!
//! ## Example
//!
//! ```rust
//! use sepa::{validate_iban, Pain001Builder, CreditTransferEntry};
//! use sepa::pain001::LocalInstrument;
//!
//! let debtor_iban   = validate_iban("DE89370400440532013000").unwrap();
//! let creditor_iban = validate_iban("NL91ABNA0417164300").unwrap();
//!
//! // Standard SCT
//! let xml = Pain001Builder::new("Acme GmbH", &debtor_iban)
//!     .msg_id("CT-2026-07-001")
//!     .execution_date("2026-07-20")
//!     .add_entry(CreditTransferEntry::new(
//!         "Max Mustermann",
//!         creditor_iban.clone(),
//!         12000,
//!         "REFUND-2025",
//!     ).with_description("Erstattung 2025"))
//!     .build_xml();
//!
//! assert!(xml.contains("<InstdAmt Ccy=\"EUR\">120.00</InstdAmt>"));
//!
//! // SCT Instant (10-second settlement, pain.001.001.09 namespace)
//! let xml_inst = Pain001Builder::new("Acme GmbH", &debtor_iban)
//!     .msg_id("CT-INST-001")
//!     .local_instrument(LocalInstrument::Inst)
//!     .add_entry(CreditTransferEntry::new(
//!         "Max Mustermann",
//!         creditor_iban,
//!         5000,
//!         "INSTANT-001",
//!     ))
//!     .build_xml();
//!
//! assert!(xml_inst.contains("<Cd>INST</Cd>"));
//! assert!(xml_inst.contains("pain.001.001.09"));
//! ```

use crate::{Bic, Iban, ct_to_eur_str};

// ── Schema version ────────────────────────────────────────────────────────────

/// pain.001 XML schema version to emit.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CreditTransferSchema {
    /// `pain.001.003.03` — Deutsche Kreditwirtschaft DK V2.7 (2013).
    ///
    /// Default. Used by virtually all German corporate banking software (DATEV,
    /// SAP, MultiCash, etc.) and accepted by all German banks via EBICS.
    #[default]
    DkV2_7,
    /// `pain.001.001.09` — EPC SCT Rulebook 2021+ / SCT Instant.
    ///
    /// Required for SEPA Instant Credit Transfer (`LocalInstrument::Inst`).
    /// Automatically selected when [`LocalInstrument::Inst`] is set.
    IsoV9,
}

impl CreditTransferSchema {
    /// The XML namespace URI for this schema version.
    #[must_use]
    pub const fn namespace(self) -> &'static str {
        match self {
            Self::DkV2_7 => "urn:iso:std:iso:20022:tech:xsd:pain.001.003.03",
            Self::IsoV9 => "urn:iso:std:iso:20022:tech:xsd:pain.001.001.09",
        }
    }
}

// ── LocalInstrument ───────────────────────────────────────────────────────────

/// SEPA Credit Transfer local instrument variant.
///
/// When set to [`Inst`](LocalInstrument::Inst), the builder:
/// - Switches the schema to [`CreditTransferSchema::IsoV9`] automatically
/// - Adds `<LclInstrm><Cd>INST</Cd></LclInstrm>` to `PmtTpInf`
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum LocalInstrument {
    /// Standard SEPA Credit Transfer — no local instrument code (default).
    #[default]
    None,
    /// SEPA Instant Credit Transfer — 10-second settlement window.
    ///
    /// EU Regulation 2024/886 mandates PSP support for SCT Inst in the eurozone.
    Inst,
}

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

/// Builder for an ISO 20022 pain.001 (SEPA Credit Transfer) XML batch.
///
/// Defaults to `pain.001.003.03` (DK V2.7). Use [`local_instrument(LocalInstrument::Inst)`](Self::local_instrument)
/// for SCT Instant, which automatically selects `pain.001.001.09`.
#[derive(Debug)]
pub struct Pain001Builder {
    debtor_name: String,
    debtor_iban: Iban,
    debtor_bic: Option<Bic>,
    msg_id: String,
    execution_date: String,
    schema: CreditTransferSchema,
    local_instrument: LocalInstrument,
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
            schema: CreditTransferSchema::DkV2_7,
            local_instrument: LocalInstrument::None,
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

    /// Override the pain.001 XML schema version (default: [`CreditTransferSchema::DkV2_7`]).
    ///
    /// Setting [`LocalInstrument::Inst`] via [`local_instrument`](Self::local_instrument)
    /// automatically overrides this to [`CreditTransferSchema::IsoV9`].
    #[must_use]
    pub fn schema(mut self, schema: CreditTransferSchema) -> Self {
        self.schema = schema;
        self
    }

    /// Set the local instrument (default: [`LocalInstrument::None`]).
    ///
    /// Setting [`LocalInstrument::Inst`] automatically switches the schema to
    /// `pain.001.001.09` as required by the EPC SCT Inst Rulebook.
    #[must_use]
    pub fn local_instrument(mut self, li: LocalInstrument) -> Self {
        self.local_instrument = li;
        if li == LocalInstrument::Inst {
            self.schema = CreditTransferSchema::IsoV9;
        }
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

    /// Generate the pain.001 XML string.
    ///
    /// The emitted namespace URI is determined by the configured [`CreditTransferSchema`].
    /// `ChrgBr` is always `SLEV` (the only value permitted by the EPC SCT Rulebook).
    ///
    /// For streaming output use [`write_xml_to`](Self::write_xml_to).
    #[must_use]
    pub fn build_xml(&self) -> String {
        // Estimate: ~850 B for header/footer + ~420 B per transaction.
        let mut buf = String::with_capacity(850 + self.entries.len() * 420);
        self.write_xml_to(&mut buf)
            .expect("in-memory String write is infallible");
        buf
    }

    /// Write the pain.001 XML to any [`fmt::Write`](std::fmt::Write) target.
    ///
    /// ```rust
    /// use sepa::{Pain001Builder, validate_iban};
    ///
    /// let iban = validate_iban("DE89370400440532013000").unwrap();
    /// let builder = Pain001Builder::new("Test", &iban).msg_id("X");
    ///
    /// let mut buf = String::new();
    /// builder.write_xml_to(&mut buf).unwrap();
    /// assert!(buf.contains("<PmtMtd>TRF</PmtMtd>"));
    /// ```
    pub fn write_xml_to<W: std::fmt::Write>(&self, w: &mut W) -> std::fmt::Result {
        use crate::xml_util::write_escaped;

        let now = crate::pain008::iso8601_now();
        let total_eur = ct_to_eur_str(self.total_ct());
        let nb = self.entries.len();
        let debtor_bic = self.debtor_bic.as_ref().map_or("NOTPROVIDED", Bic::as_str);
        let namespace = self.schema.namespace();
        let debtor_iban = self.debtor_iban.as_str();
        let execution_date = &self.execution_date;

        // Each line written individually — no `\` continuation that silently strips indentation.
        w.write_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n")?;
        writeln!(w, "<Document xmlns=\"{namespace}\">")?;
        w.write_str("  <CstmrCdtTrfInitn>\n    <GrpHdr>\n      <MsgId>")?;
        write_escaped(w, &self.msg_id)?;
        write!(w, "</MsgId>\n      <CreDtTm>{now}</CreDtTm>\n")?;
        write!(
            w,
            "      <NbOfTxs>{nb}</NbOfTxs>\n      <CtrlSum>{total_eur}</CtrlSum>\n"
        )?;
        w.write_str("      <InitgPty><Nm>")?;
        write_escaped(w, &self.debtor_name)?;
        w.write_str("</Nm></InitgPty>\n    </GrpHdr>\n    <PmtInf>\n      <PmtInfId>")?;
        write_escaped(w, &self.msg_id)?;
        w.write_str("-1</PmtInfId>\n")?;
        w.write_str("      <PmtMtd>TRF</PmtMtd>\n")?;
        write!(
            w,
            "      <NbOfTxs>{nb}</NbOfTxs>\n      <CtrlSum>{total_eur}</CtrlSum>\n"
        )?;
        w.write_str("      <PmtTpInf>\n        <SvcLvl><Cd>SEPA</Cd></SvcLvl>\n")?;
        if self.local_instrument == LocalInstrument::Inst {
            w.write_str("        <LclInstrm><Cd>INST</Cd></LclInstrm>\n")?;
        }
        w.write_str("      </PmtTpInf>\n")?;
        writeln!(w, "      <ReqdExctnDt>{execution_date}</ReqdExctnDt>")?;
        w.write_str("      <Dbtr><Nm>")?;
        write_escaped(w, &self.debtor_name)?;
        w.write_str("</Nm></Dbtr>\n")?;
        writeln!(
            w,
            "      <DbtrAcct><Id><IBAN>{debtor_iban}</IBAN></Id></DbtrAcct>"
        )?;
        writeln!(
            w,
            "      <DbtrAgt><FinInstnId><BIC>{debtor_bic}</BIC></FinInstnId></DbtrAgt>"
        )?;
        w.write_str("      <ChrgBr>SLEV</ChrgBr>\n")?;

        for entry in &self.entries {
            Self::write_transaction(w, entry)?;
        }

        w.write_str("    </PmtInf>\n  </CstmrCdtTrfInitn>\n</Document>")
    }

    /// Write the pain.001 XML to any [`io::Write`](std::io::Write) target.
    ///
    /// Streams directly to a `BufWriter<File>`, `TcpStream`, or `Vec<u8>`.
    ///
    /// ```rust
    /// use sepa::{Pain001Builder, validate_iban};
    ///
    /// let iban = validate_iban("DE89370400440532013000").unwrap();
    /// let mut buf: Vec<u8> = Vec::new();
    /// Pain001Builder::new("Test", &iban).write_xml_to_io(&mut buf).unwrap();
    /// assert!(buf.starts_with(b"<?xml"));
    /// ```
    pub fn write_xml_to_io<W: std::io::Write>(&self, w: &mut W) -> std::io::Result<()> {
        let mut bridge = crate::xml_util::IoWriterBridge {
            inner: w,
            error: None,
        };
        if self.write_xml_to(&mut bridge).is_err() {
            if let Some(e) = bridge.error {
                return Err(e);
            }
        }
        Ok(())
    }

    fn write_transaction<W: std::fmt::Write>(
        w: &mut W,
        e: &CreditTransferEntry,
    ) -> std::fmt::Result {
        use crate::xml_util::{write_escaped, write_eur};
        let creditor_bic = e.creditor_bic.as_ref().map_or("NOTPROVIDED", Bic::as_str);

        w.write_str("    <CdtTrfTxInf>\n      <PmtId>\n        <EndToEndId>")?;
        write_escaped(w, &e.end_to_end_id)?;
        w.write_str("</EndToEndId>\n      </PmtId>\n      <Amt><InstdAmt Ccy=\"EUR\">")?;
        write_eur(w, e.amount_ct)?;
        w.write_str("</InstdAmt></Amt>\n      <CdtrAgt><FinInstnId><BIC>")?;
        w.write_str(creditor_bic)?;
        w.write_str("</BIC></FinInstnId></CdtrAgt>\n      <Cdtr><Nm>")?;
        write_escaped(w, &e.creditor_name)?;
        w.write_str("</Nm></Cdtr>\n      <CdtrAcct><Id><IBAN>")?;
        w.write_str(e.creditor_iban.as_str())?;
        w.write_str("</IBAN></Id></CdtrAcct>\n")?;

        if let Some(desc) = &e.description {
            let truncated = if desc.len() > 140 { &desc[..140] } else { desc };
            w.write_str("      <RmtInf><Ustrd>")?;
            write_escaped(w, truncated)?;
            w.write_str("</Ustrd></RmtInf>\n")?;
        }

        w.write_str("    </CdtTrfTxInf>\n")
    }
}

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
        // EPC SCT Rulebook mandatory: charge bearer must be SLEV
        assert!(xml.contains("<ChrgBr>SLEV</ChrgBr>"));
        // No LclInstrm for standard SCT
        assert!(!xml.contains("<LclInstrm>"));
    }

    #[test]
    fn sct_inst_uses_pain001_001_09() {
        use crate::pain001::LocalInstrument;
        let xml = Pain001Builder::new("Acme GmbH", &de_iban())
            .msg_id("CT-INST-001")
            .local_instrument(LocalInstrument::Inst)
            .add_entry(entry(5_000))
            .build_xml();

        // Auto-selects IsoV9 namespace
        assert!(xml.contains("urn:iso:std:iso:20022:tech:xsd:pain.001.001.09"));
        assert!(!xml.contains("pain.001.003.03"));
        // SCT Inst local instrument code
        assert!(xml.contains("<LclInstrm><Cd>INST</Cd></LclInstrm>"));
        assert!(xml.contains("<ChrgBr>SLEV</ChrgBr>"));
    }

    #[test]
    fn explicit_iso_v9_schema() {
        use crate::pain001::CreditTransferSchema;
        let xml = Pain001Builder::new("Test", &de_iban())
            .schema(CreditTransferSchema::IsoV9)
            .build_xml();
        assert!(xml.contains("pain.001.001.09"));
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
