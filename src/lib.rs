//! # sepa — SEPA payment utilities for Rust
//!
//! Provides:
//! - **IBAN validation** ([`iban`]) — ISO 13616 mod-97 **and** the registered
//!   national BBAN structure, 89-country registry, SEPA membership
//! - **BIC validation** ([`bic`]) — ISO 9362:2022, including the country code
//! - **Creditor Identifier validation** ([`creditor_id`]) — EPC AT-02
//! - **Postal addresses** ([`address`]) — structured and hybrid `PstlAdr`
//! - **pain.001 builder** ([`pain001`]) — SEPA Credit Transfer + SCT Instant
//! - **pain.008 builder** ([`pain008`]) — SEPA Direct Debit (CORE + B2B)
//! - **pain.007 builder** ([`pain007`]) — SEPA Direct Debit reversal
//! - **pain.002 parser** ([`pain002`]) — Payment Status Report, incl. Verification of Payee
//! - **camt.055 builder** ([`camt055`]) — Payment Cancellation Request (recall)
//! - **camt.029 parser** ([`camt029`]) — Resolution of Investigation — the answer to a recall
//! - **camt.052 parser** ([`camt052`]) — Bank-to-Customer Report (intraday)
//! - **camt.053 parser** ([`camt053`]) — Bank-to-Customer Statement (end-of-day)
//! - **camt.054 parser** ([`camt054`]) — Bank-to-Customer Notification, returns
//! - **EPC field validation** ([`validate`]) — the rules the XSD does not enforce
//! - **SEPA character set** ([`charset`]) — validation and transliteration
//! - **Typed dates** ([`date`]) — [`IsoDate`] / [`IsoDateTime`] for every date field
//! - **Integer-safe money utilities** ([`ct_to_eur_str`], [`ct_from_eur_str`])
//!
//! All monetary amounts use `i64` cents (1 ct = 0.01 EUR) — **never `f64`**.
//! Every date is an [`IsoDate`] — never a hand-formatted string.
//!
//! **Dependencies:** [`thiserror`](https://crates.io/crates/thiserror) and
//! [`quick-xml`](https://crates.io/crates/quick-xml) (required);
//! [`serde`](https://crates.io/crates/serde),
//! [`time`](https://crates.io/crates/time) and
//! [`chrono`](https://crates.io/crates/chrono) (optional features).
//!
//! ## Schema versions
//!
//! The builders default to the versions the EPC 2023 rulebooks mandated from
//! 19 November 2023 — `pain.001.001.09` and `pain.008.001.08`. Which version a
//! bank actually requires varies by bank and by regulatory cutover, so it is a
//! per-message choice:
//!
//! | Message | Variants |
//! |---|---|
//! | pain.001 | `pain.001.001.09` (default), `pain.001.001.03`, `pain.001.003.03` |
//! | pain.008 | `pain.008.001.08` (default), `pain.008.001.02`, `pain.008.003.02` |
//! | pain.007 | `pain.007.001.09` — the only version SEPA defines |
//! | camt.055 / camt.029 | `camt.055.001.05` / `camt.029.001.06` — the pair the DFÜ-Abkommen names |
//!
//! Select one with [`Pain001Builder::schema`] / [`Pain008Builder::schema`].
//! Both enums implement `FromStr` over the message identifier and the namespace
//! URN, so the target version can come from configuration:
//!
//! ```
//! use sepa::pain008::DirectDebitSchema;
//! let schema: DirectDebitSchema = "pain.008.001.02".parse()?;
//! # Ok::<(), sepa::UnknownSchema>(())
//! ```
//!
//! Every generated document is validated against the real ISO 20022 schema in
//! CI, one test per version.
//!
//! ### These are deliberately not ISO's newest versions
//!
//! ISO advises using the most recent message definition available, and has
//! published `pain.001.001.13`, `pain.008.001.12` and `pain.002.001.15`. That
//! advice is aimed at communities free to choose their own version; a SEPA
//! participant is not one, because the version is fixed by the scheme rulebook.
//! Sending `pain.001.001.13` to a SEPA bank gets it rejected. The versions
//! above are the ones the EPC rulebooks have mandated since 19 November 2023,
//! The EPC's address migration is often mistaken for a message-version change
//! and is not one: it is about structured *addresses*, and the versions above
//! stay. A migration to a newer ISO 20022 version **is** now proposed, for
//! November 2029; it is a recommendation under consultation, not a rule, and it
//! does not change what to send today.
//!
//! ## Regulatory references
//!
//! | Standard | Module | Usage |
//! |---|---|---|
//! | ISO 13616-1 + SWIFT IBAN Registry | [`iban`] | IBAN validation + country-length registry |
//! | EPC409-09 v8.0 | [`iban`] | SEPA scheme country list |
//! | ISO 9362 | [`bic`] | BIC/SWIFT validation |
//! | ISO 3166-1 alpha-2 | [`country`] | Country codes for BICs and addresses |
//! | EPC153-22 v2.1 | [`address`] | Structured and hybrid addresses |
//! | EPC262-08 | [`creditor_id`] | Creditor Identifier check digits |
//! | ISO 20022 pain.001 | [`pain001`] | SEPA Credit Transfer (SCT + SCT Inst) |
//! | ISO 20022 pain.008 | [`pain008`] | SEPA Direct Debit (CORE + B2B) |
//! | ISO 20022 pain.007 | [`pain007`] | SEPA Direct Debit reversal |
//! | ISO 20022 pain.002 | [`pain002`] | Payment Status Report |
//! | EPC103-24 | [`pain002`] | Verification of Payee outcomes |
//! | ISO 20022 camt.052 | [`camt052`] | Bank-to-Customer Report (intraday) |
//! | ISO 20022 camt.053 | [`camt053`] | Bank-to-Customer Statement |
//! | ISO 20022 camt.054 | [`camt054`] | Payment notifications |
//! | ISO 20022 camt.055 | [`camt055`] | Payment Cancellation Request (recall) |
//! | ISO 20022 camt.029 | [`camt029`] | Resolution of Investigation |
//! | EPC217-08 | [`charset`] | SEPA character set + conversion table |
//! | EPC SEPA Rulebooks 2023/2025 | all | Governs all SEPA transactions |
//!
//! ## Quick start
//!
//! ```rust
//! use sepa::{
//!     CreditTransferEntry, CreditTransferGroup, DirectDebitEntry, DirectDebitGroup, IsoDate,
//!     Pain001Builder, Pain008Builder, SequenceType, validate_creditor_id, validate_iban,
//! };
//!
//! let iban = validate_iban("DE89 3704 0044 0532 0130 00")?;
//! assert_eq!(iban.as_str(), "DE89370400440532013000");
//! assert_eq!(iban.to_string(), "DE89 3704 0044 0532 0130 00");
//!
//! // pain.001 — Credit Transfer (Überweisung)
//! let ct_xml = Pain001Builder::new("Debtor GmbH", "CT-2026-07-001")
//!     .add_group(
//!         CreditTransferGroup::new("Debtor GmbH", &iban, IsoDate::new(2026, 7, 20)?)
//!             .add_entry(CreditTransferEntry::new(
//!                 "Supplier AG", iban.clone(), 12_000, "INV-2026-001",
//!             )),
//!     )
//!     .build()?;
//! assert!(ct_xml.contains("pain.001.001.09"));
//!
//! // pain.008 — a direct debit run carrying FRST and RCUR in one file.
//! let ci = validate_creditor_id("DE98ZZZ09999999999")?;
//! let dd_xml = Pain008Builder::new("Creditor GmbH", "DD-2026-07-001")
//!     .add_group(
//!         DirectDebitGroup::new("Creditor GmbH", &iban, &ci, IsoDate::new(2026, 7, 20)?)
//!             .sequence_type(SequenceType::Frst)
//!             .add_entry(DirectDebitEntry::new(
//!                 "MND-1", "2026-06-01".parse()?, "Neu Kunde", iban.clone(), 5_000, "R-001",
//!             )),
//!     )
//!     .add_group(
//!         DirectDebitGroup::new("Creditor GmbH", &iban, &ci, IsoDate::new(2026, 7, 18)?)
//!             .sequence_type(SequenceType::Rcur)
//!             .add_entry(DirectDebitEntry::new(
//!                 "MND-2", "2024-06-01".parse()?, "Alt Kunde", iban.clone(), 7_500, "R-002",
//!             )),
//!     )
//!     .build()?;
//! assert!(dd_xml.contains("<SeqTp>FRST</SeqTp>"));
//! assert!(dd_xml.contains("<SeqTp>RCUR</SeqTp>"));
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! ## Payment groups
//!
//! A message carries one or more [`CreditTransferGroup`] / [`DirectDebitGroup`]
//! blocks, each becoming a `PmtInf`. Sequence type, execution date, debtor
//! account, batch booking and category purpose all live at that level, so
//! several groups are what let a single file mix `FRST` with `RCUR`, or carry
//! two execution dates — rather than forcing a separate submission per
//! combination.
//!
//! ## Validation
//!
//! [`Pain001Builder::build`] and [`Pain008Builder::build`] return a
//! [`Result`]: they enforce the EPC field rules that the XSD does not — amount
//! range, identifier and name lengths, non-empty batches, schema-feature
//! compatibility — and by default transliterate text into the SEPA character
//! set, so `Müller & Söhne` is emitted as `Mueller + Soehne`. See [`validate`]
//! and [`charset`].
//!
//! Failures come back as a [`BuildError`]: a typed [`ValidationError`] naming
//! the ISO 20022 element, plus the [`Location`] — group and transaction index —
//! it came from, so a rejected collection run points at the row to fix.
//!
//! Two classes of mistake are absent from that list because they cannot fail
//! there — they are unrepresentable rather than caught late. [`IsoDate`]
//! validates at construction, so an impossible `ReqdColltnDt` never reaches a
//! batch; and [`PostalAddress`] requires a town and a country, so the
//! free-text-only address form the EPC schemes are retiring is not a value this
//! crate can be asked to emit.
//!
//! ## Nothing that matters is defaulted from a clock
//!
//! `MsgId` and the payment date are constructor arguments, because neither has
//! a safe default. `MsgId` is the key a bank de-duplicates submissions by, so
//! it has to come from a sequence that survives a restart. `ReqdExctnDt` and
//! `ReqdColltnDt` are the day money leaves an account, which depends on the
//! scheme, the sequence type, TARGET2 and the bank's cut-off — a
//! banking-calendar question this crate cannot answer.
//!
//! One implicit clock read remains: `GrpHdr/CreDtTm`. Pin it with
//! `created_at()` and a submitted file regenerates byte-for-byte.
//!
//! ## Undoing a payment
//!
//! Three messages undo one, and they are not interchangeable — picking the
//! wrong one wastes the window in which anything can still be done:
//!
//! | You want to | Message | Note |
//! |---|---|---|
//! | Stop a file you just sent | [`camt055`] | A **request**. The bank may refuse; the answer is [`camt029`] |
//! | Give back a collection that settled | [`pain007`] | An instruction, and only for a direct debit |
//! | Learn a payment came back | [`camt054`] | The bank telling you, after the fact |
//!
//! ```
//! use sepa::{Camt055Builder, CancellationEntry, CancellationGroup, CancellationReason,
//!            OriginalMessage, Pain008Builder, parse_camt029, validate_bic};
//!
//! # let submitted = Pain008Builder::new("Stadtwerke GmbH", "DD-2026-07-001");
//! let recall = Camt055Builder::new(
//!     "CXL-2026-07-001",
//!     "Stadtwerke GmbH",
//!     validate_bic("COBADEFFXXX")?,
//!     OriginalMessage::from_direct_debit(&submitted),
//! )
//! .add_group(
//!     CancellationGroup::new("PMT-2026-07-A")
//!         .add_entry(CancellationEntry::new("E2E-1", CancellationReason::Dupl)),
//! );
//! # let _ = recall;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! Nothing has been cancelled until the camt.029 says so, and `PDCR` — pending
//! — is neither answer. [`Camt029Document::is_final`] is what separates the two.
//!
//! [`Camt029Document::is_final`]: camt029::Camt029Document::is_final
//!
//! ## Reading bank files
//!
//! Every parser returns a `Result` with a typed error naming what it could not
//! read — a reconciliation import can always say *why* it skipped a row, not
//! just that it did. For a batch-booked camt entry, read
//! [`EntryDetail::signed_ct`] per transaction and check
//! [`CashEntry::details_reconcile`] before posting; [`CashEntry::batch`] carries
//! the `PmtInfId` of the group you submitted, which is what matches a booking
//! back to your own file.
//!
//! Bank input is kept verbatim and typed alongside, never in place of, the raw
//! value: [`CashEntry::booking_date`] returns an [`IsoDate`] and
//! `booking_date_raw` the text that arrived, so a non-conforming file is
//! readable rather than rejected.
//!
//! Two things a ledger needs are reported beside the amount rather than inside
//! it. [`CashEntry::charges`] carries the return fee on a bounced collection —
//! check [`Charges::all_included_in_amount`] before posting it, since a charge
//! already inside the entry amount must not be booked twice. And a `pain.002`
//! rejection explains itself at exactly one of three levels depending on how
//! far the bank got, so [`Pain002Document::reason_codes`] gathers all of them:
//! a submission refused outright carries its reason at group level with no
//! transaction blocks to inspect at all.
//!
//! [`EntryDetail::signed_ct`]: camt::EntryDetail::signed_ct
//! [`CashEntry::details_reconcile`]: camt::CashEntry::details_reconcile
//! [`CashEntry::booking_date`]: camt::CashEntry::booking_date
//! [`CashEntry::batch`]: camt::CashEntry::batch
//! [`CashEntry::charges`]: camt::CashEntry::charges
//! [`Charges::all_included_in_amount`]: camt::Charges::all_included_in_amount
//! [`Pain002Document::reason_codes`]: pain002::Pain002Document::reason_codes

// The panic-oriented lints (`unwrap_used`, `indexing_slicing`, …) guard the
// library's own code paths, where a panic on bank input is a real defect.
// Inside tests, `unwrap()` *is* the assertion, so they are relaxed there.
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
    )
)]

/// Compile-tests every example in `README.md`, so the README cannot drift out
/// of sync with the API.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
pub struct ReadmeDoctests;

/// Compile-tests every Rust example on the documentation site.
///
/// A published guide that no longer compiles is worse than no guide, so the
/// site's pages are run through the same doctest harness as the crate's own
/// documentation. Adding a page without adding it here is the one way a sample
/// could drift, which is why the list is explicit rather than a glob.
#[cfg(doctest)]
mod site_doctests {
    macro_rules! guide {
        ($name:ident, $path:literal) => {
            #[doc = include_str!($path)]
            pub struct $name;
        };
    }
    guide!(Landing, "../site/content/_index.md");
    guide!(GettingStarted, "../site/content/docs/getting-started.md");
    guide!(Identifiers, "../site/content/docs/identifiers.md");
    guide!(CreditTransfers, "../site/content/docs/credit-transfers.md");
    guide!(DirectDebits, "../site/content/docs/direct-debits.md");
    guide!(Reversals, "../site/content/docs/reversals.md");
    guide!(Recalls, "../site/content/docs/recalls.md");
    guide!(StatusReports, "../site/content/docs/status-reports.md");
    guide!(BankStatements, "../site/content/docs/bank-statements.md");
    guide!(Addresses, "../site/content/docs/addresses.md");
    guide!(Validation, "../site/content/docs/validation.md");
    guide!(SchemaVersions, "../site/content/docs/schema-versions.md");
}

pub mod address;
pub mod bic;
pub mod camt;
pub mod camt029;
pub mod camt052;
pub mod camt053;
pub mod camt054;
pub mod camt055;
pub mod charset;
mod charset_table;
pub mod country;
pub mod creditor_id;
pub mod currency;
pub mod date;
pub mod iban;
pub mod pain001;
pub mod pain002;
pub mod pain007;
pub mod pain008;
pub mod party;
pub mod purpose;
pub mod reference;
pub mod validate;
mod xml;
mod xml_util;

pub use address::{AddressError, AddressFormat, PostalAddress};
pub use bic::{Bic, BicError, BicPattern, validate_bic};
pub use camt::{
    AccountRef, BalanceType, BatchInfo, CashEntry, ChargeRecord, Charges, EntryDetail, EntryStatus,
    StatementBalance,
};
pub use camt029::{
    Camt029Document, Camt029ParseError, CancellationCount, CancellationStatus,
    GroupCancellationStatus, PaymentInfoCancellationStatus, RejectionReason, ResolutionOutcome,
    TransactionCancellationStatus, parse_camt029,
};
pub use camt052::{Camt052Document, Camt052ParseError, Camt052Report, parse_camt052};
pub use camt053::{Camt053Document, Camt053ParseError, Camt053Statement, parse_camt053};
pub use camt054::{
    Camt054Document, Camt054Notification, Camt054ParseError, CreditDebitIndicator,
    UnknownIndicator, parse_camt054,
};
pub use camt055::{
    Camt055Builder, CancellationEntry, CancellationGroup, CancellationReason, CaseParty,
    OriginalMessage,
};
pub use charset::{Transliteration, is_sepa_text, transliterate};
pub use country::is_country_code;
pub use creditor_id::{
    CreditorId, CreditorIdError, creditor_id_check_digits, validate_creditor_id,
};
pub use currency::{Currency, CurrencyError};
#[cfg(any(feature = "time", feature = "chrono"))]
pub use date::ConversionError;
pub use date::{DateError, DateTimeError, IsoDate, IsoDateTime};
pub use iban::{
    BbanCharClass, Iban, IbanError, iban_bban_format, iban_check_digits, iban_country_length,
    is_sepa_country, validate_iban,
};
pub use pain001::{
    ChargeBearer, CreditTransferEntry, CreditTransferGroup, CreditTransferKind,
    CreditTransferSchema, ExecutionMoment, Pain001Builder,
};
pub use pain002::{
    OriginalMessageType, Pain002Document, Pain002ParseError, PaymentInfoStatus, PaymentStatus,
    ReasonCode, StatusCount, TransactionStatus, VerificationOutcome, parse_pain002,
};
pub use pain007::{
    OriginalCollection, Pain007Builder, ReversalEntry, ReversalGroup, ReversalReason,
};
pub use pain008::{
    DirectDebitEntry, DirectDebitGroup, DirectDebitSchema, DirectDebitScheme, MandateAmendment,
    Pain008Builder, SequenceType, UnknownSequenceType,
};
pub use party::{IdentifierKind, Party, PartyIdentifier};
pub use purpose::{CategoryPurpose, Purpose, PurposeCodeError};
pub use reference::{RemittanceInfo, RfReference, RfReferenceError};
pub use validate::{
    BuildError, CharsetPolicy, Location, UnknownSchema, ValidationError, WriteError,
};
pub use xml::XmlError;

/// Format `ct` (1/100 EUR) as `"1234.56"` — pure integer arithmetic, no f64.
///
/// Uses integer division and modulo to produce exact decimal output.
/// `i64::MIN` is handled correctly via [`i64::unsigned_abs`].
///
/// # Examples
///
/// ```
/// use sepa::ct_to_eur_str;
/// assert_eq!(ct_to_eur_str(7500),    "75.00");
/// assert_eq!(ct_to_eur_str(1),       "0.01");
/// assert_eq!(ct_to_eur_str(100_000), "1000.00");
/// assert_eq!(ct_to_eur_str(-500),    "-5.00");
/// assert_eq!(ct_to_eur_str(0),       "0.00");
/// ```
#[inline]
#[must_use]
pub fn ct_to_eur_str(ct: i64) -> String {
    let sign = if ct < 0 { "-" } else { "" };
    let abs = ct.unsigned_abs();
    format!("{sign}{}.{:02}", abs / 100, abs % 100)
}

/// Error returned when a decimal amount string cannot be read as integer cents.
///
/// Amounts arrive from bank files and operator input, so a rejection needs a
/// reason an operator can act on — "this row's amount was `1,50`" is a
/// different problem from "this row's amount overflowed".
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum AmountError {
    /// The input was empty or contained only whitespace.
    #[error("amount is empty")]
    Empty,

    /// The input is not a decimal number — a thousands separator, a currency
    /// symbol, a comma decimal mark, or anything else non-numeric.
    #[error("{value:?} is not a decimal amount")]
    Malformed {
        /// The rejected text.
        value: String,
    },

    /// The value is a number but does not fit in `i64` cents.
    #[error("{value:?} does not fit in i64 cents")]
    Overflow {
        /// The rejected text.
        value: String,
    },

    /// The value carries significant digits below one cent.
    ///
    /// `ActiveOrHistoricCurrencyAndAmount` permits five fraction digits, so a
    /// bank can legally send `1.23456` in a camt statement even though the EPC
    /// restricts SEPA itself to two. This type counts whole cents and has
    /// nowhere to put the remainder, and **silently truncating it loses money**
    /// — a tenth of a cent on each of a million collections is a real number.
    ///
    /// Trailing zeros are not significant: `"1.500"` is 150 ct, not an error.
    #[error("{value:?} has {digits} significant fraction digits; ct holds 2")]
    SubCentPrecision {
        /// The rejected text.
        value: String,
        /// How many significant fraction digits it carries.
        digits: usize,
    },
}

/// Parse a `"1234.56"` EUR string into integer cents — pure integer arithmetic, no f64.
///
/// Accepts:
/// - Positive values: `"155.42"` → `15542`, and `"+155.42"` — a leading `+` is
///   legal `xs:decimal` and banks send it
/// - Negative values: `"-75.00"` → `-7500`
/// - Integer string: `"100"` → `10000`
/// - One decimal place: `"0.5"` → `50`
/// - Insignificant trailing zeros: `"1.500"` → `150`, `"1.23000"` → `123`
///
/// The grammar is exactly `[+-]?[0-9]*(\.[0-9]*)?` with at least one digit,
/// matching `xs:decimal`. A repeated sign and trailing junk are rejected rather
/// than silently reinterpreted — `i64::from_str` would accept both.
///
/// **A value with significant digits below one cent is rejected**, not
/// truncated: `"1.999"` is [`AmountError::SubCentPrecision`], not `199`. The
/// ISO schema permits five fraction digits, so this is reachable from a bank
/// file, and truncation would discard money without telling anyone.
///
/// # Errors
///
/// [`AmountError`], naming which of empty, malformed or overflowing input was
/// hit — so a skipped bank row can be logged with a reason rather than as a
/// bare `None`.
///
/// # Examples
///
/// ```
/// use sepa::{AmountError, ct_from_eur_str};
/// assert_eq!(ct_from_eur_str("155.42"), Ok(15542));
/// assert_eq!(ct_from_eur_str("-5.00"),  Ok(-500));
/// assert_eq!(ct_from_eur_str("100"),    Ok(10000));
/// // The whole `i64` range round-trips through `ct_to_eur_str`, ends included.
/// assert_eq!(ct_from_eur_str(&sepa::ct_to_eur_str(i64::MIN)), Ok(i64::MIN));
/// assert_eq!(ct_from_eur_str(""),       Err(AmountError::Empty));
/// assert!(matches!(ct_from_eur_str("1,50"), Err(AmountError::Malformed { .. })));
/// assert!(matches!(ct_from_eur_str("--5"), Err(AmountError::Malformed { .. })));
/// ```
#[inline]
pub fn ct_from_eur_str(s: &str) -> Result<i64, AmountError> {
    let malformed = || AmountError::Malformed {
        value: s.to_owned(),
    };
    let overflow = || AmountError::Overflow {
        value: s.to_owned(),
    };

    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Err(AmountError::Empty);
    }
    // `xs:decimal` permits an explicit `+`, and banks send it. Rejecting a
    // legal spelling is the same mistake as `validate_bic`'s six-letter
    // prefix: it fails on valid input, at the caller, with nothing the caller
    // can do. What stays rejected is a *repeated* sign.
    let (negative, magnitude) = match trimmed.split_at_checked(1) {
        Some(("-", rest)) => (true, rest),
        Some(("+", rest)) => (false, rest),
        _ => (false, trimmed),
    };

    // Split on the decimal point first, then require both halves to be bare
    // ASCII digits. Deferring to `i64::from_str` instead would accept a second
    // sign in either half: `"--5"` parsed as +5.00 EUR and `"1.-5"` turned
    // 1.50 EUR into 0.95 EUR. Checking the *whole* fractional part — not just
    // the two digits that survive truncation — also rejects `"1.50abc"`.
    let (euro_str, frac_str) = magnitude.split_once('.').unwrap_or((magnitude, ""));
    let digits = |part: &str| part.bytes().all(|b| b.is_ascii_digit());
    if (euro_str.is_empty() && frac_str.is_empty()) || !digits(euro_str) || !digits(frac_str) {
        return Err(malformed());
    }

    let euros: i64 = if euro_str.is_empty() {
        0
    } else {
        // Every byte is an ASCII digit, so the only way to fail is overflow.
        euro_str.parse().map_err(|_| overflow())?
    };
    // Anything below one cent has nowhere to go, so it is refused rather than
    // dropped. Trailing zeros carry no value and are not significant, which is
    // why `"1.500"` is 150 ct and `"1.999"` is an error. Every byte is an ASCII
    // digit here, so `get(..2)` and the parses cannot fail on content.
    let significant = frac_str.trim_end_matches('0');
    if significant.len() > 2 {
        return Err(AmountError::SubCentPrecision {
            value: s.to_owned(),
            digits: significant.len(),
        });
    }
    let cents: i64 = match significant.len() {
        0 => 0,
        1 => significant.parse::<i64>().map_err(|_| malformed())? * 10,
        _ => significant.parse().map_err(|_| malformed())?,
    };

    // Accumulate directly in the sign the input asked for. Building the
    // magnitude first and negating it afterwards costs the one value that has
    // no positive counterpart: `i64::MIN` is -92233720368547758.08 EUR, which
    // `ct_to_eur_str` happily prints and the parser then rejected as overflow.
    // A formatter and a parser that disagree about their own range is a bug
    // waiting for the row that hits it.
    if negative {
        euros
            .checked_mul(-100)
            .and_then(|e| e.checked_sub(cents))
            .ok_or_else(overflow)
    } else {
        euros
            .checked_mul(100)
            .and_then(|e| e.checked_add(cents))
            .ok_or_else(overflow)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ct_to_eur_roundtrip() {
        // Both ends included: `ct_to_eur_str` is total over `i64`, so
        // `ct_from_eur_str` has to be total over its output. Regression:
        // `i64::MIN` printed fine and came back as `Overflow`, because the
        // magnitude was built positive and negated afterwards.
        for ct in [
            0i64,
            1,
            99,
            100,
            1234,
            10_000,
            -1,
            -99,
            -100,
            i64::MAX,
            i64::MIN,
            i64::MIN + 1,
        ] {
            let s = ct_to_eur_str(ct);
            assert_eq!(ct_from_eur_str(&s), Ok(ct), "{ct} printed as {s:?}");
        }
    }

    #[test]
    fn one_past_each_end_still_overflows() {
        assert!(matches!(
            ct_from_eur_str("92233720368547758.08"),
            Err(AmountError::Overflow { .. })
        ));
        assert!(matches!(
            ct_from_eur_str("-92233720368547758.09"),
            Err(AmountError::Overflow { .. })
        ));
    }

    #[test]
    fn ct_from_eur_negatives() {
        assert_eq!(ct_from_eur_str("-5.00"), Ok(-500));
        assert_eq!(ct_from_eur_str("-0.01"), Ok(-1));
    }

    #[test]
    fn ct_from_eur_integer() {
        assert_eq!(ct_from_eur_str("100"), Ok(10_000));
    }

    #[test]
    fn ct_from_eur_never_panics_on_multibyte_fractions() {
        // Regression: `frac_str[..2]` panicked when byte 2 fell inside a
        // multi-byte character. Reachable from `parse_camt053` on a bank file,
        // because the XML layer decodes `&#8364;` to '€' before this sees it.
        for bad in ["1.€5", "1.5€", "0.ü9", "1.€€€", "-2.€1"] {
            assert!(
                matches!(ct_from_eur_str(bad), Err(AmountError::Malformed { .. })),
                "{bad:?} must be rejected, not panic"
            );
        }
        // Valid amounts still parse.
        assert_eq!(ct_from_eur_str("1.23"), Ok(123));
    }

    #[test]
    fn a_value_below_one_cent_is_refused_rather_than_truncated() {
        // `ActiveOrHistoricCurrencyAndAmount` permits five fraction digits, so
        // every one of these is reachable from a bank file. Truncating them —
        // which this crate did until 0.8 — discards money silently: `0.001`
        // became 0, and a tenth of a cent on each of a million collections is
        // a number somebody has to explain.
        for (bad, digits) in [("1.239", 3), ("0.001", 3), ("1.23456", 5), ("9.999", 3)] {
            assert!(
                matches!(
                    ct_from_eur_str(bad),
                    Err(AmountError::SubCentPrecision { digits: d, .. }) if d == digits
                ),
                "{bad:?} must be refused, got {:?}",
                ct_from_eur_str(bad)
            );
        }
        // Trailing zeros carry no value and are not significant.
        for (ok, ct) in [
            ("1.500", 150),
            ("1.23000", 123),
            ("2.10", 210),
            ("7.0", 700),
        ] {
            assert_eq!(ct_from_eur_str(ok), Ok(ct), "{ok:?} must parse");
        }
    }

    #[test]
    fn a_sign_is_accepted_once_and_only_at_the_front() {
        // Regression: `i64::from_str` accepts a leading sign wherever it is
        // handed one, so "1.-5" parsed its fraction as −5 cents and quietly
        // turned 1.50 into 0.95, while "--5" came back as +5.00 and "-+5"
        // as −5.00.
        for bad in [
            "1.-5", "1.+5", "1.-50", "-1.-5", "--5", "-+5", "+-5", "5-", "-", "+",
        ] {
            assert!(
                matches!(ct_from_eur_str(bad), Err(AmountError::Malformed { .. })),
                "{bad:?} must be rejected, got {:?}",
                ct_from_eur_str(bad)
            );
        }
        // A *single* leading `+` is legal `xs:decimal` and banks send it. It
        // used to be rejected here, which made every entry carrying one vanish
        // from the statement — a validator stricter than the standard, with the
        // failure hidden instead of reported.
        assert_eq!(ct_from_eur_str("+5"), Ok(500));
        assert_eq!(ct_from_eur_str("+5.00"), Ok(500));
        assert_eq!(ct_from_eur_str("+0.01"), Ok(1));
    }

    #[test]
    fn trailing_junk_after_the_cents_is_rejected() {
        // Regression: only the first two fractional characters were checked, so
        // "1.50abc" silently parsed as 1.50 EUR.
        for bad in ["1.50abc", "1.5x", "1.005 EUR", "100x", "1.2.3"] {
            assert!(
                matches!(ct_from_eur_str(bad), Err(AmountError::Malformed { .. })),
                "{bad:?} must be rejected, got {:?}",
                ct_from_eur_str(bad)
            );
        }
    }

    #[test]
    fn ct_from_eur_reports_why_it_failed() {
        assert_eq!(ct_from_eur_str(""), Err(AmountError::Empty));
        assert_eq!(ct_from_eur_str("   "), Err(AmountError::Empty));
        for bad in ["abc", "1.2.3", "1,50", "12 EUR", "-"] {
            assert!(
                matches!(ct_from_eur_str(bad), Err(AmountError::Malformed { .. })),
                "{bad:?} must be malformed"
            );
        }
        for big in ["92233720368547758.08", "99999999999999999999"] {
            assert!(
                matches!(ct_from_eur_str(big), Err(AmountError::Overflow { .. })),
                "{big:?} must overflow"
            );
        }
    }
}
