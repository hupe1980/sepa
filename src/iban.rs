//! IBAN validation — ISO 13616-1:2007 mod-97 algorithm.
//!
//! Validates IBANs from any country following the ISO 13616 standard.
//! Commonly used in SEPA payments (eurozone) and internationally.
//!
//! ## Algorithm (ISO 13616-1 §5.3)
//!
//! 1. Remove whitespace, convert to uppercase.
//! 2. Check length: 15–34 characters.
//! 3. Move first 4 characters to the end.
//! 4. Replace each letter with its numeric value: A=10, B=11, …, Z=35.
//! 5. Compute the resulting large integer modulo 97.
//! 6. Valid if result == 1.
//!
//! ## Examples
//!
//! ```
//! use sepa::iban::{validate_iban, IbanError};
//!
//! assert!(validate_iban("DE89 3704 0044 0532 0130 00").is_ok());
//! assert!(validate_iban("NL91ABNA0417164300").is_ok());
//!
//! let err = validate_iban("DE89370400440532013001").unwrap_err();
//! assert!(matches!(err, IbanError::InvalidChecksum { .. }));
//!
//! // FromStr / parse
//! let iban: sepa::Iban = "DE89370400440532013000".parse().unwrap();
//! assert_eq!(iban.country_code(), "DE");
//! ```

use std::str::FromStr;

/// A validated IBAN.  Created only via [`validate_iban`] or [`Iban::from_str`].
///
/// The inner value is normalised: whitespace removed, uppercased.
/// Cannot be forged — all constructors validate the mod-97 checksum.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Iban(String);

impl Iban {
    /// The normalised IBAN string (whitespace-stripped, uppercase).
    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Two-letter ISO 3166-1 country code (first 2 characters of the IBAN).
    #[inline]
    #[must_use]
    pub fn country_code(&self) -> &str {
        &self.0[..2]
    }

    /// Check digits (characters 3–4 of the IBAN).
    #[inline]
    #[must_use]
    pub fn check_digits(&self) -> &str {
        &self.0[2..4]
    }

    /// BBAN — Basic Bank Account Number: everything after the 4-character header.
    ///
    /// For `DE89370400440532013000` this is `370400440532013000`.
    #[inline]
    #[must_use]
    pub fn bban(&self) -> &str {
        &self.0[4..]
    }
}

impl std::fmt::Display for Iban {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Display in groups of 4: "DE89 3704 0044 ..."
        let s = &self.0;
        let mut first = true;
        for chunk in s.as_bytes().chunks(4) {
            if !first {
                write!(f, " ")?;
            }
            first = false;
            f.write_str(std::str::from_utf8(chunk).unwrap_or(""))?;
        }
        Ok(())
    }
}

impl AsRef<str> for Iban {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::ops::Deref for Iban {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}

impl std::borrow::Borrow<str> for Iban {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl From<Iban> for String {
    fn from(iban: Iban) -> Self {
        iban.0
    }
}

impl FromStr for Iban {
    type Err = IbanError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        validate_iban(s)
    }
}

impl TryFrom<&str> for Iban {
    type Error = IbanError;
    fn try_from(s: &str) -> Result<Self, Self::Error> {
        validate_iban(s)
    }
}

impl TryFrom<String> for Iban {
    type Error = IbanError;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        validate_iban(&s)
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for Iban {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for Iban {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        validate_iban(&s).map_err(serde::de::Error::custom)
    }
}

// ── Error ─────────────────────────────────────────────────────────────────────

/// Error returned when IBAN validation fails.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum IbanError {
    /// IBAN length is outside the valid range 15–34.
    #[error("IBAN length {len} is outside the valid range 15–34")]
    InvalidLength {
        /// The actual length that was rejected.
        len: usize,
    },

    /// IBAN contains a character that is not alphanumeric.
    #[error("IBAN contains invalid character: {ch:?}")]
    InvalidCharacter {
        /// The offending character.
        ch: char,
    },

    /// Mod-97 checksum did not produce the expected remainder of 1.
    #[error("IBAN checksum mismatch (mod97 = {remainder}, expected 1)")]
    InvalidChecksum {
        /// The actual mod-97 remainder.
        remainder: u64,
    },

    /// IBAN length is inconsistent with the country code (ISO 13616 registry).
    #[error("IBAN for country {country} must be {expected} characters, got {actual}")]
    WrongLengthForCountry {
        /// The two-letter country code.
        country: String,
        /// Expected length per ISO 13616 registry.
        expected: usize,
        /// The actual length of the input.
        actual: usize,
    },
}

// ── Country-length registry (ISO 13616-1) ────────────────────────────────────

/// Return the expected IBAN length for a given 2-letter country code,
/// or `None` for countries not yet in the ISO 13616 registry.
///
/// Source: SWIFT IBAN Registry (updated periodically).
#[must_use]
#[allow(clippy::match_same_arms, reason = "a data table, not control flow")]
pub fn iban_country_length(country: &str) -> Option<usize> {
    // Sorted by country code for readability.
    match country {
        "AD" => Some(24),
        "AE" => Some(23),
        "AL" => Some(28),
        "AT" => Some(20),
        "AZ" => Some(28),
        "BA" => Some(20),
        "BE" => Some(16),
        "BG" => Some(22),
        "BH" => Some(22),
        "BI" => Some(27),
        "BR" => Some(29),
        "BY" => Some(28),
        "CH" => Some(21),
        "CR" => Some(22),
        "CY" => Some(28),
        "CZ" => Some(24),
        "DE" => Some(22),
        "DJ" => Some(27),
        "DK" => Some(18),
        "DO" => Some(28),
        "EE" => Some(20),
        "EG" => Some(29),
        "ES" => Some(24),
        "FI" => Some(18),
        "FK" => Some(18),
        "FO" => Some(18),
        "FR" => Some(27),
        "GB" => Some(22),
        "GE" => Some(22),
        "GI" => Some(23),
        "GL" => Some(18),
        "GR" => Some(27),
        "GT" => Some(28),
        "HN" => Some(28),
        "HR" => Some(21),
        "HU" => Some(28),
        "IE" => Some(22),
        "IL" => Some(23),
        "IQ" => Some(23),
        "IS" => Some(26),
        "IT" => Some(27),
        "JO" => Some(30),
        "KW" => Some(30),
        "KZ" => Some(20),
        "LB" => Some(28),
        "LC" => Some(32),
        "LI" => Some(21),
        "LT" => Some(20),
        "LU" => Some(20),
        "LV" => Some(21),
        "LY" => Some(25),
        "MC" => Some(27),
        "MD" => Some(24),
        "ME" => Some(22),
        "MK" => Some(19),
        "MN" => Some(20),
        "MR" => Some(27),
        "MT" => Some(31),
        "MU" => Some(30),
        "NI" => Some(28),
        "NL" => Some(18),
        "NO" => Some(15),
        "OM" => Some(23),
        "PK" => Some(24),
        "PL" => Some(28),
        "PS" => Some(29),
        "PT" => Some(25),
        "QA" => Some(29),
        "RO" => Some(24),
        "RS" => Some(22),
        "RU" => Some(33),
        "SA" => Some(24),
        "SC" => Some(31),
        "SD" => Some(18),
        "SE" => Some(24),
        "SI" => Some(19),
        "SK" => Some(24),
        "SM" => Some(27),
        "SO" => Some(23),
        "ST" => Some(25),
        "SV" => Some(28),
        "TL" => Some(23),
        "TN" => Some(24),
        "TR" => Some(26),
        "UA" => Some(29),
        "VA" => Some(22),
        "VG" => Some(24),
        "XK" => Some(20),
        "YE" => Some(30),
        _ => None,
    }
}

/// Country codes in the SEPA scheme area (42 entries).
///
/// Source: EPC409-09 "EPC List of SEPA Scheme Countries" v8.0 (24 December 2025).
///
/// These are **IBAN** country codes, not geographic ISO 3166 codes. Two
/// distinctions bite in practice:
///
/// - **Faroe Islands (`FO`) and Greenland (`GL`) are *not* in SEPA**, despite
///   being Danish and holding their own IBAN country codes. Gibraltar (`GI`)
///   *is* in SEPA, despite being a UK territory.
/// - SEPA is **not** the eurozone. `DK`, `SE`, `PL`, `CZ`, `HU`, `RO`, `BG`,
///   `GB`, `CH`, `NO`, `IS`, `AL`, `MD`, `MK` and `RS` are SEPA countries with
///   non-EUR national currencies. SEPA membership never implies EUR.
const SEPA_COUNTRIES: [&str; 42] = [
    "AD", "AL", "AT", "BE", "BG", "CH", "CY", "CZ", "DE", "DK", "EE", "ES", "FI", "FR", "GB", "GI",
    "GR", "HR", "HU", "IE", "IS", "IT", "LI", "LT", "LU", "LV", "MC", "MD", "ME", "MK", "MT", "NL",
    "NO", "PL", "PT", "RO", "RS", "SE", "SI", "SK", "SM", "VA",
];

/// Returns `true` when `country` is an IBAN country code inside the SEPA scheme area.
///
/// Takes the two-letter **IBAN** country code — the first two characters of an
/// IBAN, e.g. [`Iban::country_code`]. Comparison is case-insensitive.
///
/// Source: EPC409-09 v8.0 (24 December 2025), which added Albania, Moldova,
/// Montenegro, North Macedonia and Serbia.
///
/// # Examples
///
/// ```
/// use sepa::iban::is_sepa_country;
///
/// assert!(is_sepa_country("DE"));
/// assert!(is_sepa_country("gi")); // Gibraltar is SEPA
/// assert!(is_sepa_country("RS")); // Serbia joined in 2025
///
/// // SEPA is not the eurozone — these use their own currencies:
/// assert!(is_sepa_country("SE"));
/// assert!(is_sepa_country("CH"));
///
/// // Danish territories with their own IBAN codes are NOT in SEPA:
/// assert!(!is_sepa_country("FO"));
/// assert!(!is_sepa_country("GL"));
/// assert!(!is_sepa_country("US"));
/// ```
#[must_use]
pub fn is_sepa_country(country: &str) -> bool {
    let mut buf = [0u8; 2];
    if country.len() != 2 || !country.is_ascii() {
        return false;
    }
    buf.copy_from_slice(country.as_bytes());
    buf.make_ascii_uppercase();
    let upper = std::str::from_utf8(&buf).unwrap_or("");
    SEPA_COUNTRIES.contains(&upper)
}

impl Iban {
    /// Returns `true` when this IBAN's country is in the SEPA scheme area.
    ///
    /// See [`is_sepa_country`] — note that SEPA membership does **not** imply
    /// the account is denominated in EUR.
    ///
    /// # Examples
    ///
    /// ```
    /// use sepa::validate_iban;
    ///
    /// assert!(validate_iban("DE89370400440532013000").unwrap().is_sepa());
    /// ```
    #[inline]
    #[must_use]
    pub fn is_sepa(&self) -> bool {
        is_sepa_country(self.country_code())
    }
}

// ── Validation ────────────────────────────────────────────────────────────────

/// Validate an IBAN using the ISO 13616 mod-97 algorithm.
///
/// Accepts IBANs with or without spaces.  Input is uppercased before validation.
/// Returns the normalised [`Iban`] on success.
///
/// Validation order:
/// 1. Strip whitespace, uppercase.
/// 2. Check overall length range (15–34).
/// 3. Check all characters are ASCII alphanumeric.
/// 4. Check country-specific length against the ISO 13616 registry
///    (only for known country codes; unknown countries skip this step).
/// 5. Compute mod-97 checksum — must equal 1.
///
/// # Errors
///
/// | Error | Condition |
/// |---|---|
/// | [`IbanError::InvalidLength`] | Length outside 15–34 |
/// | [`IbanError::InvalidCharacter`] | Non-alphanumeric character |
/// | [`IbanError::WrongLengthForCountry`] | Length wrong for known country |
/// | [`IbanError::InvalidChecksum`] | Mod-97 remainder ≠ 1 |
///
/// # Examples
///
/// ```
/// use sepa::validate_iban;
/// use sepa::iban::IbanError;
///
/// let iban = validate_iban("DE89 3704 0044 0532 0130 00").unwrap();
/// assert_eq!(iban.as_str(), "DE89370400440532013000");
/// assert_eq!(iban.bban(), "370400440532013000");
///
/// // Too short for Germany (DE IBANs are exactly 22 chars; this is 19)
/// assert!(matches!(
///     validate_iban("DE891234567890123456"),
///     Err(IbanError::WrongLengthForCountry { .. })
/// ));
/// ```
#[must_use = "ignoring a validated IBAN loses the result"]
pub fn validate_iban(raw: &str) -> Result<Iban, IbanError> {
    let normalised: String = raw
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| c.to_ascii_uppercase())
        .collect();

    let len = normalised.len();
    if !(15..=34).contains(&len) {
        return Err(IbanError::InvalidLength { len });
    }

    for c in normalised.chars() {
        if !c.is_ascii_alphanumeric() {
            return Err(IbanError::InvalidCharacter { ch: c });
        }
    }

    // Country-specific length check (step 4)
    let country = &normalised[..2];
    if let Some(expected) = iban_country_length(country) {
        if len != expected {
            return Err(IbanError::WrongLengthForCountry {
                country: country.to_owned(),
                expected,
                actual: len,
            });
        }
    }

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

    // Rolling mod-97 using groups of digits to avoid u64 overflow
    let mut remainder: u64 = 0;
    for ch in numeric.chars() {
        let digit = ch.to_digit(10).ok_or(IbanError::InvalidCharacter { ch })?;
        remainder = (remainder * 10 + u64::from(digit)) % 97;
    }

    if remainder == 1 {
        Ok(Iban(normalised))
    } else {
        Err(IbanError::InvalidChecksum { remainder })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn de_iban_with_spaces() {
        let iban = validate_iban("DE89 3704 0044 0532 0130 00").unwrap();
        assert_eq!(iban.as_str(), "DE89370400440532013000");
        assert_eq!(iban.country_code(), "DE");
        assert_eq!(iban.check_digits(), "89");
    }

    #[test]
    fn de_iban_no_spaces() {
        assert!(validate_iban("DE89370400440532013000").is_ok());
    }

    #[test]
    fn de_iban_sparkasse() {
        assert!(validate_iban("DE29100500005001065004").is_ok());
    }

    #[test]
    fn nl_iban() {
        assert!(validate_iban("NL91ABNA0417164300").is_ok());
    }

    #[test]
    fn gb_iban() {
        assert!(validate_iban("GB29 NWBK 6016 1331 9268 19").is_ok());
    }

    #[test]
    fn at_iban() {
        assert!(validate_iban("AT611904300234573201").is_ok());
    }

    #[test]
    fn ch_iban() {
        assert!(validate_iban("CH5604835012345678009").is_ok());
    }

    #[test]
    fn lowercase_normalised() {
        assert!(validate_iban("de89370400440532013000").is_ok());
    }

    #[test]
    fn wrong_checksum() {
        let err = validate_iban("DE89370400440532013001").unwrap_err();
        assert!(matches!(err, IbanError::InvalidChecksum { .. }));
    }

    #[test]
    fn bban_accessor() {
        let iban = validate_iban("DE89370400440532013000").unwrap();
        assert_eq!(iban.bban(), "370400440532013000");
    }

    #[test]
    fn wrong_length_for_de() {
        // DE IBANs are exactly 22 chars; 20 chars fails country-length check
        let err = validate_iban("DE89370400440532013").unwrap_err();
        assert!(matches!(
            err,
            IbanError::WrongLengthForCountry {
                expected: 22,
                actual: 19,
                ..
            }
        ));
    }

    #[test]
    fn wrong_length_for_nl() {
        // NL IBANs are exactly 18 chars
        let err = validate_iban("NL91ABNA041716430099").unwrap_err();
        assert!(matches!(
            err,
            IbanError::WrongLengthForCountry { expected: 18, .. }
        ));
    }

    #[test]
    fn unknown_country_skips_length_check() {
        // XX is not in the registry — only mod-97 applies
        // Construct a valid mod-97 XX IBAN (20 chars, valid checksum)
        // We just verify no WrongLengthForCountry is returned for an unknown country.
        let result = validate_iban("XX89370400440532013000");
        assert!(!matches!(
            result,
            Err(IbanError::WrongLengthForCountry { .. })
        ));
    }

    #[test]
    fn latvia_is_registered() {
        // Regression: LV sits between LU and LV alphabetically and was missing
        // from the table, so every Latvian IBAN failed validation.
        assert_eq!(iban_country_length("LV"), Some(21));
        assert!(validate_iban("LV80BANK0000435195001").is_ok());
    }

    #[test]
    fn registry_has_full_swift_entry_count() {
        // SWIFT IBAN Registry currently lists 89 countries/territories. Bump
        // this deliberately when the registry changes — it catches silent drift.
        let count = (b'A'..=b'Z')
            .flat_map(|a| (b'A'..=b'Z').map(move |b| [a, b]))
            .filter(|cc| iban_country_length(std::str::from_utf8(cc).unwrap()).is_some())
            .count();
        assert_eq!(count, 89, "IBAN country registry entry count drifted");
    }

    #[test]
    fn sepa_membership() {
        assert!(is_sepa_country("DE"));
        assert!(is_sepa_country("gi")); // case-insensitive; Gibraltar is SEPA
        assert!(is_sepa_country("RS")); // added by EPC409-09 v8.0
        // Danish territories with their own IBAN codes are NOT in SEPA
        assert!(!is_sepa_country("FO"));
        assert!(!is_sepa_country("GL"));
        assert!(!is_sepa_country("XK")); // Kosovo: in the registry, not in SEPA
        assert!(!is_sepa_country("US"));
        assert!(!is_sepa_country("D")); // malformed input must not panic
        assert!(!is_sepa_country("DEU"));
        assert!(!is_sepa_country("Ü!"));
        assert!(validate_iban("DE89370400440532013000").unwrap().is_sepa());
    }

    #[test]
    fn every_sepa_country_is_in_the_length_registry() {
        for cc in SEPA_COUNTRIES {
            assert!(
                iban_country_length(cc).is_some(),
                "{cc} is a SEPA country but has no registry length"
            );
        }
    }

    #[test]
    fn territories_are_not_iban_prefixes() {
        // French collectivities use FR IBANs; Crown Dependencies use GB IBANs.
        // None of these are IBAN country codes in their own right.
        for t in [
            "GP", "MQ", "RE", "YT", "GF", "BL", "MF", "PM", "PF", "TF", "NC", "WF", "JE", "GG",
            "IM",
        ] {
            assert_eq!(
                iban_country_length(t),
                None,
                "{t} is a territory, not an IBAN country code"
            );
        }
    }

    #[test]
    fn country_length_registry_spot_checks() {
        use super::iban_country_length;
        assert_eq!(iban_country_length("DE"), Some(22));
        assert_eq!(iban_country_length("NL"), Some(18));
        assert_eq!(iban_country_length("GB"), Some(22));
        assert_eq!(iban_country_length("FR"), Some(27));
        assert_eq!(iban_country_length("NO"), Some(15));
        assert_eq!(iban_country_length("XX"), None);
    }

    #[test]
    fn too_short() {
        assert!(matches!(
            validate_iban("DE89").unwrap_err(),
            IbanError::InvalidLength { len: 4 }
        ));
    }

    #[test]
    fn too_long() {
        let long = "DE".to_string() + &"1".repeat(33);
        assert!(matches!(
            validate_iban(&long).unwrap_err(),
            IbanError::InvalidLength { len: 35 }
        ));
    }

    #[test]
    fn empty() {
        assert!(matches!(
            validate_iban("").unwrap_err(),
            IbanError::InvalidLength { len: 0 }
        ));
    }

    #[test]
    fn special_chars_rejected() {
        assert!(matches!(
            validate_iban("DE89@70400440532013000").unwrap_err(),
            IbanError::InvalidCharacter { ch: '@' }
        ));
    }

    #[test]
    fn display_groups_of_four() {
        let iban = validate_iban("DE89370400440532013000").unwrap();
        assert_eq!(iban.to_string(), "DE89 3704 0044 0532 0130 00");
    }

    #[test]
    fn only_whitespace_rejected() {
        assert!(matches!(
            validate_iban("   ").unwrap_err(),
            IbanError::InvalidLength { len: 0 }
        ));
    }

    #[test]
    fn minimum_valid_length_norway() {
        assert!(validate_iban("NO9386011117947").is_ok());
    }

    #[test]
    fn from_str() {
        let iban: Iban = "DE89370400440532013000".parse().unwrap();
        assert_eq!(iban.as_str(), "DE89370400440532013000");
    }

    #[test]
    fn try_from_str() {
        let iban = Iban::try_from("DE89370400440532013000").unwrap();
        assert_eq!(iban.country_code(), "DE");
    }

    #[test]
    fn try_from_string() {
        let iban = Iban::try_from("DE89370400440532013000".to_owned()).unwrap();
        assert_eq!(iban.as_str(), "DE89370400440532013000");
    }

    #[test]
    fn into_string() {
        let iban = validate_iban("DE89370400440532013000").unwrap();
        let s: String = iban.into();
        assert_eq!(s, "DE89370400440532013000");
    }

    #[test]
    fn ord() {
        let a = validate_iban("AT611904300234573201").unwrap();
        let b = validate_iban("DE89370400440532013000").unwrap();
        assert!(a < b); // "AT..." < "DE..."
    }

    #[test]
    fn deref_to_str() {
        let iban = validate_iban("DE89370400440532013000").unwrap();
        assert_eq!(iban.len(), 22); // Deref to str
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_roundtrip() {
        let iban = validate_iban("DE89370400440532013000").unwrap();
        let json = serde_json::to_string(&iban).unwrap();
        assert_eq!(json, r#""DE89370400440532013000""#);
        let back: Iban = serde_json::from_str(&json).unwrap();
        assert_eq!(back, iban);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_invalid_rejected() {
        let result: Result<Iban, _> = serde_json::from_str(r#""NOTANIBAN""#);
        assert!(result.is_err());
    }
}
