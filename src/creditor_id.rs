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
//! | Check digits | 2 | Mod-97 (same algorithm as IBAN) |
//! | Creditor Business Code | 3 | Alphanumeric (usually `ZZZ`) |
//! | National Identifier | 1–28 | Alphanumeric, country-specific |
//!
//! ## Check digit algorithm
//!
//! Identical to IBAN ISO 13616: move CC + check_digits to end, expand letters
//! to decimals (A=10…Z=35), compute mod 97 — result must be 1.
//!
//! ## Examples
//!
//! ```
//! use sepa::creditor_id::validate_creditor_id;
//!
//! assert!(validate_creditor_id("DE74ZZZ09999999999").is_ok());
//! assert!(validate_creditor_id("FR20ZZZ123456").is_ok());
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

    /// Mod-97 check digit did not produce remainder 1.
    #[error("Creditor Identifier check digit mismatch (mod97 = {remainder}, expected 1)")]
    InvalidChecksum {
        /// The actual mod-97 remainder.
        remainder: u64,
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
/// assert!(validate_creditor_id("DE74ZZZ09999999999").is_ok());
/// assert!(validate_creditor_id("de74zzz09999999999").is_ok()); // normalised to uppercase
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

    // Mod-97 check: rearrange = business_code + national_id + CC + check_digits
    // (same algorithm as IBAN ISO 13616)
    let rearranged = format!("{}{}", &normalised[4..], &normalised[..4]);
    let numeric: String = rearranged
        .chars()
        .flat_map(|c| {
            if c.is_ascii_alphabetic() {
                let n = (c as u8 - b'A' + 10).to_string();
                n.chars().collect::<Vec<_>>()
            } else {
                vec![c]
            }
        })
        .collect();

    let mut remainder: u64 = 0;
    for ch in numeric.chars() {
        let digit = ch
            .to_digit(10)
            .ok_or(CreditorIdError::InvalidCharacter { ch })?;
        remainder = (remainder * 10 + u64::from(digit)) % 97;
    }

    if remainder == 1 {
        Ok(CreditorId(normalised))
    } else {
        Err(CreditorIdError::InvalidChecksum { remainder })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn de_creditor_id_valid() {
        assert!(validate_creditor_id("DE74ZZZ09999999999").is_ok());
    }

    #[test]
    fn lowercase_normalised() {
        let ci = validate_creditor_id("de74zzz09999999999").unwrap();
        assert_eq!(ci.as_str(), "DE74ZZZ09999999999");
    }

    #[test]
    fn parts() {
        let ci = validate_creditor_id("DE74ZZZ09999999999").unwrap();
        assert_eq!(ci.country_code(), "DE");
        assert_eq!(ci.check_digits(), "74");
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
        let ci: CreditorId = "DE74ZZZ09999999999".parse().unwrap();
        assert_eq!(ci.country_code(), "DE");
    }

    #[test]
    fn into_string() {
        let ci = validate_creditor_id("DE74ZZZ09999999999").unwrap();
        let s: String = ci.into();
        assert_eq!(s, "DE74ZZZ09999999999");
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_roundtrip() {
        let ci = validate_creditor_id("DE74ZZZ09999999999").unwrap();
        let json = serde_json::to_string(&ci).unwrap();
        assert_eq!(json, r#""DE74ZZZ09999999999""#);
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
