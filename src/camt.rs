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
//! | `TxDtls/Amt` became optional | `.001.08` | falls back to `AmtDtls/TxAmt/Amt`, then to the entry total for a single-detail entry — see [`EntryDetail::signed_ct`] |
//!
//! ## Batch bookings
//!
//! A batch-booked entry carries one `TxDtls` per original transaction. Read
//! [`EntryDetail::signed_ct`] for each — it resolves the amount and its sign
//! from whichever level reported them — and use
//! [`CashEntry::details_reconcile`] to check the parts add up to the whole
//! before posting.

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
    /// Balance date exactly as the bank reported it.
    ///
    /// `Bal/Dt` is a date/time choice, so this is `"2026-07-20"` from one bank
    /// and `"2026-07-20T23:59:59"` from the next. Read [`date`](Self::date) for
    /// the day; this field is kept verbatim so nothing is lost.
    pub date_raw: String,
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

    /// The balance date, or `None` if the bank reported none this crate can read.
    #[must_use]
    pub fn date(&self) -> Option<crate::IsoDate> {
        crate::IsoDate::parse_date_part(&self.date_raw).ok()
    }
}

// ── EntryDetail ───────────────────────────────────────────────────────────────

/// One underlying transaction within a camt.05x entry (`NtryDtls/TxDtls`).
///
/// A **batch-booked** entry — the norm for SEPA direct debit collections, where
/// the bank books one aggregate amount — carries one `TxDtls` per original
/// transaction. Reconciling such an entry requires every detail, not just the
/// first, so [`CashEntry::details`] exposes all of them.
///
/// ## Amounts
///
/// Read [`signed_ct`](Self::signed_ct), not `amount_ct`. `TxDtls/Amt` became
/// optional in `.001.08`, its sign lives in a separate `CdtDbtInd` that may sit
/// at either level, and it is denominated in the transaction's own currency
/// rather than the account's. The parser resolves all three and leaves
/// `signed_ct` as `None` when it cannot — which is the answer a reconciliation
/// routine needs, because the tempting fallback (use the entry total)
/// double-counts every batch of more than one transaction.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct EntryDetail {
    /// This transaction's own amount in **ct**, as reported (`TxDtls/Amt` or
    /// `TxDtls/AmtDtls/TxAmt/Amt`). Always positive; `None` when the bank
    /// itemises no amount. See [`signed_ct`](Self::signed_ct).
    pub amount_ct: Option<i64>,
    /// ISO 4217 currency of `amount_ct`, when one was reported.
    ///
    /// A detail in a different currency from its entry is a foreign-currency
    /// transaction whose booked amount the statement does not restate, so it is
    /// not summable against the entry total.
    pub currency: Option<String>,
    /// Credit or debit for this transaction (`TxDtls/CdtDbtInd`), falling back
    /// to the entry's indicator when the detail does not carry its own.
    ///
    /// A returned collection inside an otherwise-credit batch is exactly the
    /// case where the two differ.
    pub indicator: CreditDebitIndicator,
    /// The resolved ledger amount in **ct**: positive for a credit, negative
    /// for a debit. `None` when it could not be established — see
    /// [`signed_ct`](Self::signed_ct).
    pub signed_amount_ct: Option<i64>,
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

impl EntryDetail {
    /// This transaction's ledger amount in **ct** — credit positive, debit
    /// negative — or `None` when the statement does not determine it.
    ///
    /// Resolved by the parser as:
    ///
    /// 1. `TxDtls/Amt`, else `TxDtls/AmtDtls/TxAmt/Amt`, signed by
    ///    `TxDtls/CdtDbtInd` where present and by the entry's indicator
    ///    otherwise; then
    /// 2. for a **single-detail** entry with no itemised amount, the entry's
    ///    own signed amount — that identity is safe precisely because there is
    ///    only one transaction to attribute it to; otherwise
    /// 3. `None`.
    ///
    /// A detail denominated in a currency other than the entry's is also
    /// `None`: its amount is real, but it is not the amount that hit the
    /// account, so summing it against the entry total would be wrong.
    ///
    /// Case 3 is the one worth handling explicitly. Falling back to the entry
    /// total per detail — the obvious workaround when this is unavailable —
    /// multiplies a batch booking by its transaction count.
    #[inline]
    #[must_use]
    pub const fn signed_ct(&self) -> Option<i64> {
        self.signed_amount_ct
    }

    /// Whether this transaction is a return (Rückläufer).
    #[inline]
    #[must_use]
    pub const fn is_return(&self) -> bool {
        self.return_reason_code.is_some()
    }
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
    /// Booking date (`BookgDt`) exactly as the bank reported it.
    ///
    /// ISO 20022 types this as a date/time choice, so it arrives as
    /// `"2026-07-20"` from one bank and `"2026-07-20T09:14:00"` from the next.
    /// Read [`booking_date`](Self::booking_date) for the day.
    pub booking_date_raw: Option<String>,
    /// Value date (`ValDt`) exactly as the bank reported it — see
    /// [`booking_date_raw`](Self::booking_date_raw).
    pub value_date_raw: Option<String>,
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

    /// The booking date — the day the entry hits the account balance.
    ///
    /// `None` when the bank reported none, or reported one this crate cannot
    /// read; [`booking_date_raw`](Self::booking_date_raw) still has whatever
    /// arrived.
    #[must_use]
    pub fn booking_date(&self) -> Option<crate::IsoDate> {
        parse_date(self.booking_date_raw.as_deref())
    }

    /// The value date — the day the entry earns or costs interest.
    #[must_use]
    pub fn value_date(&self) -> Option<crate::IsoDate> {
        parse_date(self.value_date_raw.as_deref())
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
        self.details.iter().any(EntryDetail::is_return)
    }

    /// The sum of every detail's [`signed_ct`](EntryDetail::signed_ct), or
    /// `None` if any detail's amount could not be resolved.
    ///
    /// All-or-nothing on purpose: a partial sum silently understates a batch.
    /// An entry with no details sums to `Some(0)` — the sum over nothing, not
    /// the entry total; compare against [`signed_ct`](Self::signed_ct) with
    /// [`details_reconcile`](Self::details_reconcile) rather than substituting
    /// one for the other.
    #[must_use]
    pub fn details_signed_sum_ct(&self) -> Option<i64> {
        self.details
            .iter()
            .try_fold(0i64, |acc, d| acc.checked_add(d.signed_ct()?))
    }

    /// Whether the details resolve and add up to the entry's own signed amount.
    ///
    /// The reconciliation guard for a batch booking: `false` means the
    /// statement's parts do not account for its whole, and the entry should be
    /// escalated rather than posted.
    ///
    /// An entry with no details at all reconciles trivially — there is nothing
    /// to disagree with — so check [`details`](Self::details) too when a
    /// breakdown is required.
    ///
    /// # Examples
    ///
    /// ```
    /// use sepa::parse_camt053;
    ///
    /// let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
    /// <Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.053.001.08">
    ///   <BkToCstmrStmt><GrpHdr><MsgId>M</MsgId></GrpHdr><Stmt><Id>S</Id>
    ///     <Ntry>
    ///       <Amt Ccy="EUR">125.00</Amt><CdtDbtInd>CRDT</CdtDbtInd>
    ///       <NtryDtls>
    ///         <TxDtls><Amt Ccy="EUR">100.00</Amt></TxDtls>
    ///         <TxDtls><Amt Ccy="EUR">25.00</Amt></TxDtls>
    ///       </NtryDtls>
    ///     </Ntry>
    ///   </Stmt></BkToCstmrStmt>
    /// </Document>"#;
    ///
    /// let doc = parse_camt053(xml)?;
    /// let entry = &doc.statements[0].entries[0];
    /// assert_eq!(entry.details_signed_sum_ct(), Some(12_500));
    /// assert!(entry.details_reconcile());
    /// # Ok::<(), sepa::Camt053ParseError>(())
    /// ```
    #[must_use]
    pub fn details_reconcile(&self) -> bool {
        if self.details.is_empty() {
            return true;
        }
        self.details_signed_sum_ct() == Some(self.signed_ct())
    }
}

/// Read an `Amt` element into `(cents, currency)`.
pub(crate) fn amount_of(node: &Node, tag: &str) -> Option<(i64, String)> {
    let amt = node.child(tag)?;
    let ct = crate::ct_from_eur_str(&amt.text).ok()?;
    Some((ct, amt.attr("Ccy").unwrap_or("EUR").to_owned()))
}

/// `CdtDbtInd`, defaulting to credit when absent or unrecognised.
pub(crate) fn indicator_of(node: &Node) -> CreditDebitIndicator {
    node.text_of("CdtDbtInd")
        .and_then(|s| s.parse().ok())
        .unwrap_or(CreditDebitIndicator::Credit)
}

/// The date part of an optional bank-supplied date or date-time.
fn parse_date(raw: Option<&str>) -> Option<crate::IsoDate> {
    crate::IsoDate::parse_date_part(raw?).ok()
}

/// Apply a credit/debit indicator to a magnitude.
const fn signed(indicator: CreditDebitIndicator, amount_ct: i64) -> i64 {
    match indicator {
        CreditDebitIndicator::Credit => amount_ct,
        CreditDebitIndicator::Debit => -amount_ct,
    }
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
        date_raw: date,
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
    let detail_count = details_parent.children_named("TxDtls").count();
    let batch_booked = detail_count > 1;
    let details = details_parent
        .children_named("TxDtls")
        .map(|td| {
            parse_detail(
                td,
                &EntryContext {
                    indicator,
                    amount_ct,
                    currency: &currency,
                    // The entry total may stand in for the detail's amount only
                    // when there is exactly one transaction to attribute it to.
                    // Spreading it across a batch would multiply the booking.
                    sole_detail: detail_count == 1,
                },
            )
        })
        .collect();

    Some(CashEntry {
        amount_ct,
        currency,
        indicator,
        status,
        batch_booked,
        booking_date_raw: date_of("BookgDt"),
        value_date_raw: date_of("ValDt"),
        account_servicer_ref: e.text_of("AcctSvcrRef").map(str::to_owned),
        bank_tx_code,
        details,
    })
}

/// What the enclosing `Ntry` says, for resolving a detail's amount and sign.
pub(crate) struct EntryContext<'a> {
    pub(crate) indicator: CreditDebitIndicator,
    pub(crate) amount_ct: i64,
    pub(crate) currency: &'a str,
    pub(crate) sole_detail: bool,
}

pub(crate) fn parse_detail(td: &Node, entry: &EntryContext<'_>) -> EntryDetail {
    let refs = td.child("Refs");
    let ref_of = |tag: &str| refs.and_then(|r| r.text_of(tag)).map(str::to_owned);

    // `TxDtls/CdtDbtInd` is optional and overrides the entry's when present —
    // that is how a single returned collection inside a credit batch is
    // reported.
    let indicator = td
        .text_of("CdtDbtInd")
        .and_then(|s| s.parse().ok())
        .unwrap_or(entry.indicator);

    // Counterparty: for a credit the other side is the debtor, for a debit the creditor.
    let parties = td.child("RltdPties");
    let (name_tag, acct_tag) = match indicator {
        CreditDebitIndicator::Credit => ("Dbtr", "DbtrAcct"),
        CreditDebitIndicator::Debit => ("Cdtr", "CdtrAcct"),
    };

    // `TxDtls/Amt` is the transaction amount; `AmtDtls/TxAmt/Amt` carries the
    // same figure in the messages that omit the former. Either is reported as a
    // magnitude, so the sign comes from the indicator above.
    let reported = amount_of(td, "Amt").or_else(|| {
        td.child("AmtDtls")
            .and_then(|ad| ad.child("TxAmt"))
            .and_then(|ta| amount_of(ta, "Amt"))
    });

    let signed_amount_ct = match &reported {
        // A foreign-currency transaction: the figure is real but is not what
        // hit the account, so it must not be summed against the entry total.
        Some((_, ccy)) if !ccy.eq_ignore_ascii_case(entry.currency) => None,
        Some((ct, _)) => Some(signed(indicator, ct.abs())),
        None if entry.sole_detail => Some(signed(indicator, entry.amount_ct.abs())),
        None => None,
    };

    EntryDetail {
        amount_ct: reported.as_ref().map(|(ct, _)| ct.abs()),
        currency: reported.map(|(_, ccy)| ccy),
        indicator,
        signed_amount_ct,
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

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::{CashEntry, CreditDebitIndicator};

    /// Parse a camt.053 statement whose single `Ntry` is `entry_xml`.
    fn entry(entry_xml: &str) -> CashEntry {
        let xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.053.001.08">
  <BkToCstmrStmt><GrpHdr><MsgId>M</MsgId></GrpHdr><Stmt><Id>S</Id>
    {entry_xml}
  </Stmt></BkToCstmrStmt>
</Document>"#
        );
        crate::parse_camt053(&xml).unwrap().statements[0]
            .entries
            .remove(0)
    }

    #[test]
    fn an_itemised_detail_carries_its_own_signed_amount() {
        let e = entry(
            r#"<Ntry>
                 <Amt Ccy="EUR">125.00</Amt><CdtDbtInd>CRDT</CdtDbtInd>
                 <NtryDtls>
                   <TxDtls><Amt Ccy="EUR">100.00</Amt></TxDtls>
                   <TxDtls><Amt Ccy="EUR">25.00</Amt></TxDtls>
                 </NtryDtls>
               </Ntry>"#,
        );
        assert!(e.batch_booked);
        assert_eq!(e.details[0].signed_ct(), Some(10_000));
        assert_eq!(e.details[1].signed_ct(), Some(2_500));
        assert_eq!(e.details_signed_sum_ct(), Some(12_500));
        assert!(e.details_reconcile());
    }

    #[test]
    fn a_batch_with_no_itemised_amounts_reports_none_rather_than_the_entry_total() {
        // The whole point: reusing the entry total per detail would book
        // 2 × 125.00 EUR for a single 125.00 EUR entry.
        let e = entry(
            r#"<Ntry>
                 <Amt Ccy="EUR">125.00</Amt><CdtDbtInd>CRDT</CdtDbtInd>
                 <NtryDtls>
                   <TxDtls><Refs><EndToEndId>E1</EndToEndId></Refs></TxDtls>
                   <TxDtls><Refs><EndToEndId>E2</EndToEndId></Refs></TxDtls>
                 </NtryDtls>
               </Ntry>"#,
        );
        assert_eq!(e.details[0].signed_ct(), None);
        assert_eq!(e.details[1].signed_ct(), None);
        assert_eq!(e.details_signed_sum_ct(), None);
        assert!(!e.details_reconcile());
    }

    #[test]
    fn a_sole_detail_inherits_the_entry_amount() {
        // Safe precisely because there is only one transaction to attribute it
        // to — which is the ordinary single-payment booking.
        let e = entry(
            r#"<Ntry>
                 <Amt Ccy="EUR">75.00</Amt><CdtDbtInd>DBIT</CdtDbtInd>
                 <NtryDtls><TxDtls><Refs><MndtId>MND-1</MndtId></Refs></TxDtls></NtryDtls>
               </Ntry>"#,
        );
        assert!(!e.batch_booked);
        assert_eq!(e.details[0].amount_ct, None, "nothing was itemised");
        assert_eq!(e.details[0].signed_ct(), Some(-7_500));
        assert!(e.details_reconcile());
    }

    #[test]
    fn a_detail_level_indicator_overrides_the_entry_level_one() {
        // A returned collection inside an otherwise-credit batch.
        let e = entry(
            r#"<Ntry>
                 <Amt Ccy="EUR">75.00</Amt><CdtDbtInd>CRDT</CdtDbtInd>
                 <NtryDtls>
                   <TxDtls><Amt Ccy="EUR">100.00</Amt></TxDtls>
                   <TxDtls>
                     <Amt Ccy="EUR">25.00</Amt><CdtDbtInd>DBIT</CdtDbtInd>
                     <RtrInf><Rsn><Cd>MD01</Cd></Rsn></RtrInf>
                   </TxDtls>
                 </NtryDtls>
               </Ntry>"#,
        );
        assert_eq!(e.details[0].indicator, CreditDebitIndicator::Credit);
        assert_eq!(e.details[1].indicator, CreditDebitIndicator::Debit);
        assert_eq!(e.details[1].signed_ct(), Some(-2_500));
        assert_eq!(e.details_signed_sum_ct(), Some(7_500));
        assert!(e.details_reconcile());
        assert!(e.is_return());
        assert!(e.details[1].is_return());
    }

    #[test]
    fn amt_dtls_tx_amt_stands_in_for_a_missing_tx_amount() {
        // The shape several German banks send from camt.05x.001.08 onwards.
        let e = entry(
            r#"<Ntry>
                 <Amt Ccy="EUR">30.00</Amt><CdtDbtInd>DBIT</CdtDbtInd>
                 <NtryDtls>
                   <TxDtls><AmtDtls><TxAmt><Amt Ccy="EUR">10.00</Amt></TxAmt></AmtDtls></TxDtls>
                   <TxDtls><AmtDtls><TxAmt><Amt Ccy="EUR">20.00</Amt></TxAmt></AmtDtls></TxDtls>
                 </NtryDtls>
               </Ntry>"#,
        );
        assert_eq!(e.details[0].signed_ct(), Some(-1_000));
        assert_eq!(e.details[1].signed_ct(), Some(-2_000));
        assert!(e.details_reconcile());
    }

    #[test]
    fn a_foreign_currency_detail_is_reported_but_not_summed() {
        // The figure is real; it is simply not the amount that hit the account,
        // so adding it to the entry total would be wrong.
        let e = entry(
            r#"<Ntry>
                 <Amt Ccy="EUR">92.00</Amt><CdtDbtInd>CRDT</CdtDbtInd>
                 <NtryDtls><TxDtls><Amt Ccy="USD">100.00</Amt></TxDtls></NtryDtls>
               </Ntry>"#,
        );
        assert_eq!(e.details[0].amount_ct, Some(10_000));
        assert_eq!(e.details[0].currency.as_deref(), Some("USD"));
        assert_eq!(e.details[0].signed_ct(), None);
        assert!(!e.details_reconcile());
    }

    #[test]
    fn dates_are_typed_whichever_choice_form_the_bank_used() {
        // `BookgDt` is a DateAndDateTimeChoice: a bare date from one bank, a
        // timestamp from the next. Both post on the same day.
        let bare = entry(
            r#"<Ntry>
                 <Amt Ccy="EUR">10.00</Amt><CdtDbtInd>CRDT</CdtDbtInd>
                 <BookgDt><Dt>2026-07-14</Dt></BookgDt>
                 <ValDt><Dt>2026-07-15</Dt></ValDt>
               </Ntry>"#,
        );
        let stamped = entry(
            r#"<Ntry>
                 <Amt Ccy="EUR">10.00</Amt><CdtDbtInd>CRDT</CdtDbtInd>
                 <BookgDt><DtTm>2026-07-14T09:14:00</DtTm></BookgDt>
               </Ntry>"#,
        );
        let day = crate::IsoDate::new(2026, 7, 14).unwrap();
        assert_eq!(bare.booking_date(), Some(day));
        assert_eq!(stamped.booking_date(), Some(day));
        assert_eq!(
            bare.value_date(),
            Some(crate::IsoDate::new(2026, 7, 15).unwrap())
        );

        // The raw text survives either way.
        assert_eq!(
            stamped.booking_date_raw.as_deref(),
            Some("2026-07-14T09:14:00")
        );

        // A missing or unreadable date is `None`, never a panic — the field is
        // bank-supplied.
        let absent = entry(r#"<Ntry><Amt Ccy="EUR">10.00</Amt></Ntry>"#);
        assert_eq!(absent.booking_date(), None);
        let nonsense = entry(
            r#"<Ntry><Amt Ccy="EUR">10.00</Amt><BookgDt><Dt>14.07.2026</Dt></BookgDt></Ntry>"#,
        );
        assert_eq!(nonsense.booking_date(), None);
        assert_eq!(nonsense.booking_date_raw.as_deref(), Some("14.07.2026"));
    }

    #[test]
    fn an_entry_without_details_reconciles_trivially() {
        let e = entry(r#"<Ntry><Amt Ccy="EUR">10.00</Amt><CdtDbtInd>CRDT</CdtDbtInd></Ntry>"#);
        assert!(e.details.is_empty());
        assert_eq!(e.details_signed_sum_ct(), Some(0));
        assert!(e.details_reconcile());
    }

    #[test]
    fn a_mismatched_batch_is_reported_as_not_reconciling() {
        // The statement's parts do not account for its whole: escalate rather
        // than post.
        let e = entry(
            r#"<Ntry>
                 <Amt Ccy="EUR">125.00</Amt><CdtDbtInd>CRDT</CdtDbtInd>
                 <NtryDtls>
                   <TxDtls><Amt Ccy="EUR">100.00</Amt></TxDtls>
                   <TxDtls><Amt Ccy="EUR">20.00</Amt></TxDtls>
                 </NtryDtls>
               </Ntry>"#,
        );
        assert_eq!(e.details_signed_sum_ct(), Some(12_000));
        assert_eq!(e.signed_ct(), 12_500);
        assert!(!e.details_reconcile());
    }
}
