//! # sepa — SEPA payment utilities for Rust
//!
//! Provides:
//! - **IBAN validation** ([`iban`]) — ISO 13616 mod-97 **and** the registered
//!   national BBAN structure, 89-country registry, SEPA membership
//! - **BIC validation** ([`bic`]) — ISO 9362, including the country code
//! - **Creditor Identifier validation** ([`creditor_id`]) — EPC AT-02
//! - **pain.001 builder** ([`pain001`]) — SEPA Credit Transfer + SCT Instant
//! - **pain.008 builder** ([`pain008`]) — SEPA Direct Debit (CORE + B2B)
//! - **pain.002 parser** ([`pain002`]) — Customer Payment Status Report (bank→customer)
//! - **camt.053 parser** ([`camt053`]) — Bank-to-Customer Statement (end-of-day)
//! - **camt.054 types** ([`camt054`]) — Bank-to-Customer Notification
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
//! [`serde_json`](https://crates.io/crates/serde_json),
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
//! ## Regulatory references
//!
//! | Standard | Module | Usage |
//! |---|---|---|
//! | ISO 13616-1 + SWIFT IBAN Registry | [`iban`] | IBAN validation + country-length registry |
//! | EPC409-09 v8.0 | [`iban`] | SEPA scheme country list |
//! | ISO 9362 | [`bic`] | BIC/SWIFT validation |
//! | EPC262-08 | [`creditor_id`] | Creditor Identifier check digits |
//! | ISO 20022 pain.001 | [`pain001`] | SEPA Credit Transfer (SCT + SCT Inst) |
//! | ISO 20022 pain.008 | [`pain008`] | SEPA Direct Debit (CORE + B2B) |
//! | ISO 20022 pain.002 | [`pain002`] | Payment Status Report |
//! | ISO 20022 camt.053 | [`camt053`] | Bank-to-Customer Statement |
//! | ISO 20022 camt.054 | [`camt054`] | Payment notifications |
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
//! let ct_xml = Pain001Builder::new("Debtor GmbH")
//!     .msg_id("CT-2026-07-001")
//!     .add_group(
//!         CreditTransferGroup::new("Debtor GmbH", &iban)
//!             .execution_date(IsoDate::new(2026, 7, 20)?)
//!             .add_entry(CreditTransferEntry::new(
//!                 "Supplier AG", iban.clone(), 12_000, "INV-2026-001",
//!             )),
//!     )
//!     .build()?;
//! assert!(ct_xml.contains("pain.001.001.09"));
//!
//! // pain.008 — a direct debit run carrying FRST and RCUR in one file.
//! let ci = validate_creditor_id("DE98ZZZ09999999999")?;
//! let dd_xml = Pain008Builder::new("Creditor GmbH")
//!     .msg_id("DD-2026-07-001")
//!     .add_group(
//!         DirectDebitGroup::new("Creditor GmbH", &iban, &ci)
//!             .sequence_type(SequenceType::Frst)
//!             .collection_date(IsoDate::new(2026, 7, 20)?)
//!             .add_entry(DirectDebitEntry::new(
//!                 "MND-1", "2026-06-01".parse()?, "Neu Kunde", iban.clone(), 5_000, "R-001",
//!             )),
//!     )
//!     .add_group(
//!         DirectDebitGroup::new("Creditor GmbH", &iban, &ci)
//!             .sequence_type(SequenceType::Rcur)
//!             .collection_date(IsoDate::new(2026, 7, 18)?)
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
//! Dates are not on that list because they cannot fail there: [`IsoDate`]
//! validates at construction, so an impossible `ReqdColltnDt` is unrepresentable
//! rather than caught late.
//!
//! ## Reading bank files
//!
//! Every parser returns a `Result` with a typed error naming what it could not
//! read — a reconciliation import can always say *why* it skipped a row, not
//! just that it did. For a batch-booked camt entry, read
//! [`EntryDetail::signed_ct`] per transaction and check
//! [`CashEntry::details_reconcile`] before posting.
//!
//! Bank input is kept verbatim and typed alongside, never in place of, the raw
//! value: [`CashEntry::booking_date`] returns an [`IsoDate`] and
//! `booking_date_raw` the text that arrived, so a non-conforming file is
//! readable rather than rejected.
//!
//! [`EntryDetail::signed_ct`]: camt::EntryDetail::signed_ct
//! [`CashEntry::details_reconcile`]: camt::CashEntry::details_reconcile
//! [`CashEntry::booking_date`]: camt::CashEntry::booking_date

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

pub mod bic;
pub mod camt;
pub mod camt052;
pub mod camt053;
pub mod camt054;
pub mod charset;
mod charset_table;
pub mod creditor_id;
pub mod date;
pub mod iban;
pub mod pain001;
pub mod pain002;
pub mod pain008;
pub mod party;
pub mod purpose;
pub mod reference;
pub mod validate;
mod xml;
mod xml_util;

pub use bic::{Bic, BicError, is_bic_country_code, validate_bic};
pub use camt::{BalanceType, CashEntry, EntryDetail, EntryStatus, StatementBalance};
pub use camt052::{Camt052Document, Camt052ParseError, Camt052Report, parse_camt052};
pub use camt053::{Camt053Document, Camt053ParseError, Camt053Statement, parse_camt053};
pub use camt054::{
    Camt054Document, Camt054Notification, Camt054ParseError, CreditDebitIndicator, ReturnInfo,
    UnknownIndicator, parse_camt054,
};
#[cfg(feature = "json")]
pub use camt054::{SimpleJsonError, parse_simple_json};
pub use charset::{Transliteration, is_sepa_text, transliterate};
pub use creditor_id::{
    CreditorId, CreditorIdError, creditor_id_check_digits, validate_creditor_id,
};
pub use date::{DateError, DateTimeError, IsoDate, IsoDateTime};
pub use iban::{
    BbanCharClass, Iban, IbanError, iban_bban_format, iban_country_length, is_sepa_country,
    validate_iban,
};
pub use pain001::{
    CreditTransferEntry, CreditTransferGroup, CreditTransferSchema, LocalInstrument, Pain001Builder,
};
pub use pain002::{
    OriginalMessageType, Pain002Document, Pain002ParseError, PaymentInfoStatus, PaymentStatus,
    ReasonCode, TransactionStatus, parse_pain002,
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
}

/// Parse a `"1234.56"` EUR string into integer cents — pure integer arithmetic, no f64.
///
/// Accepts:
/// - Positive values: `"155.42"` → `15542`
/// - Negative values: `"-75.00"` → `-7500`
/// - Integer string: `"100"` → `10000`
/// - One decimal place: `"0.5"` → `50`
///
/// Extra decimal places beyond 2 are truncated (not rounded).
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
/// assert_eq!(ct_from_eur_str(""),       Err(AmountError::Empty));
/// assert!(matches!(ct_from_eur_str("1,50"), Err(AmountError::Malformed { .. })));
/// ```
#[inline]
pub fn ct_from_eur_str(s: &str) -> Result<i64, AmountError> {
    let malformed = || AmountError::Malformed {
        value: s.to_owned(),
    };
    let overflow = || AmountError::Overflow {
        value: s.to_owned(),
    };

    // `i64::from_str` reports "too many digits" and "not a number" through the
    // same error type; the caller needs to tell those apart.
    let classify = |e: &std::num::ParseIntError| {
        use std::num::IntErrorKind::{NegOverflow, PosOverflow};
        if matches!(e.kind(), PosOverflow | NegOverflow) {
            overflow()
        } else {
            malformed()
        }
    };

    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Err(AmountError::Empty);
    }
    let (sign, digits) = if let Some(rest) = trimmed.strip_prefix('-') {
        (-1i64, rest)
    } else {
        (1i64, trimmed)
    };

    let ct: i64 = if let Some(dot) = digits.find('.') {
        let euro_str = &digits[..dot];
        let frac_str = &digits[dot + 1..];
        let euros: i64 = if euro_str.is_empty() {
            0
        } else {
            euro_str.parse().map_err(|e| classify(&e))?
        };
        // `get(..2)` rather than `[..2]`: the fractional part comes from
        // bank-supplied XML, and a byte index of 2 can land inside a multi-byte
        // character (`"1.€5"`), which would panic. A non-boundary index yields
        // `None` here, which correctly rejects the amount instead.
        //
        // The digit check is not redundant with `parse`: `i64::from_str`
        // accepts a leading sign, so `"1.-5"` would otherwise parse as −5
        // cents and silently turn 1.50 EUR into 0.95 EUR.
        let frac = match frac_str.len() {
            0 | 1 => frac_str,
            _ => frac_str.get(..2).ok_or_else(malformed)?,
        };
        if !frac.bytes().all(|b| b.is_ascii_digit()) {
            return Err(malformed());
        }
        let cents: i64 = match frac.len() {
            0 => 0,
            1 => frac.parse::<i64>().map_err(|_| malformed())? * 10,
            _ => frac.parse().map_err(|_| malformed())?,
        };
        euros
            .checked_mul(100)
            .and_then(|e| e.checked_add(cents))
            .ok_or_else(overflow)?
    } else {
        digits
            .parse::<i64>()
            .map_err(|e| classify(&e))?
            .checked_mul(100)
            .ok_or_else(overflow)?
    };

    ct.checked_mul(sign).ok_or_else(overflow)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ct_to_eur_roundtrip() {
        for ct in [0i64, 1, 99, 100, 1234, 10_000, i64::MAX / 100] {
            let s = ct_to_eur_str(ct);
            assert_eq!(ct_from_eur_str(&s), Ok(ct));
        }
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
        // Valid amounts still parse, including 3+ decimals (truncated).
        assert_eq!(ct_from_eur_str("1.239"), Ok(123));
    }

    #[test]
    fn a_signed_fraction_is_not_a_subtraction() {
        // Regression: `i64::from_str` accepts a leading sign, so "1.-5" parsed
        // its fraction as −5 cents and quietly turned 1.50 into 0.95.
        for bad in ["1.-5", "1.+5", "1.-50", "-1.-5"] {
            assert!(
                matches!(ct_from_eur_str(bad), Err(AmountError::Malformed { .. })),
                "{bad:?} must be rejected"
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
