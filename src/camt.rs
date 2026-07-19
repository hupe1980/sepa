//! Shared vocabulary for the camt.05x cash-management messages.
//!
//! `camt.052` (intraday report), `camt.053` (end-of-day statement) and
//! `camt.054` (debit/credit notification) describe the same underlying thing —
//! movements on an account — and differ mainly in their wrapper element and in
//! whether balances are present. They share the entry model defined here, so a
//! reconciliation routine can treat all three uniformly.
//!
//! ## Version handling
//!
//! ISO reshaped several elements across the v02 → v13 range. The parsers here
//! accept every generation:
//!
//! | Change | Introduced | Handled by |
//! |---|---|---|
//! | `Sts` became a `Cd`/`Prtry` choice | `.001.07` | accepts both forms |
//! | Parties gained a `Pty`/`Agt` wrapper | `.001.07` | accepts both nestings |
//! | `BIC` renamed to `BICFI` | `.001.03` | accepts both spellings |
//! | `TxDtls/Amt` became optional | `.001.08` | [`EntryDetail::amount_ct`] is `Option` |

use crate::camt054::CreditDebitIndicator;
use crate::xml::Node;

// ── BalanceType ───────────────────────────────────────────────────────────────

/// Type of a balance in a camt.05x message.
///
/// Appears as `Bal/Tp/CdOrPrtry/Cd`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum BalanceType {
    /// `OPBD` — Opening Booked: balance at start of statement period.
    OpeningBooked,
    /// `CLBD` — Closing Booked: balance at end of statement period.
    ClosingBooked,
    /// `ITBD` — Intraday Booked: intermediate booked balance.
    IntradayBooked,
    /// `CLAV` — Closing Available: available funds at end of period.
    ClosingAvailable,
    /// `OPAV` — Opening Available: available funds at start of period.
    OpeningAvailable,
    /// `FWAV` — Forward Available: future available balance.
    ForwardAvailable,
    /// Any other balance type code.
    Other(String),
}

impl BalanceType {
    /// ISO 20022 code string.
    #[must_use]
    pub fn as_code(&self) -> &str {
        match self {
            Self::OpeningBooked => "OPBD",
            Self::ClosingBooked => "CLBD",
            Self::IntradayBooked => "ITBD",
            Self::ClosingAvailable => "CLAV",
            Self::OpeningAvailable => "OPAV",
            Self::ForwardAvailable => "FWAV",
            Self::Other(s) => s,
        }
    }

    pub(crate) fn from_code(s: &str) -> Self {
        match s.trim().to_ascii_uppercase().as_str() {
            "OPBD" => Self::OpeningBooked,
            "CLBD" => Self::ClosingBooked,
            "ITBD" => Self::IntradayBooked,
            "CLAV" => Self::ClosingAvailable,
            "OPAV" => Self::OpeningAvailable,
            "FWAV" => Self::ForwardAvailable,
            other => Self::Other(other.to_owned()),
        }
    }
}

// ── EntryStatus ───────────────────────────────────────────────────────────────

/// Booking status of a camt.05x entry (`Ntry/Sts`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum EntryStatus {
    /// `BOOK` — Booked / settled.
    Booked,
    /// `PDNG` — Pending (not yet settled).
    Pending,
    /// `INFO` — Informational only.
    Info,
    /// `FUTR` — Future-dated entry.
    Future,
    /// Any other status code.
    Other(String),
}

impl EntryStatus {
    pub(crate) fn from_code(s: &str) -> Self {
        match s.trim().to_ascii_uppercase().as_str() {
            "BOOK" => Self::Booked,
            "PDNG" => Self::Pending,
            "INFO" => Self::Info,
            "FUTR" => Self::Future,
            other => Self::Other(other.to_owned()),
        }
    }
}

// ── StatementBalance ──────────────────────────────────────────────────────────

/// A balance entry within a camt.05x message.
#[derive(Debug, Clone)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct StatementBalance {
    /// Balance type (opening booked, closing booked, …).
    pub balance_type: BalanceType,
    /// Amount in **ct** (1/100 of `currency`). Always positive.
    pub amount_ct: i64,
    /// ISO 4217 currency of `amount_ct`, from the `Ccy` attribute.
    pub currency: String,
    /// Whether the balance is a credit (positive) or debit (negative) balance.
    pub indicator: CreditDebitIndicator,
    /// Balance date, ISO 8601 `"YYYY-MM-DD"`.
    pub date: String,
}

impl StatementBalance {
    /// Balance as signed ct value (+credit, −debit).
    #[inline]
    #[must_use]
    pub fn signed_ct(&self) -> i64 {
        match self.indicator {
            CreditDebitIndicator::Credit => self.amount_ct,
            CreditDebitIndicator::Debit => -self.amount_ct,
        }
    }
}

// ── EntryDetail ───────────────────────────────────────────────────────────────

/// One underlying transaction within a camt.05x entry (`NtryDtls/TxDtls`).
///
/// A **batch-booked** entry — the norm for SEPA direct debit collections, where
/// the bank books one aggregate amount — carries one `TxDtls` per original
/// transaction. Reconciling such an entry requires every detail, not just the
/// first, so [`CashEntry::details`] exposes all of them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct EntryDetail {
    /// This transaction's own amount in **ct**, when the bank itemises it
    /// (`TxDtls/Amt`). `None` for entries that only carry a total.
    pub amount_ct: Option<i64>,
    /// End-to-end reference from the original payment instruction.
    pub end_to_end_id: Option<String>,
    /// Mandate reference (direct debits, `MndtId`).
    pub mandate_id: Option<String>,
    /// SEPA Creditor Identifier (`CdtrId`).
    pub creditor_id: Option<String>,
    /// Remittance information / payment reference (`RmtInf/Ustrd`).
    pub reference: Option<String>,
    /// Counterparty name (debtor for credits; creditor for debits).
    pub counterparty_name: Option<String>,
    /// Counterparty IBAN.
    pub counterparty_iban: Option<String>,
    /// ISO 20022 return reason code, when this transaction is a return.
    pub return_reason_code: Option<String>,
}

// ── CashEntry ──────────────────────────────────────────────────────────────

/// A single booked or pending entry, shared by camt.052, camt.053 and camt.054.
///
/// The transaction-level fields live in [`details`](Self::details). Accessors
/// such as [`end_to_end_id`](Self::end_to_end_id) read the first detail, which
/// is what you want for an ordinary single-transaction entry; for a batch
/// booking, iterate `details` instead.
#[derive(Debug, Clone)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CashEntry {
    /// Amount in **ct** (1/100 of `currency`). Always positive — see `indicator`.
    pub amount_ct: i64,
    /// ISO 4217 currency of `amount_ct`, from the `Ccy` attribute.
    ///
    /// SEPA statements are EUR, but camt.053 is not EUR-only and banks do report
    /// foreign-currency accounts. Check this before treating the amount as EUR.
    pub currency: String,
    /// Credit (incoming) or Debit (outgoing).
    pub indicator: CreditDebitIndicator,
    /// Booking status.
    pub status: EntryStatus,
    /// `true` when the bank booked several transactions as one aggregate entry.
    pub batch_booked: bool,
    /// Booking date (`BookgDt/Dt`), ISO 8601.
    pub booking_date: Option<String>,
    /// Value date (`ValDt/Dt`), ISO 8601.
    pub value_date: Option<String>,
    /// Bank's internal transaction reference (`AcctSvcrRef`).
    pub account_servicer_ref: Option<String>,
    /// Bank transaction code (`BkTxCd`), domain code where available.
    pub bank_tx_code: Option<String>,
    /// Underlying transactions. Empty when the bank sends no `NtryDtls`.
    pub details: Vec<EntryDetail>,
}

impl CashEntry {
    /// Signed ledger amount: credit is positive (balance increase),
    /// debit is negative (balance decrease).
    #[inline]
    #[must_use]
    pub fn signed_ct(&self) -> i64 {
        match self.indicator {
            CreditDebitIndicator::Credit => self.amount_ct,
            CreditDebitIndicator::Debit => -self.amount_ct,
        }
    }

    /// The first transaction detail, if the entry has any.
    #[inline]
    #[must_use]
    pub fn first_detail(&self) -> Option<&EntryDetail> {
        self.details.first()
    }

    /// End-to-end reference of the first detail.
    #[must_use]
    pub fn end_to_end_id(&self) -> Option<&str> {
        self.first_detail()?.end_to_end_id.as_deref()
    }

    /// Mandate reference of the first detail.
    #[must_use]
    pub fn mandate_id(&self) -> Option<&str> {
        self.first_detail()?.mandate_id.as_deref()
    }

    /// SEPA Creditor Identifier of the first detail.
    #[must_use]
    pub fn creditor_id(&self) -> Option<&str> {
        self.first_detail()?.creditor_id.as_deref()
    }

    /// Remittance information of the first detail.
    #[must_use]
    pub fn reference(&self) -> Option<&str> {
        self.first_detail()?.reference.as_deref()
    }

    /// Counterparty name of the first detail.
    #[must_use]
    pub fn counterparty_name(&self) -> Option<&str> {
        self.first_detail()?.counterparty_name.as_deref()
    }

    /// Counterparty IBAN of the first detail.
    #[must_use]
    pub fn counterparty_iban(&self) -> Option<&str> {
        self.first_detail()?.counterparty_iban.as_deref()
    }

    /// Return reason code of the first detail.
    #[must_use]
    pub fn return_reason_code(&self) -> Option<&str> {
        self.first_detail()?.return_reason_code.as_deref()
    }

    /// Returns `true` if **any** underlying transaction is a return (Rückbuchung).
    ///
    /// Checks every detail, so a single returned collection inside a batch
    /// booking is still reported.
    #[must_use]
    pub fn is_return(&self) -> bool {
        self.details.iter().any(|d| d.return_reason_code.is_some())
    }
}

/// Read an `Amt` element into `(cents, currency)`.
pub(crate) fn amount_of(node: &Node, tag: &str) -> Option<(i64, String)> {
    let amt = node.child(tag)?;
    let ct = crate::ct_from_eur_str(&amt.text)?;
    Some((ct, amt.attr("Ccy").unwrap_or("EUR").to_owned()))
}

/// `CdtDbtInd`, defaulting to credit when absent or unrecognised.
pub(crate) fn indicator_of(node: &Node) -> CreditDebitIndicator {
    node.text_of("CdtDbtInd")
        .and_then(|s| s.parse().ok())
        .unwrap_or(CreditDebitIndicator::Credit)
}

/// A party name, handling both the flat and the `Party40Choice` shapes.
///
/// camt.053.001.02 nests the name as `Dbtr/Nm`; from `.001.08` the party is
/// wrapped in a choice, giving `Dbtr/Pty/Nm`. Accept either.
pub(crate) fn party_name(parties: Option<&Node>, tag: &str) -> Option<String> {
    let party = parties?.child(tag)?;
    party
        .text_of("Nm")
        .or_else(|| party.text_at(&["Pty", "Nm"]))
        .map(str::to_owned)
}

pub(crate) fn parse_balance(b: &Node) -> Option<StatementBalance> {
    let balance_type = b
        .path(&["Tp", "CdOrPrtry"])
        .and_then(Node::code)
        .map_or_else(|| BalanceType::Other(String::new()), BalanceType::from_code);

    let (amount_ct, currency) = amount_of(b, "Amt")?;

    // `Dt` is a DateAndDateTimeChoice: `Dt/Dt` or `Dt/DtTm`.
    let date = b
        .child("Dt")
        .and_then(|d| d.text_of("Dt").or_else(|| d.text_of("DtTm")))
        .unwrap_or_default()
        .to_owned();

    Some(StatementBalance {
        balance_type,
        amount_ct,
        currency,
        indicator: indicator_of(b),
        date,
    })
}

pub(crate) fn parse_entry(e: &Node) -> Option<CashEntry> {
    let (amount_ct, currency) = amount_of(e, "Amt")?;
    let indicator = indicator_of(e);

    // `Sts` is a bare code up to camt.053.001.02 (`<Sts>BOOK</Sts>`) and a
    // choice from .001.08 (`<Sts><Cd>BOOK</Cd></Sts>`). `Node::code` accepts both.
    let status = e
        .child("Sts")
        .and_then(Node::code)
        .map_or(EntryStatus::Booked, EntryStatus::from_code);

    let date_of = |tag: &str| {
        e.child(tag)
            .and_then(|d| d.text_of("Dt").or_else(|| d.text_of("DtTm")))
            .map(str::to_owned)
    };

    let bank_tx_code = e.child("BkTxCd").and_then(|c| {
        c.text_at(&["Domn", "Cd"])
            .or_else(|| c.text_at(&["Prtry", "Cd"]))
            .map(str::to_owned)
    });

    // Every TxDtls is kept: a batch-booked SEPA collection carries one per
    // original transaction, and dropping all but the first loses the data
    // reconciliation actually needs.
    //
    // `TxDtls` belongs under `NtryDtls`, but some banks place it directly under
    // `Ntry`; that non-conformant shape is accepted rather than silently
    // yielding an entry with no details at all.
    let details_parent = e.child("NtryDtls").unwrap_or(e);
    let batch_booked = details_parent.children_named("TxDtls").count() > 1;
    let details = details_parent
        .children_named("TxDtls")
        .map(|td| parse_detail(td, indicator))
        .collect();

    Some(CashEntry {
        amount_ct,
        currency,
        indicator,
        status,
        batch_booked,
        booking_date: date_of("BookgDt"),
        value_date: date_of("ValDt"),
        account_servicer_ref: e.text_of("AcctSvcrRef").map(str::to_owned),
        bank_tx_code,
        details,
    })
}

pub(crate) fn parse_detail(td: &Node, indicator: CreditDebitIndicator) -> EntryDetail {
    let refs = td.child("Refs");
    let ref_of = |tag: &str| refs.and_then(|r| r.text_of(tag)).map(str::to_owned);

    // Counterparty: for a credit the other side is the debtor, for a debit the creditor.
    let parties = td.child("RltdPties");
    let (name_tag, acct_tag) = match indicator {
        CreditDebitIndicator::Credit => ("Dbtr", "DbtrAcct"),
        CreditDebitIndicator::Debit => ("Cdtr", "CdtrAcct"),
    };

    EntryDetail {
        amount_ct: amount_of(td, "Amt").map(|(ct, _)| ct),
        end_to_end_id: ref_of("EndToEndId"),
        mandate_id: ref_of("MndtId"),
        creditor_id: ref_of("CdtrId"),
        reference: td.text_at(&["RmtInf", "Ustrd"]).map(str::to_owned),
        counterparty_name: party_name(parties, name_tag),
        counterparty_iban: parties
            .and_then(|p| p.text_at(&[acct_tag, "Id", "IBAN"]))
            .map(str::to_owned),
        return_reason_code: td
            .path(&["RtrInf", "Rsn"])
            .and_then(Node::code)
            .map(str::to_owned),
    }
}

// ── shared account / group helpers ────────────────────────────────────────────

/// The account IBAN of a statement, report or notification (`Acct/Id/IBAN`).
pub(crate) fn account_iban(node: &Node) -> String {
    node.path(&["Acct", "Id", "IBAN"])
        .map(|n| n.text.clone())
        .unwrap_or_default()
}

/// `FinInstnId/BIC` (pre-2019) or `FinInstnId/BICFI` (2019 onwards).
///
/// ISO renamed the element in `camt.05x.001.03`; both spellings are accepted so
/// a version-agnostic caller never silently loses the agent BIC.
pub(crate) fn agent_bic(agent: &Node) -> Option<&str> {
    let fin = agent.child("FinInstnId")?;
    fin.text_of("BIC").or_else(|| fin.text_of("BICFI"))
}

/// The `FrToDt` reporting period, tolerating both the `FrDtTm`/`ToDtTm` and the
/// `FrDt`/`ToDt` spellings.
pub(crate) fn period(node: &Node) -> (Option<String>, Option<String>) {
    let range = node.child("FrToDt");
    let at = |dt_tm: &str, dt: &str| {
        range
            .and_then(|r| r.text_of(dt_tm).or_else(|| r.text_of(dt)))
            .map(str::to_owned)
    };
    (at("FrDtTm", "FrDt"), at("ToDtTm", "ToDt"))
}

/// Read `Ntry` children into entries, and `Bal` children into balances.
pub(crate) fn entries_of(node: &Node) -> Vec<CashEntry> {
    node.children_named("Ntry")
        .filter_map(parse_entry)
        .collect()
}

/// Read `Bal` children into balances (camt.052 and camt.053 only).
pub(crate) fn balances_of(node: &Node) -> Vec<StatementBalance> {
    node.children_named("Bal")
        .filter_map(parse_balance)
        .collect()
}
