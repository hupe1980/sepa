//! CAMT.054 — Bank-to-Customer Debit/Credit Notification (ISO 20022).
//!
//! Typed structures for CAMT.054 entries as produced by German bank exports.
//!
//! ## What CAMT.054 contains
//!
//! Banks send CAMT.054 to notify account holders of:
//! - **Credit entries** (`CRDT`): incoming payments
//! - **Debit entries** (`DBIT`): outgoing payments, returned direct debits
//! - **Return information**: when a direct debit was returned (Rückbuchung)
//!
//! ## Amount sign convention
//!
//! CAMT.054 amounts are **always positive** with a separate [`CreditDebitIndicator`]
//! field for direction.  Call [`Camt054Entry::to_ledger_ct`] to convert to the
//! standard open-items sign convention (credit reduces outstanding balance).
//!
//! ## Example
//!
//! ```rust
//! # #[cfg(feature = "json")] {
//! use sepa::camt054::parse_simple_json;
//!
//! let json = serde_json::json!({
//!     "iban": "DE89370400440532013000",
//!     "amount_eur": "155.00",
//!     "reference": "Invoice 2026-06-001",
//!     "date": "2026-07-10"
//! });
//! let entry = parse_simple_json(&json).unwrap();
//! assert_eq!(entry.amount_ct, 15_500);
//! assert_eq!(entry.to_ledger_ct(), -15_500); // credit reduces balance
//! # }
//! ```

use std::str::FromStr;

use crate::ct_to_eur_str;

// ── CreditDebitIndicator ──────────────────────────────────────────────────────

/// Error returned when parsing a [`CreditDebitIndicator`] from a string.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown credit/debit indicator {0:?}: expected CRDT or DBIT")]
pub struct UnknownIndicator(
    /// The unrecognised code.
    pub String,
);

/// Whether a CAMT.054 entry is a credit or debit from the account holder's perspective.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum CreditDebitIndicator {
    /// Money received into the account (`CRDT`).
    #[cfg_attr(feature = "serde", serde(rename = "CRDT"))]
    Credit,
    /// Money debited from the account (`DBIT`).
    #[cfg_attr(feature = "serde", serde(rename = "DBIT"))]
    Debit,
}

impl CreditDebitIndicator {
    /// ISO 20022 code (`"CRDT"` or `"DBIT"`).
    #[inline]
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::Credit => "CRDT",
            Self::Debit => "DBIT",
        }
    }
}

impl std::fmt::Display for CreditDebitIndicator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_code())
    }
}

impl FromStr for CreditDebitIndicator {
    type Err = UnknownIndicator;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_uppercase().as_str() {
            "CRDT" => Ok(Self::Credit),
            "DBIT" => Ok(Self::Debit),
            _ => Err(UnknownIndicator(s.to_owned())),
        }
    }
}

impl TryFrom<&str> for CreditDebitIndicator {
    type Error = UnknownIndicator;
    fn try_from(s: &str) -> Result<Self, Self::Error> {
        s.parse()
    }
}

// ── ReturnInfo ────────────────────────────────────────────────────────────────

/// Return reason for a returned SEPA direct debit.
///
/// Common ISO 20022 return codes:
/// `AC01` incorrect account, `AC04` closed, `AC06` blocked,
/// `AM04` insufficient funds, `MD01` no mandate, `MD06` debtor revoked,
/// `MS02` unspecified.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ReturnInfo {
    /// ISO 20022 return reason code (e.g. `"MD01"`, `"AM04"`).
    pub reason_code: String,
    /// Optional additional information from the bank.
    pub additional_info: Option<String>,
}

impl ReturnInfo {
    /// Create a new `ReturnInfo` with the given reason code.
    pub fn new(reason_code: impl Into<String>) -> Self {
        Self {
            reason_code: reason_code.into(),
            additional_info: None,
        }
    }

    /// Set optional additional information.
    #[must_use]
    pub fn with_additional_info(mut self, info: impl Into<String>) -> Self {
        self.additional_info = Some(info.into());
        self
    }
}

// ── Camt054Entry ──────────────────────────────────────────────────────────────

/// A single entry from a CAMT.054 bank notification.
///
/// Amounts are **always positive** with a separate [`CreditDebitIndicator`].
/// Use [`to_ledger_ct`](Self::to_ledger_ct) to convert to open-items sign convention.
#[derive(Debug, Clone)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Camt054Entry {
    /// Account IBAN for which this entry applies.
    pub iban: String,
    /// Amount in ct (1/100 EUR). Always positive — see `indicator` for direction.
    pub amount_ct: i64,
    /// Whether this is a credit (incoming) or debit (outgoing).
    pub indicator: CreditDebitIndicator,
    /// Value date (Wertstellungsdatum), ISO 8601 `"YYYY-MM-DD"`.
    pub value_date: String,
    /// Booking date (Buchungsdatum), ISO 8601. May differ from `value_date`.
    pub booking_date: Option<String>,
    /// Payment reference / Verwendungszweck.
    pub reference: String,
    /// End-to-end reference from the original payment instruction.
    pub end_to_end_id: Option<String>,
    /// Return reason when this is a returned direct debit.
    pub return_reason: Option<ReturnInfo>,
    /// Counterparty name (payer for credits, payee for debits).
    pub counterparty_name: Option<String>,
    /// Counterparty IBAN.
    pub counterparty_iban: Option<String>,
}

impl Camt054Entry {
    /// Convert to open-items ledger sign convention:
    /// - Credit → **negative** ct (reduces outstanding balance)
    /// - Debit  → **positive** ct (increases outstanding balance)
    #[inline]
    #[must_use]
    pub const fn to_ledger_ct(&self) -> i64 {
        match self.indicator {
            CreditDebitIndicator::Credit => -self.amount_ct,
            CreditDebitIndicator::Debit => self.amount_ct,
        }
    }

    /// Returns `true` if this entry is a returned SEPA direct debit (Rückbuchung).
    #[inline]
    #[must_use]
    pub const fn is_return(&self) -> bool {
        self.return_reason.is_some()
    }

    /// Human-readable description for the entry.
    #[must_use]
    pub fn description(&self) -> String {
        self.return_reason.as_ref().map_or_else(
            || match self.indicator {
                CreditDebitIndicator::Credit => format!("CAMT.054 Zahlung: {}", self.reference),
                CreditDebitIndicator::Debit => format!("CAMT.054 Abbuchung: {}", self.reference),
            },
            |r| {
                format!(
                    "SEPA-Rückläufer {} ({})",
                    r.reason_code,
                    r.additional_info.as_deref().unwrap_or("Rückgabe"),
                )
            },
        )
    }

    /// Format the amount as a `"1234.56"` EUR string (no f64).
    #[inline]
    #[must_use]
    pub fn amount_eur_str(&self) -> String {
        ct_to_eur_str(self.amount_ct)
    }
}

// ── parse_simple_json ─────────────────────────────────────────────────────────

/// Parse a simplified CAMT.054 JSON record from a bank CSV/JSON export.
///
/// Expected fields:
/// ```json
/// {
///   "iban":       "DE89...",
///   "amount_eur": "155.00",
///   "reference":  "...",
///   "date":       "YYYY-MM-DD"
/// }
/// ```
///
/// - `amount_eur` may be a string `"155.42"` or a JSON number.
///   Positive = credit, negative = debit.
/// - Parsed with integer arithmetic only — **no f64 rounding**.
///
/// Returns `None` when required fields are missing, unparseable, or would overflow.
#[cfg(feature = "json")]
pub fn parse_simple_json(value: &serde_json::Value) -> Option<Camt054Entry> {
    let iban = value.get("iban").and_then(|v| v.as_str())?.to_owned();
    let date = value
        .get("date")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_owned();
    let reference = value
        .get("reference")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_owned();

    let amount_ct: i64 = parse_amount_ct(value.get("amount_eur")?)?;

    let (amount_ct, indicator) = if amount_ct >= 0 {
        (amount_ct, CreditDebitIndicator::Credit)
    } else {
        (amount_ct.checked_neg()?, CreditDebitIndicator::Debit)
    };

    Some(Camt054Entry {
        iban,
        amount_ct,
        indicator,
        value_date: date.clone(),
        booking_date: Some(date),
        reference,
        end_to_end_id: None,
        return_reason: None,
        counterparty_name: value
            .get("counterparty_name")
            .and_then(|v: &serde_json::Value| v.as_str())
            .map(str::to_owned),
        counterparty_iban: value
            .get("counterparty_iban")
            .and_then(|v: &serde_json::Value| v.as_str())
            .map(str::to_owned),
    })
}

/// Parse `amount_eur` JSON value to integer cents without f64.
///
/// Accepts string `"155.42"`, negative `"-75.00"`, integer string `"100"`,
/// and JSON numbers (converted via `.to_string()` to avoid f64 rounding).
/// Returns `None` on overflow or parse failure.
#[cfg(feature = "json")]
fn parse_amount_ct(val: &serde_json::Value) -> Option<i64> {
    let raw = match val {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        _ => return None,
    };
    let trimmed = raw.trim();
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
        // Truncate fractional part to 2 decimal places
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

    fn credit_entry() -> Camt054Entry {
        Camt054Entry {
            iban: "DE89370400440532013000".to_owned(),
            amount_ct: 15_500,
            indicator: CreditDebitIndicator::Credit,
            value_date: "2026-07-10".to_owned(),
            booking_date: Some("2026-07-10".to_owned()),
            reference: "Invoice 2026-06-001".to_owned(),
            end_to_end_id: None,
            return_reason: None,
            counterparty_name: Some("Max Mustermann".to_owned()),
            counterparty_iban: None,
        }
    }

    #[test]
    fn credit_to_ledger_ct_is_negative() {
        assert_eq!(credit_entry().to_ledger_ct(), -15_500);
    }

    #[test]
    fn debit_to_ledger_ct_is_positive() {
        let mut entry = credit_entry();
        entry.indicator = CreditDebitIndicator::Debit;
        assert_eq!(entry.to_ledger_ct(), 15_500);
    }

    #[test]
    fn is_return_false_by_default() {
        assert!(!credit_entry().is_return());
    }

    #[test]
    fn return_info_constructor() {
        let r = ReturnInfo::new("MD01").with_additional_info("No mandate");
        assert_eq!(r.reason_code, "MD01");
        assert_eq!(r.additional_info.as_deref(), Some("No mandate"));
    }

    #[test]
    fn is_return_true_with_reason() {
        let mut entry = credit_entry();
        entry.indicator = CreditDebitIndicator::Debit;
        entry.return_reason = Some(ReturnInfo::new("MD01"));
        assert!(entry.is_return());
        assert!(entry.description().contains("MD01"));
    }

    #[test]
    fn amount_eur_str_no_f64() {
        assert_eq!(credit_entry().amount_eur_str(), "155.00");
    }

    #[test]
    fn indicator_display_and_parse() {
        assert_eq!(CreditDebitIndicator::Credit.to_string(), "CRDT");
        assert_eq!(CreditDebitIndicator::Debit.to_string(), "DBIT");
        assert_eq!(
            "CRDT".parse::<CreditDebitIndicator>().unwrap(),
            CreditDebitIndicator::Credit
        );
        assert_eq!(
            "dbit".parse::<CreditDebitIndicator>().unwrap(),
            CreditDebitIndicator::Debit
        );
        assert!("INVALID".parse::<CreditDebitIndicator>().is_err());
    }

    #[test]
    fn credit_debit_indicator_codes() {
        assert_eq!(CreditDebitIndicator::Credit.as_code(), "CRDT");
        assert_eq!(CreditDebitIndicator::Debit.as_code(), "DBIT");
    }

    #[cfg(feature = "json")]
    #[test]
    fn parse_json_credit_string_amount() {
        let json = serde_json::json!({
            "iban": "DE89370400440532013000",
            "amount_eur": "155.42",
            "reference": "Ref001",
            "date": "2026-07-10"
        });
        let entry = parse_simple_json(&json).unwrap();
        assert_eq!(entry.amount_ct, 15_542);
        assert_eq!(entry.indicator, CreditDebitIndicator::Credit);
        assert_eq!(entry.to_ledger_ct(), -15_542);
    }

    #[cfg(feature = "json")]
    #[test]
    fn parse_json_debit_negative_amount() {
        let json = serde_json::json!({
            "iban": "DE89370400440532013000",
            "amount_eur": "-75.00",
            "reference": "Ref002",
            "date": "2026-07-10"
        });
        let entry = parse_simple_json(&json).unwrap();
        assert_eq!(entry.amount_ct, 7_500);
        assert_eq!(entry.indicator, CreditDebitIndicator::Debit);
        assert_eq!(entry.to_ledger_ct(), 7_500);
    }

    #[cfg(feature = "json")]
    #[test]
    fn parse_json_amount_no_decimal() {
        let json = serde_json::json!({
            "iban": "DE89370400440532013000",
            "amount_eur": "100",
            "reference": "Ref003",
            "date": "2026-07-10"
        });
        let entry = parse_simple_json(&json).unwrap();
        assert_eq!(entry.amount_ct, 10_000);
    }

    #[cfg(feature = "json")]
    #[test]
    fn parse_json_amount_fractional_only() {
        // ".42" edge case
        let json = serde_json::json!({
            "iban": "DE89370400440532013000",
            "amount_eur": ".42",
            "reference": "R",
            "date": "2026-07-10"
        });
        let entry = parse_simple_json(&json).unwrap();
        assert_eq!(entry.amount_ct, 42);
    }

    #[cfg(feature = "json")]
    #[test]
    fn parse_json_missing_iban_returns_none() {
        let json =
            serde_json::json!({ "amount_eur": "100", "reference": "R", "date": "2026-07-10" });
        assert!(parse_simple_json(&json).is_none());
    }
}
