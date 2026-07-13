//! # sepa — SEPA payment utilities for Rust
//!
//! Provides:
//! - **IBAN validation** (ISO 13616 mod-97 algorithm)
//! - **BIC validation** (ISO 9362 format)
//! - **pain.008.003.02** XML builder — SEPA Core Direct Debit initiation (ISO 20022)
//! - **pain.001.003.03** XML builder — SEPA Credit Transfer initiation (ISO 20022)
//! - **CAMT.054** typed entry — Bank-to-Customer Debit/Credit Notification (ISO 20022)
//! - **Integer-safe money formatting** ([`ct_to_eur_str`])
//!
//! Zero required dependencies. Optional `serde` and `json` features.
//! All monetary amounts use `i64` cents (1 ct = 0.01 EUR) — **never `f64`**.
//!
//! ## Regulatory references
//!
//! | Standard | Module | Usage |
//! |---|---|---|
//! | ISO 13616-1 | [`iban`] | IBAN validation |
//! | ISO 9362 | [`bic`] | BIC validation |
//! | ISO 20022 pain.001 | [`pain001`] | SEPA Credit Transfer |
//! | ISO 20022 pain.008 | [`pain008`] | SEPA Core Direct Debit |
//! | ISO 20022 camt.054 | [`camt054`] | Payment notifications |
//! | EPC SEPA Rulebook | all | Governs all SEPA transactions |
//!
//! ## Quick start
//!
//! ```rust
//! use sepa::{validate_iban, validate_bic, Pain008Builder, Pain001Builder};
//! use sepa::{DirectDebitEntry, CreditTransferEntry};
//! use sepa::pain008::SequenceType;
//!
//! let iban = validate_iban("DE89 3704 0044 0532 0130 00").unwrap();
//! assert_eq!(iban.as_str(), "DE89370400440532013000");
//!
//! // pain.008 — Direct Debit (Lastschrift)
//! let _dd_xml = Pain008Builder::new("Creditor GmbH", &iban)
//!     .msg_id("DD-2026-07-001")
//!     .sequence_type(SequenceType::Rcur)
//!     .build_xml();
//!
//! // pain.001 — Credit Transfer (Überweisung)
//! let _ct_xml = Pain001Builder::new("Debtor GmbH", &iban)
//!     .msg_id("CT-2026-07-001")
//!     .build_xml();
//! ```

#![deny(unsafe_code)]
#![warn(missing_docs)]

pub mod bic;
pub mod camt054;
pub mod creditor_id;
pub mod iban;
pub mod pain001;
pub mod pain008;

pub use bic::{Bic, BicError, validate_bic};
pub use camt054::{Camt054Entry, CreditDebitIndicator, ReturnInfo, UnknownIndicator};
pub use creditor_id::{CreditorId, CreditorIdError, validate_creditor_id};
pub use iban::{Iban, IbanError, validate_iban};
pub use pain001::{CreditTransferEntry, Pain001Builder};
pub use pain008::{DirectDebitEntry, Pain008Builder, SequenceType, UnknownSequenceType};

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
