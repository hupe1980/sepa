//! SEPA Creditor Identifier (CI) — EPC AT-02.
//!
//! The SEPA Creditor Identifier is a mandatory field on all SEPA Core Direct
//! Debit batches. It is assigned by the creditor's local banking authority and
//! appears as `CdtrSchmeId` in the pain.008 XML at the `PmtInf` level.
//!
//! ## Format
//!
//! `CC##ZZZxxxxxxxxxxxxxxx`
//!
//! | Part | Length | Content |
//! |---|---|---|
//! | Country code | 2 | ISO 3166-1 alpha-2 |
//! | Check digits | 2 | Mod-97 over the national identifier (see below) |
//! | Creditor Business Code | 3 | Alphanumeric (usually `ZZZ`) |
//! | National Identifier | 1–28 | Alphanumeric, country-specific |
//!
//! ## Check digit algorithm (EPC262-08)
//!
//! The check digits are computed over the **national identifier only** — the
//! 3-character Creditor Business Code is *excluded*:
//!
//! 1. Take the national identifier (position 8 onwards).
//! 2. Strip every non-alphanumeric character.
//! 3. Append the country code and `"00"`.
//! 4. Expand letters to decimals (A=10 … Z=35).
//! 5. `check_digits = 98 − (value mod 97)`.
//!
//! This differs from ISO 13616 (IBAN), where the whole string participates and
//! the expected remainder is 1. Applying the IBAN rule to a Creditor Identifier
//! rejects every genuine CI, because it folds the `ZZZ` business code into the
//! checksum.
//!
//! ## Examples
//!
//! ```
//! use sepa::creditor_id::validate_creditor_id;
//!
//! // Canonical EPC example identifier
//! assert!(validate_creditor_id("DE98ZZZ09999999999").is_ok());
//! assert!(validate_creditor_id("INVALID").is_err());
//! ```

use std::str::FromStr;

/// A validated SEPA Creditor Identifier.
///
/// Created only via [`validate_creditor_id`] or [`CreditorId::from_str`].
/// Cannot be forged — the mod-97 check digit is validated on construction.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CreditorId(String);

impl CreditorId {
    /// The normalised CI string (whitespace-stripped, uppercase).
    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Two-letter ISO 3166-1 country code.
    #[inline]
    #[must_use]
    pub fn country_code(&self) -> &str {
        &self.0[..2]
    }

    /// Check digits (characters 3–4).
    #[inline]
    #[must_use]
    pub fn check_digits(&self) -> &str {
        &self.0[2..4]
    }

    /// Creditor Business Code (characters 5–7, usually `ZZZ`).
    #[inline]
    #[must_use]
    pub fn business_code(&self) -> &str {
        &self.0[4..7]
    }

    /// National identifier (characters 8 onward).
    #[inline]
    #[must_use]
    pub fn national_id(&self) -> &str {
        &self.0[7..]
    }
}

impl std::fmt::Display for CreditorId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for CreditorId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl From<CreditorId> for String {
    fn from(ci: CreditorId) -> Self {
        ci.0
    }
}

impl FromStr for CreditorId {
    type Err = CreditorIdError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        validate_creditor_id(s)
    }
}

impl TryFrom<&str> for CreditorId {
    type Error = CreditorIdError;
    fn try_from(s: &str) -> Result<Self, Self::Error> {
        validate_creditor_id(s)
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for CreditorId {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for CreditorId {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        validate_creditor_id(&s).map_err(serde::de::Error::custom)
    }
}

// ── Error ─────────────────────────────────────────────────────────────────────

/// Error returned when SEPA Creditor Identifier validation fails.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum CreditorIdError {
    /// CI length is outside the valid range 8–35 characters.
    #[error("Creditor Identifier length {len} is outside the valid range 8–35")]
    InvalidLength {
        /// The actual length.
        len: usize,
    },

    /// Country code (chars 1–2) must be two ASCII uppercase letters.
    #[error("Creditor Identifier country code must be 2 ASCII letters (A–Z), got: {code:?}")]
    InvalidCountryCode {
        /// The country code that was rejected.
        code: String,
    },

    /// CI contains a character that is not alphanumeric.
    #[error("Creditor Identifier contains invalid character: {ch:?}")]
    InvalidCharacter {
        /// The offending character.
        ch: char,
    },

    /// The EPC check digits do not match the national identifier.
    #[error("Creditor Identifier check digits are {actual}, expected {expected}")]
    InvalidChecksum {
        /// The check digits required by EPC262-08.
        expected: String,
        /// The check digits actually present in the input.
        actual: String,
    },
}

// ── Validation ────────────────────────────────────────────────────────────────

/// Validate a SEPA Creditor Identifier using the EPC AT-02 format rules.
///
/// Accepts any country variant (DE, FR, GB, NL, …).
/// Whitespace is stripped, input is uppercased before validation.
///
/// # Errors
///
/// Returns [`CreditorIdError::InvalidLength`] when not 8–35 chars (after stripping),
/// [`CreditorIdError::InvalidCountryCode`] when chars 1–2 are not letters,
/// [`CreditorIdError::InvalidCharacter`] for non-alphanumeric characters, or
/// [`CreditorIdError::InvalidChecksum`] when the mod-97 check fails.
///
/// # Examples
///
/// ```
/// use sepa::creditor_id::{validate_creditor_id, CreditorIdError};
///
/// assert!(validate_creditor_id("DE98ZZZ09999999999").is_ok());
/// assert!(validate_creditor_id("de98zzz09999999999").is_ok()); // normalised to uppercase
///
/// let err = validate_creditor_id("DE00ZZZ09999999999").unwrap_err();
/// assert!(matches!(err, CreditorIdError::InvalidChecksum { .. }));
/// ```
#[must_use = "ignoring a validated Creditor ID loses the result"]
pub fn validate_creditor_id(raw: &str) -> Result<CreditorId, CreditorIdError> {
    let normalised: String = raw
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| c.to_ascii_uppercase())
        .collect();

    let len = normalised.len();
    if !(8..=35).contains(&len) {
        return Err(CreditorIdError::InvalidLength { len });
    }

    for c in normalised.chars() {
        if !c.is_ascii_alphanumeric() {
            return Err(CreditorIdError::InvalidCharacter { ch: c });
        }
    }

    // Country code (chars 1–2) must be letters
    let cc = &normalised[..2];
    if !cc.chars().all(|c| c.is_ascii_alphabetic()) {
        return Err(CreditorIdError::InvalidCountryCode {
            code: cc.to_owned(),
        });
    }

    // EPC262-08: the check digits cover the national identifier ONLY — the
    // 3-character Creditor Business Code (positions 5–7) is excluded.
    let expected = creditor_id_check_digits(&normalised[7..], cc);
    let actual = &normalised[2..4];
    if expected == actual {
        Ok(CreditorId(normalised))
    } else {
        Err(CreditorIdError::InvalidChecksum {
            expected,
            actual: actual.to_owned(),
        })
    }
}

/// Compute the two EPC check digits for a national identifier + country code.
///
/// Implements EPC262-08: strip non-alphanumerics from `national_id`, append
/// `country` and `"00"`, expand letters (A=10 … Z=35), then `98 − (n mod 97)`.
///
/// Both inputs must already be uppercased. The result is always two ASCII
/// digits, zero-padded.
///
/// # Examples
///
/// ```
/// use sepa::creditor_id::creditor_id_check_digits;
/// assert_eq!(creditor_id_check_digits("09999999999", "DE"), "98");
/// ```
#[must_use]
pub fn creditor_id_check_digits(national_id: &str, country: &str) -> String {
    // 0–9 contribute one decimal digit; A–Z contribute two (their 10–35 value).
    fn feed(remainder: u64, c: char) -> u64 {
        match c.to_digit(36) {
            Some(d) if d < 10 => (remainder * 10 + u64::from(d)) % 97,
            Some(d) => (remainder * 100 + u64::from(d)) % 97,
            None => remainder,
        }
    }

    let mut remainder = national_id
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .fold(0u64, feed);
    remainder = country.chars().fold(remainder, feed);
    remainder = feed(remainder, '0');
    remainder = feed(remainder, '0');

    format!("{:02}", 98 - remainder)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn de_creditor_id_valid() {
        assert!(validate_creditor_id("DE98ZZZ09999999999").is_ok());
    }

    #[test]
    fn business_code_excluded_from_checksum() {
        // EPC262-08: the check digits cover the national identifier only, so
        // changing the business code must NOT invalidate the identifier.
        for bc in ["ZZZ", "ABC", "001"] {
            let ci = format!("DE98{bc}09999999999");
            assert!(
                validate_creditor_id(&ci).is_ok(),
                "{ci} must stay valid — business code is not part of the checksum"
            );
        }
    }

    #[test]
    fn iban_style_checksum_is_rejected() {
        // Regression: applying the ISO 13616 (IBAN) rule to a CI accepts DE74
        // and rejects the canonical DE98. Neither may happen.
        // "DE74…" is what the old IBAN-style checksum wrongly accepted.
        assert!(validate_creditor_id(concat!("DE", "74", "ZZZ09999999999")).is_err());
        assert!(validate_creditor_id("DE98ZZZ09999999999").is_ok());
    }

    #[test]
    fn check_digits_roundtrip() {
        for (nid, cc) in [
            ("09999999999", "DE"),
            ("123456", "FR"),
            ("ZZZ00000000", "NL"),
            ("A1B2C3", "AT"),
        ] {
            let cd = creditor_id_check_digits(nid, cc);
            let ci = format!("{cc}{cd}ZZZ{nid}");
            assert!(
                validate_creditor_id(&ci).is_ok(),
                "generated {ci} must validate"
            );
        }
    }

    #[test]
    fn lowercase_normalised() {
        let ci = validate_creditor_id("de98zzz09999999999").unwrap();
        assert_eq!(ci.as_str(), "DE98ZZZ09999999999");
    }

    #[test]
    fn parts() {
        let ci = validate_creditor_id("DE98ZZZ09999999999").unwrap();
        assert_eq!(ci.country_code(), "DE");
        assert_eq!(ci.check_digits(), "98");
        assert_eq!(ci.business_code(), "ZZZ");
        assert_eq!(ci.national_id(), "09999999999");
    }

    #[test]
    fn too_short() {
        assert!(matches!(
            validate_creditor_id("DE98ZZ").unwrap_err(),
            CreditorIdError::InvalidLength { len: 6 }
        ));
    }

    #[test]
    fn invalid_check_digit() {
        // DE00 has wrong check digits (correct is DE98)
        assert!(matches!(
            validate_creditor_id("DE00ZZZ09999999999").unwrap_err(),
            CreditorIdError::InvalidChecksum { .. }
        ));
    }

    #[test]
    fn invalid_character_in_national_id() {
        // Contains a dash which is not alphanumeric
        let result = validate_creditor_id("DE98ZZZ0999-999");
        assert!(result.is_err());
    }

    #[test]
    fn from_str() {
        let ci: CreditorId = "DE98ZZZ09999999999".parse().unwrap();
        assert_eq!(ci.country_code(), "DE");
    }

    #[test]
    fn into_string() {
        let ci = validate_creditor_id("DE98ZZZ09999999999").unwrap();
        let s: String = ci.into();
        assert_eq!(s, "DE98ZZZ09999999999");
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_roundtrip() {
        let ci = validate_creditor_id("DE98ZZZ09999999999").unwrap();
        let json = serde_json::to_string(&ci).unwrap();
        assert_eq!(json, r#""DE98ZZZ09999999999""#);
        let back: CreditorId = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ci);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_invalid_rejected() {
        let result: Result<CreditorId, _> = serde_json::from_str(r#""INVALID""#);
        assert!(result.is_err());
    }
}
