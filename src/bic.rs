//! BIC (Business Identifier Code) validation — ISO 9362:2022.
//!
//! A BIC identifies a financial institution. ISO 9362 builds it from four
//! parts, and the character classes are **not** uniform:
//!
//! ```text
//! C O B A D E F F X X X
//! ─────┬───── ─┬─ ─┬─ ─┬─
//!      │       │   │   └── branch code, 3 alphanumerics, optional
//!      │       │   └────── business party suffix, 2 alphanumerics
//!      │       └────────── country code, 2 letters, ISO 3166-1
//!      └────────────────── business party prefix, 4 **alphanumerics**
//! ```
//!
//! ## The prefix is alphanumeric, and that is recent
//!
//! ISO 9362:2022 §6.3.1 widened the business party prefix from four letters to
//! four alphanumerics, and SWIFT allocates BICs such as `E097AEXX` under it.
//! Every implementation that still writes `[A-Z]{6}` for the first six
//! characters **rejects a real BIC**, which is the loud half of getting this
//! wrong; the quiet half is emitting one into a schema that cannot hold it.
//!
//! ISO 20022 tracks the same split, and this crate emits messages on both sides
//! of it:
//!
//! | [`BicPattern`] | Pattern | Messages |
//! |---|---|---|
//! | [`Alphanumeric`](BicPattern::Alphanumeric) | `[A-Z0-9]{4}[A-Z]{2}[A-Z0-9]{2}([A-Z0-9]{3}){0,1}` | `pain.001.001.09`, `pain.008.001.08`, `pain.007.001.09`, `pain.002.001.10` |
//! | [`LettersOnly`](BicPattern::LettersOnly) | `[A-Z]{6}[A-Z2-9][A-NP-Z0-9]([A-Z0-9]{3}){0,1}` | `pain.001.001.03`, `pain.001.003.03`, `pain.008.001.02`, `pain.008.003.02`, `camt.055.001.05`, `camt.029.001.06` |
//!
//! The element name is a **separate** question from the pattern, and the two do
//! not move together: `pain.008.001.08` writes `BICFI` over the wide pattern,
//! `pain.008.001.02` writes `BIC` over the narrow one, and `camt.055.001.05`
//! writes `BICFI` over the narrow one. Two ISO 20022 types are even *named*
//! `BICFIIdentifier` with different patterns, which is why a violation is
//! reported as the pattern rather than as a type name.
//!
//! [`validate_bic`] accepts the **current** standard, so no genuine BIC is
//! refused. Whether a given BIC also fits the older, narrower pattern is a
//! property of the value ([`Bic::fits`]) that the builders check against the
//! selected schema — see [`BicPattern`]. A BIC that only the current pattern
//! admits is therefore usable everywhere it is legal and refused, by name,
//! exactly where it is not.
//!
//! ## Examples
//!
//! ```
//! use sepa::bic::{validate_bic, BicPattern};
//!
//! assert!(validate_bic("COBADEFFXXX").is_ok());
//! assert!(validate_bic("DEUTDEDB").is_ok());
//! assert!(validate_bic("NOTPROVIDED").is_err()); // EPC placeholder
//!
//! // ISO 9362:2022 admits digits in the business party prefix.
//! let modern = validate_bic("E097AEXX")?;
//! assert!(modern.fits(BicPattern::Alphanumeric));
//! assert!(!modern.fits(BicPattern::LettersOnly)); // no pre-2019 schema can hold it
//!
//! // parse / FromStr
//! let bic: sepa::Bic = "COBADEFFXXX".parse().unwrap();
//! assert_eq!(bic.country_code(), "DE");
//! # Ok::<(), sepa::BicError>(())
//! ```

use std::str::FromStr;

use crate::country::is_country_code;

// ── BicPattern ────────────────────────────────────────────────────────────────

/// Which character pattern an ISO 20022 message constrains its BICs to.
///
/// ISO 20022 has two, and **the element name does not tell you which**:
/// `pain.008.001.08` writes `BICFI` over the wide pattern, `pain.008.001.02`
/// writes `BIC` over the narrow one — and `camt.055.001.05` writes `BICFI` over
/// the *narrow* one. So the pattern is a fact a message version states, and the
/// element name is a separate fact it also states. See the [module docs](self).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum BicPattern {
    /// `[A-Z0-9]{4}[A-Z]{2}[A-Z0-9]{2}([A-Z0-9]{3}){0,1}` — ISO 9362:2022.
    ///
    /// The business party prefix may hold digits and the suffix is
    /// unrestricted. ISO 20022 calls it `BICFIDec2014Identifier`, and it is
    /// what [`validate_bic`] enforces.
    Alphanumeric,

    /// `[A-Z]{6}[A-Z2-9][A-NP-Z0-9]([A-Z0-9]{3}){0,1}` — the pre-2019 form.
    ///
    /// A letters-only business party prefix, no `0`/`1` at the head of the
    /// suffix and no `O` at its tail. ISO 20022 spells it `BICIdentifier` in
    /// the older payment-initiation messages and, confusingly,
    /// `BICFIIdentifier` in `camt.055.001.05` and `camt.029.001.06`.
    LettersOnly,
}

impl BicPattern {
    /// The pattern as the XSD writes it.
    ///
    /// This is what a [`ValidationError::SchemaPattern`] names, because a type
    /// name would be ambiguous — two different ISO 20022 types share the name
    /// `BICFIIdentifier` with different patterns.
    ///
    /// [`ValidationError::SchemaPattern`]: crate::ValidationError::SchemaPattern
    #[must_use]
    pub const fn as_xsd_pattern(self) -> &'static str {
        match self {
            Self::Alphanumeric => "[A-Z0-9]{4}[A-Z]{2}[A-Z0-9]{2}([A-Z0-9]{3}){0,1}",
            Self::LettersOnly => "[A-Z]{6}[A-Z2-9][A-NP-Z0-9]([A-Z0-9]{3}){0,1}",
        }
    }

    /// Whether `bic` — already normalised to 8 or 11 uppercase alphanumerics —
    /// satisfies this pattern.
    fn admits(self, bic: &str) -> bool {
        let bytes = bic.as_bytes();
        let at = |i: usize| bytes.get(i).copied().unwrap_or(0);
        match self {
            // `validate_bic` has already enforced this one.
            Self::Alphanumeric => true,
            Self::LettersOnly => {
                (0..6).all(|i| at(i).is_ascii_uppercase())
                    && matches!(at(6), b'A'..=b'Z' | b'2'..=b'9')
                    && (at(7).is_ascii_digit() || (at(7).is_ascii_uppercase() && at(7) != b'O'))
            }
        }
    }
}

impl std::fmt::Display for BicPattern {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_xsd_pattern())
    }
}

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

    /// The 4-character business party prefix (chars 1–4).
    ///
    /// ISO 9362 called this the institution code while it was letters-only;
    /// the 2022 revision widened it to alphanumerics and renamed it. See the
    /// [module docs](self).
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

    /// The 2-character business party suffix (chars 7–8), historically the
    /// "location code".
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

    /// Whether this BIC satisfies `pattern`.
    ///
    /// Always `true` for [`BicPattern::Alphanumeric`], which is what
    /// [`validate_bic`] enforces. [`BicPattern::LettersOnly`] is the
    /// interesting case: a BIC with a digit in its business party prefix cannot
    /// be written into `pain.001.001.03`, `pain.001.003.03`,
    /// `pain.008.001.02`, `pain.008.003.02`, `camt.055.001.05` or
    /// `camt.029.001.06`, and the builders refuse it there by name rather than
    /// emitting a document those schemas reject.
    ///
    /// # Examples
    ///
    /// ```
    /// use sepa::bic::{validate_bic, BicPattern};
    ///
    /// let classic = validate_bic("COBADEFFXXX")?;
    /// assert!(classic.fits(BicPattern::LettersOnly));
    /// assert!(classic.fits(BicPattern::Alphanumeric));
    ///
    /// // Digits in the prefix — legal since ISO 9362:2022, and only there.
    /// assert!(!validate_bic("E097AEXX")?.fits(BicPattern::LettersOnly));
    /// # Ok::<(), sepa::BicError>(())
    /// ```
    #[inline]
    #[must_use]
    pub fn fits(&self, pattern: BicPattern) -> bool {
        pattern.admits(&self.0)
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

// ── Validation ────────────────────────────────────────────────────────────────

/// Validate a BIC against ISO 9362:2022.
///
/// Accepts 8-character (`COBADEFF`) and 11-character (`COBADEFFXXX`) BICs, with
/// or without spaces. Input is whitespace-stripped and uppercased before
/// validation, exactly as [`validate_iban`](crate::validate_iban) and
/// [`validate_creditor_id`](crate::validate_creditor_id) treat theirs.
///
/// The accepted form is `[A-Z0-9]{4}[A-Z]{2}[A-Z0-9]{2}([A-Z0-9]{3}){0,1}` —
/// the current standard, and the `BICFIDec2014Identifier` type of every ISO
/// 20022 message published since the 2019 maintenance release. The business
/// party prefix is **alphanumeric**: `E097AEXX` is a real BIC, and a validator
/// that insists on six leading letters rejects it. The narrower pre-2019
/// pattern is not lost — it is a property of the value, [`Bic::fits`], which
/// the builders check against the schema they are emitting.
///
/// Two rules go beyond the pattern:
///
/// - The EPC `"NOTPROVIDED"` placeholder is rejected ([`BicError::Placeholder`]).
///   Use `Option<Bic>` with `None` when the BIC is not known; the writers emit
///   the placeholder into `Othr/Id`, never into a `BICFI` element where a bank
///   would read it as an institution.
/// - Characters 5–6 must be a country code a BIC may carry, not merely two
///   letters — see [`is_country_code`]. The pattern alone accepts `COBAZZFF`,
///   which addresses nothing.
///
/// # Errors
///
/// Returns [`BicError::Placeholder`] for the EPC `"NOTPROVIDED"` sentinel,
/// [`BicError::InvalidLength`] when not 8 or 11 characters,
/// [`BicError::InvalidCharacter`] for any character outside its position's
/// class, with the zero-based position of the offender, or
/// [`BicError::UnknownCountryCode`] when characters 5–6 are letters but not a
/// country.
///
/// # Examples
///
/// ```
/// use sepa::bic::{validate_bic, BicError, BicPattern};
///
/// assert!(validate_bic("COBADEFFXXX").is_ok());
/// assert!(validate_bic("DEUTDEDB").is_ok());
///
/// // ISO 9362:2022 — digits in the business party prefix.
/// assert!(validate_bic("E097AEXX").is_ok());
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
    // Whitespace-stripped and uppercased, like `validate_iban` and
    // `validate_creditor_id`: a BIC copied off a bank letter arrives as
    // "COBA DE FF XXX", and three identifier validators that disagree about
    // whether that is acceptable is a papercut at every call site.
    let upper: String = raw
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| c.to_ascii_uppercase())
        .collect();

    // Reject the EPC "NOTPROVIDED" placeholder before the length check, so the
    // error names the actual problem rather than reporting 11 characters.
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
            // Country code: letters only, in every generation of the standard.
            4 | 5 => ch.is_ascii_uppercase(),
            // Business party prefix, suffix and branch code: alphanumeric.
            _ => ch.is_ascii_uppercase() || ch.is_ascii_digit(),
        };
        if !ok {
            return Err(BicError::InvalidCharacter { ch, pos });
        }
    }
    // Every character is now ASCII, so byte indices are character indices.

    // The pattern says "two letters"; only a country code addresses a bank.
    let country = &upper[4..6];
    if !is_country_code(country) {
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
    fn lowercase_and_spacing_are_normalised() {
        assert!(validate_bic("cobadeff").is_ok());
        // The grouped form printed on bank letters and invoices.
        assert_eq!(
            validate_bic("COBA DE FF XXX").unwrap().as_str(),
            "COBADEFFXXX"
        );
        assert_eq!(validate_bic("  cobadeff\n").unwrap().as_str(), "COBADEFF");
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
    fn an_alphanumeric_business_party_prefix_is_accepted() {
        // Regression: the prefix was checked as `[A-Z]{4}`, which is the
        // pre-2019 ISO 20022 pattern and *not* the standard. ISO 9362:2022
        // §6.3.1 types it 4!c, SWIFT allocates BICs under it, and every
        // message this crate emits by default uses BICFIDec2014Identifier —
        // so rejecting one refused a BIC the schema accepts.
        for b in ["E097AEXX", "WG11US335AB", "C0BADEFF"] {
            let bic = validate_bic(b).unwrap_or_else(|e| panic!("{b} must validate: {e}"));
            assert!(bic.fits(BicPattern::Alphanumeric));
            assert!(
                !bic.fits(BicPattern::LettersOnly),
                "{b} has a digit in the prefix, so no pre-2019 schema can hold it"
            );
        }
    }

    #[test]
    fn the_legacy_pattern_keeps_the_pre_2019_suffix_restrictions() {
        // `BICIdentifier` is `[A-Z]{6}[A-Z2-9][A-NP-Z0-9]…`: no '0'/'1' at the
        // head of the business party suffix, no 'O' at its tail. The 2019 type
        // dropped both, so they are a property of the *schema*, not of the BIC.
        for narrow in ["DEUTDE0B", "DEUTDE1B", "DEUTDEFO"] {
            let bic = validate_bic(narrow).unwrap();
            assert!(
                bic.fits(BicPattern::Alphanumeric),
                "{narrow} is a valid BIC today"
            );
            assert!(
                !bic.fits(BicPattern::LettersOnly),
                "{narrow} must not be written into a pre-2019 schema"
            );
        }
        for wide in ["DEUTDEF0", "DEUTDEF2", "COBADEFFXXX"] {
            assert!(validate_bic(wide).unwrap().fits(BicPattern::LettersOnly));
        }
    }

    #[test]
    fn each_pattern_reports_the_xsd_it_came_from() {
        assert_eq!(
            BicPattern::Alphanumeric.as_xsd_pattern(),
            "[A-Z0-9]{4}[A-Z]{2}[A-Z0-9]{2}([A-Z0-9]{3}){0,1}"
        );
        assert_eq!(
            BicPattern::LettersOnly.as_xsd_pattern(),
            "[A-Z]{6}[A-Z2-9][A-NP-Z0-9]([A-Z0-9]{3}){0,1}"
        );
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
