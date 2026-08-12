//! ISO 20022 pain.002 — Customer Payment Status Report parser.
//!
//! Parses the bank's response to an initiated payment batch (pain.001 or pain.008).
//! The parser is namespace- and version-agnostic, so every generation below is
//! read by the same code:
//!
//! | Schema | Used by |
//! |---|---|
//! | `pain.002.001.10` | The EPC 2025 Customer-to-PSP guidelines — the reply to a `pain.001.001.09` / `pain.008.001.08` |
//! | `pain.002.001.03` | ISO namespace of the pre-2023 generation |
//! | `pain.002.003.03` | Deutsche Kreditwirtschaft (DK) standard |
//! | `pain.002.002.03` | DFÜ-Abkommen reference, some banks |
//!
//! ## Which fields a bank may omit
//!
//! `OrgnlEndToEndId` and `TxSts` are both `0..1` in every version, so they are
//! [`Option`]s here rather than being filled with a stand-in. That matters:
//! `OrgnlEndToEndId` is what a rejection is matched back to a transaction with,
//! and a substituted `"NOTPROVIDED"` is indistinguishable from a bank sending
//! that string for real.
//!
//! ## Pain.002 message lifecycle
//!
//! ```text
//! Customer sends pain.001 or pain.008
//!     ↓
//! Bank validates, then sends pain.002 with:
//!   GrpSts = ACTC  →  format/schema OK
//!   GrpSts = PART  →  some transactions rejected
//!   GrpSts = RJCT  →  entire batch rejected
//!     ↓
//! For each rejected transaction: TxSts = RJCT + StsRsnInf/Rsn/Cd
//! ```
//!
//! ## Example
//!
//! ```rust
//! use sepa::pain002::{parse_pain002, PaymentStatus};
//!
//! let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
//! <Document xmlns="urn:iso:std:iso:20022:tech:xsd:pain.002.003.03">
//!   <CstmrPmtStsRpt>
//!     <GrpHdr>
//!       <MsgId>AAAADEBBXXX20260714000001</MsgId>
//!       <CreDtTm>2026-07-14T10:20:30</CreDtTm>
//!     </GrpHdr>
//!     <OrgnlGrpInfAndSts>
//!       <OrgnlMsgId>CT-2026-07-001</OrgnlMsgId>
//!       <OrgnlMsgNmId>pain.001</OrgnlMsgNmId>
//!       <GrpSts>ACTC</GrpSts>
//!     </OrgnlGrpInfAndSts>
//!   </CstmrPmtStsRpt>
//! </Document>"#;
//!
//! let doc = parse_pain002(xml).unwrap();
//! assert_eq!(doc.original_msg_id, "CT-2026-07-001");
//! assert_eq!(doc.group_status, Some(PaymentStatus::Actc));
//! assert!(doc.group_status.unwrap().is_accepted());
//! ```
//!
//! ## Verification of Payee
//!
//! Mandatory for euro credit transfers since 9 October 2025: the payer's PSP
//! checks the payee name against the account before execution and reports the
//! outcome here. A status report is therefore no longer only about acceptance
//! and rejection, and a verification status is deliberately **not** an
//! acceptance — `RCVC` says a name matched, which is a different question from
//! whether the payment was taken.
//!
//! ```rust
//! use sepa::{VerificationOutcome, parse_pain002};
//!
//! let xml = r#"<Document xmlns="urn:iso:std:iso:20022:tech:xsd:pain.002.001.10">
//!   <CstmrPmtStsRpt>
//!     <GrpHdr><MsgId>M</MsgId><CreDtTm>2025-11-10T09:31:30Z</CreDtTm></GrpHdr>
//!     <OrgnlGrpInfAndSts>
//!       <OrgnlMsgId>K563</OrgnlMsgId><OrgnlMsgNmId>pain.001</OrgnlMsgNmId>
//!       <GrpSts>RVCM</GrpSts>
//!       <NbOfTxsPerSts><DtldNbOfTxs>454</DtldNbOfTxs><DtldSts>RCVC</DtldSts></NbOfTxsPerSts>
//!     </OrgnlGrpInfAndSts>
//!     <OrgnlPmtInfAndSts>
//!       <OrgnlPmtInfId>B001</OrgnlPmtInfId>
//!       <TxInfAndSts>
//!         <OrgnlEndToEndId>T087</OrgnlEndToEndId>
//!         <TxSts>RVMC</TxSts>
//!         <StsRsnInf><AddtlInf>Peter Schmitz</AddtlInf></StsRsnInf>
//!         <OrgnlTxRef><Cdtr><Pty><Nm>P. Schmitz</Nm></Pty></Cdtr></OrgnlTxRef>
//!       </TxInfAndSts>
//!     </OrgnlPmtInfAndSts>
//!   </CstmrPmtStsRpt>
//! </Document>"#;
//!
//! let doc = parse_pain002(xml)?;
//!
//! // 454 payments matched — reported as a count, not 454 elements.
//! assert_eq!(doc.group_status_counts[0].count, 454);
//!
//! // The one that needs a decision, with the name the payee's bank holds.
//! let tx = &doc.payment_info_statuses[0].transactions[0];
//! assert_eq!(
//!     tx.status.as_ref().unwrap().verification(),
//!     Some(VerificationOutcome::CloseMatch),
//! );
//! assert_eq!(tx.original_creditor_name.as_deref(), Some("P. Schmitz"));
//! assert_eq!(tx.additional_info, ["Peter Schmitz"]);
//! # Ok::<(), sepa::Pain002ParseError>(())
//! ```

use crate::xml::{Document, Node, XmlError};

// ── known namespaces ──────────────────────────────────────────────────────────

/// Known pain.002 XML namespace URIs.
///
/// Informational: [`parse_pain002`] is namespace-agnostic and does not consult
/// these. They are here so an application can recognise or log which generation
/// a bank replied in.
pub mod ns {
    /// `pain.002.001.10` — the version the EPC 2025 Customer-to-PSP
    /// Implementation Guidelines specify, and the reply to a
    /// `pain.001.001.09` / `pain.008.001.08` submission.
    pub const PAIN002_001_10: &str = "urn:iso:std:iso:20022:tech:xsd:pain.002.001.10";
    /// `pain.002.001.03` — the ISO namespace of the pre-2023 generation.
    pub const PAIN002_001_03: &str = "urn:iso:std:iso:20022:tech:xsd:pain.002.001.03";
    /// `pain.002.003.03` — legacy Deutsche Kreditwirtschaft DK V2.7.
    pub const PAIN002_003_03: &str = "urn:iso:std:iso:20022:tech:xsd:pain.002.003.03";
    /// `pain.002.002.03` — DFÜ-Abkommen reference schema.
    pub const PAIN002_002_03: &str = "urn:iso:std:iso:20022:tech:xsd:pain.002.002.03";
}

// ── PaymentStatus ─────────────────────────────────────────────────────────────

/// ISO 20022 payment status code — used at group, payment-info, and transaction level.
///
/// The status codes appear in `GrpSts`, `PmtInfSts`, and `TxSts` elements.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PaymentStatus {
    /// Accepted Technical Validation — schema/format valid.
    Actc,
    /// Accepted Customer Profile — account/mandate check passed.
    Accp,
    /// Accepted Settlement in Process — in interbank clearing.
    Acsp,
    /// Accepted Settlement Completed — funds transferred.
    Acsc,
    /// Accepted With Change — accepted but with a minor modification.
    Acwc,
    /// Partially Accepted — some transactions in the batch were rejected.
    Part,
    /// Pending — awaiting processing decision.
    Pdng,
    /// Rejected — not processed (see [`ReasonCode`] for details).
    Rjct,

    // ── Verification of Payee ────────────────────────────────────────────────
    /// `RCVC` — verification completed, the payee name **matched**.
    Rcvc,
    /// `RVMC` — verification completed, the payee name was a **close match**.
    ///
    /// The name the payee's PSP holds is returned in
    /// [`TransactionStatus::additional_info`], so it can be shown to the payer.
    Rvmc,
    /// `RVNM` — verification completed, the payee name did **not** match.
    Rvnm,
    /// `RVNA` — verification **not applicable**: no answer from the payee's
    /// PSP, a timeout, or a PSP outside the scheme.
    Rvna,
    /// `RVCM` — group level: verification completed **with mismatches**.
    ///
    /// Summarises a file in which at least one payment did not match; the
    /// per-payment outcomes are the four codes above.
    Rvcm,

    /// Unknown or bank-specific status code.
    Other(String),
}

/// The outcome of a Verification of Payee check on one payment.
///
/// `VoP` has been mandatory for euro credit transfers since 9 October 2025 under
/// the Instant Payments Regulation. The payer's PSP checks the payee name
/// against the account before execution and reports the result back in the
/// pain.002 — so a `pain.002` is no longer only about acceptance and rejection.
///
/// Read it with [`PaymentStatus::verification`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum VerificationOutcome {
    /// The name matched (`RCVC`). Nothing to do.
    Match,
    /// A close match (`RVMC`) — the payee's actual name is in
    /// [`TransactionStatus::additional_info`]. Show it to the payer and let
    /// them decide.
    CloseMatch,
    /// No match (`RVNM`). Executing anyway shifts liability to the payer.
    NoMatch,
    /// Not applicable (`RVNA`) — no answer, a timeout, or a PSP outside the
    /// scheme. The reason code distinguishes them: `AB11` is a timeout, `AG03`
    /// a PSP that does not offer `VoP`.
    NotApplicable,
}

impl PaymentStatus {
    /// ISO 20022 wire code (`"ACTC"`, `"RJCT"`, …).
    #[must_use]
    pub fn as_code(&self) -> &str {
        match self {
            Self::Actc => "ACTC",
            Self::Accp => "ACCP",
            Self::Acsp => "ACSP",
            Self::Acsc => "ACSC",
            Self::Acwc => "ACWC",
            Self::Part => "PART",
            Self::Pdng => "PDNG",
            Self::Rjct => "RJCT",
            Self::Rcvc => "RCVC",
            Self::Rvmc => "RVMC",
            Self::Rvnm => "RVNM",
            Self::Rvna => "RVNA",
            Self::Rvcm => "RVCM",
            Self::Other(s) => s,
        }
    }

    /// Returns `true` for accepted statuses (ACTC, ACCP, ACSP, ACSC, ACWC).
    ///
    /// The Verification of Payee codes are **not** acceptances: `RCVC` says a
    /// name matched, which is a different question from whether the payment was
    /// taken. Read [`verification`](Self::verification) for those.
    #[inline]
    #[must_use]
    pub fn is_accepted(&self) -> bool {
        matches!(
            self,
            Self::Actc | Self::Accp | Self::Acsp | Self::Acsc | Self::Acwc
        )
    }

    /// The Verification of Payee outcome, when this status reports one.
    ///
    /// `None` for every ordinary acceptance or rejection status, and for the
    /// group-level `RVCM` summary, which is about a whole file rather than one
    /// payee.
    #[inline]
    #[must_use]
    pub const fn verification(&self) -> Option<VerificationOutcome> {
        match self {
            Self::Rcvc => Some(VerificationOutcome::Match),
            Self::Rvmc => Some(VerificationOutcome::CloseMatch),
            Self::Rvnm => Some(VerificationOutcome::NoMatch),
            Self::Rvna => Some(VerificationOutcome::NotApplicable),
            _ => None,
        }
    }

    /// Whether this status is about payee verification rather than payment
    /// processing — including the group-level `RVCM` summary.
    #[inline]
    #[must_use]
    pub const fn is_verification(&self) -> bool {
        matches!(self, Self::Rvcm) || self.verification().is_some()
    }

    /// Returns `true` for terminal statuses (ACSC = fully settled, RJCT = fully rejected).
    #[inline]
    #[must_use]
    pub fn is_final(&self) -> bool {
        matches!(self, Self::Acsc | Self::Rjct)
    }

    /// Returns `true` if the payment was rejected (RJCT).
    #[inline]
    #[must_use]
    pub fn is_rejected(&self) -> bool {
        matches!(self, Self::Rjct)
    }

    fn from_code(s: &str) -> Self {
        match s.to_ascii_uppercase().as_str() {
            "ACTC" => Self::Actc,
            "ACCP" => Self::Accp,
            "ACSP" => Self::Acsp,
            "ACSC" => Self::Acsc,
            "ACWC" => Self::Acwc,
            "PART" => Self::Part,
            "PDNG" => Self::Pdng,
            "RJCT" => Self::Rjct,
            "RCVC" => Self::Rcvc,
            "RVMC" => Self::Rvmc,
            "RVNM" => Self::Rvnm,
            "RVNA" => Self::Rvna,
            "RVCM" => Self::Rvcm,
            other => Self::Other(other.to_owned()),
        }
    }
}

impl std::fmt::Display for PaymentStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_code())
    }
}

impl std::str::FromStr for PaymentStatus {
    type Err = std::convert::Infallible;
    /// Always succeeds — unknown codes become `Other(code)`.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::from_code(s))
    }
}

// ── ReasonCode ────────────────────────────────────────────────────────────────

/// ISO 20022 status reason code — explains *why* a payment was rejected.
///
/// Appears in `StsRsnInf/Rsn/Cd` within a rejected transaction.
///
/// References: ISO 20022 `ExternalStatusReason1Code`, EPC SCT/SDD Rulebooks.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ReasonCode {
    // ── Account ───────────────────────────────────────────────────────────────
    /// `AC01` — Incorrect account number (format error or wrong IBAN).
    Ac01,
    /// `AC04` — Closed account number.
    Ac04,
    /// `AC06` — Blocked account.
    Ac06,
    /// `AC13` — Invalid debtor account type (e.g., account type not permitted).
    Ac13,
    // ── Amount ────────────────────────────────────────────────────────────────
    /// `AM04` — Insufficient funds.
    Am04,
    /// `AM05` — Duplicate payment.
    Am05,
    // ── Mandate (SEPA Direct Debit) ───────────────────────────────────────────
    /// `MD01` — No valid mandate.
    Md01,
    /// `MD02` — Missing mandatory information in mandate.
    Md02,
    /// `MD06` — Return of funds requested by end customer (revocation).
    Md06,
    /// `MD07` — End customer deceased.
    Md07,
    // ── Agent ─────────────────────────────────────────────────────────────────
    /// `AG01` — Transaction forbidden (account type does not allow this transaction).
    Ag01,
    /// `AG02` — Invalid bank operation code.
    Ag02,
    /// `RC01` — Bank identifier (BIC/sort code) incorrect.
    Rc01,
    // ── Creditor / Debtor identification ──────────────────────────────────────
    /// `RR01` — Missing debtor account or identification.
    Rr01,
    /// `RR02` — Missing debtor name or address.
    Rr02,
    /// `RR03` — Missing creditor name or address.
    Rr03,
    /// `RR04` — Regulatory reason (sanction screening, AML).
    Rr04,
    // ── Miscellaneous ─────────────────────────────────────────────────────────
    /// `MS02` — Not specified reason — generated by customer/initiating party.
    Ms02,
    /// `MS03` — Not specified reason — generated by agent/bank.
    Ms03,
    // ── Format ────────────────────────────────────────────────────────────────
    /// `FF01` — Invalid file format (schema validation failure).
    Ff01,
    // ── Bank-specific ─────────────────────────────────────────────────────────
    /// `DS02` — Order to stop payment.
    Ds02,
    /// `NARR` — Narrative reason (see additional info).
    Narr,
    /// Any code not listed above.
    Other(String),
}

impl ReasonCode {
    /// ISO 20022 code string (`"AC01"`, `"MD01"`, …).
    #[must_use]
    pub fn as_code(&self) -> &str {
        match self {
            Self::Ac01 => "AC01",
            Self::Ac04 => "AC04",
            Self::Ac06 => "AC06",
            Self::Ac13 => "AC13",
            Self::Am04 => "AM04",
            Self::Am05 => "AM05",
            Self::Md01 => "MD01",
            Self::Md02 => "MD02",
            Self::Md06 => "MD06",
            Self::Md07 => "MD07",
            Self::Ag01 => "AG01",
            Self::Ag02 => "AG02",
            Self::Rc01 => "RC01",
            Self::Rr01 => "RR01",
            Self::Rr02 => "RR02",
            Self::Rr03 => "RR03",
            Self::Rr04 => "RR04",
            Self::Ms02 => "MS02",
            Self::Ms03 => "MS03",
            Self::Ff01 => "FF01",
            Self::Ds02 => "DS02",
            Self::Narr => "NARR",
            Self::Other(s) => s,
        }
    }

    fn from_code(s: &str) -> Self {
        match s.to_ascii_uppercase().as_str() {
            "AC01" => Self::Ac01,
            "AC04" => Self::Ac04,
            "AC06" => Self::Ac06,
            "AC13" => Self::Ac13,
            "AM04" => Self::Am04,
            "AM05" => Self::Am05,
            "MD01" => Self::Md01,
            "MD02" => Self::Md02,
            "MD06" => Self::Md06,
            "MD07" => Self::Md07,
            "AG01" => Self::Ag01,
            "AG02" => Self::Ag02,
            "RC01" => Self::Rc01,
            "RR01" => Self::Rr01,
            "RR02" => Self::Rr02,
            "RR03" => Self::Rr03,
            "RR04" => Self::Rr04,
            "MS02" => Self::Ms02,
            "MS03" => Self::Ms03,
            "FF01" => Self::Ff01,
            "DS02" => Self::Ds02,
            "NARR" => Self::Narr,
            other => Self::Other(other.to_owned()),
        }
    }
}

impl std::fmt::Display for ReasonCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_code())
    }
}

impl std::str::FromStr for ReasonCode {
    type Err = std::convert::Infallible;
    /// Always succeeds — unknown codes become `Other(code)`.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::from_code(s))
    }
}

// ── OriginalMessageType ───────────────────────────────────────────────────────

/// Which message type triggered this status report.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum OriginalMessageType {
    /// Status report for a pain.001 Credit Transfer initiation (SCT).
    CreditTransfer,
    /// Status report for a pain.008 Direct Debit initiation (SDD).
    DirectDebit,
    /// Other / unrecognised original message type.
    Other(String),
}

impl OriginalMessageType {
    fn from_msg_name_id(s: &str) -> Self {
        let s = s.trim();
        if s.starts_with("pain.001") {
            Self::CreditTransfer
        } else if s.starts_with("pain.008") {
            Self::DirectDebit
        } else {
            Self::Other(s.to_owned())
        }
    }
}

impl std::fmt::Display for OriginalMessageType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CreditTransfer => f.write_str("pain.001"),
            Self::DirectDebit => f.write_str("pain.008"),
            Self::Other(s) => f.write_str(s),
        }
    }
}

impl std::str::FromStr for OriginalMessageType {
    type Err = std::convert::Infallible;
    /// Always succeeds — unknown values become `Other(s)`.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::from_msg_name_id(s))
    }
}

// ── StatusCount ───────────────────────────────────────────────────────────────

/// One `NbOfTxsPerSts` row — how many transactions carry a given status.
///
/// A Verification of Payee report leans on this: rather than listing 462
/// matched payments, the bank reports the counts per outcome and itemises only
/// the ones that need the payer's attention.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct StatusCount {
    /// `DtldSts` — the status these transactions share.
    pub status: PaymentStatus,
    /// `DtldNbOfTxs` — how many there are.
    pub count: u64,
    /// `DtldCtrlSum` — their total in **ct**, when the bank reported one.
    pub total_ct: Option<i64>,
}

fn parse_status_counts(block: &Node) -> Vec<StatusCount> {
    block
        .children_named("NbOfTxsPerSts")
        .filter_map(|n| {
            Some(StatusCount {
                status: PaymentStatus::from_code(n.text_of("DtldSts")?),
                count: n.text_of("DtldNbOfTxs")?.parse().ok()?,
                total_ct: n
                    .text_of("DtldCtrlSum")
                    .and_then(|v| crate::ct_from_eur_str(v).ok()),
            })
        })
        .collect()
}

/// A party name, accepting both the flat and the `Party40Choice` nestings.
///
/// `pain.002.001.03` types `Dbtr` and `Cdtr` as a plain party, so the name is
/// `Cdtr/Nm`. From `.001.10` they are a `Party40Choice`, which wraps it as
/// `Cdtr/Pty/Nm`. Reading only the flat form silently loses every party name in
/// a current-version report.
fn party_name(reference: &Node, tag: &str) -> Option<String> {
    let party = reference.child(tag)?;
    party
        .text_of("Nm")
        .or_else(|| party.text_at(&["Pty", "Nm"]))
        .map(str::to_owned)
}

// ── TransactionStatus ─────────────────────────────────────────────────────────

/// Status of a single transaction within a pain.002 report.
#[derive(Debug, Clone)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TransactionStatus {
    /// Original end-to-end ID from the initiating message (`OrgnlEndToEndId`).
    ///
    /// Optional in every pain.002 version, so `None` is a real answer and not
    /// an error — but it is also the key you match a rejection back to your own
    /// transaction with, so a `None` here needs escalating rather than
    /// defaulting. See [`original_instruction_id`](Self::original_instruction_id)
    /// for the alternative key some banks echo instead.
    pub original_end_to_end_id: Option<String>,
    /// Original `InstrId` from the initiating message (`OrgnlInstrId`).
    pub original_instruction_id: Option<String>,
    /// Transaction-level status code (`TxSts`), when the bank reported one.
    pub status: Option<PaymentStatus>,
    /// Reason codes explaining a rejection (`StsRsnInf/Rsn/Cd`).
    pub reason_codes: Vec<ReasonCode>,
    /// Additional reason information (`StsRsnInf/AddtlInf`), in document order.
    ///
    /// `maxOccurs="unbounded"`, and banks use that: a legal notice runs to
    /// several lines, and on a Verification of Payee **close match** this is
    /// where the payee's actual name comes back — split across two entries when
    /// it exceeds 105 characters.
    pub additional_info: Vec<String>,
    /// Original instructed amount in **ct** (1/100 EUR), if present in `OrgnlTxRef`.
    pub original_amount_ct: Option<i64>,
    /// Original debtor name, if echoed back.
    pub original_debtor_name: Option<String>,
    /// Original debtor IBAN, if echoed back.
    pub original_debtor_iban: Option<String>,
    /// Original creditor name, if echoed back.
    pub original_creditor_name: Option<String>,
    /// Original creditor IBAN, if echoed back.
    pub original_creditor_iban: Option<String>,
}

// ── PaymentInfoStatus ─────────────────────────────────────────────────────────

/// Status of a payment information block (`OrgnlPmtInfAndSts`).
///
/// One `PaymentInfoStatus` corresponds to one `<PmtInf>` block in the original
/// pain.001 or pain.008 message.
#[derive(Debug, Clone)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PaymentInfoStatus {
    /// Original `PmtInfId` from the initiating message (`OrgnlPmtInfId`).
    pub original_payment_info_id: Option<String>,
    /// Payment-information-level status, if present.
    pub status: Option<PaymentStatus>,
    /// `NbOfTxsPerSts` — how many transactions carry each status.
    pub status_counts: Vec<StatusCount>,
    /// Per-transaction statuses within this payment info block.
    pub transactions: Vec<TransactionStatus>,
}

impl TransactionStatus {
    /// Whether this transaction was rejected (`TxSts` = `RJCT`).
    ///
    /// A transaction the bank listed without a status is **not** a rejection.
    #[must_use]
    pub fn is_rejected(&self) -> bool {
        self.status.as_ref().is_some_and(PaymentStatus::is_rejected)
    }
}

impl PaymentInfoStatus {
    /// Returns `true` when any transaction in this block was rejected.
    #[must_use]
    pub fn has_rejections(&self) -> bool {
        self.transactions.iter().any(TransactionStatus::is_rejected)
            || self.status.as_ref().is_some_and(PaymentStatus::is_rejected)
    }

    /// Collect all reason codes from rejected transactions.
    #[must_use]
    pub fn rejection_reasons(&self) -> Vec<&ReasonCode> {
        self.transactions
            .iter()
            .filter(|t| t.is_rejected())
            .flat_map(|t| &t.reason_codes)
            .collect()
    }
}

// ── Pain002Document ───────────────────────────────────────────────────────────

/// A parsed pain.002 Customer Payment Status Report.
///
/// Produced by [`parse_pain002`].
#[derive(Debug, Clone)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Pain002Document {
    /// pain.002 message ID (generated by the bank).
    pub msg_id: String,
    /// pain.002 creation timestamp (`CreDtTm`), ISO 8601.
    pub created_at: String,
    /// BIC of the agent that generated the report.
    /// For SCT: `DbtrAgt`; for SDD: `CdtrAgt`.
    pub forwarding_agent_bic: Option<String>,
    /// Detected XML namespace URI (e.g. `pain.002.003.03`).
    pub namespace: Option<String>,
    /// `OrgnlMsgId` — message ID of the original pain.001 or pain.008.
    ///
    /// Mandatory in every pain.002 version, so a report without it is rejected
    /// as [`Pain002ParseError::MissingElement`] rather than parsed with a
    /// stand-in: this is the identifier you match the report to your own
    /// submission with.
    pub original_msg_id: String,
    /// `OrgnlMsgNmId` — identifies whether this is a SCT or SDD response.
    ///
    /// Mandatory in the schema; `None` means the bank omitted it, which is
    /// reported rather than papered over with an empty code.
    pub original_msg_type: Option<OriginalMessageType>,
    /// Group-level status, if present in `OrgnlGrpInfAndSts/GrpSts`.
    pub group_status: Option<PaymentStatus>,
    /// `OrgnlGrpInfAndSts/NbOfTxsPerSts` — file-wide counts per status.
    pub group_status_counts: Vec<StatusCount>,
    /// Per-payment-info statuses (one per `<PmtInf>` in the original message).
    pub payment_info_statuses: Vec<PaymentInfoStatus>,
}

impl Pain002Document {
    /// `true` if the entire batch was accepted (any accepted group status + no rejections).
    ///
    /// Note: `ACTC` means "technically validated" (format OK) but the payment is
    /// still in-flight. `ACSC` means "settlement completed". Use [`PaymentStatus::is_final`]
    /// on [`group_status`](Self::group_status) if you need to wait for a terminal state.
    #[must_use]
    pub fn is_fully_accepted(&self) -> bool {
        let reported = self.group_status.is_some() || !self.payment_info_statuses.is_empty();
        reported
            && self
                .group_status
                .as_ref()
                .is_none_or(PaymentStatus::is_accepted)
            && self.payment_info_statuses.iter().all(|p| {
                p.status.as_ref().is_none_or(PaymentStatus::is_accepted)
                    && p.transactions
                        .iter()
                        .all(|t| t.status.as_ref().is_none_or(PaymentStatus::is_accepted))
            })
    }

    /// `true` if any transaction was rejected.
    #[must_use]
    pub fn has_rejections(&self) -> bool {
        self.group_status
            .as_ref()
            .is_some_and(PaymentStatus::is_rejected)
            || self
                .payment_info_statuses
                .iter()
                .any(PaymentInfoStatus::has_rejections)
    }

    /// Collect all rejected transactions across all payment info blocks.
    #[must_use]
    pub fn rejected_transactions(&self) -> Vec<&TransactionStatus> {
        self.payment_info_statuses
            .iter()
            .flat_map(|p| &p.transactions)
            .filter(|t| t.is_rejected())
            .collect()
    }
}

// ── Error ─────────────────────────────────────────────────────────────────────

/// Error returned when pain.002 XML cannot be parsed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum Pain002ParseError {
    /// The input is not well-formed XML.
    #[error(transparent)]
    Xml(#[from] XmlError),

    /// The root element `CstmrPmtStsRpt` was not found — not a pain.002 document.
    #[error("not a pain.002 document: root element <CstmrPmtStsRpt> not found")]
    NotPain002,
    /// A required XML element was absent.
    ///
    /// Only the elements a status report is useless without: `GrpHdr`,
    /// `GrpHdr/MsgId`, `OrgnlGrpInfAndSts` and `OrgnlGrpInfAndSts/OrgnlMsgId`.
    /// Everything else is optional and simply reads as `None`.
    #[error("missing required pain.002 element: <{tag}>")]
    MissingElement {
        /// Name of the missing XML element.
        tag: &'static str,
    },
}

// ── Parser ────────────────────────────────────────────────────────────────────

/// Parse a pain.002 Customer Payment Status Report XML string.
///
/// Accepts all known DK/EPC namespace variants (`pain.002.003.03`,
/// `pain.002.002.03`, `pain.002.001.03`) and also handles namespace-prefixed
/// documents (e.g. `<ns2:Document xmlns:ns2="…">`).
///
/// # Errors
///
/// Returns [`Pain002ParseError::NotPain002`] when the root element is missing,
/// or [`Pain002ParseError::MissingElement`] for absent mandatory elements.
pub fn parse_pain002(xml: &str) -> Result<Pain002Document, Pain002ParseError> {
    let doc = Document::parse(xml)?;
    let namespace = doc.namespace;

    let root = doc
        .root
        .child("CstmrPmtStsRpt")
        .ok_or(Pain002ParseError::NotPain002)?;

    let grp_hdr = root
        .child("GrpHdr")
        .ok_or(Pain002ParseError::MissingElement { tag: "GrpHdr" })?;

    let msg_id = grp_hdr
        .text_of("MsgId")
        .ok_or(Pain002ParseError::MissingElement { tag: "MsgId" })?
        .to_owned();

    let created_at = grp_hdr.text_of("CreDtTm").unwrap_or_default().to_owned();

    // Forwarding agent BIC: DbtrAgt (SCT) or CdtrAgt (SDD). ISO renamed the
    // element from `BIC` to `BICFI` in the 2019 maintenance release, so accept
    // both rather than silently dropping the BIC on newer messages.
    let forwarding_agent_bic = grp_hdr
        .child("DbtrAgt")
        .or_else(|| grp_hdr.child("CdtrAgt"))
        .and_then(bic_of_agent)
        .map(str::to_owned);

    let orig_grp = root
        .child("OrgnlGrpInfAndSts")
        .ok_or(Pain002ParseError::MissingElement {
            tag: "OrgnlGrpInfAndSts",
        })?;

    let original_msg_id = orig_grp
        .text_of("OrgnlMsgId")
        .ok_or(Pain002ParseError::MissingElement { tag: "OrgnlMsgId" })?
        .to_owned();

    let original_msg_type = orig_grp
        .text_of("OrgnlMsgNmId")
        .map(OriginalMessageType::from_msg_name_id);

    let group_status = orig_grp.text_of("GrpSts").map(PaymentStatus::from_code);
    let group_status_counts = parse_status_counts(orig_grp);

    let payment_info_statuses = root
        .children_named("OrgnlPmtInfAndSts")
        .map(parse_payment_info_status)
        .collect();

    Ok(Pain002Document {
        msg_id,
        created_at,
        forwarding_agent_bic,
        namespace,
        original_msg_id,
        original_msg_type,
        group_status,
        group_status_counts,
        payment_info_statuses,
    })
}

/// `FinInstnId/BIC` (pre-2019) or `FinInstnId/BICFI` (2019 onwards).
fn bic_of_agent(agent: &Node) -> Option<&str> {
    let fin = agent.child("FinInstnId")?;
    fin.text_of("BIC").or_else(|| fin.text_of("BICFI"))
}

fn parse_payment_info_status(block: &Node) -> PaymentInfoStatus {
    PaymentInfoStatus {
        // Mandatory in the schema, so `None` means the bank sent a malformed
        // block. Reported as absent rather than substituted: `"NOTPROVIDED"` is
        // a legal `PmtInfId`, and inventing it here could match a real group.
        original_payment_info_id: block.text_of("OrgnlPmtInfId").map(str::to_owned),
        status: block.text_of("PmtInfSts").map(PaymentStatus::from_code),
        status_counts: parse_status_counts(block),
        transactions: block
            .children_named("TxInfAndSts")
            .map(parse_transaction_status)
            .collect(),
    }
}

fn parse_transaction_status(tx: &Node) -> TransactionStatus {
    // Both are `0..1` in the schema. The previous stand-ins — `"NOTPROVIDED"`
    // for the reference and `Other("UNKNOWN")` for the status — were
    // indistinguishable from a bank genuinely sending those values, and the
    // reference is what a rejection is matched back to a transaction with.
    let original_end_to_end_id = tx.text_of("OrgnlEndToEndId").map(str::to_owned);
    let original_instruction_id = tx.text_of("OrgnlInstrId").map(str::to_owned);
    let status = tx.text_of("TxSts").map(PaymentStatus::from_code);

    // One reason code per StsRsnInf block. `Rsn` is a choice between a typed
    // `Cd` and a bank-proprietary `Prtry`; each block is inspected separately so
    // a proprietary code in a later block is not masked by a typed code in an
    // earlier one.
    let mut reason_codes = Vec::new();
    let mut additional_info: Vec<String> = Vec::new();
    for rsn_block in tx.children_named("StsRsnInf") {
        if let Some(code) = rsn_block.child("Rsn").and_then(Node::code) {
            reason_codes.push(ReasonCode::from_code(code));
        }
        // `AddtlInf` is unbounded and every occurrence matters — a legal notice
        // spans lines, and a VoP close match returns the payee's real name here.
        additional_info.extend(
            rsn_block
                .children_named("AddtlInf")
                .map(|n| n.text.clone())
                .filter(|t| !t.is_empty()),
        );
    }

    let orig_tx_ref = tx.child("OrgnlTxRef");

    // `Amt` is an AmountType4Choice: InstdAmt or EqvtAmt/Amt. Some banks also
    // place InstdAmt directly under OrgnlTxRef, so fall back to a deep search.
    let original_amount_ct = orig_tx_ref
        .and_then(|r| {
            r.text_at(&["Amt", "InstdAmt"])
                .or_else(|| r.text_of_descendant("InstdAmt"))
        })
        .and_then(|raw| crate::ct_from_eur_str(raw).ok());

    let party = |tags: [&str; 2]| -> (Option<String>, Option<String>) {
        orig_tx_ref.map_or((None, None), |r| {
            (
                party_name(r, tags[0]),
                r.text_at(&[tags[1], "Id", "IBAN"]).map(str::to_owned),
            )
        })
    };

    let (original_debtor_name, original_debtor_iban) = party(["Dbtr", "DbtrAcct"]);
    let (original_creditor_name, original_creditor_iban) = party(["Cdtr", "CdtrAcct"]);

    TransactionStatus {
        original_end_to_end_id,
        original_instruction_id,
        status,
        reason_codes,
        additional_info,
        original_amount_ct,
        original_debtor_name,
        original_debtor_iban,
        original_creditor_name,
        original_creditor_iban,
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const PAIN002_ACTC: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Document xmlns="urn:iso:std:iso:20022:tech:xsd:pain.002.003.03">
  <CstmrPmtStsRpt>
    <GrpHdr>
      <MsgId>AAAADEBBXXX20260714000001</MsgId>
      <CreDtTm>2026-07-14T10:20:30</CreDtTm>
      <DbtrAgt><FinInstnId><BIC>COBADEFFXXX</BIC></FinInstnId></DbtrAgt>
    </GrpHdr>
    <OrgnlGrpInfAndSts>
      <OrgnlMsgId>CT-2026-07-001</OrgnlMsgId>
      <OrgnlMsgNmId>pain.001</OrgnlMsgNmId>
      <GrpSts>ACTC</GrpSts>
    </OrgnlGrpInfAndSts>
  </CstmrPmtStsRpt>
</Document>"#;

    const PAIN002_PART_RJCT: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Document xmlns="urn:iso:std:iso:20022:tech:xsd:pain.002.001.03">
  <CstmrPmtStsRpt>
    <GrpHdr>
      <MsgId>AAAADEBBJJJJMMTT0000000001</MsgId>
      <CreDtTm>2026-07-14T10:20:30</CreDtTm>
      <DbtrAgt><FinInstnId><BIC>AAAADEBB</BIC></FinInstnId></DbtrAgt>
    </GrpHdr>
    <OrgnlGrpInfAndSts>
      <OrgnlMsgId>CT-BATCH-001</OrgnlMsgId>
      <OrgnlMsgNmId>pain.001</OrgnlMsgNmId>
      <GrpSts>PART</GrpSts>
    </OrgnlGrpInfAndSts>
    <OrgnlPmtInfAndSts>
      <OrgnlPmtInfId>PMT-001</OrgnlPmtInfId>
      <TxInfAndSts>
        <OrgnlEndToEndId>E2E-001</OrgnlEndToEndId>
        <TxSts>RJCT</TxSts>
        <StsRsnInf>
          <Orgtr><Id><OrgId><BICOrBEI>AAAADEBBXXX</BICOrBEI></OrgId></Id></Orgtr>
          <Rsn><Cd>AC04</Cd></Rsn>
        </StsRsnInf>
        <OrgnlTxRef>
          <Amt><InstdAmt Ccy="EUR">88.88</InstdAmt></Amt>
          <Dbtr><Nm>Max Mustermann</Nm></Dbtr>
          <DbtrAcct><Id><IBAN>DE99888888885555555555</IBAN></Id></DbtrAcct>
          <Cdtr><Nm>Creditor GmbH</Nm></Cdtr>
          <CdtrAcct><Id><IBAN>DE89370400440532013000</IBAN></Id></CdtrAcct>
        </OrgnlTxRef>
      </TxInfAndSts>
      <TxInfAndSts>
        <OrgnlEndToEndId>E2E-002</OrgnlEndToEndId>
        <TxSts>RJCT</TxSts>
        <StsRsnInf>
          <Rsn><Cd>DS02</Cd></Rsn>
          <AddtlInf>Customer order to stop</AddtlInf>
        </StsRsnInf>
      </TxInfAndSts>
    </OrgnlPmtInfAndSts>
  </CstmrPmtStsRpt>
</Document>"#;

    #[test]
    fn parse_actc_accepted() {
        let doc = parse_pain002(PAIN002_ACTC).unwrap();
        assert_eq!(doc.msg_id, "AAAADEBBXXX20260714000001");
        assert_eq!(doc.created_at, "2026-07-14T10:20:30");
        assert_eq!(doc.original_msg_id, "CT-2026-07-001");
        assert_eq!(doc.group_status, Some(PaymentStatus::Actc));
        assert!(doc.group_status.as_ref().unwrap().is_accepted());
        assert_eq!(
            doc.namespace.as_deref(),
            Some("urn:iso:std:iso:20022:tech:xsd:pain.002.003.03")
        );
        assert_eq!(
            doc.original_msg_type,
            Some(OriginalMessageType::CreditTransfer)
        );
        assert_eq!(doc.forwarding_agent_bic.as_deref(), Some("COBADEFFXXX"));
        assert!(!doc.has_rejections());
        assert!(doc.is_fully_accepted());
    }

    #[test]
    fn parse_part_with_rejections() {
        let doc = parse_pain002(PAIN002_PART_RJCT).unwrap();
        assert_eq!(doc.group_status, Some(PaymentStatus::Part));
        assert!(!doc.is_fully_accepted());
        assert!(doc.has_rejections());

        let rejected = doc.rejected_transactions();
        assert_eq!(rejected.len(), 2);

        let tx1 = &rejected[0];
        assert_eq!(tx1.original_end_to_end_id.as_deref(), Some("E2E-001"));
        assert_eq!(tx1.reason_codes, vec![ReasonCode::Ac04]);
        assert_eq!(tx1.original_amount_ct, Some(8888));
        assert_eq!(
            tx1.original_debtor_iban.as_deref(),
            Some("DE99888888885555555555")
        );
        assert_eq!(
            tx1.original_creditor_iban.as_deref(),
            Some("DE89370400440532013000")
        );

        let tx2 = &rejected[1];
        assert_eq!(tx2.reason_codes, vec![ReasonCode::Ds02]);
        assert_eq!(tx2.additional_info, ["Customer order to stop"]);
    }

    #[test]
    fn omitted_optional_fields_read_as_absent_not_as_a_stand_in() {
        // `OrgnlEndToEndId` and `TxSts` are both 0..1 in every pain.002
        // version. They used to be filled with "NOTPROVIDED" and
        // Other("UNKNOWN") — values a bank can also send for real, so a caller
        // could not tell an unattributable rejection from an attributed one.
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Document xmlns="urn:iso:std:iso:20022:tech:xsd:pain.002.001.10">
  <CstmrPmtStsRpt>
    <GrpHdr><MsgId>M</MsgId><CreDtTm>2026-07-14T10:00:00</CreDtTm></GrpHdr>
    <OrgnlGrpInfAndSts>
      <OrgnlMsgId>CT-1</OrgnlMsgId>
      <OrgnlMsgNmId>pain.001.001.09</OrgnlMsgNmId>
      <GrpSts>PART</GrpSts>
    </OrgnlGrpInfAndSts>
    <OrgnlPmtInfAndSts>
      <OrgnlPmtInfId>PMT-1</OrgnlPmtInfId>
      <TxInfAndSts>
        <OrgnlInstrId>INSTR-7</OrgnlInstrId>
        <TxSts>RJCT</TxSts>
        <StsRsnInf><Rsn><Cd>AC01</Cd></Rsn></StsRsnInf>
      </TxInfAndSts>
      <TxInfAndSts>
        <OrgnlEndToEndId>E2E-2</OrgnlEndToEndId>
      </TxInfAndSts>
    </OrgnlPmtInfAndSts>
  </CstmrPmtStsRpt>
</Document>"#;

        let doc = parse_pain002(xml).unwrap();
        let txs = &doc.payment_info_statuses[0].transactions;

        // No end-to-end reference: absent, not "NOTPROVIDED". The instruction
        // id is the only key this rejection can be matched by.
        assert_eq!(txs[0].original_end_to_end_id, None);
        assert_eq!(txs[0].original_instruction_id.as_deref(), Some("INSTR-7"));
        assert!(txs[0].is_rejected());

        // No status at all: absent, not Other("UNKNOWN"), and not a rejection.
        assert_eq!(txs[1].status, None);
        assert!(!txs[1].is_rejected());

        assert!(doc.has_rejections());
        assert!(!doc.is_fully_accepted());
    }

    #[test]
    fn full_acceptance_looks_at_all_three_status_levels() {
        let report = |grp: &str, pmt: &str| {
            format!(
                r#"<Document xmlns="urn:iso:std:iso:20022:tech:xsd:pain.002.001.10">
  <CstmrPmtStsRpt>
    <GrpHdr><MsgId>M</MsgId><CreDtTm>T</CreDtTm></GrpHdr>
    <OrgnlGrpInfAndSts><OrgnlMsgId>O</OrgnlMsgId><GrpSts>{grp}</GrpSts></OrgnlGrpInfAndSts>
    <OrgnlPmtInfAndSts><OrgnlPmtInfId>P</OrgnlPmtInfId><PmtInfSts>{pmt}</PmtInfSts></OrgnlPmtInfAndSts>
  </CstmrPmtStsRpt>
</Document>"#
            )
        };

        assert!(
            parse_pain002(&report("ACTC", "ACTC"))
                .unwrap()
                .is_fully_accepted()
        );
        assert!(
            !parse_pain002(&report("ACTC", "RJCT"))
                .unwrap()
                .is_fully_accepted()
        );
        assert!(
            !parse_pain002(&report("PART", "ACTC"))
                .unwrap()
                .is_fully_accepted()
        );

        // The case the old implementation got wrong: it asked only whether any
        // status was *rejected*, so a payment-information block that was
        // merely pending or partially accepted counted as fully accepted.
        for not_yet in ["PDNG", "PART", "ZZZZ"] {
            assert!(
                !parse_pain002(&report("ACTC", not_yet))
                    .unwrap()
                    .is_fully_accepted(),
                "PmtInfSts {not_yet} is not an acceptance"
            );
        }

        // A report that states no status at all is not an acceptance.
        let silent = r#"<Document xmlns="urn:iso:std:iso:20022:tech:xsd:pain.002.001.10">
  <CstmrPmtStsRpt>
    <GrpHdr><MsgId>M</MsgId><CreDtTm>T</CreDtTm></GrpHdr>
    <OrgnlGrpInfAndSts><OrgnlMsgId>O</OrgnlMsgId></OrgnlGrpInfAndSts>
  </CstmrPmtStsRpt>
</Document>"#;
        let doc = parse_pain002(silent).unwrap();
        assert_eq!(doc.group_status, None);
        assert!(!doc.is_fully_accepted());
    }

    /// Abridged from the Deutsche Kreditwirtschaft's published example
    /// `pain.002.001.10-VOP Status Report.xml` (Anlage 3, status April 2025).
    /// Verification of Payee has been mandatory since 9 October 2025, so this
    /// is the shape a payer now gets back for every credit transfer file.
    const VOP_REPORT: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Document xmlns="urn:iso:std:iso:20022:tech:xsd:pain.002.001.10">
  <CstmrPmtStsRpt>
    <GrpHdr>
      <MsgId>B78567267384</MsgId>
      <CreDtTm>2025-11-10T09:31:30Z</CreDtTm>
      <DbtrAgt><FinInstnId><BICFI>SPUEDE2UXXX</BICFI></FinInstnId></DbtrAgt>
    </GrpHdr>
    <OrgnlGrpInfAndSts>
      <OrgnlMsgId>K563</OrgnlMsgId>
      <OrgnlMsgNmId>pain.001</OrgnlMsgNmId>
      <OrgnlNbOfTxs>462</OrgnlNbOfTxs>
      <GrpSts>RVCM</GrpSts>
      <StsRsnInf>
        <AddtlInf>RVMC Message text e.g. with legal notice</AddtlInf>
        <AddtlInf>RVMC continuation of the message text</AddtlInf>
      </StsRsnInf>
      <NbOfTxsPerSts><DtldNbOfTxs>454</DtldNbOfTxs><DtldSts>RCVC</DtldSts></NbOfTxsPerSts>
      <NbOfTxsPerSts><DtldNbOfTxs>3</DtldNbOfTxs><DtldSts>RVNM</DtldSts></NbOfTxsPerSts>
      <NbOfTxsPerSts><DtldNbOfTxs>2</DtldNbOfTxs><DtldSts>RVMC</DtldSts></NbOfTxsPerSts>
      <NbOfTxsPerSts><DtldNbOfTxs>3</DtldNbOfTxs><DtldSts>RVNA</DtldSts></NbOfTxsPerSts>
    </OrgnlGrpInfAndSts>
    <OrgnlPmtInfAndSts>
      <OrgnlPmtInfId>B001</OrgnlPmtInfId>
      <OrgnlNbOfTxs>350</OrgnlNbOfTxs>
      <NbOfTxsPerSts><DtldNbOfTxs>344</DtldNbOfTxs><DtldSts>RCVC</DtldSts></NbOfTxsPerSts>
      <NbOfTxsPerSts><DtldNbOfTxs>1</DtldNbOfTxs><DtldSts>RVNM</DtldSts></NbOfTxsPerSts>
      <TxInfAndSts>
        <OrgnlEndToEndId>K563-B001-T021</OrgnlEndToEndId>
        <TxSts>RVNM</TxSts>
        <OrgnlTxRef>
          <Cdtr><Pty><Nm>Creditor Name</Nm></Pty></Cdtr>
          <CdtrAcct><Id><IBAN>DE21500500009876543210</IBAN></Id></CdtrAcct>
        </OrgnlTxRef>
      </TxInfAndSts>
      <TxInfAndSts>
        <OrgnlEndToEndId>K563-B001-T087</OrgnlEndToEndId>
        <TxSts>RVMC</TxSts>
        <StsRsnInf><AddtlInf>Peter Schmitz</AddtlInf></StsRsnInf>
        <OrgnlTxRef>
          <Cdtr><Pty><Nm>P. Schmitz</Nm></Pty></Cdtr>
          <CdtrAcct><Id><IBAN>DE34500500009876543210</IBAN></Id></CdtrAcct>
        </OrgnlTxRef>
      </TxInfAndSts>
    </OrgnlPmtInfAndSts>
  </CstmrPmtStsRpt>
</Document>"#;

    #[test]
    fn a_verification_of_payee_report_is_read_as_such() {
        let doc = parse_pain002(VOP_REPORT).unwrap();

        // The group summarises a file that completed with mismatches. That is
        // not an acceptance, and must not be reported as one.
        assert_eq!(doc.group_status, Some(PaymentStatus::Rvcm));
        assert!(doc.group_status.as_ref().unwrap().is_verification());
        assert!(!doc.is_fully_accepted());
        assert!(!doc.has_rejections(), "a name mismatch is not a rejection");

        // 462 payments, reported as counts rather than 462 elements.
        assert_eq!(doc.group_status_counts.len(), 4);
        let matched = doc
            .group_status_counts
            .iter()
            .find(|c| c.status == PaymentStatus::Rcvc)
            .unwrap();
        assert_eq!(matched.count, 454);
        assert_eq!(
            doc.group_status_counts.iter().map(|c| c.count).sum::<u64>(),
            462
        );

        let block = &doc.payment_info_statuses[0];
        assert_eq!(block.status_counts.len(), 2);

        // A no-match: the payer must decide whether to proceed.
        let no_match = &block.transactions[0];
        assert_eq!(
            no_match.status.as_ref().unwrap().verification(),
            Some(VerificationOutcome::NoMatch)
        );
        // `Cdtr` is a Party40Choice from .001.10 — the name lives under `Pty`,
        // and reading only the flat form lost every party name in a current
        // report.
        assert_eq!(
            no_match.original_creditor_name.as_deref(),
            Some("Creditor Name")
        );
        assert_eq!(
            no_match.original_creditor_iban.as_deref(),
            Some("DE21500500009876543210")
        );

        // A close match carries the payee's *actual* name for display.
        let close = &block.transactions[1];
        assert_eq!(
            close.status.as_ref().unwrap().verification(),
            Some(VerificationOutcome::CloseMatch)
        );
        assert_eq!(close.original_creditor_name.as_deref(), Some("P. Schmitz"));
        assert_eq!(close.additional_info, ["Peter Schmitz"]);
    }

    #[test]
    fn every_addtlinf_occurrence_is_kept() {
        // `AddtlInf` is maxOccurs="unbounded"; a legal notice spans lines and a
        // close-match name over 105 characters arrives split in two. Keeping
        // only the first truncated both.
        let doc = parse_pain002(VOP_REPORT).unwrap();
        let _ = doc;
        let xml = VOP_REPORT.replace(
            "<StsRsnInf><AddtlInf>Peter Schmitz</AddtlInf></StsRsnInf>",
            "<StsRsnInf><AddtlInf>Peter</AddtlInf><AddtlInf>Schmitz</AddtlInf></StsRsnInf>",
        );
        let doc = parse_pain002(&xml).unwrap();
        assert_eq!(
            doc.payment_info_statuses[0].transactions[1].additional_info,
            ["Peter", "Schmitz"]
        );
    }

    #[test]
    fn verification_statuses_are_not_acceptances() {
        // `RCVC` says a name matched, which is a different question from
        // whether the payment was taken.
        for code in ["RCVC", "RVMC", "RVNM", "RVNA", "RVCM"] {
            let status = PaymentStatus::from_code(code);
            assert!(!status.is_accepted(), "{code} is not an acceptance");
            assert!(!status.is_rejected(), "{code} is not a rejection");
            assert!(status.is_verification(), "{code} is a verification status");
            assert_eq!(status.as_code(), code);
        }
        assert_eq!(
            PaymentStatus::Rcvc.verification(),
            Some(VerificationOutcome::Match)
        );
        // The group-level summary is about a file, not one payee.
        assert_eq!(PaymentStatus::Rvcm.verification(), None);
        assert!(!PaymentStatus::Actc.is_verification());
    }

    #[test]
    fn parse_not_pain002() {
        let err = parse_pain002("<Document><SomethingElse/></Document>").unwrap_err();
        assert_eq!(err, Pain002ParseError::NotPain002);
    }

    #[test]
    fn payment_status_codes() {
        assert!(PaymentStatus::Actc.is_accepted());
        assert!(PaymentStatus::Acsc.is_final());
        assert!(PaymentStatus::Rjct.is_rejected());
        assert!(PaymentStatus::Rjct.is_final());
        assert!(!PaymentStatus::Pdng.is_final());
        assert!(!PaymentStatus::Part.is_accepted());
    }

    #[test]
    fn reason_code_roundtrip() {
        assert_eq!(ReasonCode::from_code("AC01"), ReasonCode::Ac01);
        assert_eq!(ReasonCode::from_code("md01"), ReasonCode::Md01);
        assert_eq!(
            ReasonCode::from_code("ZZZZ"),
            ReasonCode::Other("ZZZZ".into())
        );
        assert_eq!(ReasonCode::Md06.as_code(), "MD06");
    }

    #[test]
    fn payment_status_fromstr() {
        assert_eq!(
            "ACTC".parse::<PaymentStatus>().unwrap(),
            PaymentStatus::Actc
        );
        assert_eq!(
            "rjct".parse::<PaymentStatus>().unwrap(),
            PaymentStatus::Rjct
        );
        assert_eq!(
            "XXXX".parse::<PaymentStatus>().unwrap(),
            PaymentStatus::Other("XXXX".into())
        );
    }

    #[test]
    fn reason_code_fromstr() {
        assert_eq!("MD01".parse::<ReasonCode>().unwrap(), ReasonCode::Md01);
        assert_eq!("am04".parse::<ReasonCode>().unwrap(), ReasonCode::Am04);
    }

    #[test]
    fn payment_status_display() {
        assert_eq!(PaymentStatus::Actc.to_string(), "ACTC");
        assert_eq!(PaymentStatus::Rjct.to_string(), "RJCT");
        assert_eq!(PaymentStatus::Other("CUST".into()).to_string(), "CUST");
    }

    #[test]
    fn original_message_type_fromstr() {
        assert_eq!(
            "pain.001".parse::<OriginalMessageType>().unwrap(),
            OriginalMessageType::CreditTransfer
        );
        assert_eq!(
            "pain.001.003.03".parse::<OriginalMessageType>().unwrap(),
            OriginalMessageType::CreditTransfer
        );
        assert_eq!(
            "pain.008".parse::<OriginalMessageType>().unwrap(),
            OriginalMessageType::DirectDebit
        );
        assert_eq!(
            "pain.007".parse::<OriginalMessageType>().unwrap(),
            OriginalMessageType::Other("pain.007".into())
        );
    }

    #[test]
    fn prtry_reason_code_per_block() {
        // Bug fix: Prtry should be extracted even if an earlier block set reason_codes
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Document xmlns="urn:iso:std:iso:20022:tech:xsd:pain.002.003.03">
  <CstmrPmtStsRpt>
    <GrpHdr><MsgId>PRTRY-TEST</MsgId><CreDtTm>2026-07-14T10:00:00</CreDtTm></GrpHdr>
    <OrgnlGrpInfAndSts>
      <OrgnlMsgId>ORIG-001</OrgnlMsgId>
      <OrgnlMsgNmId>pain.008</OrgnlMsgNmId>
      <GrpSts>PART</GrpSts>
    </OrgnlGrpInfAndSts>
    <OrgnlPmtInfAndSts>
      <OrgnlPmtInfId>PMT-001</OrgnlPmtInfId>
      <TxInfAndSts>
        <OrgnlEndToEndId>E2E-PRTRY</OrgnlEndToEndId>
        <TxSts>RJCT</TxSts>
        <StsRsnInf>
          <Rsn><Cd>AC04</Cd></Rsn>
        </StsRsnInf>
        <StsRsnInf>
          <Rsn><Prtry>BANK-INTERNAL-007</Prtry></Rsn>
          <AddtlInf>Proprietary bank reason</AddtlInf>
        </StsRsnInf>
      </TxInfAndSts>
    </OrgnlPmtInfAndSts>
  </CstmrPmtStsRpt>
</Document>"#;
        let doc = parse_pain002(xml).unwrap();
        let tx = &doc.payment_info_statuses[0].transactions[0];
        // Both blocks should be extracted — not just the first
        assert_eq!(
            tx.reason_codes.len(),
            2,
            "both Cd and Prtry must be captured"
        );
        assert_eq!(tx.reason_codes[0], ReasonCode::Ac04);
        assert_eq!(
            tx.reason_codes[1],
            ReasonCode::Other("BANK-INTERNAL-007".into())
        );
        assert_eq!(tx.additional_info, ["Proprietary bank reason"]);
    }

    #[test]
    fn rsn_cd_path_is_precise() {
        // Bug fix: Cd should be read via StsRsnInf/Rsn/Cd path, not anywhere in block
        // This test verifies that a hypothetical <Cd>SEPA</Cd> in another sub-element
        // doesn't accidentally get picked up as a reason code.
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Document xmlns="urn:iso:std:iso:20022:tech:xsd:pain.002.003.03">
  <CstmrPmtStsRpt>
    <GrpHdr><MsgId>RSN-PATH-TEST</MsgId><CreDtTm>2026-07-14T10:00:00</CreDtTm></GrpHdr>
    <OrgnlGrpInfAndSts>
      <OrgnlMsgId>ORIG-001</OrgnlMsgId>
      <OrgnlMsgNmId>pain.001</OrgnlMsgNmId>
      <GrpSts>RJCT</GrpSts>
    </OrgnlGrpInfAndSts>
    <OrgnlPmtInfAndSts>
      <OrgnlPmtInfId>PMT-001</OrgnlPmtInfId>
      <TxInfAndSts>
        <OrgnlEndToEndId>E2E-001</OrgnlEndToEndId>
        <TxSts>RJCT</TxSts>
        <StsRsnInf>
          <Orgtr><Id><OrgId><BICOrBEI>BANKDEFF</BICOrBEI></OrgId></Id></Orgtr>
          <Rsn><Cd>AM04</Cd></Rsn>
        </StsRsnInf>
      </TxInfAndSts>
    </OrgnlPmtInfAndSts>
  </CstmrPmtStsRpt>
</Document>"#;
        let doc = parse_pain002(xml).unwrap();
        let tx = &doc.payment_info_statuses[0].transactions[0];
        assert_eq!(tx.reason_codes, vec![ReasonCode::Am04]);
    }

    #[test]
    fn parse_prefixed_namespace() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ns2:Document xmlns:ns2="urn:iso:std:iso:20022:tech:xsd:pain.002.003.03">
  <ns2:CstmrPmtStsRpt>
    <ns2:GrpHdr>
      <ns2:MsgId>MSG-PREFIX-001</ns2:MsgId>
      <ns2:CreDtTm>2026-07-14T12:00:00</ns2:CreDtTm>
    </ns2:GrpHdr>
    <ns2:OrgnlGrpInfAndSts>
      <ns2:OrgnlMsgId>ORIG-001</ns2:OrgnlMsgId>
      <ns2:OrgnlMsgNmId>pain.008</ns2:OrgnlMsgNmId>
      <ns2:GrpSts>ACTC</ns2:GrpSts>
    </ns2:OrgnlGrpInfAndSts>
  </ns2:CstmrPmtStsRpt>
</ns2:Document>"#;
        let doc = parse_pain002(xml).unwrap();
        assert_eq!(doc.msg_id, "MSG-PREFIX-001");
        assert_eq!(doc.original_msg_id, "ORIG-001");
        assert_eq!(
            doc.original_msg_type,
            Some(OriginalMessageType::DirectDebit)
        );
        assert_eq!(doc.group_status, Some(PaymentStatus::Actc));
    }
}
