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
//! ISO reshaped several elements across the v02 → v13 range. This shared model
//! handles the shapes below; each public reader enforces its own exact supported
//! namespace/version list, not every ISO generation:
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
//! A batch-booked entry carries a `Btch` block plus one `TxDtls` per original
//! transaction. [`CashEntry::batch`] exposes the former — including the
//! `PmtInfId` of the group that produced the booking, which is what matches a
//! statement entry back to a submitted pain.008 — and [`CashEntry::details`]
//! the latter. Read [`EntryDetail::signed_ct`] for each detail (it resolves the
//! amount and its sign from whichever level reported them) and use
//! [`CashEntry::details_reconcile`] to check the parts add up to the whole
//! before posting.

use crate::camt054::CreditDebitIndicator;
use crate::xml::Node;

/// Why a financial observation cannot be used as an exact financial fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum FinancialIssue {
    /// A required magnitude is absent.
    MissingAmount,
    /// The reported text is not a decimal amount.
    InvalidAmount,
    /// The reported transaction-count text is not a readable unsigned integer.
    InvalidCount,
    /// An amount magnitude is negative.
    NegativeAmount,
    /// Significant digits below one hundredth cannot be represented.
    SubCentPrecision,
    /// The exact amount or count cannot fit its integer representation.
    Overflow,
    /// The amount has no reported currency.
    MissingCurrency,
    /// Currency spelling is malformed or reserved, not a registry lookup.
    InvalidCurrency,
    /// A required ledger direction is absent.
    MissingDirection,
    /// A reported direction is not an exact supported wire code.
    InvalidDirection,
    /// Raw and interpreted facts disagree.
    InconsistentValue,
    /// Independently reported alternatives disagree or violate a choice.
    ConflictingAlternatives,
    /// Denominations are not comparable for the requested aggregate.
    CurrencyMismatch,
    /// Independently reported aggregate and record magnitudes disagree.
    TotalMismatch,
}

/// Observed source of an original instructed amount, never inferred from value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum OriginalAmountSource {
    /// Transaction-level `OrgnlInstdAmt`.
    OriginalInstructed,
    /// `OrgnlTxRef/Amt/InstdAmt`.
    TransactionReference,
    /// Historical direct `OrgnlTxRef/InstdAmt` observation.
    LegacyTransactionReference,
}

impl OriginalAmountSource {
    pub(crate) const fn path(self) -> &'static str {
        match self {
            Self::OriginalInstructed => "OrgnlInstdAmt",
            Self::TransactionReference => "OrgnlTxRef/Amt/InstdAmt",
            Self::LegacyTransactionReference => "OrgnlTxRef/InstdAmt",
        }
    }
}

/// Observed route of a cash transaction's itemized amount.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum DetailAmountSource {
    /// Direct `TxDtls/Amt`.
    Direct,
    /// Nested `TxDtls/AmtDtls/TxAmt/Amt`.
    TransactionAmount,
}

impl DetailAmountSource {
    pub(crate) const fn path(self) -> &'static str {
        match self {
            Self::Direct => "Amt",
            Self::TransactionAmount => "AmtDtls/TxAmt/Amt",
        }
    }
}

/// A located observation problem, not an XSD verdict or payment decision.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FinancialDiagnostic {
    /// Logical ISO field location; retained-record indices are zero-based.
    /// Detail indices address flattened observations, not executable `XPath`.
    pub path: String,
    /// Machine-readable reason.
    pub issue: FinancialIssue,
    /// Decoded reported text, including empty/padded text.
    /// `None` indicates no scalar text, including derived/container problems.
    pub raw: Option<String>,
}

pub(crate) fn currency_diagnostics(path: &str, raw: Option<&str>) -> Vec<FinancialDiagnostic> {
    let issue = match raw {
        None => Some(FinancialIssue::MissingCurrency),
        Some(raw)
            if crate::Currency::new(raw)
                .ok()
                .is_none_or(|currency| currency.code() != raw) =>
        {
            Some(FinancialIssue::InvalidCurrency)
        }
        Some(_) => None,
    };
    issue
        .into_iter()
        .map(|issue| FinancialDiagnostic {
            path: path.to_owned(),
            issue,
            raw: raw.map(str::to_owned),
        })
        .collect()
}

pub(crate) fn decimal_diagnostics(
    path: &str,
    raw: Option<&str>,
    interpreted: Option<i64>,
    nonnegative: bool,
) -> Vec<FinancialDiagnostic> {
    let issue = match raw.map(crate::ct_from_eur_str) {
        None if interpreted.is_none() => Some(FinancialIssue::MissingAmount),
        None => interpreted
            .filter(|value| nonnegative && *value < 0)
            .map(|_| FinancialIssue::NegativeAmount),
        Some(Err(crate::AmountError::SubCentPrecision { .. })) => {
            Some(FinancialIssue::SubCentPrecision)
        }
        Some(Err(crate::AmountError::Overflow { .. })) => Some(FinancialIssue::Overflow),
        Some(Err(_)) => Some(FinancialIssue::InvalidAmount),
        Some(Ok(value)) if nonnegative && value < 0 => Some(FinancialIssue::NegativeAmount),
        Some(Ok(value)) if Some(value) != interpreted => Some(FinancialIssue::InconsistentValue),
        Some(Ok(_)) => None,
    };
    issue
        .into_iter()
        .map(|issue| FinancialDiagnostic {
            path: path.to_owned(),
            issue,
            raw: raw.map(str::to_owned),
        })
        .collect()
}

pub(crate) fn optional_amount_diagnostics(
    path: &str,
    raw: Option<&str>,
    ct: Option<i64>,
    currency: Option<&str>,
) -> Vec<FinancialDiagnostic> {
    if raw.is_none() && ct.is_none() && currency.is_none() {
        return Vec::new();
    }
    ReportedAmount {
        ct,
        amount_raw: raw.map(str::to_owned),
        currency: currency.map(str::to_owned),
        ..Default::default()
    }
    .diagnostics(path, None)
}

pub(crate) fn optional_count_diagnostics(
    path: &str,
    raw: Option<&str>,
    count: Option<u64>,
) -> Vec<FinancialDiagnostic> {
    let issue = match raw {
        Some(raw) => match raw.trim().parse::<u64>() {
            Ok(value) if Some(value) == count => None,
            Ok(_) => Some(FinancialIssue::InconsistentValue),
            Err(error) if matches!(error.kind(), std::num::IntErrorKind::PosOverflow) => {
                Some(FinancialIssue::Overflow)
            }
            Err(_) => Some(FinancialIssue::InvalidCount),
        },
        None => count.map(|_| FinancialIssue::InconsistentValue),
    };
    issue.map_or_else(Vec::new, |issue| {
        vec![FinancialDiagnostic {
            path: path.to_owned(),
            issue,
            raw: raw.map(str::to_owned),
        }]
    })
}

pub(crate) fn optional_decimal_diagnostics(
    path: &str,
    raw: Option<&str>,
    ct: Option<i64>,
) -> Vec<FinancialDiagnostic> {
    if raw.is_none() && ct.is_none() {
        Vec::new()
    } else {
        decimal_diagnostics(path, raw, ct, false)
    }
}

fn charge_diagnostics(path: &str, charges: &Charges, result: &mut Vec<FinancialDiagnostic>) {
    if charges.has_mixed_sources() {
        result.push(FinancialDiagnostic {
            path: path.to_owned(),
            issue: FinancialIssue::ConflictingAlternatives,
            raw: None,
        });
    }
    let aggregate = format!("{path}/TtlChrgsAndTaxAmt");
    result.extend(optional_amount_diagnostics(
        &aggregate,
        charges.total_amount_raw.as_deref(),
        charges.total_ct,
        charges.total_currency.as_deref(),
    ));
    let currency = charges
        .records
        .first()
        .and_then(|record| record.amount.currency.as_deref());
    for (index, record) in charges.records.iter().enumerate() {
        let record_path = if record.source == Some(ChargeRecordSource::LegacyFlat) {
            path.to_owned()
        } else {
            format!("{path}/Rcrd[{index}]")
        };
        result.extend(record.amount.diagnostics(
            &format!("{record_path}/Amt"),
            Some(&format!("{record_path}/CdtDbtInd")),
        ));
        if let (Some(expected), Some(actual)) = (currency, record.amount.currency.as_deref())
            && expected != actual
        {
            result.push(FinancialDiagnostic {
                path: format!("{record_path}/Amt/@Ccy"),
                issue: FinancialIssue::CurrencyMismatch,
                raw: Some(actual.to_owned()),
            });
        }
    }
    if let (Some(expected), Some(actual)) = (currency, charges.total_currency.as_deref())
        && charges.total_amount_raw.is_some()
        && expected != actual
    {
        result.push(FinancialDiagnostic {
            path: format!("{aggregate}/@Ccy"),
            issue: FinancialIssue::CurrencyMismatch,
            raw: Some(actual.to_owned()),
        });
    }
    if let Some(total) = charges.total_ct {
        let comparable = charges.total_currency.as_deref().is_some_and(|currency| {
            charges.records.iter().all(|record| {
                record.amount.currency.as_deref() == Some(currency)
                    && record.amount.diagnostics("", None).is_empty()
            })
        }) && optional_amount_diagnostics(
            "",
            charges.total_amount_raw.as_deref(),
            charges.total_ct,
            charges.total_currency.as_deref(),
        )
        .is_empty();
        let sum = charges.records.iter().try_fold(0_i128, |sum, record| {
            sum.checked_add(i128::from(record.amount.ct?))
        });
        if comparable
            && !charges.has_mixed_sources()
            && !charges.records.is_empty()
            && sum.is_some_and(|sum| sum != i128::from(total))
        {
            result.push(FinancialDiagnostic {
                path: aggregate,
                issue: FinancialIssue::TotalMismatch,
                raw: charges.total_amount_raw.clone(),
            });
        }
    }
}

pub(crate) fn cash_diagnostics(
    path: &str,
    balances: &[StatementBalance],
    entries: &[CashEntry],
) -> Vec<FinancialDiagnostic> {
    let mut result = Vec::new();
    for (index, balance) in balances.iter().enumerate() {
        let location = format!("{path}/Bal[{index}]");
        result.extend(balance.amount.diagnostics(
            &format!("{location}/Amt"),
            Some(&format!("{location}/CdtDbtInd")),
        ));
    }
    let currency = entries
        .first()
        .and_then(|entry| entry.amount.currency.as_deref());
    for (index, entry) in entries.iter().enumerate() {
        let location = format!("{path}/Ntry[{index}]");
        result.extend(entry.amount.diagnostics(
            &format!("{location}/Amt"),
            Some(&format!("{location}/CdtDbtInd")),
        ));
        if let (Some(expected), Some(actual)) = (currency, entry.amount.currency.as_deref())
            && expected != actual
        {
            result.push(FinancialDiagnostic {
                path: format!("{location}/Amt/@Ccy"),
                issue: FinancialIssue::CurrencyMismatch,
                raw: Some(actual.to_owned()),
            });
        }
        if let Some(charges) = &entry.charges {
            charge_diagnostics(&format!("{location}/Chrgs"), charges, &mut result);
        }
        for (index, detail) in entry.details.iter().enumerate() {
            let detail_path = format!("{location}/NtryDtls/TxDtls[{index}]");
            let source = detail
                .amount_source
                .map_or("amount", DetailAmountSource::path);
            if detail.amount_source.is_some()
                || detail.amount.amount_raw.is_some()
                || detail.amount.ct.is_some()
                || detail.amount.currency.is_some()
            {
                result.extend(detail.amount.diagnostics(
                    &format!("{detail_path}/{source}"),
                    Some(&format!("{detail_path}/CdtDbtInd")),
                ));
            }
            if let Some(transaction) = &detail.transaction_amount {
                if detail.amount_source != Some(DetailAmountSource::TransactionAmount) {
                    result.extend(
                        transaction.diagnostics(&format!("{detail_path}/AmtDtls/TxAmt/Amt"), None),
                    );
                }
                if detail
                    .amount
                    .ct
                    .zip(transaction.ct)
                    .is_some_and(|(direct, nested)| direct != nested)
                    || detail
                        .amount
                        .currency
                        .as_deref()
                        .zip(transaction.currency.as_deref())
                        .is_some_and(|(direct, nested)| direct != nested)
                {
                    result.push(FinancialDiagnostic {
                        path: detail_path.clone(),
                        issue: FinancialIssue::ConflictingAlternatives,
                        raw: None,
                    });
                }
            }
            if detail
                .amount
                .currency
                .as_deref()
                .or(detail.signed_currency.as_deref())
                .zip(entry.amount.currency.as_deref())
                .is_some_and(|(own, parent)| own != parent)
            {
                result.push(FinancialDiagnostic {
                    path: format!("{detail_path}/{source}/@Ccy"),
                    issue: FinancialIssue::CurrencyMismatch,
                    raw: detail.amount.currency.clone(),
                });
            }
            if let Some(charges) = &detail.charges {
                charge_diagnostics(&format!("{detail_path}/Chrgs"), charges, &mut result);
            }
        }
    }
    result
}

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
    /// The statement carried no `Tp/CdOrPrtry` at all.
    ///
    /// Distinct from `Other("")`, which is a bank that sent an *empty* code.
    /// `Tp` is mandatory on a `CashBalance`, so this means the document is
    /// already outside the schema — and an unlabelled balance must not be
    /// mistaken for one whose label happened to be blank.
    Unspecified,
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
            Self::Unspecified => "",
        }
    }

    pub(crate) fn from_code(s: &str) -> Self {
        match s.trim() {
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
    /// `BOOK` — booked on the account, not proof of irrevocable settlement.
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
        match s.trim() {
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
    /// The balance, as far as the statement determined it.
    pub amount: ReportedAmount,
    /// Balance date exactly as the bank reported it.
    ///
    /// `Bal/Dt` is a date/time choice, so this is `"2026-07-20"` from one bank
    /// and `"2026-07-20T23:59:59"` from the next. Read [`date`](Self::date) for
    /// the day; this field is kept verbatim so nothing is lost.
    pub date_raw: String,
}

impl StatementBalance {
    /// Balance as a signed ct value (+credit, −debit), or `None` when the
    /// statement does not determine it.
    ///
    /// A balance defaulted to credit reports an overdraft as funds available,
    /// which is why there is no default. See [`ReportedAmount`].
    #[inline]
    #[must_use]
    pub fn signed_ct(&self) -> Option<i64> {
        self.amount.signed_ct()
    }

    /// The balance date, or `None` if the bank reported none this crate can read.
    #[must_use]
    pub fn date(&self) -> Option<crate::IsoDate> {
        crate::IsoDate::parse_date_part(&self.date_raw).ok()
    }
}

/// An amount as a bank statement reported it — and only as far as it did.
///
/// Money has three parts and a statement may fail to give any of them: a
/// magnitude (`Amt`), a currency (`Amt/@Ccy`) and a direction (`CdtDbtInd`).
/// **None of them is defaulted.** An unknown direction read as a credit turns a
/// EUR 1,000 debit into a EUR 1,000 credit — a double-sized error in a ledger —
/// so [`signed_ct`](Self::signed_ct) answers `None` instead, and the `*_raw`
/// fields keep what arrived for the operator to look at.
///
/// Every level of a camt document that carries money embeds this type, so the
/// rule has one definition rather than four copies of a convention.
///
/// ```
/// use sepa::parse_camt053;
///
/// # let xml = r#"<Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.053.001.08">
/// # <BkToCstmrStmt><GrpHdr><MsgId>M</MsgId><CreDtTm>2026-07-21T23:59:00</CreDtTm></GrpHdr><Stmt><Id>S</Id>
/// # <Ntry><Amt Ccy="EUR">1000.00</Amt><CdtDbtInd>DBTI</CdtDbtInd><Sts><Cd>BOOK</Cd></Sts>
/// #
/// #
/// # <BkTxCd><Domn><Cd>PMNT</Cd><Fmly><Cd>RDDT</Cd><SubFmlyCd>PMDD</SubFmlyCd></Fmly></Domn></BkTxCd>
/// # </Ntry>
/// # </Stmt></BkToCstmrStmt></Document>"#;
/// let doc = parse_camt053(xml)?;
/// let entry = &doc.statements[0].entries[0];
///
/// // `DBTI` is not a direction this crate knows, so there is no ledger figure.
/// assert_eq!(entry.signed_ct(), None);
/// // The magnitude is still known, and what arrived is still readable.
/// assert_eq!(entry.amount.ct, Some(100_000));
/// assert_eq!(entry.amount.direction_raw.as_deref(), Some("DBTI"));
/// # Ok::<(), sepa::Camt053ParseError>(())
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ReportedAmount {
    /// Parsed magnitude in **ct** — 1/100 of [`currency`](Self::currency).
    /// Signing rechecks public/restored observations before trusting them.
    ///
    /// `None` when `Amt` was absent, or carried a value this type cannot hold:
    /// a magnitude outside `i64` ct, or significant digits below one cent.
    pub ct: Option<i64>,
    /// ISO 4217 code from the `Ccy` attribute.
    ///
    /// `None` when the attribute was absent. It is **not** defaulted to `EUR`:
    /// camt statements are not EUR-only, and a fabricated currency propagates —
    /// a detail in a different currency is excluded from its entry's sum, so
    /// guessing here silently changes which transactions are counted.
    pub currency: Option<String>,
    /// Decoded `Amt` text, preserving padding/empty values but not source bytes.
    pub amount_raw: Option<String>,
    /// Credit (into the account) or debit (out of it).
    ///
    /// `None` when `CdtDbtInd` was absent or carried a code this crate does not
    /// recognise. The direction is the entire content of this field, and a
    /// wrong one is a two-for-one error in a ledger.
    pub direction: Option<CreditDebitIndicator>,
    /// `CdtDbtInd` exactly as the bank wrote it.
    pub direction_raw: Option<String>,
}

pub(crate) fn net_movement_ct(entries: &[CashEntry]) -> Option<i64> {
    let mut currency = None;
    let total = entries.iter().try_fold(0_i128, |total, entry| {
        let denomination = entry.amount.currency.as_deref()?;
        if currency.is_some_and(|previous| previous != denomination) {
            return None;
        }
        currency = Some(denomination);
        total.checked_add(i128::from(entry.signed_ct()?))
    })?;
    i64::try_from(total).ok()
}

impl ReportedAmount {
    /// Inspect current facts without mutating them or guessing missing values.
    ///
    /// `amount_path` locates the amount element. Supply `direction_path` only
    /// when a ledger sign is required; instructed amounts need no such sign.
    /// Currency checks cover spelling/reserved codes, not registry membership.
    #[must_use]
    pub fn diagnostics(
        &self,
        amount_path: &str,
        direction_path: Option<&str>,
    ) -> Vec<FinancialDiagnostic> {
        let mut result =
            decimal_diagnostics(amount_path, self.amount_raw.as_deref(), self.ct, true);
        result.extend(currency_diagnostics(
            &format!("{amount_path}/@Ccy"),
            self.currency.as_deref(),
        ));
        if let Some(path) = direction_path {
            let issue = match (self.direction_raw.as_deref(), self.direction) {
                (None, None) => Some(FinancialIssue::MissingDirection),
                (Some(raw), _) if raw.parse::<CreditDebitIndicator>().is_err() => {
                    Some(FinancialIssue::InvalidDirection)
                }
                (Some(raw), direction) if raw.parse::<CreditDebitIndicator>().ok() != direction => {
                    Some(FinancialIssue::InconsistentValue)
                }
                _ => None,
            };
            if let Some(issue) = issue {
                result.push(FinancialDiagnostic {
                    path: path.to_owned(),
                    issue,
                    raw: self.direction_raw.clone(),
                });
            }
        }
        result
    }

    /// The ledger figure: positive for a credit, negative for a debit.
    ///
    /// `None` when the statement did not determine it — either the magnitude or
    /// the direction/currency is missing or facts conflict. There is no safe substitute, and
    /// the raw fields hold whatever arrived so an importer can log *what* it
    /// could not read and escalate the row rather than post a guess.
    #[inline]
    #[must_use]
    pub fn signed_ct(&self) -> Option<i64> {
        let currency = self.currency.as_deref()?;
        if crate::Currency::new(currency).ok()?.code() != currency {
            return None;
        }
        let direction = self.direction?;
        let magnitude = self.ct?;
        if self
            .amount_raw
            .as_deref()
            .is_some_and(|raw| crate::ct_from_eur_str(raw).ok() != Some(magnitude))
            || self
                .direction_raw
                .as_deref()
                .is_some_and(|raw| raw.parse::<CreditDebitIndicator>().ok() != Some(direction))
        {
            return None;
        }
        signed(direction, magnitude)
    }

    /// Whether a ledger figure could be established at all.
    #[inline]
    #[must_use]
    pub fn is_resolved(&self) -> bool {
        self.signed_ct().is_some()
    }

    /// Whether this amount is denominated in `currency`, case-insensitively.
    ///
    /// `false` when either side did not state one: an unknown currency is not
    /// a match, because the alternative is summing figures that are not
    /// comparable.
    #[must_use]
    pub fn is_currency(&self, currency: Option<&str>) -> bool {
        match (self.currency.as_deref(), currency) {
            (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
            _ => false,
        }
    }

    /// Read `Amt` and `CdtDbtInd` from a node, inventing nothing.
    pub(crate) fn parse(node: &Node, tag: &str) -> Self {
        let (ct, currency, amount_raw) = match node.child(tag) {
            None => (None, None, None),
            Some(amt) => (
                crate::ct_from_eur_str(&amt.text)
                    .ok()
                    .filter(|value| *value >= 0),
                amt.attr("Ccy").map(str::to_owned),
                Some(amt.text.clone()),
            ),
        };
        let (direction, direction_raw) = match node.raw_text_of("CdtDbtInd") {
            None => (None, None),
            Some(raw) => (raw.parse().ok(), Some(raw.to_owned())),
        };
        Self {
            ct,
            currency,
            amount_raw,
            direction,
            direction_raw,
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
    /// What **this transaction** stated about its own amount (`TxDtls/Amt`, or
    /// `TxDtls/AmtDtls/TxAmt/Amt` in the versions that omit it).
    ///
    /// Distinct from [`signed_ct()`](Self::signed_ct), which is the
    /// *resolved* figure and may have been inherited from the entry. A detail
    /// in a different currency from its entry is real but is not the amount
    /// that hit the account, so it is not summable against the entry total.
    pub amount: ReportedAmount,
    /// Observed itemized amount route; `None` when no amount was reported.
    /// This metadata is not inferred from value equality or inherited money.
    pub amount_source: Option<DetailAmountSource>,
    /// Independent `AmtDtls/TxAmt/Amt` observation, even when direct `Amt` exists.
    /// The detail/entry direction applies; disagreement makes signing unavailable.
    pub transaction_amount: Option<ReportedAmount>,
    /// The resolved ledger amount in **ct**: positive for a credit, negative
    /// for a debit. `None` when it could not be established — see
    /// [`signed_ct`](Self::signed_ct).
    #[cfg_attr(feature = "serde", serde(skip_deserializing))]
    signed_amount_ct: Option<i64>,
    #[cfg_attr(feature = "serde", serde(skip_deserializing))]
    signed_currency: Option<String>,
    /// End-to-end reference from the original payment instruction.
    pub end_to_end_id: Option<String>,
    /// Mandate reference (direct debits, `MndtId`).
    pub mandate_id: Option<String>,
    /// SEPA Creditor Identifier (`CdtrId`).
    pub creditor_id: Option<String>,
    /// Remittance information / payment reference (`RmtInf/Ustrd`).
    ///
    /// `Ustrd` is unbounded in camt, and German banks routinely split one
    /// *Verwendungszweck* across several occurrences of 35 characters each.
    /// All of them are joined with a single space, so the reference reads as
    /// the payer wrote it rather than being cut at the first line.
    pub reference: Option<String>,
    /// Counterparty name (debtor for credits; creditor for debits).
    pub counterparty_name: Option<String>,
    /// Counterparty IBAN.
    pub counterparty_iban: Option<String>,
    /// ISO 20022 return reason code, when this transaction is a return.
    pub return_reason_code: Option<String>,
    /// `TxDtls/RtrInf/AddtlInf` — the bank's free text about the return.
    pub return_additional_info: Option<String>,
    /// `TxDtls/AddtlTxInf` — the bank's free text about this transaction.
    pub additional_info: Option<String>,
    /// `TxDtls/Chrgs` — charges attributed to this transaction.
    ///
    /// On a returned direct debit this is the return fee. See [`Charges`].
    pub charges: Option<Charges>,
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
    pub fn signed_ct(&self) -> Option<i64> {
        if let Some(transaction) = &self.transaction_amount {
            let nested = transaction.signed_ct()?;
            if self.amount.currency != transaction.currency
                || self.amount.signed_ct() != Some(nested)
            {
                return None;
            }
        }
        let cached = self.signed_amount_ct?;
        let currency = self.signed_currency.as_deref()?;
        if self.amount.amount_raw.is_some() {
            if self.amount.currency.as_deref() != Some(currency)
                || self.amount.signed_ct() != Some(cached)
            {
                return None;
            }
        } else {
            let direction = self.amount.direction?;
            if self.amount.ct.is_some()
                || self.amount.currency.is_some()
                || signed(direction, cached.checked_abs()?) != Some(cached)
                || self
                    .amount
                    .direction_raw
                    .as_deref()
                    .is_some_and(|raw| raw.parse::<CreditDebitIndicator>().ok() != Some(direction))
            {
                return None;
            }
        }
        Some(cached)
    }

    /// Whether this transaction is a return (Rückläufer).
    #[inline]
    #[must_use]
    pub const fn is_return(&self) -> bool {
        self.return_reason_code.is_some()
    }
}

// ── Charges ───────────────────────────────────────────────────────────────────

/// Observed shape of an itemized charge, not inferred from amount equality.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ChargeRecordSource {
    /// Modern `Chrgs/Rcrd` record.
    Record,
    /// Historical fields directly under `Chrgs`.
    LegacyFlat,
}

/// One charge a bank levied on an entry or a transaction (`Chrgs/Rcrd`).
///
/// For SEPA the case that matters is a **returned direct debit**: the debtor's
/// bank returns the collection and the creditor's bank passes on a return fee.
/// That fee is real money and has to be booked, and it is reported here rather
/// than in the entry amount.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ChargeRecord {
    /// Observed charge shape; absent for records without retained provenance.
    pub source: Option<ChargeRecordSource>,
    /// The charge, as far as the statement determined it.
    ///
    /// Missing and unrecognised directions remain unresolved. A fee is not
    /// evidence of a reported debit direction.
    pub amount: ReportedAmount,
    /// `ChrgInclInd` — whether this charge is **already included** in the
    /// entry's own amount.
    ///
    /// This is the field that decides whether a ledger adds the charge or not.
    /// `Some(true)` means the entry amount already carries it and posting it
    /// again double-counts; `Some(false)` means it is separate. `None` means
    /// the value was not determined — treat it as unresolved rather than
    /// picking a default.
    ///
    /// `None` covers two different situations, and
    /// [`included_in_amount_raw`](Self::included_in_amount_raw) is what tells
    /// them apart: the bank sent no `ChrgInclInd` at all (raw is `None` too),
    /// or it sent one this crate could not read (raw holds it verbatim).
    pub included_in_amount: Option<bool>,
    /// `ChrgInclInd` exactly as it arrived, when it arrived at all.
    ///
    /// `xs:boolean` has four lexical forms — `true`, `false`, `1`, `0` — and
    /// all four resolve. Anything else leaves
    /// [`included_in_amount`](Self::included_in_amount) `None` and lands here,
    /// so "the bank said nothing" and "the bank said `TRUE`" stay
    /// distinguishable. Conflating them would be the `CdtDbtInd` defect on a
    /// field that also decides whether money is posted twice.
    pub included_in_amount_raw: Option<String>,
    /// `Tp/Cd` or `Tp/Prtry` — what kind of charge, when the bank names one.
    pub type_code: Option<String>,
}

impl ChargeRecord {
    /// The charge as a signed ledger amount: negative for a debit. `None` when
    /// the statement did not determine it.
    #[inline]
    #[must_use]
    pub fn signed_ct(&self) -> Option<i64> {
        self.amount.signed_ct()
    }
}

/// The `Chrgs` block on an entry or a transaction.
///
/// ISO reshaped this across versions: up to `camt.05x.001.02` the charge sits
/// directly under `Chrgs` as an `Amt`/`CdtDbtInd` pair, and from `.001.04` it
/// moved into `0..n` `Rcrd` blocks with a total beside them. Both are read, and
/// the flat form is reported as a single record so a caller has one shape.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Charges {
    /// `TtlChrgsAndTaxAmt` — the bank's own total, when it states one.
    ///
    /// Present only in the newer shape, and not always then. Prefer
    /// [`total_signed_ct`](Self::total_signed_ct), which falls back to the
    /// records.
    pub total_ct: Option<i64>,
    /// ISO 4217 currency of `total_ct`.
    pub total_currency: Option<String>,
    /// Decoded `TtlChrgsAndTaxAmt` text, retained even when not representable.
    pub total_amount_raw: Option<String>,
    /// The individual charges.
    pub records: Vec<ChargeRecord>,
}

impl Charges {
    /// The summed signed charge in **ct**, or `None` on overflow or when any
    /// record's direction is unreadable.
    ///
    /// Taken from the records, which is the level that carries the
    /// credit/debit indicator; `TtlChrgsAndTaxAmt` is a magnitude with no sign
    /// of its own. All-or-nothing, like
    /// [`CashEntry::details_signed_sum_ct`]: a partial sum of fees understates
    /// them, and a fee understated is a fee somebody eats.
    #[must_use]
    pub fn total_signed_ct(&self) -> Option<i64> {
        if self.has_mixed_sources() {
            return None;
        }
        let currency = self.records.first()?.amount.currency.as_deref()?;
        if self.total_amount_raw.is_some() {
            if !self
                .total_currency
                .as_deref()?
                .eq_ignore_ascii_case(currency)
            {
                return None;
            }
            let magnitude = self.records.iter().try_fold(0i64, |sum, record| {
                let magnitude = record.amount.ct.filter(|value| *value >= 0)?;
                sum.checked_add(magnitude)
            })?;
            if self.total_ct? != magnitude {
                return None;
            }
        }
        self.records.iter().try_fold(0i64, |acc, record| {
            if !record.amount.is_currency(Some(currency)) {
                return None;
            }
            acc.checked_add(record.signed_ct()?)
        })
    }

    /// Whether every record says it is already inside the entry amount.
    ///
    /// `false` means not every record explicitly says included. It is **not**
    /// permission to post: inspect each record's inclusion, and escalate `None`.
    #[must_use]
    pub fn all_included_in_amount(&self) -> bool {
        !self.records.is_empty()
            && self
                .records
                .iter()
                .all(|r| r.included_in_amount == Some(true))
    }

    fn has_mixed_sources(&self) -> bool {
        self.records
            .iter()
            .any(|record| record.source == Some(ChargeRecordSource::LegacyFlat))
            && self
                .records
                .iter()
                .any(|record| record.source != Some(ChargeRecordSource::LegacyFlat))
    }

    fn parse(node: &Node) -> Option<Self> {
        let chrgs = node.child("Chrgs")?;
        // A charge record is never dropped for being unreadable: a lost fee is
        // a fee somebody eats, and `total_signed_ct` would then sum what is
        // left and report a confident, understated total.
        let record_of = |n: &Node, source| {
            let amount = ReportedAmount::parse(n, "Amt");
            let incl_raw = n.raw_text_of("ChrgInclInd");
            ChargeRecord {
                source: Some(source),
                amount,
                // The whole `xs:boolean` lexical space, and nothing else.
                included_in_amount: incl_raw.and_then(|v| match v.trim() {
                    "true" | "1" => Some(true),
                    "false" | "0" => Some(false),
                    _ => None,
                }),
                included_in_amount_raw: incl_raw.map(str::to_owned),
                type_code: n.child("Tp").and_then(Node::code).map(str::to_owned),
            }
        };

        let mut records: Vec<ChargeRecord> = chrgs
            .children_named("Rcrd")
            .map(|node| record_of(node, ChargeRecordSource::Record))
            .collect();
        // The pre-.001.04 shape puts the charge directly under `Chrgs`.
        let flat = record_of(chrgs, ChargeRecordSource::LegacyFlat);
        if flat.amount.amount_raw.is_some() {
            records.push(flat);
        }

        let total = ReportedAmount::parse(chrgs, "TtlChrgsAndTaxAmt");
        if records.is_empty() && total.amount_raw.is_none() {
            return None;
        }
        Some(Self {
            total_ct: total.ct,
            total_currency: total.currency,
            total_amount_raw: total.amount_raw,
            records,
        })
    }
}

// ── AccountRef ────────────────────────────────────────────────────────────────

/// The account a statement, report or notification is about (`Acct`).
///
/// ISO 20022 types the identifier as a choice — an `IBAN` **or** a proprietary
/// `Othr/Id` — so an account that is not IBAN-addressable is a legal thing for
/// a bank to send. Both are exposed, because collapsing the choice to a single
/// string cannot distinguish "no account identifier" from "an identifier that
/// is not an IBAN".
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AccountRef {
    /// `Acct/Id/IBAN`, when the account is IBAN-addressable.
    pub iban: Option<String>,
    /// `Acct/Id/Othr/Id` — the proprietary alternative to an IBAN.
    pub other_id: Option<String>,
    /// `Acct/Ccy` — the currency the account is denominated in.
    ///
    /// Worth checking before treating an entry amount as EUR: camt is not a
    /// EUR-only format, and an entry carries its own `Ccy` too.
    pub currency: Option<String>,
    /// BIC of the account servicing institution (`Acct/Svcr`).
    pub servicer_bic: Option<String>,
}

impl AccountRef {
    /// The IBAN, or the proprietary identifier when there is no IBAN.
    ///
    /// For display and logging. Match on [`iban`](Self::iban) directly when the
    /// distinction matters — only an IBAN can be validated or paid to.
    #[must_use]
    pub fn any_id(&self) -> Option<&str> {
        self.iban.as_deref().or(self.other_id.as_deref())
    }

    fn parse(node: &Node) -> Self {
        let Some(acct) = node.child("Acct") else {
            return Self::default();
        };
        let id = acct.child("Id");
        Self {
            iban: id.and_then(|i| i.text_of("IBAN")).map(str::to_owned),
            other_id: id
                .and_then(|i| i.text_at(&["Othr", "Id"]))
                .map(str::to_owned),
            currency: acct.text_of("Ccy").map(str::to_owned),
            servicer_bic: acct.child("Svcr").and_then(agent_bic).map(str::to_owned),
        }
    }
}

// ── BatchInfo ─────────────────────────────────────────────────────────────────

/// The `NtryDtls/Btch` block a bank attaches to an aggregate booking.
///
/// This is the element that closes the loop on a direct debit run: a batch
/// booking carries back the `PmtInfId` of the `PmtInf` that produced it, so a
/// camt entry can be matched to the group of a submitted pain.008 without
/// guessing from amounts and dates.
///
/// Its presence — not the number of `TxDtls` — is what the bank actually
/// asserts about a booking being aggregate; see
/// [`CashEntry::batch_booked`](CashEntry::batch_booked).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BatchInfo {
    /// `Btch/MsgId` — the `GrpHdr/MsgId` of the message that was submitted.
    pub message_id: Option<String>,
    /// `Btch/PmtInfId` — the `PmtInf/PmtInfId` of the group that was submitted.
    pub payment_info_id: Option<String>,
    /// `Btch/NbOfTxs` — how many transactions the bank aggregated.
    ///
    /// This is the bank's own count, which may exceed the number of `TxDtls`
    /// elements it chose to itemise.
    pub transaction_count: Option<u64>,
}

impl BatchInfo {
    fn parse(node: &Node) -> Self {
        Self {
            message_id: node.text_of("MsgId").map(str::to_owned),
            payment_info_id: node.text_of("PmtInfId").map(str::to_owned),
            transaction_count: node.text_of("NbOfTxs").and_then(|n| n.parse().ok()),
        }
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
    /// The booking amount, as far as the statement determined it.
    ///
    /// The entry is reported even when this resolves to nothing — a booking is
    /// never dropped for being unreadable, because a missing booking is
    /// indistinguishable from one that never happened.
    pub amount: ReportedAmount,
    /// Booking status.
    pub status: EntryStatus,
    /// `true` when the bank booked several transactions as one aggregate entry.
    ///
    /// Taken from the presence of [`batch`](Self::batch) — the bank's own
    /// assertion — and only otherwise inferred from there being more than one
    /// `TxDtls`. Counting details alone misses a batch of one, which is exactly
    /// the case where treating the entry as a single payment double-books it
    /// against a collection run.
    pub batch_booked: bool,
    /// The `NtryDtls/Btch` block, when the bank sent one.
    ///
    /// Carries the `MsgId` and `PmtInfId` of the message that produced this
    /// booking — see [`BatchInfo`].
    pub batch: Option<BatchInfo>,
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
    /// `Ntry/AddtlNtryInf` — the bank's free-text description of the booking.
    ///
    /// This is where several German banks put the text a customer sees on the
    /// statement, and for an entry with no `NtryDtls` at all it is often the
    /// only remittance information there is.
    pub additional_info: Option<String>,
    /// `Ntry/Chrgs` — charges the bank levied on this booking.
    ///
    /// A returned SEPA direct debit carries the return fee here or on the
    /// transaction detail, depending on the bank. Read
    /// [`Charges::all_included_in_amount`] before adding it to a ledger: a
    /// charge already inside the entry amount must not be posted twice.
    pub charges: Option<Charges>,
    /// Underlying transactions. Empty when the bank sends no `NtryDtls`.
    pub details: Vec<EntryDetail>,
}

impl CashEntry {
    /// Signed ledger amount: credit is positive (balance increase), debit is
    /// negative (balance decrease). `None` when the statement does not
    /// determine it.
    ///
    /// `None` means the statement did not say — the magnitude or the direction
    /// was missing or unreadable. Escalate the row; [`amount`](Self::amount)
    /// holds what arrived. There is no defensible default: an unknown direction
    /// taken for a credit is a double-sized error in a ledger.
    #[inline]
    #[must_use]
    pub fn signed_ct(&self) -> Option<i64> {
        self.amount.signed_ct()
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
        if self.details.is_empty() {
            return Some(0);
        }
        let currency = self.amount.currency.as_deref()?;
        let total = self.details.iter().try_fold(0_i128, |sum, detail| {
            if detail.signed_currency.as_deref() != Some(currency) {
                return None;
            }
            sum.checked_add(i128::from(detail.signed_ct()?))
        })?;
        i64::try_from(total).ok()
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
    ///   <BkToCstmrStmt><GrpHdr><MsgId>M</MsgId><CreDtTm>2026-07-21T23:59:00</CreDtTm></GrpHdr><Stmt><Id>S</Id>
    ///     <Ntry>
    ///       <Amt Ccy="EUR">125.00</Amt><CdtDbtInd>CRDT</CdtDbtInd><Sts><Cd>BOOK</Cd></Sts>
    ///       
    ///       <BkTxCd><Domn><Cd>PMNT</Cd><Fmly><Cd>RDDT</Cd><SubFmlyCd>PMDD</SubFmlyCd></Fmly></Domn></BkTxCd>
    /// <NtryDtls>
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
        match (self.details_signed_sum_ct(), self.signed_ct()) {
            (Some(sum), Some(total)) => sum == total,
            // The entry's own amount or direction is unreadable, so there is
            // nothing to reconcile *against*. That is not agreement.
            _ => false,
        }
    }
}

/// The date part of an optional bank-supplied date or date-time.
fn parse_date(raw: Option<&str>) -> Option<crate::IsoDate> {
    crate::IsoDate::parse_date_part(raw?).ok()
}

/// Apply a credit/debit indicator to a magnitude.
///
/// Public/restored observations cannot be trusted to contain a valid magnitude.
const fn signed(indicator: CreditDebitIndicator, amount_ct: i64) -> Option<i64> {
    if amount_ct < 0 {
        return None;
    }
    match indicator {
        CreditDebitIndicator::Credit => Some(amount_ct),
        CreditDebitIndicator::Debit => amount_ct.checked_neg(),
    }
}

/// Every `Ustrd` under `RmtInf`, joined with a single space.
///
/// `RmtInf/Ustrd` is `0..n`. A German bank splits a long *Verwendungszweck*
/// across several 35-character occurrences, so reading only the first one
/// truncates the reference an invoice is matched by — usually right where the
/// invoice number sits. `None` when there is no non-empty occurrence.
fn joined_unstructured(rmt_inf: &Node) -> Option<String> {
    let mut out = String::new();
    for part in rmt_inf.children_named("Ustrd") {
        if part.text.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&part.text);
    }
    (!out.is_empty()).then_some(out)
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

pub(crate) fn parse_balance(b: &Node) -> StatementBalance {
    let balance_type = b
        .path(&["Tp", "CdOrPrtry"])
        .and_then(Node::code)
        .map_or(BalanceType::Unspecified, BalanceType::from_code);

    // `Dt` is a DateAndDateTimeChoice: `Dt/Dt` or `Dt/DtTm`.
    let date = b
        .child("Dt")
        .and_then(|d| d.text_of("Dt").or_else(|| d.text_of("DtTm")))
        .unwrap_or_default()
        .to_owned();

    StatementBalance {
        balance_type,
        amount: ReportedAmount::parse(b, "Amt"),
        date_raw: date,
    }
}

pub(crate) fn parse_entry(e: &Node) -> CashEntry {
    let amount = ReportedAmount::parse(e, "Amt");

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
    // `Btch` is the bank's own statement that this booking is an aggregate,
    // and it is authoritative: a batch of one transaction carries `Btch` but
    // only one `TxDtls`, so counting details alone would call it a single
    // payment.
    let batch = details_parent.child("Btch").map(BatchInfo::parse);
    let batch_booked = batch.is_some() || detail_count > 1;
    // The entry total may stand in for a detail's amount only when there is
    // exactly one transaction to attribute it to. Spreading it across a batch
    // would multiply the booking, so the bank's own `NbOfTxs` overrules the
    // number of details it happened to itemise.
    let sole_detail = detail_count == 1
        && batch
            .as_ref()
            .and_then(|b| b.transaction_count)
            .is_none_or(|n| n == 1);
    let details = details_parent
        .children_named("TxDtls")
        .map(|td| {
            parse_detail(
                td,
                &EntryContext {
                    amount: &amount,
                    sole_detail,
                },
            )
        })
        .collect();

    CashEntry {
        amount,
        status,
        batch_booked,
        batch,
        booking_date_raw: date_of("BookgDt"),
        value_date_raw: date_of("ValDt"),
        account_servicer_ref: e.text_of("AcctSvcrRef").map(str::to_owned),
        bank_tx_code,
        additional_info: e.text_of("AddtlNtryInf").map(str::to_owned),
        charges: Charges::parse(e),
        details,
    }
}

/// What the enclosing `Ntry` says, for resolving a detail's amount and sign.
pub(crate) struct EntryContext<'a> {
    pub(crate) amount: &'a ReportedAmount,
    pub(crate) sole_detail: bool,
}

pub(crate) fn parse_detail(td: &Node, entry: &EntryContext<'_>) -> EntryDetail {
    let refs = td.child("Refs");
    let ref_of = |tag: &str| refs.and_then(|r| r.text_of(tag)).map(str::to_owned);

    // `TxDtls/CdtDbtInd` is optional and overrides the entry's when present —
    // that is how a single returned collection inside a credit batch is
    // reported.
    let mut amount = ReportedAmount::parse(td, "Amt");
    let mut amount_source = td.child("Amt").map(|_| DetailAmountSource::Direct);
    // `TxDtls/CdtDbtInd` is optional and overrides the entry's when present.
    // When it is *absent* the entry's applies; when it is present but
    // unreadable it does not silently fall back, because that would resolve a
    // direction the detail itself contradicted.
    if amount.direction_raw.is_none() {
        amount.direction = entry.amount.direction;
    }
    let indicator = amount.direction;

    // Counterparty: for a credit the other side is the debtor, for a debit the creditor.
    let parties = td.child("RltdPties");
    let (name_tag, acct_tag) = match indicator {
        // An unknown direction cannot pick a side; `Dbtr`/`Cdtr` are then read
        // in that order so a name is still surfaced where the file has one.
        Some(CreditDebitIndicator::Credit) | None => ("Dbtr", "DbtrAcct"),
        Some(CreditDebitIndicator::Debit) => ("Cdtr", "CdtrAcct"),
    };

    // `TxDtls/Amt` is the transaction amount; `AmtDtls/TxAmt/Amt` carries the
    // same figure in the messages that omit the former. The direction stays the
    // one resolved above — only the magnitude and currency come from the
    // fallback.
    let transaction_amount = td.path(&["AmtDtls", "TxAmt"]).map(|transaction| {
        let mut reported = ReportedAmount::parse(transaction, "Amt");
        reported.direction = amount.direction;
        reported.direction_raw.clone_from(&amount.direction_raw);
        reported
    });
    if amount.amount_raw.is_none()
        && let Some(fallback) = &transaction_amount
    {
        amount.ct = fallback.ct;
        amount.currency.clone_from(&fallback.currency);
        amount.amount_raw.clone_from(&fallback.amount_raw);
        amount_source = Some(DetailAmountSource::TransactionAmount);
    }

    let signed_amount_ct = if amount.amount_raw.is_some() {
        // A foreign-currency transaction, or one whose currency (or the
        // entry's) the statement did not give: either way the figure is not
        // what hit the account and must not be summed against the entry total.
        if amount.is_currency(entry.amount.currency.as_deref()) {
            amount.signed_ct()
        } else {
            None
        }
    } else if entry.sole_detail && amount_source.is_none() {
        // A sole detail with no itemised amount inherits the entry's, which is
        // safe precisely because there is one transaction to attribute it to.
        entry.amount.signed_ct()
    } else {
        None
    };

    EntryDetail {
        amount,
        amount_source,
        transaction_amount,
        signed_amount_ct,
        signed_currency: entry.amount.currency.clone(),
        end_to_end_id: ref_of("EndToEndId"),
        mandate_id: ref_of("MndtId"),
        creditor_id: ref_of("CdtrId"),
        reference: td.child("RmtInf").and_then(joined_unstructured),
        counterparty_name: party_name(parties, name_tag),
        counterparty_iban: parties
            .and_then(|p| p.text_at(&[acct_tag, "Id", "IBAN"]))
            .map(str::to_owned),
        return_reason_code: td
            .path(&["RtrInf", "Rsn"])
            .and_then(Node::code)
            .map(str::to_owned),
        return_additional_info: td.text_at(&["RtrInf", "AddtlInf"]).map(str::to_owned),
        additional_info: td.text_of("AddtlTxInf").map(str::to_owned),
        charges: Charges::parse(td),
    }
}

// ── shared account / group helpers ────────────────────────────────────────────

/// The account a statement, report or notification is about.
pub(crate) fn account_of(node: &Node) -> AccountRef {
    AccountRef::parse(node)
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
    node.children_named("Ntry").map(parse_entry).collect()
}

/// Read `Bal` children into balances (camt.052 and camt.053 only).
pub(crate) fn balances_of(node: &Node) -> Vec<StatementBalance> {
    node.children_named("Bal").map(parse_balance).collect()
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::{CashEntry, CreditDebitIndicator};

    #[test]
    fn decoded_money_is_retained_and_conflicting_public_facts_are_unresolved() {
        let mut parsed =
            entry("<Ntry><Amt Ccy=\"EUR\"> \n1.00\t </Amt><CdtDbtInd>CRDT</CdtDbtInd></Ntry>");
        assert_eq!(parsed.amount.amount_raw.as_deref(), Some(" \n1.00\t "));
        assert_eq!(parsed.signed_ct(), Some(100));
        parsed.amount.ct = Some(200);
        assert_eq!(parsed.signed_ct(), None);
        parsed.amount.ct = Some(100);
        parsed.amount.direction = Some(CreditDebitIndicator::Debit);
        assert_eq!(parsed.signed_ct(), None);
        let mut detail = entry("<Ntry><Amt Ccy=\"EUR\">1.00</Amt><CdtDbtInd>CRDT</CdtDbtInd><NtryDtls><TxDtls><Amt Ccy=\"EUR\">1.00</Amt></TxDtls></NtryDtls></Ntry>").details.remove(0);
        detail.amount.ct = Some(-100);
        assert_eq!(detail.signed_ct(), None);
        let mut inherited = entry("<Ntry><Amt Ccy=\"EUR\">1.00</Amt><CdtDbtInd>CRDT</CdtDbtInd><NtryDtls><TxDtls/></NtryDtls></Ntry>").details.remove(0);
        assert_eq!(inherited.signed_ct(), Some(100));
        inherited.amount.direction = Some(CreditDebitIndicator::Debit);
        assert_eq!(inherited.signed_ct(), None);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn restoration_cannot_inject_a_trusted_detail_cache() {
        let detail = entry("<Ntry><Amt Ccy=\"EUR\">1.00</Amt><CdtDbtInd>CRDT</CdtDbtInd><NtryDtls><TxDtls/></NtryDtls></Ntry>").details.remove(0);
        assert_eq!(detail.signed_ct(), Some(100));
        let mut value = serde_json::to_value(&detail).unwrap();
        value["signed_amount_ct"] = serde_json::json!(900);
        let restored: super::EntryDetail = serde_json::from_value(value).unwrap();
        assert_eq!(restored.signed_ct(), None);
    }

    #[test]
    fn invalid_magnitudes_and_missing_charge_directions_never_resolve() {
        for magnitude in [-1, i64::MIN] {
            let amount = super::ReportedAmount {
                ct: Some(magnitude),
                currency: Some("EUR".to_owned()),
                direction: Some(CreditDebitIndicator::Debit),
                ..Default::default()
            };
            assert_eq!(amount.signed_ct(), None);
            assert!(!amount.is_resolved());
        }
        let parsed = entry("<Ntry><Chrgs><Rcrd><Amt Ccy=\"EUR\">3.00</Amt></Rcrd></Chrgs></Ntry>");
        assert_eq!(parsed.charges.unwrap().total_signed_ct(), None);
        let parsed = entry("<Ntry><Amt Ccy=\"EUR\">-1.00</Amt><CdtDbtInd>DBIT</CdtDbtInd></Ntry>");
        assert_eq!(parsed.signed_ct(), None);
    }

    #[test]
    fn unrecorded_charge_totals_and_mixed_currencies_are_not_zero_or_a_sum() {
        let parsed = entry(
            "<Ntry><Chrgs><TtlChrgsAndTaxAmt Ccy=\"USD\">not-money</TtlChrgsAndTaxAmt></Chrgs></Ntry>",
        );
        assert!(parsed.charges.is_some());
        assert_eq!(parsed.charges.unwrap().total_signed_ct(), None);
        let parsed = entry(
            "<Ntry><Chrgs><Rcrd><Amt Ccy=\"EUR\">1.00</Amt><CdtDbtInd>DBIT</CdtDbtInd></Rcrd><Rcrd><Amt Ccy=\"USD\">2.00</Amt><CdtDbtInd>DBIT</CdtDbtInd></Rcrd></Chrgs></Ntry>",
        );
        assert_eq!(parsed.charges.unwrap().total_signed_ct(), None);
    }

    /// Parse a camt.053 statement whose single `Ntry` is `entry_xml`.
    fn entry(entry_xml: &str) -> CashEntry {
        let xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.053.001.08">
  <BkToCstmrStmt><GrpHdr><MsgId>M</MsgId><CreDtTm>2026-07-21T23:59:00</CreDtTm></GrpHdr><Stmt><Id>S</Id>
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
        assert_eq!(e.details[0].amount.ct, None, "nothing was itemised");
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
        assert_eq!(
            e.details[0].amount.direction,
            Some(CreditDebitIndicator::Credit)
        );
        assert_eq!(
            e.details[1].amount.direction,
            Some(CreditDebitIndicator::Debit)
        );
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
        assert_eq!(e.details[0].amount.ct, Some(10_000));
        assert_eq!(e.details[0].amount.currency.as_deref(), Some("USD"));
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
    fn a_return_fee_is_read_at_either_level_and_in_either_shape() {
        // A returned direct debit is where charges actually matter: the fee is
        // real money the creditor is out, and it is reported beside the entry
        // rather than inside its amount.
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.053.001.08">
  <BkToCstmrStmt><GrpHdr><MsgId>M</MsgId><CreDtTm>2026-07-21T23:59:00</CreDtTm></GrpHdr><Stmt><Id>S</Id><Acct><Id><IBAN>DE89370400440532013000</IBAN></Id></Acct><Bal><Tp><CdOrPrtry><Cd>CLBD</Cd></CdOrPrtry></Tp><Amt Ccy="EUR">0.00</Amt><CdtDbtInd>CRDT</CdtDbtInd><Dt><Dt>2026-07-21</Dt></Dt></Bal>
    <Ntry>
      <Amt Ccy="EUR">75.00</Amt><CdtDbtInd>DBIT</CdtDbtInd><Sts><Cd>BOOK</Cd></Sts>
      <BkTxCd><Domn><Cd>PMNT</Cd><Fmly><Cd>RDDT</Cd><SubFmlyCd>PMDD</SubFmlyCd></Fmly></Domn></BkTxCd>
        <Chrgs>
        <TtlChrgsAndTaxAmt Ccy="EUR">3.00</TtlChrgsAndTaxAmt>
        <Rcrd><Amt Ccy="EUR">3.00</Amt><CdtDbtInd>DBIT</CdtDbtInd>
              <ChrgInclInd>false</ChrgInclInd>
              <Tp><Prtry><Id>RETURN_FEE</Id></Prtry></Tp></Rcrd>
      </Chrgs>
      
        <NtryDtls><TxDtls>
        <Amt Ccy="EUR">75.00</Amt>
        <Chrgs><Rcrd><Amt Ccy="EUR">1.50</Amt><CdtDbtInd>DBIT</CdtDbtInd></Rcrd></Chrgs>
        <RtrInf><Rsn><Cd>MS02</Cd></Rsn></RtrInf>
      </TxDtls></NtryDtls>
    </Ntry>
  </Stmt></BkToCstmrStmt>
</Document>"#;
        let doc = crate::parse_camt053(xml).unwrap();
        let entry = &doc.statements[0].entries[0];

        let charges = entry.charges.as_ref().expect("Ntry/Chrgs must be read");
        assert_eq!(charges.total_ct, Some(300));
        assert_eq!(charges.total_signed_ct(), Some(-300));
        assert_eq!(charges.records[0].type_code.as_deref(), Some("RETURN_FEE"));
        // Stated as separate, so a ledger must post it in addition to the entry.
        assert!(!charges.all_included_in_amount());
        assert_eq!(charges.records[0].included_in_amount, Some(false));

        // The detail carries its own, which is where several German banks put it.
        let detail = &entry.details[0];
        assert!(detail.is_return());
        assert_eq!(
            detail.charges.as_ref().unwrap().total_signed_ct(),
            Some(-150)
        );

        // The pre-.001.04 shape puts the charge directly under `Chrgs`.
        let flat = xml.replace(
            "<Chrgs><Rcrd><Amt Ccy=\"EUR\">1.50</Amt><CdtDbtInd>DBIT</CdtDbtInd></Rcrd></Chrgs>",
            "<Chrgs><Amt Ccy=\"EUR\">1.50</Amt><CdtDbtInd>DBIT</CdtDbtInd></Chrgs>",
        );
        let doc = crate::parse_camt053(&flat).unwrap();
        let detail = &doc.statements[0].entries[0].details[0];
        assert_eq!(
            detail.charges.as_ref().unwrap().total_signed_ct(),
            Some(-150)
        );
    }

    #[test]
    fn a_charge_that_does_not_say_whether_it_is_included_is_not_assumed_to_be() {
        // `ChrgInclInd` is optional, and "the bank did not say" is a third
        // answer. Treating silence as "included" would silently drop a fee;
        // treating it as "separate" would silently double-count one. Both are
        // wrong, so `all_included_in_amount` is false and the caller decides.
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.053.001.08">
  <BkToCstmrStmt><GrpHdr><MsgId>M</MsgId><CreDtTm>2026-07-21T23:59:00</CreDtTm></GrpHdr><Stmt><Id>S</Id><Acct><Id><IBAN>DE89370400440532013000</IBAN></Id></Acct><Bal><Tp><CdOrPrtry><Cd>CLBD</Cd></CdOrPrtry></Tp><Amt Ccy="EUR">0.00</Amt><CdtDbtInd>CRDT</CdtDbtInd><Dt><Dt>2026-07-21</Dt></Dt></Bal>
    <Ntry><Amt Ccy="EUR">10.00</Amt><CdtDbtInd>DBIT</CdtDbtInd><Sts><Cd>BOOK</Cd></Sts>
      <BkTxCd><Domn><Cd>PMNT</Cd><Fmly><Cd>RDDT</Cd><SubFmlyCd>PMDD</SubFmlyCd></Fmly></Domn></BkTxCd>
        <Chrgs><Rcrd><Amt Ccy="EUR">2.00</Amt></Rcrd></Chrgs>
        
      </Ntry>
  </Stmt></BkToCstmrStmt>
</Document>"#;
        let doc = crate::parse_camt053(xml).unwrap();
        let charges = doc.statements[0].entries[0].charges.as_ref().unwrap();
        assert_eq!(charges.records[0].included_in_amount, None);
        assert!(!charges.all_included_in_amount());
        assert_eq!(charges.records[0].signed_ct(), None);
    }

    #[test]
    fn the_btch_element_identifies_the_submitted_group() {
        // The reconciliation loop closes here: a batch booking names the
        // PmtInfId of the PmtInf that produced it.
        let e = entry(
            r#"<Ntry>
                 <Amt Ccy="EUR">125.00</Amt><CdtDbtInd>DBIT</CdtDbtInd>
                 <NtryDtls>
                   <Btch>
                     <MsgId>DD-2026-07</MsgId>
                     <PmtInfId>DD-2026-07-1</PmtInfId>
                     <NbOfTxs>2</NbOfTxs>
                   </Btch>
                   <TxDtls><Amt Ccy="EUR">100.00</Amt></TxDtls>
                   <TxDtls><Amt Ccy="EUR">25.00</Amt></TxDtls>
                 </NtryDtls>
               </Ntry>"#,
        );
        let batch = e.batch.as_ref().unwrap();
        assert_eq!(batch.message_id.as_deref(), Some("DD-2026-07"));
        assert_eq!(batch.payment_info_id.as_deref(), Some("DD-2026-07-1"));
        assert_eq!(batch.transaction_count, Some(2));
        assert!(e.batch_booked);
        assert!(e.details_reconcile());
    }

    #[test]
    fn a_batch_of_one_is_still_a_batch() {
        // Regression: `batch_booked` was `TxDtls count > 1`, so a single-
        // transaction collection run read as an ordinary payment — and the
        // entry total was then attributed to the one itemised detail even
        // though the bank said it aggregated three.
        let e = entry(
            r#"<Ntry>
                 <Amt Ccy="EUR">125.00</Amt><CdtDbtInd>DBIT</CdtDbtInd>
                 <NtryDtls>
                   <Btch><PmtInfId>DD-1</PmtInfId><NbOfTxs>3</NbOfTxs></Btch>
                   <TxDtls><Refs><EndToEndId>E1</EndToEndId></Refs></TxDtls>
                 </NtryDtls>
               </Ntry>"#,
        );
        assert!(e.batch_booked, "Btch is the bank's own assertion");
        assert_eq!(
            e.details[0].signed_ct(),
            None,
            "3 transactions were aggregated; the entry total is not this one's"
        );
        assert!(!e.details_reconcile());

        // With NbOfTxs = 1 the identity is safe again.
        let single = entry(
            r#"<Ntry>
                 <Amt Ccy="EUR">75.00</Amt><CdtDbtInd>DBIT</CdtDbtInd>
                 <NtryDtls>
                   <Btch><NbOfTxs>1</NbOfTxs></Btch>
                   <TxDtls><Refs><MndtId>MND-1</MndtId></Refs></TxDtls>
                 </NtryDtls>
               </Ntry>"#,
        );
        assert!(single.batch_booked);
        assert_eq!(single.details[0].signed_ct(), Some(-7_500));
    }

    #[test]
    fn the_banks_free_text_survives_at_both_levels() {
        // `AddtlNtryInf` is where several German banks put the statement text,
        // and for an entry with no NtryDtls it is the only description there
        // is. Both it and `AddtlTxInf` used to be dropped on the floor.
        let e = entry(
            r#"<Ntry>
                 <Amt Ccy="EUR">75.00</Amt><CdtDbtInd>DBIT</CdtDbtInd>
                 <AddtlNtryInf>SEPA-LASTSCHRIFT EINZUG</AddtlNtryInf>
                 <NtryDtls><TxDtls>
                   <AddtlTxInf>Kundennummer 4711</AddtlTxInf>
                   <RtrInf><Rsn><Cd>AM04</Cd></Rsn><AddtlInf>Konto nicht gedeckt</AddtlInf></RtrInf>
                 </TxDtls></NtryDtls>
               </Ntry>"#,
        );
        assert_eq!(
            e.additional_info.as_deref(),
            Some("SEPA-LASTSCHRIFT EINZUG")
        );
        assert_eq!(
            e.details[0].additional_info.as_deref(),
            Some("Kundennummer 4711")
        );
        assert_eq!(e.details[0].return_reason_code.as_deref(), Some("AM04"));
        assert_eq!(
            e.details[0].return_additional_info.as_deref(),
            Some("Konto nicht gedeckt")
        );
    }

    #[test]
    fn a_remittance_split_across_several_ustrd_is_read_whole() {
        // Regression: only the first `Ustrd` was read. German banks split a
        // long Verwendungszweck into 35-character occurrences, so the invoice
        // number — which usually sits at the end — was being thrown away.
        let e = entry(
            r#"<Ntry>
                 <Amt Ccy="EUR">75.00</Amt><CdtDbtInd>CRDT</CdtDbtInd>
                 <NtryDtls><TxDtls><RmtInf>
                   <Ustrd>Rechnung 2026-07 Teilzahlung 1 von</Ustrd>
                   <Ustrd>3, Kundennummer 4711</Ustrd>
                   <Ustrd>RG-NR 2026-000815</Ustrd>
                 </RmtInf></TxDtls></NtryDtls>
               </Ntry>"#,
        );
        assert_eq!(
            e.reference(),
            Some("Rechnung 2026-07 Teilzahlung 1 von 3, Kundennummer 4711 RG-NR 2026-000815")
        );

        // A single occurrence is unchanged, and an empty one reads as absent.
        let one = entry(
            r#"<Ntry><Amt Ccy="EUR">1.00</Amt><CdtDbtInd>CRDT</CdtDbtInd>
                 <NtryDtls><TxDtls><RmtInf><Ustrd>Miete Juli</Ustrd></RmtInf></TxDtls></NtryDtls>
               </Ntry>"#,
        );
        assert_eq!(one.reference(), Some("Miete Juli"));
        let none = entry(
            r#"<Ntry><Amt Ccy="EUR">1.00</Amt><CdtDbtInd>CRDT</CdtDbtInd>
                 <NtryDtls><TxDtls><RmtInf><Ustrd/></RmtInf></TxDtls></NtryDtls>
               </Ntry>"#,
        );
        assert_eq!(none.reference(), None);
    }

    #[test]
    fn an_entry_without_details_reconciles_trivially() {
        let e = entry(r#"<Ntry><Amt Ccy="EUR">10.00</Amt><CdtDbtInd>CRDT</CdtDbtInd></Ntry>"#);
        assert_eq!(e.details.len(), 0);
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
        assert_eq!(e.signed_ct(), Some(12_500));
        assert!(!e.details_reconcile());
    }

    /// Every type in this module that reports a ledger figure is accounted
    /// for, so a new one cannot be added silently.
    ///
    /// `tests/conformance.rs::cross_level_consistency` requires these types to
    /// answer identically for an unreadable code — but it *names* them, and a
    /// test whose coverage is a list stops covering things. The list is
    /// checked against the module's own source here, so a fifth `signed_ct`
    /// fails the build until somebody classifies it.
    #[test]
    fn every_type_that_reports_money_is_covered_by_the_cross_level_gate() {
        const SRC: &str = include_str!("camt.rs");

        /// Types whose `signed_ct` delegates to [`ReportedAmount`], so the
        /// resolution rule is written once. `tests/conformance.rs` feeds all
        /// of these one unreadable code and requires identical answers.
        const DELEGATES: &[&str] = &["StatementBalance", "ChargeRecord", "CashEntry"];
        /// Types that deliberately answer differently, each with a reason.
        const DIVERGES: &[(&str, &str)] = &[
            ("ReportedAmount", "the definition the others delegate to"),
            (
                "EntryDetail",
                "reports a *resolved* figure that may be inherited from its \
                 entry, so it is not a function of its own `amount` alone",
            ),
        ];

        // Walk the source for `impl <Type> {` blocks containing `fn signed_ct`.
        let mut found: Vec<&str> = Vec::new();
        let mut current: Option<&str> = None;
        for line in SRC.lines() {
            if let Some(rest) = line.strip_prefix("impl ") {
                current = rest.split_whitespace().next().map(str::trim);
            }
            if line.contains("fn signed_ct(")
                && let Some(ty) = current
            {
                found.push(ty);
            }
        }
        found.sort_unstable();
        found.dedup();

        let mut accounted: Vec<&str> = DELEGATES
            .iter()
            .copied()
            .chain(DIVERGES.iter().map(|(t, _)| *t))
            .collect();
        accounted.sort_unstable();

        assert_eq!(
            found, accounted,
            "a `signed_ct` in this module is not accounted for.\n\
             Add it to DELEGATES (and to the four types named in \
             tests/conformance.rs::cross_level_consistency), or to DIVERGES \
             with the reason it may answer differently.\n\
             found:     {found:?}\n\
             accounted: {accounted:?}"
        );

        // The guard is only worth having if it can fail, so prove the walk
        // actually sees something rather than comparing two empty lists.
        assert!(
            found.len() >= 5,
            "the source walk found {} types — it has stopped working",
            found.len()
        );
    }
}
