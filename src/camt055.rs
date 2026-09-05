//! ISO 20022 camt.055 — Customer Payment Cancellation Request (recall).
//!
//! The message that undoes a submission **before** it settles. A pain.001 or
//! pain.008 file has gone to the bank and something is wrong with it — it was
//! sent twice, the amounts came from a bad run, a collection is fraudulent —
//! and camt.055 asks the bank to stop it.
//!
//! ## Where it sits among the R-messages
//!
//! Three different messages undo a payment, and picking the wrong one wastes
//! the window in which anything can still be done:
//!
//! | Message | Who sends it | When | Effect |
//! |---|---|---|---|
//! | **camt.055** (this module) | the originator, to its bank | before settlement | *asks* the bank to stop it |
//! | [`pain.007`](crate::pain007) | the creditor, to its bank | after a direct debit settled | *instructs* a reversal — money goes back |
//! | R-transaction in [`camt.054`](crate::camt054) | the bank, to the customer | after settlement | *reports* a return the debtor's side triggered |
//!
//! The distinction that matters: a camt.055 is a **request**, and the bank may
//! refuse it. The answer arrives as [`camt.029`](crate::camt029) — see
//! [`parse_camt029`](crate::parse_camt029) — and until it does, nothing has
//! been cancelled. A pain.007 is not a request; it is an instruction, and it
//! only applies to a collection that has already been taken.
//!
//! ## Three scopes, and they are alternatives
//!
//! The schema is a nest of optional elements and admits combinations that mean
//! nothing. This module makes the three real ones the only reachable ones:
//!
//! | Scope | Built with | Emits |
//! |---|---|---|
//! | The whole file | [`Camt055Builder::cancel_whole_message`] | `OrgnlGrpInfAndCxl` with `GrpCxl` = `true` |
//! | A whole `PmtInf` group | [`CancellationGroup::cancel_whole_group`] | `OrgnlPmtInfAndCxl` with `PmtInfCxl` = `true` |
//! | Named transactions | [`CancellationGroup::add_entry`] | `OrgnlPmtInfAndCxl` with one `TxInf` each |
//!
//! Mixing them is rejected by `build()` rather than emitted: "cancel the whole
//! message, and also specifically these two transactions" is not a thing a bank
//! can action, and the XSD is perfectly happy with it.
//!
//! ## A reason is required
//!
//! ISO types `CxlRsnInf` as optional. A bank cannot act on a reasonless recall,
//! and every published usage guideline makes it mandatory — so here it is a
//! constructor argument on all three scopes and the reasonless form is not a
//! value this module can be asked to emit. See [`CancellationReason`].
//!
//! ## Version
//!
//! `camt.055.001.05`, which is the version the DFÜ-Abkommen names; the answer
//! is `camt.029.001.06`. Unlike the payment-initiation messages this is not a
//! per-bank choice — the DK specifies one version, and no other is accepted.
//!
//! ## Example
//!
//! ```
//! use sepa::{
//!     CancellationEntry, CancellationGroup, CancellationReason, Camt055Builder,
//!     DirectDebitEntry, DirectDebitGroup, IsoDate, OriginalMessage, Pain008Builder,
//!     validate_bic, validate_creditor_id, validate_iban,
//! };
//!
//! let iban = validate_iban("DE89370400440532013000")?;
//! let ci = validate_creditor_id("DE98ZZZ09999999999")?;
//! let collect = IsoDate::new(2026, 7, 20)?;
//!
//! // The run that went out this morning.
//! let group = DirectDebitGroup::new("Stadtwerke GmbH", &iban, &ci, collect)
//!     .payment_info_id("PMT-2026-07-A")
//!     .add_entry(DirectDebitEntry::new(
//!         "MND-1", "2024-06-01".parse()?, "Max Mustermann", iban.clone(), 7_500, "E2E-1",
//!     ));
//! let submitted = Pain008Builder::new("Stadtwerke GmbH", "DD-2026-07-001").add_group(group.clone());
//! let _ = submitted.build()?;
//!
//! // One collection in it was a duplicate. Recall that one, not the file.
//! let recall = Camt055Builder::new(
//!     "CXL-2026-07-001",
//!     "Stadtwerke GmbH",
//!     validate_bic("COBADEFFXXX")?,
//!     OriginalMessage::from_direct_debit(&submitted),
//! )
//! .add_group(
//!     CancellationGroup::new("PMT-2026-07-A")
//!         .add_entry(CancellationEntry::new("E2E-1", CancellationReason::Dupl)),
//! )
//! .build()?;
//!
//! assert!(recall.contains("camt.055.001.05"));
//! assert!(recall.contains("<OrgnlEndToEndId>E2E-1</OrgnlEndToEndId>"));
//! assert!(recall.contains("<Cd>DUPL</Cd>"));
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use crate::bic::BicPattern;
use crate::date::IsoDate;
use crate::party::Party;
use crate::validate::{
    BuildError, CharsetPolicy, Locate, Location, MAX_ADDITIONAL_INFO_LEN, MAX_ID_LEN,
    ValidationError, WriteError, check_amount, check_id, check_text,
};
use crate::xml_util::{write_escaped, write_eur};
use crate::{Bic, IsoDateTime, ct_to_eur_str};

/// The XML namespace of the one camt.055 version SEPA uses.
pub const NAMESPACE: &str = "urn:iso:std:iso:20022:tech:xsd:camt.055.001.05";

/// The ISO 20022 message identifier, for `OrgnlMsgNmId` on the way back.
pub const MESSAGE_ID: &str = "camt.055.001.05";

// ── CancellationReason ────────────────────────────────────────────────────────

/// Why a payment is being recalled (`CxlRsnInf/Rsn`).
///
/// `CancellationReason5Code` is a **closed** enumeration — unlike the purpose
/// code lists, ISO does not revise it quarterly — so an unrecognised code is
/// not a `Cd` the schema would accept. [`Other`](Self::Other) is therefore
/// written into `Prtry`, which is the branch the choice provides for exactly
/// this, rather than into `Cd` where it would be schema-invalid.
///
/// # Examples
///
/// ```
/// use sepa::CancellationReason;
///
/// assert_eq!(CancellationReason::Dupl.as_code(), "DUPL");
/// assert_eq!("fraud".parse::<CancellationReason>().unwrap(), CancellationReason::Frad);
///
/// // A code outside the closed list stays proprietary rather than pretending.
/// let local: CancellationReason = "XY99".parse().unwrap();
/// assert!(!local.is_iso_code());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum CancellationReason {
    /// `DUPL` — the payment was sent twice.
    Dupl,
    /// `AGNT` — an agent in the chain is wrong or unreachable.
    Agnt,
    /// `CURR` — the currency is wrong.
    Curr,
    /// `CUST` — the customer asked for it, with no further reason given.
    Cust,
    /// `UPAY` — the payment was not due ("undue payment").
    Upay,
    /// `CUTA` — cancelled to enable a technical transfer in its place.
    Cuta,
    /// `TECH` — a technical problem produced the instruction.
    Tech,
    /// `FRAD` — the instruction is fraudulent.
    Frad,
    /// A code outside `CancellationReason5Code`, written to `Prtry`.
    Other(String),
}

impl CancellationReason {
    /// The code as it goes on the wire.
    #[must_use]
    pub fn as_code(&self) -> &str {
        match self {
            Self::Dupl => "DUPL",
            Self::Agnt => "AGNT",
            Self::Curr => "CURR",
            Self::Cust => "CUST",
            Self::Upay => "UPAY",
            Self::Cuta => "CUTA",
            Self::Tech => "TECH",
            Self::Frad => "FRAD",
            Self::Other(s) => s,
        }
    }

    /// Whether this is one of the eight `CancellationReason5Code` values, and
    /// so goes in `Cd` rather than `Prtry`.
    #[must_use]
    pub const fn is_iso_code(&self) -> bool {
        !matches!(self, Self::Other(_))
    }

    /// Validate the code's shape.
    ///
    /// # Errors
    ///
    /// [`ValidationError::Empty`] for a blank code, [`ValidationError::TooLong`]
    /// past `Max35Text` for a proprietary one, or
    /// [`ValidationError::InvalidCharacter`] for anything non-alphanumeric —
    /// `Prtry` is a text element, so it also has to be SEPA-legal.
    pub fn validate(&self, field: &'static str) -> Result<(), ValidationError> {
        let code = self.as_code();
        check_text(field, code, MAX_ID_LEN)?;
        match code.chars().find(|c| !c.is_ascii_alphanumeric()) {
            Some(ch) => Err(ValidationError::InvalidCharacter { field, ch }),
            None => Ok(()),
        }
    }

    /// Write `<Rsn>` with the branch this code belongs in.
    fn write_xml<W: std::fmt::Write>(&self, w: &mut W) -> std::fmt::Result {
        if self.is_iso_code() {
            write!(w, "<Rsn><Cd>{}</Cd></Rsn>", self.as_code())
        } else {
            w.write_str("<Rsn><Prtry>")?;
            write_escaped(w, self.as_code())?;
            w.write_str("</Prtry></Rsn>")
        }
    }
}

impl std::fmt::Display for CancellationReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_code())
    }
}

impl std::str::FromStr for CancellationReason {
    type Err = std::convert::Infallible;
    /// Always succeeds — a code outside the closed list becomes
    /// [`Other`](Self::Other) and is written to `Prtry`.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.trim().to_ascii_uppercase().as_str() {
            "DUPL" | "DUPLICATE" => Self::Dupl,
            "AGNT" => Self::Agnt,
            "CURR" => Self::Curr,
            "CUST" => Self::Cust,
            "UPAY" => Self::Upay,
            "CUTA" => Self::Cuta,
            "TECH" => Self::Tech,
            "FRAD" | "FRAUD" => Self::Frad,
            other => Self::Other(other.to_owned()),
        })
    }
}

// ── CaseParty ─────────────────────────────────────────────────────────────────

/// One side of the case assignment — the party asking, or the one asked.
///
/// ISO types both as a `Party12Choice`: a non-financial party (`Pty`) or a
/// financial institution (`Agt`). For a customer recall the assigner is the
/// originator by name and the assignee is its bank by BIC, which is what the
/// `From` implementations make the short spelling.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum CaseParty {
    /// `Pty` — a non-financial party.
    Party(Party),
    /// `Agt` — a financial institution, identified by its BIC.
    Agent(Bic),
}

impl CaseParty {
    fn validate(
        &self,
        field: &'static str,
        charset: CharsetPolicy,
        pattern: BicPattern,
    ) -> Result<(), ValidationError> {
        match self {
            Self::Party(p) => p.validate(field, charset),
            Self::Agent(bic) if bic.fits(pattern) => Ok(()),
            Self::Agent(bic) => Err(ValidationError::SchemaPattern {
                field,
                value: bic.as_str().to_owned(),
                schema: MESSAGE_ID,
                expected: pattern.as_xsd_pattern(),
            }),
        }
    }

    fn write_xml<W: std::fmt::Write>(
        &self,
        w: &mut W,
        tag: &str,
        charset: CharsetPolicy,
    ) -> std::fmt::Result {
        match self {
            // `Party::write_xml` emits `Nm` and the `Id` choice, which is
            // exactly what `PartyIdentification43` admits here.
            Self::Party(p) => {
                write!(w, "<{tag}>")?;
                p.write_xml_inline(w, "Pty", charset)?;
                write!(w, "</{tag}>")
            }
            Self::Agent(bic) => write!(
                w,
                "<{tag}><Agt><FinInstnId><BICFI>{}</BICFI></FinInstnId></Agt></{tag}>",
                bic.as_str()
            ),
        }
    }
}

impl From<Party> for CaseParty {
    fn from(p: Party) -> Self {
        Self::Party(p)
    }
}

impl From<&str> for CaseParty {
    fn from(name: &str) -> Self {
        Self::Party(Party::new(name))
    }
}

impl From<String> for CaseParty {
    fn from(name: String) -> Self {
        Self::Party(Party::new(name))
    }
}

impl From<Bic> for CaseParty {
    fn from(bic: Bic) -> Self {
        Self::Agent(bic)
    }
}

// ── OriginalMessage ───────────────────────────────────────────────────────────

/// The submission being recalled (`OrgnlMsgId` / `OrgnlMsgNmId`).
///
/// Build it from the builder that produced the file — [`from_credit_transfer`]
/// and [`from_direct_debit`] copy the identifier, the message name, the pinned
/// creation timestamp and the totals, so a recall cannot name a message that
/// was never sent under that identifier.
///
/// [`from_credit_transfer`]: Self::from_credit_transfer
/// [`from_direct_debit`]: Self::from_direct_debit
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct OriginalMessage {
    /// `OrgnlMsgId` — the `GrpHdr/MsgId` of the file that was submitted.
    pub message_id: String,
    /// `OrgnlMsgNmId` — its message identifier, e.g. `pain.008.001.08`.
    pub message_name_id: String,
    /// `OrgnlCreDtTm` — its `GrpHdr/CreDtTm`, when it was pinned.
    ///
    /// `None` when the sender did not pin one: a re-derived "now" would name a
    /// different message than the one that was sent, which is the opposite of
    /// what this element is for.
    pub created_at: Option<IsoDateTime>,
    /// `NbOfTxs` of the original message.
    pub number_of_transactions: Option<u64>,
    /// `CtrlSum` of the original message, in ct.
    pub control_sum_ct: Option<i64>,
}

impl OriginalMessage {
    /// Name a submission by its identifier and message name.
    ///
    /// `message_name_id` is the ISO identifier of what was sent, such as
    /// `"pain.008.001.08"` — a `DirectDebitSchema` or `CreditTransferSchema`
    /// renders to exactly that string.
    pub fn new(message_id: impl Into<String>, message_name_id: impl Into<String>) -> Self {
        Self {
            message_id: message_id.into(),
            message_name_id: message_name_id.into(),
            created_at: None,
            number_of_transactions: None,
            control_sum_ct: None,
        }
    }

    /// Describe the pain.001 message this builder produces.
    ///
    /// Takes the builder rather than the XML: every field this element needs is
    /// already in it, and re-parsing a document to recall it would be reading
    /// back what the caller has.
    #[must_use]
    pub fn from_credit_transfer(builder: &crate::Pain001Builder) -> Self {
        Self {
            message_id: builder.message_id().to_owned(),
            message_name_id: builder.schema_version().message_id().to_owned(),
            created_at: builder.creation_timestamp(),
            number_of_transactions: Some(builder.entry_count() as u64),
            control_sum_ct: Some(builder.total_ct()),
        }
    }

    /// Describe the pain.008 message this builder produces.
    #[must_use]
    pub fn from_direct_debit(builder: &crate::Pain008Builder) -> Self {
        Self {
            message_id: builder.message_id().to_owned(),
            message_name_id: builder.schema_version().message_id().to_owned(),
            created_at: builder.creation_timestamp(),
            number_of_transactions: Some(builder.entry_count() as u64),
            control_sum_ct: Some(builder.total_ct()),
        }
    }

    /// Pin `OrgnlCreDtTm` — the creation timestamp the submitted file carried.
    #[must_use]
    pub fn created_at(mut self, timestamp: IsoDateTime) -> Self {
        self.created_at = Some(timestamp);
        self
    }

    /// Record the original `NbOfTxs` and `CtrlSum`.
    #[must_use]
    pub fn totals(mut self, transactions: u64, control_sum_ct: i64) -> Self {
        self.number_of_transactions = Some(transactions);
        self.control_sum_ct = Some(control_sum_ct);
        self
    }

    fn validate(&self) -> Result<(), ValidationError> {
        check_id("OrgnlGrpInf/OrgnlMsgId", &self.message_id)?;
        check_id("OrgnlGrpInf/OrgnlMsgNmId", &self.message_name_id)
    }

    /// Write the three `OriginalGroupInformation3` elements, unindented.
    fn write_reference<W: std::fmt::Write>(&self, w: &mut W) -> std::fmt::Result {
        w.write_str("<OrgnlMsgId>")?;
        write_escaped(w, &self.message_id)?;
        w.write_str("</OrgnlMsgId><OrgnlMsgNmId>")?;
        write_escaped(w, &self.message_name_id)?;
        w.write_str("</OrgnlMsgNmId>")?;
        if let Some(t) = self.created_at {
            write!(w, "<OrgnlCreDtTm>{t}</OrgnlCreDtTm>")?;
        }
        Ok(())
    }
}

// ── CancellationEntry ─────────────────────────────────────────────────────────

/// One transaction to recall (`TxInf`).
///
/// The bank matches it by `OrgnlEndToEndId`, so that is the required argument;
/// everything else narrows the match and is optional. Copying the original
/// amount and date with [`from_credit_transfer`] or [`from_direct_debit`] is
/// what turns an ambiguous match into an exact one when the same reference has
/// been used twice.
///
/// [`from_credit_transfer`]: Self::from_credit_transfer
/// [`from_direct_debit`]: Self::from_direct_debit
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CancellationEntry {
    /// `OrgnlEndToEndId` — the reference the original transaction carried.
    pub original_end_to_end_id: String,
    /// `CxlRsnInf/Rsn` — why this one is being recalled.
    pub reason: CancellationReason,
    /// `CxlId` — the sender's own identifier for this cancellation.
    pub cancellation_id: Option<String>,
    /// `OrgnlInstrId` — the original `InstrId`, where one was used.
    pub original_instruction_id: Option<String>,
    /// `OrgnlInstdAmt` in ct — the amount the original transaction carried.
    pub original_amount_ct: Option<i64>,
    /// `OrgnlReqdExctnDt` — the original `ReqdExctnDt` of a credit transfer.
    pub original_execution_date: Option<IsoDate>,
    /// `OrgnlReqdColltnDt` — the original `ReqdColltnDt` of a direct debit.
    pub original_collection_date: Option<IsoDate>,
    /// `CxlRsnInf/AddtlInf` — free text, `Max105Text`.
    pub additional_info: Option<String>,
}

impl CancellationEntry {
    /// Recall the transaction that carried `original_end_to_end_id`.
    pub fn new(original_end_to_end_id: impl Into<String>, reason: CancellationReason) -> Self {
        Self {
            original_end_to_end_id: original_end_to_end_id.into(),
            reason,
            cancellation_id: None,
            original_instruction_id: None,
            original_amount_ct: None,
            original_execution_date: None,
            original_collection_date: None,
            additional_info: None,
        }
    }

    /// Recall a credit transfer, copying its reference, amount and date.
    ///
    /// The counterpart of [`ReversalEntry::reverse`](crate::ReversalEntry::reverse):
    /// take the group and the entry you sent, and the identifying fields come
    /// across rather than being retyped.
    #[must_use]
    pub fn from_credit_transfer(
        group: &crate::CreditTransferGroup,
        entry: &crate::CreditTransferEntry,
        reason: CancellationReason,
    ) -> Self {
        Self {
            original_amount_ct: Some(entry.amount_ct),
            original_execution_date: Some(group.requested_execution_date()),
            ..Self::new(entry.end_to_end_id.clone(), reason)
        }
    }

    /// Recall a direct debit collection, copying its reference, amount and date.
    #[must_use]
    pub fn from_direct_debit(
        group: &crate::DirectDebitGroup,
        entry: &crate::DirectDebitEntry,
        reason: CancellationReason,
    ) -> Self {
        Self {
            original_amount_ct: Some(entry.amount_ct),
            original_collection_date: Some(group.requested_collection_date()),
            ..Self::new(entry.end_to_end_id.clone(), reason)
        }
    }

    /// Set `CxlId` — the sender's identifier for this cancellation.
    #[must_use]
    pub fn cancellation_id(mut self, id: impl Into<String>) -> Self {
        self.cancellation_id = Some(id.into());
        self
    }

    /// Set `OrgnlInstrId`.
    #[must_use]
    pub fn original_instruction_id(mut self, id: impl Into<String>) -> Self {
        self.original_instruction_id = Some(id.into());
        self
    }

    /// Set `OrgnlInstdAmt`, in ct.
    #[must_use]
    pub fn original_amount(mut self, amount_ct: i64) -> Self {
        self.original_amount_ct = Some(amount_ct);
        self
    }

    /// Set `CxlRsnInf/AddtlInf` — up to 105 characters of free text.
    #[must_use]
    pub fn additional_info(mut self, text: impl Into<String>) -> Self {
        self.additional_info = Some(text.into());
        self
    }

    fn validate(&self, charset: CharsetPolicy) -> Result<(), ValidationError> {
        check_id("TxInf/OrgnlEndToEndId", &self.original_end_to_end_id)?;
        if let Some(id) = &self.cancellation_id {
            check_id("TxInf/CxlId", id)?;
        }
        if let Some(id) = &self.original_instruction_id {
            check_id("TxInf/OrgnlInstrId", id)?;
        }
        if let Some(amount) = self.original_amount_ct {
            check_amount("TxInf/OrgnlInstdAmt", amount)?;
        }
        self.reason.validate("CxlRsnInf/Rsn")?;
        if let Some(text) = &self.additional_info {
            check_text(
                "CxlRsnInf/AddtlInf",
                &charset.apply("CxlRsnInf/AddtlInf", text)?,
                MAX_ADDITIONAL_INFO_LEN,
            )?;
        }
        Ok(())
    }

    fn write_xml<W: std::fmt::Write>(&self, w: &mut W, charset: CharsetPolicy) -> std::fmt::Result {
        // XSD sequence: CxlId, Case, OrgnlInstrId, OrgnlEndToEndId,
        // OrgnlInstdAmt, OrgnlReqdExctnDt, OrgnlReqdColltnDt, CxlRsnInf.
        w.write_str("        <TxInf>")?;
        if let Some(id) = &self.cancellation_id {
            w.write_str("<CxlId>")?;
            write_escaped(w, id)?;
            w.write_str("</CxlId>")?;
        }
        if let Some(id) = &self.original_instruction_id {
            w.write_str("<OrgnlInstrId>")?;
            write_escaped(w, id)?;
            w.write_str("</OrgnlInstrId>")?;
        }
        w.write_str("<OrgnlEndToEndId>")?;
        write_escaped(w, &self.original_end_to_end_id)?;
        w.write_str("</OrgnlEndToEndId>")?;
        if let Some(amount) = self.original_amount_ct {
            w.write_str("<OrgnlInstdAmt Ccy=\"EUR\">")?;
            write_eur(w, amount)?;
            w.write_str("</OrgnlInstdAmt>")?;
        }
        if let Some(d) = self.original_execution_date {
            write!(w, "<OrgnlReqdExctnDt>{d}</OrgnlReqdExctnDt>")?;
        }
        if let Some(d) = self.original_collection_date {
            write!(w, "<OrgnlReqdColltnDt>{d}</OrgnlReqdColltnDt>")?;
        }
        write_reason(w, &self.reason, self.additional_info.as_deref(), charset)?;
        w.write_str("</TxInf>\n")
    }
}

/// Write one `CxlRsnInf` block.
fn write_reason<W: std::fmt::Write>(
    w: &mut W,
    reason: &CancellationReason,
    additional_info: Option<&str>,
    charset: CharsetPolicy,
) -> std::fmt::Result {
    w.write_str("<CxlRsnInf>")?;
    reason.write_xml(w)?;
    if let Some(text) = additional_info {
        w.write_str("<AddtlInf>")?;
        write_escaped(w, &charset.render(text))?;
        w.write_str("</AddtlInf>")?;
    }
    w.write_str("</CxlRsnInf>")
}

// ── CancellationGroup ─────────────────────────────────────────────────────────

/// One `OrgnlPmtInfAndCxl` — the recall scope for a single submitted `PmtInf`.
///
/// Either the whole group goes ([`cancel_whole_group`]) or named transactions
/// do ([`add_entry`]) — never both, because "cancel all of it, and also these
/// two" is not an instruction. `build()` rejects the combination.
///
/// [`cancel_whole_group`]: Self::cancel_whole_group
/// [`add_entry`]: Self::add_entry
///
/// # Examples
///
/// ```
/// use sepa::{CancellationEntry, CancellationGroup, CancellationReason};
///
/// // The whole group — one batch run twice.
/// let all = CancellationGroup::new("PMT-A").cancel_whole_group(CancellationReason::Dupl);
///
/// // Or two collections out of it.
/// let some = CancellationGroup::new("PMT-B")
///     .add_entry(CancellationEntry::new("E2E-1", CancellationReason::Frad))
///     .add_entry(CancellationEntry::new("E2E-2", CancellationReason::Frad));
/// assert_eq!(some.entry_count(), 2);
/// # let _ = all;
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct CancellationGroup {
    original_payment_info_id: String,
    payment_cancellation_id: Option<String>,
    whole_group: Option<CancellationReason>,
    additional_info: Option<String>,
    entries: Vec<CancellationEntry>,
}

impl CancellationGroup {
    /// Recall inside the `PmtInf` that carried `original_payment_info_id`.
    ///
    /// This is the `PmtInfId` of the group as it was *submitted*, which is why
    /// [`Pain008Builder::payment_info_id`](crate::Pain008Builder) derives a
    /// stable one rather than a random one: a group you cannot name is a group
    /// you cannot recall.
    pub fn new(original_payment_info_id: impl Into<String>) -> Self {
        Self {
            original_payment_info_id: original_payment_info_id.into(),
            payment_cancellation_id: None,
            whole_group: None,
            additional_info: None,
            entries: Vec::new(),
        }
    }

    /// Recall every transaction in the group — `PmtInfCxl` = `true`.
    ///
    /// Mutually exclusive with [`add_entry`](Self::add_entry).
    #[must_use]
    pub fn cancel_whole_group(mut self, reason: CancellationReason) -> Self {
        self.whole_group = Some(reason);
        self
    }

    /// Set `PmtCxlId` — the sender's identifier for this group's cancellation.
    #[must_use]
    pub fn payment_cancellation_id(mut self, id: impl Into<String>) -> Self {
        self.payment_cancellation_id = Some(id.into());
        self
    }

    /// Set `CxlRsnInf/AddtlInf` for a whole-group cancellation (`Max105Text`).
    ///
    /// Belongs to the block [`cancel_whole_group`](Self::cancel_whole_group)
    /// writes, so setting it on a group that names transactions instead is a
    /// `ValidationError::Requires` rather than text that quietly vanishes —
    /// per-transaction free text goes on
    /// [`CancellationEntry::additional_info`].
    #[must_use]
    pub fn additional_info(mut self, text: impl Into<String>) -> Self {
        self.additional_info = Some(text.into());
        self
    }

    /// Recall one named transaction.
    ///
    /// Mutually exclusive with [`cancel_whole_group`](Self::cancel_whole_group).
    #[must_use]
    pub fn add_entry(mut self, entry: CancellationEntry) -> Self {
        self.entries.push(entry);
        self
    }

    /// Recall several named transactions.
    #[must_use]
    pub fn add_entries(mut self, entries: impl IntoIterator<Item = CancellationEntry>) -> Self {
        self.entries.extend(entries);
        self
    }

    /// The `PmtInfId` this group recalls from.
    #[must_use]
    pub fn original_payment_info_id(&self) -> &str {
        &self.original_payment_info_id
    }

    /// How many transactions this group names.
    ///
    /// Zero for a whole-group cancellation: it names none and takes all.
    #[must_use]
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// Total of the original amounts this group names, or `None` when any entry
    /// did not carry one.
    #[must_use]
    pub fn total_ct(&self) -> Option<i64> {
        self.entries
            .iter()
            .try_fold(0i64, |acc, e| acc.checked_add(e.original_amount_ct?))
    }
}

// ── Builder ───────────────────────────────────────────────────────────────────

/// Builder for an ISO 20022 camt.055 Customer Payment Cancellation Request.
///
/// See the [module docs](self) for the three cancellation scopes and why they
/// are alternatives.
#[derive(Debug, Clone)]
pub struct Camt055Builder {
    assignment_id: String,
    assigner: CaseParty,
    assignee: CaseParty,
    created_at: Option<IsoDateTime>,
    case_id: Option<String>,
    original: OriginalMessage,
    whole_message: Option<CancellationReason>,
    whole_message_info: Option<String>,
    groups: Vec<CancellationGroup>,
    charset: CharsetPolicy,
}

impl Camt055Builder {
    /// A recall of `original`, assigned by `assigner` to `assignee`.
    ///
    /// `assignment_id` identifies this request — it is what a
    /// [`camt.029`](crate::camt029) answer echoes back, so it has to come from
    /// a sequence that survives a restart, exactly as a `MsgId` does.
    ///
    /// `assigner` is the party asking (a `&str` becomes a named party) and
    /// `assignee` the one asked (a [`Bic`] becomes an agent).
    #[allow(
        clippy::similar_names,
        reason = "`Assgnr` and `Assgne` are the schema's own element names"
    )]
    pub fn new(
        assignment_id: impl Into<String>,
        assigner: impl Into<CaseParty>,
        assignee: impl Into<CaseParty>,
        original: OriginalMessage,
    ) -> Self {
        Self {
            assignment_id: assignment_id.into(),
            assigner: assigner.into(),
            assignee: assignee.into(),
            created_at: None,
            case_id: None,
            original,
            whole_message: None,
            whole_message_info: None,
            groups: Vec::new(),
            charset: CharsetPolicy::default(),
        }
    }

    /// Pin `Assgnmt/CreDtTm`.
    ///
    /// The same single clock read the payment builders have: left unset,
    /// `build()` stamps [`IsoDateTime::now`].
    #[must_use]
    pub fn created_at(mut self, timestamp: IsoDateTime) -> Self {
        self.created_at = Some(timestamp);
        self
    }

    /// Open a `Case` with this identifier, created by the assigner.
    ///
    /// Optional. A case identifier is what lets a follow-up message refer to
    /// this investigation rather than opening a second one.
    #[must_use]
    pub fn case_id(mut self, id: impl Into<String>) -> Self {
        self.case_id = Some(id.into());
        self
    }

    /// Set how text outside the SEPA character set is handled.
    #[must_use]
    pub fn charset(mut self, policy: CharsetPolicy) -> Self {
        self.charset = policy;
        self
    }

    /// Recall the entire submission — `GrpCxl` = `true`.
    ///
    /// Mutually exclusive with [`add_group`](Self::add_group): naming groups
    /// after asking for the whole file says two different things.
    #[must_use]
    pub fn cancel_whole_message(mut self, reason: CancellationReason) -> Self {
        self.whole_message = Some(reason);
        self
    }

    /// Free text for a whole-message cancellation (`Max105Text`).
    ///
    /// Belongs to the block
    /// [`cancel_whole_message`](Self::cancel_whole_message) writes, so setting
    /// it without one is a `ValidationError::Requires` rather than text that
    /// quietly vanishes.
    #[must_use]
    pub fn additional_info(mut self, text: impl Into<String>) -> Self {
        self.whole_message_info = Some(text.into());
        self
    }

    /// Add a per-group cancellation scope.
    #[must_use]
    pub fn add_group(mut self, group: CancellationGroup) -> Self {
        self.groups.push(group);
        self
    }

    /// Number of cancellation groups.
    #[must_use]
    pub fn group_count(&self) -> usize {
        self.groups.len()
    }

    /// Number of individually named transactions across every group.
    ///
    /// This is `CtrlData/NbOfTxs`. A whole-message or whole-group cancellation
    /// names none, and then no `CtrlData` is written at all — the element
    /// counts what the message lists, and "0" would assert something false.
    #[must_use]
    pub fn entry_count(&self) -> usize {
        self.groups.iter().map(CancellationGroup::entry_count).sum()
    }

    /// `CtrlData/CtrlSum` — the total of the named originals, when every one of
    /// them stated an amount.
    #[must_use]
    pub fn total_ct(&self) -> Option<i64> {
        self.groups
            .iter()
            .try_fold(0i64, |acc, g| acc.checked_add(g.total_ct()?))
    }

    /// Validate the request without producing XML.
    ///
    /// # Errors
    ///
    /// A [`BuildError`] naming both the broken rule and the group and
    /// transaction it belongs to.
    pub fn validate(&self) -> Result<(), BuildError> {
        let msg = Location::message();
        // `Undrlyg` is 1..n, and both of its children are optional — so an
        // empty one is schema-valid and cancels nothing at all.
        if self.whole_message.is_none() && self.groups.is_empty() {
            return Err(BuildError::message(ValidationError::EmptyBatch));
        }
        if self.whole_message.is_some() && !self.groups.is_empty() {
            return Err(BuildError::message(ValidationError::MutuallyExclusive {
                field: "Undrlyg",
                first: "OrgnlGrpInfAndCxl/GrpCxl",
                second: "OrgnlPmtInfAndCxl",
            }));
        }
        check_id("Assgnmt/Id", &self.assignment_id).at(msg)?;
        if let Some(id) = &self.case_id {
            check_id("Case/Id", id).at(msg)?;
        }
        let pattern = BicPattern::LettersOnly;
        self.assigner
            .validate("Assgnmt/Assgnr", self.charset, pattern)
            .at(msg)?;
        self.assignee
            .validate("Assgnmt/Assgne", self.charset, pattern)
            .at(msg)?;
        self.original.validate().at(msg)?;
        match (&self.whole_message, &self.whole_message_info) {
            (Some(reason), info) => {
                reason.validate("OrgnlGrpInfAndCxl/CxlRsnInf/Rsn").at(msg)?;
                self.check_info(info.as_deref()).at(msg)?;
            }
            // `CxlRsnInf` hangs off the group-cancellation block, so free text
            // with no such block has nowhere to go. Dropping it silently is how
            // a caller finds out from a diff of its own output.
            (None, Some(_)) => {
                return Err(BuildError::message(ValidationError::Requires {
                    feature: "OrgnlGrpInfAndCxl/CxlRsnInf/AddtlInf",
                    requires: "cancel_whole_message — the block it belongs to",
                }));
            }
            (None, None) => {}
        }

        let mut seen = std::collections::BTreeSet::new();
        for (i, g) in self.groups.iter().enumerate() {
            self.validate_group(i, g, &mut seen)?;
        }
        Ok(())
    }

    fn check_info(&self, text: Option<&str>) -> Result<(), ValidationError> {
        let Some(text) = text else { return Ok(()) };
        check_text(
            "CxlRsnInf/AddtlInf",
            &self.charset.apply("CxlRsnInf/AddtlInf", text)?,
            MAX_ADDITIONAL_INFO_LEN,
        )
    }

    fn validate_group(
        &self,
        i: usize,
        g: &CancellationGroup,
        seen: &mut std::collections::BTreeSet<String>,
    ) -> Result<(), BuildError> {
        let at = Location::group(i);
        check_id(
            "OrgnlPmtInfAndCxl/OrgnlPmtInfId",
            &g.original_payment_info_id,
        )
        .at(at)?;
        // Two blocks naming one submitted group leave the bank to guess which
        // instruction wins — the same rule `PmtInfId` gets on the way out.
        if !seen.insert(g.original_payment_info_id.clone()) {
            return Err(BuildError::group(
                i,
                ValidationError::Duplicate {
                    field: "OrgnlPmtInfAndCxl/OrgnlPmtInfId",
                    value: g.original_payment_info_id.clone(),
                },
            ));
        }
        if let Some(id) = &g.payment_cancellation_id {
            check_id("OrgnlPmtInfAndCxl/PmtCxlId", id).at(at)?;
        }
        match (&g.whole_group, g.entries.is_empty()) {
            // Neither: this block cancels nothing, which the XSD permits.
            (None, true) => return Err(BuildError::group(i, ValidationError::EmptyBatch)),
            // Both: "all of it, and specifically these" is not an instruction.
            (Some(_), false) => {
                return Err(BuildError::group(
                    i,
                    ValidationError::MutuallyExclusive {
                        field: "OrgnlPmtInfAndCxl",
                        first: "PmtInfCxl",
                        second: "TxInf",
                    },
                ));
            }
            (Some(reason), true) => {
                reason.validate("CxlRsnInf/Rsn").at(at)?;
                self.check_info(g.additional_info.as_deref()).at(at)?;
            }
            (None, false) => {
                // Same argument one level down: the group's own `CxlRsnInf` is
                // written only beside `PmtInfCxl`. Per-transaction free text
                // goes on the transaction.
                if g.additional_info.is_some() {
                    return Err(BuildError::group(
                        i,
                        ValidationError::Requires {
                            feature: "OrgnlPmtInfAndCxl/CxlRsnInf/AddtlInf",
                            requires: "cancel_whole_group — the block it belongs to",
                        },
                    ));
                }
                for (j, e) in g.entries.iter().enumerate() {
                    e.validate(self.charset).at(Location::transaction(i, j))?;
                }
            }
        }
        Ok(())
    }

    /// Validate the request and generate the camt.055 XML.
    ///
    /// # Errors
    ///
    /// See [`validate`](Self::validate).
    pub fn build(&self) -> Result<String, BuildError> {
        self.validate()?;
        let mut buf = String::with_capacity(700 + self.entry_count() * 220);
        // Writing into a String is infallible.
        let _ = self.write_xml_to(&mut buf);
        Ok(buf)
    }

    /// Validate and stream the camt.055 XML to an [`io::Write`](std::io::Write).
    ///
    /// # Errors
    ///
    /// [`WriteError::Validation`] or [`WriteError::Io`].
    pub fn write_to<W: std::io::Write>(&self, w: &mut W) -> Result<(), WriteError> {
        self.validate()?;
        let mut bridge = crate::xml_util::IoWriterBridge {
            inner: w,
            error: None,
        };
        if self.write_xml_to(&mut bridge).is_err() {
            return Err(WriteError::Io(bridge.error.unwrap_or_else(|| {
                std::io::Error::other("XML serialisation failed")
            })));
        }
        Ok(())
    }

    fn write_xml_to<W: std::fmt::Write>(&self, w: &mut W) -> std::fmt::Result {
        let now = self.created_at.unwrap_or_else(IsoDateTime::now);

        w.write_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n")?;
        writeln!(w, "<Document xmlns=\"{NAMESPACE}\">")?;
        w.write_str("  <CstmrPmtCxlReq>\n    <Assgnmt><Id>")?;
        write_escaped(w, &self.assignment_id)?;
        w.write_str("</Id>")?;
        self.assigner.write_xml(w, "Assgnr", self.charset)?;
        self.assignee.write_xml(w, "Assgne", self.charset)?;
        writeln!(w, "<CreDtTm>{now}</CreDtTm></Assgnmt>")?;

        if let Some(id) = &self.case_id {
            w.write_str("    <Case><Id>")?;
            write_escaped(w, id)?;
            w.write_str("</Id>")?;
            self.assigner.write_xml(w, "Cretr", self.charset)?;
            w.write_str("</Case>\n")?;
        }

        // `CtrlData` counts what this message lists. A whole-message or
        // whole-group recall lists nothing, so the element is omitted rather
        // than asserting a zero.
        if self.entry_count() > 0 {
            write!(w, "    <CtrlData><NbOfTxs>{}</NbOfTxs>", self.entry_count())?;
            if let Some(total) = self.total_ct() {
                write!(w, "<CtrlSum>{}</CtrlSum>", ct_to_eur_str(total))?;
            }
            w.write_str("</CtrlData>\n")?;
        }

        w.write_str("    <Undrlyg>\n")?;
        if let Some(reason) = &self.whole_message {
            w.write_str("      <OrgnlGrpInfAndCxl>")?;
            self.original.write_reference(w)?;
            if let Some(n) = self.original.number_of_transactions {
                write!(w, "<NbOfTxs>{n}</NbOfTxs>")?;
            }
            if let Some(ct) = self.original.control_sum_ct {
                write!(w, "<CtrlSum>{}</CtrlSum>", ct_to_eur_str(ct))?;
            }
            w.write_str("<GrpCxl>true</GrpCxl>")?;
            write_reason(w, reason, self.whole_message_info.as_deref(), self.charset)?;
            w.write_str("</OrgnlGrpInfAndCxl>\n")?;
        }
        for g in &self.groups {
            self.write_group(w, g)?;
        }
        w.write_str("    </Undrlyg>\n  </CstmrPmtCxlReq>\n</Document>")
    }

    fn write_group<W: std::fmt::Write>(
        &self,
        w: &mut W,
        g: &CancellationGroup,
    ) -> std::fmt::Result {
        // XSD sequence: PmtCxlId, Case, OrgnlPmtInfId, OrgnlGrpInf, NbOfTxs,
        // CtrlSum, PmtInfCxl, CxlRsnInf, TxInf.
        w.write_str("      <OrgnlPmtInfAndCxl>")?;
        if let Some(id) = &g.payment_cancellation_id {
            w.write_str("<PmtCxlId>")?;
            write_escaped(w, id)?;
            w.write_str("</PmtCxlId>")?;
        }
        w.write_str("<OrgnlPmtInfId>")?;
        write_escaped(w, &g.original_payment_info_id)?;
        w.write_str("</OrgnlPmtInfId>")?;
        // `OrgnlGrpInf` is what names the submitted file when there is no
        // `OrgnlGrpInfAndCxl` beside these blocks — and there never is, since
        // the two scopes are alternatives.
        w.write_str("<OrgnlGrpInf>")?;
        self.original.write_reference(w)?;
        w.write_str("</OrgnlGrpInf>")?;
        if let Some(reason) = &g.whole_group {
            w.write_str("<PmtInfCxl>true</PmtInfCxl>")?;
            write_reason(w, reason, g.additional_info.as_deref(), self.charset)?;
            return w.write_str("</OrgnlPmtInfAndCxl>\n");
        }
        w.write_str("\n")?;
        for entry in &g.entries {
            entry.write_xml(w, self.charset)?;
        }
        w.write_str("      </OrgnlPmtInfAndCxl>\n")
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        DirectDebitEntry, DirectDebitGroup, Pain008Builder, validate_bic, validate_creditor_id,
        validate_iban,
    };

    fn d(s: &str) -> IsoDate {
        s.parse().unwrap()
    }
    fn iban() -> crate::Iban {
        validate_iban("DE89370400440532013000").unwrap()
    }
    fn bank() -> Bic {
        validate_bic("COBADEFFXXX").unwrap()
    }
    fn submitted() -> Pain008Builder {
        Pain008Builder::new("Stadtwerke GmbH", "DD-2026-07-001")
            .created_at("2026-07-15T09:00:00".parse().unwrap())
            .add_group(
                DirectDebitGroup::new(
                    "Stadtwerke GmbH",
                    &iban(),
                    &validate_creditor_id("DE98ZZZ09999999999").unwrap(),
                    d("2026-07-20"),
                )
                .payment_info_id("PMT-A")
                .add_entry(DirectDebitEntry::new(
                    "MND-1",
                    d("2024-06-01"),
                    "Max Mustermann",
                    iban(),
                    7_500,
                    "E2E-1",
                )),
            )
    }
    fn recall() -> Camt055Builder {
        Camt055Builder::new(
            "CXL-1",
            "Stadtwerke GmbH",
            bank(),
            OriginalMessage::from_direct_debit(&submitted()),
        )
        .created_at("2026-07-15T11:30:00".parse().unwrap())
    }

    #[test]
    fn the_document_has_the_cancellation_shape() {
        let xml = recall()
            .add_group(
                CancellationGroup::new("PMT-A")
                    .add_entry(CancellationEntry::new("E2E-1", CancellationReason::Dupl)),
            )
            .build()
            .unwrap();
        assert!(xml.contains(NAMESPACE));
        assert!(xml.contains("<CstmrPmtCxlReq>"));
        assert!(xml.contains("<Assgnmt><Id>CXL-1</Id>"));
        assert!(xml.contains("<Assgnr><Pty><Nm>Stadtwerke GmbH</Nm></Pty></Assgnr>"));
        assert!(xml.contains(
            "<Assgne><Agt><FinInstnId><BICFI>COBADEFFXXX</BICFI></FinInstnId></Agt></Assgne>"
        ));
        assert!(xml.contains("<CreDtTm>2026-07-15T11:30:00</CreDtTm>"));
        assert!(xml.contains("<OrgnlEndToEndId>E2E-1</OrgnlEndToEndId>"));
        assert!(xml.contains("<Cd>DUPL</Cd>"));
    }

    #[test]
    fn the_original_message_is_copied_from_the_builder_that_sent_it() {
        // Retyping `OrgnlMsgId` is how a recall comes to name a file that was
        // never submitted under that identifier.
        let original = OriginalMessage::from_direct_debit(&submitted());
        assert_eq!(original.message_id, "DD-2026-07-001");
        assert_eq!(original.message_name_id, "pain.008.001.08");
        assert_eq!(original.number_of_transactions, Some(1));
        assert_eq!(original.control_sum_ct, Some(7_500));
        assert_eq!(
            original.created_at.map(|t| t.to_string()).as_deref(),
            Some("2026-07-15T09:00:00")
        );

        let xml = Camt055Builder::new("CXL-1", "Stadtwerke GmbH", bank(), original)
            .cancel_whole_message(CancellationReason::Tech)
            .build()
            .unwrap();
        assert!(xml.contains("<OrgnlMsgId>DD-2026-07-001</OrgnlMsgId>"));
        assert!(xml.contains("<OrgnlMsgNmId>pain.008.001.08</OrgnlMsgNmId>"));
        assert!(xml.contains("<OrgnlCreDtTm>2026-07-15T09:00:00</OrgnlCreDtTm>"));
        assert!(xml.contains("<GrpCxl>true</GrpCxl>"));
    }

    #[test]
    fn an_unpinned_creation_timestamp_is_not_invented() {
        // `from_direct_debit` on a builder with no `created_at` must leave
        // `OrgnlCreDtTm` out rather than stamping "now" — the element names the
        // moment the *original* was created, and that moment is gone.
        let unpinned = Pain008Builder::new("Acme", "DD-9");
        assert_eq!(
            OriginalMessage::from_direct_debit(&unpinned).created_at,
            None
        );
        let xml = Camt055Builder::new(
            "CXL-9",
            "Acme",
            bank(),
            OriginalMessage::from_direct_debit(&unpinned),
        )
        .cancel_whole_message(CancellationReason::Cust)
        .build()
        .unwrap();
        assert!(!xml.contains("OrgnlCreDtTm"));
    }

    #[test]
    fn a_transaction_recall_copies_the_amount_and_date_it_is_matched_by() {
        let group = DirectDebitGroup::new(
            "Stadtwerke GmbH",
            &iban(),
            &validate_creditor_id("DE98ZZZ09999999999").unwrap(),
            d("2026-07-20"),
        );
        let entry = DirectDebitEntry::new("MND-1", d("2024-06-01"), "Max", iban(), 7_500, "E2E-1");
        let xml = recall()
            .add_group(CancellationGroup::new("PMT-A").add_entry(
                CancellationEntry::from_direct_debit(&group, &entry, CancellationReason::Frad),
            ))
            .build()
            .unwrap();
        assert!(xml.contains("<OrgnlInstdAmt Ccy=\"EUR\">75.00</OrgnlInstdAmt>"));
        assert!(xml.contains("<OrgnlReqdColltnDt>2026-07-20</OrgnlReqdColltnDt>"));
        assert!(
            !xml.contains("OrgnlReqdExctnDt"),
            "a collection has no execution date"
        );
        // CtrlData counts what the message lists, and totals what it states.
        assert!(xml.contains("<CtrlData><NbOfTxs>1</NbOfTxs><CtrlSum>75.00</CtrlSum></CtrlData>"));
    }

    #[test]
    fn a_whole_group_recall_lists_nothing_and_writes_no_control_data() {
        // `CtrlData/NbOfTxs` counts the transactions the message names. A
        // whole-group recall names none, and "0" would assert something false.
        let xml = recall()
            .add_group(CancellationGroup::new("PMT-A").cancel_whole_group(CancellationReason::Upay))
            .build()
            .unwrap();
        assert!(xml.contains("<PmtInfCxl>true</PmtInfCxl>"));
        assert!(!xml.contains("CtrlData"));
        assert!(!xml.contains("TxInf"));
        // The group block still names the file it belongs to.
        assert!(xml.contains("<OrgnlGrpInf><OrgnlMsgId>DD-2026-07-001</OrgnlMsgId>"));
    }

    #[test]
    fn the_three_scopes_are_alternatives() {
        // "Cancel the whole message, and also these two" is schema-valid and
        // means nothing, which is exactly the class this crate refuses to emit.
        let both = recall()
            .cancel_whole_message(CancellationReason::Tech)
            .add_group(
                CancellationGroup::new("PMT-A")
                    .add_entry(CancellationEntry::new("E2E-1", CancellationReason::Tech)),
            )
            .build()
            .unwrap_err();
        assert!(matches!(
            both.kind,
            ValidationError::MutuallyExclusive {
                field: "Undrlyg",
                ..
            }
        ));

        let group_both = recall()
            .add_group(
                CancellationGroup::new("PMT-A")
                    .cancel_whole_group(CancellationReason::Tech)
                    .add_entry(CancellationEntry::new("E2E-1", CancellationReason::Tech)),
            )
            .build()
            .unwrap_err();
        assert_eq!(group_both.location, Location::group(0));
        assert!(matches!(
            group_both.kind,
            ValidationError::MutuallyExclusive {
                field: "OrgnlPmtInfAndCxl",
                ..
            }
        ));
    }

    #[test]
    fn free_text_with_no_block_to_live_in_is_rejected_not_dropped() {
        // `CxlRsnInf` hangs off the whole-message and whole-group blocks. Text
        // set without one had nowhere to be written, and silently vanished —
        // which a caller would only discover from a diff of its own output.
        let err = recall()
            .additional_info("Fehlerhafter Lauf")
            .add_group(
                CancellationGroup::new("PMT-A")
                    .add_entry(CancellationEntry::new("E2E-1", CancellationReason::Dupl)),
            )
            .build()
            .unwrap_err();
        assert!(matches!(
            err.kind,
            ValidationError::Requires {
                feature: "OrgnlGrpInfAndCxl/CxlRsnInf/AddtlInf",
                ..
            }
        ));

        let err = recall()
            .add_group(
                CancellationGroup::new("PMT-A")
                    .additional_info("Lauf zurueckgezogen")
                    .add_entry(CancellationEntry::new("E2E-1", CancellationReason::Dupl)),
            )
            .build()
            .unwrap_err();
        assert_eq!(err.location, Location::group(0));
        assert!(matches!(
            err.kind,
            ValidationError::Requires {
                feature: "OrgnlPmtInfAndCxl/CxlRsnInf/AddtlInf",
                ..
            }
        ));

        // Per-transaction free text is where it belongs, and is written.
        let xml = recall()
            .add_group(
                CancellationGroup::new("PMT-A").add_entry(
                    CancellationEntry::new("E2E-1", CancellationReason::Dupl)
                        .additional_info("Doppelte Einreichung"),
                ),
            )
            .build()
            .unwrap();
        assert!(xml.contains("<AddtlInf>Doppelte Einreichung</AddtlInf>"));
    }

    #[test]
    fn a_request_that_cancels_nothing_is_rejected() {
        assert!(matches!(
            recall().build().unwrap_err().kind,
            ValidationError::EmptyBatch
        ));
        // A group naming neither a whole-group cancellation nor a transaction
        // is schema-valid and inert.
        let err = recall()
            .add_group(CancellationGroup::new("PMT-A"))
            .build()
            .unwrap_err();
        assert_eq!(err.location, Location::group(0));
        assert!(matches!(err.kind, ValidationError::EmptyBatch));
    }

    #[test]
    fn two_groups_may_not_name_one_submitted_group() {
        let err = recall()
            .add_group(
                CancellationGroup::new("PMT-A")
                    .add_entry(CancellationEntry::new("E2E-1", CancellationReason::Dupl)),
            )
            .add_group(
                CancellationGroup::new("PMT-A")
                    .add_entry(CancellationEntry::new("E2E-2", CancellationReason::Dupl)),
            )
            .build()
            .unwrap_err();
        assert!(matches!(
            err.kind,
            ValidationError::Duplicate {
                field: "OrgnlPmtInfAndCxl/OrgnlPmtInfId",
                ..
            }
        ));
    }

    #[test]
    fn a_proprietary_reason_goes_to_prtry_not_to_cd() {
        // `CancellationReason5Code` is a closed enumeration — unlike the purpose
        // lists — so an unrecognised code in `Cd` is schema-invalid. The choice
        // has a `Prtry` branch for exactly this.
        let reason: CancellationReason = "XY99".parse().unwrap();
        assert!(!reason.is_iso_code());
        let xml = recall()
            .add_group(
                CancellationGroup::new("PMT-A").add_entry(CancellationEntry::new("E2E-1", reason)),
            )
            .build()
            .unwrap();
        assert!(xml.contains("<Rsn><Prtry>XY99</Prtry></Rsn>"));
        assert!(!xml.contains("<Cd>XY99</Cd>"));
    }

    #[test]
    fn additional_info_is_capped_at_max105text_and_transliterated() {
        let ok = recall()
            .add_group(
                CancellationGroup::new("PMT-A").add_entry(
                    CancellationEntry::new("E2E-1", CancellationReason::Cust)
                        .additional_info("Rückruf für Müller"),
                ),
            )
            .build()
            .unwrap();
        assert!(ok.contains("<AddtlInf>Rueckruf fuer Mueller</AddtlInf>"));

        let err = recall()
            .add_group(
                CancellationGroup::new("PMT-A").add_entry(
                    CancellationEntry::new("E2E-1", CancellationReason::Cust)
                        .additional_info("A".repeat(106)),
                ),
            )
            .build()
            .unwrap_err();
        assert!(matches!(
            err.kind,
            ValidationError::TooLong {
                field: "CxlRsnInf/AddtlInf",
                max: 105,
                actual: 106
            }
        ));
    }

    #[test]
    fn the_assignee_bic_is_held_to_the_pattern_this_schema_uses() {
        // camt.055.001.05 names its type `BICFI` but keeps the *pre-2019*
        // pattern, so a BIC with a digit in the prefix is invalid here even
        // though `pain.008.001.08` takes it.
        let modern = validate_bic("E097DEFF").unwrap();
        let err = Camt055Builder::new(
            "CXL-1",
            "Acme",
            modern,
            OriginalMessage::new("DD-1", "pain.008.001.08"),
        )
        .cancel_whole_message(CancellationReason::Tech)
        .build()
        .unwrap_err();
        assert!(matches!(
            err.kind,
            ValidationError::SchemaPattern {
                field: "Assgnmt/Assgne",
                schema: "camt.055.001.05",
                ..
            }
        ));
    }

    #[test]
    fn streaming_matches_the_in_memory_build() {
        let builder = recall().add_group(
            CancellationGroup::new("PMT-A")
                .add_entry(CancellationEntry::new("E2E-1", CancellationReason::Dupl)),
        );
        let mut streamed = Vec::new();
        builder.write_to(&mut streamed).unwrap();
        assert_eq!(
            String::from_utf8(streamed).unwrap(),
            builder.build().unwrap()
        );
    }

    #[test]
    fn reason_codes_round_trip_and_reject_malformed_custom_ones() {
        for code in [
            "DUPL", "AGNT", "CURR", "CUST", "UPAY", "CUTA", "TECH", "FRAD",
        ] {
            let parsed: CancellationReason = code.parse().unwrap();
            assert_eq!(parsed.as_code(), code);
            assert!(parsed.is_iso_code());
            assert!(parsed.validate("Rsn").is_ok());
        }
        assert_eq!(
            "fraud".parse::<CancellationReason>().unwrap(),
            CancellationReason::Frad
        );
        assert!(
            CancellationReason::Other("BAD CODE".to_owned())
                .validate("Rsn")
                .is_err()
        );
        assert!(
            CancellationReason::Other(String::new())
                .validate("Rsn")
                .is_err()
        );
    }
}
