//! ISO 20022 pain.001 — credit transfer initiation, for three EPC schemes.
//!
//! Builds credit-transfer XML for outgoing payments: supplier credits, refunds
//! to customers, or any IBAN-to-IBAN transfer.
//!
//! ## Three schemes, one message
//!
//! The EPC runs three credit-transfer schemes and they share this message
//! entirely — same version, same elements, same answers back. What differs is
//! a handful of coded values, and a wrong one produces a file that passes
//! `xmllint` and is rejected on ingestion. [`CreditTransferKind`] is the axis:
//!
//! | Scheme | Variant | `SvcLvl` | `LclInstrm` | `ChrgBr` |
//! |---|---|---|---|---|
//! | SCT | [`CreditTransferKind::Standard`] | `SEPA` | — | `SLEV` |
//! | SCT Inst | [`CreditTransferKind::Instant`] | `SEPA` | `INST` | `SLEV` |
//! | OCT Inst | [`CreditTransferKind::OneLegOutInstant`] | `EOLO` | `INST` | `CRED`/`DEBT`/`SHAR` |
//!
//! **OCT Inst** — One-Leg Out Instant Credit Transfer — is the euro leg of a
//! payment whose other leg leaves SEPA. It is the one scheme here that is not
//! euro-only: the amount may be ordered in the beneficiary's currency
//! ([`CreditTransferEntry::with_currency`]), and the currency the payee is to
//! receive travels in `InstrForCdtrAgt`
//! ([`CreditTransferEntry::with_non_euro_leg_currency`]). Every combination
//! the schemes forbid — `EOLO` with `SLEV`, a non-euro amount under SEPA — is
//! either unconstructible or a named error from `build()`.
//!
//! The scheme and the schema version are **separate axes**: all three schemes
//! are specified against `pain.001.001.09`, and a future ISO migration would
//! move all three together.
//!
//! ## Schema versions
//!
//! | Schema | Variant | Use |
//! |---|---|---|
//! | `pain.001.001.09` | [`CreditTransferSchema::IsoV9`] | Current SEPA version (**default**) |
//! | `pain.001.001.03` | [`CreditTransferSchema::IsoV3`] | EPC version until Nov 2023; still accepted by many banks |
//! | `pain.001.003.03` | [`CreditTransferSchema::DkV2_7`] | Legacy DK V2.7, end-of-life since Nov 2022 |
//!
//! Which one a bank requires varies by bank and by regulatory cutover date, so
//! it is a per-message choice. Select it with [`Pain001Builder::schema`], or
//! parse it from configuration — the enum implements [`FromStr`]
//! over both the message identifier (`"pain.001.001.03"`) and the namespace URN.
//!
//! ## Postal addresses
//!
//! `Dbtr/PstlAdr` sits on the group (it belongs to the account holder) and
//! `Cdtr/PstlAdr` on each transfer. Both are optional, and both must be
//! structured or hybrid — see [`PostalAddress`] for which forms SEPA accepts.
//! The legacy DK schema cannot carry one and says so with
//! [`ValidationError::UnsupportedBySchema`].
//!
//! ## References
//!
//! - ISO 20022 pain.001.001.03 / pain.001.001.09 / pain.001.003.03 schemas
//! - EPC SEPA Credit Transfer Rulebook (SCT), 2025 version
//! - EPC SEPA Instant Credit Transfer Rulebook (SCT Inst), 2025 version
//! - EPC158-22 One-Leg Out Instant Credit Transfer Scheme Rulebook, 2025 v1.1
//! - EPC250-22 OCT Inst Customer-to-PSP Implementation Guidelines, 2025 v1.0
//! - EPC153-22 v2.1, Provision of Addresses under the EPC Payment Schemes
//! - Deutsche Kreditwirtschaft DFÜ-Abkommen V2.7
//!
//! ## Example
//!
//! ```rust
//! use sepa::{CreditTransferEntry, CreditTransferGroup, IsoDate, Pain001Builder, validate_iban};
//! use sepa::pain001::CreditTransferKind;
//!
//! let debtor    = validate_iban("DE89370400440532013000")?;
//! let creditor  = validate_iban("NL91ABNA0417164300")?;
//! let creditor2 = validate_iban("NL91ABNA0417164300")?;
//! let execute  = IsoDate::new(2026, 7, 20)?;
//!
//! let xml = Pain001Builder::new("Acme GmbH", "CT-2026-07-001")
//!     .add_group(
//!         CreditTransferGroup::new("Acme GmbH", &debtor, execute)
//!             .add_entry(
//!                 CreditTransferEntry::new("Max Mustermann", creditor.clone(), 12_000, "REFUND")
//!                     .with_description("Erstattung 2025"),
//!             ),
//!     )
//!     .build()?;
//!
//! assert!(xml.contains("<InstdAmt Ccy=\"EUR\">120.00</InstdAmt>"));
//! // pain.001.001.09 wraps the execution date in a <Dt> choice child.
//! assert!(xml.contains("<ReqdExctnDt><Dt>2026-07-20</Dt></ReqdExctnDt>"));
//!
//! // SCT Instant is a property of the group.
//! let inst = Pain001Builder::new("Acme GmbH", "CT-INST-001")
//!     .add_group(
//!         CreditTransferGroup::new("Acme GmbH", &debtor, execute)
//!             .kind(CreditTransferKind::Instant)
//!             .add_entry(CreditTransferEntry::new("Max", creditor, 5_000, "INST-001")),
//!     )
//!     .build()?;
//! assert!(inst.contains("<Cd>INST</Cd>"));
//!
//! // OCT Inst: the euro leg of a payment leaving SEPA, paying out in USD.
//! let oct = Pain001Builder::new("Acme GmbH", "OCT-2026-001")
//!     .add_group(
//!         CreditTransferGroup::new("Acme GmbH", &debtor, execute)
//!             .kind(CreditTransferKind::OneLegOutInstant)
//!             .add_entry(
//!                 CreditTransferEntry::new("Payee", creditor2, 5_000, "OCT-001")
//!                     .with_currency("USD".parse()?)
//!                     .with_non_euro_leg_currency("USD".parse()?),
//!             ),
//!     )
//!     .build()?;
//! assert!(oct.contains("<SvcLvl><Cd>EOLO</Cd></SvcLvl>"));
//! assert!(oct.contains("<ChrgBr>SHAR</ChrgBr>"));
//! assert!(oct.contains("<InstrForCdtrAgt><InstrInf>USD</InstrInf></InstrForCdtrAgt>"));
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use std::str::FromStr;

use crate::address::PostalAddress;
use crate::bic::BicPattern;
use crate::date::IsoDate;
use crate::party::Party;
use crate::purpose::{CategoryPurpose, Purpose};
use crate::reference::RemittanceInfo;
use crate::validate::{
    BuildError, CharsetPolicy, Locate, Location, MAX_ID_LEN, UnknownSchema, ValidationError,
    WriteError, accumulate_control_sum, check_amount, check_id, check_name, truncate_chars,
};
use crate::{Bic, Currency, Iban, IsoDateTime, ct_to_eur_str};

// ── Schema version ────────────────────────────────────────────────────────────

/// pain.001 XML schema version to emit.
///
/// The versions differ in ways that matter to the wire format, not just the
/// namespace — see [`CreditTransferSchema::namespace`] and the notes on each
/// variant. [`FromStr`] accepts the message identifier (`"pain.001.001.03"`)
/// and the full namespace URN, which is what makes the target version
/// configurable rather than compiled in.
///
/// # Examples
///
/// ```
/// use sepa::pain001::CreditTransferSchema;
///
/// let from_config: CreditTransferSchema = "pain.001.001.03".parse()?;
/// assert_eq!(from_config, CreditTransferSchema::IsoV3);
/// assert_eq!(from_config.to_string(), "pain.001.001.03");
/// # Ok::<(), sepa::UnknownSchema>(())
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CreditTransferSchema {
    /// `pain.001.001.09` — the current SEPA version (**default**).
    ///
    /// Mandated by the EPC 2023 SCT Rulebook with effect from 19 November 2023
    /// and carried unchanged into the 2025 rulebooks.
    ///
    /// Wire-format specifics versus the older versions:
    /// - `ReqdExctnDt` is a `DateAndDateTime2Choice`, so the date is wrapped:
    ///   `<ReqdExctnDt><Dt>2026-07-20</Dt></ReqdExctnDt>`
    /// - the agent BIC element is named `BICFI`, not `BIC`
    #[default]
    IsoV9,

    /// `pain.001.001.03` — the EPC version in force from 2009 to 19 Nov 2023.
    ///
    /// Superseded by [`IsoV9`](Self::IsoV9) but still the version many banks
    /// and corporate channels accept, and the version the pre-2023 SCT Inst
    /// rulebooks used. Emits a bare `<ReqdExctnDt>2026-07-20</ReqdExctnDt>` and
    /// names the agent BIC element `BIC`.
    IsoV3,

    /// `pain.001.003.03` — legacy Deutsche Kreditwirtschaft DK V2.7 (2013).
    ///
    /// **End-of-life.** Superseded in November 2022 by DK Anlage 3 V3.6 and
    /// absent from every DK format specification since. Retained only for
    /// systems still pinned to it; new integrations should use
    /// [`IsoV9`](Self::IsoV9).
    ///
    /// Emits a bare `<ReqdExctnDt>2026-07-20</ReqdExctnDt>` and names the agent
    /// BIC element `BIC`. Its `PmtTpInf` has no `LclInstrm` element at all, so
    /// it cannot carry [`CreditTransferKind::Instant`].
    DkV2_7,
}

impl CreditTransferSchema {
    /// Every schema version this builder can emit, newest first.
    pub const ALL: &'static [Self] = &[Self::IsoV9, Self::IsoV3, Self::DkV2_7];

    /// The ISO 20022 message identifier, e.g. `"pain.001.001.09"`.
    #[must_use]
    pub const fn message_id(self) -> &'static str {
        match self {
            Self::IsoV9 => "pain.001.001.09",
            Self::IsoV3 => "pain.001.001.03",
            Self::DkV2_7 => "pain.001.003.03",
        }
    }

    /// The XML namespace URI for this schema version.
    #[must_use]
    pub const fn namespace(self) -> &'static str {
        match self {
            Self::IsoV9 => "urn:iso:std:iso:20022:tech:xsd:pain.001.001.09",
            Self::IsoV3 => "urn:iso:std:iso:20022:tech:xsd:pain.001.001.03",
            Self::DkV2_7 => "urn:iso:std:iso:20022:tech:xsd:pain.001.003.03",
        }
    }

    /// Whether this schema can carry `PmtTpInf/LclInstrm`.
    ///
    /// The DK schema cannot: `PaymentTypeInformationSCT1` is a sequence of
    /// `InstrPrty`, `SvcLvl` and `CtgyPurp` with no local-instrument element,
    /// so an `INST` batch built against it is schema-invalid rather than merely
    /// unusual.
    #[must_use]
    pub const fn supports_local_instrument(self) -> bool {
        !matches!(self, Self::DkV2_7)
    }

    /// Whether this schema can carry a structured `PstlAdr`.
    ///
    /// The DK schema cannot: its `PostalAddressSEPA` type holds nothing but
    /// `Ctry` and two `AdrLine`s — precisely the free-text-only form the EPC is
    /// retiring — so there is no element to put a town or a street in. See
    /// [`PostalAddress`].
    #[must_use]
    pub const fn supports_postal_address(self) -> bool {
        !matches!(self, Self::DkV2_7)
    }

    /// Which character pattern this schema constrains agent BICs to.
    ///
    /// A BIC that only the wider pattern admits — one with a digit in its
    /// business party prefix, legal since ISO 9362:2022 — is rejected by
    /// `build()` here rather than written into a schema that cannot hold it.
    /// See [`BicPattern`].
    #[must_use]
    pub const fn bic_pattern(self) -> BicPattern {
        match self {
            Self::IsoV9 => BicPattern::Alphanumeric,
            Self::IsoV3 | Self::DkV2_7 => BicPattern::LettersOnly,
        }
    }

    /// The element name carrying an agent's BIC.
    ///
    /// ISO renamed `BIC` to `BICFI` in the 2019 maintenance release. That is a
    /// *separate* fact from [`bic_pattern`](Self::bic_pattern) — `camt.055`
    /// pairs the new name with the old pattern — so the two are read from the
    /// schema independently rather than derived from one another.
    #[must_use]
    const fn bic_element(self) -> &'static str {
        match self {
            Self::IsoV9 => "BICFI",
            Self::IsoV3 | Self::DkV2_7 => "BIC",
        }
    }

    /// Whether `ReqdExctnDt` is a date/time choice rather than a bare date.
    ///
    /// Only `pain.001.001.09` types it as `DateAndDateTime2Choice`. That is
    /// both why the date needs a `<Dt>` wrapper there and why a timed execution
    /// ([`ExecutionMoment::At`]) is expressible only on that version.
    #[must_use]
    pub const fn supports_execution_time(self) -> bool {
        matches!(self, Self::IsoV9)
    }
}

impl std::fmt::Display for CreditTransferSchema {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message_id())
    }
}

impl FromStr for CreditTransferSchema {
    type Err = UnknownSchema;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let key = s.trim().to_ascii_lowercase();
        Self::ALL
            .iter()
            .copied()
            .find(|schema| key == schema.message_id() || key == schema.namespace())
            .ok_or_else(|| UnknownSchema {
                value: s.to_owned(),
                supported: "pain.001.001.09, pain.001.001.03, pain.001.003.03",
            })
    }
}

impl TryFrom<&str> for CreditTransferSchema {
    type Error = UnknownSchema;
    fn try_from(s: &str) -> Result<Self, Self::Error> {
        s.parse()
    }
}

// ── Scheme ────────────────────────────────────────────────────────────────────

/// Which EPC payment scheme a credit-transfer group is executed under.
///
/// The EPC runs three credit-transfer schemes, and they differ in their
/// *rules*, not in their messages: all three are carried by `pain.001.001.09`,
/// answered by `pain.002.001.10` and notified by `camt.054.001.08`. What
/// changes is a handful of coded elements, and getting one of them wrong
/// produces a file that validates against the XSD and is rejected on
/// ingestion.
///
/// | | `SvcLvl/Cd` | `LclInstrm/Cd` | `ChrgBr` |
/// |---|---|---|---|
/// | [`Standard`](Self::Standard) — SCT | `SEPA` | *absent* | `SLEV` |
/// | [`Instant`](Self::Instant) — SCT Inst | `SEPA` | `INST` | `SLEV` |
/// | [`OneLegOutInstant`](Self::OneLegOutInstant) — OCT Inst | `EOLO` | `INST` | `CRED`, `DEBT` or `SHAR` |
///
/// One enum rather than two orthogonal fields, because the combinations are
/// not orthogonal: `EOLO` without `INST` is not a scheme, and neither is
/// `EOLO` with `SLEV`. A type that can hold only the three real answers
/// retires the checks for the rest.
///
/// # Examples
///
/// ```
/// use sepa::pain001::CreditTransferKind;
///
/// assert_eq!(CreditTransferKind::Standard.service_level(), "SEPA");
/// assert_eq!(CreditTransferKind::OneLegOutInstant.service_level(), "EOLO");
/// assert_eq!(CreditTransferKind::Instant.local_instrument(), Some("INST"));
/// assert_eq!(CreditTransferKind::Standard.local_instrument(), None);
/// assert!(CreditTransferKind::OneLegOutInstant.is_instant());
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum CreditTransferKind {
    /// **SCT** — an ordinary SEPA Credit Transfer (default).
    #[default]
    Standard,

    /// **SCT Inst** — SEPA Instant Credit Transfer, 10-second settlement.
    ///
    /// Adds `<LclInstrm><Cd>INST</Cd></LclInstrm>`, so it needs a schema that
    /// has the element: `pain.001.001.09` or `pain.001.001.03`. Regulation
    /// (EU) 2024/886 mandates PSP support for it in the euro area.
    Instant,

    /// **OCT Inst** — One-Leg Out Instant Credit Transfer, the euro leg of a
    /// payment whose other leg leaves SEPA.
    ///
    /// The scheme is implied by `SvcLvl/Cd=EOLO` together with
    /// `LclInstrm/Cd=INST`; there is no `OCTI` code anywhere, which is the
    /// usual shape of a rule no XSD can express. Three further differences are
    /// enforced by `build()`:
    ///
    /// - `ChrgBr` must be `CRED`, `DEBT` or `SHAR` — **never** `SLEV`, which
    ///   is what the four SEPA schemes require.
    /// - the instructed amount may be ordered in a non-euro currency
    ///   ([`CreditTransferEntry::with_currency`]), which no other scheme here
    ///   permits.
    /// - the currency the payee is to receive travels in `InstrForCdtrAgt`
    ///   ([`CreditTransferEntry::with_non_euro_leg_currency`]).
    ///
    /// Specified against the 2019 message version only, so
    /// [`CreditTransferSchema::IsoV9`] is the one schema that can carry it.
    ///
    /// Source: EPC250-22 *OCT Inst Customer-to-PSP Implementation Guidelines*,
    /// 2025 v1.0, effective 5 October 2025.
    OneLegOutInstant,
}

impl CreditTransferKind {
    /// The `PmtTpInf/SvcLvl/Cd` this scheme is identified by.
    #[inline]
    #[must_use]
    pub const fn service_level(self) -> &'static str {
        match self {
            Self::Standard | Self::Instant => "SEPA",
            Self::OneLegOutInstant => "EOLO",
        }
    }

    /// The `PmtTpInf/LclInstrm/Cd`, or `None` where the scheme has none.
    #[inline]
    #[must_use]
    pub const fn local_instrument(self) -> Option<&'static str> {
        match self {
            Self::Standard => None,
            Self::Instant | Self::OneLegOutInstant => Some("INST"),
        }
    }

    /// Whether this scheme settles instantly.
    ///
    /// A timed execution (`ReqdExctnDt/DtTm`) is only meaningful for a scheme
    /// that settles at a moment rather than during a day, so this is what
    /// gates it.
    #[inline]
    #[must_use]
    pub const fn is_instant(self) -> bool {
        matches!(self, Self::Instant | Self::OneLegOutInstant)
    }

    /// The `ChrgBr` used when the caller does not choose one.
    ///
    /// `SLEV` for the SEPA schemes, which mandate it. `SHAR` for OCT Inst,
    /// which forbids `SLEV` and lists `SHAR` among the three it allows.
    #[inline]
    #[must_use]
    pub const fn default_charge_bearer(self) -> ChargeBearer {
        match self {
            Self::Standard | Self::Instant => ChargeBearer::Slev,
            Self::OneLegOutInstant => ChargeBearer::Shar,
        }
    }

    /// Whether `bearer` is allowed under this scheme.
    #[inline]
    #[must_use]
    pub const fn allows_charge_bearer(self, bearer: ChargeBearer) -> bool {
        match self {
            Self::Standard | Self::Instant => matches!(bearer, ChargeBearer::Slev),
            Self::OneLegOutInstant => !matches!(bearer, ChargeBearer::Slev),
        }
    }

    /// Whether an amount may be ordered in a currency other than the euro.
    ///
    /// True for OCT Inst alone: its far leg leaves the euro area by design.
    #[inline]
    #[must_use]
    pub const fn allows_non_euro_amount(self) -> bool {
        matches!(self, Self::OneLegOutInstant)
    }
}

// ── ChargeBearer ──────────────────────────────────────────────────────────────

/// Who bears the transaction charges (`ChrgBr`).
///
/// The SEPA schemes permit exactly one value, `SLEV` — "following service
/// level" — and this crate emitted it as a literal until OCT Inst arrived,
/// which forbids it and allows the other three. See
/// [`CreditTransferKind::allows_charge_bearer`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ChargeBearer {
    /// `SLEV` — charges follow the service level. The only value the four
    /// SEPA schemes permit, and the default for them.
    #[default]
    Slev,
    /// `CRED` — all charges borne by the creditor. OCT Inst only.
    Cred,
    /// `DEBT` — all charges borne by the debtor. OCT Inst only.
    Debt,
    /// `SHAR` — sender-side charges to the debtor, receiver-side to the
    /// creditor. OCT Inst only, and its default.
    Shar,
}

impl ChargeBearer {
    /// The four-letter `ChargeBearerType1Code`.
    #[inline]
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::Slev => "SLEV",
            Self::Cred => "CRED",
            Self::Debt => "DEBT",
            Self::Shar => "SHAR",
        }
    }
}

impl std::fmt::Display for ChargeBearer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_code())
    }
}

// ── ExecutionMoment ───────────────────────────────────────────────────────────

/// When a payment group is to be executed (`ReqdExctnDt`).
///
/// `pain.001.001.09` types this as a `DateAndDateTime2Choice`, so it is either
/// a bare day or a day *and* a time. The timed form is what the Deutsche
/// Kreditwirtschaft calls a **terminierte Echtzeitüberweisung** — a scheduled
/// SEPA Instant transfer that is to leave the account at a stated moment rather
/// than at some point during the day.
///
/// The older schemas type `ReqdExctnDt` as a bare `ISODate` and have no choice
/// wrapper at all, so [`At`](Self::At) on those is rejected with
/// [`ValidationError::UnsupportedBySchema`] rather than silently downgraded to
/// the date — a payment that was meant to leave at 11:00 must not quietly
/// become "some time that day".
///
/// ## Two rules no XSD can express
///
/// The DK validation subset annotates `DtTm` *"Only allowed for `SCTinst`"*, with
/// the usage rule *"Only UTC time format or local time with UTC offset format
/// can be used"*. Both are prose in the schema, so `xmllint` accepts a file
/// that breaks either and the bank rejects it on ingestion. `build()` returns
/// [`ValidationError::Requires`] instead:
///
/// - [`At`](Self::At) needs [`CreditTransferKind::Instant`] on the same group. An
///   ordinary SCT settles some time during the banking day, so a time of day on
///   one instructs nothing.
/// - The timestamp needs a UTC offset — `2026-07-20T11:00:00Z` or
///   `2026-07-20T13:00:00+02:00`. See [`IsoDateTime::in_utc`].
///
/// # Examples
///
/// ```
/// use sepa::{CreditTransferGroup, IsoDate, IsoDateTime, validate_iban};
/// use sepa::pain001::CreditTransferKind;
///
/// let iban = validate_iban("DE89370400440532013000")?;
///
/// // The ordinary case — a day.
/// let plain = CreditTransferGroup::new("Acme", &iban, IsoDate::new(2026, 7, 20)?);
///
/// // A scheduled instant transfer — a day and a time.
/// let timed = CreditTransferGroup::new("Acme", &iban, "2026-07-20T11:00:00Z".parse::<IsoDateTime>()?)
///     .kind(CreditTransferKind::Instant);
/// # let _ = (plain, timed);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ExecutionMoment {
    /// `<ReqdExctnDt><Dt>2026-07-20</Dt></ReqdExctnDt>` — a banking day.
    On(IsoDate),
    /// `<ReqdExctnDt><DtTm>2026-07-20T11:00:00Z</DtTm></ReqdExctnDt>` — a moment.
    At(IsoDateTime),
}

impl ExecutionMoment {
    /// The day this group executes on, whichever form was given.
    #[must_use]
    pub const fn date(self) -> IsoDate {
        match self {
            Self::On(date) => date,
            Self::At(moment) => moment.date(),
        }
    }

    /// Whether this carries a time of day, and so needs the `DtTm` branch.
    #[must_use]
    pub const fn is_timed(self) -> bool {
        matches!(self, Self::At(_))
    }
}

impl From<IsoDate> for ExecutionMoment {
    fn from(date: IsoDate) -> Self {
        Self::On(date)
    }
}

impl From<IsoDateTime> for ExecutionMoment {
    fn from(moment: IsoDateTime) -> Self {
        Self::At(moment)
    }
}

/// The ISO 20022 element path a remittance violation should name.
///
/// `Ustrd` and `Strd` are alternatives inside one `RmtInf`, and an error that
/// says which branch failed is what points an operator at the right field.
pub(crate) fn remittance_field(remittance: &RemittanceInfo) -> &'static str {
    match remittance {
        RemittanceInfo::Unstructured(_) => "RmtInf/Ustrd",
        _ => "RmtInf/Strd",
    }
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
    /// Beneficiary BIC (`CdtrAgt`).
    ///
    /// When `None` the whole `CdtrAgt` element is **omitted**, which is what
    /// the EPC guidelines say to do for an unknown creditor agent —
    /// `pain.001.003.03` makes it structural, since its `CdtrAgt` type has a
    /// mandatory BIC and no `Othr` branch to put a placeholder in. The
    /// `NOTPROVIDED` placeholder applies to `DbtrAgt`, which is mandatory.
    pub creditor_bic: Option<Bic>,
    /// Beneficiary postal address (`Cdtr/PstlAdr`).
    pub creditor_address: Option<PostalAddress>,
    /// Payment amount in **minor units** (1/100 of [`currency`](Self::currency)).
    /// Must be positive.
    pub amount_ct: i64,
    /// Currency of [`amount_ct`](Self::amount_ct) — `None` means euro.
    ///
    /// Only [`CreditTransferKind::OneLegOutInstant`] may set this to anything
    /// but the euro; under the four SEPA schemes a non-euro value is a
    /// [`ValidationError::Requires`] at `build()`. The OCT Inst guidelines cap
    /// the fractional part at two digits whatever the currency, which is what
    /// lets one `i64` of minor units serve every case.
    pub currency: Option<Currency>,
    /// The currency the payee is to receive on the non-euro leg (AT-T020).
    ///
    /// Emitted as `InstrForCdtrAgt/InstrInf`, which is where EPC250-22 puts
    /// it — there is no dedicated element. OCT Inst only.
    pub non_euro_leg_currency: Option<Currency>,
    /// Unique end-to-end reference (`EndToEndId`) visible on beneficiary's statement.
    pub end_to_end_id: String,
    /// Remittance information (`RmtInf`) — free text or a structured reference.
    pub remittance: Option<RemittanceInfo>,
    /// Ultimate debtor (`UltmtDbtr`) — who the money is really from.
    pub ultimate_debtor: Option<Party>,
    /// Ultimate creditor (`UltmtCdtr`) — who the money is really for.
    pub ultimate_creditor: Option<Party>,
    /// Purpose code (`Purp/Cd`), informational.
    pub purpose: Option<Purpose>,
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
            currency: None,
            non_euro_leg_currency: None,
            creditor_bic: None,
            creditor_address: None,
            remittance: None,
            ultimate_debtor: None,
            ultimate_creditor: None,
            purpose: None,
        }
    }

    /// Order the amount in a currency other than the euro (OCT Inst only).
    ///
    /// `amount_ct` is then that currency's minor units rather than euro cents.
    /// EPC250-22 caps the fractional part of `InstdAmt` at two digits for
    /// every currency, so the representation does not change.
    ///
    /// Setting this under any scheme but
    /// [`CreditTransferKind::OneLegOutInstant`] is rejected by `build()`.
    #[must_use]
    pub fn with_currency(mut self, currency: Currency) -> Self {
        self.currency = Some(currency);
        self
    }

    /// State the currency the payee is to receive on the non-euro leg
    /// (AT-T020, OCT Inst only).
    ///
    /// Travels in `InstrForCdtrAgt/InstrInf` because that is where EPC250-22
    /// puts it. This is a *different* question from
    /// [`with_currency`](Self::with_currency): that one says what the payer
    /// ordered, this one says what the beneficiary should get.
    #[must_use]
    pub fn with_non_euro_leg_currency(mut self, currency: Currency) -> Self {
        self.non_euro_leg_currency = Some(currency);
        self
    }

    /// The currency of this entry's amount — the euro unless one was set.
    #[must_use]
    pub fn effective_currency(&self) -> Currency {
        self.currency.unwrap_or(Currency::EUR)
    }

    /// Set the beneficiary's BIC (optional).
    #[must_use]
    pub fn with_bic(mut self, bic: Bic) -> Self {
        self.creditor_bic = Some(bic);
        self
    }

    /// Set the beneficiary's postal address (`Cdtr/PstlAdr`).
    ///
    /// Optional in the SEPA schemes, but asked for by some banks and by
    /// sanction screening. See [`PostalAddress`] for the structured and hybrid
    /// rules.
    #[must_use]
    pub fn with_creditor_address(mut self, address: PostalAddress) -> Self {
        self.creditor_address = Some(address);
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

    /// Set the ultimate debtor (`UltmtDbtr`) — who the money is really from,
    /// when that differs from the account holder.
    #[must_use]
    pub fn with_ultimate_debtor(mut self, party: impl Into<Party>) -> Self {
        self.ultimate_debtor = Some(party.into());
        self
    }

    /// Set the ultimate creditor (`UltmtCdtr`) — who the money is really for.
    ///
    /// pain.001 allows this only at transaction level; the element does not
    /// exist at payment-information level.
    #[must_use]
    pub fn with_ultimate_creditor(mut self, party: impl Into<Party>) -> Self {
        self.ultimate_creditor = Some(party.into());
        self
    }

    /// Set the purpose code (`Purp/Cd`).
    ///
    /// Informational — see [`Purpose`] for why this is not [`CategoryPurpose`].
    ///
    /// [`CategoryPurpose`]: crate::CategoryPurpose
    #[must_use]
    pub fn with_purpose(mut self, purpose: Purpose) -> Self {
        self.purpose = Some(purpose);
        self
    }
}

// ── CreditTransferGroup ───────────────────────────────────────────────────────

/// One `PmtInf` block — a set of transfers sharing a debtor account, an
/// execution date and a local instrument.
///
/// A pain.001 message may carry several of these. That is the only way to put
/// transfers with **different execution dates**, different debtor accounts or a
/// mix of ordinary and instant transfers into a single file, rather than
/// submitting several files to the bank.
///
/// ## Examples
///
/// ```
/// use sepa::{CreditTransferEntry, CreditTransferGroup, IsoDate, validate_iban};
///
/// let iban = validate_iban("DE89370400440532013000")?;
/// let group = CreditTransferGroup::new("Acme GmbH", &iban, IsoDate::new(2026, 7, 20)?)
///     .add_entry(CreditTransferEntry::new("Supplier AG", iban.clone(), 12_000, "E2E-1"));
/// assert_eq!(group.entry_count(), 1);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct CreditTransferGroup {
    payment_info_id: Option<String>,
    debtor_name: String,
    debtor_iban: Iban,
    debtor_bic: Option<Bic>,
    debtor_address: Option<PostalAddress>,
    execution: ExecutionMoment,
    kind: CreditTransferKind,
    charge_bearer: Option<ChargeBearer>,
    batch_booking: Option<bool>,
    category_purpose: Option<CategoryPurpose>,
    ultimate_debtor: Option<Party>,
    entries: Vec<CreditTransferEntry>,
}

impl CreditTransferGroup {
    /// A new payment group drawn on `debtor_iban`, executing at `execution`.
    ///
    /// `execution` takes an [`IsoDate`] for an ordinary transfer and an
    /// [`IsoDateTime`] for the DK's *terminierte Echtzeitüberweisung* — see
    /// [`ExecutionMoment`], which both convert into.
    ///
    /// It is a required argument rather than a defaulted field on purpose:
    /// `ReqdExctnDt` is when money leaves an account, and a default for it is a
    /// value nobody chose. Deriving one from the system clock also made a batch
    /// depend on which machine built it and on the wall-clock second it ran.
    pub fn new(
        debtor_name: impl Into<String>,
        debtor_iban: &Iban,
        execution: impl Into<ExecutionMoment>,
    ) -> Self {
        Self {
            payment_info_id: None,
            debtor_name: debtor_name.into(),
            debtor_iban: debtor_iban.clone(),
            debtor_bic: None,
            debtor_address: None,
            execution: execution.into(),
            kind: CreditTransferKind::Standard,
            charge_bearer: None,
            batch_booking: None,
            category_purpose: None,
            ultimate_debtor: None,
            entries: Vec::new(),
        }
    }

    /// Override the `PmtInfId`.
    ///
    /// Defaults to the message's `MsgId` for a single group, and to
    /// `MsgId-<n>` when there are several — truncated if needed to stay inside
    /// the 35-character limit.
    #[must_use]
    pub fn payment_info_id(mut self, id: impl Into<String>) -> Self {
        self.payment_info_id = Some(id.into());
        self
    }

    /// Which EPC scheme this group is executed under.
    #[must_use]
    pub const fn scheme(&self) -> CreditTransferKind {
        self.kind
    }

    /// When this group is to execute, in whichever form it was given.
    #[must_use]
    pub const fn execution(&self) -> ExecutionMoment {
        self.execution
    }

    /// The day this group is to execute on, whether or not a time was set.
    #[must_use]
    pub const fn requested_execution_date(&self) -> IsoDate {
        self.execution.date()
    }

    /// Set the debtor's BIC (`DbtrAgt`).
    #[must_use]
    pub fn debtor_bic(mut self, bic: Bic) -> Self {
        self.debtor_bic = Some(bic);
        self
    }

    /// Set the debtor's postal address (`Dbtr/PstlAdr`).
    ///
    /// The address belongs to the account holder, so it lives on the group
    /// rather than on each transfer. See
    /// [`PostalAddress`].
    #[must_use]
    pub fn debtor_address(mut self, address: PostalAddress) -> Self {
        self.debtor_address = Some(address);
        self
    }

    /// Set which EPC scheme this group is executed under.
    ///
    /// Defaults to [`CreditTransferKind::Standard`]. The scheme decides
    /// `SvcLvl`, `LclInstrm` and which `ChrgBr` values are legal; the schema
    /// version is chosen once for the whole message and is a separate axis —
    /// see [`Pain001Builder::schema`].
    ///
    /// # Examples
    ///
    /// ```
    /// use sepa::pain001::{CreditTransferGroup, CreditTransferKind};
    /// # use sepa::{IsoDate, validate_iban};
    /// # let iban = validate_iban("DE89370400440532013000")?;
    /// # let day = IsoDate::new(2026, 7, 20)?;
    /// let g = CreditTransferGroup::new("Acme", &iban, day)
    ///     .kind(CreditTransferKind::Instant);
    /// assert_eq!(g.scheme(), CreditTransferKind::Instant);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    #[must_use]
    pub fn kind(mut self, kind: CreditTransferKind) -> Self {
        self.kind = kind;
        self
    }

    /// Override `ChrgBr`.
    ///
    /// Defaults to [`CreditTransferKind::default_charge_bearer`] — `SLEV` for
    /// the SEPA schemes, `SHAR` for OCT Inst. A value the scheme does not
    /// allow is a [`ValidationError::ChargeBearerNotAllowed`] at `build()`
    /// rather than a file the bank answers.
    #[must_use]
    pub fn charge_bearer(mut self, bearer: ChargeBearer) -> Self {
        self.charge_bearer = Some(bearer);
        self
    }

    /// The `ChrgBr` this group will emit.
    #[must_use]
    pub const fn effective_charge_bearer(&self) -> ChargeBearer {
        match self.charge_bearer {
            Some(b) => b,
            None => self.kind.default_charge_bearer(),
        }
    }

    /// Request batch booking (`BtchBookg`).
    ///
    /// `true` asks for one aggregate entry on the statement, `false` for one
    /// entry per transaction. Omitted by default, which defers to the agreement
    /// with the bank — there is no scheme-wide default, and German banks treat
    /// an absent value as `true`. `false` takes effect only where a
    /// single-entry agreement is in place.
    #[must_use]
    pub fn batch_booking(mut self, batch: bool) -> Self {
        self.batch_booking = Some(batch);
        self
    }

    /// Set the category purpose (`PmtTpInf/CtgyPurp`) for this group.
    ///
    /// Unlike a [`Purpose`], which is information for the counterparty, a
    /// category purpose may trigger special handling by the banks. ISO permits
    /// it at group *or* transaction level and the EPC allows only one of the
    /// two; this crate emits it at group level only, so the conflict is not
    /// expressible.
    #[must_use]
    pub fn category_purpose(mut self, purpose: CategoryPurpose) -> Self {
        self.category_purpose = Some(purpose);
        self
    }

    /// Set the ultimate debtor for the whole group (`PmtInf/UltmtDbtr`).
    ///
    /// Mutually exclusive with the per-entry ultimate debtor: the DK rules
    /// require one level or the other, never both.
    #[must_use]
    pub fn ultimate_debtor(mut self, party: impl Into<Party>) -> Self {
        self.ultimate_debtor = Some(party.into());
        self
    }

    /// Add a credit transfer to this group.
    #[must_use]
    pub fn add_entry(mut self, entry: CreditTransferEntry) -> Self {
        self.entries.push(entry);
        self
    }

    /// Add several credit transfers.
    #[must_use]
    pub fn add_entries(mut self, entries: impl IntoIterator<Item = CreditTransferEntry>) -> Self {
        self.entries.extend(entries);
        self
    }

    /// Number of transfers in this group.
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

/// Builder for an ISO 20022 pain.001 (SEPA Credit Transfer) message.
///
/// A message carries one or more [`CreditTransferGroup`]s, each becoming a
/// `PmtInf` block.
#[derive(Debug, Clone)]
pub struct Pain001Builder {
    initiating_party: String,
    msg_id: String,
    created_at: Option<IsoDateTime>,
    schema: CreditTransferSchema,
    charset: CharsetPolicy,
    groups: Vec<CreditTransferGroup>,
}

impl Pain001Builder {
    /// A new message initiated by `initiating_party`, identified by `msg_id`.
    ///
    /// `MsgId` is how a bank de-duplicates submissions: two files sharing one
    /// are a duplicate, and the second is rejected — or, worse, accepted and
    /// silently discarded. It is therefore required, and must come from a
    /// sequence that survives a restart. Earlier versions generated a
    /// clock-derived placeholder; a value that looks like an identifier and is
    /// not one is worse than no value at all.
    pub fn new(initiating_party: impl Into<String>, msg_id: impl Into<String>) -> Self {
        Self {
            initiating_party: initiating_party.into(),
            msg_id: msg_id.into(),
            created_at: None,
            schema: CreditTransferSchema::default(),
            charset: CharsetPolicy::default(),
            groups: Vec::new(),
        }
    }

    /// Pin the creation timestamp (`GrpHdr/CreDtTm`).
    ///
    /// This is the crate's **only** implicit clock read: left unset, `build()`
    /// stamps [`IsoDateTime::now`]. Set it to make output byte-reproducible —
    /// for golden-file tests, or to regenerate a submitted file identically for
    /// an audit.
    #[must_use]
    pub fn created_at(mut self, timestamp: IsoDateTime) -> Self {
        self.created_at = Some(timestamp);
        self
    }

    /// Select the pain.001 schema version (default
    /// [`CreditTransferSchema::IsoV9`]).
    #[must_use]
    pub fn schema(mut self, schema: CreditTransferSchema) -> Self {
        self.schema = schema;
        self
    }

    /// Set how text outside the SEPA character set is handled.
    #[must_use]
    pub fn charset(mut self, policy: CharsetPolicy) -> Self {
        self.charset = policy;
        self
    }

    /// Add a payment group (`PmtInf`).
    #[must_use]
    pub fn add_group(mut self, group: CreditTransferGroup) -> Self {
        self.groups.push(group);
        self
    }

    /// The `GrpHdr/MsgId` this message will carry.
    ///
    /// What a bank de-duplicates by, and therefore what a
    /// [`camt.055`](crate::camt055) recall has to name — see
    /// [`OriginalMessage::from_credit_transfer`](crate::OriginalMessage) and
    /// its direct-debit counterpart.
    #[must_use]
    pub fn message_id(&self) -> &str {
        &self.msg_id
    }

    /// The schema version this message will be emitted against.
    #[must_use]
    pub const fn schema_version(&self) -> CreditTransferSchema {
        self.schema
    }

    /// The pinned `GrpHdr/CreDtTm`, or `None` when `build()` will stamp one.
    ///
    /// `None` rather than "now": a timestamp read here would not be the one the
    /// document ends up carrying, and a recall that quotes the wrong
    /// `OrgnlCreDtTm` names a message that was never sent.
    #[must_use]
    pub const fn creation_timestamp(&self) -> Option<IsoDateTime> {
        self.created_at
    }

    /// Number of payment groups.
    #[must_use]
    pub fn group_count(&self) -> usize {
        self.groups.len()
    }

    /// Total number of transactions across every group — the `GrpHdr/NbOfTxs`.
    #[must_use]
    pub fn entry_count(&self) -> usize {
        self.groups
            .iter()
            .map(CreditTransferGroup::entry_count)
            .sum()
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
        // Several groups need distinct identifiers. Suffix the MsgId, trimming
        // it first so the result still fits Max35Text.
        let suffix = format!("-{}", index + 1);
        let keep = MAX_ID_LEN.saturating_sub(suffix.chars().count());
        format!("{}{suffix}", truncate_chars(&self.msg_id, keep))
    }

    /// Refuse a BIC the selected schema's `BIC`/`BICFI` type cannot express.
    ///
    /// ISO 9362:2022 admits a digit in the business party prefix and the
    /// pre-2019 `BICIdentifier` does not, so this is the one place a valid
    /// value and a valid schema can still be incompatible. Failing here names
    /// the BIC; emitting it would fail at the bank with an XSD error.
    fn check_bic_supported(&self, field: &'static str, bic: &Bic) -> Result<(), ValidationError> {
        let pattern = self.schema.bic_pattern();
        if bic.fits(pattern) {
            Ok(())
        } else {
            Err(ValidationError::SchemaPattern {
                field,
                value: bic.as_str().to_owned(),
                schema: self.schema.message_id(),
                expected: pattern.as_xsd_pattern(),
            })
        }
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
            self.validate_group(i, g, &mut seen_ids, &mut total)?;
        }
        Ok(())
    }

    /// The rules that tie a group's local instrument to its execution moment.
    ///
    /// None of these are expressible in XSD, so a file that breaks them
    /// validates cleanly against the schema and is rejected on ingestion.
    fn check_instrument_and_timing(
        g: &CreditTransferGroup,
        schema: CreditTransferSchema,
    ) -> Result<(), ValidationError> {
        let unsupported = |feature| ValidationError::UnsupportedBySchema {
            feature,
            schema: schema.message_id(),
        };
        // `LclInstrm` does not exist in every schema, so an SCT Inst group on
        // the DK schema would silently produce an invalid file.
        if g.kind.local_instrument().is_some() && !schema.supports_local_instrument() {
            return Err(unsupported("PmtTpInf/LclInstrm (SCT Inst)"));
        }
        // OCT Inst is specified against the 2019 message version and nothing
        // else: EPC250-22 names `pain.001.001.09` throughout, and the older
        // schemas predate the scheme by a decade. Emitting `EOLO` into one of
        // them would be inventing a combination no rulebook describes.
        if g.kind == CreditTransferKind::OneLegOutInstant && schema != CreditTransferSchema::IsoV9 {
            return Err(unsupported("PmtTpInf/SvcLvl/Cd = EOLO (OCT Inst)"));
        }
        // `SLEV` is mandatory for the four SEPA schemes and forbidden for OCT
        // Inst, which allows only CRED, DEBT and SHAR. Both directions are a
        // rejection on ingestion and neither is expressible in XSD.
        let bearer = g.effective_charge_bearer();
        if !g.kind.allows_charge_bearer(bearer) {
            return Err(ValidationError::ChargeBearerNotAllowed {
                bearer: bearer.as_code(),
                scheme: g.kind.service_level(),
            });
        }
        // A timed execution exists only where `ReqdExctnDt` is a choice…
        let ExecutionMoment::At(moment) = g.execution else {
            return Ok(());
        };
        if !schema.supports_execution_time() {
            return Err(unsupported("ReqdExctnDt/DtTm (timed execution)"));
        }
        // …and only for SCT Inst. The DK validation subset annotates `DtTm`
        // "Only allowed for SCTinst": an ordinary SCT settles some time during
        // the banking day, so a time of day on one instructs nothing.
        if !g.kind.is_instant() {
            return Err(ValidationError::Requires {
                feature: "ReqdExctnDt/DtTm (timed execution)",
                requires: "an instant scheme — CreditTransferKind::Instant or ::OneLegOutInstant",
            });
        }
        // The same annotation carries the usage rule "Only UTC time format or
        // local time with UTC offset format can be used" — a bare local time
        // does not name an instant, and naming one is the whole point.
        if moment.offset_minutes().is_none() {
            return Err(ValidationError::Requires {
                feature: "ReqdExctnDt/DtTm (timed execution)",
                requires: "a UTC offset — see IsoDateTime::in_utc",
            });
        }
        Ok(())
    }

    /// Validate one `PmtInf` and add its amounts to the running control sum.
    fn validate_group(
        &self,
        i: usize,
        g: &CreditTransferGroup,
        seen_ids: &mut std::collections::BTreeSet<String>,
        total: &mut i64,
    ) -> Result<(), BuildError> {
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
            "Dbtr/Nm",
            &self.charset.apply("Dbtr/Nm", &g.debtor_name).at(at)?,
        )
        .at(at)?;
        Self::check_instrument_and_timing(g, self.schema).at(at)?;
        if let Some(bic) = &g.debtor_bic {
            self.check_bic_supported("DbtrAgt/FinInstnId", bic).at(at)?;
        }
        if let Some(a) = &g.debtor_address {
            self.check_address_supported("Dbtr/PstlAdr").at(at)?;
            a.validate(self.charset).at(at)?;
        }
        if let Some(p) = &g.category_purpose {
            p.validate("PmtTpInf/CtgyPurp/Cd").at(at)?;
        }
        if let Some(p) = &g.ultimate_debtor {
            p.validate("PmtInf/UltmtDbtr", self.charset).at(at)?;
        }

        for (j, e) in g.entries.iter().enumerate() {
            let at = Location::transaction(i, j);
            self.validate_entry(at, g, e)?;
            *total = accumulate_control_sum(*total, e.amount_ct).at(at)?;
        }
        Ok(())
    }

    /// Validate one `CdtTrfTxInf` against its enclosing group.
    fn validate_entry(
        &self,
        at: Location,
        g: &CreditTransferGroup,
        e: &CreditTransferEntry,
    ) -> Result<(), BuildError> {
        // The DK forbids the same ultimate party at both levels.
        if g.ultimate_debtor.is_some() && e.ultimate_debtor.is_some() {
            return Err(BuildError {
                location: at,
                kind: ValidationError::ConflictingLevels { field: "UltmtDbtr" },
            });
        }
        check_id("CdtTrfTxInf/PmtId/EndToEndId", &e.end_to_end_id).at(at)?;
        check_amount("CdtTrfTxInf/Amt/InstdAmt", e.amount_ct).at(at)?;
        // A non-euro amount and an AT-T020 instruction are both OCT Inst
        // features. Under a SEPA scheme they would produce a document that is
        // schema-valid and means something the rulebook does not define, so
        // they are refused rather than dropped (D9: never silently discard a
        // value the caller asked for).
        if !g.kind.allows_non_euro_amount() {
            if e.currency.is_some_and(|c| !c.is_euro()) {
                return Err(BuildError {
                    location: at,
                    kind: ValidationError::Requires {
                        feature: "CdtTrfTxInf/Amt/InstdAmt @Ccy other than EUR",
                        requires: "CreditTransferKind::OneLegOutInstant",
                    },
                });
            }
            if e.non_euro_leg_currency.is_some() {
                return Err(BuildError {
                    location: at,
                    kind: ValidationError::Requires {
                        feature: "InstrForCdtrAgt/InstrInf (AT-T020)",
                        requires: "CreditTransferKind::OneLegOutInstant",
                    },
                });
            }
        }
        check_name(
            "Cdtr/Nm",
            &self.charset.apply("Cdtr/Nm", &e.creditor_name).at(at)?,
        )
        .at(at)?;
        if let Some(bic) = &e.creditor_bic {
            self.check_bic_supported("CdtrAgt/FinInstnId", bic).at(at)?;
        }
        if let Some(a) = &e.creditor_address {
            self.check_address_supported("Cdtr/PstlAdr").at(at)?;
            a.validate(self.charset).at(at)?;
        }
        if let Some(p) = &e.ultimate_debtor {
            p.validate("CdtTrfTxInf/UltmtDbtr", self.charset).at(at)?;
        }
        if let Some(p) = &e.ultimate_creditor {
            p.validate("CdtTrfTxInf/UltmtCdtr", self.charset).at(at)?;
        }
        if let Some(p) = &e.purpose {
            p.validate("CdtTrfTxInf/Purp/Cd").at(at)?;
        }
        if let Some(r) = &e.remittance {
            r.validate(remittance_field(r), self.charset).at(at)?;
        }
        Ok(())
    }

    /// Validate the message and generate the pain.001 XML.
    ///
    /// # Errors
    ///
    /// See [`validate`](Self::validate).
    ///
    /// # Examples
    ///
    /// ```
    /// use sepa::{CreditTransferEntry, CreditTransferGroup, IsoDate, Pain001Builder, validate_iban};
    ///
    /// let iban = validate_iban("DE89370400440532013000")?;
    /// let xml = Pain001Builder::new("Acme GmbH", "CT-001")
    ///     .add_group(
    ///         CreditTransferGroup::new("Acme GmbH", &iban, IsoDate::new(2026, 7, 20)?)
    ///             .add_entry(CreditTransferEntry::new("Payee", iban.clone(), 100, "E2E-1")),
    ///     )
    ///     .build()?;
    /// assert!(xml.contains("<NbOfTxs>1</NbOfTxs>"));
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn build(&self) -> Result<String, BuildError> {
        self.validate()?;
        let mut buf = String::with_capacity(850 + self.entry_count() * 420);
        // Writing into a String is infallible.
        let _ = self.write_xml_to(&mut buf);
        Ok(buf)
    }

    /// Validate and stream the pain.001 XML to an [`io::Write`](std::io::Write).
    ///
    /// Validation runs before anything is written, so a rejected message leaves
    /// the writer untouched.
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

    /// Serialise. Private: callers go through `build` or `write_to`, which validate.
    fn write_xml_to<W: std::fmt::Write>(&self, w: &mut W) -> std::fmt::Result {
        use crate::xml_util::write_escaped;

        let now = self.created_at.unwrap_or_else(IsoDateTime::now);
        let namespace = self.schema.namespace();
        let initiating = self.charset.render(&self.initiating_party);

        w.write_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n")?;
        writeln!(w, "<Document xmlns=\"{namespace}\">")?;
        w.write_str("  <CstmrCdtTrfInitn>\n    <GrpHdr>\n      <MsgId>")?;
        write_escaped(w, &self.msg_id)?;
        write!(w, "</MsgId>\n      <CreDtTm>{now}</CreDtTm>\n")?;
        // GrpHdr totals span every group.
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

        w.write_str("  </CstmrCdtTrfInitn>\n</Document>")
    }

    /// Write one `PmtInf` block, in XSD sequence order.
    fn write_group<W: std::fmt::Write>(
        &self,
        w: &mut W,
        g: &CreditTransferGroup,
        payment_info_id: &str,
    ) -> std::fmt::Result {
        use crate::xml_util::write_escaped;

        let bic_el = self.schema.bic_element();
        let debtor_name = self.charset.render(&g.debtor_name);

        w.write_str("    <PmtInf>\n      <PmtInfId>")?;
        write_escaped(w, payment_info_id)?;
        w.write_str("</PmtInfId>\n      <PmtMtd>TRF</PmtMtd>\n")?;
        if let Some(batch) = g.batch_booking {
            writeln!(w, "      <BtchBookg>{batch}</BtchBookg>")?;
        }
        write!(
            w,
            "      <NbOfTxs>{}</NbOfTxs>\n      <CtrlSum>{}</CtrlSum>\n",
            g.entry_count(),
            ct_to_eur_str(g.total_ct())
        )?;
        writeln!(
            w,
            "      <PmtTpInf>\n        <SvcLvl><Cd>{}</Cd></SvcLvl>",
            g.kind.service_level()
        )?;
        if let Some(code) = g.kind.local_instrument() {
            writeln!(w, "        <LclInstrm><Cd>{code}</Cd></LclInstrm>")?;
        }
        if let Some(p) = &g.category_purpose {
            writeln!(w, "        <CtgyPurp><Cd>{}</Cd></CtgyPurp>", p.as_code())?;
        }
        w.write_str("      </PmtTpInf>\n")?;

        // pain.001.001.09 types ReqdExctnDt as a date/time choice; the older
        // schemas take a bare ISODate.
        match (self.schema.supports_execution_time(), g.execution) {
            (true, ExecutionMoment::On(date)) => {
                writeln!(w, "      <ReqdExctnDt><Dt>{date}</Dt></ReqdExctnDt>")?;
            }
            (true, ExecutionMoment::At(moment)) => {
                writeln!(w, "      <ReqdExctnDt><DtTm>{moment}</DtTm></ReqdExctnDt>")?;
            }
            // `validate` has already refused a timed moment here.
            (false, moment) => {
                writeln!(w, "      <ReqdExctnDt>{}</ReqdExctnDt>", moment.date())?;
            }
        }

        w.write_str("      <Dbtr><Nm>")?;
        write_escaped(w, &debtor_name)?;
        // XSD sequence inside PartyIdentification: Nm, PstlAdr, Id, …
        w.write_str("</Nm>")?;
        if let Some(address) = &g.debtor_address {
            address.write_xml(w, self.charset)?;
        }
        w.write_str("</Dbtr>\n")?;
        writeln!(
            w,
            "      <DbtrAcct><Id><IBAN>{}</IBAN></Id></DbtrAcct>",
            g.debtor_iban.as_str()
        )?;
        // DbtrAgt is mandatory; with no BIC the EPC "IBAN only" form applies.
        w.write_str("      <DbtrAgt><FinInstnId>")?;
        match &g.debtor_bic {
            Some(bic) => write!(w, "<{bic_el}>{}</{bic_el}>", bic.as_str())?,
            None => w.write_str("<Othr><Id>NOTPROVIDED</Id></Othr>")?,
        }
        w.write_str("</FinInstnId></DbtrAgt>\n")?;

        if let Some(p) = &g.ultimate_debtor {
            p.write_xml(w, "UltmtDbtr", "      ", self.charset)?;
        }
        writeln!(w, "      <ChrgBr>{}</ChrgBr>", g.effective_charge_bearer())?;

        for entry in &g.entries {
            self.write_transaction(w, entry)?;
        }
        w.write_str("    </PmtInf>\n")
    }

    fn write_transaction<W: std::fmt::Write>(
        &self,
        w: &mut W,
        e: &CreditTransferEntry,
    ) -> std::fmt::Result {
        use crate::xml_util::{write_escaped, write_eur};

        let bic_el = self.schema.bic_element();
        let name = self.charset.render(&e.creditor_name);

        w.write_str("    <CdtTrfTxInf>\n      <PmtId>\n        <EndToEndId>")?;
        write_escaped(w, &e.end_to_end_id)?;
        write!(
            w,
            "</EndToEndId>\n      </PmtId>\n      <Amt><InstdAmt Ccy=\"{}\">",
            e.effective_currency()
        )?;
        write_eur(w, e.amount_ct)?;
        w.write_str("</InstdAmt></Amt>\n")?;

        // XSD sequence: UltmtDbtr precedes the creditor block, UltmtCdtr follows
        // CdtrAcct, and Purp sits between UltmtCdtr and RmtInf.
        if let Some(p) = &e.ultimate_debtor {
            p.write_xml(w, "UltmtDbtr", "      ", self.charset)?;
        }

        // CdtrAgt is optional for SCT. The EPC guidelines say that when the BIC
        // is unknown the element is simply omitted — and pain.001.003.03 makes
        // that structural, since its CdtrAgt type has a mandatory BIC and no
        // Othr branch, leaving no way to express "not provided".
        if let Some(bic) = &e.creditor_bic {
            writeln!(
                w,
                "      <CdtrAgt><FinInstnId><{bic_el}>{}</{bic_el}></FinInstnId></CdtrAgt>",
                bic.as_str()
            )?;
        }

        w.write_str("      <Cdtr><Nm>")?;
        write_escaped(w, &name)?;
        w.write_str("</Nm>")?;
        if let Some(address) = &e.creditor_address {
            address.write_xml(w, self.charset)?;
        }
        w.write_str("</Cdtr>\n      <CdtrAcct><Id><IBAN>")?;
        w.write_str(e.creditor_iban.as_str())?;
        w.write_str("</IBAN></Id></CdtrAcct>\n")?;

        if let Some(p) = &e.ultimate_creditor {
            p.write_xml(w, "UltmtCdtr", "      ", self.charset)?;
        }
        // XSD sequence in CreditTransferTransaction34: InstrForCdtrAgt sits
        // between UltmtCdtr and Purp.
        if let Some(currency) = &e.non_euro_leg_currency {
            writeln!(
                w,
                "      <InstrForCdtrAgt><InstrInf>{currency}</InstrInf></InstrForCdtrAgt>"
            )?;
        }
        if let Some(purpose) = &e.purpose {
            writeln!(w, "      <Purp><Cd>{}</Cd></Purp>", purpose.as_code())?;
        }

        if let Some(remittance) = &e.remittance {
            // `Strd` is emitted minified: the EPC caps the whole block at 140
            // characters including tags, and pretty-printing alone overruns it.
            remittance.write_xml(w, "      ", self.charset)?;
        }

        w.write_str("    </CdtTrfTxInf>\n")
    }
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
    fn entry(amount_ct: i64) -> CreditTransferEntry {
        CreditTransferEntry::new("Max Mustermann", nl_iban(), amount_ct, "E2E-001")
    }
    /// A one-group message with everything `build` requires.
    fn one_group(name: &str) -> Pain001Builder {
        Pain001Builder::new(name, "CT-001").add_group(
            CreditTransferGroup::new(name, &de_iban(), d("2026-07-20")).add_entry(entry(12_000)),
        )
    }

    #[test]
    fn basic_structure() {
        let xml = one_group("Acme GmbH").build().unwrap();
        assert!(xml.contains("urn:iso:std:iso:20022:tech:xsd:pain.001.001.09"));
        assert!(xml.contains("<MsgId>CT-001</MsgId>"));
        assert!(xml.contains("<PmtMtd>TRF</PmtMtd>"));
        assert!(xml.contains("<InstdAmt Ccy=\"EUR\">120.00</InstdAmt>"));
        assert!(xml.contains("<ChrgBr>SLEV</ChrgBr>"));
        // pain.001.001.09 wraps the date in a <Dt> choice child.
        assert!(xml.contains("<ReqdExctnDt><Dt>2026-07-20</Dt></ReqdExctnDt>"));
    }

    #[test]
    fn several_groups_carry_their_own_dates_and_totals() {
        let xml = Pain001Builder::new("Acme GmbH", "CT-MULTI")
            .add_group(
                CreditTransferGroup::new("Acme GmbH", &de_iban(), d("2026-07-20"))
                    .add_entry(entry(10_000)),
            )
            .add_group(
                CreditTransferGroup::new("Acme GmbH", &nl_iban(), d("2026-07-25"))
                    .add_entry(entry(5_000))
                    .add_entry(entry(2_500)),
            )
            .build()
            .unwrap();

        assert_eq!(xml.matches("<PmtInf>").count(), 2);
        // Different execution dates in one file — impossible with one PmtInf.
        assert!(xml.contains("<Dt>2026-07-20</Dt>"));
        assert!(xml.contains("<Dt>2026-07-25</Dt>"));
        // GrpHdr totals span every group; each PmtInf carries its own.
        assert!(xml.contains("<NbOfTxs>3</NbOfTxs>"));
        assert!(xml.contains("<CtrlSum>175.00</CtrlSum>"));
        assert!(xml.contains("<NbOfTxs>1</NbOfTxs>"));
        assert!(xml.contains("<CtrlSum>100.00</CtrlSum>"));
        assert!(xml.contains("<NbOfTxs>2</NbOfTxs>"));
        assert!(xml.contains("<CtrlSum>75.00</CtrlSum>"));
    }

    #[test]
    fn payment_info_ids_are_unique_and_within_max35text() {
        let msg_id = "M".repeat(35);
        let b = Pain001Builder::new("Acme", &msg_id);
        let b = (0..3).fold(b, |b, _| {
            b.add_group(
                CreditTransferGroup::new("Acme", &de_iban(), d("2026-07-20")).add_entry(entry(100)),
            )
        });
        let xml = b.build().unwrap();

        let ids: Vec<&str> = xml
            .split("<PmtInfId>")
            .skip(1)
            .map(|c| c.split('<').next().unwrap())
            .collect();
        assert_eq!(ids.len(), 3);
        for id in &ids {
            assert!(id.chars().count() <= 35, "{id} exceeds Max35Text");
        }
        let unique: std::collections::BTreeSet<_> = ids.iter().collect();
        assert_eq!(unique.len(), 3, "PmtInfId must be unique per group");
    }

    #[test]
    fn two_groups_may_not_share_a_payment_info_id() {
        // A bank echoes PmtInfId back in pain.002 and in the camt `Btch` block.
        // Two groups sharing one make a booking unattributable, and duplicate
        // detection may drop the second outright.
        let g = |id: &str| {
            CreditTransferGroup::new("Acme", &de_iban(), d("2026-07-20"))
                .payment_info_id(id)
                .add_entry(entry(100))
        };
        let err = Pain001Builder::new("Acme", "CT-DUP")
            .add_group(g("PMT-1"))
            .add_group(g("PMT-1"))
            .build()
            .unwrap_err();
        assert_eq!(err.location, Location::group(1));
        assert_eq!(
            err.kind,
            ValidationError::Duplicate {
                field: "PmtInf/PmtInfId",
                value: "PMT-1".to_owned(),
            }
        );
        // Distinct identifiers, and the generated defaults, are fine.
        assert!(
            Pain001Builder::new("Acme", "CT-OK")
                .add_group(g("PMT-1"))
                .add_group(g("PMT-2"))
                .build()
                .is_ok()
        );
    }

    #[test]
    fn the_message_id_is_the_callers_and_is_validated() {
        // There is deliberately no generated default. The old one was
        // `sct-<epoch seconds>`, so two messages built in the same second — the
        // normal case in a batch job — shared the key a bank de-duplicates
        // submissions by. A caller-supplied id is still checked like any other.
        assert!(matches!(
            Pain001Builder::new("Acme", "X".repeat(36))
                .add_group(
                    CreditTransferGroup::new("Acme", &de_iban(), d("2026-07-20"))
                        .add_entry(entry(100))
                )
                .build()
                .unwrap_err()
                .kind,
            ValidationError::TooLong {
                field: "GrpHdr/MsgId",
                ..
            }
        ));
    }

    #[test]
    fn single_group_reuses_the_msg_id_verbatim() {
        let xml = one_group("Acme").build().unwrap();
        assert!(xml.contains("<PmtInfId>CT-001</PmtInfId>"));
    }

    #[test]
    fn legacy_dk_schema_emits_a_bare_date_and_bic() {
        let xml = Pain001Builder::new("Test", "CT-DK")
            .schema(CreditTransferSchema::DkV2_7)
            .add_group(
                CreditTransferGroup::new("Test", &de_iban(), d("2026-07-20"))
                    .debtor_bic("COBADEFF".parse().unwrap())
                    .add_entry(entry(5_000).with_bic("ABNANL2A".parse().unwrap())),
            )
            .build()
            .unwrap();
        assert!(xml.contains("pain.001.003.03"));
        assert!(xml.contains("<ReqdExctnDt>2026-07-20</ReqdExctnDt>"));
        assert!(xml.contains("<BIC>COBADEFF</BIC>"));
        assert!(!xml.contains("BICFI"));
    }

    #[test]
    fn iban_only_agents_and_omitted_creditor_agent() {
        let xml = one_group("Test").build().unwrap();
        assert!(!xml.contains("<CdtrAgt>"), "EPC omits CdtrAgt with no BIC");
        assert!(xml.contains("<DbtrAgt><FinInstnId><Othr><Id>NOTPROVIDED</Id></Othr>"));
        assert!(!xml.contains("NOTPROVIDED</BIC"));
    }

    #[test]
    fn sct_instant_marks_the_group() {
        let xml = Pain001Builder::new("Acme", "CT-INST")
            .add_group(
                CreditTransferGroup::new("Acme", &de_iban(), d("2026-07-20"))
                    .kind(CreditTransferKind::Instant)
                    .add_entry(entry(5_000)),
            )
            .build()
            .unwrap();
        assert!(xml.contains("pain.001.001.09"));
        assert!(xml.contains("<LclInstrm><Cd>INST</Cd></LclInstrm>"));
    }

    #[test]
    fn postal_addresses_sit_inside_the_party_after_the_name() {
        // PartyIdentification is a sequence: Nm, then PstlAdr. Emitting them the
        // other way round is schema-invalid even though both elements are legal.
        let xml = Pain001Builder::new("Acme GmbH", "CT-ADR")
            .add_group(
                CreditTransferGroup::new("Acme GmbH", &de_iban(), d("2026-07-20"))
                    .debtor_address(
                        crate::PostalAddress::new("Berlin", "DE")
                            .unwrap()
                            .street("Unter den Linden")
                            .building_number("77")
                            .post_code("10117"),
                    )
                    .add_entry(entry(12_000).with_creditor_address(
                        crate::PostalAddress::new("Amsterdam", "NL").unwrap(),
                    )),
            )
            .build()
            .unwrap();

        assert!(xml.contains(
            "<Dbtr><Nm>Acme GmbH</Nm><PstlAdr><StrtNm>Unter den Linden</StrtNm>\
             <BldgNb>77</BldgNb><PstCd>10117</PstCd><TwnNm>Berlin</TwnNm>\
             <Ctry>DE</Ctry></PstlAdr></Dbtr>"
        ));
        assert!(xml.contains(
            "<Cdtr><Nm>Max Mustermann</Nm><PstlAdr><TwnNm>Amsterdam</TwnNm>\
             <Ctry>NL</Ctry></PstlAdr></Cdtr>"
        ));
    }

    #[test]
    fn an_address_on_a_schema_without_one_is_rejected() {
        // pain.001.003.03's PostalAddressSEPA holds only Ctry and two AdrLines,
        // so a town and a street have nowhere to go.
        let err = Pain001Builder::new("Acme", "CT-DK-ADR")
            .schema(CreditTransferSchema::DkV2_7)
            .add_group(
                CreditTransferGroup::new("Acme", &de_iban(), d("2026-07-20"))
                    .debtor_address(crate::PostalAddress::new("Berlin", "DE").unwrap())
                    .add_entry(entry(100)),
            )
            .build()
            .unwrap_err();
        assert_eq!(
            err.kind,
            ValidationError::UnsupportedBySchema {
                feature: "Dbtr/PstlAdr",
                schema: "pain.001.003.03",
            }
        );
        assert_eq!(err.location, Location::group(0));
    }

    #[test]
    fn address_violations_name_the_element_and_the_transaction() {
        let err = Pain001Builder::new("Acme", "CT-ADR-BAD")
            .add_group(
                CreditTransferGroup::new("Acme", &de_iban(), d("2026-07-20"))
                    .add_entry(entry(100))
                    .add_entry(
                        entry(100).with_creditor_address(
                            crate::PostalAddress::new("Berlin", "DE")
                                .unwrap()
                                .post_code("1".repeat(17)),
                        ),
                    ),
            )
            .build()
            .unwrap_err();
        assert_eq!(err.location, Location::transaction(0, 1));
        assert!(matches!(
            err.kind,
            ValidationError::TooLong {
                field: "PstlAdr/PstCd",
                max: 16,
                ..
            }
        ));
    }

    #[test]
    fn batch_booking_and_category_purpose_are_group_level() {
        let xml = Pain001Builder::new("Acme", "CT-OPT")
            .add_group(
                CreditTransferGroup::new("Acme", &de_iban(), d("2026-07-20"))
                    .batch_booking(true)
                    .category_purpose(CategoryPurpose::Sala)
                    .add_entry(entry(100)),
            )
            .build()
            .unwrap();
        assert!(xml.contains("<BtchBookg>true</BtchBookg>"));
        assert!(xml.contains("<CtgyPurp><Cd>SALA</Cd></CtgyPurp>"));
    }

    #[test]
    fn batch_booking_is_omitted_unless_set() {
        // There is no scheme-wide default; omitting defers to the bank agreement.
        assert!(!one_group("Acme").build().unwrap().contains("BtchBookg"));
    }

    #[test]
    fn ultimate_debtor_cannot_be_set_at_both_levels() {
        let b = Pain001Builder::new("Acme", "CT-ULT").add_group(
            CreditTransferGroup::new("Acme", &de_iban(), d("2026-07-20"))
                .ultimate_debtor(Party::new("Gruppe"))
                .add_entry(entry(100).with_ultimate_debtor(Party::new("Transaktion"))),
        );
        assert_eq!(
            b.build(),
            Err(BuildError::transaction(
                0,
                0,
                ValidationError::ConflictingLevels { field: "UltmtDbtr" }
            ))
        );
    }

    #[test]
    fn empty_message_and_empty_group_are_both_rejected() {
        assert_eq!(
            Pain001Builder::new("Acme", "E").build(),
            Err(BuildError::message(ValidationError::EmptyBatch))
        );
        assert_eq!(
            Pain001Builder::new("Acme", "E")
                .add_group(CreditTransferGroup::new(
                    "Acme",
                    &de_iban(),
                    d("2026-07-20")
                ))
                .build(),
            Err(BuildError::group(0, ValidationError::EmptyBatch))
        );
    }

    #[test]
    fn validation_rejects_bad_fields() {
        let g = || CreditTransferGroup::new("Acme", &de_iban(), d("2026-07-20"));
        let b = || Pain001Builder::new("Acme", "OK");
        let kind = |r: Result<String, BuildError>| r.unwrap_err().kind;

        assert!(matches!(
            kind(b().add_group(g().add_entry(entry(0))).build()),
            ValidationError::AmountOutOfRange { .. }
        ));
        assert!(matches!(
            kind(b().add_group(g().add_entry(entry(100_000_000_000))).build()),
            ValidationError::AmountOutOfRange { .. }
        ));
        assert!(matches!(
            kind(
                Pain001Builder::new("Acme", "X".repeat(36))
                    .add_group(g().add_entry(entry(100)))
                    .build()
            ),
            ValidationError::TooLong { .. }
        ));
    }

    #[test]
    fn errors_name_the_group_and_transaction_that_failed() {
        let g = || CreditTransferGroup::new("Acme", &de_iban(), d("2026-07-20"));
        let err = Pain001Builder::new("Acme", "CT-LOC")
            .add_group(g().add_entry(entry(100)))
            .add_group(g().add_entry(entry(100)).add_entry(entry(0)))
            .build()
            .unwrap_err();
        assert_eq!(err.location, Location::transaction(1, 1));
        assert!(err.to_string().starts_with("PmtInf[1]/Tx[1]: "));
    }

    #[test]
    fn schema_versions_round_trip_through_their_identifiers() {
        for schema in CreditTransferSchema::ALL {
            assert_eq!(
                schema.message_id().parse::<CreditTransferSchema>(),
                Ok(*schema)
            );
            assert_eq!(
                schema.namespace().parse::<CreditTransferSchema>(),
                Ok(*schema)
            );
            assert!(schema.namespace().ends_with(schema.message_id()));
        }
        assert!("pain.001.001.99".parse::<CreditTransferSchema>().is_err());
    }

    #[test]
    fn the_epc_legacy_schema_emits_a_bare_date_and_the_pre_2019_bic_element() {
        let xml = Pain001Builder::new("Acme", "CT-V3")
            .schema(CreditTransferSchema::IsoV3)
            .add_group(
                CreditTransferGroup::new("Acme", &de_iban(), d("2026-07-20"))
                    .debtor_bic("COBADEFF".parse().unwrap())
                    .add_entry(entry(5_000).with_bic("ABNANL2A".parse().unwrap())),
            )
            .build()
            .unwrap();
        assert!(xml.contains("pain.001.001.03"));
        assert!(xml.contains("<ReqdExctnDt>2026-07-20</ReqdExctnDt>"));
        assert!(xml.contains("<BIC>COBADEFF</BIC>"));
        assert!(!xml.contains("BICFI"));
    }

    #[test]
    fn sct_instant_on_a_schema_without_lclinstrm_is_rejected() {
        // Regression: the DK schema has no LclInstrm element, so this used to
        // emit a file that failed its own XSD. It is now a typed error.
        let err = Pain001Builder::new("Acme", "CT-DK-INST")
            .schema(CreditTransferSchema::DkV2_7)
            .add_group(
                CreditTransferGroup::new("Acme", &de_iban(), d("2026-07-20"))
                    .kind(CreditTransferKind::Instant)
                    .add_entry(entry(5_000)),
            )
            .build()
            .unwrap_err();
        assert_eq!(
            err.kind,
            ValidationError::UnsupportedBySchema {
                feature: "PmtTpInf/LclInstrm (SCT Inst)",
                schema: "pain.001.003.03",
            }
        );
        assert_eq!(err.location, Location::group(0));

        // The EPC schemas both carry it.
        for schema in [CreditTransferSchema::IsoV9, CreditTransferSchema::IsoV3] {
            let xml = Pain001Builder::new("Acme", "CT-INST")
                .schema(schema)
                .add_group(
                    CreditTransferGroup::new("Acme", &de_iban(), d("2026-07-20"))
                        .kind(CreditTransferKind::Instant)
                        .add_entry(entry(5_000)),
                )
                .build()
                .unwrap();
            assert!(
                xml.contains("<LclInstrm><Cd>INST</Cd></LclInstrm>"),
                "{schema}"
            );
        }
    }

    #[test]
    fn totals_use_integer_arithmetic() {
        let b = Pain001Builder::new("Acme", "CT").add_group(
            CreditTransferGroup::new("Acme", &de_iban(), d("2026-07-20"))
                .add_entry(entry(10))
                .add_entry(entry(20)),
        );
        assert_eq!(b.total_ct(), 30);
        assert!(b.build().unwrap().contains("<CtrlSum>0.30</CtrlSum>"));
    }

    #[test]
    fn creation_timestamp_can_be_pinned_for_reproducible_output() {
        let build = || {
            one_group("Acme")
                .created_at("2026-07-19T12:00:00".parse().unwrap())
                .build()
                .unwrap()
        };
        assert_eq!(build(), build(), "output must be byte-identical");
        assert!(build().contains("<CreDtTm>2026-07-19T12:00:00</CreDtTm>"));
    }

    #[test]
    fn non_sepa_characters_are_transliterated() {
        let xml = Pain001Builder::new("Müller & Söhne GmbH", "CT-UML")
            .add_group(
                CreditTransferGroup::new("Müller & Söhne GmbH", &de_iban(), d("2026-07-20"))
                    .add_entry(
                        CreditTransferEntry::new("Ökonomie AG", nl_iban(), 100, "E2E-1")
                            .with_description("Zahlung für Groß-Auftrag"),
                    ),
            )
            .build()
            .unwrap();
        assert!(xml.contains("Mueller + Soehne GmbH"));
        assert!(xml.contains("Oekonomie AG"));
        assert!(xml.contains("Zahlung fuer Gross-Auftrag"));
    }

    #[test]
    fn strict_charset_policy_rejects_umlauts() {
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
    fn streaming_matches_the_in_memory_build() {
        let direct = one_group("Acme")
            .created_at("2026-07-19T12:00:00".parse().unwrap())
            .build()
            .unwrap();
        let mut buf: Vec<u8> = Vec::new();
        one_group("Acme")
            .created_at("2026-07-19T12:00:00".parse().unwrap())
            .write_to(&mut buf)
            .unwrap();
        assert_eq!(direct, String::from_utf8(buf).unwrap());

        // A rejected message writes nothing at all.
        let mut empty: Vec<u8> = Vec::new();
        assert!(
            Pain001Builder::new("Acme", "CT-EMPTY")
                .write_to(&mut empty)
                .is_err()
        );
        assert!(empty.is_empty());
    }

    // ── OCT Inst (EOLO) ───────────────────────────────────────────────────

    /// Everything the OCT Inst C2PSP guidelines change, in one document.
    #[test]
    fn oct_inst_emits_eolo_inst_and_a_non_slev_charge_bearer() {
        let xml = Pain001Builder::new("Acme GmbH", "OCT-001")
            .add_group(
                CreditTransferGroup::new("Acme GmbH", &de_iban(), d("2026-07-20"))
                    .kind(CreditTransferKind::OneLegOutInstant)
                    .add_entry(entry(12_000)),
            )
            .build()
            .unwrap();
        // EPC250-22: the scheme is *implied* by these two together.
        assert!(xml.contains("<SvcLvl><Cd>EOLO</Cd></SvcLvl>"), "{xml}");
        assert!(
            xml.contains("<LclInstrm><Cd>INST</Cd></LclInstrm>"),
            "{xml}"
        );
        // SLEV is mandatory for SEPA and forbidden here; SHAR is the default.
        assert!(xml.contains("<ChrgBr>SHAR</ChrgBr>"), "{xml}");
        assert!(!xml.contains("SLEV"), "{xml}");
        // The euro amount is still the ordinary case.
        assert!(
            xml.contains(r#"<InstdAmt Ccy="EUR">120.00</InstdAmt>"#),
            "{xml}"
        );
    }

    #[test]
    fn oct_inst_carries_a_non_euro_amount_and_at_t020() {
        let xml = Pain001Builder::new("Acme GmbH", "OCT-002")
            .add_group(
                CreditTransferGroup::new("Acme GmbH", &de_iban(), d("2026-07-20"))
                    .kind(CreditTransferKind::OneLegOutInstant)
                    .charge_bearer(ChargeBearer::Cred)
                    .add_entry(
                        entry(12_000)
                            .with_currency("USD".parse().unwrap())
                            .with_non_euro_leg_currency("USD".parse().unwrap()),
                    ),
            )
            .build()
            .unwrap();
        assert!(
            xml.contains(r#"<InstdAmt Ccy="USD">120.00</InstdAmt>"#),
            "{xml}"
        );
        // AT-T020 has no element of its own; EPC250-22 puts it here.
        assert!(
            xml.contains("<InstrForCdtrAgt><InstrInf>USD</InstrInf></InstrForCdtrAgt>"),
            "{xml}"
        );
        assert!(xml.contains("<ChrgBr>CRED</ChrgBr>"), "{xml}");
    }

    #[test]
    fn the_sepa_schemes_keep_slev_and_euro() {
        for kind in [CreditTransferKind::Standard, CreditTransferKind::Instant] {
            let xml = Pain001Builder::new("Acme", "CT-1")
                .add_group(
                    CreditTransferGroup::new("Acme", &de_iban(), d("2026-07-20"))
                        .kind(kind)
                        .add_entry(entry(12_000)),
                )
                .build()
                .unwrap();
            assert!(xml.contains("<SvcLvl><Cd>SEPA</Cd></SvcLvl>"), "{kind:?}");
            assert!(xml.contains("<ChrgBr>SLEV</ChrgBr>"), "{kind:?}");
            assert_eq!(
                kind == CreditTransferKind::Instant,
                xml.contains("<LclInstrm><Cd>INST</Cd></LclInstrm>"),
                "{kind:?}"
            );
        }
    }

    /// Each cross-scheme combination is refused, and refused by name.
    #[test]
    fn a_rule_from_the_wrong_scheme_is_rejected_rather_than_emitted() {
        let group = |kind| CreditTransferGroup::new("Acme", &de_iban(), d("2026-07-20")).kind(kind);
        let build =
            |g: CreditTransferGroup| Pain001Builder::new("Acme", "CT-X").add_group(g).build();

        // SLEV under OCT Inst.
        assert_eq!(
            build(
                group(CreditTransferKind::OneLegOutInstant)
                    .charge_bearer(ChargeBearer::Slev)
                    .add_entry(entry(100))
            )
            .unwrap_err()
            .kind,
            ValidationError::ChargeBearerNotAllowed {
                bearer: "SLEV",
                scheme: "EOLO",
            }
        );
        // A non-SLEV bearer under SEPA.
        assert_eq!(
            build(
                group(CreditTransferKind::Standard)
                    .charge_bearer(ChargeBearer::Shar)
                    .add_entry(entry(100))
            )
            .unwrap_err()
            .kind,
            ValidationError::ChargeBearerNotAllowed {
                bearer: "SHAR",
                scheme: "SEPA",
            }
        );
        // A non-euro amount under SEPA.
        assert_eq!(
            build(
                group(CreditTransferKind::Instant)
                    .add_entry(entry(100).with_currency("USD".parse().unwrap()))
            )
            .unwrap_err()
            .kind,
            ValidationError::Requires {
                feature: "CdtTrfTxInf/Amt/InstdAmt @Ccy other than EUR",
                requires: "CreditTransferKind::OneLegOutInstant",
            }
        );
        // AT-T020 under SEPA.
        assert_eq!(
            build(
                group(CreditTransferKind::Standard)
                    .add_entry(entry(100).with_non_euro_leg_currency("USD".parse().unwrap()))
            )
            .unwrap_err()
            .kind,
            ValidationError::Requires {
                feature: "InstrForCdtrAgt/InstrInf (AT-T020)",
                requires: "CreditTransferKind::OneLegOutInstant",
            }
        );
        // An explicit EUR is not a "non-euro amount" and stays legal anywhere.
        assert!(
            build(
                group(CreditTransferKind::Standard)
                    .add_entry(entry(100).with_currency(crate::Currency::EUR))
            )
            .is_ok()
        );
    }

    #[test]
    fn oct_inst_needs_the_2019_message_version() {
        for schema in [CreditTransferSchema::IsoV3, CreditTransferSchema::DkV2_7] {
            let err = Pain001Builder::new("Acme", "CT-OCT")
                .schema(schema)
                .add_group(
                    CreditTransferGroup::new("Acme", &de_iban(), d("2026-07-20"))
                        .kind(CreditTransferKind::OneLegOutInstant)
                        .add_entry(entry(100)),
                )
                .build()
                .unwrap_err();
            assert!(
                matches!(err.kind, ValidationError::UnsupportedBySchema { .. }),
                "{schema:?} must refuse EOLO, got {err:?}"
            );
        }
    }

    /// A timed execution is about settling at a moment, so both instant
    /// schemes allow it and the ordinary one does not.
    #[test]
    fn a_timed_execution_follows_the_scheme_not_the_local_instrument() {
        let at: IsoDateTime = "2026-07-20T11:00:00Z".parse().unwrap();
        let build = |kind| {
            Pain001Builder::new("Acme", "CT-T")
                .add_group(
                    CreditTransferGroup::new("Acme", &de_iban(), at)
                        .kind(kind)
                        .add_entry(entry(100)),
                )
                .build()
        };
        assert!(build(CreditTransferKind::Instant).is_ok());
        assert!(build(CreditTransferKind::OneLegOutInstant).is_ok());
        assert!(build(CreditTransferKind::Standard).is_err());
    }

    /// The control sum is bounded by `CtrlSum`'s own schema type, not by
    /// `i64`. Regression: entries each inside `MAX_AMOUNT_CT` could sum to a
    /// 19-digit `DecimalNumber`, which `totalDigits="18"` rejects.
    #[test]
    fn the_control_sum_is_bounded_by_the_schema_not_by_i64() {
        use crate::validate::{MAX_CTRL_SUM_CT, accumulate_control_sum};

        assert_eq!(accumulate_control_sum(0, 100), Ok(100));
        assert!(accumulate_control_sum(MAX_CTRL_SUM_CT - 1, 1).is_ok());
        // The window that used to pass validation and fail `xmllint`: over the
        // schema's bound, under `i64::MAX`.
        assert!(accumulate_control_sum(MAX_CTRL_SUM_CT, 1).is_err());

        let digits = |ct| {
            crate::ct_to_eur_str(ct)
                .chars()
                .filter(char::is_ascii_digit)
                .count()
        };
        assert_eq!(
            digits(MAX_CTRL_SUM_CT),
            18,
            "CtrlSum is a DecimalNumber with totalDigits=18"
        );
        assert_eq!(
            digits(i64::MAX),
            19,
            "i64::MAX is a 19-digit CtrlSum — one past what the schema allows, \
             which is why the bound cannot be the integer type's"
        );
    }
}
