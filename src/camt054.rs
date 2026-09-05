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
//! camt.054 amounts are **always positive** with a separate
//! [`CreditDebitIndicator`] field for direction. [`CashEntry::signed_ct`] gives
//! the ledger amount (credit positive, debit negative).
//!
//! ## Example
//!
//! ```rust
//! use sepa::parse_camt054;
//!
//! # let xml = r#"<Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.054.001.08">
//! # <BkToCstmrDbtCdtNtfctn><GrpHdr><MsgId>M</MsgId></GrpHdr><Ntfctn><Id>N</Id>
//! # <Ntry><Amt Ccy="EUR">75.00</Amt><CdtDbtInd>DBIT</CdtDbtInd>
//! # <NtryDtls><TxDtls><RtrInf><Rsn><Cd>AM04</Cd></Rsn></RtrInf></TxDtls></NtryDtls>
//! # </Ntry></Ntfctn></BkToCstmrDbtCdtNtfctn></Document>"#;
//! let doc = parse_camt054(xml)?;
//!
//! // The reason to consume camt.054: collections that came back.
//! for returned in doc.notifications[0].returns() {
//!     println!("{} ct returned, reason {:?}",
//!              returned.signed_ct(), returned.return_reason_code());
//! }
//! # Ok::<(), sepa::Camt054ParseError>(())
//! ```
//!
//! [`CashEntry`]: crate::camt::CashEntry
//! [`CashEntry::signed_ct`]: crate::camt::CashEntry::signed_ct

use std::str::FromStr;

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
    /// The account this notification covers — IBAN or proprietary identifier,
    /// currency and servicing institution. See [`AccountRef`](crate::camt::AccountRef).
    pub account: crate::camt::AccountRef,
    /// Notification period start, ISO 8601.
    pub from_date_raw: Option<String>,
    /// Notification period end, ISO 8601.
    pub to_date_raw: Option<String>,
    /// The notified entries.
    pub entries: Vec<crate::camt::CashEntry>,
}

impl Camt054Notification {
    /// The period start, typed.
    ///
    /// `FrToDt` is a date/time choice, so it arrives as `"2026-07-14"` from one
    /// bank and `"2026-07-14T00:00:00"` from the next. The text that arrived is
    /// kept in [`from_date_raw`](Self::from_date_raw) either way — the same
    /// verbatim-and-typed rule [`CashEntry::booking_date`] follows, applied
    /// here so the whole read path answers dates the same way.
    ///
    /// [`CashEntry::booking_date`]: crate::CashEntry::booking_date
    #[must_use]
    pub fn from_date(&self) -> Option<crate::IsoDate> {
        crate::IsoDate::parse_date_part(self.from_date_raw.as_deref()?).ok()
    }

    /// The period end, typed. See [`from_date`](Self::from_date).
    #[must_use]
    pub fn to_date(&self) -> Option<crate::IsoDate> {
        crate::IsoDate::parse_date_part(self.to_date_raw.as_deref()?).ok()
    }

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
    pub created_at_raw: String,
    /// Detected XML namespace URI.
    pub namespace: Option<String>,
    /// One or more notifications.
    pub notifications: Vec<Camt054Notification>,
}

impl Camt054Document {
    /// When the bank generated this document, typed.
    ///
    /// `None` when it reported none, or one this crate cannot read;
    /// [`created_at_raw`](Self::created_at_raw) still holds whatever arrived.
    #[must_use]
    pub fn created_at(&self) -> Option<crate::IsoDateTime> {
        crate::IsoDateTime::parse(&self.created_at_raw).ok()
    }
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
            account: camt::account_of(n),
            from_date_raw: from_date,
            to_date_raw: to_date,
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
        created_at_raw: text("CreDtTm"),
        namespace: doc.namespace,
        notifications: root
            .children_named("Ntfctn")
            .map(parse_notification)
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
