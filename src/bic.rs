//! BIC (Bank Identifier Code) validation — ISO 9362.
//!
//! A BIC uniquely identifies a financial institution for SEPA payments.
//! Format: `[A-Z]{6}[A-Z0-9]{2}([A-Z0-9]{3})?`, with the location-code
//! restrictions ISO 9362 adds on top of it, and a country code that has to be a
//! real country — the bare pattern accepts `COBAZZFF`, which addresses nothing.
//!
//! ## Examples
//!
//! ```
//! use sepa::bic::validate_bic;
//!
//! assert!(validate_bic("COBADEFFXXX").is_ok());
//! assert!(validate_bic("DEUTDEDB").is_ok());
//! assert!(validate_bic("NOTPROVIDED").is_err()); // EPC placeholder
//!
//! // parse / FromStr
//! let bic: sepa::Bic = "COBADEFFXXX".parse().unwrap();
//! assert_eq!(bic.country_code(), "DE");
//! ```

use std::str::FromStr;

/// A validated BIC. Created only via [`validate_bic`] or [`Bic::from_str`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Bic(String);

impl Bic {
    /// The BIC string (uppercase, 8 or 11 characters).
    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// 4-letter institution code (chars 1–4).
    #[inline]
    #[must_use]
    pub fn institution_code(&self) -> &str {
        &self.0[..4]
    }

    /// 2-letter ISO 3166-1 country code (chars 5–6).
    #[inline]
    #[must_use]
    pub fn country_code(&self) -> &str {
        &self.0[4..6]
    }

    /// 2-character location code (chars 7–8).
    #[inline]
    #[must_use]
    pub fn location_code(&self) -> &str {
        &self.0[6..8]
    }

    /// 3-character branch code (chars 9–11), or `None` for 8-character BICs.
    #[inline]
    #[must_use]
    pub fn branch_code(&self) -> Option<&str> {
        if self.0.len() == 11 {
            Some(&self.0[8..])
        } else {
            None
        }
    }

    /// Returns `true` if this is a Test & Training BIC.
    ///
    /// A test BIC is marked by the **second** character of the location code —
    /// position 8, 1-indexed — being `'0'`. Such BICs never address a live
    /// institution.
    ///
    /// This is a SWIFT network convention rather than an ISO 9362 rule: the
    /// standard defines only the structure `4!c 2!a 2!c 3!c` and says nothing
    /// about test codes.
    ///
    /// # Examples
    ///
    /// ```
    /// use sepa::validate_bic;
    ///
    /// // Location code "F0" — second character '0' → test BIC
    /// assert!(validate_bic("DEUTDEF0").unwrap().is_test());
    /// // Location code "FF" — second character 'F' → NOT a test BIC
    /// assert!(!validate_bic("COBADEFF").unwrap().is_test());
    /// ```
    #[inline]
    #[must_use]
    pub fn is_test(&self) -> bool {
        self.location_suffix() == Some(b'0')
    }

    /// Returns `true` if the institution is a **passive** SWIFT participant.
    ///
    /// Indicated by `'1'` as the second character of the location code.
    #[inline]
    #[must_use]
    pub fn is_passive(&self) -> bool {
        self.location_suffix() == Some(b'1')
    }

    /// The second character of the location code (position 8), which carries
    /// the test/passive/reverse-billing marker.
    ///
    /// Always `Some` for a constructed [`Bic`] — validation guarantees a length
    /// of 8 or 11 — but expressed fallibly so no code path can index out of
    /// bounds.
    #[inline]
    fn location_suffix(&self) -> Option<u8> {
        self.0.as_bytes().get(7).copied()
    }

    /// Returns `true` if this BIC addresses the institution's primary office.
    ///
    /// True for 8-character BICs (which implicitly mean the head office) and for
    /// 11-character BICs whose branch code is `XXX`.
    #[inline]
    #[must_use]
    pub fn is_primary_office(&self) -> bool {
        matches!(self.branch_code(), None | Some("XXX"))
    }
}

impl std::fmt::Display for Bic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for Bic {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::ops::Deref for Bic {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}

impl std::borrow::Borrow<str> for Bic {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl From<Bic> for String {
    fn from(bic: Bic) -> Self {
        bic.0
    }
}

impl FromStr for Bic {
    type Err = BicError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        validate_bic(s)
    }
}

impl TryFrom<&str> for Bic {
    type Error = BicError;
    fn try_from(s: &str) -> Result<Self, Self::Error> {
        validate_bic(s)
    }
}

impl TryFrom<String> for Bic {
    type Error = BicError;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        validate_bic(&s)
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for Bic {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for Bic {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        validate_bic(&s).map_err(serde::de::Error::custom)
    }
}

// ── Error ─────────────────────────────────────────────────────────────────────

/// Error returned when BIC validation fails.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum BicError {
    /// BIC length is not 8 or 11 characters.
    #[error("BIC length {len} is invalid — must be 8 or 11 characters")]
    InvalidLength {
        /// The actual length that was rejected.
        len: usize,
    },

    /// BIC contains a character outside `[A-Z0-9]`.
    #[error("BIC contains invalid character {ch:?} at position {pos}")]
    InvalidCharacter {
        /// The offending character.
        ch: char,
        /// Zero-based position of the offending character.
        pos: usize,
    },

    /// Characters 5–6 are letters, but not a country code any BIC may carry.
    ///
    /// The pattern check alone accepts `COBAZZFF`; there is no country `ZZ`, so
    /// no institution can be addressed by it.
    #[error("BIC country code {code:?} is not an ISO 3166-1 alpha-2 country")]
    UnknownCountryCode {
        /// The two-letter code that was rejected.
        code: String,
    },

    /// The input is the EPC `"NOTPROVIDED"` placeholder, not a real BIC.
    ///
    /// Use `Option<Bic>` with `None` when the BIC is unknown.
    #[error("\"NOTPROVIDED\" is an EPC placeholder, not a valid BIC — use Option<Bic> with None")]
    Placeholder,
}

// ── Country codes ─────────────────────────────────────────────────────────────

/// Every country code a BIC may carry: the 249 officially assigned
/// ISO 3166-1 alpha-2 codes, plus `XK`.
///
/// `XK` is not an ISO 3166 code — it is user-assigned — but SWIFT issues BICs
/// under it for Kosovo, and it is a registered IBAN country code, so rejecting
/// it would make a Kosovan BIC unusable against a Kosovan IBAN.
///
/// Sorted, so the lookup is a binary search.
static BIC_COUNTRY_CODES: [&[u8; 2]; 250] = [
    b"AD", b"AE", b"AF", b"AG", b"AI", b"AL", b"AM", b"AO", b"AQ", b"AR", b"AS", b"AT", b"AU",
    b"AW", b"AX", b"AZ", b"BA", b"BB", b"BD", b"BE", b"BF", b"BG", b"BH", b"BI", b"BJ", b"BL",
    b"BM", b"BN", b"BO", b"BQ", b"BR", b"BS", b"BT", b"BV", b"BW", b"BY", b"BZ", b"CA", b"CC",
    b"CD", b"CF", b"CG", b"CH", b"CI", b"CK", b"CL", b"CM", b"CN", b"CO", b"CR", b"CU", b"CV",
    b"CW", b"CX", b"CY", b"CZ", b"DE", b"DJ", b"DK", b"DM", b"DO", b"DZ", b"EC", b"EE", b"EG",
    b"EH", b"ER", b"ES", b"ET", b"FI", b"FJ", b"FK", b"FM", b"FO", b"FR", b"GA", b"GB", b"GD",
    b"GE", b"GF", b"GG", b"GH", b"GI", b"GL", b"GM", b"GN", b"GP", b"GQ", b"GR", b"GS", b"GT",
    b"GU", b"GW", b"GY", b"HK", b"HM", b"HN", b"HR", b"HT", b"HU", b"ID", b"IE", b"IL", b"IM",
    b"IN", b"IO", b"IQ", b"IR", b"IS", b"IT", b"JE", b"JM", b"JO", b"JP", b"KE", b"KG", b"KH",
    b"KI", b"KM", b"KN", b"KP", b"KR", b"KW", b"KY", b"KZ", b"LA", b"LB", b"LC", b"LI", b"LK",
    b"LR", b"LS", b"LT", b"LU", b"LV", b"LY", b"MA", b"MC", b"MD", b"ME", b"MF", b"MG", b"MH",
    b"MK", b"ML", b"MM", b"MN", b"MO", b"MP", b"MQ", b"MR", b"MS", b"MT", b"MU", b"MV", b"MW",
    b"MX", b"MY", b"MZ", b"NA", b"NC", b"NE", b"NF", b"NG", b"NI", b"NL", b"NO", b"NP", b"NR",
    b"NU", b"NZ", b"OM", b"PA", b"PE", b"PF", b"PG", b"PH", b"PK", b"PL", b"PM", b"PN", b"PR",
    b"PS", b"PT", b"PW", b"PY", b"QA", b"RE", b"RO", b"RS", b"RU", b"RW", b"SA", b"SB", b"SC",
    b"SD", b"SE", b"SG", b"SH", b"SI", b"SJ", b"SK", b"SL", b"SM", b"SN", b"SO", b"SR", b"SS",
    b"ST", b"SV", b"SX", b"SY", b"SZ", b"TC", b"TD", b"TF", b"TG", b"TH", b"TJ", b"TK", b"TL",
    b"TM", b"TN", b"TO", b"TR", b"TT", b"TV", b"TW", b"TZ", b"UA", b"UG", b"UM", b"US", b"UY",
    b"UZ", b"VA", b"VC", b"VE", b"VG", b"VI", b"VN", b"VU", b"WF", b"WS", b"XK", b"YE", b"YT",
    b"ZA", b"ZM", b"ZW",
];

/// Returns `true` when `code` is a country code a BIC may carry.
///
/// Takes the two-letter code — characters 5–6 of a BIC, i.e.
/// [`Bic::country_code`]. Comparison is case-insensitive.
///
/// # Examples
///
/// ```
/// use sepa::bic::is_bic_country_code;
///
/// assert!(is_bic_country_code("DE"));
/// assert!(is_bic_country_code("us"));
/// assert!(is_bic_country_code("XK")); // Kosovo — user-assigned, used by SWIFT
/// assert!(!is_bic_country_code("ZZ"));
/// assert!(!is_bic_country_code("DEU"));
/// ```
#[must_use]
pub fn is_bic_country_code(code: &str) -> bool {
    let bytes = code.as_bytes();
    let [a, b] = bytes else { return false };
    let key = [a.to_ascii_uppercase(), b.to_ascii_uppercase()];
    BIC_COUNTRY_CODES.binary_search(&&key).is_ok()
}

// ── Validation ────────────────────────────────────────────────────────────────

/// Validate a BIC using the ISO 9362 format rules.
///
/// Accepts 8-character (`COBADEFF`) and 11-character (`COBADEFFXXX`) BICs.
/// Uppercases input before validation.
///
/// The EPC `"NOTPROVIDED"` placeholder is explicitly rejected ([`BicError::Placeholder`]).
/// Use `Option<Bic>` with `None` when the debtor's BIC is not known.
///
/// Beyond the character pattern, two structural rules apply. ISO 9362 forbids
/// `0` and `1` as the first character of the location code and `O` as the
/// second; and characters 5–6 must be a country code a BIC may carry, not
/// merely two letters — see [`is_bic_country_code`].
///
/// # Errors
///
/// Returns [`BicError::Placeholder`] for the EPC `"NOTPROVIDED"` sentinel,
/// [`BicError::InvalidLength`] when not 8 or 11 characters,
/// [`BicError::InvalidCharacter`] for any character that violates the SEPA
/// pattern, with the zero-based position of the offender, or
/// [`BicError::UnknownCountryCode`] when characters 5–6 are letters but not a
/// country.
///
/// # Examples
///
/// ```
/// use sepa::bic::{validate_bic, BicError};
///
/// assert!(validate_bic("COBADEFFXXX").is_ok());
/// assert!(validate_bic("DEUTDEDB").is_ok());
///
/// assert!(matches!(
///     validate_bic("COBADEFFXXXX").unwrap_err(),
///     BicError::InvalidLength { len: 12 }
/// ));
/// assert!(matches!(validate_bic("NOTPROVIDED").unwrap_err(), BicError::Placeholder));
///
/// // Matches the pattern, but there is no country ZZ.
/// assert!(matches!(
///     validate_bic("COBAZZFF").unwrap_err(),
///     BicError::UnknownCountryCode { .. }
/// ));
/// ```
#[must_use = "ignoring a validated BIC loses the result"]
pub fn validate_bic(raw: &str) -> Result<Bic, BicError> {
    let upper: String = raw.to_ascii_uppercase();

    // Reject the EPC "NOTPROVIDED" placeholder before length check
    if upper == "NOTPROVIDED" {
        return Err(BicError::Placeholder);
    }

    // Counted in characters: a non-ASCII character is rejected below, but the
    // length reported for one must not be its UTF-8 byte count.
    let len = upper.chars().count();
    if len != 8 && len != 11 {
        return Err(BicError::InvalidLength { len });
    }

    for (pos, ch) in upper.chars().enumerate() {
        let ok = match pos {
            // Institution and country code: letters only.
            0..=5 => ch.is_ascii_uppercase(),
            // Location code, 1st char: no '0' or '1'.
            6 => ch.is_ascii_uppercase() || ('2'..='9').contains(&ch),
            // Location code, 2nd char: digits allowed, but not the letter 'O'.
            7 => (ch.is_ascii_uppercase() && ch != 'O') || ch.is_ascii_digit(),
            // Branch code: alphanumeric.
            _ => ch.is_ascii_alphanumeric(),
        };
        if !ok {
            return Err(BicError::InvalidCharacter { ch, pos });
        }
    }
    // Every character is now ASCII, so byte indices are character indices.

    // The pattern says "two letters"; only a country code addresses a bank.
    let country = &upper[4..6];
    if !is_bic_country_code(country) {
        return Err(BicError::UnknownCountryCode {
            code: country.to_owned(),
        });
    }

    Ok(Bic(upper))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cobadeff_8_char() {
        let bic = validate_bic("COBADEFF").unwrap();
        assert_eq!(bic.institution_code(), "COBA");
        assert_eq!(bic.country_code(), "DE");
        assert_eq!(bic.location_code(), "FF");
        assert!(bic.branch_code().is_none());
    }

    #[test]
    fn cobadeffxxx_11_char() {
        let bic = validate_bic("COBADEFFXXX").unwrap();
        assert_eq!(bic.branch_code(), Some("XXX"));
    }

    #[test]
    fn lowercase_normalised() {
        assert!(validate_bic("cobadeff").is_ok());
    }

    #[test]
    fn too_short() {
        assert!(matches!(
            validate_bic("COBA").unwrap_err(),
            BicError::InvalidLength { len: 4 }
        ));
    }

    #[test]
    fn too_long() {
        assert!(matches!(
            validate_bic("COBADEFFXXXX").unwrap_err(),
            BicError::InvalidLength { len: 12 }
        ));
    }

    #[test]
    fn not_provided_is_placeholder_error() {
        assert!(matches!(
            validate_bic("NOTPROVIDED").unwrap_err(),
            BicError::Placeholder
        ));
    }

    #[test]
    fn iso_only_prefixes_are_rejected_for_sepa() {
        // ISO 9362:2014 types the prefix as 4!c and offers WG11US335AB as an
        // example, but SEPA schemas require [A-Z]{6}, so these must not pass.
        assert!(validate_bic("WG11US335AB").is_err());
        assert!(validate_bic("C0BADEFF").is_err());
    }

    #[test]
    fn location_code_character_restrictions() {
        // Position 7 may not be '0' or '1'.
        assert!(matches!(
            validate_bic("DEUTDE0B").unwrap_err(),
            BicError::InvalidCharacter { ch: '0', pos: 6 }
        ));
        assert!(matches!(
            validate_bic("DEUTDE1B").unwrap_err(),
            BicError::InvalidCharacter { ch: '1', pos: 6 }
        ));
        // Position 8 may not be the letter 'O' (reserved against '0').
        assert!(matches!(
            validate_bic("DEUTDEFO").unwrap_err(),
            BicError::InvalidCharacter { ch: 'O', pos: 7 }
        ));
        // …but digits are fine at position 8.
        assert!(validate_bic("DEUTDEF0").is_ok());
        assert!(validate_bic("DEUTDEF2").is_ok());
    }

    #[test]
    fn the_country_code_must_be_a_real_country() {
        // The bare SEPA pattern accepts any two letters, so this is the last
        // structural rule between a typo and a BIC that addresses nothing.
        assert!(matches!(
            validate_bic("COBAZZFF").unwrap_err(),
            BicError::UnknownCountryCode { .. }
        ));
        assert!(matches!(
            validate_bic("COBAQQFFXXX").unwrap_err(),
            BicError::UnknownCountryCode { .. }
        ));
        // Kosovo: user-assigned rather than ISO 3166, but SWIFT issues BICs
        // under it and XK is a registered IBAN country.
        assert!(validate_bic("NCBKXKPRXXX").is_ok());
    }

    #[test]
    fn the_country_table_agrees_with_the_iban_registry() {
        // A BIC has to be usable against an IBAN from the same country, so
        // every registered IBAN country must be an acceptable BIC country.
        for a in b'A'..=b'Z' {
            for b in b'A'..=b'Z' {
                let cc = String::from_utf8(vec![a, b]).unwrap();
                if crate::iban::iban_bban_format(&cc).is_some() {
                    assert!(
                        is_bic_country_code(&cc),
                        "{cc} is an IBAN country but not a BIC country"
                    );
                }
            }
        }
    }

    #[test]
    fn country_lookup_rejects_malformed_input_without_panicking() {
        for bad in ["", "D", "DEU", "Ü!", "12"] {
            assert!(!is_bic_country_code(bad), "{bad:?} must not be a country");
        }
        assert!(is_bic_country_code("de"));
    }

    #[test]
    fn the_country_table_is_sorted_for_binary_search() {
        assert!(
            BIC_COUNTRY_CODES.windows(2).all(|w| w[0] < w[1]),
            "the table must stay sorted or the binary search silently misses"
        );
    }

    #[test]
    fn test_bic_is_marked_by_second_location_character() {
        // Regression: the rule is position 8 (index 7), not position 7.
        assert!(validate_bic("DEUTDEF0").unwrap().is_test());
        assert!(!validate_bic("COBADEFF").unwrap().is_test());
    }

    #[test]
    fn passive_participant_flag() {
        assert!(validate_bic("DEUTDEF1").unwrap().is_passive());
        assert!(!validate_bic("COBADEFF").unwrap().is_passive());
    }

    #[test]
    fn primary_office_detection() {
        assert!(validate_bic("COBADEFF").unwrap().is_primary_office());
        assert!(validate_bic("COBADEFFXXX").unwrap().is_primary_office());
        assert!(!validate_bic("COBADEFF123").unwrap().is_primary_office());
    }

    #[test]
    fn country_code_must_be_letters() {
        assert!(matches!(
            validate_bic("COBA1EFF").unwrap_err(),
            BicError::InvalidCharacter { ch: '1', pos: 4 }
        ));
    }

    #[test]
    fn real_world_sepa_bics_are_accepted() {
        for b in [
            "COBADEFFXXX", // Commerzbank
            "DEUTDEFF",    // Deutsche Bank
            "PBNKDEFF",    // Postbank
            "GENODEF1S04", // Volksbank
            "ABNANL2A",    // ABN AMRO
            "BNPAFRPP",    // BNP Paribas
            "UNCRITMM",    // UniCredit
            "CRESCHZZ80A", // Credit Suisse
        ] {
            assert!(validate_bic(b).is_ok(), "{b} must validate");
        }
    }

    #[test]
    fn sspkdehhxxx() {
        assert!(validate_bic("SSPKDEHHXXX").is_ok());
    }

    #[test]
    fn from_str() {
        let bic: Bic = "COBADEFF".parse().unwrap();
        assert_eq!(bic.as_str(), "COBADEFF");
    }

    #[test]
    fn try_from_str() {
        assert!(Bic::try_from("COBADEFF").is_ok());
        assert!(Bic::try_from("BAD").is_err());
    }

    #[test]
    fn into_string() {
        let bic = validate_bic("COBADEFF").unwrap();
        let s: String = bic.into();
        assert_eq!(s, "COBADEFF");
    }

    #[test]
    fn ord() {
        let a = validate_bic("COBADEFF").unwrap();
        let b = validate_bic("DEUTDEDB").unwrap();
        assert!(a < b);
    }

    #[test]
    fn deref_to_str() {
        let bic = validate_bic("COBADEFF").unwrap();
        assert_eq!(bic.len(), 8); // Deref to str
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_roundtrip() {
        let bic = validate_bic("COBADEFFXXX").unwrap();
        let json = serde_json::to_string(&bic).unwrap();
        assert_eq!(json, r#""COBADEFFXXX""#);
        let back: Bic = serde_json::from_str(&json).unwrap();
        assert_eq!(back, bic);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_invalid_rejected() {
        let result: Result<Bic, _> = serde_json::from_str(r#""NOTPROVIDED""#);
        assert!(result.is_err());
    }
}
