//! Postal addresses — ISO 20022 `PstlAdr`.
//!
//! A payer's or payee's address is optional in the SEPA schemes, and most
//! domestic batches never carry one. It becomes unavoidable when a bank asks
//! for it: sanction screening, payments touching a non-EEA party, and any
//! channel that has already moved to the 2025 rulebooks.
//!
//! ## Structured, hybrid, unstructured
//!
//! ISO 20022 lets an address be written three ways, and the EPC is retiring
//! one of them:
//!
//! | Form | Shape | Status |
//! |---|---|---|
//! | **Structured** | dedicated elements only — `StrtNm`, `BldgNb`, `PstCd`, `TwnNm`, `Ctry` | preferred |
//! | **Hybrid** | `TwnNm` + `Ctry` plus up to two free-text `AdrLine`s | permitted |
//! | **Unstructured** | `AdrLine` only, nothing parsable | **rejected from 15 November 2026** |
//!
//! The cut-over is one industry-wide date. Version 1.0 of the 2025 rulebooks
//! set it at 22 November 2026; version 1.1, in force since 5 October 2025,
//! moved it to **15 November 2026** to line up with that year's Swift Standards
//! MX release. From then on `TwnNm` and `Ctry` are mandatory whenever an
//! address is present at all — the address itself stays optional.
//!
//! This type therefore **cannot represent an unstructured address**: [`new`]
//! takes the town and the country, so the only reachable forms are the two that
//! survive the deadline. That is the same treatment [`Iban`](crate::Iban) and
//! [`IsoDate`](crate::IsoDate) get — the invalid state is unconstructible
//! rather than caught late.
//!
//! [`new`]: PostalAddress::new
//!
//! ## Which elements
//!
//! ISO 20022 grew the type over time: `pain.001.001.03` and `pain.008.001.02`
//! use `PostalAddress6`, while `pain.001.001.09` and `pain.008.001.08` use
//! `PostalAddress24`, which adds `BldgNm`, `Flr`, `PstBx`, `Room`, `TwnLctnNm`
//! and `DstrctNm`. Only the elements common to both are exposed here, so one
//! address value is valid against every schema this crate emits — and those
//! extras are outside the EPC's own address guidance anyway.
//!
//! The legacy Deutsche Kreditwirtschaft schemas are the exception: their
//! `PostalAddressSEPA` type holds nothing but `Ctry` and two address lines, so
//! a structured address is unrepresentable there and the builders reject it
//! with [`ValidationError::UnsupportedBySchema`].
//!
//! ## Examples
//!
//! ```
//! use sepa::address::{AddressFormat, PostalAddress};
//!
//! // Structured — the form to aim for.
//! let structured = PostalAddress::new("Berlin", "DE")?
//!     .street("Unter den Linden")
//!     .building_number("77")
//!     .post_code("10117");
//! assert_eq!(structured.format(), AddressFormat::Structured);
//!
//! // Hybrid — the leftovers stay in an address line.
//! let hybrid = PostalAddress::new("Berlin", "DE")?.line("Unter den Linden 77");
//! assert_eq!(hybrid.format(), AddressFormat::Hybrid);
//!
//! // A country that is not a country is refused at construction.
//! assert!(PostalAddress::new("Atlantis", "ZZ").is_err());
//! # Ok::<(), sepa::address::AddressError>(())
//! ```

use crate::country::is_country_code;
use crate::validate::{CharsetPolicy, ValidationError, check_text};
use crate::xml_util::write_escaped;

// ── limits ────────────────────────────────────────────────────────────────────

/// Maximum number of `AdrLine` elements the EPC permits.
///
/// The XSD allows seven; the EPC address guidance allows two, and a hybrid
/// address is defined in terms of that pair.
pub const MAX_ADDRESS_LINES: usize = 2;

const MAX_70: usize = 70;
const MAX_35: usize = 35;
const MAX_16: usize = 16;

// ── error ─────────────────────────────────────────────────────────────────────

/// Error returned when a postal address cannot be constructed.
///
/// Only the two facts that make an address structurally impossible live here.
/// Length and character-set violations are [`ValidationError`]s raised by
/// `build()`, alongside every other field rule, because transliteration can
/// change a length after the value was supplied.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum AddressError {
    /// The town name was empty or blank.
    ///
    /// `TwnNm` is mandatory whenever an address is present — see the
    /// [module docs](self).
    #[error("PstlAdr/TwnNm must not be empty")]
    EmptyTown,

    /// The country code is not an ISO 3166-1 alpha-2 country.
    #[error("PstlAdr/Ctry {code:?} is not an ISO 3166-1 alpha-2 country")]
    UnknownCountry {
        /// The two-letter code that was rejected.
        code: String,
    },
}

// ── AddressFormat ─────────────────────────────────────────────────────────────

/// Which of the ISO 20022 address forms an address is written in.
///
/// There is deliberately no `Unstructured` variant: [`PostalAddress`] requires
/// a town and a country, so the form the EPC retires on 15 November 2026 is not
/// constructible. See the [module docs](self).
///
/// This set is closed on purpose — a `match` over it needs no wildcard arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum AddressFormat {
    /// Dedicated elements only — no `AdrLine`.
    Structured,
    /// Dedicated elements plus one or two free-text `AdrLine`s.
    Hybrid,
}

// ── PostalAddress ─────────────────────────────────────────────────────────────

/// An ISO 20022 `PstlAdr`, in structured or hybrid form.
///
/// Build one with [`PostalAddress::new`] and chain the optional elements. Only
/// the elements common to `PostalAddress6` and `PostalAddress24` are exposed,
/// so the same value is valid against every ISO schema this crate emits.
///
/// # Examples
///
/// ```
/// use sepa::PostalAddress;
///
/// let address = PostalAddress::new("Berlin", "DE")?
///     .street("Unter den Linden")
///     .building_number("77")
///     .post_code("10117");
///
/// assert_eq!(address.town_name(), "Berlin");
/// assert_eq!(address.country(), "DE");
/// # Ok::<(), sepa::address::AddressError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PostalAddress {
    department: Option<String>,
    sub_department: Option<String>,
    street: Option<String>,
    building_number: Option<String>,
    post_code: Option<String>,
    town_name: String,
    country_subdivision: Option<String>,
    country: String,
    lines: Vec<String>,
}

impl PostalAddress {
    /// An address in `town`, in `country`.
    ///
    /// Both are required: from 15 November 2026 the EPC schemes reject an
    /// address that carries neither, so an address without them is not a form
    /// this crate will emit. `country` is an ISO 3166-1 alpha-2 code and is
    /// upper-cased.
    ///
    /// # Errors
    ///
    /// [`AddressError::EmptyTown`] for a blank town, or
    /// [`AddressError::UnknownCountry`] when `country` is not a country — the
    /// XSD's `CountryCode` type accepts any two letters, and `ZZ` addresses
    /// nowhere.
    ///
    /// # Examples
    ///
    /// ```
    /// use sepa::PostalAddress;
    ///
    /// assert_eq!(PostalAddress::new("Wien", "at")?.country(), "AT");
    /// assert!(PostalAddress::new("", "DE").is_err());
    /// assert!(PostalAddress::new("Atlantis", "ZZ").is_err());
    /// # Ok::<(), sepa::address::AddressError>(())
    /// ```
    pub fn new(town: impl Into<String>, country: &str) -> Result<Self, AddressError> {
        let town_name = town.into();
        if town_name.trim().is_empty() {
            return Err(AddressError::EmptyTown);
        }
        if !is_country_code(country) {
            return Err(AddressError::UnknownCountry {
                code: country.to_owned(),
            });
        }
        Ok(Self {
            department: None,
            sub_department: None,
            street: None,
            building_number: None,
            post_code: None,
            town_name,
            country_subdivision: None,
            country: country.to_ascii_uppercase(),
            lines: Vec::new(),
        })
    }

    /// Set `StrtNm` — the street name, without the number.
    #[must_use]
    pub fn street(mut self, street: impl Into<String>) -> Self {
        self.street = Some(street.into());
        self
    }

    /// Set `BldgNb` — the building number, without the street.
    #[must_use]
    pub fn building_number(mut self, number: impl Into<String>) -> Self {
        self.building_number = Some(number.into());
        self
    }

    /// Set `PstCd` — the postal code.
    #[must_use]
    pub fn post_code(mut self, code: impl Into<String>) -> Self {
        self.post_code = Some(code.into());
        self
    }

    /// Set `CtrySubDvsn` — state, province or canton.
    #[must_use]
    pub fn country_subdivision(mut self, subdivision: impl Into<String>) -> Self {
        self.country_subdivision = Some(subdivision.into());
        self
    }

    /// Set `Dept` — the department within the organisation.
    #[must_use]
    pub fn department(mut self, department: impl Into<String>) -> Self {
        self.department = Some(department.into());
        self
    }

    /// Set `SubDept` — the sub-department within the department.
    #[must_use]
    pub fn sub_department(mut self, sub_department: impl Into<String>) -> Self {
        self.sub_department = Some(sub_department.into());
        self
    }

    /// Append an `AdrLine`, making the address hybrid.
    ///
    /// Use this only for what the structured elements cannot hold. At most
    /// [`MAX_ADDRESS_LINES`] are permitted; a third is rejected by `build()`
    /// rather than silently dropped.
    #[must_use]
    pub fn line(mut self, line: impl Into<String>) -> Self {
        self.lines.push(line.into());
        self
    }

    /// The town name (`TwnNm`).
    #[must_use]
    pub fn town_name(&self) -> &str {
        &self.town_name
    }

    /// The ISO 3166-1 alpha-2 country code (`Ctry`), upper-cased.
    #[must_use]
    pub fn country(&self) -> &str {
        &self.country
    }

    /// The street name (`StrtNm`), if set.
    #[must_use]
    pub fn street_name(&self) -> Option<&str> {
        self.street.as_deref()
    }

    /// The free-text address lines (`AdrLine`), in order.
    #[must_use]
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    /// Whether this address is written structured or hybrid.
    ///
    /// # Examples
    ///
    /// ```
    /// use sepa::address::{AddressFormat, PostalAddress};
    ///
    /// let a = PostalAddress::new("Berlin", "DE")?.street("Hauptstrasse");
    /// assert_eq!(a.format(), AddressFormat::Structured);
    /// assert_eq!(a.line("2. OG").format(), AddressFormat::Hybrid);
    /// # Ok::<(), sepa::address::AddressError>(())
    /// ```
    #[must_use]
    pub fn format(&self) -> AddressFormat {
        if self.lines.is_empty() {
            AddressFormat::Structured
        } else {
            AddressFormat::Hybrid
        }
    }

    /// Validate against the ISO 20022 length limits and the EPC line cap.
    ///
    /// Field paths in the returned error are relative to the address
    /// (`PstlAdr/StrtNm`); the [`Location`](crate::Location) on the enclosing
    /// [`BuildError`](crate::BuildError) says which group or transaction — and
    /// hence which party — it belongs to.
    ///
    /// # Errors
    ///
    /// [`ValidationError::TooLong`] for an element over its `Max*Text` bound,
    /// [`ValidationError::Empty`] for a blank one, or
    /// [`ValidationError::InvalidCharacter`] under
    /// [`CharsetPolicy::Strict`].
    pub fn validate(&self, charset: CharsetPolicy) -> Result<(), ValidationError> {
        // Lengths are checked after transliteration, since `Straße` grows a
        // character on its way to `Strasse`.
        let check = |field: &'static str, value: &str, max: usize| -> Result<(), _> {
            check_text(field, &charset.apply(field, value)?, max)
        };

        check("PstlAdr/TwnNm", &self.town_name, MAX_35)?;
        for (field, value, max) in [
            ("PstlAdr/Dept", &self.department, MAX_70),
            ("PstlAdr/SubDept", &self.sub_department, MAX_70),
            ("PstlAdr/StrtNm", &self.street, MAX_70),
            ("PstlAdr/BldgNb", &self.building_number, MAX_16),
            ("PstlAdr/PstCd", &self.post_code, MAX_16),
            ("PstlAdr/CtrySubDvsn", &self.country_subdivision, MAX_35),
        ] {
            if let Some(value) = value {
                check(field, value, max)?;
            }
        }

        if self.lines.len() > MAX_ADDRESS_LINES {
            return Err(ValidationError::TooMany {
                field: "PstlAdr/AdrLine",
                max: MAX_ADDRESS_LINES,
                actual: self.lines.len(),
            });
        }
        for line in &self.lines {
            check("PstlAdr/AdrLine", line, MAX_70)?;
        }
        Ok(())
    }

    /// Write `<PstlAdr>…</PstlAdr>` in XSD sequence order, unindented.
    ///
    /// The order is the one `PostalAddress6` and `PostalAddress24` share, so a
    /// single writer serves every ISO schema this crate emits.
    pub(crate) fn write_xml<W: std::fmt::Write>(
        &self,
        w: &mut W,
        charset: CharsetPolicy,
    ) -> std::fmt::Result {
        fn element<W: std::fmt::Write>(
            w: &mut W,
            tag: &'static str,
            value: &str,
            charset: CharsetPolicy,
        ) -> std::fmt::Result {
            write!(w, "<{tag}>")?;
            write_escaped(w, &charset.render(value))?;
            write!(w, "</{tag}>")
        }

        w.write_str("<PstlAdr>")?;
        for (tag, value) in [
            ("Dept", &self.department),
            ("SubDept", &self.sub_department),
            ("StrtNm", &self.street),
            ("BldgNb", &self.building_number),
            ("PstCd", &self.post_code),
        ] {
            if let Some(value) = value {
                element(w, tag, value, charset)?;
            }
        }
        element(w, "TwnNm", &self.town_name, charset)?;
        if let Some(subdivision) = &self.country_subdivision {
            element(w, "CtrySubDvsn", subdivision, charset)?;
        }
        // `Ctry` is validated at construction and is two ASCII letters, so it
        // needs neither escaping nor transliteration.
        write!(w, "<Ctry>{}</Ctry>", self.country)?;
        for line in &self.lines {
            element(w, "AdrLine", line, charset)?;
        }
        w.write_str("</PstlAdr>")
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::{AddressError, AddressFormat, PostalAddress};
    use crate::validate::{CharsetPolicy, ValidationError};

    fn render(a: &PostalAddress) -> String {
        let mut out = String::new();
        a.write_xml(&mut out, CharsetPolicy::default()).unwrap();
        out
    }

    #[test]
    fn a_town_and_a_country_are_required_at_construction() {
        // The Nov 2026 rule, expressed as a type: an unstructured address is
        // not something this crate can be asked to emit.
        assert_eq!(PostalAddress::new("", "DE"), Err(AddressError::EmptyTown));
        assert_eq!(
            PostalAddress::new("   ", "DE"),
            Err(AddressError::EmptyTown)
        );
        assert_eq!(
            PostalAddress::new("Atlantis", "ZZ"),
            Err(AddressError::UnknownCountry {
                code: "ZZ".to_owned()
            })
        );
        // The XSD pattern is `[A-Z]{2}` and would accept both of these.
        assert!(PostalAddress::new("Nowhere", "QQ").is_err());
        assert!(PostalAddress::new("Berlin", "DEU").is_err());
    }

    #[test]
    fn the_country_code_is_normalised() {
        assert_eq!(PostalAddress::new("Wien", "at").unwrap().country(), "AT");
    }

    #[test]
    fn format_reports_structured_versus_hybrid() {
        let structured = PostalAddress::new("Berlin", "DE")
            .unwrap()
            .street("Unter den Linden")
            .building_number("77")
            .post_code("10117");
        assert_eq!(structured.format(), AddressFormat::Structured);
        assert_eq!(
            structured.clone().line("2. OG").format(),
            AddressFormat::Hybrid
        );
    }

    #[test]
    fn elements_are_written_in_xsd_sequence_order() {
        // PostalAddress6 and PostalAddress24 agree on this order, which is why
        // one writer covers every ISO schema.
        let xml = render(
            &PostalAddress::new("Berlin", "DE")
                .unwrap()
                .department("Buchhaltung")
                .sub_department("Kreditoren")
                .street("Unter den Linden")
                .building_number("77")
                .post_code("10117")
                .country_subdivision("BE")
                .line("Aufgang C"),
        );
        assert_eq!(
            xml,
            "<PstlAdr><Dept>Buchhaltung</Dept><SubDept>Kreditoren</SubDept>\
             <StrtNm>Unter den Linden</StrtNm><BldgNb>77</BldgNb><PstCd>10117</PstCd>\
             <TwnNm>Berlin</TwnNm><CtrySubDvsn>BE</CtrySubDvsn><Ctry>DE</Ctry>\
             <AdrLine>Aufgang C</AdrLine></PstlAdr>"
        );
    }

    #[test]
    fn the_minimal_address_is_town_and_country() {
        assert_eq!(
            render(&PostalAddress::new("Berlin", "DE").unwrap()),
            "<PstlAdr><TwnNm>Berlin</TwnNm><Ctry>DE</Ctry></PstlAdr>"
        );
    }

    #[test]
    fn text_is_transliterated_and_escaped() {
        let xml = render(
            &PostalAddress::new("München", "DE")
                .unwrap()
                .street("Straße & Weg"),
        );
        assert!(xml.contains("<StrtNm>Strasse + Weg</StrtNm>"));
        assert!(xml.contains("<TwnNm>Muenchen</TwnNm>"));
    }

    #[test]
    fn lengths_are_the_iso_maximums_and_are_measured_after_transliteration() {
        let policy = CharsetPolicy::default();
        let at = |a: PostalAddress| a.validate(policy);

        assert!(at(PostalAddress::new("A".repeat(35), "DE").unwrap()).is_ok());
        assert!(matches!(
            at(PostalAddress::new("A".repeat(36), "DE").unwrap()),
            Err(ValidationError::TooLong {
                field: "PstlAdr/TwnNm",
                max: 35,
                ..
            })
        ));
        assert!(matches!(
            at(PostalAddress::new("Berlin", "DE")
                .unwrap()
                .street("A".repeat(71))),
            Err(ValidationError::TooLong {
                field: "PstlAdr/StrtNm",
                max: 70,
                ..
            })
        ));
        assert!(matches!(
            at(PostalAddress::new("Berlin", "DE")
                .unwrap()
                .building_number("7".repeat(17))),
            Err(ValidationError::TooLong {
                field: "PstlAdr/BldgNb",
                max: 16,
                ..
            })
        ));

        // 35 'ü' is 35 characters but 70 in the German style — the limit binds
        // on the transliterated value, which is what the bank receives.
        assert!(matches!(
            at(PostalAddress::new("ü".repeat(35), "DE").unwrap()),
            Err(ValidationError::TooLong {
                max: 35,
                actual: 70,
                ..
            })
        ));
        assert!(at(PostalAddress::new("ü".repeat(17), "DE").unwrap()).is_ok());
    }

    #[test]
    fn a_third_address_line_is_rejected_rather_than_dropped() {
        let policy = CharsetPolicy::default();
        let two = PostalAddress::new("Berlin", "DE")
            .unwrap()
            .line("one")
            .line("two");
        assert!(two.validate(policy).is_ok());
        assert_eq!(
            two.line("three").validate(policy),
            Err(ValidationError::TooMany {
                field: "PstlAdr/AdrLine",
                max: 2,
                actual: 3,
            })
        );
    }

    #[test]
    fn a_blank_element_is_rejected() {
        assert!(matches!(
            PostalAddress::new("Berlin", "DE")
                .unwrap()
                .street("   ")
                .validate(CharsetPolicy::default()),
            Err(ValidationError::Empty {
                field: "PstlAdr/StrtNm"
            })
        ));
    }

    #[test]
    fn strict_charset_policy_rejects_rather_than_rewrites() {
        assert!(matches!(
            PostalAddress::new("München", "DE")
                .unwrap()
                .validate(CharsetPolicy::Strict),
            Err(ValidationError::InvalidCharacter {
                field: "PstlAdr/TwnNm",
                ch: 'ü'
            })
        ));
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_round_trips() {
        let a = PostalAddress::new("Berlin", "DE")
            .unwrap()
            .street("Unter den Linden")
            .line("Aufgang C");
        let json = serde_json::to_string(&a).unwrap();
        assert_eq!(serde_json::from_str::<PostalAddress>(&json).unwrap(), a);
    }
}
