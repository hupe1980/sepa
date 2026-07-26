//! ISO 20022 camt.054 — Bank-to-Customer Debit/Credit Notification.
//!
//! camt.054 notifies an account holder of specific debit and credit events. It
//! carries **no balances** — for an account position use
//! [`camt.053`](crate::camt053) (end-of-day) or
//! [`camt.052`](crate::camt052) (intraday).
//!
//! Its most important use is **returns**: this is where a bank reports that a
//! direct debit collection came back. See [`Camt054Notification::returns`].
//!
//! Entries use the shared [`CashEntry`](crate::camt::CashEntry) model, so the
//! same reconciliation code works across camt.052, camt.053 and camt.054.
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
//! # #[cfg(feature = "json")]
//! # fn demo() -> Result<(), sepa::camt054::SimpleJsonError> {
//! use sepa::camt054::parse_simple_json;
//!
//! let json = serde_json::json!({
//!     "iban": "DE89370400440532013000",
//!     "amount_eur": "155.00",
//!     "reference": "Invoice 2026-06-001",
//!     "date": "2026-07-10"
//! });
//! let entry = parse_simple_json(&json)?;
//! assert_eq!(entry.amount_ct, 15_500);
//! assert_eq!(entry.to_ledger_ct(), -15_500); // credit reduces balance
//! # Ok(())
//! # }
//! # #[cfg(feature = "json")]
//! # demo().unwrap();
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
///
/// Defaults to [`Credit`](Self::Credit), matching how the parsers treat an
/// absent or unrecognised `CdtDbtInd`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum CreditDebitIndicator {
    /// Money received into the account (`CRDT`).
    #[default]
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
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Camt054Entry {
    /// Account IBAN for which this entry applies.
    pub iban: String,
    /// Amount in ct (1/100 EUR). Always positive — see `indicator` for direction.
    pub amount_ct: i64,
    /// Whether this is a credit (incoming) or debit (outgoing).
    pub indicator: CreditDebitIndicator,
    /// Value date (Wertstellungsdatum) exactly as reported.
    ///
    /// Read [`value_date`](Self::value_date) for the day; this keeps whatever
    /// the source actually contained.
    pub value_date_raw: String,
    /// Booking date (Buchungsdatum) exactly as reported. May differ from the
    /// value date.
    pub booking_date_raw: Option<String>,
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

    /// The value date — the day this entry is posted by.
    ///
    /// `None` only when the source carried something this crate cannot read as
    /// a date; [`value_date_raw`](Self::value_date_raw) still has it.
    #[must_use]
    pub fn value_date(&self) -> Option<crate::IsoDate> {
        crate::IsoDate::parse_date_part(&self.value_date_raw).ok()
    }

    /// The booking date, when one was reported.
    #[must_use]
    pub fn booking_date(&self) -> Option<crate::IsoDate> {
        crate::IsoDate::parse_date_part(self.booking_date_raw.as_deref()?).ok()
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

// ── camt.054 XML document ─────────────────────────────────────────────────────

/// A single notification within a camt.054 document.
///
/// camt.054 carries no balances — it notifies about specific debit/credit
/// events rather than reporting an account position.
#[derive(Debug, Clone)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Camt054Notification {
    /// Notification identifier (`Id`).
    pub notification_id: String,
    /// Account IBAN.
    pub account_iban: String,
    /// BIC of the account servicing institution (`Acct/Svcr`), if reported.
    pub account_servicer_bic: Option<String>,
    /// Notification period start, ISO 8601.
    pub from_date: Option<String>,
    /// Notification period end, ISO 8601.
    pub to_date: Option<String>,
    /// The notified entries.
    pub entries: Vec<crate::camt::CashEntry>,
}

impl Camt054Notification {
    /// Net movement in ct across all entries.
    #[must_use]
    pub fn net_movement_ct(&self) -> i64 {
        self.entries
            .iter()
            .fold(0i64, |acc, e| acc.saturating_add(e.signed_ct()))
    }

    /// Entries that are SEPA returns (Rückläufer) — a returned direct debit or
    /// credit transfer.
    ///
    /// This is the main reason to consume camt.054: it is where a bank reports
    /// that a collection came back.
    pub fn returns(&self) -> impl Iterator<Item = &crate::camt::CashEntry> {
        self.entries.iter().filter(|e| e.is_return())
    }
}

/// A parsed camt.054 Bank-to-Customer Debit/Credit Notification document.
#[derive(Debug, Clone)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Camt054Document {
    /// Document message ID.
    pub msg_id: String,
    /// Document creation timestamp.
    pub created_at: String,
    /// Detected XML namespace URI.
    pub namespace: Option<String>,
    /// One or more notifications.
    pub notifications: Vec<Camt054Notification>,
}

/// Error returned when camt.054 XML cannot be parsed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum Camt054ParseError {
    /// The input is not well-formed XML.
    #[error(transparent)]
    Xml(#[from] crate::xml::XmlError),

    /// Root element `BkToCstmrDbtCdtNtfctn` not found — not a camt.054 document.
    #[error("not a camt.054 document: root element <BkToCstmrDbtCdtNtfctn> not found")]
    NotCamt054,
}

/// Parse a camt.054 Bank-to-Customer Debit/Credit Notification XML string.
///
/// Accepts every ISO version from `.001.02` to `.001.13`, including the
/// `.001.07` reshaping of `Ntry/Sts` and of the party elements, and both the
/// default-namespace and prefixed document shapes.
///
/// # Errors
///
/// Returns [`Camt054ParseError::NotCamt054`] when the root element is missing,
/// or [`Camt054ParseError::Xml`] when the input is not well-formed.
///
/// # Examples
///
/// ```
/// use sepa::camt054::parse_camt054;
///
/// let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
/// <Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.054.001.08">
///   <BkToCstmrDbtCdtNtfctn>
///     <GrpHdr><MsgId>NTF-1</MsgId><CreDtTm>2026-07-14T12:00:00</CreDtTm></GrpHdr>
///     <Ntfctn>
///       <Id>N-1</Id>
///       <Acct><Id><IBAN>DE89370400440532013000</IBAN></Id></Acct>
///       <Ntry>
///         <Amt Ccy="EUR">75.00</Amt>
///         <CdtDbtInd>DBIT</CdtDbtInd>
///         <NtryDtls><TxDtls>
///           <RtrInf><Rsn><Cd>MD01</Cd></Rsn></RtrInf>
///         </TxDtls></NtryDtls>
///       </Ntry>
///     </Ntfctn>
///   </BkToCstmrDbtCdtNtfctn>
/// </Document>"#;
///
/// let doc = parse_camt054(xml)?;
/// let returned: Vec<_> = doc.notifications[0].returns().collect();
/// assert_eq!(returned.len(), 1);
/// assert_eq!(returned[0].return_reason_code(), Some("MD01"));
/// # Ok::<(), sepa::Camt054ParseError>(())
/// ```
pub fn parse_camt054(xml: &str) -> Result<Camt054Document, Camt054ParseError> {
    use crate::camt;
    use crate::xml::{Document, Node};

    fn parse_notification(n: &Node) -> Camt054Notification {
        let (from_date, to_date) = camt::period(n);
        Camt054Notification {
            notification_id: n.text_of("Id").unwrap_or_default().to_owned(),
            account_iban: camt::account_iban(n),
            account_servicer_bic: n
                .path(&["Acct", "Svcr"])
                .and_then(camt::agent_bic)
                .map(str::to_owned),
            from_date,
            to_date,
            entries: camt::entries_of(n),
        }
    }

    let doc = Document::parse(xml)?;
    let root = doc
        .root
        .child("BkToCstmrDbtCdtNtfctn")
        .ok_or(Camt054ParseError::NotCamt054)?;

    let grp_hdr = root.child("GrpHdr");
    let text = |tag: &str| {
        grp_hdr
            .and_then(|h| h.text_of(tag))
            .unwrap_or_default()
            .to_owned()
    };

    Ok(Camt054Document {
        msg_id: text("MsgId"),
        created_at: text("CreDtTm"),
        namespace: doc.namespace,
        notifications: root
            .children_named("Ntfctn")
            .map(parse_notification)
            .collect(),
    })
}

// ── parse_simple_json ─────────────────────────────────────────────────────────

/// Error returned when a simplified CAMT.054 JSON record cannot be read.
///
/// Every variant names the offending field, so a bank row that gets skipped on
/// import can be reported with the reason it was skipped — a truncated file, a
/// German-formatted amount and a missing IBAN are three different operational
/// problems and must not look alike in a log.
#[cfg(feature = "json")]
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SimpleJsonError {
    /// The record is not a JSON object.
    #[error("expected a JSON object, found {found}")]
    NotAnObject {
        /// The JSON type that was found instead.
        found: &'static str,
    },

    /// A required field is absent.
    #[error("required field {field:?} is missing")]
    MissingField {
        /// The field name.
        field: &'static str,
    },

    /// A field is present but of the wrong JSON type.
    #[error("field {field:?} must be {expected}")]
    WrongType {
        /// The field name.
        field: &'static str,
        /// What the field should have been.
        expected: &'static str,
    },

    /// A field is present but empty.
    #[error("field {field:?} must not be empty")]
    Empty {
        /// The field name.
        field: &'static str,
    },

    /// The amount could not be read as integer cents.
    #[error("field {field:?}: {source}")]
    InvalidAmount {
        /// The field name.
        field: &'static str,
        /// Why the amount was rejected.
        #[source]
        source: crate::AmountError,
    },

    /// The date is not a valid ISO 8601 calendar date.
    #[error("field {field:?}: {source}")]
    InvalidDate {
        /// The field name.
        field: &'static str,
        /// Why the date was rejected.
        #[source]
        source: crate::DateError,
    },
}

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
/// - `iban`, `amount_eur` and `date` are required; `reference`,
///   `counterparty_name` and `counterparty_iban` are optional.
/// - `amount_eur` may be a string `"155.42"` or a JSON number.
///   Positive = credit, negative = debit.
/// - Parsed with integer arithmetic only — **no f64 rounding**.
///
/// # Errors
///
/// [`SimpleJsonError`], naming the field and the reason. A reconciliation
/// import can therefore say *why* it dropped a row rather than only that it
/// did.
///
/// # Examples
///
/// ```
/// use sepa::camt054::{SimpleJsonError, parse_simple_json};
///
/// let ok = serde_json::json!({
///     "iban": "DE89370400440532013000",
///     "amount_eur": "155.00",
///     "date": "2026-07-10",
/// });
/// assert_eq!(parse_simple_json(&ok)?.amount_ct, 15_500);
///
/// // A German-formatted amount is rejected with the field that caused it.
/// let bad = serde_json::json!({
///     "iban": "DE89370400440532013000",
///     "amount_eur": "155,00",
///     "date": "2026-07-10",
/// });
/// let err = parse_simple_json(&bad).unwrap_err();
/// assert!(matches!(err, SimpleJsonError::InvalidAmount { field: "amount_eur", .. }));
/// assert_eq!(
///     err.to_string(),
///     r#"field "amount_eur": "155,00" is not a decimal amount"#,
/// );
/// # Ok::<(), SimpleJsonError>(())
/// ```
#[cfg(feature = "json")]
pub fn parse_simple_json(value: &serde_json::Value) -> Result<Camt054Entry, SimpleJsonError> {
    use serde_json::Value;

    if !value.is_object() {
        return Err(SimpleJsonError::NotAnObject {
            found: json_type_name(value),
        });
    }

    let required_str = |field: &'static str| -> Result<String, SimpleJsonError> {
        let raw = value
            .get(field)
            .ok_or(SimpleJsonError::MissingField { field })?
            .as_str()
            .ok_or(SimpleJsonError::WrongType {
                field,
                expected: "a string",
            })?
            .trim();
        if raw.is_empty() {
            return Err(SimpleJsonError::Empty { field });
        }
        Ok(raw.to_owned())
    };
    let optional_str = |field: &'static str| -> Result<Option<String>, SimpleJsonError> {
        match value.get(field) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(s)) => Ok(Some(s.clone())),
            Some(_) => Err(SimpleJsonError::WrongType {
                field,
                expected: "a string",
            }),
        }
    };

    let iban = required_str("iban")?;

    // Validated rather than passed through: a value date is what a reconciled
    // posting is dated by, so a malformed one has to surface here.
    let date_raw = required_str("date")?;
    crate::IsoDate::parse(&date_raw).map_err(|source| SimpleJsonError::InvalidDate {
        field: "date",
        source,
    })?;

    let amount_ct = parse_amount_ct(value.get("amount_eur").ok_or(
        SimpleJsonError::MissingField {
            field: "amount_eur",
        },
    )?)?;

    // A magnitude plus a direction, as camt.054 itself reports it. `i64::MIN`
    // has no positive counterpart, so it is an overflow rather than a debit.
    let (amount_ct, indicator) = if amount_ct >= 0 {
        (amount_ct, CreditDebitIndicator::Credit)
    } else {
        let magnitude = amount_ct
            .checked_neg()
            .ok_or_else(|| SimpleJsonError::InvalidAmount {
                field: "amount_eur",
                source: crate::AmountError::Overflow {
                    value: amount_ct.to_string(),
                },
            })?;
        (magnitude, CreditDebitIndicator::Debit)
    };

    Ok(Camt054Entry {
        iban,
        amount_ct,
        indicator,
        value_date_raw: date_raw.clone(),
        booking_date_raw: Some(date_raw),
        reference: optional_str("reference")?.unwrap_or_default(),
        end_to_end_id: optional_str("end_to_end_id")?,
        return_reason: None,
        counterparty_name: optional_str("counterparty_name")?,
        counterparty_iban: optional_str("counterparty_iban")?,
    })
}

#[cfg(feature = "json")]
const fn json_type_name(v: &serde_json::Value) -> &'static str {
    match v {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "a boolean",
        serde_json::Value::Number(_) => "a number",
        serde_json::Value::String(_) => "a string",
        serde_json::Value::Array(_) => "an array",
        serde_json::Value::Object(_) => "an object",
    }
}

/// Parse an `amount_eur` JSON value to integer cents without f64.
///
/// Accepts string `"155.42"`, negative `"-75.00"`, integer string `"100"`,
/// and JSON numbers (converted via `.to_string()` to avoid f64 rounding).
#[cfg(feature = "json")]
fn parse_amount_ct(val: &serde_json::Value) -> Result<i64, SimpleJsonError> {
    const FIELD: &str = "amount_eur";
    let raw = match val {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        other => {
            return Err(SimpleJsonError::WrongType {
                field: FIELD,
                expected: match other {
                    serde_json::Value::Null => "a decimal string or number, not null",
                    _ => "a decimal string or number",
                },
            });
        }
    };
    crate::ct_from_eur_str(&raw).map_err(|source| SimpleJsonError::InvalidAmount {
        field: FIELD,
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn credit_entry() -> Camt054Entry {
        Camt054Entry {
            iban: "DE89370400440532013000".to_owned(),
            amount_ct: 15_500,
            indicator: CreditDebitIndicator::Credit,
            value_date_raw: "2026-07-10".to_owned(),
            booking_date_raw: Some("2026-07-10".to_owned()),
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
    fn every_rejection_says_which_field_and_why() {
        // The point of the Result: an import that skips a bank row has to be
        // able to tell the operator what was wrong with it.
        let base = serde_json::json!({
            "iban": "DE89370400440532013000",
            "amount_eur": "100.00",
            "date": "2026-07-10",
        });
        let with = |field: &str, v: serde_json::Value| {
            let mut j = base.clone();
            j[field] = v;
            j
        };
        let without = |field: &str| {
            let mut j = base.clone();
            j.as_object_mut().unwrap().remove(field);
            j
        };

        assert_eq!(
            parse_simple_json(&without("iban")),
            Err(SimpleJsonError::MissingField { field: "iban" })
        );
        assert_eq!(
            parse_simple_json(&without("amount_eur")),
            Err(SimpleJsonError::MissingField {
                field: "amount_eur"
            })
        );
        assert_eq!(
            parse_simple_json(&without("date")),
            Err(SimpleJsonError::MissingField { field: "date" })
        );
        assert_eq!(
            parse_simple_json(&with("iban", serde_json::json!(""))),
            Err(SimpleJsonError::Empty { field: "iban" })
        );
        assert_eq!(
            parse_simple_json(&with("iban", serde_json::json!(42))),
            Err(SimpleJsonError::WrongType {
                field: "iban",
                expected: "a string"
            })
        );
        assert!(matches!(
            parse_simple_json(&with("amount_eur", serde_json::json!("155,00"))),
            Err(SimpleJsonError::InvalidAmount {
                field: "amount_eur",
                ..
            })
        ));
        assert!(matches!(
            parse_simple_json(&with("amount_eur", serde_json::json!(true))),
            Err(SimpleJsonError::WrongType {
                field: "amount_eur",
                ..
            })
        ));
        assert!(matches!(
            parse_simple_json(&with("date", serde_json::json!("10.07.2026"))),
            Err(SimpleJsonError::InvalidDate { field: "date", .. })
        ));
        assert!(matches!(
            parse_simple_json(&serde_json::json!([1, 2, 3])),
            Err(SimpleJsonError::NotAnObject { found: "an array" })
        ));
    }

    #[cfg(feature = "json")]
    #[test]
    fn optional_fields_are_optional_but_still_type_checked() {
        let json = serde_json::json!({
            "iban": "DE89370400440532013000",
            "amount_eur": 42,
            "date": "2026-07-10",
            "counterparty_name": "Max Mustermann",
        });
        let entry = parse_simple_json(&json).unwrap();
        assert_eq!(entry.amount_ct, 4_200);
        assert_eq!(entry.reference, "");
        assert_eq!(entry.counterparty_name.as_deref(), Some("Max Mustermann"));

        let mut bad = json;
        bad["reference"] = serde_json::json!(7);
        assert_eq!(
            parse_simple_json(&bad),
            Err(SimpleJsonError::WrongType {
                field: "reference",
                expected: "a string"
            })
        );
    }
}
