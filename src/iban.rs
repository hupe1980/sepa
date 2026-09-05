//! IBAN validation — ISO 13616-1:2007 checksum **and** national structure.
//!
//! Validates IBANs from any country in the SWIFT IBAN Registry, and applies the
//! mod-97 checksum to any other.
//!
//! ## Checksum (ISO 13616-1 §5.3)
//!
//! 1. Remove whitespace, convert to uppercase.
//! 2. Check length: 15–34 characters.
//! 3. Move first 4 characters to the end.
//! 4. Replace each letter with its numeric value: A=10, B=11, …, Z=35.
//! 5. Compute the resulting large integer modulo 97.
//! 6. Valid if result == 1.
//!
//! ## Structure
//!
//! The checksum is necessary but not sufficient. It detects an altered
//! character with probability 96/97 and never says *which* one — so an `O`
//! typed for a `0` in a German account number gets through about 99% of the
//! time it is the only error. The registry publishes each country's BBAN
//! structure (`8!n10!n` for Germany, `4!a10!n` for the Netherlands), and
//! [`validate_iban`] checks every character against it, naming the position
//! that failed. See [`iban_bban_format`] and [`BbanCharClass`].
//!
//! ## Examples
//!
//! ```
//! use sepa::iban::{validate_iban, BbanCharClass, IbanError};
//!
//! assert!(validate_iban("DE89 3704 0044 0532 0130 00").is_ok());
//! assert!(validate_iban("NL91ABNA0417164300").is_ok());
//!
//! let err = validate_iban("DE89370400440532013001").unwrap_err();
//! assert!(matches!(err, IbanError::InvalidChecksum { .. }));
//!
//! // A capital O typed for a zero — caught by the structure, not the checksum.
//! let err = validate_iban("DE8937O400440532013000").unwrap_err();
//! assert!(matches!(
//!     err,
//!     IbanError::InvalidBbanFormat { position: 7, expected: BbanCharClass::Digit, .. }
//! ));
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
    /// Printed format: groups of four, e.g. `"DE89 3704 0044 0532 0130 00"`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Validation guarantees pure ASCII, so every 4-byte boundary is also a
        // character boundary and `as_chunks`-style slicing cannot split one.
        for (i, chunk) in group_of_four(&self.0).enumerate() {
            if i > 0 {
                f.write_str(" ")?;
            }
            f.write_str(chunk)?;
        }
        Ok(())
    }
}

/// Split an all-ASCII string into four-character groups.
pub(crate) fn group_of_four(s: &str) -> impl Iterator<Item = &str> {
    (0..s.len())
        .step_by(4)
        .filter_map(move |i| s.get(i..(i + 4).min(s.len())))
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

    /// Characters 1–2 are not two letters, as ISO 13616 requires.
    #[error("IBAN must start with a 2-letter country code, got {code:?}")]
    InvalidCountryCode {
        /// The two characters that were rejected.
        code: String,
    },

    /// Characters 3–4 are not two digits, as ISO 13616 requires.
    ///
    /// Distinct from [`InvalidChecksum`](Self::InvalidChecksum): those digits
    /// are present and well-formed but do not match, whereas here they are not
    /// digits at all.
    #[error("IBAN check digits must be 2 digits, got {value:?}")]
    NonNumericCheckDigits {
        /// The two characters that were rejected.
        value: String,
    },

    /// A BBAN character does not match the country's registered structure.
    ///
    /// Mod-97 is a checksum, not a format check: it accepts a letter where the
    /// registry requires a digit roughly 96 times in 97. This catches the rest —
    /// the `O`-for-`0` and `l`-for-`1` transcription errors that survive it.
    #[error(
        "IBAN position {position} must be {expected} for country {country}, got {found:?} \
         (registry structure {structure})"
    )]
    InvalidBbanFormat {
        /// The two-letter country code.
        country: String,
        /// 1-based position of the offending character in the whole IBAN.
        position: usize,
        /// The character class the registry requires there.
        expected: BbanCharClass,
        /// The character that was found.
        found: char,
        /// The country's registered BBAN structure, as `n`/`a`/`c` symbols.
        structure: &'static str,
    },
}

// ── Registry (ISO 13616 / SWIFT IBAN Registry) ───────────────────────────────

/// The character class an IBAN position admits, in ISO 13616 registry notation.
///
/// The registry writes each country's BBAN as a sequence such as `4!a6!n8!n`:
/// four letters, six digits, eight digits. These are the three classes it uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum BbanCharClass {
    /// `n` — an ASCII digit.
    Digit,
    /// `a` — an ASCII uppercase letter.
    UpperAlpha,
    /// `c` — an ASCII digit or uppercase letter.
    Alphanumeric,
}

impl BbanCharClass {
    /// The registry's one-letter symbol: `"n"`, `"a"` or `"c"`.
    #[inline]
    #[must_use]
    pub const fn as_registry_symbol(self) -> &'static str {
        match self {
            Self::Digit => "n",
            Self::UpperAlpha => "a",
            Self::Alphanumeric => "c",
        }
    }

    /// Whether `ch` belongs to this class.
    ///
    /// Input reaching this point is already uppercased, so lowercase letters
    /// are not a case the classes have to admit.
    #[inline]
    #[must_use]
    pub const fn admits(self, ch: char) -> bool {
        match self {
            Self::Digit => ch.is_ascii_digit(),
            Self::UpperAlpha => ch.is_ascii_uppercase(),
            Self::Alphanumeric => ch.is_ascii_digit() || ch.is_ascii_uppercase(),
        }
    }

    const fn from_symbol(b: u8) -> Option<Self> {
        match b {
            b'n' => Some(Self::Digit),
            b'a' => Some(Self::UpperAlpha),
            b'c' => Some(Self::Alphanumeric),
            _ => None,
        }
    }
}

impl std::fmt::Display for BbanCharClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Digit => "a digit",
            Self::UpperAlpha => "an uppercase letter",
            Self::Alphanumeric => "a digit or uppercase letter",
        })
    }
}

/// The registered BBAN structure for a country, one symbol per character.
///
/// Returns the country's ISO 13616 BBAN format expanded to one `n`/`a`/`c`
/// symbol per position — `"nnnnnnnnnnnnnnnnnn"` for Germany's `8!n10!n`. The
/// string's length is the BBAN length, so the full IBAN length is four more.
/// `None` for a country outside the SWIFT IBAN Registry.
///
/// This is the crate's single source of registry truth: [`iban_country_length`]
/// is derived from it, and [`validate_iban`] checks every BBAN character
/// against it. Structure matters because mod-97 is a checksum, not a format
/// check — it accepts a letter where the registry requires a digit roughly 96
/// times in 97.
///
/// Source: SWIFT IBAN Registry release 102 (June 2026) — 89 entries.
///
/// # Examples
///
/// ```
/// use sepa::iban::iban_bban_format;
///
/// assert_eq!(iban_bban_format("DE"), Some("nnnnnnnnnnnnnnnnnn")); // 8!n10!n
/// assert_eq!(iban_bban_format("NL"), Some("aaaannnnnnnnnn"));     // 4!a10!n
/// assert_eq!(iban_bban_format("XX"), None);
/// ```
#[must_use]
#[allow(clippy::match_same_arms, reason = "a data table, not control flow")]
#[allow(clippy::too_many_lines, reason = "89 registry entries, one per line")]
pub fn iban_bban_format(country: &str) -> Option<&'static str> {
    // Sorted by country code. Each comment carries the registry's own notation
    // so an entry can be checked against the published table by eye.
    match country {
        "AD" => Some("nnnnnnnncccccccccccc"), // 4!n4!n12!c           Andorra
        "AE" => Some("nnnnnnnnnnnnnnnnnnn"),  // 3!n16!n              United Arab Emirates (The)
        "AL" => Some("nnnnnnnncccccccccccccccc"), // 8!n16!c              Albania
        "AT" => Some("nnnnnnnnnnnnnnnn"),     // 5!n11!n              Austria
        "AZ" => Some("aaaacccccccccccccccccccc"), // 4!a20!c              Azerbaijan
        "BA" => Some("nnnnnnnnnnnnnnnn"),     // 3!n3!n8!n2!n         Bosnia and Herzegovina
        "BE" => Some("nnnnnnnnnnnn"),         // 3!n7!n2!n            Belgium
        "BG" => Some("aaaannnnnncccccccc"),   // 4!a4!n2!n8!c         Bulgaria
        "BH" => Some("aaaacccccccccccccc"),   // 4!a14!c              Bahrain
        "BI" => Some("nnnnnnnnnnnnnnnnnnnnnnn"), // 5!n5!n11!n2!n        Burundi
        "BR" => Some("nnnnnnnnnnnnnnnnnnnnnnnac"), // 8!n5!n10!n1!a1!c     Brazil
        "BY" => Some("ccccnnnncccccccccccccccc"), // 4!c4!n16!c           Belarus
        "CH" => Some("nnnnncccccccccccc"),    // 5!n12!c              Switzerland
        "CR" => Some("nnnnnnnnnnnnnnnnnn"),   // 4!n14!n              Costa Rica
        "CY" => Some("nnnnnnnncccccccccccccccc"), // 3!n5!n16!c           Cyprus
        "CZ" => Some("nnnnnnnnnnnnnnnnnnnn"), // 4!n16!n              Czechia
        "DE" => Some("nnnnnnnnnnnnnnnnnn"),   // 8!n10!n              Germany
        "DJ" => Some("nnnnnnnnnnnnnnnnnnnnnnn"), // 5!n5!n11!n2!n        Djibouti
        "DK" => Some("nnnnnnnnnnnnnn"),       // 4!n9!n1!n            Denmark
        "DO" => Some("ccccnnnnnnnnnnnnnnnnnnnn"), // 4!c20!n              Dominican Republic
        "EE" => Some("nnnnnnnnnnnnnnnn"),     // 2!n14!n              Estonia
        "EG" => Some("nnnnnnnnnnnnnnnnnnnnnnnnn"), // 4!n4!n17!n           Egypt
        "ES" => Some("nnnnnnnnnnnnnnnnnnnn"), // 4!n4!n1!n1!n10!n     Spain
        "FI" => Some("nnnnnnnnnnnnnn"),       // 3!n11!n              Finland
        "FK" => Some("aannnnnnnnnnnn"),       // 2!a12!n              Falkland Islands (Malvinas)
        "FO" => Some("nnnnnnnnnnnnnn"),       // 4!n9!n1!n            Faroe Islands
        "FR" => Some("nnnnnnnnnncccccccccccnn"), // 5!n5!n11!c2!n        France
        "GB" => Some("aaaannnnnnnnnnnnnn"),   // 4!a6!n8!n            United Kingdom
        "GE" => Some("aannnnnnnnnnnnnnnn"),   // 2!a16!n              Georgia
        "GI" => Some("aaaaccccccccccccccc"),  // 4!a15!c              Gibraltar
        "GL" => Some("nnnnnnnnnnnnnn"),       // 4!n9!n1!n            Greenland
        "GR" => Some("nnnnnnncccccccccccccccc"), // 3!n4!n16!c           Greece
        "GT" => Some("cccccccccccccccccccccccc"), // 4!c20!c              Guatemala
        "HN" => Some("aaaannnnnnnnnnnnnnnnnnnn"), // 4!a20!n              Honduras
        "HR" => Some("nnnnnnnnnnnnnnnnn"),    // 7!n10!n              Croatia
        "HU" => Some("nnnnnnnnnnnnnnnnnnnnnnnn"), // 3!n4!n1!n15!n1!n     Hungary
        "IE" => Some("aaaannnnnnnnnnnnnn"),   // 4!a6!n8!n            Ireland
        "IL" => Some("nnnnnnnnnnnnnnnnnnn"),  // 3!n3!n13!n           Israel
        "IQ" => Some("aaaannnnnnnnnnnnnnn"),  // 4!a3!n12!n           Iraq
        "IS" => Some("nnnnnnnnnnnnnnnnnnnnnn"), // 4!n2!n6!n10!n        Iceland
        "IT" => Some("annnnnnnnnncccccccccccc"), // 1!a5!n5!n12!c        Italy
        "JO" => Some("aaaannnncccccccccccccccccc"), // 4!a4!n18!c           Jordan
        "KW" => Some("aaaacccccccccccccccccccccc"), // 4!a22!c              Kuwait
        "KZ" => Some("nnnccccccccccccc"),     // 3!n13!c              Kazakhstan
        "LB" => Some("nnnncccccccccccccccccccc"), // 4!n20!c              Lebanon
        "LC" => Some("aaaacccccccccccccccccccccccc"), // 4!a24!c              Saint Lucia
        "LI" => Some("nnnnncccccccccccc"),    // 5!n12!c              Liechtenstein
        "LT" => Some("nnnnnnnnnnnnnnnn"),     // 5!n11!n              Lithuania
        "LU" => Some("nnnccccccccccccc"),     // 3!n13!c              Luxembourg
        "LV" => Some("aaaaccccccccccccc"),    // 4!a13!c              Latvia
        "LY" => Some("nnnnnnnnnnnnnnnnnnnnn"), // 3!n3!n15!n           Libya
        "MC" => Some("nnnnnnnnnncccccccccccnn"), // 5!n5!n11!c2!n        Monaco
        "MD" => Some("cccccccccccccccccccc"), // 2!c18!c              Moldova, Republic of
        "ME" => Some("nnnnnnnnnnnnnnnnnn"),   // 3!n13!n2!n           Montenegro
        "MK" => Some("nnnccccccccccnn"),      // 3!n10!c2!n           North Macedonia
        "MN" => Some("nnnnnnnnnnnnnnnn"),     // 4!n12!n              Mongolia
        "MR" => Some("nnnnnnnnnnnnnnnnnnnnnnn"), // 5!n5!n11!n2!n        Mauritania
        "MT" => Some("aaaannnnncccccccccccccccccc"), // 4!a5!n18!c           Malta
        "MU" => Some("aaaannnnnnnnnnnnnnnnnnnaaa"), // 4!a2!n2!n12!n3!n3!a  Mauritius
        "NI" => Some("aaaannnnnnnnnnnnnnnnnnnn"), // 4!a20!n              Nicaragua
        "NL" => Some("aaaannnnnnnnnn"),       // 4!a10!n              Netherlands (The)
        "NO" => Some("nnnnnnnnnnn"),          // 4!n6!n1!n            Norway
        "OM" => Some("nnncccccccccccccccc"),  // 3!n16!c              Oman
        "PK" => Some("aaaacccccccccccccccc"), // 4!a16!c              Pakistan
        "PL" => Some("nnnnnnnnnnnnnnnnnnnnnnnn"), // 8!n16!n              Poland
        "PS" => Some("aaaaccccccccccccccccccccc"), // 4!a21!c              Palestine, State of
        "PT" => Some("nnnnnnnnnnnnnnnnnnnnn"), // 4!n4!n11!n2!n        Portugal
        "QA" => Some("aaaaccccccccccccccccccccc"), // 4!a21!c              Qatar
        "RO" => Some("aaaacccccccccccccccc"), // 4!a16!c              Romania
        "RS" => Some("nnnnnnnnnnnnnnnnnn"),   // 3!n13!n2!n           Serbia
        "RU" => Some("nnnnnnnnnnnnnnccccccccccccccc"), // 9!n5!n15!c           Russian Federation
        "SA" => Some("nncccccccccccccccccc"), // 2!n18!c              Saudi Arabia
        "SC" => Some("aaaannnnnnnnnnnnnnnnnnnnaaa"), // 4!a2!n2!n16!n3!a     Seychelles
        "SD" => Some("nnnnnnnnnnnnnn"),       // 2!n12!n              Sudan
        "SE" => Some("nnnnnnnnnnnnnnnnnnnn"), // 3!n16!n1!n           Sweden
        "SI" => Some("nnnnnnnnnnnnnnn"),      // 5!n8!n2!n            Slovenia
        "SK" => Some("nnnnnnnnnnnnnnnnnnnn"), // 4!n6!n10!n           Slovakia
        "SM" => Some("annnnnnnnnncccccccccccc"), // 1!a5!n5!n12!c        San Marino
        "SO" => Some("nnnnnnnnnnnnnnnnnnn"),  // 4!n3!n12!n           Somalia
        "ST" => Some("nnnnnnnnnnnnnnnnnnnnn"), // 4!n4!n11!n2!n        Sao Tome and Principe
        "SV" => Some("aaaannnnnnnnnnnnnnnnnnnn"), // 4!a20!n              El Salvador
        "TL" => Some("nnnnnnnnnnnnnnnnnnn"),  // 3!n14!n2!n           Timor-Leste
        "TN" => Some("nnnnnnnnnnnnnnnnnnnn"), // 2!n3!n13!n2!n        Tunisia
        "TR" => Some("nnnnnncccccccccccccccc"), // 5!n1!n16!c           Turkiye
        "UA" => Some("nnnnnnccccccccccccccccccc"), // 6!n19!c              Ukraine
        "VA" => Some("nnnnnnnnnnnnnnnnnn"),   // 3!n15!n              Holy See
        "VG" => Some("aaaannnnnnnnnnnnnnnn"), // 4!a16!n              Virgin Islands (British)
        "XK" => Some("nnnnnnnnnnnnnnnn"),     // 4!n10!n2!n           Kosovo
        "YE" => Some("aaaannnncccccccccccccccccc"), // 4!a4!n18!c           Yemen
        _ => None,
    }
}

/// The class admitted at 0-based `index` of `country`'s BBAN.
fn bban_class_at(country: &str, index: usize) -> Option<BbanCharClass> {
    let symbol = *iban_bban_format(country)?.as_bytes().get(index)?;
    BbanCharClass::from_symbol(symbol)
}

/// Return the expected IBAN length for a given 2-letter country code,
/// or `None` for countries not in the ISO 13616 registry.
///
/// Derived from [`iban_bban_format`], so the length and the structure can never
/// disagree.
///
/// # Examples
///
/// ```
/// use sepa::iban::iban_country_length;
///
/// assert_eq!(iban_country_length("DE"), Some(22));
/// assert_eq!(iban_country_length("NO"), Some(15));
/// assert_eq!(iban_country_length("XX"), None);
/// ```
#[inline]
#[must_use]
pub fn iban_country_length(country: &str) -> Option<usize> {
    // 4 = the country code and check digits that precede the BBAN.
    Some(4 + iban_bban_format(country)?.len())
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
///
/// The list grew by five between 2024 and 2025 — `AL`, `ME` (November 2024),
/// `MK`, `MD` (March 2025) and `RS` (May 2025) — so a hard-coded list written
/// before then is missing them.
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
/// Source: EPC409-09 v8.0 (24 December 2025). Five countries joined across
/// 2024–2025 and are easy to miss in an older list: Albania and Montenegro
/// (November 2024), North Macedonia and Moldova (March 2025) and Serbia
/// (May 2025).
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

// ── Construction ──────────────────────────────────────────────────────────────

/// The mod-97 remainder of an IBAN's characters in ISO 13616 rotated order.
///
/// Folded in one pass: materialising the expanded decimal string would be three
/// allocations for a value consumed a digit at a time, and a 34-character IBAN
/// expands past what any integer type could hold. Non-alphanumerics are
/// skipped, so a spaced or hyphenated BBAN behaves like the stripped one.
fn mod97_rotated(header: &str, bban: &str) -> u64 {
    bban.bytes()
        .chain(header.bytes())
        .fold(0u64, |acc, b| match b {
            b'0'..=b'9' => (acc * 10 + u64::from(b - b'0')) % 97,
            b'A'..=b'Z' => (acc * 100 + u64::from(b - b'A') + 10) % 97,
            _ => acc,
        })
}

/// The two ISO 13616 check digits an IBAN would carry for `country` + `bban`.
///
/// The third of the crate's check-digit functions, beside
/// [`creditor_id_check_digits`](crate::creditor_id_check_digits) and
/// [`RfReference::check_digits_for`](crate::RfReference::check_digits_for) —
/// and the one whose absence made building an IBAN from a national bank code
/// and account number a job for somebody else's snippet.
///
/// The algorithm is `98 − (mod-97 of "<bban><country>00")`, expanding letters
/// to `A=10 … Z=35`. Whitespace and separators in `bban` are ignored and
/// lower-case letters are upper-cased, so `"3704 0044 0532 0130 00"` and
/// `"370400440532013000"` give the same answer.
///
/// This computes; it does not check. Nothing here says the country is real or
/// the BBAN is the right shape for it — [`Iban::from_bban`] does both.
///
/// # Examples
///
/// ```
/// use sepa::iban::iban_check_digits;
///
/// assert_eq!(iban_check_digits("DE", "370400440532013000"), "89");
/// assert_eq!(iban_check_digits("de", "3704 0044 0532 0130 00"), "89");
/// assert_eq!(iban_check_digits("NL", "ABNA0417164300"), "91");
/// ```
#[must_use]
pub fn iban_check_digits(country: &str, bban: &str) -> String {
    let country: String = country
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_uppercase())
        .collect();
    let bban: String = bban
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_uppercase())
        .collect();
    // `98 − n mod 97` lands in 2..=98, which is the ISO 7064 MOD 97-10 range.
    format!("{:02}", 98 - mod97_rotated(&format!("{country}00"), &bban))
}

impl Iban {
    /// Build an IBAN from a country code and a national account number.
    ///
    /// The check digits are computed with [`iban_check_digits`] and the result
    /// then goes through [`validate_iban`], so the country's registered BBAN
    /// structure is enforced exactly as it is for a parsed IBAN. A digit typed
    /// where the registry wants a letter fails here rather than at the bank.
    ///
    /// Whitespace and separators in `bban` are ignored; letters are
    /// upper-cased.
    ///
    /// # Errors
    ///
    /// Every [`IbanError`] [`validate_iban`] can produce, except
    /// [`IbanError::InvalidChecksum`] — the digits are computed, so they always
    /// agree. A country outside the registry has no published structure, so it
    /// gets the length and character rules and nothing more.
    ///
    /// # Examples
    ///
    /// ```
    /// use sepa::{Iban, iban::IbanError};
    ///
    /// let iban = Iban::from_bban("DE", "3704 0044 0532 0130 00")?;
    /// assert_eq!(iban.as_str(), "DE89370400440532013000");
    ///
    /// // The registry structure still applies: a German BBAN is all digits.
    /// assert!(matches!(
    ///     Iban::from_bban("DE", "37O400440532013000"),
    ///     Err(IbanError::InvalidBbanFormat { .. })
    /// ));
    ///
    /// // As does the registry length: 20 BBAN digits is a legal IBAN length,
    /// // and the wrong one for Germany.
    /// assert!(matches!(
    ///     Iban::from_bban("DE", "12345678901234567890"),
    ///     Err(IbanError::WrongLengthForCountry { expected: 22, actual: 24, .. })
    /// ));
    /// # Ok::<(), IbanError>(())
    /// ```
    pub fn from_bban(country: &str, bban: &str) -> Result<Self, IbanError> {
        let check = iban_check_digits(country, bban);
        let normalised: String = country
            .chars()
            .chain(check.chars())
            .chain(bban.chars())
            .filter(|c| !c.is_whitespace())
            .collect();
        validate_iban(&normalised)
    }
}

// ── Validation ────────────────────────────────────────────────────────────────

/// Validate an IBAN using the ISO 13616 mod-97 algorithm.
///
/// Accepts IBANs with or without spaces.  Input is uppercased before validation.
/// Returns the normalised [`Iban`] on success.
///
/// Validation order — earliest and most specific failure wins:
/// 1. Strip whitespace, uppercase.
/// 2. Check overall length range (15–34).
/// 3. Check all characters are ASCII alphanumeric.
/// 4. Check the ISO 13616 header: 2 letters, then 2 digits.
/// 5. Check country-specific length against the SWIFT IBAN Registry
///    (only for registered country codes; others skip steps 5 and 6).
/// 6. Check every BBAN character against the country's registered structure.
/// 7. Compute mod-97 checksum — must equal 1.
///
/// Step 6 is not redundant with step 7. Mod-97 is a checksum: it detects an
/// altered character with probability 96/97, and says nothing about *which*
/// one. The registry structure catches the specific transcription errors that
/// slip through — an `O` typed for a `0`, a letter in a numeric bank code —
/// and names the position.
///
/// # Errors
///
/// | Error | Condition |
/// |---|---|
/// | [`IbanError::InvalidLength`] | Length outside 15–34 |
/// | [`IbanError::InvalidCharacter`] | Non-alphanumeric character |
/// | [`IbanError::InvalidCountryCode`] | Characters 1–2 are not letters |
/// | [`IbanError::NonNumericCheckDigits`] | Characters 3–4 are not digits |
/// | [`IbanError::WrongLengthForCountry`] | Length wrong for a registered country |
/// | [`IbanError::InvalidBbanFormat`] | BBAN character violates the registered structure |
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
///
/// // A capital O typed for a zero: the German BBAN is all digits.
/// let err = validate_iban("DE8937O400440532013000").unwrap_err();
/// assert!(matches!(err, IbanError::InvalidBbanFormat { position: 7, .. }));
/// ```
#[must_use = "ignoring a validated IBAN loses the result"]
pub fn validate_iban(raw: &str) -> Result<Iban, IbanError> {
    let normalised: String = raw
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| c.to_ascii_uppercase())
        .collect();

    // Counted in characters: a non-ASCII character is rejected below, but the
    // length reported for one must not be its UTF-8 byte count.
    let len = normalised.chars().count();
    if !(15..=34).contains(&len) {
        return Err(IbanError::InvalidLength { len });
    }

    for c in normalised.chars() {
        if !c.is_ascii_alphanumeric() {
            return Err(IbanError::InvalidCharacter { ch: c });
        }
    }
    // Every character is now ASCII, so byte indices are character indices.

    // ISO 13616 fixes the header: `CCkk`. Mod-97 alone would not catch a
    // digit in the country code — the expansion happily consumes one.
    let country = &normalised[..2];
    if !country.bytes().all(|b| b.is_ascii_uppercase()) {
        return Err(IbanError::InvalidCountryCode {
            code: country.to_owned(),
        });
    }
    let check_digits = &normalised[2..4];
    if !check_digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(IbanError::NonNumericCheckDigits {
            value: check_digits.to_owned(),
        });
    }

    // Registry checks. A country outside the registry has no published
    // structure, so it gets the checksum and nothing more.
    if let Some(structure) = iban_bban_format(country) {
        let expected_len = 4 + structure.len();
        if len != expected_len {
            return Err(IbanError::WrongLengthForCountry {
                country: country.to_owned(),
                expected: expected_len,
                actual: len,
            });
        }
        for (i, ch) in normalised[4..].chars().enumerate() {
            let Some(class) = bban_class_at(country, i) else {
                continue;
            };
            if !class.admits(ch) {
                return Err(IbanError::InvalidBbanFormat {
                    country: country.to_owned(),
                    position: i + 5, // 1-based, past the 4-character header
                    expected: class,
                    found: ch,
                    structure,
                });
            }
        }
    }

    // ISO 13616 §5.3: move the first four characters to the end, expand each
    // letter to its two-digit value (A=10 … Z=35), then take the whole thing
    // mod 97. The same fold `iban_check_digits` runs, so a generated IBAN and a
    // validated one cannot disagree about the arithmetic.
    let remainder = mod97_rotated(&normalised[..4], &normalised[4..]);

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
    fn every_registry_example_is_reproduced_from_its_own_bban() {
        // The generator is checked against the same artefact the validator is:
        // SWIFT's published examples. Round-tripping the two against each other
        // would only prove they share an implementation — which they do.
        for example in REGISTRY_EXAMPLES {
            let iban = validate_iban(example).unwrap();
            let rebuilt = Iban::from_bban(iban.country_code(), iban.bban())
                .unwrap_or_else(|e| panic!("{example} must rebuild: {e}"));
            assert_eq!(rebuilt, iban, "{example} did not reproduce");
            assert_eq!(
                iban_check_digits(iban.country_code(), iban.bban()),
                iban.check_digits(),
                "{example} check digits"
            );
        }
    }

    #[test]
    fn from_bban_applies_the_registry_structure_not_just_the_checksum() {
        // The whole point of computing the digits here rather than in a
        // caller's snippet: the result still has to be a well-formed IBAN.
        assert!(matches!(
            Iban::from_bban("DE", "37O400440532013000"),
            Err(IbanError::InvalidBbanFormat {
                position: 7,
                expected: BbanCharClass::Digit,
                ..
            })
        ));
        // 20 BBAN digits is a legal IBAN length overall, and the wrong one
        // for Germany — which is the error the registry is there to give.
        assert!(matches!(
            Iban::from_bban("DE", "12345678901234567890"),
            Err(IbanError::WrongLengthForCountry {
                expected: 22,
                actual: 24,
                ..
            })
        ));
        // Below the ISO 13616 floor, so the generic rule fires first.
        assert!(matches!(
            Iban::from_bban("DE", "12345"),
            Err(IbanError::InvalidLength { len: 9 })
        ));
        // A country outside the registry keeps the checksum and nothing more.
        assert!(Iban::from_bban("ZZ", "12345678901234").is_ok());
    }

    #[test]
    fn separators_in_a_bban_do_not_change_the_check_digits() {
        assert_eq!(
            Iban::from_bban("DE", "3704 0044 0532 0130 00").unwrap(),
            Iban::from_bban("de", "370400440532013000").unwrap()
        );
    }

    #[test]
    fn generated_check_digits_are_always_two_digits_in_range() {
        // ISO 7064 MOD 97-10 yields 02..=98; a bare `98 - n` would print "2"
        // rather than "02" and silently shorten the IBAN by a character.
        for n in 0..2_000u32 {
            let cd = iban_check_digits("DE", &format!("{n:018}"));
            assert_eq!(cd.len(), 2, "{n} gave {cd:?}");
            let value: u32 = cd.parse().unwrap();
            assert!((2..=98).contains(&value), "{n} gave {cd:?}");
        }
    }

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

    /// Real IBAN examples published in the SWIFT IBAN Registry, one per
    /// country. Every one must pass length, structure and checksum — this is
    /// the table's defence against a transcription slip in 89 hand-entered
    /// registry rows.
    const REGISTRY_EXAMPLES: [&str; 78] = [
        "AD1200012030200359100100",
        "AE070331234567890123456",
        "AL47212110090000000235698741",
        "AT611904300234573201",
        "AZ21NABZ00000000137010001944",
        "BA391290079401028494",
        "BE68539007547034",
        "BG80BNBG96611020345678",
        "BH67BMAG00001299123456",
        "BR9700360305000010009795493P1",
        "BY13NBRB3600900000002Z00AB00",
        "CH9300762011623852957",
        "CR05015202001026284066",
        "CY17002001280000001200527600",
        "CZ6508000000192000145399",
        "DE89370400440532013000",
        "DJ2110002010010409943020008",
        "DK5000400440116243",
        "DO28BAGR00000001212453611324",
        "EE382200221020145685",
        "EG380019000500000000263180002",
        "ES9121000418450200051332",
        "FI2112345600000785",
        "FO2000400440116243",
        "FR1420041010050500013M02606",
        "GB29NWBK60161331926819",
        "GE29NB0000000101904917",
        "GI75NWBK000000007099453",
        "GL2000400440116243",
        "GR1601101250000000012300695",
        "GT82TRAJ01020000001210029690",
        "HN54PISA00000000000000123124",
        "HR1210010051863000160",
        "HU42117730161111101800000000",
        "IE29AIBK93115212345678",
        "IL620108000000099999999",
        "IQ98NBIQ850123456789012",
        "IS140159260076545510730339",
        "IT60X0542811101000000123456",
        "JO94CBJO0010000000000131000302",
        "KW81CBKU0000000000001234560101",
        "KZ86125KZT5004100100",
        "LB62099900000001001901229114",
        "LC55HEMM000100010012001200023015",
        "LI21088100002324013AA",
        "LT121000011101001000",
        "LU280019400644750000",
        "LV80BANK0000435195001",
        "MC5811222000010123456789030",
        "MD24AG000225100013104168",
        "ME25505000012345678951",
        "MK07250120000058984",
        "MR1300020001010000123456753",
        "MT84MALT011000012345MTLCAST001S",
        "MU17BOMM0101101030300200000MUR",
        "NL91ABNA0417164300",
        "NO9386011117947",
        "PK36SCBL0000001123456702",
        "PL61109010140000071219812874",
        "PS92PALS000000000400123456702",
        "PT50000201231234567890154",
        "QA58DOHB00001234567890ABCDEFG",
        "RO49AAAA1B31007593840000",
        "RS35260005601001611379",
        "SA0380000000608010167519",
        "SC18SSCB11010000000000001497USD",
        "SE4550000000058398257466",
        "SI56191000000123438",
        "SK3112000000198742637541",
        "SM86U0322509800000000270100",
        "ST68000100010051845310112",
        "SV62CENR00000000000000700025",
        "TL380080012345678910157",
        "TN5910006035183598478831",
        "TR330006100519786457841326",
        "UA213996220000026007233566001",
        "VG96VPVG0000012345678901",
        "XK051212012345678906",
    ];

    #[test]
    fn every_published_registry_example_validates() {
        for example in REGISTRY_EXAMPLES {
            assert!(
                validate_iban(example).is_ok(),
                "registry example {example} must validate: {:?}",
                validate_iban(example)
            );
        }
    }

    #[test]
    fn registry_structures_are_well_formed() {
        for a in b'A'..=b'Z' {
            for b in b'A'..=b'Z' {
                let cc = String::from_utf8(vec![a, b]).unwrap();
                let Some(structure) = iban_bban_format(&cc) else {
                    continue;
                };
                assert!(
                    structure.bytes().all(|s| matches!(s, b'n' | b'a' | b'c')),
                    "{cc} structure {structure:?} has an unknown symbol"
                );
                // The registry's own bound: 15–34 for the whole IBAN.
                let len = 4 + structure.len();
                assert!((15..=34).contains(&len), "{cc} length {len} is impossible");
                assert_eq!(iban_country_length(&cc), Some(len));
            }
        }
    }

    #[test]
    fn a_letter_typed_for_a_digit_is_caught_and_located() {
        // The classic transcription error mod-97 lets through 96 times in 97:
        // a capital O for a zero. The German BBAN is 18 digits.
        let err = validate_iban("DE8937O400440532013000").unwrap_err();
        assert_eq!(
            err,
            IbanError::InvalidBbanFormat {
                country: "DE".to_owned(),
                position: 7,
                expected: BbanCharClass::Digit,
                found: 'O',
                structure: "nnnnnnnnnnnnnnnnnn",
            }
        );
        // And the converse: a digit where the registry requires a letter.
        // NL is 4!a10!n, so position 5 must be a letter.
        assert!(matches!(
            validate_iban("NL914BNA0417164300"),
            Err(IbanError::InvalidBbanFormat {
                position: 5,
                expected: BbanCharClass::UpperAlpha,
                ..
            })
        ));
    }

    #[test]
    fn the_iso_13616_header_is_checked_before_the_checksum() {
        // Mod-97 expands letters happily, so neither of these is caught by the
        // checksum — the country code would simply not be a country code.
        assert!(matches!(
            validate_iban("1289370400440532013000"),
            Err(IbanError::InvalidCountryCode { .. })
        ));
        assert!(matches!(
            validate_iban("DEX9370400440532013000"),
            Err(IbanError::NonNumericCheckDigits { .. })
        ));
    }

    #[test]
    fn an_unregistered_country_gets_the_checksum_and_nothing_more() {
        // No published structure means nothing to check it against; the mod-97
        // result still decides.
        assert_eq!(iban_bban_format("QQ"), None);
        assert!(matches!(
            validate_iban("QQ00ABC123"),
            Err(IbanError::InvalidLength { .. })
        ));
        let unregistered = validate_iban("XX89370400440532013000");
        assert!(!matches!(
            unregistered,
            Err(IbanError::WrongLengthForCountry { .. } | IbanError::InvalidBbanFormat { .. })
        ));
    }

    #[test]
    fn char_classes_admit_what_the_registry_says() {
        assert!(BbanCharClass::Digit.admits('7'));
        assert!(!BbanCharClass::Digit.admits('A'));
        assert!(BbanCharClass::UpperAlpha.admits('A'));
        assert!(!BbanCharClass::UpperAlpha.admits('7'));
        // Input is uppercased before it reaches the check.
        assert!(!BbanCharClass::UpperAlpha.admits('a'));
        assert!(BbanCharClass::Alphanumeric.admits('A'));
        assert!(BbanCharClass::Alphanumeric.admits('7'));
        assert!(!BbanCharClass::Alphanumeric.admits('-'));
        assert_eq!(BbanCharClass::Digit.as_registry_symbol(), "n");
        assert_eq!(BbanCharClass::UpperAlpha.to_string(), "an uppercase letter");
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
