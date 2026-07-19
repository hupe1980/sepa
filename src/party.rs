//! Ultimate parties — `UltmtDbtr` and `UltmtCdtr`.
//!
//! An ultimate party names the person the payment is *really* from or for, when
//! that differs from the account holder: a subsidiary paying through a parent's
//! account, a utility collecting for a landlord, a child's account debited via
//! a parent's mandate.
//!
//! ISO 20022 calls these elements informational — they are carried to the
//! counterparty and do not affect routing or settlement.
//!
//! ## Where they are allowed
//!
//! The two messages are **asymmetric**, and the schema enforces it:
//!
//! | | `PmtInf` level | transaction level |
//! |---|---|---|
//! | pain.001 `UltmtDbtr` | ✅ | ✅ |
//! | pain.001 `UltmtCdtr` | ❌ **absent from the schema** | ✅ |
//! | pain.008 `UltmtCdtr` | ✅ | ✅ |
//! | pain.008 `UltmtDbtr` | ❌ **absent from the schema** | ✅ |
//!
//! Where both levels are allowed, the DFÜ-Abkommen adds a rule the schema
//! cannot express: *"Wenn diese Feldgruppe belegt ist, dann darf sie auf
//! Einzeltransaktionsebene nicht gefüllt sein."* — set it at one level or the
//! other, never both. [`crate::validate`] enforces this.
//!
//! ## What may go inside
//!
//! The DK technical validation subset derives the party type by restriction and
//! keeps only two children — `Nm` and `Id`. `PstlAdr`, `CtryOfRes` and
//! `CtctDtls` are **removed from the schema** for SEPA ultimate parties, so
//! emitting them fails validation even though plain ISO 20022 permits them.
//!
//! `Nm` is typed `Max140Text` but the EPC caps it at **70 characters** — a
//! limit the XSD will not catch.
//!
//! A note on `Id`: the DK recommends against it. For pain.008 `UltmtCdtr` it
//! says *"Es wird empfohlen, diese Feldgruppe nicht zu belegen"*, and for
//! pain.001 `PmtInf/UltmtDbtr` it requires bilateral agreement with the bank.
//! Name-only is the safe default; treat [`Party::with_organisation_id`] and
//! [`Party::with_private_id`] as opt-in.
//!
//! ## Examples
//!
//! ```
//! use sepa::party::Party;
//!
//! // The common case — a name.
//! let p = Party::new("Endbeguenstigter GmbH");
//! assert_eq!(p.name(), Some("Endbeguenstigter GmbH"));
//!
//! // With an organisation identifier, when the bank has agreed to it.
//! let p = Party::new("Tochter AG").with_organisation_id("CUST-4711", Some("CUST"));
//! assert!(p.identifier().is_some());
//! ```

use crate::validate::{CharsetPolicy, ValidationError, check_name};
use crate::xml_util::write_escaped;

/// How an ultimate party's identifier is classified.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum IdentifierKind {
    /// `Id/OrgId/Othr` — an organisation, e.g. a customer or supplier number.
    Organisation,
    /// `Id/PrvtId/Othr` — a natural person, e.g. a membership number.
    Private,
}

/// An identifier attached to an ultimate party.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PartyIdentifier {
    /// Whether this identifies an organisation or a private person.
    pub kind: IdentifierKind,
    /// The identifier value (`Othr/Id`), max 35 characters.
    pub id: String,
    /// Optional proprietary scheme name (`Othr/SchmeNm/Prtry`), max 35 characters.
    pub scheme_name: Option<String>,
}

/// An ultimate debtor or ultimate creditor.
///
/// Carries a name and, optionally, an identifier — the only two children SEPA
/// permits. At least one of the two must be present.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Party {
    name: Option<String>,
    identifier: Option<PartyIdentifier>,
}

impl Party {
    /// A party identified by name — the usual case.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: Some(name.into()),
            identifier: None,
        }
    }

    /// A party identified only by an organisation identifier, with no name.
    pub fn organisation_id(id: impl Into<String>, scheme_name: Option<&str>) -> Self {
        Self {
            name: None,
            identifier: Some(PartyIdentifier {
                kind: IdentifierKind::Organisation,
                id: id.into(),
                scheme_name: scheme_name.map(str::to_owned),
            }),
        }
    }

    /// Attach an organisation identifier (`Id/OrgId/Othr`).
    ///
    /// Opt-in: German banks recommend against populating `Id` at all, and for
    /// some positions require bilateral agreement first.
    #[must_use]
    pub fn with_organisation_id(
        mut self,
        id: impl Into<String>,
        scheme_name: Option<&str>,
    ) -> Self {
        self.identifier = Some(PartyIdentifier {
            kind: IdentifierKind::Organisation,
            id: id.into(),
            scheme_name: scheme_name.map(str::to_owned),
        });
        self
    }

    /// Attach a private-person identifier (`Id/PrvtId/Othr`).
    #[must_use]
    pub fn with_private_id(mut self, id: impl Into<String>, scheme_name: Option<&str>) -> Self {
        self.identifier = Some(PartyIdentifier {
            kind: IdentifierKind::Private,
            id: id.into(),
            scheme_name: scheme_name.map(str::to_owned),
        });
        self
    }

    /// The party's name, if set.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// The party's identifier, if set.
    #[must_use]
    pub fn identifier(&self) -> Option<&PartyIdentifier> {
        self.identifier.as_ref()
    }

    /// Validate against the EPC and DK rules for ultimate parties.
    ///
    /// # Errors
    ///
    /// Returns [`ValidationError::Empty`] when neither a name nor an identifier
    /// is present, or a length/character error for either field.
    pub fn validate(
        &self,
        field: &'static str,
        charset: CharsetPolicy,
    ) -> Result<(), ValidationError> {
        if self.name.is_none() && self.identifier.is_none() {
            return Err(ValidationError::Empty { field });
        }
        if let Some(name) = &self.name {
            // 70, not the schema's 140 — the EPC limit the XSD cannot catch.
            check_name(field, &charset.apply(field, name)?)?;
        }
        if let Some(id) = &self.identifier {
            crate::validate::check_id(field, &id.id)?;
            if let Some(scheme) = &id.scheme_name {
                crate::validate::check_id(field, scheme)?;
            }
        }
        Ok(())
    }

    /// Write this party as `<tag>…</tag>`.
    pub(crate) fn write_xml<W: std::fmt::Write>(
        &self,
        w: &mut W,
        tag: &str,
        indent: &str,
        charset: CharsetPolicy,
    ) -> std::fmt::Result {
        write!(w, "{indent}<{tag}>")?;
        if let Some(name) = &self.name {
            let name = charset
                .apply("Nm", name)
                .unwrap_or(std::borrow::Cow::Borrowed(name));
            w.write_str("<Nm>")?;
            write_escaped(w, &name)?;
            w.write_str("</Nm>")?;
        }
        if let Some(id) = &self.identifier {
            let branch = match id.kind {
                IdentifierKind::Organisation => "OrgId",
                IdentifierKind::Private => "PrvtId",
            };
            write!(w, "<Id><{branch}><Othr><Id>")?;
            write_escaped(w, &id.id)?;
            w.write_str("</Id>")?;
            if let Some(scheme) = &id.scheme_name {
                w.write_str("<SchmeNm><Prtry>")?;
                write_escaped(w, scheme)?;
                w.write_str("</Prtry></SchmeNm>")?;
            }
            write!(w, "</Othr></{branch}></Id>")?;
        }
        writeln!(w, "</{tag}>")
    }
}

impl From<&str> for Party {
    fn from(name: &str) -> Self {
        Self::new(name)
    }
}

impl From<String> for Party {
    fn from(name: String) -> Self {
        Self::new(name)
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn render(p: &Party, tag: &str) -> String {
        let mut out = String::new();
        p.write_xml(&mut out, tag, "", CharsetPolicy::default())
            .unwrap();
        out
    }

    #[test]
    fn name_only_is_the_common_case() {
        let xml = render(&Party::new("Endbeguenstigter GmbH"), "UltmtCdtr");
        assert_eq!(
            xml,
            "<UltmtCdtr><Nm>Endbeguenstigter GmbH</Nm></UltmtCdtr>\n"
        );
    }

    #[test]
    fn organisation_and_private_identifiers_use_different_branches() {
        let org = render(
            &Party::new("Tochter AG").with_organisation_id("CUST-4711", Some("CUST")),
            "UltmtDbtr",
        );
        assert!(org.contains("<Id><OrgId><Othr><Id>CUST-4711</Id>"));
        assert!(org.contains("<SchmeNm><Prtry>CUST</Prtry></SchmeNm>"));

        let prv = render(
            &Party::new("Max Mustermann").with_private_id("MEMBER-9", None),
            "UltmtDbtr",
        );
        assert!(prv.contains("<Id><PrvtId><Othr><Id>MEMBER-9</Id>"));
        assert!(!prv.contains("SchmeNm"), "no scheme name was supplied");
    }

    #[test]
    fn only_nm_and_id_are_ever_emitted() {
        // SEPA strips PstlAdr / CtryOfRes / CtctDtls from the party type, so
        // nothing else may appear.
        let xml = render(
            &Party::new("Test").with_organisation_id("X", Some("Y")),
            "UltmtDbtr",
        );
        for forbidden in ["PstlAdr", "CtryOfRes", "CtctDtls", "AnyBIC"] {
            assert!(!xml.contains(forbidden), "{forbidden} must not be emitted");
        }
    }

    #[test]
    fn name_is_capped_at_seventy_not_the_schema_one_forty() {
        let policy = CharsetPolicy::default();
        assert!(
            Party::new("A".repeat(70))
                .validate("UltmtDbtr/Nm", policy)
                .is_ok()
        );
        assert!(matches!(
            Party::new("A".repeat(71)).validate("UltmtDbtr/Nm", policy),
            Err(ValidationError::TooLong { max: 70, .. })
        ));
    }

    #[test]
    fn an_empty_party_is_rejected() {
        assert!(matches!(
            Party::default().validate("UltmtDbtr", CharsetPolicy::default()),
            Err(ValidationError::Empty { .. })
        ));
    }

    #[test]
    fn identifier_respects_max35text() {
        let policy = CharsetPolicy::default();
        assert!(
            Party::organisation_id("X".repeat(35), None)
                .validate("UltmtDbtr", policy)
                .is_ok()
        );
        assert!(matches!(
            Party::organisation_id("X".repeat(36), None).validate("UltmtDbtr", policy),
            Err(ValidationError::TooLong { max: 35, .. })
        ));
    }

    #[test]
    fn names_are_transliterated_and_escaped() {
        let xml = render(&Party::new("Müller & Söhne"), "UltmtCdtr");
        assert!(xml.contains("<Nm>Mueller + Soehne</Nm>"));
    }
}
