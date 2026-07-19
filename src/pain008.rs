//! ISO 20022 pain.008 — SEPA Direct Debit initiation.
//!
//! Builds standards-compliant pain.008 XML for SEPA Direct Debit (SDD).
//! Supports both CORE (consumer) and B2B (business) scheme variants.
//! All monetary amounts use integer cents (1 ct = 0.01 EUR) — no f64.
//!
//! ## Schema versions
//!
//! | Schema | Status |
//! |---|---|
//! | [`DirectDebitSchema::IsoV8`] — `pain.008.001.08` | Current SEPA version (**default**) |
//! | [`DirectDebitSchema::DkV2_7`] — `pain.008.003.02` | Legacy DK, end-of-life since Nov 2022 |
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
//! ISO 20022 pain.008 places `SeqTp` at the `PmtInf` (payment information)
//! level, not per-transaction — so one [`DirectDebitGroup`] carries one
//! sequence type. Set it with [`DirectDebitGroup::sequence_type`] (default:
//! `Rcur`), and add a group per sequence type to cover a whole collection run
//! in a single file.
//!
//! ## References
//!
//! - ISO 20022 pain.008.001.08 / pain.008.003.02 schemas
//! - EPC SEPA Core Direct Debit Rulebook
//! - EPC SEPA Business-to-Business Direct Debit Rulebook
//! - Deutsche Bundesbank pain.008 implementation guide (DFÜ-Abkommen V2.7)
//!
//! ## Example
//!
//! ```rust
//! use sepa::{
//!     DirectDebitEntry, DirectDebitGroup, Pain008Builder, SequenceType,
//!     validate_creditor_id, validate_iban,
//! };
//! use sepa::pain008::DirectDebitScheme;
//!
//! let creditor = validate_iban("DE89370400440532013000")?;
//! let debtor   = validate_iban("NL91ABNA0417164300")?;
//! // The Creditor Identifier is mandatory for every direct debit.
//! let ci       = validate_creditor_id("DE98ZZZ09999999999")?;
//!
//! let xml = Pain008Builder::new("Creditor GmbH")
//!     .msg_id("BATCH-2026-07-001")
//!     .add_group(
//!         DirectDebitGroup::new("Creditor GmbH", &creditor, ci.clone())
//!             .sequence_type(SequenceType::Rcur)
//!             .collection_date("2026-07-20")
//!             .creditor_bic("COBADEFFXXX".parse()?)
//!             .add_entry(
//!                 DirectDebitEntry::new(
//!                     "MND-00042", "2024-06-01", "Max Mustermann", debtor.clone(), 7_500, "R-001",
//!                 )
//!                 .with_description("Abschlag Juli 2026"),
//!             ),
//!     )
//!     // A B2B collection in the same file, as its own group.
//!     .add_group(
//!         DirectDebitGroup::new("Creditor GmbH", &creditor, ci)
//!             .scheme(DirectDebitScheme::B2b)
//!             .collection_date("2026-07-20")
//!             .add_entry(DirectDebitEntry::new(
//!                 "MND-B2B-001", "2024-01-01", "Corporate AG", debtor, 50_000, "INV-001",
//!             )),
//!     )
//!     .build()?;
//!
//! assert!(xml.contains("<Cd>CORE</Cd>"));
//! assert!(xml.contains("<Cd>B2B</Cd>"));
//! assert!(xml.contains("<ChrgBr>SLEV</ChrgBr>"));
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use std::str::FromStr;

use crate::creditor_id::CreditorId;
use crate::party::Party;
use crate::purpose::{CategoryPurpose, Purpose};
use crate::reference::RemittanceInfo;
use crate::validate::{
    CharsetPolicy, MAX_ID_LEN, ValidationError, WriteError, check_amount, check_date, check_id,
    check_name, check_remittance, truncate_chars,
};
use crate::{Bic, Iban, ct_to_eur_str};

// ── DirectDebitSchema ─────────────────────────────────────────────────────────

/// pain.008 XML schema version to emit.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DirectDebitSchema {
    /// `pain.008.001.08` — the current SEPA version (**default**).
    ///
    /// Mandated by the EPC 2023 SDD rulebooks from 19 November 2023 and carried
    /// unchanged into the 2025 rulebooks.
    ///
    /// Names the agent BIC element `BICFI`. Unlike pain.001.001.09, the
    /// collection date stays a bare `ISODate` — SDD did **not** move to a
    /// date/time choice, so `ReqdColltnDt` is written the same way in both
    /// versions.
    #[default]
    IsoV8,

    /// `pain.008.003.02` — legacy Deutsche Kreditwirtschaft DK V2.7 (2013).
    ///
    /// **End-of-life** since DK Anlage 3 V3.6 (November 2022). Retained for
    /// systems still pinned to it. Names the agent BIC element `BIC`.
    DkV2_7,
}

impl DirectDebitSchema {
    /// The XML namespace URI for this schema version.
    #[must_use]
    pub const fn namespace(self) -> &'static str {
        match self {
            Self::DkV2_7 => "urn:iso:std:iso:20022:tech:xsd:pain.008.003.02",
            Self::IsoV8 => "urn:iso:std:iso:20022:tech:xsd:pain.008.001.08",
        }
    }

    /// The element name carrying an agent's BIC (`BIC` before the 2019 rename).
    #[must_use]
    const fn bic_element(self) -> &'static str {
        match self {
            Self::DkV2_7 => "BIC",
            Self::IsoV8 => "BICFI",
        }
    }
}

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

// ── MandateAmendment ──────────────────────────────────────────────────────────

/// A change to a mandate since the last collection (`AmdmntInfDtls`).
///
/// When anything about the mandate changes, the next collection must carry
/// `AmdmntInd = true` plus the details of what changed. Sending an amendment
/// that is identical to the original is non-conformant, and the debtor's bank
/// is recommended to reject it with `MD02`.
///
/// ## The `SMNDA` marker
///
/// `SMNDA` means *same mandate, new debtor account*. Its placement changed:
/// before EPC IG v9.0 (effective November 2016) it went in
/// `OrgnlDbtrAgt/FinInstnId/Othr/Id` and meant "new debtor **agent**"; since
/// v9.0 it goes in `OrgnlDbtrAcct/Id/Othr/Id` and means "new debtor
/// **account**". This is an implementation-guideline change, not a schema one —
/// both forms are schema-valid in `pain.008.001.02`, so only the effective IG
/// version distinguishes them. This crate emits the current form.
///
/// ## Sequence types are unaffected
///
/// An amendment does **not** reset the sequence type to `FRST`. All four codes
/// remain valid alongside `AmdmntInd = true`; carry on with `RCUR` if that is
/// where the mandate was.
///
/// ## Examples
///
/// ```
/// use sepa::pain008::MandateAmendment;
/// use sepa::validate_creditor_id;
///
/// // The debtor moved to a different account or bank.
/// let a = MandateAmendment::debtor_account_changed();
///
/// // The creditor identifier changed — carry the old one.
/// let old = validate_creditor_id("DE98ZZZ09999999999").unwrap();
/// let b = MandateAmendment::creditor_id_changed(old);
///
/// // The mandate was renumbered.
/// let c = MandateAmendment::mandate_id_changed("OLD-MND-001");
/// # let _ = (a, b, c);
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MandateAmendment {
    /// The previous mandate reference (`OrgnlMndtId`).
    pub original_mandate_id: Option<String>,
    /// The previous creditor name (`OrgnlCdtrSchmeId/Nm`).
    pub original_creditor_name: Option<String>,
    /// The previous Creditor Identifier (`OrgnlCdtrSchmeId/Id`).
    pub original_creditor_id: Option<CreditorId>,
    /// The previous debtor IBAN (`OrgnlDbtrAcct/Id/IBAN`).
    ///
    /// Mutually exclusive with `same_mandate_new_account`.
    pub original_debtor_iban: Option<Iban>,
    /// Emit the `SMNDA` marker — the debtor changed account or bank.
    pub same_mandate_new_account: bool,
}

impl MandateAmendment {
    /// The debtor moved to a different account or a different bank.
    ///
    /// Emits the `SMNDA` marker. Use this when you do not know, or do not wish
    /// to disclose, the previous IBAN — it is also what the DK recommends even
    /// when the debtor stayed at the same bank.
    #[must_use]
    pub fn debtor_account_changed() -> Self {
        Self {
            same_mandate_new_account: true,
            ..Self::default()
        }
    }

    /// The debtor moved to a new IBAN and you want to state the previous one.
    #[must_use]
    pub fn debtor_iban_changed(previous: Iban) -> Self {
        Self {
            original_debtor_iban: Some(previous),
            ..Self::default()
        }
    }

    /// The creditor's SEPA Creditor Identifier changed.
    #[must_use]
    pub fn creditor_id_changed(previous: CreditorId) -> Self {
        Self {
            original_creditor_id: Some(previous),
            ..Self::default()
        }
    }

    /// The creditor was renamed.
    pub fn creditor_name_changed(previous: impl Into<String>) -> Self {
        Self {
            original_creditor_name: Some(previous.into()),
            ..Self::default()
        }
    }

    /// The mandate reference changed.
    pub fn mandate_id_changed(previous: impl Into<String>) -> Self {
        Self {
            original_mandate_id: Some(previous.into()),
            ..Self::default()
        }
    }

    /// Also record the previous mandate reference.
    #[must_use]
    pub fn with_original_mandate_id(mut self, previous: impl Into<String>) -> Self {
        self.original_mandate_id = Some(previous.into());
        self
    }

    /// Also record the previous creditor name.
    #[must_use]
    pub fn with_original_creditor_name(mut self, previous: impl Into<String>) -> Self {
        self.original_creditor_name = Some(previous.into());
        self
    }

    /// Validate the amendment.
    ///
    /// # Errors
    ///
    /// Returns [`ValidationError::Empty`] when nothing actually changed — an
    /// amendment carrying no detail is rejected by the debtor's bank — or a
    /// length/character error on the individual fields.
    pub fn validate(&self, charset: CharsetPolicy) -> Result<(), ValidationError> {
        if self.original_mandate_id.is_none()
            && self.original_creditor_name.is_none()
            && self.original_creditor_id.is_none()
            && self.original_debtor_iban.is_none()
            && !self.same_mandate_new_account
        {
            return Err(ValidationError::Empty {
                field: "MndtRltdInf/AmdmntInfDtls",
            });
        }
        if let Some(id) = &self.original_mandate_id {
            check_id("AmdmntInfDtls/OrgnlMndtId", id)?;
        }
        if let Some(name) = &self.original_creditor_name {
            check_name(
                "OrgnlCdtrSchmeId/Nm",
                &charset.apply("OrgnlCdtrSchmeId/Nm", name)?,
            )?;
        }
        Ok(())
    }

    /// Write `AmdmntInd` and `AmdmntInfDtls` inside `MndtRltdInf`.
    fn write_xml<W: std::fmt::Write>(&self, w: &mut W, charset: CharsetPolicy) -> std::fmt::Result {
        use crate::xml_util::write_escaped;

        w.write_str("          <AmdmntInd>true</AmdmntInd>\n")?;
        w.write_str("          <AmdmntInfDtls>")?;

        if let Some(id) = &self.original_mandate_id {
            w.write_str("<OrgnlMndtId>")?;
            write_escaped(w, id)?;
            w.write_str("</OrgnlMndtId>")?;
        }

        if self.original_creditor_name.is_some() || self.original_creditor_id.is_some() {
            w.write_str("<OrgnlCdtrSchmeId>")?;
            if let Some(name) = &self.original_creditor_name {
                let name = charset
                    .apply("OrgnlCdtrSchmeId/Nm", name)
                    .unwrap_or(std::borrow::Cow::Borrowed(name));
                w.write_str("<Nm>")?;
                write_escaped(w, &name)?;
                w.write_str("</Nm>")?;
            }
            if let Some(ci) = &self.original_creditor_id {
                w.write_str("<Id><PrvtId><Othr><Id>")?;
                w.write_str(ci.as_str())?;
                w.write_str("</Id><SchmeNm><Prtry>SEPA</Prtry></SchmeNm></Othr></PrvtId></Id>")?;
            }
            w.write_str("</OrgnlCdtrSchmeId>")?;
        }

        // SMNDA and an explicit previous IBAN are alternatives, not siblings.
        if self.same_mandate_new_account {
            w.write_str("<OrgnlDbtrAcct><Id><Othr><Id>SMNDA</Id></Othr></Id></OrgnlDbtrAcct>")?;
        } else if let Some(iban) = &self.original_debtor_iban {
            w.write_str("<OrgnlDbtrAcct><Id><IBAN>")?;
            w.write_str(iban.as_str())?;
            w.write_str("</IBAN></Id></OrgnlDbtrAcct>")?;
        }

        w.write_str("</AmdmntInfDtls>\n")
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
    /// Remittance information (`RmtInf`) — free text or a structured reference.
    pub remittance: Option<RemittanceInfo>,
    /// Ultimate creditor (`UltmtCdtr`) — who the collection is really for.
    pub ultimate_creditor: Option<Party>,
    /// Ultimate debtor (`UltmtDbtr`) — who is really being debited.
    ///
    /// The EPC makes this **conditionally mandatory**: populate it whenever the
    /// mandate names a debtor other than the account holder.
    pub ultimate_debtor: Option<Party>,
    /// Purpose code (`Purp/Cd`), informational.
    pub purpose: Option<Purpose>,
    /// Mandate amendment details (`AmdmntInd` / `AmdmntInfDtls`).
    pub amendment: Option<MandateAmendment>,
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
            remittance: None,
            ultimate_creditor: None,
            ultimate_debtor: None,
            purpose: None,
            amendment: None,
        }
    }

    /// Set the debtor's BIC (optional — use when known for faster processing).
    #[must_use]
    pub fn with_bic(mut self, bic: Bic) -> Self {
        self.debtor_bic = Some(bic);
        self
    }

    /// Set free-text remittance information (`RmtInf/Ustrd`), max 140 characters.
    ///
    /// This is the German *Verwendungszweck*: human-readable, but useless for
    /// automatic reconciliation. Prefer
    /// [`with_reference`](Self::with_reference) when you control the invoice.
    #[must_use]
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.remittance = Some(RemittanceInfo::Unstructured(desc.into()));
        self
    }

    /// Set a structured ISO 11649 creditor reference (`RmtInf/Strd/CdtrRefInf`).
    ///
    /// Mutually exclusive with [`with_description`](Self::with_description) —
    /// the EPC permits one or the other, and the later call wins.
    #[must_use]
    pub fn with_reference(mut self, reference: crate::RfReference) -> Self {
        self.remittance = Some(RemittanceInfo::Structured(reference));
        self
    }

    /// Set remittance information directly.
    #[must_use]
    pub fn with_remittance(mut self, remittance: RemittanceInfo) -> Self {
        self.remittance = Some(remittance);
        self
    }

    /// Set the ultimate creditor (`UltmtCdtr`).
    #[must_use]
    pub fn with_ultimate_creditor(mut self, party: impl Into<Party>) -> Self {
        self.ultimate_creditor = Some(party.into());
        self
    }

    /// Set the ultimate debtor (`UltmtDbtr`).
    ///
    /// Populate this whenever the mandate names a debtor other than the account
    /// holder — the EPC treats it as mandatory in that case.
    #[must_use]
    pub fn with_ultimate_debtor(mut self, party: impl Into<Party>) -> Self {
        self.ultimate_debtor = Some(party.into());
        self
    }

    /// Set the purpose code (`Purp/Cd`).
    #[must_use]
    pub fn with_purpose(mut self, purpose: Purpose) -> Self {
        self.purpose = Some(purpose);
        self
    }

    /// Declare a mandate amendment (`AmdmntInd` = `true`).
    ///
    /// Required whenever the mandate changed since the last collection — a new
    /// debtor account, a new creditor identifier, a renamed creditor or a
    /// renumbered mandate. Omitting it gets the collection rejected with `MD02`.
    ///
    /// See [`MandateAmendment`] for the individual scenarios.
    #[must_use]
    pub fn with_amendment(mut self, amendment: MandateAmendment) -> Self {
        self.amendment = Some(amendment);
        self
    }
}

// ── DirectDebitGroup ──────────────────────────────────────────────────────────

/// One `PmtInf` block — collections sharing a creditor account, a sequence
/// type, a scheme and a collection date.
///
/// A pain.008 message may carry several. That is the only way to put `FRST` and
/// `RCUR` collections, or different collection dates, into a single file
/// instead of submitting several files to the bank — which is the normal shape
/// of a real direct debit run.
///
/// ## Examples
///
/// ```
/// use sepa::{DirectDebitEntry, DirectDebitGroup, SequenceType, validate_creditor_id, validate_iban};
///
/// let iban = validate_iban("DE89370400440532013000")?;
/// let ci = validate_creditor_id("DE98ZZZ09999999999")?;
///
/// let first = DirectDebitGroup::new("Stadtwerke GmbH", &iban, ci)
///     .sequence_type(SequenceType::Frst)
///     .collection_date("2026-07-20")
///     .add_entry(DirectDebitEntry::new(
///         "MND-1", "2026-06-01", "Neu Kunde", iban.clone(), 5_000, "E2E-1",
///     ));
/// assert_eq!(first.entry_count(), 1);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct DirectDebitGroup {
    payment_info_id: Option<String>,
    creditor_name: String,
    creditor_iban: Iban,
    creditor_bic: Option<Bic>,
    creditor_id: CreditorId,
    sequence_type: SequenceType,
    scheme: DirectDebitScheme,
    collection_date: String,
    batch_booking: Option<bool>,
    category_purpose: Option<CategoryPurpose>,
    ultimate_creditor: Option<Party>,
    entries: Vec<DirectDebitEntry>,
}

impl DirectDebitGroup {
    /// A new collection group.
    ///
    /// The Creditor Identifier is required up front — the EPC mandates
    /// `CdtrSchmeId` for every direct debit, so there is no valid group without
    /// one.
    pub fn new(
        creditor_name: impl Into<String>,
        creditor_iban: &Iban,
        creditor_id: CreditorId,
    ) -> Self {
        Self {
            payment_info_id: None,
            creditor_name: creditor_name.into(),
            creditor_iban: creditor_iban.clone(),
            creditor_bic: None,
            creditor_id,
            sequence_type: SequenceType::Rcur,
            scheme: DirectDebitScheme::Core,
            collection_date: default_collection_date(),
            batch_booking: None,
            category_purpose: None,
            ultimate_creditor: None,
            entries: Vec::new(),
        }
    }

    /// Override the `PmtInfId`. Defaults as described on [`Pain008Builder`].
    #[must_use]
    pub fn payment_info_id(mut self, id: impl Into<String>) -> Self {
        self.payment_info_id = Some(id.into());
        self
    }

    /// Set the sequence type for this group (default `RCUR`).
    ///
    /// `SeqTp` sits at `PmtInf` level, so one group carries one sequence type.
    /// Use separate groups to mix `FRST` and `RCUR` in the same file.
    #[must_use]
    pub fn sequence_type(mut self, st: SequenceType) -> Self {
        self.sequence_type = st;
        self
    }

    /// Set the scheme (default [`DirectDebitScheme::Core`]).
    #[must_use]
    pub fn scheme(mut self, scheme: DirectDebitScheme) -> Self {
        self.scheme = scheme;
        self
    }

    /// Set the requested collection date (`ReqdColltnDt`), `YYYY-MM-DD`.
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

    /// Request batch booking (`BtchBookg`).
    ///
    /// Omitted by default, which defers to the agreement with the bank. German
    /// banks treat an absent value as `true`, and honour `false` only where a
    /// single-entry agreement exists.
    #[must_use]
    pub fn batch_booking(mut self, batch: bool) -> Self {
        self.batch_booking = Some(batch);
        self
    }

    /// Set the category purpose (`PmtTpInf/CtgyPurp`).
    #[must_use]
    pub fn category_purpose(mut self, purpose: CategoryPurpose) -> Self {
        self.category_purpose = Some(purpose);
        self
    }

    /// Set the ultimate creditor for the whole group (`PmtInf/UltmtCdtr`).
    ///
    /// Mutually exclusive with the per-entry ultimate creditor.
    #[must_use]
    pub fn ultimate_creditor(mut self, party: impl Into<Party>) -> Self {
        self.ultimate_creditor = Some(party.into());
        self
    }

    /// Add a collection to this group.
    #[must_use]
    pub fn add_entry(mut self, entry: DirectDebitEntry) -> Self {
        self.entries.push(entry);
        self
    }

    /// Add several collections.
    #[must_use]
    pub fn add_entries(mut self, entries: impl IntoIterator<Item = DirectDebitEntry>) -> Self {
        self.entries.extend(entries);
        self
    }

    /// Number of collections in this group.
    #[must_use]
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// Total amount in ct across this group, saturating on overflow.
    #[must_use]
    pub fn total_ct(&self) -> i64 {
        self.entries
            .iter()
            .fold(0i64, |acc, e| acc.saturating_add(e.amount_ct))
    }
}

// ── Builder ───────────────────────────────────────────────────────────────────

/// Builder for an ISO 20022 pain.008 (SEPA Direct Debit) message.
///
/// A message carries one or more [`DirectDebitGroup`]s, each becoming a
/// `PmtInf` block. `PmtInfId` defaults to the `MsgId` for a single group and to
/// `MsgId-<n>` when there are several, truncated to stay inside 35 characters.
#[derive(Debug, Clone)]
pub struct Pain008Builder {
    initiating_party: String,
    msg_id: String,
    created_at: Option<String>,
    schema: DirectDebitSchema,
    charset: CharsetPolicy,
    groups: Vec<DirectDebitGroup>,
}

impl Pain008Builder {
    /// A new message initiated by `initiating_party`.
    pub fn new(initiating_party: impl Into<String>) -> Self {
        Self {
            initiating_party: initiating_party.into(),
            msg_id: format!("sepa-{}", epoch_secs()),
            created_at: None,
            schema: DirectDebitSchema::default(),
            charset: CharsetPolicy::default(),
            groups: Vec::new(),
        }
    }

    /// Override the `MsgId` (max 35 chars).
    #[must_use]
    pub fn msg_id(mut self, id: impl Into<String>) -> Self {
        self.msg_id = id.into();
        self
    }

    /// Pin the creation timestamp (`CreDtTm`), making output reproducible.
    #[must_use]
    pub fn created_at(mut self, timestamp: impl Into<String>) -> Self {
        self.created_at = Some(timestamp.into());
        self
    }

    /// Override the pain.008 schema version.
    #[must_use]
    pub fn schema(mut self, schema: DirectDebitSchema) -> Self {
        self.schema = schema;
        self
    }

    /// Set how text outside the SEPA character set is handled.
    #[must_use]
    pub fn charset(mut self, policy: CharsetPolicy) -> Self {
        self.charset = policy;
        self
    }

    /// Add a collection group (`PmtInf`).
    #[must_use]
    pub fn add_group(mut self, group: DirectDebitGroup) -> Self {
        self.groups.push(group);
        self
    }

    /// Number of collection groups.
    #[must_use]
    pub fn group_count(&self) -> usize {
        self.groups.len()
    }

    /// Total number of collections across every group.
    #[must_use]
    pub fn entry_count(&self) -> usize {
        self.groups.iter().map(DirectDebitGroup::entry_count).sum()
    }

    /// Total amount in ct across every group, saturating on overflow.
    #[must_use]
    pub fn total_ct(&self) -> i64 {
        self.groups
            .iter()
            .fold(0i64, |acc, g| acc.saturating_add(g.total_ct()))
    }

    /// The effective `PmtInfId` for group `index`.
    fn payment_info_id(&self, index: usize) -> String {
        if let Some(id) = self
            .groups
            .get(index)
            .and_then(|g| g.payment_info_id.clone())
        {
            return id;
        }
        if self.groups.len() <= 1 {
            return self.msg_id.clone();
        }
        let suffix = format!("-{}", index + 1);
        let keep = MAX_ID_LEN.saturating_sub(suffix.chars().count());
        format!("{}{suffix}", truncate_chars(&self.msg_id, keep))
    }

    /// Validate the message without producing XML.
    ///
    /// # Errors
    ///
    /// See [`ValidationError`].
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.groups.is_empty() || self.entry_count() == 0 {
            return Err(ValidationError::EmptyBatch);
        }
        check_id("GrpHdr/MsgId", &self.msg_id)?;
        check_name(
            "InitgPty/Nm",
            &self.charset.apply("InitgPty/Nm", &self.initiating_party)?,
        )?;

        let mut total: i64 = 0;
        for (i, g) in self.groups.iter().enumerate() {
            if g.entries.is_empty() {
                return Err(ValidationError::EmptyBatch);
            }
            check_id("PmtInf/PmtInfId", &self.payment_info_id(i))?;
            check_date("PmtInf/ReqdColltnDt", &g.collection_date)?;
            check_name("Cdtr/Nm", &self.charset.apply("Cdtr/Nm", &g.creditor_name)?)?;
            if let Some(p) = &g.category_purpose {
                p.validate("PmtTpInf/CtgyPurp/Cd")?;
            }
            if let Some(p) = &g.ultimate_creditor {
                p.validate("PmtInf/UltmtCdtr", self.charset)?;
            }

            for e in &g.entries {
                if g.ultimate_creditor.is_some() && e.ultimate_creditor.is_some() {
                    return Err(ValidationError::ConflictingLevels { field: "UltmtCdtr" });
                }
                check_id("DrctDbtTxInf/PmtId/EndToEndId", &e.end_to_end_id)?;
                check_id("MndtRltdInf/MndtId", &e.mandate_ref)?;
                check_date("MndtRltdInf/DtOfSgntr", &e.mandate_signed_at)?;
                check_amount("DrctDbtTxInf/InstdAmt", e.amount_ct)?;
                check_name("Dbtr/Nm", &self.charset.apply("Dbtr/Nm", &e.debtor_name)?)?;
                if let Some(p) = &e.ultimate_creditor {
                    p.validate("DrctDbtTxInf/UltmtCdtr", self.charset)?;
                }
                if let Some(p) = &e.ultimate_debtor {
                    p.validate("DrctDbtTxInf/UltmtDbtr", self.charset)?;
                }
                if let Some(p) = &e.purpose {
                    p.validate("DrctDbtTxInf/Purp/Cd")?;
                }
                if let Some(a) = &e.amendment {
                    a.validate(self.charset)?;
                }
                if let Some(r) = &e.remittance {
                    if let RemittanceInfo::Unstructured(text) = r {
                        check_remittance(
                            "RmtInf/Ustrd",
                            &self.charset.apply("RmtInf/Ustrd", text)?,
                        )?;
                    } else {
                        r.validate("RmtInf/Strd")?;
                    }
                }
                total = total
                    .checked_add(e.amount_ct)
                    .ok_or(ValidationError::ControlSumOverflow)?;
            }
        }
        Ok(())
    }

    /// Validate the message and generate the pain.008 XML.
    ///
    /// # Errors
    ///
    /// See [`validate`](Self::validate).
    ///
    /// # Examples
    ///
    /// A single file carrying both a first and a recurring collection:
    ///
    /// ```
    /// use sepa::{
    ///     DirectDebitEntry, DirectDebitGroup, Pain008Builder, SequenceType,
    ///     validate_creditor_id, validate_iban,
    /// };
    ///
    /// let iban = validate_iban("DE89370400440532013000")?;
    /// let ci = validate_creditor_id("DE98ZZZ09999999999")?;
    ///
    /// let xml = Pain008Builder::new("Stadtwerke GmbH")
    ///     .msg_id("DD-2026-07")
    ///     .add_group(
    ///         DirectDebitGroup::new("Stadtwerke GmbH", &iban, ci.clone())
    ///             .sequence_type(SequenceType::Frst)
    ///             .collection_date("2026-07-20")
    ///             .add_entry(DirectDebitEntry::new(
    ///                 "MND-1", "2026-06-01", "Neu Kunde", iban.clone(), 5_000, "E2E-1",
    ///             )),
    ///     )
    ///     .add_group(
    ///         DirectDebitGroup::new("Stadtwerke GmbH", &iban, ci)
    ///             .sequence_type(SequenceType::Rcur)
    ///             .collection_date("2026-07-18")
    ///             .add_entry(DirectDebitEntry::new(
    ///                 "MND-2", "2024-06-01", "Alt Kunde", iban.clone(), 7_500, "E2E-2",
    ///             )),
    ///     )
    ///     .build()?;
    ///
    /// assert!(xml.contains("<SeqTp>FRST</SeqTp>"));
    /// assert!(xml.contains("<SeqTp>RCUR</SeqTp>"));
    /// assert!(xml.contains("<NbOfTxs>2</NbOfTxs>")); // group header total
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn build(&self) -> Result<String, ValidationError> {
        self.validate()?;
        let mut buf = String::with_capacity(900 + self.entry_count() * 480);
        let _ = self.write_xml_to(&mut buf);
        Ok(buf)
    }

    /// Validate and stream the pain.008 XML to an [`io::Write`](std::io::Write).
    ///
    /// # Errors
    ///
    /// [`WriteError::Validation`] or [`WriteError::Io`].
    pub fn write_to<W: std::io::Write>(&self, w: &mut W) -> Result<(), WriteError> {
        self.validate()?;
        let mut bridge = crate::xml_util::IoWriterBridge {
            inner: w,
            error: None,
        };
        if self.write_xml_to(&mut bridge).is_err() {
            return Err(WriteError::Io(bridge.error.unwrap_or_else(|| {
                std::io::Error::other("XML serialisation failed")
            })));
        }
        Ok(())
    }

    /// Serialise. Private: callers go through `build` or `write_to`.
    fn write_xml_to<W: std::fmt::Write>(&self, w: &mut W) -> std::fmt::Result {
        use crate::xml_util::write_escaped;

        let now = self.created_at.clone().unwrap_or_else(iso8601_now);
        let namespace = self.schema.namespace();
        let initiating = self
            .charset
            .apply("InitgPty/Nm", &self.initiating_party)
            .unwrap_or(std::borrow::Cow::Borrowed(&self.initiating_party));

        w.write_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n")?;
        writeln!(w, "<Document xmlns=\"{namespace}\">")?;
        w.write_str("  <CstmrDrctDbtInitn>\n    <GrpHdr>\n      <MsgId>")?;
        write_escaped(w, &self.msg_id)?;
        write!(w, "</MsgId>\n      <CreDtTm>{now}</CreDtTm>\n")?;
        write!(
            w,
            "      <NbOfTxs>{}</NbOfTxs>\n      <CtrlSum>{}</CtrlSum>\n",
            self.entry_count(),
            ct_to_eur_str(self.total_ct())
        )?;
        w.write_str("      <InitgPty><Nm>")?;
        write_escaped(w, &initiating)?;
        w.write_str("</Nm></InitgPty>\n    </GrpHdr>\n")?;

        for (i, g) in self.groups.iter().enumerate() {
            self.write_group(w, g, &self.payment_info_id(i))?;
        }

        w.write_str("  </CstmrDrctDbtInitn>\n</Document>")
    }

    /// Write one `PmtInf` block, in XSD sequence order.
    fn write_group<W: std::fmt::Write>(
        &self,
        w: &mut W,
        g: &DirectDebitGroup,
        payment_info_id: &str,
    ) -> std::fmt::Result {
        use crate::xml_util::write_escaped;

        let bic_el = self.schema.bic_element();
        let creditor_name = self
            .charset
            .apply("Cdtr/Nm", &g.creditor_name)
            .unwrap_or(std::borrow::Cow::Borrowed(&g.creditor_name));

        w.write_str("    <PmtInf>\n      <PmtInfId>")?;
        write_escaped(w, payment_info_id)?;
        w.write_str("</PmtInfId>\n      <PmtMtd>DD</PmtMtd>\n")?;
        if let Some(batch) = g.batch_booking {
            writeln!(w, "      <BtchBookg>{batch}</BtchBookg>")?;
        }
        write!(
            w,
            "      <NbOfTxs>{}</NbOfTxs>\n      <CtrlSum>{}</CtrlSum>\n",
            g.entry_count(),
            ct_to_eur_str(g.total_ct())
        )?;
        w.write_str("      <PmtTpInf>\n        <SvcLvl><Cd>SEPA</Cd></SvcLvl>\n")?;
        writeln!(
            w,
            "        <LclInstrm><Cd>{}</Cd></LclInstrm>",
            g.scheme.as_code()
        )?;
        writeln!(w, "        <SeqTp>{}</SeqTp>", g.sequence_type.as_code())?;
        if let Some(p) = &g.category_purpose {
            writeln!(w, "        <CtgyPurp><Cd>{}</Cd></CtgyPurp>", p.as_code())?;
        }
        w.write_str("      </PmtTpInf>\n")?;
        writeln!(
            w,
            "      <ReqdColltnDt>{}</ReqdColltnDt>",
            g.collection_date
        )?;

        w.write_str("      <Cdtr><Nm>")?;
        write_escaped(w, &creditor_name)?;
        w.write_str("</Nm></Cdtr>\n")?;
        writeln!(
            w,
            "      <CdtrAcct><Id><IBAN>{}</IBAN></Id></CdtrAcct>",
            g.creditor_iban.as_str()
        )?;
        w.write_str("      <CdtrAgt><FinInstnId>")?;
        match &g.creditor_bic {
            Some(bic) => write!(w, "<{bic_el}>{}</{bic_el}>", bic.as_str())?,
            None => w.write_str("<Othr><Id>NOTPROVIDED</Id></Othr>")?,
        }
        w.write_str("</FinInstnId></CdtrAgt>\n")?;

        if let Some(p) = &g.ultimate_creditor {
            p.write_xml(w, "UltmtCdtr", "      ", self.charset)?;
        }
        w.write_str("      <ChrgBr>SLEV</ChrgBr>\n")?;

        // CdtrSchmeId is mandatory for SDD and sits last before the transactions.
        w.write_str("      <CdtrSchmeId><Id><PrvtId><Othr><Id>")?;
        w.write_str(g.creditor_id.as_str())?;
        w.write_str(
            "</Id><SchmeNm><Prtry>SEPA</Prtry></SchmeNm></Othr></PrvtId></Id></CdtrSchmeId>\n",
        )?;

        for entry in &g.entries {
            self.write_transaction(w, entry)?;
        }
        w.write_str("    </PmtInf>\n")
    }

    fn write_transaction<W: std::fmt::Write>(
        &self,
        w: &mut W,
        e: &DirectDebitEntry,
    ) -> std::fmt::Result {
        use crate::xml_util::{write_escaped, write_eur};

        let bic_el = self.schema.bic_element();
        let debtor_name = self
            .charset
            .apply("Dbtr/Nm", &e.debtor_name)
            .unwrap_or(std::borrow::Cow::Borrowed(&e.debtor_name));

        w.write_str("    <DrctDbtTxInf>\n      <PmtId>\n        <EndToEndId>")?;
        write_escaped(w, &e.end_to_end_id)?;
        w.write_str("</EndToEndId>\n      </PmtId>\n      <InstdAmt Ccy=\"EUR\">")?;
        write_eur(w, e.amount_ct)?;
        w.write_str("</InstdAmt>\n      <DrctDbtTx>\n        <MndtRltdInf>\n          <MndtId>")?;
        write_escaped(w, &e.mandate_ref)?;
        w.write_str("</MndtId>\n          <DtOfSgntr>")?;
        w.write_str(&e.mandate_signed_at)?;
        w.write_str("</DtOfSgntr>\n")?;
        if let Some(amendment) = &e.amendment {
            amendment.write_xml(w, self.charset)?;
        }
        w.write_str("        </MndtRltdInf>\n      </DrctDbtTx>\n")?;

        // XSD sequence: UltmtCdtr sits between DrctDbtTx and DbtrAgt.
        if let Some(p) = &e.ultimate_creditor {
            p.write_xml(w, "UltmtCdtr", "      ", self.charset)?;
        }
        w.write_str("      <DbtrAgt><FinInstnId>")?;
        match &e.debtor_bic {
            Some(bic) => write!(w, "<{bic_el}>{}</{bic_el}>", bic.as_str())?,
            None => w.write_str("<Othr><Id>NOTPROVIDED</Id></Othr>")?,
        }
        w.write_str("</FinInstnId></DbtrAgt>\n      <Dbtr><Nm>")?;
        write_escaped(w, &debtor_name)?;
        w.write_str("</Nm></Dbtr>\n      <DbtrAcct><Id><IBAN>")?;
        w.write_str(e.debtor_iban.as_str())?;
        w.write_str("</IBAN></Id></DbtrAcct>\n")?;

        // XSD sequence: UltmtDbtr follows DbtrAcct, then Purp, then RmtInf.
        if let Some(p) = &e.ultimate_debtor {
            p.write_xml(w, "UltmtDbtr", "      ", self.charset)?;
        }
        if let Some(purpose) = &e.purpose {
            writeln!(w, "      <Purp><Cd>{}</Cd></Purp>", purpose.as_code())?;
        }

        if let Some(remittance) = &e.remittance {
            // `Strd` is emitted minified: the EPC caps the whole block at 140
            // characters including tags, and pretty-printing alone overruns it.
            remittance.write_xml(w, "      ", self.charset)?;
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
    use crate::validate::{CharsetPolicy, ValidationError};

    fn de_iban() -> Iban {
        validate_iban("DE89370400440532013000").unwrap()
    }
    fn nl_iban() -> Iban {
        validate_iban("NL91ABNA0417164300").unwrap()
    }
    fn ci() -> CreditorId {
        crate::validate_creditor_id("DE98ZZZ09999999999").unwrap()
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
    fn group(name: &str) -> DirectDebitGroup {
        DirectDebitGroup::new(name, &de_iban(), ci()).collection_date("2026-07-20")
    }
    fn one_group(name: &str) -> Pain008Builder {
        Pain008Builder::new(name)
            .msg_id("DD-001")
            .add_group(group(name).add_entry(entry("MND-001", 7_500)))
    }

    #[test]
    fn basic_structure() {
        let xml = one_group("Test GmbH").build().unwrap();
        assert!(xml.contains("urn:iso:std:iso:20022:tech:xsd:pain.008.001.08"));
        assert!(xml.contains("<MsgId>DD-001</MsgId>"));
        assert!(xml.contains("<PmtMtd>DD</PmtMtd>"));
        assert!(xml.contains("<InstdAmt Ccy=\"EUR\">75.00</InstdAmt>"));
        assert!(xml.contains("<SeqTp>RCUR</SeqTp>"));
        assert!(xml.contains("<Cd>CORE</Cd>"));
        assert!(xml.contains("<ChrgBr>SLEV</ChrgBr>"));
        assert!(xml.contains("<CdtrSchmeId>"));
        // SDD keeps a bare ISODate — unlike pain.001.001.09.
        assert!(xml.contains("<ReqdColltnDt>2026-07-20</ReqdColltnDt>"));
    }

    #[test]
    fn first_and_recurring_collections_fit_in_one_file() {
        // The whole point of the group restructure: a real direct debit run has
        // both, and previously needed two files.
        let xml = Pain008Builder::new("Stadtwerke GmbH")
            .msg_id("DD-RUN")
            .add_group(
                group("Stadtwerke GmbH")
                    .sequence_type(SequenceType::Frst)
                    .collection_date("2026-07-20")
                    .add_entry(entry("MND-NEW", 5_000)),
            )
            .add_group(
                group("Stadtwerke GmbH")
                    .sequence_type(SequenceType::Rcur)
                    .collection_date("2026-07-18")
                    .add_entry(entry("MND-OLD", 7_500))
                    .add_entry(entry("MND-OLD-2", 2_500)),
            )
            .build()
            .unwrap();

        assert_eq!(xml.matches("<PmtInf>").count(), 2);
        assert!(xml.contains("<SeqTp>FRST</SeqTp>"));
        assert!(xml.contains("<SeqTp>RCUR</SeqTp>"));
        assert!(xml.contains("<ReqdColltnDt>2026-07-20</ReqdColltnDt>"));
        assert!(xml.contains("<ReqdColltnDt>2026-07-18</ReqdColltnDt>"));
        assert!(xml.contains("<NbOfTxs>3</NbOfTxs>"));
        assert!(xml.contains("<CtrlSum>150.00</CtrlSum>"));
    }

    #[test]
    fn core_and_b2b_can_coexist_in_one_file() {
        let xml = Pain008Builder::new("Test")
            .msg_id("DD-MIX")
            .add_group(
                group("Test")
                    .scheme(DirectDebitScheme::Core)
                    .add_entry(entry("M1", 1_000)),
            )
            .add_group(
                group("Test")
                    .scheme(DirectDebitScheme::B2b)
                    .add_entry(entry("M2", 2_000)),
            )
            .build()
            .unwrap();
        assert!(xml.contains("<Cd>CORE</Cd>"));
        assert!(xml.contains("<Cd>B2B</Cd>"));
    }

    #[test]
    fn creditor_scheme_id_is_emitted_per_group() {
        let xml = one_group("Test").build().unwrap();
        assert!(xml.contains("DE98ZZZ09999999999"));
        assert!(xml.contains("<Prtry>SEPA</Prtry>"));
    }

    #[test]
    fn legacy_dk_schema_uses_bic_element() {
        let xml = Pain008Builder::new("Test")
            .schema(DirectDebitSchema::DkV2_7)
            .msg_id("DD-DK")
            .add_group(
                group("Test")
                    .creditor_bic("COBADEFF".parse().unwrap())
                    .add_entry(entry("M1", 1_000)),
            )
            .build()
            .unwrap();
        assert!(xml.contains("pain.008.003.02"));
        assert!(xml.contains("<BIC>COBADEFF</BIC>"));
        assert!(!xml.contains("BICFI"));
    }

    #[test]
    fn iban_only_agents_use_othr_not_a_fake_bic() {
        let xml = one_group("Test").build().unwrap();
        assert!(xml.contains("<CdtrAgt><FinInstnId><Othr><Id>NOTPROVIDED</Id></Othr>"));
        assert!(xml.contains("<DbtrAgt><FinInstnId><Othr><Id>NOTPROVIDED</Id></Othr>"));
        assert!(!xml.contains("NOTPROVIDED</BIC"));
    }

    #[test]
    fn mandate_amendment_emits_smnda_in_its_current_position() {
        let xml = Pain008Builder::new("Test")
            .msg_id("DD-AMD")
            .add_group(group("Test").add_entry(
                entry("M1", 1_000).with_amendment(MandateAmendment::debtor_account_changed()),
            ))
            .build()
            .unwrap();
        assert!(xml.contains("<AmdmntInd>true</AmdmntInd>"));
        assert!(
            xml.contains("<OrgnlDbtrAcct><Id><Othr><Id>SMNDA</Id></Othr></Id></OrgnlDbtrAcct>")
        );
        // The pre-2016 position under OrgnlDbtrAgt must not be used.
        assert!(!xml.contains("OrgnlDbtrAgt"));
    }

    #[test]
    fn an_amendment_with_no_detail_is_rejected() {
        // Sending an amendment identical to the original earns an MD02 reject.
        assert!(matches!(
            Pain008Builder::new("Test")
                .msg_id("DD-AMD")
                .add_group(
                    group("Test")
                        .add_entry(entry("M1", 1_000).with_amendment(MandateAmendment::default()),)
                )
                .build(),
            Err(ValidationError::Empty { .. })
        ));
    }

    #[test]
    fn ultimate_creditor_cannot_be_set_at_both_levels() {
        let b = Pain008Builder::new("Test").msg_id("DD-ULT").add_group(
            group("Test")
                .ultimate_creditor(Party::new("Gruppe"))
                .add_entry(entry("M1", 100).with_ultimate_creditor(Party::new("Transaktion"))),
        );
        assert_eq!(
            b.build(),
            Err(ValidationError::ConflictingLevels { field: "UltmtCdtr" })
        );
    }

    #[test]
    fn identifiers_are_rejected_rather_than_transliterated() {
        // An identifier is the key the bank echoes back; rewriting it silently
        // would break the caller's own reconciliation.
        assert!(matches!(
            Pain008Builder::new("Test")
                .msg_id("DD-1")
                .add_group(group("Test").add_entry(DirectDebitEntry::new(
                    "MND-Ü",
                    "2024-01-01",
                    "Max",
                    nl_iban(),
                    100,
                    "E2E"
                )))
                .build(),
            Err(ValidationError::InvalidCharacter {
                field: "MndtRltdInf/MndtId",
                ch: 'Ü'
            })
        ));
    }

    #[test]
    fn empty_message_and_empty_group_are_both_rejected() {
        assert_eq!(
            Pain008Builder::new("Test").msg_id("E").build(),
            Err(ValidationError::EmptyBatch)
        );
        assert_eq!(
            Pain008Builder::new("Test")
                .msg_id("E")
                .add_group(group("Test"))
                .build(),
            Err(ValidationError::EmptyBatch)
        );
    }

    #[test]
    fn validation_rejects_bad_fields() {
        let b = || Pain008Builder::new("Test").msg_id("OK");
        assert!(matches!(
            b().add_group(group("Test").add_entry(entry("M1", 0)))
                .build(),
            Err(ValidationError::AmountOutOfRange { .. })
        ));
        assert!(matches!(
            b().add_group(
                DirectDebitGroup::new("Test", &de_iban(), ci())
                    .collection_date("2026-02-30")
                    .add_entry(entry("M1", 100))
            )
            .build(),
            Err(ValidationError::InvalidDate { .. })
        ));
        assert!(matches!(
            b().add_group(group("Test").add_entry(DirectDebitEntry::new(
                "M1",
                "01.06.2024",
                "Max",
                nl_iban(),
                100,
                "E2E"
            )))
            .build(),
            Err(ValidationError::InvalidDate {
                field: "MndtRltdInf/DtOfSgntr",
                ..
            })
        ));
    }

    #[test]
    fn totals_use_integer_arithmetic() {
        let b = Pain008Builder::new("Test").msg_id("DD").add_group(
            group("Test")
                .add_entry(entry("M1", 10))
                .add_entry(entry("M2", 20)),
        );
        assert_eq!(b.total_ct(), 30);
        assert!(b.build().unwrap().contains("<CtrlSum>0.30</CtrlSum>"));
    }

    #[test]
    fn non_sepa_characters_are_transliterated() {
        let xml = Pain008Builder::new("Müller & Söhne GmbH")
            .msg_id("DD-UML")
            .add_group(
                DirectDebitGroup::new("Müller & Söhne GmbH", &de_iban(), ci())
                    .collection_date("2026-07-20")
                    .add_entry(
                        DirectDebitEntry::new(
                            "MND-001",
                            "2024-01-01",
                            "Jörg Groß",
                            nl_iban(),
                            100,
                            "E2E-1",
                        )
                        .with_description("Abschlag für Straße 1"),
                    ),
            )
            .build()
            .unwrap();
        assert!(xml.contains("Mueller + Soehne GmbH"));
        assert!(xml.contains("Joerg Gross"));
        assert!(xml.contains("Abschlag fuer Strasse 1"));
    }

    #[test]
    fn sequence_type_parsing_and_display() {
        assert_eq!("FRST".parse::<SequenceType>().unwrap(), SequenceType::Frst);
        assert_eq!("rcur".parse::<SequenceType>().unwrap(), SequenceType::Rcur);
        assert!("INVALID".parse::<SequenceType>().is_err());
        assert_eq!(SequenceType::Ooff.to_string(), "OOFF");
        assert_eq!(DirectDebitScheme::B2b.to_string(), "B2B");
    }

    #[test]
    fn streaming_matches_the_in_memory_build() {
        let make = || one_group("Test").created_at("2026-07-19T12:00:00");
        let direct = make().build().unwrap();
        let mut buf: Vec<u8> = Vec::new();
        make().write_to(&mut buf).unwrap();
        assert_eq!(direct, String::from_utf8(buf).unwrap());
    }

    #[test]
    fn strict_charset_policy_rejects_umlauts_in_names() {
        assert!(matches!(
            one_group("Müller GmbH")
                .charset(CharsetPolicy::Strict)
                .build(),
            Err(ValidationError::InvalidCharacter { ch: 'ü', .. })
        ));
    }

    #[test]
    fn days_to_ymd_epoch() {
        assert_eq!(days_to_ymd(0), (1970, 1, 1));
    }
}
