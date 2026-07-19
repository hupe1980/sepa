//! ISO 20022 purpose codes — `Purp` and `CtgyPurp`.
//!
//! These are **two different code sets** that are easy to confuse, and mixing
//! them up produces a file the schema accepts and the bank rejects.
//!
//! | | [`Purpose`] (`Purp`) | [`CategoryPurpose`] (`CtgyPurp`) |
//! |---|---|---|
//! | Code set | `ExternalPurpose1Code` (325 codes) | `ExternalCategoryPurpose1Code` (43 codes) |
//! | Placement | transaction level only | inside `PmtTpInf`, at **one** level |
//! | Meaning | information for the counterparty | an instruction to the banks |
//!
//! The EPC guidelines put it plainly: `Purp` is *"a content element, which is
//! not used for processing by any of the agents"*, whereas `CtgyPurp` *"is
//! likely to trigger special processing by any of the agents involved in the
//! payment chain."*
//!
//! ## Codes that exist in only one of the two sets
//!
//! This is where implementations go wrong:
//!
//! - **`RENT` is not a valid category purpose** — it exists only as a purpose.
//!   The same applies to `COMM`, `PHON`, `EDUC`, `INSU`, `ELEC`, `GASB`,
//!   `WTER`, `IVPT` and `GDDS`.
//! - **`DIVI` vs `DIVD`** — both mean "dividend", but `DIVI` is category-only
//!   and `DIVD` is purpose-only. Trivial to swap, and wrong either way.
//! - Category-only: `CIPC`, `CONC`, `DIVI`, `FCDT`, `FCIN`, `GP2P`, `LBOX`,
//!   `RPRE`, `SWEP`, `TOPG`, `VOST`, `ZABA`.
//!
//! Because both lists are *external* code sets that ISO revises quarterly, the
//! enums here cover the codes in common use and carry an `Other` variant rather
//! than rejecting anything unrecognised. Length and character rules are still
//! enforced.
//!
//! ## German specifics
//!
//! The DFÜ-Abkommen makes one purpose code **mandatory**: if the remittance
//! information carries *(Alters-)Vermögenswirksame Leistungen*, `Purp` must be
//! [`Purpose::Cbff`] (capital-building) or [`Purpose::Cbfr`] (for retirement).
//!
//! German banks also derive the statement's *Geschäftsvorfallcode* from `Purp`,
//! not `CtgyPurp` — `BONU`, `PENS`, `SALA`, `PAYR` and `SPSP` all map to GVC
//! 153. The DK spec states that category purpose codes *"werden nicht im
//! Kontoauszug dargestellt"* and are ignored for that derivation, so the common
//! belief that `CtgyPurp = SALA` hides salary details on a statement is not
//! supported by the specification.
//!
//! ## Examples
//!
//! ```
//! use sepa::purpose::{CategoryPurpose, Purpose};
//!
//! assert_eq!(Purpose::Supp.as_code(), "SUPP");
//! assert_eq!(CategoryPurpose::Sala.as_code(), "SALA");
//!
//! // RENT is a purpose, never a category purpose.
//! assert!("RENT".parse::<Purpose>().is_ok());
//! assert!(matches!(
//!     "RENT".parse::<CategoryPurpose>().unwrap(),
//!     CategoryPurpose::Other(_)
//! ));
//! ```

use std::str::FromStr;

use crate::validate::ValidationError;

/// Error returned when a purpose code is structurally invalid.
///
/// Unrecognised-but-well-formed codes are **not** an error — they become
/// `Other`, because ISO revises these external code sets quarterly.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum PurposeCodeError {
    /// The code is not 1–4 characters.
    #[error("purpose code {code:?} must be 1–4 characters")]
    InvalidLength {
        /// The rejected code.
        code: String,
    },
    /// The code contains a character outside `A–Z` and `0–9`.
    #[error("purpose code {code:?} must be alphanumeric")]
    InvalidCharacter {
        /// The rejected code.
        code: String,
    },
}

/// Validate the shared shape of both code sets: 1–4 uppercase alphanumerics.
fn check_code(code: &str) -> Result<String, PurposeCodeError> {
    let upper = code.trim().to_ascii_uppercase();
    if upper.is_empty() || upper.chars().count() > 4 {
        return Err(PurposeCodeError::InvalidLength { code: upper });
    }
    if !upper.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(PurposeCodeError::InvalidCharacter { code: upper });
    }
    Ok(upper)
}

/// Build a code enum plus its `as_code` / `FromStr` / `Display` boilerplate.
macro_rules! code_enum {
    ($(#[$meta:meta])* $name:ident { $($(#[$vmeta:meta])* $variant:ident => $code:literal),* $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash)]
        #[non_exhaustive]
        #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
        pub enum $name {
            $($(#[$vmeta])* $variant,)*
            /// A code not listed here — these are external code sets that ISO
            /// revises quarterly, so unknown codes are carried through.
            Other(String),
        }

        impl $name {
            /// The four-letter ISO 20022 code.
            #[must_use]
            pub fn as_code(&self) -> &str {
                match self {
                    $(Self::$variant => $code,)*
                    Self::Other(s) => s,
                }
            }

            /// Validate this code against the ISO 20022 shape rules.
            ///
            /// # Errors
            ///
            /// Returns [`ValidationError::InvalidCharacter`] or
            /// [`ValidationError::TooLong`] for a malformed `Other` code.
            pub fn validate(&self, field: &'static str) -> Result<(), ValidationError> {
                let code = self.as_code();
                match check_code(code) {
                    Ok(_) => Ok(()),
                    Err(PurposeCodeError::InvalidLength { .. }) => {
                        Err(ValidationError::TooLong {
                            field,
                            max: 4,
                            actual: code.chars().count(),
                        })
                    }
                    Err(PurposeCodeError::InvalidCharacter { .. }) => {
                        let ch = code.chars().find(|c| !c.is_ascii_alphanumeric()).unwrap_or('?');
                        Err(ValidationError::InvalidCharacter { field, ch })
                    }
                }
            }
        }

        impl FromStr for $name {
            type Err = PurposeCodeError;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                let upper = check_code(s)?;
                Ok(match upper.as_str() {
                    $($code => Self::$variant,)*
                    _ => Self::Other(upper),
                })
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_code())
            }
        }
    };
}

code_enum! {
    /// A `Purp/Cd` value — `ExternalPurpose1Code`.
    ///
    /// Placed at **transaction level only**, in `CdtTrfTxInf/Purp` or
    /// `DrctDbtTxInf/Purp`. Informational: it is passed to the counterparty and
    /// does not instruct the banks.
    Purpose {
        /// `SALA` — salary payment.
        Sala => "SALA",
        /// `BONU` — bonus payment.
        Bonu => "BONU",
        /// `PENS` — pension payment.
        Pens => "PENS",
        /// `PAYR` — payroll.
        Payr => "PAYR",
        /// `SPSP` — salary/pension, savings plan.
        Spsp => "SPSP",
        /// `SUPP` — supplier payment.
        Supp => "SUPP",
        /// `TAXS` — tax payment.
        Taxs => "TAXS",
        /// `VATX` — value-added tax payment.
        Vatx => "VATX",
        /// `GDDS` — purchase and sale of goods.
        Gdds => "GDDS",
        /// `SCVE` — purchase and sale of services.
        Scve => "SCVE",
        /// `RENT` — rent. **Not valid as a category purpose.**
        Rent => "RENT",
        /// `LOAN` — loan.
        Loan => "LOAN",
        /// `INSU` — insurance premium.
        Insu => "INSU",
        /// `ELEC` — electricity bill.
        Elec => "ELEC",
        /// `GASB` — gas bill.
        Gasb => "GASB",
        /// `WTER` — water bill.
        Wter => "WTER",
        /// `PHON` — telephone bill.
        Phon => "PHON",
        /// `EDUC` — education.
        Educ => "EDUC",
        /// `CHAR` — charity payment.
        Char => "CHAR",
        /// `DIVD` — dividend. Note: the *category* purpose spelling is `DIVI`.
        Divd => "DIVD",
        /// `INTE` — interest.
        Inte => "INTE",
        /// `GOVT` — government payment.
        Govt => "GOVT",
        /// `SSBE` — social security benefit.
        Ssbe => "SSBE",
        /// `BENE` — unemployment or disability benefit.
        Bene => "BENE",
        /// `IVPT` — invoice payment.
        Ivpt => "IVPT",
        /// `RINP` — recurring instalment payment.
        Rinp => "RINP",
        /// `TRAD` — trade services.
        Trad => "TRAD",
        /// `TREA` — treasury payment.
        Trea => "TREA",
        /// `CASH` — cash management transfer.
        Cash => "CASH",
        /// `INTC` — intra-company payment.
        Intc => "INTC",
        /// `CBFF` — capital-building fringe fortune.
        ///
        /// **Mandatory in Germany** when the remittance information carries
        /// *Vermögenswirksame Leistungen* (DFÜ-Abkommen Anlage 3).
        Cbff => "CBFF",
        /// `CBFR` — capital-building fringe fortune for retirement.
        ///
        /// **Mandatory in Germany** for *Altersvermögenswirksame Leistungen*.
        Cbfr => "CBFR",
        /// `RRCT` — reimbursement of a previous credit transfer.
        Rrct => "RRCT",
        /// `RRTP` — transfer resulting from a Request to Pay.
        Rrtp => "RRTP",
        /// `OTHR` — other.
        Othr => "OTHR",
    }
}

code_enum! {
    /// A `CtgyPurp/Cd` value — `ExternalCategoryPurpose1Code`.
    ///
    /// Placed inside `PmtTpInf`, at **either** payment-information or
    /// transaction level but never both. Unlike [`Purpose`], this is an
    /// instruction that may trigger special handling by the banks.
    CategoryPurpose {
        /// `SALA` — salary payment.
        Sala => "SALA",
        /// `PENS` — pension payment.
        Pens => "PENS",
        /// `SUPP` — supplier payment.
        Supp => "SUPP",
        /// `TAXS` — tax payment.
        Taxs => "TAXS",
        /// `TREA` — treasury payment.
        Trea => "TREA",
        /// `CASH` — cash management transfer.
        Cash => "CASH",
        /// `INTC` — intra-company payment.
        Intc => "INTC",
        /// `GOVT` — government payment.
        Govt => "GOVT",
        /// `LOAN` — loan.
        Loan => "LOAN",
        /// `BONU` — bonus payment.
        Bonu => "BONU",
        /// `SSBE` — social security benefit.
        Ssbe => "SSBE",
        /// `TRAD` — trade services.
        Trad => "TRAD",
        /// `EPAY` — epayment.
        Epay => "EPAY",
        /// `CORT` — trade settlement payment.
        Cort => "CORT",
        /// `HEDG` — hedging.
        Hedg => "HEDG",
        /// `INTE` — interest.
        Inte => "INTE",
        /// `SECU` — securities.
        Secu => "SECU",
        /// `VATX` — value-added tax payment.
        Vatx => "VATX",
        /// `WHLD` — withholding tax.
        Whld => "WHLD",
        /// `DIVI` — dividend. Note: the *purpose* spelling is `DIVD`.
        Divi => "DIVI",
        /// `CIPC` — cash in, physical cash.
        Cipc => "CIPC",
        /// `CONC` — cash concentration.
        Conc => "CONC",
        /// `SWEP` — sweep account.
        Swep => "SWEP",
        /// `TOPG` — top-up account.
        Topg => "TOPG",
        /// `ZABA` — zero-balance account.
        Zaba => "ZABA",
        /// `LBOX` — lockbox.
        Lbox => "LBOX",
        /// `OTHR` — other.
        Othr => "OTHR",
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_round_trip() {
        for code in ["SALA", "SUPP", "RENT", "CBFF", "OTHR"] {
            assert_eq!(code.parse::<Purpose>().unwrap().as_code(), code);
        }
        for code in ["SALA", "INTC", "DIVI", "OTHR"] {
            assert_eq!(code.parse::<CategoryPurpose>().unwrap().as_code(), code);
        }
    }

    #[test]
    fn the_two_code_sets_are_genuinely_different() {
        // RENT is a purpose, never a category purpose.
        assert_eq!("RENT".parse::<Purpose>().unwrap(), Purpose::Rent);
        assert_eq!(
            "RENT".parse::<CategoryPurpose>().unwrap(),
            CategoryPurpose::Other("RENT".to_owned())
        );

        // DIVD is the purpose spelling; DIVI is the category spelling.
        assert_eq!("DIVD".parse::<Purpose>().unwrap(), Purpose::Divd);
        assert_eq!(
            "DIVI".parse::<CategoryPurpose>().unwrap(),
            CategoryPurpose::Divi
        );
        // …and each is unknown to the other set.
        assert!(matches!(
            "DIVI".parse::<Purpose>().unwrap(),
            Purpose::Other(_)
        ));
        assert!(matches!(
            "DIVD".parse::<CategoryPurpose>().unwrap(),
            CategoryPurpose::Other(_)
        ));

        // Category-only codes are not purposes.
        for c in ["CIPC", "CONC", "SWEP", "TOPG", "ZABA", "LBOX"] {
            assert!(
                matches!(c.parse::<Purpose>().unwrap(), Purpose::Other(_)),
                "{c} is category-only"
            );
        }
    }

    #[test]
    fn unknown_but_well_formed_codes_are_carried_through() {
        // These are external code sets revised quarterly; rejecting unknown
        // codes would break the crate every time ISO publishes an update.
        assert_eq!(
            "ZZZZ".parse::<Purpose>().unwrap(),
            Purpose::Other("ZZZZ".to_owned())
        );
        assert_eq!("zzzz".parse::<Purpose>().unwrap().as_code(), "ZZZZ");
    }

    #[test]
    fn malformed_codes_are_rejected() {
        for bad in ["", "TOOLONG", "SAL A", "SA-A"] {
            assert!(bad.parse::<Purpose>().is_err(), "{bad:?} must be rejected");
            assert!(bad.parse::<CategoryPurpose>().is_err());
        }
        // 1–4 characters is the permitted range.
        assert!("A".parse::<Purpose>().is_ok());
        assert!("ABCD".parse::<Purpose>().is_ok());
        assert!("ABCDE".parse::<Purpose>().is_err());
    }

    #[test]
    fn validate_catches_a_hand_built_other() {
        // `Other` can be constructed directly, so validation must still check it.
        assert!(matches!(
            Purpose::Other("TOOLONG".to_owned()).validate("Purp/Cd"),
            Err(ValidationError::TooLong { .. })
        ));
        assert!(matches!(
            Purpose::Other("A-B".to_owned()).validate("Purp/Cd"),
            Err(ValidationError::InvalidCharacter { .. })
        ));
        assert!(Purpose::Supp.validate("Purp/Cd").is_ok());
    }

    #[test]
    fn display_matches_the_code() {
        assert_eq!(Purpose::Cbff.to_string(), "CBFF");
        assert_eq!(CategoryPurpose::Sala.to_string(), "SALA");
        assert_eq!(Purpose::Other("XYZ".to_owned()).to_string(), "XYZ");
    }
}
