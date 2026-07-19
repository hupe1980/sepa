//! EPC field-level rules for outgoing pain.001 / pain.008 messages.
//!
//! Almost none of these constraints live in the XSD. The published ISO schemas
//! allow amounts of `0`, five decimal places, any ISO 4217 currency, 140-character
//! names and arbitrary Unicode — so a file can validate cleanly against the
//! schema and still be rejected by the bank on ingestion. The rules here come
//! from the EPC Implementation Guidelines and the DK DFÜ-Abkommen, and they are
//! what actually determines whether a batch is accepted.
//!
//! | Field | Rule | Source |
//! |---|---|---|
//! | `MsgId`, `PmtInfId`, `EndToEndId`, `MndtId` | 1–35 chars, no leading/trailing `/`, no `//` | `Max35Text`, EPC230-15 |
//! | `Nm` (any party) | 1–70 chars (XSD permits 140) | EPC IG |
//! | `Ustrd` | 1–140 chars, one occurrence | EPC IG |
//! | `InstdAmt` | 0.01 – 999,999,999.99 EUR, 2 decimals | EPC IG §2.95 |
//! | Dates | `YYYY-MM-DD`, real calendar date | `ISODate` |
//! | Batch | at least one transaction | `CdtTrfTxInf`/`DrctDbtTxInf` are `1..n` |
//!
//! The amount ceiling is uniform across SCT, SCT Inst, SDD Core and SDD B2B.
//! The old 100,000 EUR SCT Inst cap was removed from the scheme on
//! 5 October 2025 (2025 SCT Inst Rulebook, following Art 5a(6) of the amended
//! SEPA Regulation), so it is deliberately not enforced here — a lower ceiling
//! is now a PSP or user policy limit, not a scheme rule.

use crate::charset::{Transliteration, first_invalid_char, transliterate};

// ── limits ────────────────────────────────────────────────────────────────────

/// Maximum length of an ISO 20022 `Max35Text` identifier.
pub const MAX_ID_LEN: usize = 35;
/// Maximum length of a party name (`Nm`) under the EPC guidelines.
pub const MAX_NAME_LEN: usize = 70;
/// Maximum length of unstructured remittance information (`Ustrd`).
pub const MAX_REMITTANCE_LEN: usize = 140;
/// Smallest permitted amount: 0.01 EUR.
pub const MIN_AMOUNT_CT: i64 = 1;
/// Largest permitted amount: 999,999,999.99 EUR.
pub const MAX_AMOUNT_CT: i64 = 99_999_999_999;

// ── error ─────────────────────────────────────────────────────────────────────

/// A field-level violation of the EPC rules for a payment batch.
///
/// Every variant names the offending `field` using its ISO 20022 element path,
/// so the message points at the exact element a bank would reject.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ValidationError {
    /// The batch contains no transactions.
    ///
    /// `CdtTrfTxInf` and `DrctDbtTxInf` are both `1..n`, so an empty batch is
    /// not merely useless — it is schema-invalid.
    #[error("batch contains no transactions: at least one is required")]
    EmptyBatch,

    /// A mandatory field was empty or blank.
    #[error("{field} must not be empty")]
    Empty {
        /// ISO 20022 element path of the offending field.
        field: &'static str,
    },

    /// A field exceeded its maximum length.
    #[error("{field} is {actual} characters, exceeding the maximum of {max}")]
    TooLong {
        /// ISO 20022 element path of the offending field.
        field: &'static str,
        /// Maximum permitted length in characters.
        max: usize,
        /// Actual length in characters.
        actual: usize,
    },

    /// A field contained a character outside the SEPA Basic Latin set.
    ///
    /// Only produced under [`CharsetPolicy::Strict`]; the default policy
    /// transliterates instead.
    #[error("{field} contains {ch:?}, which is not in the SEPA character set")]
    InvalidCharacter {
        /// ISO 20022 element path of the offending field.
        field: &'static str,
        /// The first offending character.
        ch: char,
    },

    /// An identifier began or ended with `/`, or contained `//`.
    #[error("{field} must not start or end with '/' nor contain '//' (EPC230-15)")]
    SlashRule {
        /// ISO 20022 element path of the offending field.
        field: &'static str,
    },

    /// An amount fell outside 0.01 – 999,999,999.99 EUR.
    #[error("{field} is {amount_ct} ct, outside the permitted 1 – {MAX_AMOUNT_CT} ct")]
    AmountOutOfRange {
        /// ISO 20022 element path of the offending field.
        field: &'static str,
        /// The rejected amount in cents.
        amount_ct: i64,
    },

    /// The sum of a batch's amounts overflowed `i64`.
    #[error("batch control sum overflows i64")]
    ControlSumOverflow,

    /// A date was not a valid `YYYY-MM-DD` calendar date.
    #[error("{field} must be a valid YYYY-MM-DD date, got {value:?}")]
    InvalidDate {
        /// ISO 20022 element path of the offending field.
        field: &'static str,
        /// The rejected value.
        value: String,
    },

    /// The same element was set at both payment-information and transaction
    /// level, where the rules permit one or the other.
    #[error("{field} must be set at either payment-information or transaction level, not both")]
    ConflictingLevels {
        /// The ISO 20022 element name.
        field: &'static str,
    },

    /// A SEPA Direct Debit batch had no Creditor Identifier.
    ///
    /// `CdtrSchmeId` is mandatory for SDD — the EPC guidelines require it at
    /// either payment-information or transaction level.
    #[error("SEPA Direct Debit requires a Creditor Identifier (CdtrSchmeId)")]
    MissingCreditorId,
}

/// Error returned when streaming a batch to an [`io::Write`](std::io::Write) target.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum WriteError {
    /// The batch violates an EPC rule; nothing was written.
    #[error(transparent)]
    Validation(#[from] ValidationError),

    /// The underlying writer failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

// ── charset policy ────────────────────────────────────────────────────────────

/// How a builder handles text outside the SEPA Basic Latin character set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CharsetPolicy {
    /// Convert unsupported characters automatically (default).
    ///
    /// Length limits are checked **after** conversion, since German-style
    /// transliteration can lengthen a string (`Müller` → `Mueller`).
    Transliterate(Transliteration),
    /// Reject unsupported characters with [`ValidationError::InvalidCharacter`].
    ///
    /// Use when the caller has already sanitised its data and wants a hard
    /// guarantee that nothing is silently rewritten.
    Strict,
}

impl CharsetPolicy {
    /// Apply this policy to `text`, returning SEPA-legal output.
    ///
    /// # Errors
    ///
    /// Under [`CharsetPolicy::Strict`], returns [`ValidationError::InvalidCharacter`]
    /// for the first character outside the SEPA set.
    pub fn apply<'a>(
        self,
        field: &'static str,
        text: &'a str,
    ) -> Result<std::borrow::Cow<'a, str>, ValidationError> {
        match self {
            Self::Transliterate(style) => Ok(transliterate(text, style)),
            Self::Strict => match first_invalid_char(text) {
                Some(ch) => Err(ValidationError::InvalidCharacter { field, ch }),
                None => Ok(std::borrow::Cow::Borrowed(text)),
            },
        }
    }
}

impl Default for CharsetPolicy {
    fn default() -> Self {
        Self::Transliterate(Transliteration::German)
    }
}

// ── field checks ──────────────────────────────────────────────────────────────

/// Validate a `Max35Text` identifier: non-empty, ≤35 chars, slash rules.
///
/// # Errors
///
/// See [`ValidationError`].
pub fn check_id(field: &'static str, value: &str) -> Result<(), ValidationError> {
    check_len(field, value, MAX_ID_LEN)?;

    // Identifiers are never transliterated, whatever the CharsetPolicy says: an
    // identifier is a key the bank echoes back in pain.002 and camt.05x, so
    // silently rewriting `MND-Straße` to `MND-Strasse` would break the caller's
    // own reconciliation. Out-of-set characters are a hard error instead.
    if let Some(ch) = first_invalid_char(value) {
        return Err(ValidationError::InvalidCharacter { field, ch });
    }

    // EPC230-15 applies the slash rules to references and identifiers only.
    // Checked on the trimmed value, since XSD whitespace collapse would strip
    // surrounding spaces and expose a leading or trailing slash.
    let trimmed = value.trim();
    if trimmed.starts_with('/') || trimmed.ends_with('/') || trimmed.contains("//") {
        return Err(ValidationError::SlashRule { field });
    }
    Ok(())
}

/// Validate a party name: non-empty, ≤70 characters.
///
/// # Errors
///
/// See [`ValidationError`].
pub fn check_name(field: &'static str, value: &str) -> Result<(), ValidationError> {
    check_len(field, value, MAX_NAME_LEN)
}

/// Validate unstructured remittance information: non-empty, ≤140 characters.
///
/// # Errors
///
/// See [`ValidationError`].
pub fn check_remittance(field: &'static str, value: &str) -> Result<(), ValidationError> {
    check_len(field, value, MAX_REMITTANCE_LEN)
}

/// Non-empty and within `max` **characters** (not bytes).
fn check_len(field: &'static str, value: &str, max: usize) -> Result<(), ValidationError> {
    if value.trim().is_empty() {
        return Err(ValidationError::Empty { field });
    }
    // Counted in chars: the ISO limits are character limits, and a byte-based
    // check would both mis-measure and risk slicing a multi-byte character.
    let actual = value.chars().count();
    if actual > max {
        return Err(ValidationError::TooLong { field, max, actual });
    }
    Ok(())
}

/// Validate an amount in cents against the SEPA range 0.01 – 999,999,999.99.
///
/// # Errors
///
/// Returns [`ValidationError::AmountOutOfRange`] outside that range. Note this
/// rejects zero and all negative amounts.
pub fn check_amount(field: &'static str, amount_ct: i64) -> Result<(), ValidationError> {
    if !(MIN_AMOUNT_CT..=MAX_AMOUNT_CT).contains(&amount_ct) {
        return Err(ValidationError::AmountOutOfRange { field, amount_ct });
    }
    Ok(())
}

/// Validate an ISO 8601 `YYYY-MM-DD` calendar date.
///
/// Rejects malformed strings and impossible dates such as `2026-02-30`.
///
/// # Errors
///
/// Returns [`ValidationError::InvalidDate`].
pub fn check_date(field: &'static str, value: &str) -> Result<(), ValidationError> {
    if is_valid_iso_date(value) {
        return Ok(());
    }
    Err(ValidationError::InvalidDate {
        field,
        value: value.to_owned(),
    })
}

fn is_valid_iso_date(s: &str) -> bool {
    // Destructure the exact `YYYY-MM-DD` shape: this rejects any other length,
    // separator or non-digit without a single fallible index.
    let [y0, y1, y2, y3, b'-', m0, m1, b'-', d0, d1] = *s.as_bytes() else {
        return false;
    };
    let digits = [y0, y1, y2, y3, m0, m1, d0, d1];
    if !digits.iter().all(u8::is_ascii_digit) {
        return false;
    }
    let val = |bytes: &[u8]| -> u32 {
        bytes
            .iter()
            .fold(0u32, |acc, b| acc * 10 + u32::from(b - b'0'))
    };
    let (y, m, d) = (val(&[y0, y1, y2, y3]), val(&[m0, m1]), val(&[d0, d1]));
    // `xs:date` has no year zero, so "0000-01-01" is schema-invalid even though
    // it parses — and year 0 would otherwise test as a leap year.
    if y < 1 || !(1..=12).contains(&m) || d < 1 {
        return false;
    }
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let max_day = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if leap => 29,
        _ => 28,
    };
    d <= max_day
}

/// Truncate `s` to at most `max` **characters**, never splitting one.
///
/// Slicing by byte index (`&s[..140]`) panics whenever the boundary lands
/// inside a multi-byte character — which a German remittance line reaches
/// routinely — and would mis-measure the limit even when it did not panic,
/// since ISO 20022 counts characters.
///
/// Returns `Cow::Borrowed` when no truncation is needed.
#[must_use]
pub fn truncate_chars(s: &str, max: usize) -> std::borrow::Cow<'_, str> {
    match s.char_indices().nth(max) {
        Some((byte_idx, _)) => std::borrow::Cow::Borrowed(&s[..byte_idx]),
        None => std::borrow::Cow::Borrowed(s),
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_respect_length_and_slash_rules() {
        assert!(check_id("MsgId", "DD-2026-07-001").is_ok());
        assert!(check_id("MsgId", &"X".repeat(35)).is_ok());

        assert_eq!(
            check_id("MsgId", &"X".repeat(36)),
            Err(ValidationError::TooLong {
                field: "MsgId",
                max: 35,
                actual: 36
            })
        );
        assert_eq!(
            check_id("MsgId", ""),
            Err(ValidationError::Empty { field: "MsgId" })
        );
        assert_eq!(
            check_id("MsgId", "   "),
            Err(ValidationError::Empty { field: "MsgId" })
        );
        for bad in ["/leading", "trailing/", "double//slash"] {
            assert_eq!(
                check_id("EndToEndId", bad),
                Err(ValidationError::SlashRule {
                    field: "EndToEndId"
                }),
                "{bad:?} must violate the slash rule"
            );
        }
        // A single interior slash is fine.
        assert!(check_id("EndToEndId", "INV/2026/07").is_ok());
    }

    #[test]
    fn lengths_are_counted_in_characters_not_bytes() {
        // 70 'ä' is 140 bytes but only 70 characters — must be accepted.
        let name = "ä".repeat(70);
        assert_eq!(name.len(), 140);
        assert!(check_name("Nm", &name).is_ok());
        assert!(check_name("Nm", &"ä".repeat(71)).is_err());
    }

    #[test]
    fn name_and_remittance_limits() {
        assert!(check_name("Cdtr/Nm", &"A".repeat(70)).is_ok());
        assert!(check_name("Cdtr/Nm", &"A".repeat(71)).is_err());
        assert!(check_remittance("Ustrd", &"A".repeat(140)).is_ok());
        assert!(check_remittance("Ustrd", &"A".repeat(141)).is_err());
    }

    #[test]
    fn amount_range_matches_the_epc_rule() {
        assert!(check_amount("InstdAmt", 1).is_ok());
        assert!(check_amount("InstdAmt", MAX_AMOUNT_CT).is_ok());

        for bad in [0, -1, MAX_AMOUNT_CT + 1, i64::MIN, i64::MAX] {
            assert_eq!(
                check_amount("InstdAmt", bad),
                Err(ValidationError::AmountOutOfRange {
                    field: "InstdAmt",
                    amount_ct: bad
                }),
                "{bad} must be rejected"
            );
        }
    }

    #[test]
    fn sct_inst_100k_cap_is_not_enforced() {
        // Removed from the scheme on 5 Oct 2025 — 250,000 EUR must be accepted.
        assert!(check_amount("InstdAmt", 25_000_000).is_ok());
    }

    #[test]
    fn dates_must_be_real_calendar_dates() {
        for good in ["2026-07-20", "2024-02-29", "2000-02-29", "2026-12-31"] {
            assert!(check_date("ReqdExctnDt", good).is_ok(), "{good} is valid");
        }
        for bad in [
            "2026-02-30",
            "2026-13-01",
            "2026-00-10",
            "2026-07-00",
            "2026-07-32",
            "2023-02-29",
            "1900-02-29", // 1900 is not a leap year
            "2026-7-20",
            "20260720",
            "2026/07/20",
            "",
            "not-a-date",
            "2026-07-20T00:00:00",
        ] {
            assert!(check_date("ReqdExctnDt", bad).is_err(), "{bad} is invalid");
        }
    }

    #[test]
    fn identifiers_reject_non_sepa_characters_under_every_policy() {
        // An identifier is a reconciliation key: it must round-trip exactly, so
        // it is never transliterated, even under the default policy.
        for field in ["MsgId", "EndToEndId", "MndtId"] {
            assert_eq!(
                check_id(field, "MND-Straße"),
                Err(ValidationError::InvalidCharacter { field, ch: 'ß' })
            );
        }
        assert!(check_id("MsgId", "MND-Strasse-01").is_ok());
    }

    #[test]
    fn slash_rule_survives_whitespace_padding() {
        // XSD whitespace collapse would strip the padding and expose the slash.
        assert_eq!(
            check_id("MsgId", " /x "),
            Err(ValidationError::SlashRule { field: "MsgId" })
        );
        assert_eq!(
            check_id("MsgId", " x/ "),
            Err(ValidationError::SlashRule { field: "MsgId" })
        );
    }

    #[test]
    fn year_zero_is_rejected() {
        // "0000-01-01" parses but is not a valid xs:date.
        assert!(check_date("Dt", "0000-01-01").is_err());
        assert!(check_date("Dt", "0000-02-29").is_err());
        assert!(check_date("Dt", "0001-01-01").is_ok());
    }

    #[test]
    fn strict_policy_rejects_non_sepa_characters() {
        let policy = CharsetPolicy::Strict;
        assert_eq!(policy.apply("Nm", "Mueller").unwrap(), "Mueller");
        assert_eq!(
            policy.apply("Nm", "Müller"),
            Err(ValidationError::InvalidCharacter {
                field: "Nm",
                ch: 'ü'
            })
        );
    }

    #[test]
    fn default_policy_transliterates() {
        let policy = CharsetPolicy::default();
        assert_eq!(policy.apply("Nm", "Müller & Co").unwrap(), "Mueller + Co");
    }

    #[test]
    fn truncation_never_splits_a_character() {
        // Regression: `&desc[..140]` panicked when byte 140 fell inside 'ü'.
        let s = format!("{}ü", "A".repeat(139));
        assert_eq!(s.len(), 141, "141 bytes, 140 characters");
        let out = truncate_chars(&s, 140);
        assert_eq!(out.chars().count(), 140);
        assert!(out.ends_with('ü'), "the whole character must survive");

        // One character over: the 'ü' is dropped whole, not split.
        let s2 = format!("{}ü", "A".repeat(140));
        let out2 = truncate_chars(&s2, 140);
        assert_eq!(out2, "A".repeat(140));
    }

    #[test]
    fn truncation_is_a_no_op_when_within_limit() {
        assert!(matches!(
            truncate_chars("short", 140),
            std::borrow::Cow::Borrowed("short")
        ));
        assert_eq!(truncate_chars("", 140), "");
    }

    #[test]
    fn epc_policy_transliterates_one_to_one() {
        let policy = CharsetPolicy::Transliterate(Transliteration::Epc);
        assert_eq!(policy.apply("Nm", "Müller & Co").unwrap(), "Muller + Co");
    }
}
