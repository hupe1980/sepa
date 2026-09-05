//! ISO 20022 camt.029 — Resolution of Investigation: the answer to a recall.
//!
//! A [`camt.055`](crate::camt055) request asks the bank to stop a payment. This
//! is what comes back, and until it does **nothing has been cancelled** — the
//! request was a request. The distinction is the whole reason this parser
//! exists separately from the pain.002 one: a pain.002 tells you whether a file
//! was *accepted*, and a camt.029 tells you whether a recall *worked*, which
//! are different questions about different money.
//!
//! ## Reading one
//!
//! Three levels carry a status, and which one answers depends on how far the
//! bank got — exactly as in [`pain.002`](crate::pain002):
//!
//! | Level | Element | Means |
//! |---|---|---|
//! | Message | `Sts/Conf` | the outcome of the case as a whole — see [`ResolutionOutcome`] |
//! | Group | `GrpCxlSts`, `PmtInfCxlSts` | a whole submission or `PmtInf` was accepted or refused |
//! | Transaction | `TxCxlSts` | one named transaction was accepted or refused |
//!
//! A refusal at message level carries no transaction blocks at all, so a reader
//! that only walks `TxInfAndSts` sees an empty document and concludes the
//! recall succeeded. [`Camt029Document::is_accepted`] and
//! [`Camt029Document::rejection_reasons`] read all three.
//!
//! ## `PDCR` is not an answer yet
//!
//! `ACCR` accepted, `RJCR` rejected, **`PDCR` pending** — the bank has taken
//! the case and has not resolved it. Treating pending as either outcome is the
//! mistake that either double-collects or writes off money that is coming back,
//! so [`CancellationStatus::is_final`] exists and is `false` for it.
//!
//! ## Version
//!
//! `camt.029.001.06`, the answer to `camt.055.001.05` under the DFÜ-Abkommen.
//! The parser is namespace-agnostic like the others, so a bank that sends a
//! neighbouring version is read rather than refused.
//!
//! ## Example
//!
//! ```
//! use sepa::{parse_camt029, CancellationStatus};
//!
//! let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
//! <Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.029.001.06">
//!   <RsltnOfInvstgtn>
//!     <Assgnmt><Id>RES-1</Id>
//!       <Assgnr><Agt><FinInstnId><BICFI>COBADEFFXXX</BICFI></FinInstnId></Agt></Assgnr>
//!       <Assgne><Pty><Nm>Stadtwerke GmbH</Nm></Pty></Assgne>
//!       <CreDtTm>2026-07-15T14:02:00</CreDtTm></Assgnmt>
//!     <RslvdCase><Id>CXL-1</Id><Cretr><Pty><Nm>Stadtwerke GmbH</Nm></Pty></Cretr></RslvdCase>
//!     <Sts><Conf>CNCL</Conf></Sts>
//!     <CxlDtls>
//!       <OrgnlPmtInfAndSts>
//!         <OrgnlPmtInfId>PMT-A</OrgnlPmtInfId>
//!         <TxInfAndSts>
//!           <OrgnlEndToEndId>E2E-1</OrgnlEndToEndId>
//!           <TxCxlSts>ACCR</TxCxlSts>
//!         </TxInfAndSts>
//!       </OrgnlPmtInfAndSts>
//!     </CxlDtls>
//!   </RsltnOfInvstgtn>
//! </Document>"#;
//!
//! let doc = parse_camt029(xml)?;
//! assert_eq!(doc.resolved_case_id.as_deref(), Some("CXL-1"));
//! assert!(doc.is_accepted());
//! let tx = &doc.groups[0].payment_infos[0].transactions[0];
//! assert_eq!(tx.original_end_to_end_id.as_deref(), Some("E2E-1"));
//! assert_eq!(tx.status, Some(CancellationStatus::Accepted));
//! # Ok::<(), sepa::Camt029ParseError>(())
//! ```

use crate::camt::amount_of;
use crate::date::IsoDate;
use crate::xml::{Document, Node, XmlError};

/// The XML namespace of the one camt.029 version SEPA uses.
pub const NAMESPACE: &str = "urn:iso:std:iso:20022:tech:xsd:camt.029.001.06";

// ── CancellationStatus ────────────────────────────────────────────────────────

/// Whether a cancellation was accepted, refused or is still open.
///
/// The same three codes appear at group level (`GrpCxlSts`, `PmtInfCxlSts`,
/// which add `PACR`) and at transaction level (`TxCxlSts`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum CancellationStatus {
    /// `ACCR` — accepted. The payment will not go out, or is being returned.
    Accepted,
    /// `RJCR` — rejected. The payment stands; see the reason codes.
    Rejected,
    /// `PDCR` — **pending**. The bank has the case and has not decided.
    ///
    /// Neither outcome. See [`is_final`](Self::is_final).
    Pending,
    /// `PACR` — partially accepted, at group level only: some of the named
    /// transactions were cancelled and some were not.
    PartiallyAccepted,
    /// Any other status code the bank sent.
    Other(String),
}

impl CancellationStatus {
    /// The ISO 20022 code string.
    #[must_use]
    pub fn as_code(&self) -> &str {
        match self {
            Self::Accepted => "ACCR",
            Self::Rejected => "RJCR",
            Self::Pending => "PDCR",
            Self::PartiallyAccepted => "PACR",
            Self::Other(s) => s,
        }
    }

    pub(crate) fn from_code(s: &str) -> Self {
        match s.trim().to_ascii_uppercase().as_str() {
            "ACCR" => Self::Accepted,
            "RJCR" => Self::Rejected,
            "PDCR" => Self::Pending,
            "PACR" => Self::PartiallyAccepted,
            other => Self::Other(other.to_owned()),
        }
    }

    /// Whether the cancellation went through.
    ///
    /// `false` for [`PartiallyAccepted`](Self::PartiallyAccepted): some of it
    /// did not, and which part is in the transaction blocks.
    #[must_use]
    pub const fn is_accepted(&self) -> bool {
        matches!(self, Self::Accepted)
    }

    /// Whether the cancellation was refused.
    #[must_use]
    pub const fn is_rejected(&self) -> bool {
        matches!(self, Self::Rejected)
    }

    /// Whether the bank has decided at all.
    ///
    /// `false` for [`Pending`](Self::Pending) and for an unrecognised code.
    /// Posting a pending recall either way is how a collection gets taken twice
    /// or written off while the money is still coming back.
    #[must_use]
    pub const fn is_final(&self) -> bool {
        matches!(
            self,
            Self::Accepted | Self::Rejected | Self::PartiallyAccepted
        )
    }
}

impl std::fmt::Display for CancellationStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_code())
    }
}

// ── ResolutionOutcome ─────────────────────────────────────────────────────────

/// The message-level outcome of the case (`Sts/Conf`).
///
/// `InvestigationExecutionConfirmation3Code` covers every investigation type
/// ISO defines, most of which SEPA never uses. The variants named here are the
/// ones a cancellation case can produce; everything else is
/// [`Other`](Self::Other) rather than dropped.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ResolutionOutcome {
    /// `CNCL` — the payment was cancelled. The recall worked.
    Cancelled,
    /// `PECR` — partially executed: some transactions were cancelled.
    PartiallyCancelled,
    /// `RJCR` — the cancellation was rejected. The payment stands.
    RejectedCancellation,
    /// `PDCR` — pending. The case is open and has no outcome yet.
    PendingCancellation,
    /// `IDUP` — the request duplicated one already in flight.
    Duplicate,
    /// `CWFW` — cancellation is waiting for the beneficiary's answer.
    CancellationWaitingForFurtherWork,
    /// `INFO` — an informational reply that resolves nothing.
    Information,
    /// Any other confirmation code.
    Other(String),
}

impl ResolutionOutcome {
    /// The ISO 20022 code string.
    #[must_use]
    pub fn as_code(&self) -> &str {
        match self {
            Self::Cancelled => "CNCL",
            Self::PartiallyCancelled => "PECR",
            Self::RejectedCancellation => "RJCR",
            Self::PendingCancellation => "PDCR",
            Self::Duplicate => "IDUP",
            Self::CancellationWaitingForFurtherWork => "CWFW",
            Self::Information => "INFO",
            Self::Other(s) => s,
        }
    }

    fn from_code(s: &str) -> Self {
        match s.trim().to_ascii_uppercase().as_str() {
            "CNCL" => Self::Cancelled,
            "PECR" => Self::PartiallyCancelled,
            "RJCR" => Self::RejectedCancellation,
            "PDCR" => Self::PendingCancellation,
            "IDUP" => Self::Duplicate,
            "CWFW" => Self::CancellationWaitingForFurtherWork,
            "INFO" => Self::Information,
            other => Self::Other(other.to_owned()),
        }
    }

    /// Whether this outcome says the payment was stopped.
    #[must_use]
    pub const fn is_accepted(&self) -> bool {
        matches!(self, Self::Cancelled)
    }

    /// Whether this outcome says the payment stands.
    #[must_use]
    pub const fn is_rejected(&self) -> bool {
        matches!(self, Self::RejectedCancellation)
    }

    /// Whether the case is resolved either way.
    #[must_use]
    pub const fn is_final(&self) -> bool {
        matches!(
            self,
            Self::Cancelled | Self::PartiallyCancelled | Self::RejectedCancellation
        )
    }
}

impl std::fmt::Display for ResolutionOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_code())
    }
}

// ── RejectionReason ───────────────────────────────────────────────────────────

/// Why a cancellation was refused (`CxlStsRsnInf/Rsn`).
///
/// `PaymentCancellationRejection2Code`, plus whatever else a bank sends. The
/// distinction that matters operationally is between "we could not" (`ARDT`,
/// the payment already settled) and "we would not" (`LEGL`, `CUST`): the first
/// means a [`pain.007`](crate::pain007) reversal is the remaining route, the
/// second means nothing is.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum RejectionReason {
    /// `LEGL` — refused for legal reasons.
    Legl,
    /// `AGNT` — an agent in the chain refused.
    Agnt,
    /// `CUST` — the beneficiary refused to return the funds.
    Cust,
    /// `ARDT` — **already returned**: the transaction settled and has been
    /// returned, so there is nothing left to cancel.
    Ardt,
    /// `NOAS` — no answer from the beneficiary.
    Noas,
    /// `NOOR` — no original transaction received; the bank cannot find it.
    Noor,
    /// `AC04` — the account is closed.
    Ac04,
    /// `AM04` — insufficient funds to return.
    Am04,
    /// Any other reason code.
    Other(String),
}

impl RejectionReason {
    /// The ISO 20022 code string.
    #[must_use]
    pub fn as_code(&self) -> &str {
        match self {
            Self::Legl => "LEGL",
            Self::Agnt => "AGNT",
            Self::Cust => "CUST",
            Self::Ardt => "ARDT",
            Self::Noas => "NOAS",
            Self::Noor => "NOOR",
            Self::Ac04 => "AC04",
            Self::Am04 => "AM04",
            Self::Other(s) => s,
        }
    }

    fn from_code(s: &str) -> Self {
        match s.trim().to_ascii_uppercase().as_str() {
            "LEGL" => Self::Legl,
            "AGNT" => Self::Agnt,
            "CUST" => Self::Cust,
            "ARDT" => Self::Ardt,
            "NOAS" => Self::Noas,
            "NOOR" => Self::Noor,
            "AC04" => Self::Ac04,
            "AM04" => Self::Am04,
            other => Self::Other(other.to_owned()),
        }
    }

    /// Whether the payment had already settled, so a cancellation was never
    /// going to be possible.
    ///
    /// For a direct debit this is the point at which a
    /// [`pain.007`](crate::pain007) reversal takes over.
    #[must_use]
    pub const fn is_too_late(&self) -> bool {
        matches!(self, Self::Ardt)
    }
}

impl std::fmt::Display for RejectionReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_code())
    }
}

// ── StatusCount ───────────────────────────────────────────────────────────────

/// `NbOfTxsPerCxlSts` — how many transactions ended in a given status.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CancellationCount {
    /// The status these transactions carry.
    pub status: CancellationStatus,
    /// How many of them there are (`DtldNbOfTxs`).
    pub count: Option<u64>,
    /// Their total, in ct (`DtldCtrlSum`).
    pub control_sum_ct: Option<i64>,
}

// ── TransactionCancellationStatus ─────────────────────────────────────────────

/// The outcome for one named transaction (`TxInfAndSts`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TransactionCancellationStatus {
    /// `CxlStsId` — the bank's identifier for this status.
    pub cancellation_status_id: Option<String>,
    /// `OrgnlEndToEndId` — the reference of the transaction that was recalled.
    ///
    /// This is the key you match the answer back to your own row with. `None`
    /// is a real answer and not an error, but it is one to escalate rather than
    /// default: the schema types it `0..1`.
    pub original_end_to_end_id: Option<String>,
    /// `OrgnlInstrId` — the alternative key, where the bank echoes that instead.
    pub original_instruction_id: Option<String>,
    /// `TxCxlSts` — accepted, rejected or pending.
    pub status: Option<CancellationStatus>,
    /// `CxlStsRsnInf/Rsn` — why, when the answer is no.
    pub reason_codes: Vec<RejectionReason>,
    /// `CxlStsRsnInf/AddtlInf`, in document order.
    ///
    /// `maxOccurs="unbounded"`, and banks use it: a legal refusal runs to
    /// several lines.
    pub additional_info: Vec<String>,
    /// `OrgnlInstdAmt` in ct, when the bank echoed it back.
    pub original_amount_ct: Option<i64>,
    /// ISO 4217 currency of `original_amount_ct`.
    pub original_currency: Option<String>,
    /// `OrgnlReqdExctnDt` exactly as the bank reported it.
    pub original_execution_date_raw: Option<String>,
    /// `OrgnlReqdColltnDt` exactly as the bank reported it.
    pub original_collection_date_raw: Option<String>,
}

impl TransactionCancellationStatus {
    /// Whether this transaction's cancellation was accepted.
    ///
    /// A transaction the bank listed without a status is **not** an acceptance.
    #[must_use]
    pub fn is_accepted(&self) -> bool {
        self.status
            .as_ref()
            .is_some_and(CancellationStatus::is_accepted)
    }

    /// Whether this transaction's cancellation was refused.
    #[must_use]
    pub fn is_rejected(&self) -> bool {
        self.status
            .as_ref()
            .is_some_and(CancellationStatus::is_rejected)
    }

    /// The original execution date, when the bank reported one this crate can
    /// read. The raw text is kept alongside — see
    /// [`original_execution_date_raw`](Self::original_execution_date_raw).
    #[must_use]
    pub fn original_execution_date(&self) -> Option<IsoDate> {
        IsoDate::parse_date_part(self.original_execution_date_raw.as_deref()?).ok()
    }

    /// The original collection date, typed. See
    /// [`original_execution_date`](Self::original_execution_date).
    #[must_use]
    pub fn original_collection_date(&self) -> Option<IsoDate> {
        IsoDate::parse_date_part(self.original_collection_date_raw.as_deref()?).ok()
    }
}

// ── PaymentInfoCancellationStatus ─────────────────────────────────────────────

/// The outcome for one submitted `PmtInf` (`OrgnlPmtInfAndSts`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PaymentInfoCancellationStatus {
    /// `OrgnlPmtInfCxlId` — the `PmtCxlId` from the request, echoed back.
    pub original_cancellation_id: Option<String>,
    /// `OrgnlPmtInfId` — the `PmtInfId` of the group that was recalled.
    pub original_payment_info_id: Option<String>,
    /// `PmtInfCxlSts` — the status of the whole group's cancellation.
    pub status: Option<CancellationStatus>,
    /// `CxlStsRsnInf/Rsn` at group level.
    ///
    /// When a bank refuses a whole group the reason is here and on no
    /// transaction, because no transaction was reached.
    pub reason_codes: Vec<RejectionReason>,
    /// `CxlStsRsnInf/AddtlInf` at group level.
    pub additional_info: Vec<String>,
    /// `NbOfTxsPerCxlSts`.
    pub status_counts: Vec<CancellationCount>,
    /// The per-transaction outcomes.
    pub transactions: Vec<TransactionCancellationStatus>,
}

impl PaymentInfoCancellationStatus {
    /// Whether anything in this group was refused.
    #[must_use]
    pub fn has_rejections(&self) -> bool {
        self.status
            .as_ref()
            .is_some_and(CancellationStatus::is_rejected)
            || self
                .transactions
                .iter()
                .any(TransactionCancellationStatus::is_rejected)
    }
}

// ── GroupCancellationStatus ───────────────────────────────────────────────────

/// The outcome for one recalled submission (`CxlDtls`).
///
/// Flattens `UnderlyingTransaction14`: the group header, the per-`PmtInf`
/// blocks, and the transaction blocks that ISO allows *directly* under
/// `CxlDtls` rather than inside a payment-information block.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GroupCancellationStatus {
    /// `OrgnlGrpCxlId` — the identifier of the group cancellation.
    pub original_cancellation_id: Option<String>,
    /// `OrgnlMsgId` — the `MsgId` of the file that was recalled.
    pub original_message_id: Option<String>,
    /// `OrgnlMsgNmId` — its message identifier, e.g. `pain.008.001.08`.
    pub original_message_name_id: Option<String>,
    /// `OrgnlCreDtTm` exactly as the bank reported it.
    pub original_created_at_raw: Option<String>,
    /// `OrgnlNbOfTxs`.
    pub original_number_of_transactions: Option<u64>,
    /// `OrgnlCtrlSum`, in ct.
    pub original_control_sum_ct: Option<i64>,
    /// `GrpCxlSts` — the status of cancelling the whole submission.
    pub status: Option<CancellationStatus>,
    /// `CxlStsRsnInf/Rsn` at message level.
    pub reason_codes: Vec<RejectionReason>,
    /// `CxlStsRsnInf/AddtlInf` at message level.
    pub additional_info: Vec<String>,
    /// `NbOfTxsPerCxlSts`.
    pub status_counts: Vec<CancellationCount>,
    /// The per-`PmtInf` outcomes.
    pub payment_infos: Vec<PaymentInfoCancellationStatus>,
    /// Transaction outcomes reported directly under `CxlDtls`.
    ///
    /// ISO allows `TxInfAndSts` at this level as well as inside
    /// `OrgnlPmtInfAndSts`, and banks use both. Reading only the nested ones
    /// loses every answer on a request that named transactions without naming
    /// their group.
    pub transactions: Vec<TransactionCancellationStatus>,
}

impl GroupCancellationStatus {
    /// Every transaction outcome in this group, at either level.
    pub fn all_transactions(&self) -> impl Iterator<Item = &TransactionCancellationStatus> {
        self.payment_infos
            .iter()
            .flat_map(|p| p.transactions.iter())
            .chain(self.transactions.iter())
    }
}

// ── Camt029Document ───────────────────────────────────────────────────────────

/// A parsed camt.029 Resolution of Investigation.
///
/// Produced by [`parse_camt029`].
#[derive(Debug, Clone)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Camt029Document {
    /// `Assgnmt/Id` — the bank's identifier for this answer.
    pub assignment_id: String,
    /// `Assgnmt/CreDtTm` exactly as the bank reported it.
    pub created_at: String,
    /// BIC of the institution that answered (`Assgnmt/Assgnr/Agt`).
    pub assigner_bic: Option<String>,
    /// Name of the party that answered, when it is not an institution.
    pub assigner_name: Option<String>,
    /// `RslvdCase/Id` — the `Assgnmt/Id` of the [`camt.055`](crate::camt055)
    /// request this answers.
    ///
    /// The key you match the answer to your own recall with.
    pub resolved_case_id: Option<String>,
    /// `Sts/Conf` — the outcome of the case as a whole.
    ///
    /// `None` when the bank used one of the other `Sts` branches, which SEPA
    /// does not: `RjctdMod`, `DplctOf` and `AssgnmtCxlConf` belong to
    /// modification and assignment cases.
    pub outcome: Option<ResolutionOutcome>,
    /// Detected XML namespace URI.
    pub namespace: Option<String>,
    /// One per `CxlDtls` block.
    pub groups: Vec<GroupCancellationStatus>,
}

impl Camt029Document {
    /// Every transaction outcome in the document, at either level of any group.
    pub fn transactions(&self) -> impl Iterator<Item = &TransactionCancellationStatus> {
        self.groups
            .iter()
            .flat_map(GroupCancellationStatus::all_transactions)
    }

    /// Whether the recall succeeded, as far as this document says.
    ///
    /// True when the message-level outcome is `CNCL`, or — when the bank sent
    /// no message-level confirmation — when every status it *did* send is
    /// `ACCR`. An empty document is **not** an acceptance: a bank that lists
    /// nothing has told you nothing.
    #[must_use]
    pub fn is_accepted(&self) -> bool {
        if let Some(outcome) = &self.outcome {
            return outcome.is_accepted();
        }
        let mut any = false;
        for status in self.statuses() {
            any = true;
            if !status.is_accepted() {
                return false;
            }
        }
        any
    }

    /// Whether anything in the document reports a refusal.
    #[must_use]
    pub fn has_rejections(&self) -> bool {
        self.outcome
            .as_ref()
            .is_some_and(ResolutionOutcome::is_rejected)
            || self.statuses().any(CancellationStatus::is_rejected)
    }

    /// Whether every status in the document is resolved.
    ///
    /// `false` while anything is `PDCR`, which is the state that must not be
    /// posted either way.
    #[must_use]
    pub fn is_final(&self) -> bool {
        self.outcome
            .as_ref()
            .is_none_or(ResolutionOutcome::is_final)
            && self.statuses().all(CancellationStatus::is_final)
    }

    /// Every reason code in the document, at all three levels.
    ///
    /// A refusal explains itself at exactly one level depending on how far the
    /// bank got, and a whole-submission refusal carries no transaction blocks
    /// to inspect at all — so gathering them is the only way to be sure a
    /// reason is not silently missed.
    #[must_use]
    pub fn rejection_reasons(&self) -> Vec<&RejectionReason> {
        let mut out = Vec::new();
        for g in &self.groups {
            out.extend(&g.reason_codes);
            for p in &g.payment_infos {
                out.extend(&p.reason_codes);
                for t in &p.transactions {
                    out.extend(&t.reason_codes);
                }
            }
            for t in &g.transactions {
                out.extend(&t.reason_codes);
            }
        }
        out
    }

    /// Every status the document carries, at group, payment-info and
    /// transaction level.
    fn statuses(&self) -> impl Iterator<Item = &CancellationStatus> {
        self.groups.iter().flat_map(|g| {
            g.status
                .iter()
                .chain(g.payment_infos.iter().flat_map(|p| p.status.iter()))
                .chain(g.all_transactions().filter_map(|t| t.status.as_ref()))
        })
    }
}

// ── Error ─────────────────────────────────────────────────────────────────────

/// Error returned when camt.029 XML cannot be parsed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum Camt029ParseError {
    /// The input is not well-formed XML.
    #[error(transparent)]
    Xml(#[from] XmlError),

    /// Root element `RsltnOfInvstgtn` not found — not a camt.029 document.
    #[error("not a camt.029 document: root element <RsltnOfInvstgtn> not found")]
    NotCamt029,

    /// A mandatory element was absent.
    ///
    /// Only for the elements that identify the answer: without them there is no
    /// way to match it to a request, and a document that cannot be matched is
    /// worse than one that fails to parse.
    #[error("camt.029 is missing mandatory element <{tag}>")]
    MissingElement {
        /// The element that was expected.
        tag: &'static str,
    },
}

// ── Parser ────────────────────────────────────────────────────────────────────

/// Parse a camt.029 Resolution of Investigation.
///
/// Accepts both the default-namespace and prefixed document shapes, like every
/// other parser here.
///
/// # Errors
///
/// [`Camt029ParseError::NotCamt029`] when the root element is missing,
/// [`Camt029ParseError::MissingElement`] for an absent `Assgnmt`/`Id`, or
/// [`Camt029ParseError::Xml`] when the input is not well-formed.
pub fn parse_camt029(xml: &str) -> Result<Camt029Document, Camt029ParseError> {
    let doc = Document::parse(xml)?;
    let namespace = doc.namespace;
    let root = doc
        .root
        .child("RsltnOfInvstgtn")
        .ok_or(Camt029ParseError::NotCamt029)?;

    let assignment = root
        .child("Assgnmt")
        .ok_or(Camt029ParseError::MissingElement { tag: "Assgnmt" })?;
    let assignment_id = assignment
        .text_of("Id")
        .ok_or(Camt029ParseError::MissingElement { tag: "Assgnmt/Id" })?
        .to_owned();

    let assigner = assignment.child("Assgnr");
    Ok(Camt029Document {
        assignment_id,
        created_at: assignment.text_of("CreDtTm").unwrap_or_default().to_owned(),
        assigner_bic: assigner
            .and_then(|a| a.child("Agt"))
            .and_then(crate::camt::agent_bic)
            .map(str::to_owned),
        assigner_name: assigner
            .and_then(|a| a.text_at(&["Pty", "Nm"]))
            .map(str::to_owned),
        // `RslvdCase/Id` is what the request's `Assgnmt/Id` comes back as.
        resolved_case_id: root.text_at(&["RslvdCase", "Id"]).map(str::to_owned),
        outcome: root
            .path(&["Sts", "Conf"])
            .map(|n| ResolutionOutcome::from_code(&n.text))
            // Some banks put the code straight in `Sts`, which the choice does
            // not allow but which is unambiguous to read.
            .or_else(|| {
                root.child("Sts")
                    .filter(|n| !n.text.is_empty())
                    .map(|n| ResolutionOutcome::from_code(&n.text))
            }),
        namespace,
        groups: root.children_named("CxlDtls").map(parse_group).collect(),
    })
}

/// `CxlStsRsnInf` — the reason codes and free text of one block.
fn parse_reasons(block: &Node) -> (Vec<RejectionReason>, Vec<String>) {
    let mut codes = Vec::new();
    let mut info = Vec::new();
    for r in block.children_named("CxlStsRsnInf") {
        if let Some(code) = r.child("Rsn").and_then(Node::code) {
            codes.push(RejectionReason::from_code(code));
        }
        info.extend(
            r.children_named("AddtlInf")
                .filter(|n| !n.text.is_empty())
                .map(|n| n.text.clone()),
        );
    }
    (codes, info)
}

/// `NbOfTxsPerCxlSts` — the per-status counts of one block.
fn parse_counts(block: &Node) -> Vec<CancellationCount> {
    block
        .children_named("NbOfTxsPerCxlSts")
        .map(|n| CancellationCount {
            status: n.text_of("DtldSts").map_or_else(
                || CancellationStatus::Other(String::new()),
                CancellationStatus::from_code,
            ),
            count: n.text_of("DtldNbOfTxs").and_then(|v| v.parse().ok()),
            control_sum_ct: n
                .text_of("DtldCtrlSum")
                .and_then(|v| crate::ct_from_eur_str(v).ok()),
        })
        .collect()
}

fn parse_group(block: &Node) -> GroupCancellationStatus {
    let header = block.child("OrgnlGrpInfAndSts");
    let (reason_codes, additional_info) = header.map_or_else(Default::default, parse_reasons);
    let control_sum = header.and_then(|h| h.text_of("OrgnlCtrlSum"));
    GroupCancellationStatus {
        original_cancellation_id: header
            .and_then(|h| h.text_of("OrgnlGrpCxlId"))
            .map(str::to_owned),
        original_message_id: header
            .and_then(|h| h.text_of("OrgnlMsgId"))
            .map(str::to_owned),
        original_message_name_id: header
            .and_then(|h| h.text_of("OrgnlMsgNmId"))
            .map(str::to_owned),
        original_created_at_raw: header
            .and_then(|h| h.text_of("OrgnlCreDtTm"))
            .map(str::to_owned),
        original_number_of_transactions: header
            .and_then(|h| h.text_of("OrgnlNbOfTxs"))
            .and_then(|v| v.parse().ok()),
        original_control_sum_ct: control_sum.and_then(|v| crate::ct_from_eur_str(v).ok()),
        status: header
            .and_then(|h| h.text_of("GrpCxlSts"))
            .map(CancellationStatus::from_code),
        reason_codes,
        additional_info,
        status_counts: header.map(parse_counts).unwrap_or_default(),
        payment_infos: block
            .children_named("OrgnlPmtInfAndSts")
            .map(parse_payment_info)
            .collect(),
        transactions: block
            .children_named("TxInfAndSts")
            .map(parse_transaction)
            .collect(),
    }
}

fn parse_payment_info(block: &Node) -> PaymentInfoCancellationStatus {
    let (reason_codes, additional_info) = parse_reasons(block);
    PaymentInfoCancellationStatus {
        original_cancellation_id: block.text_of("OrgnlPmtInfCxlId").map(str::to_owned),
        original_payment_info_id: block.text_of("OrgnlPmtInfId").map(str::to_owned),
        status: block
            .text_of("PmtInfCxlSts")
            .map(CancellationStatus::from_code),
        reason_codes,
        additional_info,
        status_counts: parse_counts(block),
        transactions: block
            .children_named("TxInfAndSts")
            .map(parse_transaction)
            .collect(),
    }
}

fn parse_transaction(tx: &Node) -> TransactionCancellationStatus {
    let (reason_codes, additional_info) = parse_reasons(tx);
    // `OrgnlInstdAmt` sits on the transaction; `OrgnlTxRef/Amt/InstdAmt` is
    // where some banks put the same figure instead.
    let amount = amount_of(tx, "OrgnlInstdAmt").or_else(|| {
        tx.child("OrgnlTxRef").and_then(|r| {
            r.child("Amt")
                .map_or_else(|| amount_of(r, "InstdAmt"), |a| amount_of(a, "InstdAmt"))
        })
    });
    let orig_ref = tx.child("OrgnlTxRef");
    let date_of = |tag: &str| {
        tx.text_of(tag)
            .or_else(|| orig_ref.and_then(|r| r.text_of(tag.trim_start_matches("Orgnl"))))
            .map(str::to_owned)
    };
    TransactionCancellationStatus {
        cancellation_status_id: tx.text_of("CxlStsId").map(str::to_owned),
        original_end_to_end_id: tx.text_of("OrgnlEndToEndId").map(str::to_owned),
        original_instruction_id: tx.text_of("OrgnlInstrId").map(str::to_owned),
        status: tx.text_of("TxCxlSts").map(CancellationStatus::from_code),
        reason_codes,
        additional_info,
        original_amount_ct: amount.as_ref().map(|(ct, _)| *ct),
        original_currency: amount.map(|(_, ccy)| ccy),
        original_execution_date_raw: date_of("OrgnlReqdExctnDt"),
        original_collection_date_raw: date_of("OrgnlReqdColltnDt"),
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape a bank sends when it accepted a per-transaction recall.
    const ACCEPTED: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.029.001.06">
  <RsltnOfInvstgtn>
    <Assgnmt><Id>RES-1</Id>
      <Assgnr><Agt><FinInstnId><BICFI>COBADEFFXXX</BICFI></FinInstnId></Agt></Assgnr>
      <Assgne><Pty><Nm>Stadtwerke GmbH</Nm></Pty></Assgne>
      <CreDtTm>2026-07-15T14:02:00</CreDtTm></Assgnmt>
    <RslvdCase><Id>CXL-1</Id><Cretr><Pty><Nm>Stadtwerke GmbH</Nm></Pty></Cretr></RslvdCase>
    <Sts><Conf>CNCL</Conf></Sts>
    <CxlDtls>
      <OrgnlGrpInfAndSts>
        <OrgnlMsgId>DD-2026-07-001</OrgnlMsgId>
        <OrgnlMsgNmId>pain.008.001.08</OrgnlMsgNmId>
        <OrgnlCreDtTm>2026-07-15T09:00:00</OrgnlCreDtTm>
        <OrgnlNbOfTxs>3</OrgnlNbOfTxs>
        <OrgnlCtrlSum>225.00</OrgnlCtrlSum>
      </OrgnlGrpInfAndSts>
      <OrgnlPmtInfAndSts>
        <OrgnlPmtInfCxlId>PC-1</OrgnlPmtInfCxlId>
        <OrgnlPmtInfId>PMT-A</OrgnlPmtInfId>
        <NbOfTxsPerCxlSts><DtldNbOfTxs>1</DtldNbOfTxs><DtldSts>ACCR</DtldSts>
          <DtldCtrlSum>75.00</DtldCtrlSum></NbOfTxsPerCxlSts>
        <TxInfAndSts>
          <CxlStsId>CS-1</CxlStsId>
          <OrgnlEndToEndId>E2E-1</OrgnlEndToEndId>
          <TxCxlSts>ACCR</TxCxlSts>
          <OrgnlInstdAmt Ccy="EUR">75.00</OrgnlInstdAmt>
          <OrgnlReqdColltnDt>2026-07-20</OrgnlReqdColltnDt>
        </TxInfAndSts>
      </OrgnlPmtInfAndSts>
    </CxlDtls>
  </RsltnOfInvstgtn>
</Document>"#;

    /// A refusal at *message* level, with no transaction blocks at all.
    const REFUSED_WHOLESALE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.029.001.06">
  <RsltnOfInvstgtn>
    <Assgnmt><Id>RES-2</Id>
      <Assgnr><Agt><FinInstnId><BICFI>COBADEFFXXX</BICFI></FinInstnId></Agt></Assgnr>
      <Assgne><Pty><Nm>Stadtwerke GmbH</Nm></Pty></Assgne>
      <CreDtTm>2026-07-15T14:02:00</CreDtTm></Assgnmt>
    <RslvdCase><Id>CXL-2</Id><Cretr><Pty><Nm>Stadtwerke GmbH</Nm></Pty></Cretr></RslvdCase>
    <Sts><Conf>RJCR</Conf></Sts>
    <CxlDtls>
      <OrgnlGrpInfAndSts>
        <OrgnlMsgId>DD-2026-07-001</OrgnlMsgId>
        <OrgnlMsgNmId>pain.008.001.08</OrgnlMsgNmId>
        <GrpCxlSts>RJCR</GrpCxlSts>
        <CxlStsRsnInf><Rsn><Cd>ARDT</Cd></Rsn>
          <AddtlInf>Bereits ausgefuehrt und zurueckgegeben</AddtlInf>
          <AddtlInf>Bitte pain.007 verwenden</AddtlInf></CxlStsRsnInf>
      </OrgnlGrpInfAndSts>
    </CxlDtls>
  </RsltnOfInvstgtn>
</Document>"#;

    #[test]
    fn an_accepted_recall_reads_back_the_keys_it_was_sent_with() {
        let doc = parse_camt029(ACCEPTED).unwrap();
        assert_eq!(doc.assignment_id, "RES-1");
        assert_eq!(doc.assigner_bic.as_deref(), Some("COBADEFFXXX"));
        assert_eq!(doc.resolved_case_id.as_deref(), Some("CXL-1"));
        assert_eq!(doc.outcome, Some(ResolutionOutcome::Cancelled));
        assert!(doc.is_accepted());
        assert!(doc.is_final());
        assert!(!doc.has_rejections());
        assert!(doc.rejection_reasons().is_empty());

        let g = &doc.groups[0];
        assert_eq!(g.original_message_id.as_deref(), Some("DD-2026-07-001"));
        assert_eq!(g.original_number_of_transactions, Some(3));
        assert_eq!(g.original_control_sum_ct, Some(22_500));

        let p = &g.payment_infos[0];
        assert_eq!(p.original_payment_info_id.as_deref(), Some("PMT-A"));
        assert_eq!(p.original_cancellation_id.as_deref(), Some("PC-1"));
        assert_eq!(p.status_counts[0].status, CancellationStatus::Accepted);
        assert_eq!(p.status_counts[0].count, Some(1));
        assert_eq!(p.status_counts[0].control_sum_ct, Some(7_500));

        let t = &p.transactions[0];
        assert_eq!(t.original_end_to_end_id.as_deref(), Some("E2E-1"));
        assert_eq!(t.status, Some(CancellationStatus::Accepted));
        assert_eq!(t.original_amount_ct, Some(7_500));
        assert_eq!(t.original_currency.as_deref(), Some("EUR"));
        assert_eq!(
            t.original_collection_date(),
            Some(IsoDate::new(2026, 7, 20).unwrap())
        );
        assert!(t.is_accepted());
    }

    #[test]
    fn a_whole_file_refusal_still_explains_itself() {
        // The camt.029 counterpart of the pain.002 defect: a refusal at group
        // level carries no `TxInfAndSts` at all, so a reader that walks only
        // transactions sees an empty document and calls it a success.
        let doc = parse_camt029(REFUSED_WHOLESALE).unwrap();
        assert!(!doc.is_accepted());
        assert!(doc.has_rejections());
        assert!(doc.is_final());
        assert_eq!(doc.transactions().count(), 0);

        let reasons = doc.rejection_reasons();
        assert_eq!(reasons, [&RejectionReason::Ardt]);
        assert!(reasons[0].is_too_late(), "ARDT means pain.007 is the route");
        assert_eq!(
            doc.groups[0].additional_info,
            [
                "Bereits ausgefuehrt und zurueckgegeben",
                "Bitte pain.007 verwenden"
            ]
        );
    }

    #[test]
    fn pending_is_neither_outcome() {
        // `PDCR` is the state that must not be posted either way: booking it as
        // accepted writes off money that is not coming back, and booking it as
        // rejected collects twice.
        let xml = ACCEPTED
            .replace("<Conf>CNCL</Conf>", "<Conf>PDCR</Conf>")
            .replace("<TxCxlSts>ACCR</TxCxlSts>", "<TxCxlSts>PDCR</TxCxlSts>");
        let doc = parse_camt029(&xml).unwrap();
        assert_eq!(doc.outcome, Some(ResolutionOutcome::PendingCancellation));
        assert!(!doc.is_accepted());
        assert!(!doc.has_rejections());
        assert!(!doc.is_final(), "nothing has been decided yet");
        assert_eq!(
            doc.transactions().next().unwrap().status,
            Some(CancellationStatus::Pending)
        );
    }

    #[test]
    fn an_empty_document_is_not_an_acceptance() {
        // A bank that lists nothing has told you nothing, and defaulting that
        // to success is how a recall gets assumed to have worked.
        let xml = r#"<Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.029.001.06">
          <RsltnOfInvstgtn><Assgnmt><Id>R</Id><CreDtTm>2026-07-15T14:00:00</CreDtTm></Assgnmt>
          </RsltnOfInvstgtn></Document>"#;
        let doc = parse_camt029(xml).unwrap();
        assert_eq!(doc.outcome, None);
        assert!(!doc.is_accepted());
        assert!(doc.groups.is_empty());
    }

    #[test]
    fn transactions_reported_outside_a_payment_info_block_are_not_lost() {
        // ISO allows `TxInfAndSts` directly under `CxlDtls` as well as inside
        // `OrgnlPmtInfAndSts`, and banks use both. Reading only the nested ones
        // loses every answer to a request that named no group.
        let xml = r#"<Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.029.001.06">
          <RsltnOfInvstgtn>
            <Assgnmt><Id>R</Id><CreDtTm>2026-07-15T14:00:00</CreDtTm></Assgnmt>
            <Sts><Conf>PECR</Conf></Sts>
            <CxlDtls>
              <TxInfAndSts><OrgnlEndToEndId>E2E-1</OrgnlEndToEndId>
                <TxCxlSts>ACCR</TxCxlSts></TxInfAndSts>
              <TxInfAndSts><OrgnlEndToEndId>E2E-2</OrgnlEndToEndId><TxCxlSts>RJCR</TxCxlSts>
                <CxlStsRsnInf><Rsn><Cd>CUST</Cd></Rsn></CxlStsRsnInf></TxInfAndSts>
            </CxlDtls>
          </RsltnOfInvstgtn></Document>"#;
        let doc = parse_camt029(xml).unwrap();
        let refs: Vec<_> = doc
            .transactions()
            .filter_map(|t| t.original_end_to_end_id.as_deref())
            .collect();
        assert_eq!(refs, ["E2E-1", "E2E-2"]);
        assert_eq!(doc.outcome, Some(ResolutionOutcome::PartiallyCancelled));
        assert!(doc.has_rejections());
        assert!(!doc.is_accepted());
        assert_eq!(doc.rejection_reasons(), [&RejectionReason::Cust]);
    }

    #[test]
    fn a_prefixed_namespace_reads_identically() {
        let prefixed = ACCEPTED
            .replace("<Document xmlns=", "<ns2:Document xmlns:ns2=")
            .replace("</Document>", "</ns2:Document>");
        // Only the root is prefixed here, which is malformed; prefix the lot.
        let mut out = String::new();
        for part in prefixed.split('<') {
            if part.is_empty() {
                continue;
            }
            out.push('<');
            if part.starts_with("ns2:") || part.starts_with("/ns2:") || part.starts_with('?') {
                out.push_str(part);
            } else if let Some(rest) = part.strip_prefix('/') {
                out.push_str("/ns2:");
                out.push_str(rest);
            } else {
                out.push_str("ns2:");
                out.push_str(part);
            }
        }
        let doc = parse_camt029(&out).unwrap();
        assert_eq!(doc.resolved_case_id.as_deref(), Some("CXL-1"));
        assert!(doc.is_accepted());
    }

    #[test]
    fn a_document_that_is_not_camt029_is_refused() {
        let xml = r#"<Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.053.001.08">
          <BkToCstmrStmt/></Document>"#;
        assert!(matches!(
            parse_camt029(xml),
            Err(Camt029ParseError::NotCamt029)
        ));
        assert!(matches!(
            parse_camt029("<Document><RsltnOfInvstgtn/></Document>"),
            Err(Camt029ParseError::MissingElement { tag: "Assgnmt" })
        ));
    }

    #[test]
    fn status_and_outcome_codes_round_trip() {
        for (code, status) in [
            ("ACCR", CancellationStatus::Accepted),
            ("RJCR", CancellationStatus::Rejected),
            ("PDCR", CancellationStatus::Pending),
            ("PACR", CancellationStatus::PartiallyAccepted),
        ] {
            assert_eq!(CancellationStatus::from_code(code), status);
            assert_eq!(status.as_code(), code);
        }
        assert!(CancellationStatus::PartiallyAccepted.is_final());
        assert!(!CancellationStatus::PartiallyAccepted.is_accepted());
        assert!(!CancellationStatus::Other("XX".to_owned()).is_final());
        assert_eq!(
            ResolutionOutcome::from_code("cncl"),
            ResolutionOutcome::Cancelled
        );
        assert_eq!(RejectionReason::from_code("ardt"), RejectionReason::Ardt);
    }
}
