//! ISO 20022 pain.008.003.02 — SEPA Direct Debit initiation.
//!
//! Builds standards-compliant pain.008 XML for SEPA Direct Debit (SDD).
//! Supports both CORE (consumer) and B2B (business) scheme variants.
//! All monetary amounts use integer cents (1 ct = 0.01 EUR) — no f64.
//!
//! ## Scheme variants
//!
//! | Scheme | Code | Use |
//! |---|---|---|
//! | [`DirectDebitScheme::Core`] | `CORE` | Consumer accounts, default |
//! | [`DirectDebitScheme::B2b`] | `B2B` | Business accounts only, shorter cycles |
//!
//! ## Sequence type and batch homogeneity
//!
//! ISO 20022 pain.008 places `SeqTp` at the `PmtInf` (payment information) level,
//! not per-transaction.  All entries in one [`Pain008Builder`] batch must share the
//! same sequence type.  Set it with [`Pain008Builder::sequence_type`] (default: `Rcur`).
//! For mixed lifecycle collections, create separate builders per sequence type.
//!
//! ## References
//!
//! - ISO 20022 pain.008.003.02 schema
//! - EPC SEPA Core Direct Debit Rulebook
//! - EPC SEPA Business-to-Business Direct Debit Rulebook
//! - Deutsche Bundesbank pain.008 implementation guide (DFÜ-Abkommen V2.7)
//!
//! ## Example
//!
//! ```rust
//! use sepa::{validate_iban, Pain008Builder, DirectDebitEntry};
//! use sepa::pain008::{SequenceType, DirectDebitScheme};
//!
//! let creditor_iban = validate_iban("DE89370400440532013000").unwrap();
//! let debtor_iban   = validate_iban("NL91ABNA0417164300").unwrap();
//!
//! // SEPA Core Direct Debit (default)
//! let xml = Pain008Builder::new("Creditor GmbH", &creditor_iban)
//!     .msg_id("BATCH-2026-07-001")
//!     .sequence_type(SequenceType::Rcur)
//!     .collection_date("2026-07-20")
//!     .creditor_bic("COBADEFFXXX".parse().unwrap())
//!     .add_entry(DirectDebitEntry::new(
//!         "MND-00042",
//!         "2024-06-01",
//!         "Max Mustermann",
//!         debtor_iban,
//!         7500,
//!         "R2026-06-001",
//!     ).with_description("Abschlag Juli 2026"))
//!     .build_xml();
//!
//! assert!(xml.contains("<Cd>CORE</Cd>"));
//! assert!(xml.contains("<ChrgBr>SLEV</ChrgBr>"));
//!
//! // SEPA B2B Direct Debit
//! let debtor_b2b = validate_iban("DE29100500005001065004").unwrap();
//! let xml_b2b = Pain008Builder::new("Creditor GmbH", &creditor_iban)
//!     .scheme(DirectDebitScheme::B2b)
//!     .add_entry(DirectDebitEntry::new(
//!         "MND-B2B-001", "2024-01-01", "Corporate AG", debtor_b2b, 50000, "INV-001",
//!     ))
//!     .build_xml();
//! assert!(xml_b2b.contains("<Cd>B2B</Cd>"));
//! ```

use std::str::FromStr;

use crate::creditor_id::CreditorId;
use crate::{Bic, Iban, ct_to_eur_str};

// ── DirectDebitScheme ─────────────────────────────────────────────────────────

/// SEPA Direct Debit scheme variant.
///
/// Determines the `<LclInstrm><Cd>…</Cd></LclInstrm>` value in the XML.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum DirectDebitScheme {
    /// SEPA Core Direct Debit — consumer accounts (default).
    ///
    /// Pre-notification deadlines: 5 banking days for FRST/OOFF, 2 days for RCUR/FNAL.
    #[default]
    Core,
    /// SEPA Business-to-Business Direct Debit — business accounts only.
    ///
    /// Shorter settlement: 1 banking day. Mandate must be confirmed with debtor's bank.
    B2b,
}

impl DirectDebitScheme {
    /// ISO 20022 local instrument code (`"CORE"` or `"B2B"`).
    #[inline]
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::Core => "CORE",
            Self::B2b => "B2B",
        }
    }
}

impl std::fmt::Display for DirectDebitScheme {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_code())
    }
}

// ── Errors ────────────────────────────────────────────────────────────────────

/// Error returned when parsing a [`SequenceType`] from a string.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown sequence type {0:?}: expected FRST, RCUR, FNAL, or OOFF")]
pub struct UnknownSequenceType(
    /// The unrecognised code.
    pub String,
);

// ── SequenceType ──────────────────────────────────────────────────────────────

/// SEPA direct debit sequence type (ISO 20022 `SeqTp`).
///
/// Applied at the **batch level** (`PmtInf/PmtTpInf/SeqTp`), not per-transaction.
/// All entries in a [`Pain008Builder`] share the same sequence type.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "SCREAMING_SNAKE_CASE"))]
pub enum SequenceType {
    /// First collection — mandate just activated.
    Frst,
    /// Recurring collection — all subsequent debits.
    #[default]
    Rcur,
    /// Final collection — mandate revoked after this collection.
    Fnal,
    /// One-off — mandate used only once (no `Frst`/`Rcur` lifecycle).
    Ooff,
}

impl SequenceType {
    /// ISO 20022 XML code string (`"FRST"`, `"RCUR"`, `"FNAL"`, `"OOFF"`).
    #[inline]
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::Frst => "FRST",
            Self::Rcur => "RCUR",
            Self::Fnal => "FNAL",
            Self::Ooff => "OOFF",
        }
    }
}

impl std::fmt::Display for SequenceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_code())
    }
}

impl FromStr for SequenceType {
    type Err = UnknownSequenceType;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_uppercase().as_str() {
            "FRST" => Ok(Self::Frst),
            "RCUR" => Ok(Self::Rcur),
            "FNAL" => Ok(Self::Fnal),
            "OOFF" => Ok(Self::Ooff),
            _ => Err(UnknownSequenceType(s.to_owned())),
        }
    }
}

impl TryFrom<&str> for SequenceType {
    type Error = UnknownSequenceType;
    fn try_from(s: &str) -> Result<Self, Self::Error> {
        s.parse()
    }
}

// ── DirectDebitEntry ──────────────────────────────────────────────────────────

/// A single direct debit transaction in a pain.008 batch.
///
/// Construct with [`DirectDebitEntry::new`] and chain optional fields.
#[derive(Debug, Clone)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DirectDebitEntry {
    /// SEPA mandate reference (`MndtId`) — creditor-assigned unique ID.
    pub mandate_ref: String,
    /// Date the mandate was signed (ISO 8601: `"YYYY-MM-DD"`).
    pub mandate_signed_at: String,
    /// Debtor's full name (`Dbtr/Nm`).
    pub debtor_name: String,
    /// Debtor's IBAN (validated).
    pub debtor_iban: Iban,
    /// Debtor's BIC. Uses `NOTPROVIDED` in XML when `None` (EPC allowance).
    pub debtor_bic: Option<Bic>,
    /// Collection amount in **ct** (1/100 EUR). Must be positive.
    pub amount_ct: i64,
    /// Unique end-to-end reference (`EndToEndId`) visible on debtor's bank statement.
    pub end_to_end_id: String,
    /// Remittance information (`Ustrd`), max 140 chars.
    pub description: Option<String>,
}

impl DirectDebitEntry {
    /// Create a new direct debit entry with required fields.
    ///
    /// Optional fields default to `None`.  Chain [`with_bic`](Self::with_bic) and
    /// [`with_description`](Self::with_description) to set them.
    pub fn new(
        mandate_ref: impl Into<String>,
        mandate_signed_at: impl Into<String>,
        debtor_name: impl Into<String>,
        debtor_iban: Iban,
        amount_ct: i64,
        end_to_end_id: impl Into<String>,
    ) -> Self {
        Self {
            mandate_ref: mandate_ref.into(),
            mandate_signed_at: mandate_signed_at.into(),
            debtor_name: debtor_name.into(),
            debtor_iban,
            amount_ct,
            end_to_end_id: end_to_end_id.into(),
            debtor_bic: None,
            description: None,
        }
    }

    /// Set the debtor's BIC (optional — use when known for faster processing).
    #[must_use]
    pub fn with_bic(mut self, bic: Bic) -> Self {
        self.debtor_bic = Some(bic);
        self
    }

    /// Set remittance information shown on the debtor's bank statement (max 140 chars).
    #[must_use]
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }
}

// ── Builder ───────────────────────────────────────────────────────────────────

/// Builder for an ISO 20022 pain.008.003.02 (SEPA Direct Debit) XML batch.
///
/// All entries share the same sequence type and scheme.
/// For mixed lifecycle collections or mixed schemes, use multiple builders.
#[derive(Debug)]
pub struct Pain008Builder {
    creditor_name: String,
    creditor_iban: Iban,
    creditor_bic: Option<Bic>,
    /// SEPA Creditor Identifier (EPC AT-02) — required by the EPC SEPA Core DD Rulebook.
    /// Rendered as `PmtInf/CdtrSchmeId` in the generated XML.
    creditor_id: Option<CreditorId>,
    msg_id: String,
    collection_date: String,
    sequence_type: SequenceType,
    scheme: DirectDebitScheme,
    entries: Vec<DirectDebitEntry>,
}

impl Pain008Builder {
    /// Create a new builder.
    ///
    /// Defaults: `scheme = Core`, `sequence_type = Rcur`, `msg_id = "sepa-<timestamp>"`,
    /// `collection_date = today + 5 calendar days`.
    pub fn new(creditor_name: impl Into<String>, creditor_iban: &Iban) -> Self {
        Self {
            creditor_name: creditor_name.into(),
            creditor_iban: creditor_iban.clone(),
            creditor_bic: None,
            creditor_id: None,
            msg_id: format!("sepa-{}", epoch_secs()),
            collection_date: default_collection_date(),
            sequence_type: SequenceType::Rcur,
            scheme: DirectDebitScheme::Core,
            entries: Vec::new(),
        }
    }

    /// Override the `MsgId` (message identifier, max 35 chars).
    #[must_use]
    pub fn msg_id(mut self, id: impl Into<String>) -> Self {
        self.msg_id = id.into();
        self
    }

    /// Set the SEPA Direct Debit scheme (default: [`DirectDebitScheme::Core`]).
    ///
    /// Use [`DirectDebitScheme::B2b`] for business-to-business direct debits.
    #[must_use]
    pub fn scheme(mut self, scheme: DirectDebitScheme) -> Self {
        self.scheme = scheme;
        self
    }

    /// Set the sequence type for all entries in this batch (default: `Rcur`).
    ///
    /// Applied at the `PmtInf/PmtTpInf/SeqTp` level — one type per batch.
    #[must_use]
    pub fn sequence_type(mut self, st: SequenceType) -> Self {
        self.sequence_type = st;
        self
    }

    /// Set the requested collection date (`ReqdColltnDt`), ISO 8601 `"YYYY-MM-DD"`.
    ///
    /// Must be ≥2 banking days in the future per EPC rules (not validated here).
    #[must_use]
    pub fn collection_date(mut self, date: impl Into<String>) -> Self {
        self.collection_date = date.into();
        self
    }

    /// Set the creditor's BIC (`CdtrAgt`).
    #[must_use]
    pub fn creditor_bic(mut self, bic: Bic) -> Self {
        self.creditor_bic = Some(bic);
        self
    }

    /// Set the SEPA Creditor Identifier (`CdtrSchmeId`, EPC AT-02).
    ///
    /// Required by the EPC SEPA Core Direct Debit Rulebook for all SDD batches.
    /// Obtain your CI from your bank or national SEPA authority.
    /// German format example: `DE74ZZZ09999999999`.
    #[must_use]
    pub fn creditor_id(mut self, id: CreditorId) -> Self {
        self.creditor_id = Some(id);
        self
    }

    /// Add a direct debit entry to the batch.
    #[must_use]
    pub fn add_entry(mut self, entry: DirectDebitEntry) -> Self {
        self.entries.push(entry);
        self
    }

    /// Add multiple entries.
    #[must_use]
    pub fn add_entries(mut self, entries: impl IntoIterator<Item = DirectDebitEntry>) -> Self {
        self.entries.extend(entries);
        self
    }

    /// Number of entries currently in the batch.
    #[must_use]
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// Total amount in ct across all entries.
    #[must_use]
    pub fn total_ct(&self) -> i64 {
        self.entries.iter().map(|e| e.amount_ct).sum()
    }

    /// Generate the pain.008.003.02 XML string.
    ///
    /// For streaming output (to a file, socket, or `Vec<u8>`), use
    /// [`write_xml_to`](Self::write_xml_to) directly to avoid the final
    /// `String` allocation.
    #[must_use]
    pub fn build_xml(&self) -> String {
        // Estimate: ~900 B for header/footer + ~480 B per transaction.
        let mut buf = String::with_capacity(900 + self.entries.len() * 480);
        // String::write_fmt is infallible — it can only fail on OOM, which panics.
        self.write_xml_to(&mut buf)
            .expect("in-memory String write is infallible");
        buf
    }

    /// Write the pain.008.003.02 XML to any [`fmt::Write`](std::fmt::Write) target.
    ///
    /// Use this to stream directly into a `BufWriter<File>`, a `Vec<u8>`, or
    /// any other writer without allocating a final `String`.
    ///
    /// ```rust
    /// use sepa::{Pain008Builder, validate_iban};
    ///
    /// let iban = validate_iban("DE89370400440532013000").unwrap();
    /// let builder = Pain008Builder::new("Test", &iban).msg_id("X");
    ///
    /// let mut buf = String::new();
    /// builder.write_xml_to(&mut buf).unwrap();
    /// assert!(buf.contains("<PmtMtd>DD</PmtMtd>"));
    /// ```
    pub fn write_xml_to<W: std::fmt::Write>(&self, w: &mut W) -> std::fmt::Result {
        use crate::xml_util::write_escaped;

        let now = iso8601_now();
        let total_eur = ct_to_eur_str(self.total_ct());
        let nb = self.entries.len();
        let creditor_bic = self
            .creditor_bic
            .as_ref()
            .map_or("NOTPROVIDED", Bic::as_str);
        let scheme_code = self.scheme.as_code();
        let seq_tp = self.sequence_type.as_code();
        let creditor_iban = self.creditor_iban.as_str();
        let collection_date = &self.collection_date;

        // Each line written individually — no `\` continuation that silently strips indentation.
        w.write_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n")?;
        writeln!(
            w,
            "<Document xmlns=\"urn:iso:std:iso:20022:tech:xsd:pain.008.003.02\">"
        )?;
        w.write_str("  <CstmrDrctDbtInitn>\n    <GrpHdr>\n      <MsgId>")?;
        write_escaped(w, &self.msg_id)?;
        write!(w, "</MsgId>\n      <CreDtTm>{now}</CreDtTm>\n")?;
        write!(
            w,
            "      <NbOfTxs>{nb}</NbOfTxs>\n      <CtrlSum>{total_eur}</CtrlSum>\n"
        )?;
        w.write_str("      <InitgPty><Nm>")?;
        write_escaped(w, &self.creditor_name)?;
        w.write_str("</Nm></InitgPty>\n    </GrpHdr>\n    <PmtInf>\n      <PmtInfId>")?;
        write_escaped(w, &self.msg_id)?;
        w.write_str("-1</PmtInfId>\n")?;
        w.write_str("      <PmtMtd>DD</PmtMtd>\n")?;
        write!(
            w,
            "      <NbOfTxs>{nb}</NbOfTxs>\n      <CtrlSum>{total_eur}</CtrlSum>\n"
        )?;
        w.write_str("      <PmtTpInf>\n        <SvcLvl><Cd>SEPA</Cd></SvcLvl>\n")?;
        writeln!(w, "        <LclInstrm><Cd>{scheme_code}</Cd></LclInstrm>")?;
        write!(w, "        <SeqTp>{seq_tp}</SeqTp>\n      </PmtTpInf>\n")?;
        writeln!(w, "      <ReqdColltnDt>{collection_date}</ReqdColltnDt>")?;
        w.write_str("      <Cdtr><Nm>")?;
        write_escaped(w, &self.creditor_name)?;
        w.write_str("</Nm></Cdtr>\n")?;
        writeln!(
            w,
            "      <CdtrAcct><Id><IBAN>{creditor_iban}</IBAN></Id></CdtrAcct>"
        )?;
        writeln!(
            w,
            "      <CdtrAgt><FinInstnId><BIC>{creditor_bic}</BIC></FinInstnId></CdtrAgt>"
        )?;
        w.write_str("      <ChrgBr>SLEV</ChrgBr>\n")?;

        if let Some(ci) = &self.creditor_id {
            w.write_str("      <CdtrSchmeId><Id><PrvtId><Othr><Id>")?;
            w.write_str(ci.as_str())?;
            w.write_str(
                "</Id><SchmeNm><Prtry>SEPA</Prtry></SchmeNm></Othr></PrvtId></Id></CdtrSchmeId>\n",
            )?;
        }

        for entry in &self.entries {
            Self::write_transaction(w, entry)?;
        }

        w.write_str("    </PmtInf>\n  </CstmrDrctDbtInitn>\n</Document>")
    }

    /// Write the pain.008.003.02 XML to any [`io::Write`](std::io::Write) target.
    ///
    /// Streams directly to a `BufWriter<File>`, `TcpStream`, or `Vec<u8>` without
    /// building an intermediate `String` — ideal for large batches or EBICS uploads.
    ///
    /// ```rust
    /// use sepa::{Pain008Builder, validate_iban};
    ///
    /// let iban = validate_iban("DE89370400440532013000").unwrap();
    /// let builder = Pain008Builder::new("Test", &iban).msg_id("X");
    ///
    /// let mut buf: Vec<u8> = Vec::new();
    /// builder.write_xml_to_io(&mut buf).unwrap();
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

    /// Zero-allocation transaction writer.
    ///
    /// All dynamic fields are written directly into `w` via [`write_escaped`] and
    /// [`write_eur`] — no intermediate `String` allocations.
    fn write_transaction<W: std::fmt::Write>(w: &mut W, e: &DirectDebitEntry) -> std::fmt::Result {
        use crate::xml_util::{write_escaped, write_eur};

        let debtor_bic = e.debtor_bic.as_ref().map_or("NOTPROVIDED", Bic::as_str);

        w.write_str("    <DrctDbtTxInf>\n      <PmtId>\n        <EndToEndId>")?;
        write_escaped(w, &e.end_to_end_id)?;
        w.write_str("</EndToEndId>\n      </PmtId>\n      <InstdAmt Ccy=\"EUR\">")?;
        write_eur(w, e.amount_ct)?;
        w.write_str("</InstdAmt>\n      <DrctDbtTx>\n        <MndtRltdInf>\n          <MndtId>")?;
        write_escaped(w, &e.mandate_ref)?;
        w.write_str("</MndtId>\n          <DtOfSgntr>")?;
        w.write_str(&e.mandate_signed_at)?;
        w.write_str("</DtOfSgntr>\n        </MndtRltdInf>\n      </DrctDbtTx>\n")?;
        w.write_str("      <DbtrAgt><FinInstnId><BIC>")?;
        w.write_str(debtor_bic)?;
        w.write_str("</BIC></FinInstnId></DbtrAgt>\n      <Dbtr><Nm>")?;
        write_escaped(w, &e.debtor_name)?;
        w.write_str("</Nm></Dbtr>\n      <DbtrAcct><Id><IBAN>")?;
        w.write_str(e.debtor_iban.as_str())?;
        w.write_str("</IBAN></Id></DbtrAcct>\n")?;

        if let Some(desc) = &e.description {
            let truncated = if desc.len() > 140 { &desc[..140] } else { desc };
            w.write_str("      <RmtInf><Ustrd>")?;
            write_escaped(w, truncated)?;
            w.write_str("</Ustrd></RmtInf>\n")?;
        }

        w.write_str("    </DrctDbtTxInf>\n")
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

pub(crate) fn iso8601_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let (y, mo, d) = days_to_ymd(secs / 86400);
    let ss = secs % 60;
    let mm = (secs / 60) % 60;
    let hh = (secs / 3600) % 24;
    format!("{y:04}-{mo:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}")
}

pub(crate) fn epoch_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub(crate) fn default_collection_date() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let (y, m, d) = days_to_ymd(secs / 86400 + 5);
    format!("{y:04}-{m:02}-{d:02}")
}

// SAFETY of the casts in `days_to_ymd`:
//   `days` fits in i64 for any representable calendar date (< 2^62 years from epoch).
//   `doe` = z - era*146097 is always 0..146096 — fits in u32, is non-negative.
//   `yoe` is a u32, losslessly widened to i64 via From.
//   The final year `y` is always positive for years after 0 AD; fits in u32.
#[allow(
    clippy::cast_possible_wrap,       // days fits in i64 for any calendar date
    clippy::cast_possible_truncation, // doe is 0..146096, fits u32
    clippy::cast_sign_loss,           // doe is always non-negative
)]
fn days_to_ymd(days: u64) -> (u32, u32, u32) {
    // Algorithm: https://howardhinnant.github.io/date_algorithms.html
    let z = days as i64 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u32; // 0 ≤ doe < 146_097
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = i64::from(yoe) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (u32::try_from(y).unwrap_or(0), m, d)
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

    fn entry(mandate: &str, amount_ct: i64) -> DirectDebitEntry {
        DirectDebitEntry::new(
            mandate,
            "2024-01-01",
            "Max Mustermann",
            nl_iban(),
            amount_ct,
            mandate,
        )
    }

    #[test]
    fn basic_xml_structure() {
        let xml = Pain008Builder::new("Test GmbH", &de_iban())
            .msg_id("TEST-001")
            .sequence_type(SequenceType::Rcur)
            .collection_date("2026-07-20")
            .add_entry(entry("MND-001", 7500))
            .build_xml();

        assert!(xml.contains("urn:iso:std:iso:20022:tech:xsd:pain.008.003.02"));
        assert!(xml.contains("<MsgId>TEST-001</MsgId>"));
        assert!(xml.contains("<NbOfTxs>1</NbOfTxs>"));
        assert!(xml.contains("<InstdAmt Ccy=\"EUR\">75.00</InstdAmt>"));
        assert!(xml.contains("<MndtId>MND-001</MndtId>"));
        assert!(xml.contains("<SeqTp>RCUR</SeqTp>"));
        assert!(xml.contains("<IBAN>NL91ABNA0417164300</IBAN>"));
        assert!(xml.contains("<ReqdColltnDt>2026-07-20</ReqdColltnDt>"));
        // Indentation: GrpHdr children must have exactly 6 spaces
        assert!(
            xml.contains("      <NbOfTxs>1</NbOfTxs>"),
            "NbOfTxs must be indented 6 spaces"
        );
        assert!(
            xml.contains("      <InitgPty>"),
            "InitgPty must be indented 6 spaces"
        );
    }

    #[test]
    fn frst_sequence_type_in_xml() {
        let xml = Pain008Builder::new("Test", &de_iban())
            .msg_id("T")
            .sequence_type(SequenceType::Frst)
            .add_entry(entry("M1", 1000))
            .build_xml();
        assert!(xml.contains("<SeqTp>FRST</SeqTp>"));
        assert!(!xml.contains("RCUR"));
    }

    #[test]
    fn ooff_sequence_type_in_xml() {
        let xml = Pain008Builder::new("Test", &de_iban())
            .msg_id("T")
            .sequence_type(SequenceType::Ooff)
            .add_entry(entry("M1", 1000))
            .build_xml();
        assert!(xml.contains("<SeqTp>OOFF</SeqTp>"));
    }

    #[test]
    fn ctrl_sum_equals_sum_of_entries() {
        let xml = Pain008Builder::new("Test", &de_iban())
            .msg_id("TEST-002")
            .collection_date("2026-07-20")
            .add_entry(entry("M1", 5_000))
            .add_entry(entry("M2", 7_500))
            .build_xml();
        assert!(xml.contains("<CtrlSum>125.00</CtrlSum>"));
        assert!(xml.contains("<NbOfTxs>2</NbOfTxs>"));
    }

    #[test]
    fn no_f64_rounding() {
        // 10 ct + 20 ct = 30 ct = 0.30 EUR — f64 can represent "0.30000000000000004"
        let builder = Pain008Builder::new("Test", &de_iban())
            .add_entry(entry("M1", 10))
            .add_entry(entry("M2", 20));
        assert_eq!(builder.total_ct(), 30);
        assert!(builder.build_xml().contains("<CtrlSum>0.30</CtrlSum>"));
    }

    #[test]
    fn xml_escaping() {
        let xml = Pain008Builder::new("Test & Co. <GmbH>", &de_iban())
            .msg_id("ESC")
            .build_xml();
        assert!(xml.contains("Test &amp; Co. &lt;GmbH&gt;"));
    }

    #[test]
    fn entry_with_bic_and_description() {
        let bic = "ABNANL2A".parse::<Bic>().unwrap();
        let e = DirectDebitEntry::new("MND", "2024-01-01", "A", nl_iban(), 100, "E2E")
            .with_bic(bic.clone())
            .with_description("Test payment");
        assert_eq!(e.debtor_bic.as_ref().map(Bic::as_str), Some("ABNANL2A"));
        assert_eq!(e.description.as_deref(), Some("Test payment"));
    }

    #[test]
    fn sequence_type_from_str() {
        assert_eq!("FRST".parse::<SequenceType>().unwrap(), SequenceType::Frst);
        assert_eq!("rcur".parse::<SequenceType>().unwrap(), SequenceType::Rcur);
        assert_eq!("OOFF".parse::<SequenceType>().unwrap(), SequenceType::Ooff);
        assert!("INVALID".parse::<SequenceType>().is_err());
    }

    #[test]
    fn sequence_type_display() {
        assert_eq!(SequenceType::Frst.to_string(), "FRST");
        assert_eq!(SequenceType::Ooff.to_string(), "OOFF");
    }

    #[test]
    fn days_to_ymd_epoch() {
        // Unix epoch = 1970-01-01
        assert_eq!(days_to_ymd(0), (1970, 1, 1));
    }

    #[test]
    fn days_to_ymd_known_date() {
        // 2026-07-13 = days since epoch
        // Calculate: (2026-1970)*365 + leap_days + day_of_year
        let days = (2026 - 1970) * 365
            + (1970..2026)
                .filter(|y| y % 4 == 0 && (y % 100 != 0 || y % 400 == 0))
                .count() as u64
            + 31
            + 28
            + 31
            + 30
            + 31
            + 30
            + 13
            - 1; // Jan+Feb+Mar+Apr+May+Jun+13days - 1
        assert_eq!(days_to_ymd(days), (2026, 7, 13));
    }

    #[test]
    fn creditor_scheme_id_in_xml_when_set() {
        use crate::creditor_id::validate_creditor_id;
        let ci = validate_creditor_id("DE74ZZZ09999999999").unwrap();
        let xml = Pain008Builder::new("Test GmbH", &de_iban())
            .msg_id("CI-TEST")
            .creditor_id(ci)
            .add_entry(entry("MND-CI", 5000))
            .build_xml();
        assert!(
            xml.contains("<CdtrSchmeId>"),
            "CdtrSchmeId must be present when creditor_id set"
        );
        assert!(
            xml.contains("DE74ZZZ09999999999"),
            "CI value must appear in XML"
        );
        assert!(
            xml.contains("<Prtry>SEPA</Prtry>"),
            "Scheme name must be SEPA"
        );
    }

    #[test]
    fn no_creditor_scheme_id_when_not_set() {
        let xml = Pain008Builder::new("Test", &de_iban())
            .msg_id("NO-CI")
            .add_entry(entry("MND-1", 1000))
            .build_xml();
        // CdtrSchmeId is optional — absent when not configured
        assert!(
            !xml.contains("<CdtrSchmeId>"),
            "CdtrSchmeId must be absent when creditor_id not set"
        );
    }

    #[test]
    fn chrgbr_slev_always_present() {
        // EPC SDD Rulebook §2.8: ChrgBr must be SLEV
        let xml = Pain008Builder::new("Test", &de_iban())
            .msg_id("CHRGBR")
            .add_entry(entry("M1", 1000))
            .build_xml();
        assert!(
            xml.contains("<ChrgBr>SLEV</ChrgBr>"),
            "ChrgBr SLEV is mandatory per EPC SDD Rulebook"
        );
    }

    #[test]
    fn core_scheme_default() {
        let xml = Pain008Builder::new("Test", &de_iban())
            .add_entry(entry("M1", 1000))
            .build_xml();
        assert!(xml.contains("<Cd>CORE</Cd>"));
        assert!(!xml.contains("<Cd>B2B</Cd>"));
    }

    #[test]
    fn b2b_scheme() {
        let xml = Pain008Builder::new("Test", &de_iban())
            .scheme(DirectDebitScheme::B2b)
            .add_entry(entry("M1", 1000))
            .build_xml();
        assert!(xml.contains("<Cd>B2B</Cd>"));
        assert!(!xml.contains("<Cd>CORE</Cd>"));
        // ChrgBr still required for B2B
        assert!(xml.contains("<ChrgBr>SLEV</ChrgBr>"));
    }

    #[test]
    fn direct_debit_scheme_display() {
        assert_eq!(DirectDebitScheme::Core.to_string(), "CORE");
        assert_eq!(DirectDebitScheme::B2b.to_string(), "B2B");
    }

    #[test]
    fn write_xml_to_io_matches_build_xml() {
        let xml = Pain008Builder::new("Test GmbH", &de_iban())
            .msg_id("IO-TEST")
            .add_entry(entry("M1", 5000))
            .build_xml();

        let mut buf: Vec<u8> = Vec::new();
        Pain008Builder::new("Test GmbH", &de_iban())
            .msg_id("IO-TEST")
            .add_entry(entry("M1", 5000))
            .write_xml_to_io(&mut buf)
            .unwrap();

        assert_eq!(xml, String::from_utf8(buf).unwrap());
    }

    #[test]
    fn xml_escape_in_name_via_write_escaped() {
        // Verify that names with XML special chars are correctly escaped in output
        let xml = Pain008Builder::new("AT&T \"Corp\" <GmbH>", &de_iban())
            .msg_id("ESC2")
            .add_entry(entry("M1", 100))
            .build_xml();
        assert!(xml.contains("AT&amp;T &quot;Corp&quot; &lt;GmbH&gt;"));
    }
}
