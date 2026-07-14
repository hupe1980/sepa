//! # sepa — SEPA payment utilities for Rust
//!
//! Provides:
//! - **IBAN validation** (ISO 13616 mod-97 + country-length registry)
//! - **BIC validation** (ISO 9362 format)
//! - **pain.008.003.02** XML builder — SEPA Direct Debit (CORE + B2B)
//! - **pain.001.003.03 / pain.001.001.09** XML builders — SEPA Credit Transfer + SCT Instant
//! - **pain.002** parser — Customer Payment Status Report (bank→customer)
//! - **camt.053** parser — Bank-to-Customer Statement (end-of-day)
//! - **camt.054** typed entry — Bank-to-Customer Notification
//! - **Integer-safe money utilities** ([`ct_to_eur_str`], [`ct_from_eur_str`])
//!
//! **Dependencies:** [`thiserror`](https://crates.io/crates/thiserror) (required);
//! [`serde`](https://crates.io/crates/serde) and
//! [`serde_json`](https://crates.io/crates/serde_json) (optional features).
//! All monetary amounts use `i64` cents (1 ct = 0.01 EUR) — **never `f64`**.
//!
//! ## Regulatory references
//!
//! | Standard | Module | Usage |
//! |---|---|---|
//! | ISO 13616-1 | [`iban`] | IBAN validation + country-length registry |
//! | ISO 9362 | [`bic`] | BIC/SWIFT validation |
//! | ISO 20022 pain.001 | [`pain001`] | SEPA Credit Transfer (SCT + SCT Inst) |
//! | ISO 20022 pain.008 | [`pain008`] | SEPA Direct Debit (CORE + B2B) |
//! | ISO 20022 pain.002 | [`pain002`] | Payment Status Report |
//! | ISO 20022 camt.053 | [`camt053`] | Bank-to-Customer Statement |
//! | ISO 20022 camt.054 | [`camt054`] | Payment notifications |
//! | EPC SEPA Rulebooks | all | Governs all SEPA transactions |
//!
//! ## Quick start
//!
//! ```rust
//! use sepa::{validate_iban, validate_bic, Pain008Builder, Pain001Builder};
//! use sepa::{DirectDebitEntry, CreditTransferEntry};
//! use sepa::pain008::SequenceType;
//! use sepa::pain001::LocalInstrument;
//!
//! let iban = validate_iban("DE89 3704 0044 0532 0130 00").unwrap();
//! assert_eq!(iban.as_str(), "DE89370400440532013000");
//! assert_eq!(iban.bban(), "370400440532013000");
//! assert_eq!(iban.to_string(), "DE89 3704 0044 0532 0130 00");
//!
//! // pain.008 — Direct Debit CORE (Lastschrift)
//! let _dd_xml = Pain008Builder::new("Creditor GmbH", &iban)
//!     .msg_id("DD-2026-07-001")
//!     .sequence_type(SequenceType::Rcur)
//!     .build_xml();
//!
//! // pain.001 — Credit Transfer (Überweisung)
//! let _ct_xml = Pain001Builder::new("Debtor GmbH", &iban)
//!     .msg_id("CT-2026-07-001")
//!     .build_xml();
//!
//! // pain.001 — SCT Instant (switches to pain.001.001.09 namespace automatically)
//! let _inst_xml = Pain001Builder::new("Debtor GmbH", &iban)
//!     .local_instrument(LocalInstrument::Inst)
//!     .build_xml();
//! ```

#![deny(unsafe_code)]
#![warn(missing_docs)]

pub mod bic;
pub mod camt053;
pub mod camt054;
pub mod creditor_id;
pub mod iban;
pub mod pain001;
pub mod pain002;
pub mod pain008;
mod xml_util;

pub use bic::{Bic, BicError, validate_bic};
pub use camt053::{
    BalanceType, Camt053Document, Camt053Entry, Camt053ParseError, Camt053Statement, EntryStatus,
    StatementBalance, parse_camt053,
};
pub use camt054::{Camt054Entry, CreditDebitIndicator, ReturnInfo, UnknownIndicator};
pub use creditor_id::{CreditorId, CreditorIdError, validate_creditor_id};
pub use iban::{Iban, IbanError, validate_iban};
pub use pain001::{CreditTransferEntry, CreditTransferSchema, LocalInstrument, Pain001Builder};
pub use pain002::{
    OriginalMessageType, Pain002Document, Pain002ParseError, PaymentInfoStatus, PaymentStatus,
    ReasonCode, TransactionStatus, parse_pain002,
};
pub use pain008::{
    DirectDebitEntry, DirectDebitScheme, Pain008Builder, SequenceType, UnknownSequenceType,
};

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

/// Parse a `"1234.56"` EUR string into integer cents — pure integer arithmetic, no f64.
///
/// Accepts:
/// - Positive values: `"155.42"` → `Some(15542)`
/// - Negative values: `"-75.00"` → `Some(-7500)`
/// - Integer string: `"100"` → `Some(10000)`
/// - One decimal place: `"0.5"` → `Some(50)`
///
/// Returns `None` on malformed input, overflow, or empty input.
/// Extra decimal places beyond 2 are truncated (not rounded).
///
/// # Examples
///
/// ```
/// use sepa::ct_from_eur_str;
/// assert_eq!(ct_from_eur_str("155.42"), Some(15542));
/// assert_eq!(ct_from_eur_str("-5.00"),  Some(-500));
/// assert_eq!(ct_from_eur_str("100"),    Some(10000));
/// assert_eq!(ct_from_eur_str("abc"),    None);
/// assert_eq!(ct_from_eur_str(""),       None);
/// ```
#[inline]
#[must_use]
pub fn ct_from_eur_str(s: &str) -> Option<i64> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return None;
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
            euro_str.parse().ok()?
        };
        let cents: i64 = match frac_str.len() {
            0 => 0,
            1 => frac_str.parse::<i64>().ok()?.checked_mul(10)?,
            _ => frac_str[..2].parse().ok()?,
        };
        euros.checked_mul(100)?.checked_add(cents)?
    } else {
        digits.parse::<i64>().ok()?.checked_mul(100)?
    };

    ct.checked_mul(sign)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ct_to_eur_roundtrip() {
        for ct in [0i64, 1, 99, 100, 1234, 10_000, i64::MAX / 100] {
            let s = ct_to_eur_str(ct);
            assert_eq!(ct_from_eur_str(&s), Some(ct));
        }
    }

    #[test]
    fn ct_from_eur_negatives() {
        assert_eq!(ct_from_eur_str("-5.00"), Some(-500));
        assert_eq!(ct_from_eur_str("-0.01"), Some(-1));
    }

    #[test]
    fn ct_from_eur_integer() {
        assert_eq!(ct_from_eur_str("100"), Some(10_000));
    }

    #[test]
    fn ct_from_eur_invalid() {
        assert_eq!(ct_from_eur_str(""), None);
        assert_eq!(ct_from_eur_str("abc"), None);
        assert_eq!(ct_from_eur_str("1.2.3"), None);
    }
}
