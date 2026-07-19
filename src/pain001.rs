//! ISO 20022 pain.001 — SEPA Credit Transfer (SCT) initiation.
//!
//! Builds SEPA Credit Transfer XML for outgoing payments:
//! supplier credits, refunds to customers, or any IBAN-to-IBAN EUR transfer.
//! Supports both the DK legacy schema and the current EPC/SCT Inst schema.
//!
//! ## Schema versions
//!
//! | Schema | Namespace | Use |
//! |---|---|---|
//! | `pain.001.001.09` | `urn:iso:std:iso:20022:tech:xsd:pain.001.001.09` | Current SEPA version (**default**), required for SCT Inst |
//! | `pain.001.003.03` | `urn:iso:std:iso:20022:tech:xsd:pain.001.003.03` | Legacy DK V2.7, end-of-life since Nov 2022 |
//!
//! ## References
//!
//! - ISO 20022 pain.001.003.03 / pain.001.001.09 schemas
//! - EPC SEPA Credit Transfer Rulebook (SCT)
//! - EPC SEPA Instant Credit Transfer Rulebook (SCT Inst)
//! - Deutsche Kreditwirtschaft DFÜ-Abkommen V2.7
//!
//! ## Example
//!
//! ```rust
//! use sepa::{CreditTransferEntry, CreditTransferGroup, Pain001Builder, validate_iban};
//! use sepa::pain001::LocalInstrument;
//!
//! let debtor   = validate_iban("DE89370400440532013000")?;
//! let creditor = validate_iban("NL91ABNA0417164300")?;
//!
//! let xml = Pain001Builder::new("Acme GmbH")
//!     .msg_id("CT-2026-07-001")
//!     .add_group(
//!         CreditTransferGroup::new("Acme GmbH", &debtor)
//!             .execution_date("2026-07-20")
//!             .add_entry(
//!                 CreditTransferEntry::new("Max Mustermann", creditor.clone(), 12_000, "REFUND")
//!                     .with_description("Erstattung 2025"),
//!             ),
//!     )
//!     .build()?;
//!
//! assert!(xml.contains("<InstdAmt Ccy=\"EUR\">120.00</InstdAmt>"));
//! // pain.001.001.09 wraps the execution date in a <Dt> choice child.
//! assert!(xml.contains("<ReqdExctnDt><Dt>2026-07-20</Dt></ReqdExctnDt>"));
//!
//! // SCT Instant is a property of the group.
//! let inst = Pain001Builder::new("Acme GmbH")
//!     .msg_id("CT-INST-001")
//!     .add_group(
//!         CreditTransferGroup::new("Acme GmbH", &debtor)
//!             .local_instrument(LocalInstrument::Inst)
//!             .execution_date("2026-07-20")
//!             .add_entry(CreditTransferEntry::new("Max", creditor, 5_000, "INST-001")),
//!     )
//!     .build()?;
//! assert!(inst.contains("<Cd>INST</Cd>"));
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use crate::party::Party;
use crate::purpose::{CategoryPurpose, Purpose};
use crate::reference::RemittanceInfo;
use crate::validate::{
    CharsetPolicy, MAX_ID_LEN, ValidationError, WriteError, check_amount, check_date, check_id,
    check_name, check_remittance, truncate_chars,
};
use crate::{Bic, Iban, ct_to_eur_str};

// ── Schema version ────────────────────────────────────────────────────────────

/// pain.001 XML schema version to emit.
///
/// The two versions differ in ways that matter to the wire format, not just the
/// namespace — see [`CreditTransferSchema::namespace`] and the notes on each
/// variant.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CreditTransferSchema {
    /// `pain.001.001.09` — the current SEPA version (**default**).
    ///
    /// Mandated by the EPC 2023 SCT Rulebook with effect from 19 November 2023
    /// and carried unchanged into the 2025 rulebooks. Required for SCT Instant.
    ///
    /// Wire-format specifics versus [`DkV2_7`](Self::DkV2_7):
    /// - `ReqdExctnDt` is a `DateAndDateTime2Choice`, so the date is wrapped:
    ///   `<ReqdExctnDt><Dt>2026-07-20</Dt></ReqdExctnDt>`
    /// - the agent BIC element is named `BICFI`, not `BIC`
    #[default]
    IsoV9,

    /// `pain.001.003.03` — legacy Deutsche Kreditwirtschaft DK V2.7 (2013).
    ///
    /// **End-of-life.** Superseded in November 2022 by DK Anlage 3 V3.6 and
    /// absent from every DK format specification since. Retained only for
    /// systems still pinned to it; new integrations should use
    /// [`IsoV9`](Self::IsoV9).
    ///
    /// Emits a bare `<ReqdExctnDt>2026-07-20</ReqdExctnDt>` and names the agent
    /// BIC element `BIC`.
    DkV2_7,
}

impl CreditTransferSchema {
    /// The XML namespace URI for this schema version.
    #[must_use]
    pub const fn namespace(self) -> &'static str {
        match self {
            Self::DkV2_7 => "urn:iso:std:iso:20022:tech:xsd:pain.001.003.03",
            Self::IsoV9 => "urn:iso:std:iso:20022:tech:xsd:pain.001.001.09",
        }
    }

    /// The element name carrying an agent's BIC.
    ///
    /// ISO renamed `BIC` to `BICFI` in the 2019 maintenance release.
    #[must_use]
    const fn bic_element(self) -> &'static str {
        match self {
            Self::DkV2_7 => "BIC",
            Self::IsoV9 => "BICFI",
        }
    }

    /// Whether `ReqdExctnDt` needs a `<Dt>` wrapper child.
    #[must_use]
    const fn wraps_execution_date(self) -> bool {
        matches!(self, Self::IsoV9)
    }
}

// ── LocalInstrument ───────────────────────────────────────────────────────────

/// SEPA Credit Transfer local instrument variant.
///
/// When set to [`Inst`](LocalInstrument::Inst), the builder:
/// - Switches the schema to [`CreditTransferSchema::IsoV9`] automatically
/// - Adds `<LclInstrm><Cd>INST</Cd></LclInstrm>` to `PmtTpInf`
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum LocalInstrument {
    /// Standard SEPA Credit Transfer — no local instrument code (default).
    #[default]
    None,
    /// SEPA Instant Credit Transfer — 10-second settlement window.
    ///
    /// EU Regulation 2024/886 mandates PSP support for SCT Inst in the eurozone.
    Inst,
}

// ── CreditTransferEntry ───────────────────────────────────────────────────────

/// A single credit transfer in a pain.001 batch.
///
/// Construct with [`CreditTransferEntry::new`] and chain optional fields.
#[derive(Debug, Clone)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CreditTransferEntry {
    /// Beneficiary name (`Cdtr/Nm`).
    pub creditor_name: String,
    /// Beneficiary IBAN (validated).
    pub creditor_iban: Iban,
    /// Beneficiary BIC. Uses `NOTPROVIDED` in XML when `None`.
    pub creditor_bic: Option<Bic>,
    /// Payment amount in **ct** (1/100 EUR). Must be positive.
    pub amount_ct: i64,
    /// Unique end-to-end reference (`EndToEndId`) visible on beneficiary's statement.
    pub end_to_end_id: String,
    /// Remittance information (`RmtInf`) — free text or a structured reference.
    pub remittance: Option<RemittanceInfo>,
    /// Ultimate debtor (`UltmtDbtr`) — who the money is really from.
    pub ultimate_debtor: Option<Party>,
    /// Ultimate creditor (`UltmtCdtr`) — who the money is really for.
    pub ultimate_creditor: Option<Party>,
    /// Purpose code (`Purp/Cd`), informational.
    pub purpose: Option<Purpose>,
}

impl CreditTransferEntry {
    /// Create a new credit transfer entry with required fields.
    ///
    /// Chain [`with_bic`](Self::with_bic) and [`with_description`](Self::with_description).
    pub fn new(
        creditor_name: impl Into<String>,
        creditor_iban: Iban,
        amount_ct: i64,
        end_to_end_id: impl Into<String>,
    ) -> Self {
        Self {
            creditor_name: creditor_name.into(),
            creditor_iban,
            amount_ct,
            end_to_end_id: end_to_end_id.into(),
            creditor_bic: None,
            remittance: None,
            ultimate_debtor: None,
            ultimate_creditor: None,
            purpose: None,
        }
    }

    /// Set the beneficiary's BIC (optional).
    #[must_use]
    pub fn with_bic(mut self, bic: Bic) -> Self {
        self.creditor_bic = Some(bic);
        self
    }

    /// Set free-text remittance information (`RmtInf/Ustrd`), max 140 characters.
    ///
    /// This is the German *Verwendungszweck*: human-readable, but useless for
    /// automatic reconciliation. Prefer
    /// [`with_reference`](Self::with_reference) when you control the invoice.
    #[must_use]
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.remittance = Some(RemittanceInfo::Unstructured(desc.into()));
        self
    }

    /// Set a structured ISO 11649 creditor reference (`RmtInf/Strd/CdtrRefInf`).
    ///
    /// Mutually exclusive with [`with_description`](Self::with_description) —
    /// the EPC permits one or the other, and the later call wins.
    #[must_use]
    pub fn with_reference(mut self, reference: crate::RfReference) -> Self {
        self.remittance = Some(RemittanceInfo::Structured(reference));
        self
    }

    /// Set remittance information directly.
    #[must_use]
    pub fn with_remittance(mut self, remittance: RemittanceInfo) -> Self {
        self.remittance = Some(remittance);
        self
    }

    /// Set the ultimate debtor (`UltmtDbtr`) — who the money is really from,
    /// when that differs from the account holder.
    #[must_use]
    pub fn with_ultimate_debtor(mut self, party: impl Into<Party>) -> Self {
        self.ultimate_debtor = Some(party.into());
        self
    }

    /// Set the ultimate creditor (`UltmtCdtr`) — who the money is really for.
    ///
    /// pain.001 allows this only at transaction level; the element does not
    /// exist at payment-information level.
    #[must_use]
    pub fn with_ultimate_creditor(mut self, party: impl Into<Party>) -> Self {
        self.ultimate_creditor = Some(party.into());
        self
    }

    /// Set the purpose code (`Purp/Cd`).
    ///
    /// Informational — see [`Purpose`] for why this is not [`CategoryPurpose`].
    ///
    /// [`CategoryPurpose`]: crate::CategoryPurpose
    #[must_use]
    pub fn with_purpose(mut self, purpose: Purpose) -> Self {
        self.purpose = Some(purpose);
        self
    }
}

// ── CreditTransferGroup ───────────────────────────────────────────────────────

/// One `PmtInf` block — a set of transfers sharing a debtor account, an
/// execution date and a local instrument.
///
/// A pain.001 message may carry several of these. That is the only way to put
/// transfers with **different execution dates**, different debtor accounts or a
/// mix of ordinary and instant transfers into a single file, rather than
/// submitting several files to the bank.
///
/// ## Examples
///
/// ```
/// use sepa::{CreditTransferEntry, CreditTransferGroup, validate_iban};
///
/// let iban = validate_iban("DE89370400440532013000")?;
/// let group = CreditTransferGroup::new("Acme GmbH", &iban)
///     .execution_date("2026-07-20")
///     .add_entry(CreditTransferEntry::new("Supplier AG", iban.clone(), 12_000, "E2E-1"));
/// assert_eq!(group.entry_count(), 1);
/// # Ok::<(), sepa::IbanError>(())
/// ```
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct CreditTransferGroup {
    payment_info_id: Option<String>,
    debtor_name: String,
    debtor_iban: Iban,
    debtor_bic: Option<Bic>,
    execution_date: String,
    local_instrument: LocalInstrument,
    batch_booking: Option<bool>,
    category_purpose: Option<CategoryPurpose>,
    ultimate_debtor: Option<Party>,
    entries: Vec<CreditTransferEntry>,
}

impl CreditTransferGroup {
    /// A new payment group drawn on `debtor_iban`.
    pub fn new(debtor_name: impl Into<String>, debtor_iban: &Iban) -> Self {
        Self {
            payment_info_id: None,
            debtor_name: debtor_name.into(),
            debtor_iban: debtor_iban.clone(),
            debtor_bic: None,
            execution_date: crate::pain008::default_collection_date(),
            local_instrument: LocalInstrument::None,
            batch_booking: None,
            category_purpose: None,
            ultimate_debtor: None,
            entries: Vec::new(),
        }
    }

    /// Override the `PmtInfId`.
    ///
    /// Defaults to the message's `MsgId` for a single group, and to
    /// `MsgId-<n>` when there are several — truncated if needed to stay inside
    /// the 35-character limit.
    #[must_use]
    pub fn payment_info_id(mut self, id: impl Into<String>) -> Self {
        self.payment_info_id = Some(id.into());
        self
    }

    /// Set the requested execution date (`ReqdExctnDt`), `YYYY-MM-DD`.
    #[must_use]
    pub fn execution_date(mut self, date: impl Into<String>) -> Self {
        self.execution_date = date.into();
        self
    }

    /// Set the debtor's BIC (`DbtrAgt`).
    #[must_use]
    pub fn debtor_bic(mut self, bic: Bic) -> Self {
        self.debtor_bic = Some(bic);
        self
    }

    /// Set the local instrument for this group.
    ///
    /// [`LocalInstrument::Inst`] marks the group as SEPA Instant. Note the
    /// schema is chosen once for the whole message — see
    /// [`Pain001Builder::schema`].
    #[must_use]
    pub fn local_instrument(mut self, li: LocalInstrument) -> Self {
        self.local_instrument = li;
        self
    }

    /// Request batch booking (`BtchBookg`).
    ///
    /// `true` asks for one aggregate entry on the statement, `false` for one
    /// entry per transaction. Omitted by default, which defers to the agreement
    /// with the bank — there is no scheme-wide default, and German banks treat
    /// an absent value as `true`. `false` takes effect only where a
    /// single-entry agreement is in place.
    #[must_use]
    pub fn batch_booking(mut self, batch: bool) -> Self {
        self.batch_booking = Some(batch);
        self
    }

    /// Set the category purpose (`PmtTpInf/CtgyPurp`) for this group.
    ///
    /// Unlike a purpose code, this may trigger special handling by the banks.
    /// It belongs at one level only, so setting it here excludes setting it per
    /// transaction.
    #[must_use]
    pub fn category_purpose(mut self, purpose: CategoryPurpose) -> Self {
        self.category_purpose = Some(purpose);
        self
    }

    /// Set the ultimate debtor for the whole group (`PmtInf/UltmtDbtr`).
    ///
    /// Mutually exclusive with the per-entry ultimate debtor: the DK rules
    /// require one level or the other, never both.
    #[must_use]
    pub fn ultimate_debtor(mut self, party: impl Into<Party>) -> Self {
        self.ultimate_debtor = Some(party.into());
        self
    }

    /// Add a credit transfer to this group.
    #[must_use]
    pub fn add_entry(mut self, entry: CreditTransferEntry) -> Self {
        self.entries.push(entry);
        self
    }

    /// Add several credit transfers.
    #[must_use]
    pub fn add_entries(mut self, entries: impl IntoIterator<Item = CreditTransferEntry>) -> Self {
        self.entries.extend(entries);
        self
    }

    /// Number of transfers in this group.
    #[must_use]
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// Total amount in ct across this group, saturating on overflow.
    #[must_use]
    pub fn total_ct(&self) -> i64 {
        self.entries
            .iter()
            .fold(0i64, |acc, e| acc.saturating_add(e.amount_ct))
    }
}

// ── Builder ───────────────────────────────────────────────────────────────────

/// Builder for an ISO 20022 pain.001 (SEPA Credit Transfer) message.
///
/// A message carries one or more [`CreditTransferGroup`]s, each becoming a
/// `PmtInf` block.
#[derive(Debug, Clone)]
pub struct Pain001Builder {
    initiating_party: String,
    msg_id: String,
    created_at: Option<String>,
    schema: CreditTransferSchema,
    charset: CharsetPolicy,
    groups: Vec<CreditTransferGroup>,
}

impl Pain001Builder {
    /// A new message initiated by `initiating_party`.
    pub fn new(initiating_party: impl Into<String>) -> Self {
        Self {
            initiating_party: initiating_party.into(),
            msg_id: format!("sct-{}", crate::pain008::epoch_secs()),
            created_at: None,
            schema: CreditTransferSchema::default(),
            charset: CharsetPolicy::default(),
            groups: Vec::new(),
        }
    }

    /// Override the `MsgId` (max 35 chars).
    #[must_use]
    pub fn msg_id(mut self, id: impl Into<String>) -> Self {
        self.msg_id = id.into();
        self
    }

    /// Pin the creation timestamp (`CreDtTm`), ISO 8601.
    ///
    /// Defaults to the current time. Set it explicitly to make output
    /// byte-reproducible — for golden-file tests, or to regenerate a submitted
    /// file identically for an audit.
    #[must_use]
    pub fn created_at(mut self, timestamp: impl Into<String>) -> Self {
        self.created_at = Some(timestamp.into());
        self
    }

    /// Override the pain.001 schema version.
    #[must_use]
    pub fn schema(mut self, schema: CreditTransferSchema) -> Self {
        self.schema = schema;
        self
    }

    /// Set how text outside the SEPA character set is handled.
    #[must_use]
    pub fn charset(mut self, policy: CharsetPolicy) -> Self {
        self.charset = policy;
        self
    }

    /// Add a payment group (`PmtInf`).
    #[must_use]
    pub fn add_group(mut self, group: CreditTransferGroup) -> Self {
        self.groups.push(group);
        self
    }

    /// Number of payment groups.
    #[must_use]
    pub fn group_count(&self) -> usize {
        self.groups.len()
    }

    /// Total number of transactions across every group — the `GrpHdr/NbOfTxs`.
    #[must_use]
    pub fn entry_count(&self) -> usize {
        self.groups
            .iter()
            .map(CreditTransferGroup::entry_count)
            .sum()
    }

    /// Total amount in ct across every group, saturating on overflow.
    #[must_use]
    pub fn total_ct(&self) -> i64 {
        self.groups
            .iter()
            .fold(0i64, |acc, g| acc.saturating_add(g.total_ct()))
    }

    /// The effective `PmtInfId` for group `index`.
    fn payment_info_id(&self, index: usize) -> String {
        if let Some(id) = self
            .groups
            .get(index)
            .and_then(|g| g.payment_info_id.clone())
        {
            return id;
        }
        if self.groups.len() <= 1 {
            return self.msg_id.clone();
        }
        // Several groups need distinct identifiers. Suffix the MsgId, trimming
        // it first so the result still fits Max35Text.
        let suffix = format!("-{}", index + 1);
        let keep = MAX_ID_LEN.saturating_sub(suffix.chars().count());
        format!("{}{suffix}", truncate_chars(&self.msg_id, keep))
    }

    /// Validate the message without producing XML.
    ///
    /// # Errors
    ///
    /// See [`ValidationError`].
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.groups.is_empty() || self.entry_count() == 0 {
            // Both PmtInf and CdtTrfTxInf are 1..n.
            return Err(ValidationError::EmptyBatch);
        }
        check_id("GrpHdr/MsgId", &self.msg_id)?;
        check_name(
            "InitgPty/Nm",
            &self.charset.apply("InitgPty/Nm", &self.initiating_party)?,
        )?;

        let mut total: i64 = 0;
        for (i, g) in self.groups.iter().enumerate() {
            if g.entries.is_empty() {
                return Err(ValidationError::EmptyBatch);
            }
            check_id("PmtInf/PmtInfId", &self.payment_info_id(i))?;
            check_date("PmtInf/ReqdExctnDt", &g.execution_date)?;
            check_name("Dbtr/Nm", &self.charset.apply("Dbtr/Nm", &g.debtor_name)?)?;
            if let Some(p) = &g.category_purpose {
                p.validate("PmtTpInf/CtgyPurp/Cd")?;
            }
            if let Some(p) = &g.ultimate_debtor {
                p.validate("PmtInf/UltmtDbtr", self.charset)?;
            }

            for e in &g.entries {
                // The DK forbids the same ultimate party at both levels.
                if g.ultimate_debtor.is_some() && e.ultimate_debtor.is_some() {
                    return Err(ValidationError::ConflictingLevels { field: "UltmtDbtr" });
                }
                check_id("CdtTrfTxInf/PmtId/EndToEndId", &e.end_to_end_id)?;
                check_amount("CdtTrfTxInf/Amt/InstdAmt", e.amount_ct)?;
                check_name("Cdtr/Nm", &self.charset.apply("Cdtr/Nm", &e.creditor_name)?)?;
                if let Some(p) = &e.ultimate_debtor {
                    p.validate("CdtTrfTxInf/UltmtDbtr", self.charset)?;
                }
                if let Some(p) = &e.ultimate_creditor {
                    p.validate("CdtTrfTxInf/UltmtCdtr", self.charset)?;
                }
                if let Some(p) = &e.purpose {
                    p.validate("CdtTrfTxInf/Purp/Cd")?;
                }
                if let Some(r) = &e.remittance {
                    if let RemittanceInfo::Unstructured(text) = r {
                        check_remittance(
                            "RmtInf/Ustrd",
                            &self.charset.apply("RmtInf/Ustrd", text)?,
                        )?;
                    } else {
                        r.validate("RmtInf/Strd")?;
                    }
                }
                total = total
                    .checked_add(e.amount_ct)
                    .ok_or(ValidationError::ControlSumOverflow)?;
            }
        }
        Ok(())
    }

    /// Validate the message and generate the pain.001 XML.
    ///
    /// # Errors
    ///
    /// See [`validate`](Self::validate).
    ///
    /// # Examples
    ///
    /// ```
    /// use sepa::{CreditTransferEntry, CreditTransferGroup, Pain001Builder, validate_iban};
    ///
    /// let iban = validate_iban("DE89370400440532013000")?;
    /// let xml = Pain001Builder::new("Acme GmbH")
    ///     .msg_id("CT-001")
    ///     .add_group(
    ///         CreditTransferGroup::new("Acme GmbH", &iban)
    ///             .execution_date("2026-07-20")
    ///             .add_entry(CreditTransferEntry::new("Payee", iban.clone(), 100, "E2E-1")),
    ///     )
    ///     .build()?;
    /// assert!(xml.contains("<NbOfTxs>1</NbOfTxs>"));
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn build(&self) -> Result<String, ValidationError> {
        self.validate()?;
        let mut buf = String::with_capacity(850 + self.entry_count() * 420);
        // Writing into a String is infallible.
        let _ = self.write_xml_to(&mut buf);
        Ok(buf)
    }

    /// Validate and stream the pain.001 XML to an [`io::Write`](std::io::Write).
    ///
    /// Validation runs before anything is written, so a rejected message leaves
    /// the writer untouched.
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

    /// Serialise. Private: callers go through `build` or `write_to`, which validate.
    fn write_xml_to<W: std::fmt::Write>(&self, w: &mut W) -> std::fmt::Result {
        use crate::xml_util::write_escaped;

        let now = self
            .created_at
            .clone()
            .unwrap_or_else(crate::pain008::iso8601_now);
        let namespace = self.schema.namespace();
        let initiating = self
            .charset
            .apply("InitgPty/Nm", &self.initiating_party)
            .unwrap_or(std::borrow::Cow::Borrowed(&self.initiating_party));

        w.write_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n")?;
        writeln!(w, "<Document xmlns=\"{namespace}\">")?;
        w.write_str("  <CstmrCdtTrfInitn>\n    <GrpHdr>\n      <MsgId>")?;
        write_escaped(w, &self.msg_id)?;
        write!(w, "</MsgId>\n      <CreDtTm>{now}</CreDtTm>\n")?;
        // GrpHdr totals span every group.
        write!(
            w,
            "      <NbOfTxs>{}</NbOfTxs>\n      <CtrlSum>{}</CtrlSum>\n",
            self.entry_count(),
            ct_to_eur_str(self.total_ct())
        )?;
        w.write_str("      <InitgPty><Nm>")?;
        write_escaped(w, &initiating)?;
        w.write_str("</Nm></InitgPty>\n    </GrpHdr>\n")?;

        for (i, g) in self.groups.iter().enumerate() {
            self.write_group(w, g, &self.payment_info_id(i))?;
        }

        w.write_str("  </CstmrCdtTrfInitn>\n</Document>")
    }

    /// Write one `PmtInf` block, in XSD sequence order.
    fn write_group<W: std::fmt::Write>(
        &self,
        w: &mut W,
        g: &CreditTransferGroup,
        payment_info_id: &str,
    ) -> std::fmt::Result {
        use crate::xml_util::write_escaped;

        let bic_el = self.schema.bic_element();
        let debtor_name = self
            .charset
            .apply("Dbtr/Nm", &g.debtor_name)
            .unwrap_or(std::borrow::Cow::Borrowed(&g.debtor_name));

        w.write_str("    <PmtInf>\n      <PmtInfId>")?;
        write_escaped(w, payment_info_id)?;
        w.write_str("</PmtInfId>\n      <PmtMtd>TRF</PmtMtd>\n")?;
        if let Some(batch) = g.batch_booking {
            writeln!(w, "      <BtchBookg>{batch}</BtchBookg>")?;
        }
        write!(
            w,
            "      <NbOfTxs>{}</NbOfTxs>\n      <CtrlSum>{}</CtrlSum>\n",
            g.entry_count(),
            ct_to_eur_str(g.total_ct())
        )?;
        w.write_str("      <PmtTpInf>\n        <SvcLvl><Cd>SEPA</Cd></SvcLvl>\n")?;
        if g.local_instrument == LocalInstrument::Inst {
            w.write_str("        <LclInstrm><Cd>INST</Cd></LclInstrm>\n")?;
        }
        if let Some(p) = &g.category_purpose {
            writeln!(w, "        <CtgyPurp><Cd>{}</Cd></CtgyPurp>", p.as_code())?;
        }
        w.write_str("      </PmtTpInf>\n")?;

        // pain.001.001.09 types ReqdExctnDt as a date/time choice.
        if self.schema.wraps_execution_date() {
            writeln!(
                w,
                "      <ReqdExctnDt><Dt>{}</Dt></ReqdExctnDt>",
                g.execution_date
            )?;
        } else {
            writeln!(w, "      <ReqdExctnDt>{}</ReqdExctnDt>", g.execution_date)?;
        }

        w.write_str("      <Dbtr><Nm>")?;
        write_escaped(w, &debtor_name)?;
        w.write_str("</Nm></Dbtr>\n")?;
        writeln!(
            w,
            "      <DbtrAcct><Id><IBAN>{}</IBAN></Id></DbtrAcct>",
            g.debtor_iban.as_str()
        )?;
        // DbtrAgt is mandatory; with no BIC the EPC "IBAN only" form applies.
        w.write_str("      <DbtrAgt><FinInstnId>")?;
        match &g.debtor_bic {
            Some(bic) => write!(w, "<{bic_el}>{}</{bic_el}>", bic.as_str())?,
            None => w.write_str("<Othr><Id>NOTPROVIDED</Id></Othr>")?,
        }
        w.write_str("</FinInstnId></DbtrAgt>\n")?;

        if let Some(p) = &g.ultimate_debtor {
            p.write_xml(w, "UltmtDbtr", "      ", self.charset)?;
        }
        w.write_str("      <ChrgBr>SLEV</ChrgBr>\n")?;

        for entry in &g.entries {
            self.write_transaction(w, entry)?;
        }
        w.write_str("    </PmtInf>\n")
    }

    fn write_transaction<W: std::fmt::Write>(
        &self,
        w: &mut W,
        e: &CreditTransferEntry,
    ) -> std::fmt::Result {
        use crate::xml_util::{write_escaped, write_eur};

        let bic_el = self.schema.bic_element();
        let name = self
            .charset
            .apply("Cdtr/Nm", &e.creditor_name)
            .unwrap_or(std::borrow::Cow::Borrowed(&e.creditor_name));

        w.write_str("    <CdtTrfTxInf>\n      <PmtId>\n        <EndToEndId>")?;
        write_escaped(w, &e.end_to_end_id)?;
        w.write_str("</EndToEndId>\n      </PmtId>\n      <Amt><InstdAmt Ccy=\"EUR\">")?;
        write_eur(w, e.amount_ct)?;
        w.write_str("</InstdAmt></Amt>\n")?;

        // XSD sequence: UltmtDbtr precedes the creditor block, UltmtCdtr follows
        // CdtrAcct, and Purp sits between UltmtCdtr and RmtInf.
        if let Some(p) = &e.ultimate_debtor {
            p.write_xml(w, "UltmtDbtr", "      ", self.charset)?;
        }

        // CdtrAgt is optional for SCT. The EPC guidelines say that when the BIC
        // is unknown the element is simply omitted — and pain.001.003.03 makes
        // that structural, since its CdtrAgt type has a mandatory BIC and no
        // Othr branch, leaving no way to express "not provided".
        if let Some(bic) = &e.creditor_bic {
            writeln!(
                w,
                "      <CdtrAgt><FinInstnId><{bic_el}>{}</{bic_el}></FinInstnId></CdtrAgt>",
                bic.as_str()
            )?;
        }

        w.write_str("      <Cdtr><Nm>")?;
        write_escaped(w, &name)?;
        w.write_str("</Nm></Cdtr>\n      <CdtrAcct><Id><IBAN>")?;
        w.write_str(e.creditor_iban.as_str())?;
        w.write_str("</IBAN></Id></CdtrAcct>\n")?;

        if let Some(p) = &e.ultimate_creditor {
            p.write_xml(w, "UltmtCdtr", "      ", self.charset)?;
        }
        if let Some(purpose) = &e.purpose {
            writeln!(w, "      <Purp><Cd>{}</Cd></Purp>", purpose.as_code())?;
        }

        if let Some(remittance) = &e.remittance {
            // `Strd` is emitted minified: the EPC caps the whole block at 140
            // characters including tags, and pretty-printing alone overruns it.
            remittance.write_xml(w, "      ", self.charset)?;
        }

        w.write_str("    </CdtTrfTxInf>\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::iban::validate_iban;
    use crate::validate::{CharsetPolicy, ValidationError};

    fn de_iban() -> Iban {
        validate_iban("DE89370400440532013000").unwrap()
    }
    fn nl_iban() -> Iban {
        validate_iban("NL91ABNA0417164300").unwrap()
    }
    fn entry(amount_ct: i64) -> CreditTransferEntry {
        CreditTransferEntry::new("Max Mustermann", nl_iban(), amount_ct, "E2E-001")
    }
    /// A one-group message with everything `build` requires.
    fn one_group(name: &str) -> Pain001Builder {
        Pain001Builder::new(name).msg_id("CT-001").add_group(
            CreditTransferGroup::new(name, &de_iban())
                .execution_date("2026-07-20")
                .add_entry(entry(12_000)),
        )
    }

    #[test]
    fn basic_structure() {
        let xml = one_group("Acme GmbH").build().unwrap();
        assert!(xml.contains("urn:iso:std:iso:20022:tech:xsd:pain.001.001.09"));
        assert!(xml.contains("<MsgId>CT-001</MsgId>"));
        assert!(xml.contains("<PmtMtd>TRF</PmtMtd>"));
        assert!(xml.contains("<InstdAmt Ccy=\"EUR\">120.00</InstdAmt>"));
        assert!(xml.contains("<ChrgBr>SLEV</ChrgBr>"));
        // pain.001.001.09 wraps the date in a <Dt> choice child.
        assert!(xml.contains("<ReqdExctnDt><Dt>2026-07-20</Dt></ReqdExctnDt>"));
    }

    #[test]
    fn several_groups_carry_their_own_dates_and_totals() {
        let xml = Pain001Builder::new("Acme GmbH")
            .msg_id("CT-MULTI")
            .add_group(
                CreditTransferGroup::new("Acme GmbH", &de_iban())
                    .execution_date("2026-07-20")
                    .add_entry(entry(10_000)),
            )
            .add_group(
                CreditTransferGroup::new("Acme GmbH", &nl_iban())
                    .execution_date("2026-07-25")
                    .add_entry(entry(5_000))
                    .add_entry(entry(2_500)),
            )
            .build()
            .unwrap();

        assert_eq!(xml.matches("<PmtInf>").count(), 2);
        // Different execution dates in one file — impossible with one PmtInf.
        assert!(xml.contains("<Dt>2026-07-20</Dt>"));
        assert!(xml.contains("<Dt>2026-07-25</Dt>"));
        // GrpHdr totals span every group; each PmtInf carries its own.
        assert!(xml.contains("<NbOfTxs>3</NbOfTxs>"));
        assert!(xml.contains("<CtrlSum>175.00</CtrlSum>"));
        assert!(xml.contains("<NbOfTxs>1</NbOfTxs>"));
        assert!(xml.contains("<CtrlSum>100.00</CtrlSum>"));
        assert!(xml.contains("<NbOfTxs>2</NbOfTxs>"));
        assert!(xml.contains("<CtrlSum>75.00</CtrlSum>"));
    }

    #[test]
    fn payment_info_ids_are_unique_and_within_max35text() {
        let msg_id = "M".repeat(35);
        let b = Pain001Builder::new("Acme").msg_id(&msg_id);
        let b = (0..3).fold(b, |b, _| {
            b.add_group(
                CreditTransferGroup::new("Acme", &de_iban())
                    .execution_date("2026-07-20")
                    .add_entry(entry(100)),
            )
        });
        let xml = b.build().unwrap();

        let ids: Vec<&str> = xml
            .split("<PmtInfId>")
            .skip(1)
            .map(|c| c.split('<').next().unwrap())
            .collect();
        assert_eq!(ids.len(), 3);
        for id in &ids {
            assert!(id.chars().count() <= 35, "{id} exceeds Max35Text");
        }
        let unique: std::collections::BTreeSet<_> = ids.iter().collect();
        assert_eq!(unique.len(), 3, "PmtInfId must be unique per group");
    }

    #[test]
    fn single_group_reuses_the_msg_id_verbatim() {
        let xml = one_group("Acme").build().unwrap();
        assert!(xml.contains("<PmtInfId>CT-001</PmtInfId>"));
    }

    #[test]
    fn legacy_dk_schema_emits_a_bare_date_and_bic() {
        let xml = Pain001Builder::new("Test")
            .schema(CreditTransferSchema::DkV2_7)
            .msg_id("CT-DK")
            .add_group(
                CreditTransferGroup::new("Test", &de_iban())
                    .execution_date("2026-07-20")
                    .debtor_bic("COBADEFF".parse().unwrap())
                    .add_entry(entry(5_000).with_bic("ABNANL2A".parse().unwrap())),
            )
            .build()
            .unwrap();
        assert!(xml.contains("pain.001.003.03"));
        assert!(xml.contains("<ReqdExctnDt>2026-07-20</ReqdExctnDt>"));
        assert!(xml.contains("<BIC>COBADEFF</BIC>"));
        assert!(!xml.contains("BICFI"));
    }

    #[test]
    fn iban_only_agents_and_omitted_creditor_agent() {
        let xml = one_group("Test").build().unwrap();
        assert!(!xml.contains("<CdtrAgt>"), "EPC omits CdtrAgt with no BIC");
        assert!(xml.contains("<DbtrAgt><FinInstnId><Othr><Id>NOTPROVIDED</Id></Othr>"));
        assert!(!xml.contains("NOTPROVIDED</BIC"));
    }

    #[test]
    fn sct_instant_selects_the_iso_schema_and_marks_the_group() {
        let xml = Pain001Builder::new("Acme")
            .msg_id("CT-INST")
            .add_group(
                CreditTransferGroup::new("Acme", &de_iban())
                    .local_instrument(LocalInstrument::Inst)
                    .execution_date("2026-07-20")
                    .add_entry(entry(5_000)),
            )
            .build()
            .unwrap();
        assert!(xml.contains("pain.001.001.09"));
        assert!(xml.contains("<LclInstrm><Cd>INST</Cd></LclInstrm>"));
    }

    #[test]
    fn batch_booking_and_category_purpose_are_group_level() {
        let xml = Pain001Builder::new("Acme")
            .msg_id("CT-OPT")
            .add_group(
                CreditTransferGroup::new("Acme", &de_iban())
                    .execution_date("2026-07-20")
                    .batch_booking(true)
                    .category_purpose(CategoryPurpose::Sala)
                    .add_entry(entry(100)),
            )
            .build()
            .unwrap();
        assert!(xml.contains("<BtchBookg>true</BtchBookg>"));
        assert!(xml.contains("<CtgyPurp><Cd>SALA</Cd></CtgyPurp>"));
    }

    #[test]
    fn batch_booking_is_omitted_unless_set() {
        // There is no scheme-wide default; omitting defers to the bank agreement.
        assert!(!one_group("Acme").build().unwrap().contains("BtchBookg"));
    }

    #[test]
    fn ultimate_debtor_cannot_be_set_at_both_levels() {
        let b = Pain001Builder::new("Acme").msg_id("CT-ULT").add_group(
            CreditTransferGroup::new("Acme", &de_iban())
                .execution_date("2026-07-20")
                .ultimate_debtor(Party::new("Gruppe"))
                .add_entry(entry(100).with_ultimate_debtor(Party::new("Transaktion"))),
        );
        assert_eq!(
            b.build(),
            Err(ValidationError::ConflictingLevels { field: "UltmtDbtr" })
        );
    }

    #[test]
    fn empty_message_and_empty_group_are_both_rejected() {
        assert_eq!(
            Pain001Builder::new("Acme").msg_id("E").build(),
            Err(ValidationError::EmptyBatch)
        );
        assert_eq!(
            Pain001Builder::new("Acme")
                .msg_id("E")
                .add_group(CreditTransferGroup::new("Acme", &de_iban()))
                .build(),
            Err(ValidationError::EmptyBatch)
        );
    }

    #[test]
    fn validation_rejects_bad_fields() {
        let g = || CreditTransferGroup::new("Acme", &de_iban()).execution_date("2026-07-20");
        let b = || Pain001Builder::new("Acme").msg_id("OK");

        assert!(matches!(
            b().add_group(g().add_entry(entry(0))).build(),
            Err(ValidationError::AmountOutOfRange { .. })
        ));
        assert!(matches!(
            b().add_group(g().add_entry(entry(100_000_000_000))).build(),
            Err(ValidationError::AmountOutOfRange { .. })
        ));
        assert!(matches!(
            b().msg_id("X".repeat(36))
                .add_group(g().add_entry(entry(100)))
                .build(),
            Err(ValidationError::TooLong { .. })
        ));
        assert!(matches!(
            b().add_group(
                CreditTransferGroup::new("Acme", &de_iban())
                    .execution_date("2026-02-30")
                    .add_entry(entry(100))
            )
            .build(),
            Err(ValidationError::InvalidDate { .. })
        ));
    }

    #[test]
    fn totals_use_integer_arithmetic() {
        let b = Pain001Builder::new("Acme").msg_id("CT").add_group(
            CreditTransferGroup::new("Acme", &de_iban())
                .execution_date("2026-07-20")
                .add_entry(entry(10))
                .add_entry(entry(20)),
        );
        assert_eq!(b.total_ct(), 30);
        assert!(b.build().unwrap().contains("<CtrlSum>0.30</CtrlSum>"));
    }

    #[test]
    fn creation_timestamp_can_be_pinned_for_reproducible_output() {
        let build = || {
            one_group("Acme")
                .created_at("2026-07-19T12:00:00")
                .build()
                .unwrap()
        };
        assert_eq!(build(), build(), "output must be byte-identical");
        assert!(build().contains("<CreDtTm>2026-07-19T12:00:00</CreDtTm>"));
    }

    #[test]
    fn non_sepa_characters_are_transliterated() {
        let xml = Pain001Builder::new("Müller & Söhne GmbH")
            .msg_id("CT-UML")
            .add_group(
                CreditTransferGroup::new("Müller & Söhne GmbH", &de_iban())
                    .execution_date("2026-07-20")
                    .add_entry(
                        CreditTransferEntry::new("Ökonomie AG", nl_iban(), 100, "E2E-1")
                            .with_description("Zahlung für Groß-Auftrag"),
                    ),
            )
            .build()
            .unwrap();
        assert!(xml.contains("Mueller + Soehne GmbH"));
        assert!(xml.contains("Oekonomie AG"));
        assert!(xml.contains("Zahlung fuer Gross-Auftrag"));
    }

    #[test]
    fn strict_charset_policy_rejects_umlauts() {
        assert!(matches!(
            one_group("Müller GmbH")
                .charset(CharsetPolicy::Strict)
                .build(),
            Err(ValidationError::InvalidCharacter { ch: 'ü', .. })
        ));
    }

    #[test]
    fn streaming_matches_the_in_memory_build() {
        let direct = one_group("Acme")
            .created_at("2026-07-19T12:00:00")
            .build()
            .unwrap();
        let mut buf: Vec<u8> = Vec::new();
        one_group("Acme")
            .created_at("2026-07-19T12:00:00")
            .write_to(&mut buf)
            .unwrap();
        assert_eq!(direct, String::from_utf8(buf).unwrap());

        // A rejected message writes nothing at all.
        let mut empty: Vec<u8> = Vec::new();
        assert!(Pain001Builder::new("Acme").write_to(&mut empty).is_err());
        assert!(empty.is_empty());
    }
}
