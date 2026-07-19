//! ISO 11649 RF Creditor Reference and structured remittance information.
//!
//! An RF Creditor Reference is a self-checking invoice reference that survives
//! the round trip through the banking system: the creditor puts it on the
//! invoice, the debtor's bank carries it in `RmtInf/Strd/CdtrRefInf/Ref`, and it
//! comes back on the camt statement — letting the creditor match an incoming
//! payment to an invoice without human intervention.
//!
//! ## Structure (ISO 11649:2009 §5)
//!
//! ```text
//! RF 18 539007547034
//! ─┬ ─┬ ──────┬─────
//!  │  │       └─ creditor's own reference, 1–21 chars, A–Z and 0–9
//!  │  └───────── check digits, ISO 7064 MOD 97-10
//!  └──────────── literal "RF"
//! ```
//!
//! Maximum 25 characters. Printed in groups of four for legibility.
//!
//! ## In pain.001 / pain.008
//!
//! ```xml
//! <RmtInf>
//!   <Strd>
//!     <CdtrRefInf>
//!       <Tp><CdOrPrtry><Cd>SCOR</Cd></CdOrPrtry><Issr>ISO</Issr></Tp>
//!       <Ref>RF18539007547034</Ref>
//!     </CdtrRefInf>
//!   </Strd>
//! </RmtInf>
//! ```
//!
//! The EPC guidelines permit only `SCOR` in `Cd`, and require `Issr` to be
//! `ISO` when `Ref` carries an RF reference. Note the element is `CdOrPrtry`
//! (capital `O`) — a spelling several implementations get wrong, producing
//! schema-invalid output.
//!
//! ## Examples
//!
//! ```
//! use sepa::reference::RfReference;
//!
//! // Generate from your own invoice number — check digits are computed for you.
//! let rf = RfReference::generate("2348231")?;
//! assert_eq!(rf.as_str(), "RF712348231");
//!
//! // Validate one that arrived from outside.
//! let parsed: RfReference = "RF18 5390 0754 7034".parse()?;
//! assert_eq!(parsed.as_str(), "RF18539007547034");
//! assert_eq!(parsed.reference(), "539007547034");
//! assert_eq!(parsed.to_string(), "RF18 5390 0754 7034"); // grouped for printing
//! # Ok::<(), sepa::reference::RfReferenceError>(())
//! ```

use std::str::FromStr;

use crate::validate::{MAX_REMITTANCE_LEN, ValidationError, check_remittance};

/// Maximum total length of an RF Creditor Reference, including `RF` and the
/// check digits (ISO 11649 §5).
pub const MAX_RF_LEN: usize = 25;

/// Maximum length of the creditor's own part of an RF reference.
pub const MAX_RF_REFERENCE_LEN: usize = MAX_RF_LEN - 4;

// ── error ─────────────────────────────────────────────────────────────────────

/// Error returned when an RF Creditor Reference is invalid.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum RfReferenceError {
    /// The reference does not begin with `RF`.
    #[error("RF Creditor Reference must start with \"RF\", got {prefix:?}")]
    MissingRfPrefix {
        /// The first two characters that were found.
        prefix: String,
    },

    /// The reference is empty, or longer than 25 characters.
    #[error("RF Creditor Reference length {len} is outside the valid range 5–{MAX_RF_LEN}")]
    InvalidLength {
        /// The actual length after whitespace was stripped.
        len: usize,
    },

    /// The reference contains a character outside `A–Z` and `0–9`.
    #[error("RF Creditor Reference contains invalid character {ch:?}")]
    InvalidCharacter {
        /// The offending character.
        ch: char,
    },

    /// The check digits do not match the reference.
    #[error("RF Creditor Reference check digits are {actual}, expected {expected}")]
    InvalidChecksum {
        /// The check digits required by ISO 11649.
        expected: String,
        /// The check digits actually present.
        actual: String,
    },
}

// ── RfReference ───────────────────────────────────────────────────────────────

/// A validated ISO 11649 RF Creditor Reference.
///
/// Constructed only via [`RfReference::generate`] or [`RfReference::from_str`],
/// both of which enforce the MOD 97-10 check digits — so a value of this type
/// is always well-formed.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RfReference(String);

impl RfReference {
    /// The reference in electronic format, e.g. `"RF18539007547034"`.
    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The two check digits (characters 3–4).
    #[inline]
    #[must_use]
    pub fn check_digits(&self) -> &str {
        self.0.get(2..4).unwrap_or_default()
    }

    /// The creditor's own reference — everything after the check digits.
    #[inline]
    #[must_use]
    pub fn reference(&self) -> &str {
        self.0.get(4..).unwrap_or_default()
    }

    /// Compute an RF reference for `reference`, deriving the check digits.
    ///
    /// Non-alphanumeric characters are stripped first, as ISO 11649 §6.3.1
    /// requires, and lower-case letters are upper-cased. So `"inv-2026/0042"`
    /// and `"INV20260042"` produce the same reference.
    ///
    /// # Errors
    ///
    /// Returns [`RfReferenceError::InvalidLength`] when `reference` is empty or
    /// longer than 21 characters after stripping.
    ///
    /// # Examples
    ///
    /// ```
    /// use sepa::reference::RfReference;
    ///
    /// // The worked example from ISO 11649 Annex B.
    /// assert_eq!(RfReference::generate("2348231")?.as_str(), "RF712348231");
    ///
    /// // Separators are stripped before the check digits are computed.
    /// let a = RfReference::generate("inv-2026/0042")?;
    /// let b = RfReference::generate("INV20260042")?;
    /// assert_eq!(a, b);
    /// # Ok::<(), sepa::reference::RfReferenceError>(())
    /// ```
    pub fn generate(reference: &str) -> Result<Self, RfReferenceError> {
        let core: String = reference
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .map(|c| c.to_ascii_uppercase())
            .collect();

        if core.is_empty() || core.len() > MAX_RF_REFERENCE_LEN {
            return Err(RfReferenceError::InvalidLength {
                len: core.len() + 4,
            });
        }

        let check = check_digits_for(&core);
        Ok(Self(format!("RF{check}{core}")))
    }

    /// Compute the check digits an RF reference would carry, without building one.
    ///
    /// # Examples
    ///
    /// ```
    /// use sepa::reference::RfReference;
    /// assert_eq!(RfReference::check_digits_for("2348231"), "71");
    /// assert_eq!(RfReference::check_digits_for("539007547034"), "18");
    /// ```
    #[must_use]
    pub fn check_digits_for(reference: &str) -> String {
        let core: String = reference
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .map(|c| c.to_ascii_uppercase())
            .collect();
        check_digits_for(&core)
    }
}

/// ISO 11649 §6.3: append `"RF00"`, expand letters (A=10 … Z=35), then `98 − n mod 97`.
fn check_digits_for(core: &str) -> String {
    let remainder = mod97(core.chars().chain("RF00".chars()));
    format!("{:02}", 98 - remainder)
}

/// Streaming ISO 7064 MOD 97-10 over alphanumeric characters.
///
/// Digits contribute one decimal place, letters two (their 10–35 value), so the
/// running remainder never approaches `u64` overflow.
fn mod97(chars: impl Iterator<Item = char>) -> u64 {
    chars.fold(0u64, |acc, c| match c.to_digit(36) {
        Some(d) if d < 10 => (acc * 10 + u64::from(d)) % 97,
        Some(d) => (acc * 100 + u64::from(d)) % 97,
        None => acc,
    })
}

impl FromStr for RfReference {
    type Err = RfReferenceError;

    /// Parse and validate an RF reference in electronic or printed format.
    ///
    /// Whitespace is stripped and letters are upper-cased before validation.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let normalised: String = s
            .chars()
            .filter(|c| !c.is_whitespace())
            .map(|c| c.to_ascii_uppercase())
            .collect();

        // Shortest useful reference is "RF" + 2 check digits + 1 char.
        if !(5..=MAX_RF_LEN).contains(&normalised.len()) {
            return Err(RfReferenceError::InvalidLength {
                len: normalised.len(),
            });
        }
        if let Some(ch) = normalised.chars().find(|c| !c.is_ascii_alphanumeric()) {
            return Err(RfReferenceError::InvalidCharacter { ch });
        }
        let prefix = normalised.get(..2).unwrap_or_default();
        if prefix != "RF" {
            return Err(RfReferenceError::MissingRfPrefix {
                prefix: prefix.to_owned(),
            });
        }

        // ISO 11649 §6.2: rotate the first four characters to the end; valid iff
        // the remainder is 1.
        let (head, tail) = normalised.split_at(4);
        if mod97(tail.chars().chain(head.chars())) == 1 {
            Ok(Self(normalised))
        } else {
            let expected = check_digits_for(tail);
            Err(RfReferenceError::InvalidChecksum {
                expected,
                actual: normalised.get(2..4).unwrap_or_default().to_owned(),
            })
        }
    }
}

impl TryFrom<&str> for RfReference {
    type Error = RfReferenceError;
    fn try_from(s: &str) -> Result<Self, Self::Error> {
        s.parse()
    }
}

impl std::fmt::Display for RfReference {
    /// Printed format: groups of four, e.g. `"RF18 5390 0754 7034"`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (i, chunk) in self.0.as_bytes().chunks(4).enumerate() {
            if i > 0 {
                f.write_str(" ")?;
            }
            f.write_str(std::str::from_utf8(chunk).unwrap_or_default())?;
        }
        Ok(())
    }
}

impl AsRef<str> for RfReference {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl From<RfReference> for String {
    fn from(r: RfReference) -> Self {
        r.0
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for RfReference {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for RfReference {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

// ── RemittanceInfo ────────────────────────────────────────────────────────────

/// What to put in `RmtInf` — free text or a structured creditor reference.
///
/// The EPC permits **one or the other, not both**, and allows only a single
/// `Strd` occurrence.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum RemittanceInfo {
    /// Unstructured free text (`RmtInf/Ustrd`), up to 140 characters.
    ///
    /// The German *Verwendungszweck*. Readable by humans, useless to machines.
    Unstructured(String),

    /// A structured ISO 11649 creditor reference (`RmtInf/Strd/CdtrRefInf`).
    ///
    /// Prefer this when you control the invoice: it round-trips through the
    /// banking system and lets you reconcile automatically.
    Structured(RfReference),

    /// A structured reference issued under a national scheme rather than
    /// ISO 11649 — e.g. a Belgian structured communication.
    ///
    /// Emitted with `Cd = SCOR` but **without** `Issr = ISO`, since the EPC
    /// reserves that marker for genuine RF references. Not check-digit
    /// validated: the scheme is national and its rules vary.
    Proprietary {
        /// The reference value, up to 35 characters.
        reference: String,
        /// The issuing scheme, placed in `Tp/Issr`.
        issuer: Option<String>,
    },
}

impl RemittanceInfo {
    /// Free-text remittance information.
    pub fn unstructured(text: impl Into<String>) -> Self {
        Self::Unstructured(text.into())
    }

    /// A structured ISO 11649 reference.
    #[must_use]
    pub fn structured(reference: RfReference) -> Self {
        Self::Structured(reference)
    }

    /// Validate against the EPC length rules.
    ///
    /// # Errors
    ///
    /// Returns [`ValidationError`] when the text is empty or too long.
    pub fn validate(&self, field: &'static str) -> Result<(), ValidationError> {
        match self {
            Self::Unstructured(text) => check_remittance(field, text),
            // An RfReference is at most 25 characters by construction.
            Self::Structured(_) => Ok(()),
            Self::Proprietary { reference, .. } => {
                // Ref is Max35Text, and the whole Strd block must stay under 140.
                crate::validate::check_id(field, reference)
            }
        }
    }

    /// Write this remittance information as `RmtInf` XML.
    pub(crate) fn write_xml<W: std::fmt::Write>(
        &self,
        w: &mut W,
        indent: &str,
        charset: crate::validate::CharsetPolicy,
    ) -> std::fmt::Result {
        use crate::validate::truncate_chars;
        use crate::xml_util::write_escaped;

        write!(w, "{indent}<RmtInf>")?;
        match self {
            Self::Unstructured(text) => {
                let text = charset
                    .apply("RmtInf/Ustrd", text)
                    .unwrap_or(std::borrow::Cow::Borrowed(text));
                w.write_str("<Ustrd>")?;
                write_escaped(w, &truncate_chars(&text, MAX_REMITTANCE_LEN))?;
                w.write_str("</Ustrd>")?;
            }
            Self::Structured(rf) => {
                // `CdOrPrtry`, not `CdorPrtry` — the latter is schema-invalid and
                // is a mistake other implementations have shipped.
                w.write_str(
                    "<Strd><CdtrRefInf><Tp><CdOrPrtry><Cd>SCOR</Cd></CdOrPrtry>\
                     <Issr>ISO</Issr></Tp><Ref>",
                )?;
                w.write_str(rf.as_str())?;
                w.write_str("</Ref></CdtrRefInf></Strd>")?;
            }
            Self::Proprietary { reference, issuer } => {
                w.write_str("<Strd><CdtrRefInf><Tp><CdOrPrtry><Cd>SCOR</Cd></CdOrPrtry>")?;
                if let Some(issuer) = issuer {
                    w.write_str("<Issr>")?;
                    write_escaped(w, issuer)?;
                    w.write_str("</Issr>")?;
                }
                w.write_str("</Tp><Ref>")?;
                write_escaped(w, reference)?;
                w.write_str("</Ref></CdtrRefInf></Strd>")?;
            }
        }
        w.write_str("</RmtInf>\n")
    }
}

impl From<&str> for RemittanceInfo {
    fn from(text: &str) -> Self {
        Self::Unstructured(text.to_owned())
    }
}

impl From<String> for RemittanceInfo {
    fn from(text: String) -> Self {
        Self::Unstructured(text)
    }
}

impl From<RfReference> for RemittanceInfo {
    fn from(rf: RfReference) -> Self {
        Self::Structured(rf)
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_11649_annex_b_worked_example() {
        // ISO 11649 Annex B.1: 2348231 -> RF712348231
        let rf = RfReference::generate("2348231").unwrap();
        assert_eq!(rf.as_str(), "RF712348231");
        assert_eq!(rf.check_digits(), "71");
        assert_eq!(rf.reference(), "2348231");
    }

    #[test]
    fn printed_format_round_trips() {
        let rf: RfReference = "RF18539007547034".parse().unwrap();
        assert_eq!(rf.to_string(), "RF18 5390 0754 7034");
        assert_eq!("RF18 5390 0754 7034".parse::<RfReference>().unwrap(), rf);
    }

    #[test]
    fn the_iso_annex_a_string_is_not_a_valid_reference() {
        // ISO 11649 Annex A illustrates *print grouping* with "RF68539007547034".
        // Annex A is informative and that string's check digits are wrong
        // (mod 97 = 51, not 1) — yet it circulates widely as a test vector.
        // The correct reference for 539007547034 is RF18.
        assert!("RF68539007547034".parse::<RfReference>().is_err());
        assert_eq!(
            RfReference::generate("539007547034").unwrap().as_str(),
            "RF18539007547034"
        );
    }

    #[test]
    fn published_reference_vectors() {
        // Finance Finland and ISO 11649 published examples, each independently
        // verified to satisfy mod 97 == 1.
        for v in [
            "RF712348231",               // ISO 11649 Annex B
            "RF541234",                  // Finance Finland
            "RF98REF1234",               // Finance Finland
            "RF18539007547034",          // EPC069-12 QR example
            "RF040",                     // minimum length, leading-zero check digit
            "RF95ABCDEFGHIJKLMNOPQRSTU", // maximum length
        ] {
            assert!(v.parse::<RfReference>().is_ok(), "{v} must validate");
        }
        for v in [
            "RF712348232",  // check digit off by one
            "RF12",         // too short
            "RF7A2348231",  // non-numeric check digits
            "XY712348231",  // wrong prefix
            "RF7123-48231", // punctuation
        ] {
            assert!(v.parse::<RfReference>().is_err(), "{v} must be rejected");
        }
    }

    #[test]
    fn leading_zero_check_digits_are_padded() {
        // "0" needs check digits "04" — a bare `98 - n` would emit "4".
        let rf = RfReference::generate("0").unwrap();
        assert_eq!(rf.as_str(), "RF040");
        assert_eq!(rf.check_digits(), "04");
    }

    #[test]
    fn generated_references_always_validate() {
        for input in [
            "1",
            "2348231",
            "539007547034",
            "INV20260042",
            "ABCDEFGHIJKLMNOPQRSTU", // 21 chars, the maximum
            "0000000001",
            "Z",
        ] {
            let rf = RfReference::generate(input).unwrap();
            assert_eq!(
                rf.as_str().parse::<RfReference>().unwrap(),
                rf,
                "generated {} must re-validate",
                rf.as_str()
            );
            assert!(rf.as_str().len() <= MAX_RF_LEN);
        }
    }

    #[test]
    fn separators_are_stripped_before_computing_check_digits() {
        let a = RfReference::generate("inv-2026/0042").unwrap();
        let b = RfReference::generate("INV20260042").unwrap();
        assert_eq!(a, b);
        assert_eq!(a.reference(), "INV20260042");
    }

    #[test]
    fn check_digits_helper_matches_generation() {
        for input in ["2348231", "539007547034", "INV1"] {
            let rf = RfReference::generate(input).unwrap();
            assert_eq!(RfReference::check_digits_for(input), rf.check_digits());
        }
    }

    #[test]
    fn wrong_check_digits_are_rejected() {
        // RF71 is correct for 2348231, so RF72 must fail.
        let err = "RF722348231".parse::<RfReference>().unwrap_err();
        assert_eq!(
            err,
            RfReferenceError::InvalidChecksum {
                expected: "71".to_owned(),
                actual: "72".to_owned(),
            }
        );
    }

    #[test]
    fn malformed_input_is_rejected() {
        assert!(matches!(
            "XX712348231".parse::<RfReference>(),
            Err(RfReferenceError::MissingRfPrefix { .. })
        ));
        assert!(matches!(
            "RF71-2348".parse::<RfReference>(),
            Err(RfReferenceError::InvalidCharacter { ch: '-' })
        ));
        assert!(matches!(
            "".parse::<RfReference>(),
            Err(RfReferenceError::InvalidLength { .. })
        ));
        assert!(matches!(
            "RF18".parse::<RfReference>(),
            Err(RfReferenceError::InvalidLength { len: 4 })
        ));
        // 26 characters — one over the maximum.
        assert!(matches!(
            "RF181234567890123456789012".parse::<RfReference>(),
            Err(RfReferenceError::InvalidLength { len: 26 })
        ));
    }

    #[test]
    fn over_long_reference_is_rejected_at_generation() {
        // 22 characters of payload exceeds the 21-character limit.
        assert!(matches!(
            RfReference::generate(&"A".repeat(22)),
            Err(RfReferenceError::InvalidLength { .. })
        ));
        assert!(RfReference::generate(&"A".repeat(21)).is_ok());
        assert!(matches!(
            RfReference::generate("---"),
            Err(RfReferenceError::InvalidLength { .. })
        ));
    }

    #[test]
    fn lowercase_input_is_accepted_on_parse() {
        // ISO 11649 §6.2.3 permits lower case when validating.
        let rf: RfReference = "rf18539007547034".parse().unwrap();
        assert_eq!(rf.as_str(), "RF18539007547034");
    }

    #[test]
    fn structured_remittance_xml_uses_the_correct_element_names() {
        let rf = RfReference::generate("539007547034").unwrap();
        let mut out = String::new();
        RemittanceInfo::Structured(rf)
            .write_xml(&mut out, "  ", crate::validate::CharsetPolicy::default())
            .unwrap();

        assert!(
            out.contains("<CdOrPrtry>"),
            "must be CdOrPrtry, not CdorPrtry"
        );
        assert!(out.contains("<Cd>SCOR</Cd>"));
        assert!(out.contains("<Issr>ISO</Issr>"));
        assert!(out.contains("<Ref>RF18539007547034</Ref>"));
        assert!(!out.contains("<Ustrd>"), "structured excludes unstructured");
    }

    #[test]
    fn unstructured_remittance_xml_is_escaped_and_transliterated() {
        let mut out = String::new();
        RemittanceInfo::unstructured("Zahlung für Müller & Co")
            .write_xml(&mut out, "  ", crate::validate::CharsetPolicy::default())
            .unwrap();
        assert!(out.contains("<Ustrd>Zahlung fuer Mueller + Co</Ustrd>"));
        assert!(!out.contains("<Strd>"));
    }

    #[test]
    fn proprietary_reference_omits_the_iso_issuer_marker() {
        let mut out = String::new();
        RemittanceInfo::Proprietary {
            reference: "+++090/9337/55493+++".replace(['+', '/'], ""),
            issuer: Some("BE".to_owned()),
        }
        .write_xml(&mut out, "  ", crate::validate::CharsetPolicy::default())
        .unwrap();
        assert!(out.contains("<Cd>SCOR</Cd>"));
        assert!(out.contains("<Issr>BE</Issr>"));
        assert!(
            !out.contains("<Issr>ISO</Issr>"),
            "ISO marks a genuine ISO 11649 reference only"
        );
    }

    #[test]
    fn remittance_validation_enforces_epc_limits() {
        assert!(
            RemittanceInfo::unstructured("A".repeat(140))
                .validate("RmtInf/Ustrd")
                .is_ok()
        );
        assert!(matches!(
            RemittanceInfo::unstructured("A".repeat(141)).validate("RmtInf/Ustrd"),
            Err(ValidationError::TooLong { .. })
        ));
        assert!(matches!(
            RemittanceInfo::unstructured("").validate("RmtInf/Ustrd"),
            Err(ValidationError::Empty { .. })
        ));
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_roundtrip_validates() {
        let rf = RfReference::generate("2348231").unwrap();
        let json = serde_json::to_string(&rf).unwrap();
        assert_eq!(json, r#""RF712348231""#);
        assert_eq!(serde_json::from_str::<RfReference>(&json).unwrap(), rf);
        // A bad checksum must not deserialise.
        assert!(serde_json::from_str::<RfReference>(r#""RF722348231""#).is_err());
    }
}
