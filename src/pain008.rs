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
//! | [`DirectDebitSchema::IsoV2`] — `pain.008.001.02` | EPC version until Nov 2023; still accepted by many banks |
//! | [`DirectDebitSchema::DkV2_7`] — `pain.008.003.02` | Legacy DK, end-of-life since Nov 2022 |
//!
//! Which one a bank requires varies by bank and by regulatory cutover date, so
//! it is a per-message choice rather than a crate-wide constant. Select it with
//! [`Pain008Builder::schema`], or parse it from configuration — the enum
//! implements [`FromStr`] over both the message identifier
//! (`"pain.008.001.02"`) and the full namespace URN.
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
//! ## Postal addresses
//!
//! `Cdtr/PstlAdr` sits on the group (it belongs to the account holder) and
//! `Dbtr/PstlAdr` on each collection. Both are optional, and both must be
//! structured or hybrid — see [`PostalAddress`] for the
//! 15 November 2026 cut-over. The legacy DK schema cannot carry one and says
//! so with [`ValidationError::UnsupportedBySchema`].
//!
//! ## References
//!
//! - ISO 20022 pain.008.001.08 / pain.008.001.02 / pain.008.003.02 schemas
//! - EPC130-08 SDD Core Customer-to-PSP Implementation Guidelines, 2025 version
//! - EPC131-08 SDD B2B Customer-to-PSP Implementation Guidelines, 2025 version
//! - EPC153-22 v2.1, Provision of Addresses under the EPC Payment Schemes
//! - Deutsche Bundesbank pain.008 implementation guide (DFÜ-Abkommen V2.7)
//!
//! ## Example
//!
//! ```rust
//! use sepa::{
//!     DirectDebitEntry, DirectDebitGroup, IsoDate, Pain008Builder, SequenceType,
//!     validate_creditor_id, validate_iban,
//! };
//! use sepa::pain008::DirectDebitScheme;
//!
//! let creditor = validate_iban("DE89370400440532013000")?;
//! let debtor   = validate_iban("NL91ABNA0417164300")?;
//! // The Creditor Identifier is mandatory for every direct debit.
//! let ci       = validate_creditor_id("DE98ZZZ09999999999")?;
//!
//! let collect = IsoDate::new(2026, 7, 20)?;
//!
//! let xml = Pain008Builder::new("Creditor GmbH")
//!     .msg_id("BATCH-2026-07-001")
//!     .add_group(
//!         DirectDebitGroup::new("Creditor GmbH", &creditor, &ci)
//!             .sequence_type(SequenceType::Rcur)
//!             .collection_date(collect)
//!             .creditor_bic("COBADEFFXXX".parse()?)
//!             .add_entry(
//!                 DirectDebitEntry::new(
//!                     "MND-00042", "2024-06-01".parse()?, "Max Mustermann",
//!                     debtor.clone(), 7_500, "R-001",
//!                 )
//!                 .with_description("Abschlag Juli 2026"),
//!             ),
//!     )
//!     // A B2B collection in the same file, as its own group.
//!     .add_group(
//!         DirectDebitGroup::new("Creditor GmbH", &creditor, &ci)
//!             .scheme(DirectDebitScheme::B2b)
//!             .collection_date(collect)
//!             .add_entry(DirectDebitEntry::new(
//!                 "MND-B2B-001", "2024-01-01".parse()?, "Corporate AG",
//!                 debtor, 50_000, "INV-001",
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

use crate::address::PostalAddress;
use crate::creditor_id::CreditorId;
use crate::date::IsoDate;
use crate::party::Party;
use crate::purpose::{CategoryPurpose, Purpose};
use crate::reference::RemittanceInfo;
use crate::validate::{
    BuildError, CharsetPolicy, Locate, Location, MAX_ID_LEN, UnknownSchema, ValidationError,
    WriteError, check_amount, check_id, check_name, truncate_chars,
};
use crate::{Bic, Iban, IsoDateTime, ct_to_eur_str};

// ── DirectDebitSchema ─────────────────────────────────────────────────────────

/// pain.008 XML schema version to emit.
///
/// Which version a bank requires varies by bank and by regulatory cutover, so
/// this is a per-message choice. [`FromStr`] accepts the message identifier
/// (`"pain.008.001.02"`) and the full namespace URN, which is what makes the
/// target version configurable rather than compiled in.
///
/// # Examples
///
/// ```
/// use sepa::pain008::DirectDebitSchema;
///
/// let from_config: DirectDebitSchema = "pain.008.001.02".parse()?;
/// assert_eq!(from_config, DirectDebitSchema::IsoV2);
/// assert_eq!(from_config.to_string(), "pain.008.001.02");
/// assert_eq!(DirectDebitSchema::default(), DirectDebitSchema::IsoV8);
/// # Ok::<(), sepa::UnknownSchema>(())
/// ```
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
    /// date/time choice, so `ReqdColltnDt` is written the same way in every
    /// version here.
    #[default]
    IsoV8,

    /// `pain.008.001.02` — the EPC version in force from 2009 to 19 Nov 2023.
    ///
    /// Superseded by [`IsoV8`](Self::IsoV8) but still the version many banks
    /// and corporate channels accept — and some still mandate. Names the agent
    /// BIC element `BIC`; otherwise structurally identical to `IsoV8` for
    /// everything this builder emits.
    IsoV2,

    /// `pain.008.003.02` — legacy Deutsche Kreditwirtschaft DK V2.7 (2013).
    ///
    /// **End-of-life** since DK Anlage 3 V3.6 (November 2022). Retained for
    /// systems still pinned to it. Names the agent BIC element `BIC`, and
    /// places the `SMNDA` amendment marker in its pre-2016 position — see
    /// [`MandateAmendment`].
    DkV2_7,
}

impl DirectDebitSchema {
    /// Every schema version this builder can emit, newest first.
    pub const ALL: &'static [Self] = &[Self::IsoV8, Self::IsoV2, Self::DkV2_7];

    /// The ISO 20022 message identifier, e.g. `"pain.008.001.08"`.
    #[must_use]
    pub const fn message_id(self) -> &'static str {
        match self {
            Self::IsoV8 => "pain.008.001.08",
            Self::IsoV2 => "pain.008.001.02",
            Self::DkV2_7 => "pain.008.003.02",
        }
    }

    /// The XML namespace URI for this schema version.
    #[must_use]
    pub const fn namespace(self) -> &'static str {
        match self {
            Self::IsoV8 => "urn:iso:std:iso:20022:tech:xsd:pain.008.001.08",
            Self::IsoV2 => "urn:iso:std:iso:20022:tech:xsd:pain.008.001.02",
            Self::DkV2_7 => "urn:iso:std:iso:20022:tech:xsd:pain.008.003.02",
        }
    }

    /// Whether this schema can carry a structured `PstlAdr`.
    ///
    /// The DK schema cannot: its `PostalAddressSEPA` type holds nothing but
    /// `Ctry` and two `AdrLine`s — precisely the unstructured form the EPC
    /// retires on 15 November 2026 — so there is no element to put a town or a
    /// street in. See [`PostalAddress`].
    #[must_use]
    pub const fn supports_postal_address(self) -> bool {
        !matches!(self, Self::DkV2_7)
    }

    /// The element name carrying an agent's BIC (`BIC` before the 2019 rename).
    #[must_use]
    const fn bic_element(self) -> &'static str {
        match self {
            Self::IsoV8 => "BICFI",
            Self::IsoV2 | Self::DkV2_7 => "BIC",
        }
    }

    /// Whether the `SMNDA` marker belongs under `OrgnlDbtrAgt` rather than
    /// `OrgnlDbtrAcct`.
    ///
    /// True only for the DK schema, whose `OrgnlDbtrAcct/Id` admits nothing but
    /// an `IBAN` — see [`MandateAmendment`] for the history.
    #[must_use]
    const fn smnda_in_original_agent(self) -> bool {
        matches!(self, Self::DkV2_7)
    }
}

impl std::fmt::Display for DirectDebitSchema {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message_id())
    }
}

impl FromStr for DirectDebitSchema {
    type Err = UnknownSchema;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let key = s.trim().to_ascii_lowercase();
        Self::ALL
            .iter()
            .copied()
            .find(|schema| key == schema.message_id() || key == schema.namespace())
            .ok_or_else(|| UnknownSchema {
                value: s.to_owned(),
                supported: "pain.008.001.08, pain.008.001.02, pain.008.003.02",
            })
    }
}

impl TryFrom<&str> for DirectDebitSchema {
    type Error = UnknownSchema;
    fn try_from(s: &str) -> Result<Self, Self::Error> {
        s.parse()
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
/// **account**".
///
/// For the ISO schemas that is an implementation-guideline change rather than a
/// schema one — both forms are structurally legal in `pain.008.001.02` — so the
/// builder emits the current form. The DK schema `pain.008.003.02` predates the
/// change and encodes the old placement in its types: `OrgnlDbtrAcct/Id` admits
/// nothing but an `IBAN`, and `SMNDA` is an enumerated value of
/// `OrgnlDbtrAgt/FinInstnId/Othr/Id`. Selecting
/// [`DirectDebitSchema::DkV2_7`] therefore moves the marker; anything else
/// would be schema-invalid.
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
    /// Mutually exclusive with `same_mandate_new_account`: both fill the single
    /// `OrgnlDbtrAcct` element, and setting both is rejected by
    /// [`validate`](Self::validate).
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
    /// amendment carrying no detail is rejected by the debtor's bank —
    /// [`ValidationError::MutuallyExclusive`] when the previous debtor account
    /// is both stated and marked `SMNDA`, or a length/character error on the
    /// individual fields.
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
        // `OrgnlDbtrAcct` occurs once; SMNDA and an explicit previous IBAN are
        // two ways of filling it. The writer used to silently prefer SMNDA and
        // drop the IBAN, which is not something a batch should discover from a
        // diff of its own output.
        if self.same_mandate_new_account && self.original_debtor_iban.is_some() {
            return Err(ValidationError::MutuallyExclusive {
                field: "AmdmntInfDtls/OrgnlDbtrAcct",
                first: "SMNDA",
                second: "OrgnlDbtrAcct/Id/IBAN",
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
    fn write_xml<W: std::fmt::Write>(
        &self,
        w: &mut W,
        charset: CharsetPolicy,
        schema: DirectDebitSchema,
    ) -> std::fmt::Result {
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
                let name = charset.render(name);
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
        // The marker's element differs by schema — see the type-level docs.
        if self.same_mandate_new_account {
            if schema.smnda_in_original_agent() {
                w.write_str(
                    "<OrgnlDbtrAgt><FinInstnId><Othr><Id>SMNDA</Id></Othr></FinInstnId></OrgnlDbtrAgt>",
                )?;
            } else {
                w.write_str("<OrgnlDbtrAcct><Id><Othr><Id>SMNDA</Id></Othr></Id></OrgnlDbtrAcct>")?;
            }
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
    /// Date the mandate was signed (`DtOfSgntr`).
    pub mandate_signed_at: IsoDate,
    /// Debtor's full name (`Dbtr/Nm`).
    pub debtor_name: String,
    /// Debtor's IBAN (validated).
    pub debtor_iban: Iban,
    /// Debtor's BIC. Uses `NOTPROVIDED` in XML when `None` (EPC allowance).
    pub debtor_bic: Option<Bic>,
    /// Debtor's postal address (`Dbtr/PstlAdr`).
    pub debtor_address: Option<PostalAddress>,
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
    ///
    /// `mandate_signed_at` is an [`IsoDate`], not a string: like the [`Iban`]
    /// beside it, the value is validated where it is constructed, so no
    /// hand-formatted date can reach a batch.
    ///
    /// # Examples
    ///
    /// ```
    /// use sepa::{DirectDebitEntry, IsoDate, validate_iban};
    ///
    /// let iban = validate_iban("NL91ABNA0417164300")?;
    /// let signed = IsoDate::new(2024, 6, 1)?;          // from your own date type
    /// let also_signed: IsoDate = "2024-06-01".parse()?; // or from stored text
    /// assert_eq!(signed, also_signed);
    ///
    /// let entry = DirectDebitEntry::new("MND-1", signed, "Max", iban, 7_500, "E2E-1");
    /// assert_eq!(entry.mandate_signed_at.year(), 2024);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn new(
        mandate_ref: impl Into<String>,
        mandate_signed_at: IsoDate,
        debtor_name: impl Into<String>,
        debtor_iban: Iban,
        amount_ct: i64,
        end_to_end_id: impl Into<String>,
    ) -> Self {
        Self {
            mandate_ref: mandate_ref.into(),
            mandate_signed_at,
            debtor_name: debtor_name.into(),
            debtor_iban,
            amount_ct,
            end_to_end_id: end_to_end_id.into(),
            debtor_bic: None,
            debtor_address: None,
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

    /// Set the debtor's postal address (`Dbtr/PstlAdr`).
    ///
    /// Optional in the SEPA schemes, but asked for by some banks and by
    /// sanction screening. See [`PostalAddress`] for the
    /// structured/hybrid rules and the 15 November 2026 cut-over.
    #[must_use]
    pub fn with_debtor_address(mut self, address: PostalAddress) -> Self {
        self.debtor_address = Some(address);
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
/// use sepa::{
///     DirectDebitEntry, DirectDebitGroup, IsoDate, SequenceType, validate_creditor_id,
///     validate_iban,
/// };
///
/// let iban = validate_iban("DE89370400440532013000")?;
/// let ci = validate_creditor_id("DE98ZZZ09999999999")?;
///
/// let first = DirectDebitGroup::new("Stadtwerke GmbH", &iban, &ci)
///     .sequence_type(SequenceType::Frst)
///     .collection_date(IsoDate::new(2026, 7, 20)?)
///     .add_entry(DirectDebitEntry::new(
///         "MND-1", "2026-06-01".parse()?, "Neu Kunde", iban.clone(), 5_000, "E2E-1",
///     ));
/// assert_eq!(first.entry_count(), 1);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct DirectDebitGroup {
    payment_info_id: Option<String>,
    // Visible to `pain007`, which copies a collection into the `OrgnlTxRef` of
    // the reversal that undoes it.
    pub(crate) creditor_name: String,
    pub(crate) creditor_iban: Iban,
    pub(crate) creditor_bic: Option<Bic>,
    creditor_address: Option<PostalAddress>,
    pub(crate) creditor_id: CreditorId,
    pub(crate) sequence_type: SequenceType,
    pub(crate) scheme: DirectDebitScheme,
    pub(crate) collection_date: IsoDate,
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
    /// one. It is borrowed, like the IBAN beside it: a collection run building
    /// one group per sequence type reuses the same creditor identity for all of
    /// them.
    pub fn new(
        creditor_name: impl Into<String>,
        creditor_iban: &Iban,
        creditor_id: &CreditorId,
    ) -> Self {
        Self {
            payment_info_id: None,
            creditor_name: creditor_name.into(),
            creditor_iban: creditor_iban.clone(),
            creditor_bic: None,
            creditor_address: None,
            creditor_id: creditor_id.clone(),
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

    /// Set the requested collection date (`ReqdColltnDt`).
    ///
    /// Defaults to five days out, the SDD Core pre-notification floor for a
    /// first or one-off collection.
    #[must_use]
    pub fn collection_date(mut self, date: IsoDate) -> Self {
        self.collection_date = date;
        self
    }

    /// The requested collection date.
    #[must_use]
    pub const fn requested_collection_date(&self) -> IsoDate {
        self.collection_date
    }

    /// Set the creditor's BIC (`CdtrAgt`).
    #[must_use]
    pub fn creditor_bic(mut self, bic: Bic) -> Self {
        self.creditor_bic = Some(bic);
        self
    }

    /// Set the creditor's postal address (`Cdtr/PstlAdr`).
    ///
    /// The address belongs to the account holder, so it lives on the group
    /// rather than on each collection. See
    /// [`PostalAddress`].
    #[must_use]
    pub fn creditor_address(mut self, address: PostalAddress) -> Self {
        self.creditor_address = Some(address);
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
    created_at: Option<IsoDateTime>,
    schema: DirectDebitSchema,
    charset: CharsetPolicy,
    groups: Vec<DirectDebitGroup>,
}

impl Pain008Builder {
    /// A new message initiated by `initiating_party`.
    pub fn new(initiating_party: impl Into<String>) -> Self {
        Self {
            initiating_party: initiating_party.into(),
            msg_id: default_msg_id("sepa"),
            created_at: None,
            schema: DirectDebitSchema::default(),
            charset: CharsetPolicy::default(),
            groups: Vec::new(),
        }
    }

    /// Set the `MsgId` (`Max35Text`).
    ///
    /// A `MsgId` is how a bank de-duplicates submissions: two files sharing one
    /// are a duplicate, and the second is rejected — or, worse, accepted and
    /// silently discarded. **Set it from your own persistent sequence.**
    ///
    /// The default is only a placeholder. It is unique within one process, so
    /// building several messages in a loop cannot collide, but it does not
    /// survive a restart and carries no meaning a bank or an auditor can use.
    #[must_use]
    pub fn msg_id(mut self, id: impl Into<String>) -> Self {
        self.msg_id = id.into();
        self
    }

    /// Pin the creation timestamp (`CreDtTm`), making output reproducible.
    #[must_use]
    pub fn created_at(mut self, timestamp: IsoDateTime) -> Self {
        self.created_at = Some(timestamp);
        self
    }

    /// Select the pain.008 schema version (default
    /// [`DirectDebitSchema::IsoV8`]).
    ///
    /// # Examples
    ///
    /// ```
    /// use sepa::pain008::DirectDebitSchema;
    /// use sepa::Pain008Builder;
    ///
    /// // A bank still on the pre-2023 version, read from configuration.
    /// let schema: DirectDebitSchema = "pain.008.001.02".parse()?;
    /// let builder = Pain008Builder::new("Stadtwerke GmbH").schema(schema);
    /// # let _ = builder;
    /// # Ok::<(), sepa::UnknownSchema>(())
    /// ```
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

    /// Refuse a postal address on a schema whose `PstlAdr` cannot hold one.
    fn check_address_supported(&self, feature: &'static str) -> Result<(), ValidationError> {
        if self.schema.supports_postal_address() {
            Ok(())
        } else {
            Err(ValidationError::UnsupportedBySchema {
                feature,
                schema: self.schema.message_id(),
            })
        }
    }

    /// Validate the message without producing XML.
    ///
    /// # Errors
    ///
    /// A [`BuildError`] naming both the broken rule ([`ValidationError`]) and
    /// the group and transaction it belongs to.
    pub fn validate(&self) -> Result<(), BuildError> {
        // `PmtInf` is 1..n at message level; `CdtTrfTxInf` / `DrctDbtTxInf` are
        // 1..n inside each group, and the loop below reports which group is
        // empty rather than blaming the message for it.
        if self.groups.is_empty() {
            return Err(BuildError::message(ValidationError::EmptyBatch));
        }
        let msg = Location::message();
        check_id("GrpHdr/MsgId", &self.msg_id).at(msg)?;
        check_name(
            "InitgPty/Nm",
            &self
                .charset
                .apply("InitgPty/Nm", &self.initiating_party)
                .at(msg)?,
        )
        .at(msg)?;

        let mut total: i64 = 0;
        let mut seen_ids = std::collections::BTreeSet::new();
        for (i, g) in self.groups.iter().enumerate() {
            let at = Location::group(i);
            if g.entries.is_empty() {
                return Err(BuildError::group(i, ValidationError::EmptyBatch));
            }
            // `PmtInfId` is what a bank echoes back in pain.002 and in a camt
            // `Btch` block, so two groups sharing one make the booking
            // unattributable — and duplicate detection may drop the second.
            let id = self.payment_info_id(i);
            check_id("PmtInf/PmtInfId", &id).at(at)?;
            if !seen_ids.insert(id.clone()) {
                return Err(BuildError::group(
                    i,
                    ValidationError::Duplicate {
                        field: "PmtInf/PmtInfId",
                        value: id,
                    },
                ));
            }
            check_name(
                "Cdtr/Nm",
                &self.charset.apply("Cdtr/Nm", &g.creditor_name).at(at)?,
            )
            .at(at)?;
            if let Some(a) = &g.creditor_address {
                self.check_address_supported("Cdtr/PstlAdr").at(at)?;
                a.validate(self.charset).at(at)?;
            }
            if let Some(p) = &g.category_purpose {
                p.validate("PmtTpInf/CtgyPurp/Cd").at(at)?;
            }
            if let Some(p) = &g.ultimate_creditor {
                p.validate("PmtInf/UltmtCdtr", self.charset).at(at)?;
            }

            for (j, e) in g.entries.iter().enumerate() {
                let at = Location::transaction(i, j);
                if g.ultimate_creditor.is_some() && e.ultimate_creditor.is_some() {
                    return Err(BuildError {
                        location: at,
                        kind: ValidationError::ConflictingLevels { field: "UltmtCdtr" },
                    });
                }
                check_id("DrctDbtTxInf/PmtId/EndToEndId", &e.end_to_end_id).at(at)?;
                check_id("MndtRltdInf/MndtId", &e.mandate_ref).at(at)?;
                check_amount("DrctDbtTxInf/InstdAmt", e.amount_ct).at(at)?;
                check_name(
                    "Dbtr/Nm",
                    &self.charset.apply("Dbtr/Nm", &e.debtor_name).at(at)?,
                )
                .at(at)?;
                if let Some(a) = &e.debtor_address {
                    self.check_address_supported("Dbtr/PstlAdr").at(at)?;
                    a.validate(self.charset).at(at)?;
                }
                if let Some(p) = &e.ultimate_creditor {
                    p.validate("DrctDbtTxInf/UltmtCdtr", self.charset).at(at)?;
                }
                if let Some(p) = &e.ultimate_debtor {
                    p.validate("DrctDbtTxInf/UltmtDbtr", self.charset).at(at)?;
                }
                if let Some(p) = &e.purpose {
                    p.validate("DrctDbtTxInf/Purp/Cd").at(at)?;
                }
                if let Some(a) = &e.amendment {
                    a.validate(self.charset).at(at)?;
                }
                if let Some(r) = &e.remittance {
                    r.validate(crate::pain001::remittance_field(r), self.charset)
                        .at(at)?;
                }
                total = total.checked_add(e.amount_ct).ok_or(BuildError {
                    location: at,
                    kind: ValidationError::ControlSumOverflow,
                })?;
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
    ///     DirectDebitEntry, DirectDebitGroup, IsoDate, Pain008Builder, SequenceType,
    ///     validate_creditor_id, validate_iban,
    /// };
    ///
    /// let iban = validate_iban("DE89370400440532013000")?;
    /// let ci = validate_creditor_id("DE98ZZZ09999999999")?;
    ///
    /// let xml = Pain008Builder::new("Stadtwerke GmbH")
    ///     .msg_id("DD-2026-07")
    ///     .add_group(
    ///         DirectDebitGroup::new("Stadtwerke GmbH", &iban, &ci)
    ///             .sequence_type(SequenceType::Frst)
    ///             .collection_date(IsoDate::new(2026, 7, 20)?)
    ///             .add_entry(DirectDebitEntry::new(
    ///                 "MND-1", "2026-06-01".parse()?, "Neu Kunde", iban.clone(), 5_000, "E2E-1",
    ///             )),
    ///     )
    ///     .add_group(
    ///         DirectDebitGroup::new("Stadtwerke GmbH", &iban, &ci)
    ///             .sequence_type(SequenceType::Rcur)
    ///             .collection_date(IsoDate::new(2026, 7, 18)?)
    ///             .add_entry(DirectDebitEntry::new(
    ///                 "MND-2", "2024-06-01".parse()?, "Alt Kunde", iban.clone(), 7_500, "E2E-2",
    ///             )),
    ///     )
    ///     .build()?;
    ///
    /// assert!(xml.contains("<SeqTp>FRST</SeqTp>"));
    /// assert!(xml.contains("<SeqTp>RCUR</SeqTp>"));
    /// assert!(xml.contains("<NbOfTxs>2</NbOfTxs>")); // group header total
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn build(&self) -> Result<String, BuildError> {
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

        let now = self.created_at.unwrap_or_else(IsoDateTime::now);
        let namespace = self.schema.namespace();
        let initiating = self.charset.render(&self.initiating_party);

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
        let creditor_name = self.charset.render(&g.creditor_name);

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
        // SDD kept `ReqdColltnDt` a bare `ISODate` in every version — unlike
        // pain.001.001.09's `ReqdExctnDt`, which became a date/time choice.
        writeln!(
            w,
            "      <ReqdColltnDt>{}</ReqdColltnDt>",
            g.collection_date
        )?;

        w.write_str("      <Cdtr><Nm>")?;
        write_escaped(w, &creditor_name)?;
        // XSD sequence inside PartyIdentification: Nm, PstlAdr, Id, …
        w.write_str("</Nm>")?;
        if let Some(address) = &g.creditor_address {
            address.write_xml(w, self.charset)?;
        }
        w.write_str("</Cdtr>\n")?;
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
        let debtor_name = self.charset.render(&e.debtor_name);

        w.write_str("    <DrctDbtTxInf>\n      <PmtId>\n        <EndToEndId>")?;
        write_escaped(w, &e.end_to_end_id)?;
        w.write_str("</EndToEndId>\n      </PmtId>\n      <InstdAmt Ccy=\"EUR\">")?;
        write_eur(w, e.amount_ct)?;
        w.write_str("</InstdAmt>\n      <DrctDbtTx>\n        <MndtRltdInf>\n          <MndtId>")?;
        write_escaped(w, &e.mandate_ref)?;
        w.write_str("</MndtId>\n          <DtOfSgntr>")?;
        write!(w, "{}", e.mandate_signed_at)?;
        w.write_str("</DtOfSgntr>\n")?;
        if let Some(amendment) = &e.amendment {
            amendment.write_xml(w, self.charset, self.schema)?;
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
        w.write_str("</Nm>")?;
        if let Some(address) = &e.debtor_address {
            address.write_xml(w, self.charset)?;
        }
        w.write_str("</Dbtr>\n      <DbtrAcct><Id><IBAN>")?;
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

/// A `MsgId` for a builder whose caller has not set one, as `<prefix>-<secs>-<n>`.
///
/// `MsgId` is the key a bank de-duplicates submissions by: two files sharing
/// one are a duplicate, and the second is rejected — or worse, dropped in
/// silence. A wall-clock second alone does not give that, because building two
/// messages in the same second is the normal case in a batch job, so a
/// process-wide counter is appended.
///
/// It is still only unique within one process. Anything that must survive a
/// restart — which, for duplicate detection at a bank, is everything — belongs
/// in `msg_id()` from the caller's own sequence.
pub(crate) fn default_msg_id(prefix: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{secs}-{n}")
}

/// Five days out — the SDD Core pre-notification floor for `FRST`/`OOFF`.
///
/// This is only the placeholder for a group whose date was never set; a real
/// collection run derives its date from its own banking calendar.
pub(crate) fn default_collection_date() -> IsoDate {
    let today = IsoDate::today();
    today.plus_days(5).unwrap_or(today)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::iban::validate_iban;
    use crate::validate::{CharsetPolicy, Location, ValidationError};

    fn d(s: &str) -> IsoDate {
        s.parse().unwrap()
    }

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
            d("2024-01-01"),
            "Max Mustermann",
            nl_iban(),
            amount_ct,
            mandate,
        )
    }
    fn group(name: &str) -> DirectDebitGroup {
        DirectDebitGroup::new(name, &de_iban(), &ci()).collection_date(d("2026-07-20"))
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
                    .collection_date(d("2026-07-20"))
                    .add_entry(entry("MND-NEW", 5_000)),
            )
            .add_group(
                group("Stadtwerke GmbH")
                    .sequence_type(SequenceType::Rcur)
                    .collection_date(d("2026-07-18"))
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
    fn postal_addresses_sit_inside_the_party_after_the_name() {
        let xml = Pain008Builder::new("Test")
            .msg_id("DD-ADR")
            .add_group(
                group("Stadtwerke GmbH")
                    .creditor_address(
                        crate::PostalAddress::new("Berlin", "DE")
                            .unwrap()
                            .street("Hauptstrasse")
                            .building_number("1"),
                    )
                    .add_entry(
                        entry("M1", 1_000).with_debtor_address(
                            crate::PostalAddress::new("Wien", "AT")
                                .unwrap()
                                .post_code("1010"),
                        ),
                    ),
            )
            .build()
            .unwrap();

        assert!(xml.contains(
            "<Cdtr><Nm>Stadtwerke GmbH</Nm><PstlAdr><StrtNm>Hauptstrasse</StrtNm>\
             <BldgNb>1</BldgNb><TwnNm>Berlin</TwnNm><Ctry>DE</Ctry></PstlAdr></Cdtr>"
        ));
        assert!(xml.contains(
            "<Dbtr><Nm>Max Mustermann</Nm><PstlAdr><PstCd>1010</PstCd>\
             <TwnNm>Wien</TwnNm><Ctry>AT</Ctry></PstlAdr></Dbtr>"
        ));
    }

    #[test]
    fn an_address_on_a_schema_without_one_is_rejected() {
        let err = Pain008Builder::new("Test")
            .schema(DirectDebitSchema::DkV2_7)
            .msg_id("DD-DK-ADR")
            .add_group(
                group("Test").add_entry(
                    entry("M1", 100)
                        .with_debtor_address(crate::PostalAddress::new("Wien", "AT").unwrap()),
                ),
            )
            .build()
            .unwrap_err();
        assert_eq!(
            err.kind,
            ValidationError::UnsupportedBySchema {
                feature: "Dbtr/PstlAdr",
                schema: "pain.008.003.02",
            }
        );
        assert_eq!(err.location, Location::transaction(0, 0));
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
                .build()
                .unwrap_err()
                .kind,
            ValidationError::Empty { .. }
        ));
    }

    #[test]
    fn an_amendment_cannot_both_state_and_suppress_the_previous_account() {
        // `OrgnlDbtrAcct` occurs once; the writer used to silently prefer
        // SMNDA and drop the IBAN the caller had supplied.
        let amendment = MandateAmendment {
            original_debtor_iban: Some(nl_iban()),
            same_mandate_new_account: true,
            ..MandateAmendment::default()
        };
        assert_eq!(
            Pain008Builder::new("Test")
                .msg_id("DD-AMD")
                .add_group(group("Test").add_entry(entry("M1", 100).with_amendment(amendment)))
                .build()
                .unwrap_err()
                .kind,
            ValidationError::MutuallyExclusive {
                field: "AmdmntInfDtls/OrgnlDbtrAcct",
                first: "SMNDA",
                second: "OrgnlDbtrAcct/Id/IBAN",
            }
        );

        // Either one on its own is fine.
        for a in [
            MandateAmendment::debtor_account_changed(),
            MandateAmendment::debtor_iban_changed(nl_iban()),
        ] {
            assert!(
                Pain008Builder::new("Test")
                    .msg_id("DD-AMD")
                    .add_group(group("Test").add_entry(entry("M1", 100).with_amendment(a)))
                    .build()
                    .is_ok()
            );
        }
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
            Err(BuildError::transaction(
                0,
                0,
                ValidationError::ConflictingLevels { field: "UltmtCdtr" }
            ))
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
                    d("2024-01-01"),
                    "Max",
                    nl_iban(),
                    100,
                    "E2E"
                )))
                .build(),
            Err(BuildError {
                location: Location {
                    group: Some(0),
                    transaction: Some(0)
                },
                kind: ValidationError::InvalidCharacter {
                    field: "MndtRltdInf/MndtId",
                    ch: 'Ü'
                }
            })
        ));
    }

    #[test]
    fn empty_message_and_empty_group_are_both_rejected() {
        assert_eq!(
            Pain008Builder::new("Test").msg_id("E").build(),
            Err(BuildError::message(ValidationError::EmptyBatch))
        );
        assert_eq!(
            Pain008Builder::new("Test")
                .msg_id("E")
                .add_group(group("Test"))
                .build(),
            Err(BuildError::group(0, ValidationError::EmptyBatch))
        );
    }

    #[test]
    fn validation_rejects_bad_fields() {
        let b = || Pain008Builder::new("Test").msg_id("OK");
        let err = b()
            .add_group(group("Test").add_entry(entry("M1", 0)))
            .build()
            .unwrap_err();
        assert!(matches!(err.kind, ValidationError::AmountOutOfRange { .. }));
        assert!(matches!(
            b().msg_id("X".repeat(36))
                .add_group(group("Test").add_entry(entry("M1", 100)))
                .build()
                .unwrap_err()
                .kind,
            ValidationError::TooLong { .. }
        ));
    }

    #[test]
    fn a_malformed_date_cannot_reach_a_batch() {
        // `ReqdColltnDt` and `DtOfSgntr` are `IsoDate`s, so an impossible date
        // is rejected where it is written, not on submission day.
        assert!("2026-02-30".parse::<IsoDate>().is_err());
        assert!("01.06.2024".parse::<IsoDate>().is_err());
    }

    #[test]
    fn errors_name_the_group_and_transaction_that_failed() {
        // A collection run is thousands of rows long; "Dbtr/Nm is too long" is
        // only actionable once it says which row.
        let err = Pain008Builder::new("Test")
            .msg_id("DD-LOC")
            .add_group(group("Test").add_entry(entry("M1", 100)))
            .add_group(
                group("Test")
                    .add_entry(entry("M2", 100))
                    .add_entry(entry("M3", 0)),
            )
            .build()
            .unwrap_err();
        assert_eq!(err.location, Location::transaction(1, 1));
        assert!(err.to_string().starts_with("PmtInf[1]/Tx[1]: "));
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
                DirectDebitGroup::new("Müller & Söhne GmbH", &de_iban(), &ci())
                    .collection_date(d("2026-07-20"))
                    .add_entry(
                        DirectDebitEntry::new(
                            "MND-001",
                            d("2024-01-01"),
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
        let make = || one_group("Test").created_at("2026-07-19T12:00:00".parse().unwrap());
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
                .build()
                .unwrap_err()
                .kind,
            ValidationError::InvalidCharacter { ch: 'ü', .. }
        ));
    }

    #[test]
    fn the_default_collection_date_is_five_days_out() {
        assert_eq!(
            default_collection_date(),
            IsoDate::today().plus_days(5).unwrap()
        );
    }

    #[test]
    fn schema_versions_round_trip_through_their_identifiers() {
        for schema in DirectDebitSchema::ALL {
            assert_eq!(
                schema.message_id().parse::<DirectDebitSchema>(),
                Ok(*schema)
            );
            assert_eq!(schema.namespace().parse::<DirectDebitSchema>(), Ok(*schema));
            assert!(schema.namespace().ends_with(schema.message_id()));
        }
        assert!("pain.008.001.99".parse::<DirectDebitSchema>().is_err());
        assert_eq!(
            "PAIN.008.001.02".parse::<DirectDebitSchema>(),
            Ok(DirectDebitSchema::IsoV2)
        );
    }

    #[test]
    fn the_epc_legacy_schema_uses_the_pre_2019_bic_element() {
        let xml = Pain008Builder::new("Test")
            .schema(DirectDebitSchema::IsoV2)
            .msg_id("DD-V2")
            .add_group(
                group("Test")
                    .creditor_bic("COBADEFF".parse().unwrap())
                    .add_entry(entry("M1", 1_000)),
            )
            .build()
            .unwrap();
        assert!(xml.contains("pain.008.001.02"));
        assert!(xml.contains("<BIC>COBADEFF</BIC>"));
        assert!(!xml.contains("BICFI"));
    }

    #[test]
    fn smnda_moves_to_the_original_agent_under_the_dk_schema() {
        // Regression: pain.008.003.02 admits nothing but an IBAN under
        // OrgnlDbtrAcct, and enumerates SMNDA under OrgnlDbtrAgt instead — so
        // the current placement is schema-invalid there.
        let build = |schema| {
            Pain008Builder::new("Test")
                .schema(schema)
                .msg_id("DD-AMD")
                .add_group(group("Test").add_entry(
                    entry("M1", 1_000).with_amendment(MandateAmendment::debtor_account_changed()),
                ))
                .build()
                .unwrap()
        };

        let dk = build(DirectDebitSchema::DkV2_7);
        assert!(dk.contains(
            "<OrgnlDbtrAgt><FinInstnId><Othr><Id>SMNDA</Id></Othr></FinInstnId></OrgnlDbtrAgt>"
        ));
        assert!(!dk.contains("OrgnlDbtrAcct"));

        for iso in [DirectDebitSchema::IsoV8, DirectDebitSchema::IsoV2] {
            let xml = build(iso);
            assert!(
                xml.contains("<OrgnlDbtrAcct><Id><Othr><Id>SMNDA</Id></Othr></Id></OrgnlDbtrAcct>"),
                "{iso} must use the post-2016 placement"
            );
            assert!(!xml.contains("OrgnlDbtrAgt"));
        }
    }
}
