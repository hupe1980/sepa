//! ISO 20022 pain.008.003.02 — SEPA Core Direct Debit initiation.
//!
//! Builds standards-compliant pain.008 XML for SEPA Core Direct Debit (SDD Core).
//! All monetary amounts use integer cents (1 ct = 0.01 EUR) — no f64.
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
//! - EPC SEPA Core Direct Debit Rulebook (latest version)
//! - Deutsche Bundesbank pain.008 implementation guide
//!
//! ## Example
//!
//! ```rust
//! use sepa::{validate_iban, Pain008Builder, DirectDebitEntry};
//! use sepa::pain008::SequenceType;
//!
//! let creditor_iban = validate_iban("DE89370400440532013000").unwrap();
//! let debtor_iban   = validate_iban("NL91ABNA0417164300").unwrap();
//!
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
//! assert!(xml.contains("<InstdAmt Ccy=\"EUR\">75.00</InstdAmt>"));
//! assert!(xml.contains("<MndtId>MND-00042</MndtId>"));
//! assert!(xml.contains("<SeqTp>RCUR</SeqTp>"));
//! ```

use std::str::FromStr;

use crate::{Bic, Iban, ct_to_eur_str};

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

/// Builder for an ISO 20022 pain.008.003.02 (SEPA Core Direct Debit) XML batch.
///
/// All entries share the same sequence type (see [`sequence_type`](Self::sequence_type)).
/// For mixed lifecycle collections, use multiple builders.
#[derive(Debug)]
pub struct Pain008Builder {
    creditor_name: String,
    creditor_iban: Iban,
    creditor_bic: Option<Bic>,
    msg_id: String,
    collection_date: String,
    sequence_type: SequenceType,
    entries: Vec<DirectDebitEntry>,
}

impl Pain008Builder {
    /// Create a new builder.
    ///
    /// Defaults: `sequence_type = Rcur`, `msg_id = "sepa-<timestamp>"`,
    /// `collection_date = today + 5 calendar days`.
    pub fn new(creditor_name: impl Into<String>, creditor_iban: &Iban) -> Self {
        Self {
            creditor_name: creditor_name.into(),
            creditor_iban: creditor_iban.clone(),
            creditor_bic: None,
            msg_id: format!("sepa-{}", epoch_secs()),
            collection_date: default_collection_date(),
            sequence_type: SequenceType::Rcur,
            entries: Vec::new(),
        }
    }

    /// Override the `MsgId` (message identifier, max 35 chars).
    #[must_use]
    pub fn msg_id(mut self, id: impl Into<String>) -> Self {
        self.msg_id = id.into();
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
    /// Returns a valid empty-batch document (`NbOfTxs` = 0) when no entries have been added.
    #[must_use]
    pub fn build_xml(&self) -> String {
        let now = iso8601_now();
        let total_eur = ct_to_eur_str(self.total_ct());
        let nb = self.entries.len();
        let creditor_bic = self
            .creditor_bic
            .as_ref()
            .map_or("NOTPROVIDED", Bic::as_str);
        let seq_tp = self.sequence_type.as_code();

        let transactions: String = self
            .entries
            .iter()
            .map(Self::render_transaction)
            .collect::<Vec<_>>()
            .join("\n");

        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<Document xmlns=\"urn:iso:std:iso:20022:tech:xsd:pain.008.003.02\">\n\
  <CstmrDrctDbtInitn>\n\
    <GrpHdr>\n\
      <MsgId>{msg_id}</MsgId>\n\
      <CreDtTm>{now}</CreDtTm>\n\
      <NbOfTxs>{nb}</NbOfTxs>\n\
      <CtrlSum>{total_eur}</CtrlSum>\n\
      <InitgPty><Nm>{creditor_name}</Nm></InitgPty>\n\
    </GrpHdr>\n\
    <PmtInf>\n\
      <PmtInfId>{msg_id}-1</PmtInfId>\n\
      <PmtMtd>DD</PmtMtd>\n\
      <NbOfTxs>{nb}</NbOfTxs>\n\
      <CtrlSum>{total_eur}</CtrlSum>\n\
      <PmtTpInf>\n\
        <SvcLvl><Cd>SEPA</Cd></SvcLvl>\n\
        <LclInstrm><Cd>CORE</Cd></LclInstrm>\n\
        <SeqTp>{seq_tp}</SeqTp>\n\
      </PmtTpInf>\n\
      <ReqdColltnDt>{collection_date}</ReqdColltnDt>\n\
      <Cdtr><Nm>{creditor_name}</Nm></Cdtr>\n\
      <CdtrAcct><Id><IBAN>{creditor_iban}</IBAN></Id></CdtrAcct>\n\
      <CdtrAgt><FinInstnId><BIC>{creditor_bic}</BIC></FinInstnId></CdtrAgt>\n\
{transactions}\n\
    </PmtInf>\n\
  </CstmrDrctDbtInitn>\n\
</Document>",
            msg_id = xml_escape(&self.msg_id),
            now = now,
            nb = nb,
            total_eur = total_eur,
            creditor_name = xml_escape(&self.creditor_name),
            seq_tp = seq_tp,
            collection_date = self.collection_date,
            creditor_iban = self.creditor_iban.as_str(),
            creditor_bic = creditor_bic,
            transactions = transactions,
        )
    }

    fn render_transaction(e: &DirectDebitEntry) -> String {
        let amount_eur = ct_to_eur_str(e.amount_ct);
        let debtor_bic = e.debtor_bic.as_ref().map_or("NOTPROVIDED", Bic::as_str);
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
            "    <DrctDbtTxInf>\n\
      <PmtId>\n\
        <EndToEndId>{end_to_end_id}</EndToEndId>\n\
      </PmtId>\n\
      <InstdAmt Ccy=\"EUR\">{amount_eur}</InstdAmt>\n\
      <DrctDbtTx>\n\
        <MndtRltdInf>\n\
          <MndtId>{mandate_ref}</MndtId>\n\
          <DtOfSgntr>{mandate_signed_at}</DtOfSgntr>\n\
        </MndtRltdInf>\n\
      </DrctDbtTx>\n\
      <DbtrAgt><FinInstnId><BIC>{debtor_bic}</BIC></FinInstnId></DbtrAgt>\n\
      <Dbtr><Nm>{debtor_name}</Nm></Dbtr>\n\
      <DbtrAcct><Id><IBAN>{debtor_iban}</IBAN></Id></DbtrAcct>{remittance}\n\
    </DrctDbtTxInf>",
            end_to_end_id = xml_escape(&e.end_to_end_id),
            amount_eur = amount_eur,
            mandate_ref = xml_escape(&e.mandate_ref),
            mandate_signed_at = e.mandate_signed_at,
            debtor_bic = debtor_bic,
            debtor_name = xml_escape(&e.debtor_name),
            debtor_iban = e.debtor_iban.as_str(),
            remittance = remittance,
        )
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

#[inline]
pub(crate) fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

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
}
