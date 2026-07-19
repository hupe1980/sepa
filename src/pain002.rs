//! ISO 20022 pain.002 — Customer Payment Status Report parser.
//!
//! Parses the bank's response to an initiated payment batch (pain.001 or pain.008).
//! Supports all DK/EPC SEPA namespace variants:
//!
//! | Schema | Used by |
//! |---|---|
//! | `pain.002.003.03` | Deutsche Kreditwirtschaft (DK) standard |
//! | `pain.002.002.03` | DFÜ-Abkommen reference, some banks |
//! | `pain.002.001.03` | ISO standard namespace, some banks |
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

use crate::xml::{Document, Node, XmlError};

// ── known namespaces ──────────────────────────────────────────────────────────

/// Known pain.002 XML namespace URIs.
pub mod ns {
    /// Deutsche Kreditwirtschaft DK V2.7 — most common in Germany.
    pub const PAIN002_003_03: &str = "urn:iso:std:iso:20022:tech:xsd:pain.002.003.03";
    /// DFÜ-Abkommen reference schema.
    pub const PAIN002_002_03: &str = "urn:iso:std:iso:20022:tech:xsd:pain.002.002.03";
    /// ISO 20022 standard namespace.
    pub const PAIN002_001_03: &str = "urn:iso:std:iso:20022:tech:xsd:pain.002.001.03";
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
    /// Unknown or bank-specific status code.
    Other(String),
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
            Self::Other(s) => s,
        }
    }

    /// Returns `true` for accepted statuses (ACTC, ACCP, ACSP, ACSC, ACWC).
    #[inline]
    #[must_use]
    pub fn is_accepted(&self) -> bool {
        matches!(
            self,
            Self::Actc | Self::Accp | Self::Acsp | Self::Acsc | Self::Acwc
        )
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

// ── TransactionStatus ─────────────────────────────────────────────────────────

/// Status of a single transaction within a pain.002 report.
#[derive(Debug, Clone)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TransactionStatus {
    /// Original end-to-end ID from the initiating message (`OrgnlEndToEndId`).
    pub original_end_to_end_id: String,
    /// Transaction-level status code.
    pub status: PaymentStatus,
    /// Reason codes explaining a rejection (`StsRsnInf/Rsn/Cd`).
    pub reason_codes: Vec<ReasonCode>,
    /// Optional additional reason information (`StsRsnInf/AddtlInf`).
    pub additional_info: Option<String>,
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
    /// Original `PmtInfId` from the initiating message.
    pub original_payment_info_id: String,
    /// Payment-information-level status, if present.
    pub status: Option<PaymentStatus>,
    /// Per-transaction statuses within this payment info block.
    pub transactions: Vec<TransactionStatus>,
}

impl PaymentInfoStatus {
    /// Returns `true` when any transaction in this block was rejected.
    #[must_use]
    pub fn has_rejections(&self) -> bool {
        self.transactions.iter().any(|t| t.status.is_rejected())
            || self.status.as_ref().is_some_and(PaymentStatus::is_rejected)
    }

    /// Collect all reason codes from rejected transactions.
    #[must_use]
    pub fn rejection_reasons(&self) -> Vec<&ReasonCode> {
        self.transactions
            .iter()
            .filter(|t| t.status.is_rejected())
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
    pub original_msg_id: String,
    /// `OrgnlMsgNmId` — identifies whether this is a SCT or SDD response.
    pub original_msg_type: OriginalMessageType,
    /// Group-level status, if present in `OrgnlGrpInfAndSts/GrpSts`.
    pub group_status: Option<PaymentStatus>,
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
        self.group_status
            .as_ref()
            .is_some_and(PaymentStatus::is_accepted)
            && self
                .payment_info_statuses
                .iter()
                .all(|p| !p.has_rejections())
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
            .filter(|t| t.status.is_rejected())
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
    #[error("missing required pain.002 element: <{tag}>")]
    MissingElement {
        /// Name of the missing XML element.
        tag: &'static str,
    },
    /// An amount string could not be parsed as EUR cents.
    #[error("invalid amount value in pain.002: {raw:?}")]
    InvalidAmount {
        /// The raw string that failed to parse.
        raw: String,
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

    let original_msg_type = orig_grp.text_of("OrgnlMsgNmId").map_or_else(
        || OriginalMessageType::Other(String::new()),
        OriginalMessageType::from_msg_name_id,
    );

    let group_status = orig_grp.text_of("GrpSts").map(PaymentStatus::from_code);

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
        original_payment_info_id: block
            .text_of("OrgnlPmtInfId")
            .unwrap_or("NOTPROVIDED")
            .to_owned(),
        status: block.text_of("PmtInfSts").map(PaymentStatus::from_code),
        transactions: block
            .children_named("TxInfAndSts")
            .map(parse_transaction_status)
            .collect(),
    }
}

fn parse_transaction_status(tx: &Node) -> TransactionStatus {
    let original_end_to_end_id = tx
        .text_of("OrgnlEndToEndId")
        .unwrap_or("NOTPROVIDED")
        .to_owned();

    let status = tx.text_of("TxSts").map_or_else(
        || PaymentStatus::Other("UNKNOWN".to_owned()),
        PaymentStatus::from_code,
    );

    // One reason code per StsRsnInf block. `Rsn` is a choice between a typed
    // `Cd` and a bank-proprietary `Prtry`; each block is inspected separately so
    // a proprietary code in a later block is not masked by a typed code in an
    // earlier one.
    let mut reason_codes = Vec::new();
    let mut additional_info: Option<String> = None;
    for rsn_block in tx.children_named("StsRsnInf") {
        if let Some(code) = rsn_block.child("Rsn").and_then(Node::code) {
            reason_codes.push(ReasonCode::from_code(code));
        }
        if additional_info.is_none() {
            additional_info = rsn_block.text_of("AddtlInf").map(str::to_owned);
        }
    }

    let orig_tx_ref = tx.child("OrgnlTxRef");

    // `Amt` is an AmountType4Choice: InstdAmt or EqvtAmt/Amt. Some banks also
    // place InstdAmt directly under OrgnlTxRef, so fall back to a deep search.
    let original_amount_ct = orig_tx_ref
        .and_then(|r| {
            r.text_at(&["Amt", "InstdAmt"])
                .or_else(|| r.text_of_descendant("InstdAmt"))
        })
        .and_then(crate::ct_from_eur_str);

    let party = |tags: [&str; 2]| -> (Option<String>, Option<String>) {
        orig_tx_ref.map_or((None, None), |r| {
            (
                r.text_at(&[tags[0], "Nm"]).map(str::to_owned),
                r.text_at(&[tags[1], "Id", "IBAN"]).map(str::to_owned),
            )
        })
    };

    let (original_debtor_name, original_debtor_iban) = party(["Dbtr", "DbtrAcct"]);
    let (original_creditor_name, original_creditor_iban) = party(["Cdtr", "CdtrAcct"]);

    TransactionStatus {
        original_end_to_end_id,
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
        assert_eq!(doc.original_msg_type, OriginalMessageType::CreditTransfer);
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
        assert_eq!(tx1.original_end_to_end_id, "E2E-001");
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
        assert_eq!(
            tx2.additional_info.as_deref(),
            Some("Customer order to stop")
        );
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
        assert_eq!(
            tx.additional_info.as_deref(),
            Some("Proprietary bank reason")
        );
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
        assert_eq!(doc.original_msg_type, OriginalMessageType::DirectDebit);
        assert_eq!(doc.group_status, Some(PaymentStatus::Actc));
    }
}
