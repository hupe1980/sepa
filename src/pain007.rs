//! ISO 20022 pain.007 — SEPA Direct Debit reversal.
//!
//! A **reversal** is the creditor undoing a collection it already sent: the
//! money goes back to the debtor. It is the creditor's own correction, and is
//! the opposite end of the lifecycle from the debtor-side events —
//! a *refund* (the debtor claims the money back, reported to you as a return in
//! [`camt.054`](crate::camt054)) and a *reject* (the collection never settled,
//! reported in [`pain.002`](crate::pain002)).
//!
//! ```text
//! pain.008  collection sent ──▶ settled
//!                               │
//!                               ├─ creditor was wrong  ──▶ pain.007   (this module)
//!                               ├─ debtor claims back  ──▶ camt.054 return
//!                               └─ never settled       ──▶ pain.002 RJCT
//! ```
//!
//! Only `pain.007.001.09` is defined for SEPA customer-to-PSP reversal, so
//! there is no schema-version choice here — unlike [`pain001`](crate::pain001)
//! and [`pain008`](crate::pain008), which each have three.
//!
//! ## Referring to the original collection
//!
//! ISO says a reversal may refer to the original *"by means of references only
//! or by means of references and a set of elements from the original
//! instruction"* — but the DK's technical validation subset makes `OrgnlTxRef`
//! and the mandate inside it **mandatory**, so the references-only form is not
//! one a German bank accepts. [`OriginalCollection`] is therefore required.
//!
//! | Constructor | Use when |
//! |---|---|
//! | [`ReversalEntry::reverse`] | you still have the [`DirectDebitGroup`] and [`DirectDebitEntry`] you sent |
//! | [`ReversalEntry::new`] | you are rebuilding the reference from stored data |
//!
//! Prefer the first. It copies the mandate, creditor identifier, scheme,
//! sequence type, collection date and both parties out of the objects you
//! already built, so the reversal cannot disagree with the collection it
//! reverses.
//!
//! ## References
//!
//! - ISO 20022 `pain.007.001.09`, validated against the DK GBIC 4 subset in CI
//! - EPC130-08 SDD Core Customer-to-PSP Implementation Guidelines, 2025 version
//!
//! ## Example
//!
//! ```rust
//! use sepa::{
//!     DirectDebitEntry, DirectDebitGroup, IsoDate, Pain007Builder, ReversalEntry,
//!     ReversalGroup, ReversalReason, validate_creditor_id, validate_iban,
//! };
//!
//! let creditor = validate_iban("DE89370400440532013000")?;
//! let debtor = validate_iban("NL91ABNA0417164300")?;
//! let ci = validate_creditor_id("DE98ZZZ09999999999")?;
//!
//! // The collection that went out last week.
//! let group = DirectDebitGroup::new("Stadtwerke GmbH", &creditor, &ci, IsoDate::new(2026, 7, 20)?);
//! let entry = DirectDebitEntry::new(
//!     "MND-42", "2024-06-01".parse()?, "Max Mustermann", debtor, 7_500, "E2E-1",
//! );
//!
//! // …was collected in error. Send it back.
//! let xml = Pain007Builder::new("Stadtwerke GmbH", "DD-2026-07-001", "RVSL-2026-07-001")
//!     .add_group(
//!         ReversalGroup::new("DD-2026-07-001").add_entry(ReversalEntry::reverse(
//!             &group,
//!             &entry,
//!             ReversalReason::Ms02,
//!         )),
//!     )
//!     .build()?;
//!
//! assert!(xml.contains("<CstmrPmtRvsl>"));
//! assert!(xml.contains("<OrgnlEndToEndId>E2E-1</OrgnlEndToEndId>"));
//! assert!(xml.contains("<RvsdInstdAmt Ccy=\"EUR\">75.00</RvsdInstdAmt>"));
//! assert!(xml.contains("<Cd>MS02</Cd>"));
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use std::str::FromStr;

use crate::creditor_id::CreditorId;
use crate::date::IsoDate;
use crate::pain008::{DirectDebitEntry, DirectDebitGroup, DirectDebitScheme, SequenceType};
use crate::validate::{
    BuildError, CharsetPolicy, Locate, Location, MAX_ID_LEN, ValidationError, WriteError,
    check_amount, check_id, check_name, truncate_chars,
};
use crate::xml_util::{write_escaped, write_eur};
use crate::{Bic, Iban, IsoDateTime, ct_to_eur_str};

/// The one ISO namespace SEPA defines for a customer-to-PSP reversal.
pub const NAMESPACE: &str = "urn:iso:std:iso:20022:tech:xsd:pain.007.001.09";

/// The ISO 20022 message identifier this builder emits.
pub const MESSAGE_ID: &str = "pain.007.001.09";

// ── ReversalReason ────────────────────────────────────────────────────────────

/// Why a collection is being reversed (`RvslRsnInf/Rsn/Cd`).
///
/// ISO's `ExternalReversalReason1Code` is an external code set revised
/// quarterly, so unrecognised-but-well-formed codes are carried through as
/// [`Other`](Self::Other) rather than rejected.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ReversalReason {
    /// `MS02` — no reason specified by the customer.
    ///
    /// The catch-all a creditor uses when it simply collected in error, and the
    /// code the DK's own reversal example carries.
    Ms02,
    /// `MS03` — no reason specified by the agent.
    Ms03,
    /// `AM05` — duplicate collection.
    Am05,
    /// `DUPL` — duplicate payment.
    Dupl,
    /// `CUST` — reversal requested by the customer.
    Cust,
    /// `FRAD` — fraudulent original collection.
    Frad,
    /// `TECH` — technical problem with the original collection.
    Tech,
    /// `AC04` — the debtor's account is closed.
    Ac04,
    /// A code not listed here.
    Other(String),
}

impl ReversalReason {
    /// The ISO 20022 code string.
    #[must_use]
    pub fn as_code(&self) -> &str {
        match self {
            Self::Ms02 => "MS02",
            Self::Ms03 => "MS03",
            Self::Am05 => "AM05",
            Self::Dupl => "DUPL",
            Self::Cust => "CUST",
            Self::Frad => "FRAD",
            Self::Tech => "TECH",
            Self::Ac04 => "AC04",
            Self::Other(code) => code,
        }
    }

    /// Validate the code's shape: 1–4 ASCII alphanumerics.
    ///
    /// # Errors
    ///
    /// [`ValidationError`] for a malformed hand-built [`Other`](Self::Other).
    pub fn validate(&self, field: &'static str) -> Result<(), ValidationError> {
        let code = self.as_code();
        let len = code.chars().count();
        if code.trim().is_empty() {
            return Err(ValidationError::Empty { field });
        }
        if len > 4 {
            return Err(ValidationError::TooLong {
                field,
                max: 4,
                actual: len,
            });
        }
        match code.chars().find(|c| !c.is_ascii_alphanumeric()) {
            Some(ch) => Err(ValidationError::InvalidCharacter { field, ch }),
            None => Ok(()),
        }
    }
}

impl std::fmt::Display for ReversalReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_code())
    }
}

impl FromStr for ReversalReason {
    type Err = std::convert::Infallible;
    /// Always succeeds — unknown codes become `Other(code)`.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.trim().to_ascii_uppercase().as_str() {
            "MS02" => Self::Ms02,
            "MS03" => Self::Ms03,
            "AM05" => Self::Am05,
            "DUPL" => Self::Dupl,
            "CUST" => Self::Cust,
            "FRAD" => Self::Frad,
            "TECH" => Self::Tech,
            "AC04" => Self::Ac04,
            other => Self::Other(other.to_owned()),
        })
    }
}

// ── OriginalCollection ────────────────────────────────────────────────────────

/// The `OrgnlTxRef` block — the collection being reversed.
///
/// The DK's technical validation subset makes this **mandatory** on every
/// reversal, together with the mandate reference and signature date inside it.
/// Plain ISO permits a reversal that carries references only; a German bank
/// does not, so this type is required rather than optional.
///
/// Fields are set through the builder methods rather than directly, because
/// several of them are all-or-nothing: `PmtTpInf` needs the scheme *and* the
/// sequence type, and a party needs a name *and* an account. Constructing them
/// in pairs makes a half-filled block unrepresentable.
///
/// Easiest is not to build one at all — [`ReversalEntry::reverse`] fills it
/// from the [`DirectDebitGroup`] and [`DirectDebitEntry`] you sent.
///
/// # Examples
///
/// ```
/// use sepa::{OriginalCollection, validate_iban};
///
/// let original = OriginalCollection::new("MND-42", "2024-06-01".parse()?)
///     .collection_date("2026-07-20".parse()?)
///     .debtor("Max Mustermann", validate_iban("NL91ABNA0417164300")?);
/// assert_eq!(original.mandate_ref(), "MND-42");
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct OriginalCollection {
    mandate_ref: String,
    mandate_signed_at: IsoDate,
    collection_date: Option<IsoDate>,
    creditor_id: Option<CreditorId>,
    payment_type: Option<(DirectDebitScheme, SequenceType)>,
    debtor: Option<(String, Iban)>,
    debtor_bic: Option<Bic>,
    creditor: Option<(String, Iban)>,
    creditor_bic: Option<Bic>,
}

impl OriginalCollection {
    /// The mandate the reversed collection ran under.
    ///
    /// Both arguments are required because the DK subset makes `MndtId` and
    /// `DtOfSgntr` mandatory inside `MndtRltdInf`, which is itself mandatory.
    pub fn new(mandate_ref: impl Into<String>, mandate_signed_at: IsoDate) -> Self {
        Self {
            mandate_ref: mandate_ref.into(),
            mandate_signed_at,
            collection_date: None,
            creditor_id: None,
            payment_type: None,
            debtor: None,
            debtor_bic: None,
            creditor: None,
            creditor_bic: None,
        }
    }

    /// Set `ReqdColltnDt` — when the original collection was due.
    #[must_use]
    pub fn collection_date(mut self, date: IsoDate) -> Self {
        self.collection_date = Some(date);
        self
    }

    /// Set `CdtrSchmeId` — the Creditor Identifier the collection ran under.
    #[must_use]
    pub fn creditor_id(mut self, creditor_id: CreditorId) -> Self {
        self.creditor_id = Some(creditor_id);
        self
    }

    /// Set `PmtTpInf` — the scheme and sequence type of the original.
    ///
    /// Both together: the DK subset makes `SvcLvl`, `LclInstrm` and `SeqTp` all
    /// mandatory once `PmtTpInf` is present, so a half-filled block would be
    /// schema-invalid.
    #[must_use]
    pub fn payment_type(mut self, scheme: DirectDebitScheme, sequence: SequenceType) -> Self {
        self.payment_type = Some((scheme, sequence));
        self
    }

    /// Set `Dbtr` and `DbtrAcct` — who was debited, and from where.
    #[must_use]
    pub fn debtor(mut self, name: impl Into<String>, iban: Iban) -> Self {
        self.debtor = Some((name.into(), iban));
        self
    }

    /// Set `DbtrAgt` — the debtor's bank.
    #[must_use]
    pub fn debtor_agent(mut self, bic: Bic) -> Self {
        self.debtor_bic = Some(bic);
        self
    }

    /// Set `Cdtr` and `CdtrAcct` — who collected, and to where.
    #[must_use]
    pub fn creditor(mut self, name: impl Into<String>, iban: Iban) -> Self {
        self.creditor = Some((name.into(), iban));
        self
    }

    /// Set `CdtrAgt` — the creditor's bank.
    #[must_use]
    pub fn creditor_agent(mut self, bic: Bic) -> Self {
        self.creditor_bic = Some(bic);
        self
    }

    /// The mandate reference (`MndtId`).
    #[must_use]
    pub fn mandate_ref(&self) -> &str {
        &self.mandate_ref
    }

    /// When the mandate was signed (`DtOfSgntr`).
    #[must_use]
    pub const fn mandate_signed_at(&self) -> IsoDate {
        self.mandate_signed_at
    }

    /// Validate the copied fields against the rules the collection itself met.
    fn validate(&self, charset: CharsetPolicy) -> Result<(), ValidationError> {
        check_id("MndtRltdInf/MndtId", &self.mandate_ref)?;
        if let Some((name, _)) = &self.debtor {
            check_name("Dbtr/Nm", &charset.apply("Dbtr/Nm", name)?)?;
        }
        if let Some((name, _)) = &self.creditor {
            check_name("Cdtr/Nm", &charset.apply("Cdtr/Nm", name)?)?;
        }
        Ok(())
    }

    /// Write `OrgnlTxRef` in XSD sequence order.
    fn write_xml<W: std::fmt::Write>(&self, w: &mut W, charset: CharsetPolicy) -> std::fmt::Result {
        /// `Party40Choice` wraps the party in `Pty` — unlike pain.008, where
        /// `Dbtr` is a `PartyIdentification` directly.
        fn party<W: std::fmt::Write>(
            w: &mut W,
            tag: &'static str,
            name: &str,
            iban: &Iban,
            charset: CharsetPolicy,
        ) -> std::fmt::Result {
            write!(w, "          <{tag}><Pty><Nm>")?;
            write_escaped(w, &charset.render(name))?;
            writeln!(w, "</Nm></Pty></{tag}>")?;
            writeln!(
                w,
                "          <{tag}Acct><Id><IBAN>{}</IBAN></Id></{tag}Acct>",
                iban.as_str()
            )
        }

        w.write_str("        <OrgnlTxRef>\n")?;
        if let Some(date) = self.collection_date {
            writeln!(w, "          <ReqdColltnDt>{date}</ReqdColltnDt>")?;
        }
        if let Some(ci) = &self.creditor_id {
            w.write_str("          <CdtrSchmeId><Id><PrvtId><Othr><Id>")?;
            w.write_str(ci.as_str())?;
            w.write_str(
                "</Id><SchmeNm><Prtry>SEPA</Prtry></SchmeNm></Othr></PrvtId></Id></CdtrSchmeId>\n",
            )?;
        }
        if let Some((scheme, sequence)) = self.payment_type {
            writeln!(
                w,
                "          <PmtTpInf><SvcLvl><Cd>SEPA</Cd></SvcLvl>\
                 <LclInstrm><Cd>{}</Cd></LclInstrm><SeqTp>{}</SeqTp></PmtTpInf>",
                scheme.as_code(),
                sequence.as_code()
            )?;
        }
        w.write_str("          <MndtRltdInf><MndtId>")?;
        write_escaped(w, &self.mandate_ref)?;
        writeln!(
            w,
            "</MndtId><DtOfSgntr>{}</DtOfSgntr><AmdmntInd>false</AmdmntInd></MndtRltdInf>",
            self.mandate_signed_at
        )?;
        if let Some((name, iban)) = &self.debtor {
            party(w, "Dbtr", name, iban, charset)?;
        }
        if let Some(bic) = &self.debtor_bic {
            writeln!(
                w,
                "          <DbtrAgt><FinInstnId><BICFI>{}</BICFI></FinInstnId></DbtrAgt>",
                bic.as_str()
            )?;
        }
        if let Some(bic) = &self.creditor_bic {
            writeln!(
                w,
                "          <CdtrAgt><FinInstnId><BICFI>{}</BICFI></FinInstnId></CdtrAgt>",
                bic.as_str()
            )?;
        }
        if let Some((name, iban)) = &self.creditor {
            party(w, "Cdtr", name, iban, charset)?;
        }
        w.write_str("        </OrgnlTxRef>\n")
    }
}

// ── ReversalEntry ─────────────────────────────────────────────────────────────

/// One reversed collection (`TxInf`).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ReversalEntry {
    /// `OrgnlEndToEndId` — the reference of the collection being reversed.
    pub original_end_to_end_id: String,
    /// `OrgnlInstdAmt` — what was collected, in **ct**.
    pub original_amount_ct: i64,
    /// `RvsdInstdAmt` — how much is going back, in **ct**.
    ///
    /// `None` reverses the whole collection, which is the usual case.
    pub reversed_amount_ct: Option<i64>,
    /// `RvslRsnInf/Rsn/Cd` — why.
    pub reason: ReversalReason,
    /// `OrgnlTxRef` — the collection being reversed.
    ///
    /// Mandatory: plain ISO allows a reversal that carries references only, but
    /// the DK subset requires this block, so a reference-only reversal is not
    /// something a German bank accepts.
    pub original: OriginalCollection,
}

impl ReversalEntry {
    /// Reverse a collection you describe by hand.
    ///
    /// Use this when the original [`DirectDebitEntry`] is no longer to hand —
    /// a reversal sent days later, from a database row rather than a live
    /// object. [`reverse`](Self::reverse) is preferable whenever the original
    /// objects still exist, because it cannot get the copy wrong.
    pub fn new(
        original_end_to_end_id: impl Into<String>,
        original_amount_ct: i64,
        reason: ReversalReason,
        original: OriginalCollection,
    ) -> Self {
        Self {
            original_end_to_end_id: original_end_to_end_id.into(),
            original_amount_ct,
            reversed_amount_ct: None,
            reason,
            original,
        }
    }

    /// Reverse a collection, copying its details out of the objects that
    /// produced it.
    ///
    /// This is the constructor to reach for: the mandate, creditor identifier,
    /// scheme, sequence type, collection date and both parties come straight
    /// from the [`DirectDebitGroup`] and [`DirectDebitEntry`] you sent, so the
    /// `OrgnlTxRef` cannot disagree with the collection it describes.
    #[must_use]
    pub fn reverse(
        group: &DirectDebitGroup,
        entry: &DirectDebitEntry,
        reason: ReversalReason,
    ) -> Self {
        Self {
            original_end_to_end_id: entry.end_to_end_id.clone(),
            original_amount_ct: entry.amount_ct,
            reversed_amount_ct: None,
            reason,
            original: {
                let mut original =
                    OriginalCollection::new(&entry.mandate_ref, entry.mandate_signed_at)
                        .collection_date(group.collection_date)
                        .creditor_id(group.creditor_id.clone())
                        .payment_type(group.scheme, group.sequence_type)
                        .debtor(&entry.debtor_name, entry.debtor_iban.clone())
                        .creditor(&group.creditor_name, group.creditor_iban.clone());
                if let Some(bic) = entry.debtor_bic.clone() {
                    original = original.debtor_agent(bic);
                }
                if let Some(bic) = group.creditor_bic.clone() {
                    original = original.creditor_agent(bic);
                }
                original
            },
        }
    }

    /// Reverse only part of the collection.
    ///
    /// Defaults to the full original amount. A partial reversal must be
    /// strictly less than what was collected — `build()` rejects more.
    #[must_use]
    pub fn reversed_amount(mut self, amount_ct: i64) -> Self {
        self.reversed_amount_ct = Some(amount_ct);
        self
    }

    /// The amount actually going back, in **ct**.
    #[must_use]
    pub const fn effective_amount_ct(&self) -> i64 {
        match self.reversed_amount_ct {
            Some(amount) => amount,
            None => self.original_amount_ct,
        }
    }

    fn validate(&self, charset: CharsetPolicy) -> Result<(), ValidationError> {
        check_id("TxInf/OrgnlEndToEndId", &self.original_end_to_end_id)?;
        check_amount("TxInf/OrgnlInstdAmt", self.original_amount_ct)?;
        check_amount("TxInf/RvsdInstdAmt", self.effective_amount_ct())?;
        // Reversing more than was collected would credit the debtor money the
        // creditor never took.
        if self.effective_amount_ct() > self.original_amount_ct {
            return Err(ValidationError::AmountOutOfRange {
                field: "TxInf/RvsdInstdAmt",
                amount_ct: self.effective_amount_ct(),
            });
        }
        self.reason.validate("RvslRsnInf/Rsn/Cd")?;
        self.original.validate(charset)?;
        Ok(())
    }

    fn write_xml<W: std::fmt::Write>(&self, w: &mut W, charset: CharsetPolicy) -> std::fmt::Result {
        w.write_str("      <TxInf>\n        <OrgnlEndToEndId>")?;
        write_escaped(w, &self.original_end_to_end_id)?;
        w.write_str("</OrgnlEndToEndId>\n        <OrgnlInstdAmt Ccy=\"EUR\">")?;
        write_eur(w, self.original_amount_ct)?;
        w.write_str("</OrgnlInstdAmt>\n        <RvsdInstdAmt Ccy=\"EUR\">")?;
        write_eur(w, self.effective_amount_ct())?;
        w.write_str("</RvsdInstdAmt>\n        <ChrgBr>SLEV</ChrgBr>\n")?;
        writeln!(
            w,
            "        <RvslRsnInf><Rsn><Cd>{}</Cd></Rsn></RvslRsnInf>",
            self.reason.as_code()
        )?;
        self.original.write_xml(w, charset)?;
        w.write_str("      </TxInf>\n")
    }
}

// ── ReversalGroup ─────────────────────────────────────────────────────────────

/// One `OrgnlPmtInfAndRvsl` block — the reversals belonging to a single
/// `PmtInf` of the original collection file.
///
/// A pain.008 message may carry several groups, and a reversal keeps that
/// structure: each group here names the `PmtInfId` it reverses out of.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ReversalGroup {
    original_payment_info_id: String,
    reversal_payment_info_id: Option<String>,
    batch_booking: Option<bool>,
    entries: Vec<ReversalEntry>,
}

impl ReversalGroup {
    /// Reverse out of the `PmtInf` identified by `original_payment_info_id`.
    ///
    /// That is the `PmtInfId` the original pain.008 carried — for a batch built
    /// by [`Pain008Builder`](crate::Pain008Builder) with a single group it is
    /// the message's own `MsgId`, and `MsgId-<n>` when there were several.
    pub fn new(original_payment_info_id: impl Into<String>) -> Self {
        Self {
            original_payment_info_id: original_payment_info_id.into(),
            reversal_payment_info_id: None,
            batch_booking: None,
            entries: Vec::new(),
        }
    }

    /// Set `RvslPmtInfId` — this reversal group's own identifier.
    #[must_use]
    pub fn reversal_payment_info_id(mut self, id: impl Into<String>) -> Self {
        self.reversal_payment_info_id = Some(id.into());
        self
    }

    /// Request batch booking (`BtchBookg`) for the reversal.
    #[must_use]
    pub fn batch_booking(mut self, batch: bool) -> Self {
        self.batch_booking = Some(batch);
        self
    }

    /// Add a reversed collection.
    #[must_use]
    pub fn add_entry(mut self, entry: ReversalEntry) -> Self {
        self.entries.push(entry);
        self
    }

    /// Add several reversed collections.
    #[must_use]
    pub fn add_entries(mut self, entries: impl IntoIterator<Item = ReversalEntry>) -> Self {
        self.entries.extend(entries);
        self
    }

    /// Number of reversals in this group.
    #[must_use]
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// Total amount going back in this group, in ct, saturating on overflow.
    #[must_use]
    pub fn total_ct(&self) -> i64 {
        self.entries
            .iter()
            .fold(0i64, |acc, e| acc.saturating_add(e.effective_amount_ct()))
    }
}

// ── Builder ───────────────────────────────────────────────────────────────────

/// Builder for an ISO 20022 pain.007 (SEPA Direct Debit reversal) message.
#[derive(Debug, Clone)]
pub struct Pain007Builder {
    initiating_party: String,
    msg_id: String,
    created_at: Option<IsoDateTime>,
    creditor_agent: Option<Bic>,
    original_msg_id: String,
    original_msg_name_id: String,
    charset: CharsetPolicy,
    groups: Vec<ReversalGroup>,
}

impl Pain007Builder {
    /// A reversal of the message `original_msg_id`, sent by `initiating_party`
    /// and identified by `msg_id`.
    ///
    /// `original_msg_id` is the `GrpHdr/MsgId` of the pain.008 whose
    /// collections are being reversed; both identifiers are mandatory in the
    /// schema, which is why they are constructor arguments rather than setters.
    pub fn new(
        initiating_party: impl Into<String>,
        original_msg_id: impl Into<String>,
        msg_id: impl Into<String>,
    ) -> Self {
        Self {
            initiating_party: initiating_party.into(),
            msg_id: msg_id.into(),
            created_at: None,
            creditor_agent: None,
            original_msg_id: original_msg_id.into(),
            original_msg_name_id: crate::pain008::DirectDebitSchema::default()
                .message_id()
                .to_owned(),
            charset: CharsetPolicy::default(),
            groups: Vec::new(),
        }
    }

    /// Pin the creation timestamp (`CreDtTm`), making output reproducible.
    #[must_use]
    pub fn created_at(mut self, timestamp: IsoDateTime) -> Self {
        self.created_at = Some(timestamp);
        self
    }

    /// Set `OrgnlMsgNmId` — which pain.008 version the original was.
    ///
    /// Defaults to the current default direct debit schema
    /// (`pain.008.001.08`). Set it when the collection went out under an older
    /// version, because the bank matches on this.
    #[must_use]
    pub fn original_schema(mut self, schema: crate::pain008::DirectDebitSchema) -> Self {
        self.original_msg_name_id.clear();
        self.original_msg_name_id.push_str(schema.message_id());
        self
    }

    /// Set the creditor's BIC (`GrpHdr/CdtrAgt`).
    #[must_use]
    pub fn creditor_agent(mut self, bic: Bic) -> Self {
        self.creditor_agent = Some(bic);
        self
    }

    /// Set how text outside the SEPA character set is handled.
    #[must_use]
    pub fn charset(mut self, policy: CharsetPolicy) -> Self {
        self.charset = policy;
        self
    }

    /// Add a reversal group (`OrgnlPmtInfAndRvsl`).
    #[must_use]
    pub fn add_group(mut self, group: ReversalGroup) -> Self {
        self.groups.push(group);
        self
    }

    /// Number of reversal groups.
    #[must_use]
    pub fn group_count(&self) -> usize {
        self.groups.len()
    }

    /// Total number of reversals across every group — the `GrpHdr/NbOfTxs`.
    #[must_use]
    pub fn entry_count(&self) -> usize {
        self.groups.iter().map(ReversalGroup::entry_count).sum()
    }

    /// Total amount going back across every group, in ct, saturating.
    #[must_use]
    pub fn total_ct(&self) -> i64 {
        self.groups
            .iter()
            .fold(0i64, |acc, g| acc.saturating_add(g.total_ct()))
    }

    /// The effective `RvslPmtInfId` for group `index`, when one is emitted.
    fn reversal_payment_info_id(&self, index: usize) -> Option<String> {
        let explicit = self
            .groups
            .get(index)
            .and_then(|g| g.reversal_payment_info_id.clone());
        if explicit.is_some() {
            return explicit;
        }
        if self.groups.len() <= 1 {
            return Some(self.msg_id.clone());
        }
        let suffix = format!("-{}", index + 1);
        let keep = MAX_ID_LEN.saturating_sub(suffix.chars().count());
        Some(format!("{}{suffix}", truncate_chars(&self.msg_id, keep)))
    }

    /// Validate the message without producing XML.
    ///
    /// # Errors
    ///
    /// A [`BuildError`] naming both the broken rule and the group and
    /// transaction it belongs to.
    pub fn validate(&self) -> Result<(), BuildError> {
        if self.groups.is_empty() {
            return Err(BuildError::message(ValidationError::EmptyBatch));
        }
        let msg = Location::message();
        check_id("GrpHdr/MsgId", &self.msg_id).at(msg)?;
        check_id("OrgnlGrpInf/OrgnlMsgId", &self.original_msg_id).at(msg)?;
        check_name(
            "InitgPty/Nm",
            &self
                .charset
                .apply("InitgPty/Nm", &self.initiating_party)
                .at(msg)?,
        )
        .at(msg)?;

        let mut total: i64 = 0;
        // Both identifiers have to be unique across the message, for the same
        // reason `PmtInfId` does in pain.001 and pain.008: `OrgnlPmtInfId` is
        // what the bank matches a reversal back to, and two blocks naming one
        // group make the reversal unattributable. `RvslPmtInfId` is the key the
        // bank echoes in the pain.002 that answers this file.
        let mut seen_original = std::collections::BTreeSet::new();
        let mut seen_reversal = std::collections::BTreeSet::new();
        for (i, g) in self.groups.iter().enumerate() {
            let at = Location::group(i);
            if g.entries.is_empty() {
                return Err(BuildError::group(i, ValidationError::EmptyBatch));
            }
            check_id(
                "OrgnlPmtInfAndRvsl/OrgnlPmtInfId",
                &g.original_payment_info_id,
            )
            .at(at)?;
            if !seen_original.insert(g.original_payment_info_id.clone()) {
                return Err(BuildError::group(
                    i,
                    ValidationError::Duplicate {
                        field: "OrgnlPmtInfAndRvsl/OrgnlPmtInfId",
                        value: g.original_payment_info_id.clone(),
                    },
                ));
            }
            if let Some(id) = self.reversal_payment_info_id(i) {
                check_id("OrgnlPmtInfAndRvsl/RvslPmtInfId", &id).at(at)?;
                if !seen_reversal.insert(id.clone()) {
                    return Err(BuildError::group(
                        i,
                        ValidationError::Duplicate {
                            field: "OrgnlPmtInfAndRvsl/RvslPmtInfId",
                            value: id,
                        },
                    ));
                }
            }
            for (j, e) in g.entries.iter().enumerate() {
                let at = Location::transaction(i, j);
                e.validate(self.charset).at(at)?;
                total = total
                    .checked_add(e.effective_amount_ct())
                    .ok_or(BuildError {
                        location: at,
                        kind: ValidationError::ControlSumOverflow,
                    })?;
            }
        }
        Ok(())
    }

    /// Validate the message and generate the pain.007 XML.
    ///
    /// # Errors
    ///
    /// See [`validate`](Self::validate).
    pub fn build(&self) -> Result<String, BuildError> {
        self.validate()?;
        let mut buf = String::with_capacity(700 + self.entry_count() * 900);
        let _ = self.write_xml_to(&mut buf);
        Ok(buf)
    }

    /// Validate and stream the pain.007 XML to an [`io::Write`](std::io::Write).
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
        let initiating = self.charset.render(&self.initiating_party);

        w.write_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n")?;
        writeln!(w, "<Document xmlns=\"{NAMESPACE}\">")?;
        w.write_str("  <CstmrPmtRvsl>\n    <GrpHdr>\n      <MsgId>")?;
        write_escaped(w, &self.msg_id)?;
        write!(w, "</MsgId>\n      <CreDtTm>{now}</CreDtTm>\n")?;
        write!(
            w,
            "      <NbOfTxs>{}</NbOfTxs>\n      <CtrlSum>{}</CtrlSum>\n",
            self.entry_count(),
            ct_to_eur_str(self.total_ct())
        )?;
        w.write_str("      <InitgPty><Nm>")?;
        write_escaped(w, &initiating)?;
        w.write_str("</Nm></InitgPty>\n")?;
        if let Some(bic) = &self.creditor_agent {
            writeln!(
                w,
                "      <CdtrAgt><FinInstnId><BICFI>{}</BICFI></FinInstnId></CdtrAgt>",
                bic.as_str()
            )?;
        }
        w.write_str("    </GrpHdr>\n    <OrgnlGrpInf>\n      <OrgnlMsgId>")?;
        write_escaped(w, &self.original_msg_id)?;
        w.write_str("</OrgnlMsgId>\n      <OrgnlMsgNmId>")?;
        w.write_str(&self.original_msg_name_id)?;
        w.write_str("</OrgnlMsgNmId>\n    </OrgnlGrpInf>\n")?;

        for (i, g) in self.groups.iter().enumerate() {
            w.write_str("    <OrgnlPmtInfAndRvsl>\n")?;
            if let Some(id) = self.reversal_payment_info_id(i) {
                w.write_str("      <RvslPmtInfId>")?;
                write_escaped(w, &id)?;
                w.write_str("</RvslPmtInfId>\n")?;
            }
            w.write_str("      <OrgnlPmtInfId>")?;
            write_escaped(w, &g.original_payment_info_id)?;
            w.write_str("</OrgnlPmtInfId>\n")?;
            if let Some(batch) = g.batch_booking {
                writeln!(w, "      <BtchBookg>{batch}</BtchBookg>")?;
            }
            for entry in &g.entries {
                entry.write_xml(w, self.charset)?;
            }
            w.write_str("    </OrgnlPmtInfAndRvsl>\n")?;
        }

        w.write_str("  </CstmrPmtRvsl>\n</Document>")
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Pain008Builder, validate_bic, validate_creditor_id, validate_iban};

    fn creditor_iban() -> Iban {
        validate_iban("DE89370400440532013000").unwrap()
    }
    fn debtor_iban() -> Iban {
        validate_iban("NL91ABNA0417164300").unwrap()
    }
    fn ci() -> CreditorId {
        validate_creditor_id("DE98ZZZ09999999999").unwrap()
    }
    fn d(s: &str) -> IsoDate {
        s.parse().unwrap()
    }

    fn original_group() -> DirectDebitGroup {
        DirectDebitGroup::new("Stadtwerke GmbH", &creditor_iban(), &ci(), d("2026-07-20"))
            .sequence_type(SequenceType::Frst)
            .creditor_bic(validate_bic("COBADEFFXXX").unwrap())
    }
    fn original_entry() -> DirectDebitEntry {
        DirectDebitEntry::new(
            "MND-42",
            d("2024-06-01"),
            "Max Mustermann",
            debtor_iban(),
            7_500,
            "E2E-1",
        )
    }

    fn reversal() -> Pain007Builder {
        Pain007Builder::new("Stadtwerke GmbH", "DD-2026-07-001", "RVSL-001")
            .created_at("2026-07-25T10:00:00".parse().unwrap())
            .add_group(
                ReversalGroup::new("DD-2026-07-001").add_entry(ReversalEntry::reverse(
                    &original_group(),
                    &original_entry(),
                    ReversalReason::Ms02,
                )),
            )
    }

    #[test]
    fn the_document_has_the_reversal_shape() {
        let xml = reversal().build().unwrap();
        assert!(xml.contains("urn:iso:std:iso:20022:tech:xsd:pain.007.001.09"));
        assert!(xml.contains("<CstmrPmtRvsl>"));
        assert!(xml.contains("<MsgId>RVSL-001</MsgId>"));
        assert!(xml.contains("<OrgnlMsgId>DD-2026-07-001</OrgnlMsgId>"));
        assert!(xml.contains("<OrgnlMsgNmId>pain.008.001.08</OrgnlMsgNmId>"));
        assert!(xml.contains("<NbOfTxs>1</NbOfTxs>"));
        assert!(xml.contains("<CtrlSum>75.00</CtrlSum>"));
        assert!(xml.contains("<ChrgBr>SLEV</ChrgBr>"));
    }

    #[test]
    fn reversing_from_the_original_copies_every_identifying_field() {
        // The point of `reverse`: nothing is retyped, so the OrgnlTxRef cannot
        // disagree with the collection it describes.
        let xml = reversal().build().unwrap();
        assert!(xml.contains("<OrgnlEndToEndId>E2E-1</OrgnlEndToEndId>"));
        assert!(xml.contains("<OrgnlInstdAmt Ccy=\"EUR\">75.00</OrgnlInstdAmt>"));
        assert!(xml.contains("<RvsdInstdAmt Ccy=\"EUR\">75.00</RvsdInstdAmt>"));
        assert!(xml.contains("<ReqdColltnDt>2026-07-20</ReqdColltnDt>"));
        assert!(xml.contains("DE98ZZZ09999999999"));
        assert!(xml.contains("<LclInstrm><Cd>CORE</Cd></LclInstrm><SeqTp>FRST</SeqTp>"));
        assert!(xml.contains("<MndtId>MND-42</MndtId><DtOfSgntr>2024-06-01</DtOfSgntr>"));
        // Party40Choice wraps the party in Pty — unlike pain.008.
        assert!(xml.contains("<Dbtr><Pty><Nm>Max Mustermann</Nm></Pty></Dbtr>"));
        assert!(xml.contains("<Cdtr><Pty><Nm>Stadtwerke GmbH</Nm></Pty></Cdtr>"));
        assert!(xml.contains("<IBAN>NL91ABNA0417164300</IBAN>"));
        assert!(xml.contains("<BICFI>COBADEFFXXX</BICFI>"));
    }

    #[test]
    fn a_hand_built_reference_carries_the_mandate_the_dk_requires() {
        // The minimum the DK subset accepts: OrgnlTxRef present, with MndtId
        // and DtOfSgntr inside it. Everything else in the block is optional.
        let xml = Pain007Builder::new("Stadtwerke GmbH", "DD-1", "RVSL-REF")
            .add_group(ReversalGroup::new("DD-1").add_entry(ReversalEntry::new(
                "E2E-9",
                1_000,
                ReversalReason::Am05,
                OriginalCollection::new("MND-9", d("2024-01-01")),
            )))
            .build()
            .unwrap();
        assert!(xml.contains("<OrgnlEndToEndId>E2E-9</OrgnlEndToEndId>"));
        assert!(xml.contains("<Cd>AM05</Cd>"));
        assert!(
            xml.contains("<MndtRltdInf><MndtId>MND-9</MndtId><DtOfSgntr>2024-01-01</DtOfSgntr>")
        );
        // Nothing half-filled: no PmtTpInf without all three of its children.
        assert!(!xml.contains("PmtTpInf"));
        assert!(!xml.contains("<Dbtr>"));
    }

    #[test]
    fn a_partial_reversal_may_not_exceed_what_was_collected() {
        let build = |reversed| {
            Pain007Builder::new("Stadtwerke GmbH", "DD-1", "RVSL-PART")
                .add_group(
                    ReversalGroup::new("DD-1").add_entry(
                        ReversalEntry::new(
                            "E2E-1",
                            7_500,
                            ReversalReason::Ms02,
                            OriginalCollection::new("MND-1", d("2024-01-01")),
                        )
                        .reversed_amount(reversed),
                    ),
                )
                .build()
        };

        let xml = build(2_500).unwrap();
        assert!(xml.contains("<OrgnlInstdAmt Ccy=\"EUR\">75.00</OrgnlInstdAmt>"));
        assert!(xml.contains("<RvsdInstdAmt Ccy=\"EUR\">25.00</RvsdInstdAmt>"));

        // Reversing more than was collected would credit money never taken.
        assert_eq!(
            build(9_000).unwrap_err().kind,
            ValidationError::AmountOutOfRange {
                field: "TxInf/RvsdInstdAmt",
                amount_ct: 9_000,
            }
        );
        assert!(build(0).is_err());
    }

    #[test]
    fn two_groups_may_not_reverse_the_same_original_group() {
        // `OrgnlPmtInfId` is how the bank finds the collections being undone,
        // so two blocks naming one group leave the reversal unattributable —
        // the same rule `PmtInfId` gets in pain.001 and pain.008, which this
        // builder was missing.
        let err = Pain007Builder::new("Acme", "REV-1", "DD-ORIG")
            .add_group(ReversalGroup::new("PMT-1").add_entry(ReversalEntry::new(
                "E2E-1",
                5_000,
                ReversalReason::Ms02,
                OriginalCollection::new("MND-1", d("2024-01-01")),
            )))
            .add_group(ReversalGroup::new("PMT-1").add_entry(ReversalEntry::new(
                "E2E-2",
                5_000,
                ReversalReason::Ms02,
                OriginalCollection::new("MND-2", d("2024-01-01")),
            )))
            .build()
            .unwrap_err();
        assert_eq!(err.location, Location::group(1));
        assert!(matches!(
            err.kind,
            ValidationError::Duplicate {
                field: "OrgnlPmtInfAndRvsl/OrgnlPmtInfId",
                ..
            }
        ));
    }

    #[test]
    fn two_groups_may_not_share_a_reversal_id_either() {
        let err = Pain007Builder::new("Acme", "REV-1", "DD-ORIG")
            .add_group(
                ReversalGroup::new("PMT-1")
                    .reversal_payment_info_id("SAME")
                    .add_entry(ReversalEntry::new(
                        "E2E-1",
                        5_000,
                        ReversalReason::Ms02,
                        OriginalCollection::new("MND-1", d("2024-01-01")),
                    )),
            )
            .add_group(
                ReversalGroup::new("PMT-2")
                    .reversal_payment_info_id("SAME")
                    .add_entry(ReversalEntry::new(
                        "E2E-2",
                        5_000,
                        ReversalReason::Ms02,
                        OriginalCollection::new("MND-2", d("2024-01-01")),
                    )),
            )
            .build()
            .unwrap_err();
        assert!(matches!(
            err.kind,
            ValidationError::Duplicate {
                field: "OrgnlPmtInfAndRvsl/RvslPmtInfId",
                ..
            }
        ));
    }

    #[test]
    fn several_groups_get_distinct_reversal_ids_within_max35text() {
        let msg_id = "R".repeat(35);
        let b = (0..3).fold(
            Pain007Builder::new("Stadtwerke GmbH", "DD-1", &msg_id),
            |b, i| {
                b.add_group(
                    ReversalGroup::new(format!("DD-1-{i}")).add_entry(ReversalEntry::new(
                        "E2E",
                        100,
                        ReversalReason::Ms02,
                        OriginalCollection::new("MND-1", d("2024-01-01")),
                    )),
                )
            },
        );
        let xml = b.build().unwrap();
        let ids: Vec<&str> = xml
            .split("<RvslPmtInfId>")
            .skip(1)
            .map(|c| c.split('<').next().unwrap())
            .collect();
        assert_eq!(ids.len(), 3);
        for id in &ids {
            assert!(id.chars().count() <= 35, "{id} exceeds Max35Text");
        }
        assert_eq!(
            ids.iter().collect::<std::collections::BTreeSet<_>>().len(),
            3
        );
    }

    #[test]
    fn an_empty_reversal_is_rejected() {
        assert_eq!(
            Pain007Builder::new("Stadtwerke GmbH", "DD-1", "R").build(),
            Err(BuildError::message(ValidationError::EmptyBatch))
        );
        assert_eq!(
            Pain007Builder::new("Stadtwerke GmbH", "DD-1", "R")
                .add_group(ReversalGroup::new("DD-1"))
                .build(),
            Err(BuildError::group(0, ValidationError::EmptyBatch))
        );
    }

    #[test]
    fn errors_name_the_group_and_transaction() {
        let err = Pain007Builder::new("Stadtwerke GmbH", "DD-1", "RVSL-LOC")
            .add_group(
                ReversalGroup::new("DD-1")
                    .add_entry(ReversalEntry::new(
                        "E2E-1",
                        100,
                        ReversalReason::Ms02,
                        OriginalCollection::new("MND-1", d("2024-01-01")),
                    ))
                    .add_entry(ReversalEntry::new(
                        "E2E-2",
                        0,
                        ReversalReason::Ms02,
                        OriginalCollection::new("MND-2", d("2024-01-01")),
                    )),
            )
            .build()
            .unwrap_err();
        assert_eq!(err.location, Location::transaction(0, 1));
        assert!(err.to_string().starts_with("PmtInf[0]/Tx[1]: "));
    }

    #[test]
    fn reason_codes_round_trip_and_reject_malformed_custom_ones() {
        for code in ["MS02", "AM05", "DUPL", "FRAD"] {
            assert_eq!(code.parse::<ReversalReason>().unwrap().as_code(), code);
        }
        assert_eq!(
            "zzzz".parse::<ReversalReason>().unwrap(),
            ReversalReason::Other("ZZZZ".to_owned())
        );
        assert!(matches!(
            ReversalReason::Other("TOOLONG".to_owned()).validate("Rsn/Cd"),
            Err(ValidationError::TooLong { .. })
        ));
        assert!(matches!(
            ReversalReason::Other("A-B".to_owned()).validate("Rsn/Cd"),
            Err(ValidationError::InvalidCharacter { .. })
        ));
        assert_eq!(ReversalReason::Ms02.to_string(), "MS02");
    }

    #[test]
    fn text_is_transliterated_like_every_other_message() {
        let group =
            DirectDebitGroup::new("Müller & Söhne", &creditor_iban(), &ci(), d("2026-07-20"));
        let entry = DirectDebitEntry::new(
            "MND-1",
            d("2024-06-01"),
            "Jörg Groß",
            debtor_iban(),
            100,
            "E2E-1",
        );
        let xml = Pain007Builder::new("Müller & Söhne", "DD-1", "RVSL-UML")
            .add_group(ReversalGroup::new("DD-1").add_entry(ReversalEntry::reverse(
                &group,
                &entry,
                ReversalReason::Ms02,
            )))
            .build()
            .unwrap();
        assert!(xml.contains("Mueller + Soehne"));
        assert!(xml.contains("Joerg Gross"));
    }

    #[test]
    fn streaming_matches_the_in_memory_build() {
        let direct = reversal().build().unwrap();
        let mut buf: Vec<u8> = Vec::new();
        reversal().write_to(&mut buf).unwrap();
        assert_eq!(direct, String::from_utf8(buf).unwrap());

        let mut empty: Vec<u8> = Vec::new();
        assert!(
            Pain007Builder::new("X", "DD-1", "RVSL-EMPTY")
                .write_to(&mut empty)
                .is_err()
        );
        assert!(empty.is_empty());
    }

    #[test]
    fn the_original_schema_is_named_so_the_bank_can_match_it() {
        let xml = reversal()
            .original_schema(crate::pain008::DirectDebitSchema::IsoV2)
            .build()
            .unwrap();
        assert!(xml.contains("<OrgnlMsgNmId>pain.008.001.02</OrgnlMsgNmId>"));
    }

    #[test]
    fn a_reversal_round_trips_the_payment_info_id_a_collection_emitted() {
        // The reversal has to name the PmtInfId the collection actually used,
        // which for a single-group message is the MsgId.
        let collection = Pain008Builder::new("Stadtwerke GmbH", "DD-2026-07-001")
            .add_group(original_group().add_entry(original_entry()))
            .build()
            .unwrap();
        let pmt_inf_id = collection
            .split("<PmtInfId>")
            .nth(1)
            .unwrap()
            .split('<')
            .next()
            .unwrap();
        assert_eq!(pmt_inf_id, "DD-2026-07-001");

        let xml = Pain007Builder::new("Stadtwerke GmbH", "DD-2026-07-001", "RVSL-1")
            .add_group(
                ReversalGroup::new(pmt_inf_id).add_entry(ReversalEntry::reverse(
                    &original_group(),
                    &original_entry(),
                    ReversalReason::Ms02,
                )),
            )
            .build()
            .unwrap();
        assert!(xml.contains("<OrgnlPmtInfId>DD-2026-07-001</OrgnlPmtInfId>"));
    }
}
