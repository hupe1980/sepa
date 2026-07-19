//! SEPA Basic Latin character set — validation and transliteration.
//!
//! SEPA payment messages may only carry a 73-character subset of Latin text.
//! Anything outside it — German umlauts, French accents, the euro sign — must be
//! converted before the message is sent, or the bank rejects the file.
//!
//! ## The permitted set (EPC132-08 §1.4)
//!
//! ```text
//! a b c d e f g h i j k l m n o p q r s t u v w x y z
//! A B C D E F G H I J K L M N O P Q R S T U V W X Y Z
//! 0 1 2 3 4 5 6 7 8 9
//! / - ? : ( ) . , ' +
//! (space)
//! ```
//!
//! ## Transliteration
//!
//! [`transliterate`] converts unsupported characters into that set using the
//! **published EPC217-08 conversion table**, transcribed verbatim from the
//! spreadsheet the EPC distributes alongside the guidance document — 1011
//! mappings covering Latin, Latin Extended, Greek, Cyrillic and symbols.
//!
//! This matters because a generic accent-folding transliterator agrees with the
//! EPC only for plain Latin accents and diverges elsewhere:
//!
//! | Input | EPC217-08 | Typical generic folding |
//! |---|---|---|
//! | `Æ` | `A` | `AE` |
//! | `Œ` | `O` | `OE` |
//! | `Θ` | `TH` | `TH` or dropped |
//! | `Щ` | `SHT` | `SHCH` |
//! | `Я` | `YA` | `YA` or dropped |
//! | `€` | `E` | dropped |
//!
//! Twenty-six Greek and Cyrillic letters have a genuine multi-character
//! romanisation (ISO 843 / ISO 9); the rest of the table is strictly one
//! character to one character. Characters absent from the table become `.`.
//!
//! ### The two styles
//!
//! | Input | [`Transliteration::German`] (default) | [`Transliteration::Epc`] |
//! |---|---|---|
//! | `ä ö ü` | `ae oe ue` | `a o u` |
//! | `Ä Ö Ü` | `Ae Oe Ue` | `A O U` |
//! | `ß` | `ss` | `s` |
//! | everything else | identical — the published table | identical |
//!
//! [`Transliteration::Epc`] is the table exactly as published.
//! [`Transliteration::German`] overrides precisely seven characters, following
//! the *"Alternativ auch zulässig"* column of DFÜ-Abkommen Anlage 3, which the
//! Deutsche Kreditwirtschaft sanctions. It is the default because losing the
//! umlaut changes how a name reads — and on a bank statement, who it appears to
//! be. `Müller` becomes `Mueller` rather than `Muller`.
//!
//! ### Where the spreadsheet and the prose disagree
//!
//! EPC217-08 §6.2 names some replacements in prose that the spreadsheet does
//! not carry, and contradicts it in one case. The precedence applied here is:
//! **the spreadsheet wins where it has a value**, and the prose fills the gaps.
//!
//! | Character | Spreadsheet | §6.2 prose | Emitted |
//! |---|---|---|---|
//! | `@` | `.` | `(at)` | `.` — the spreadsheet is the machine-readable source |
//! | `&` | *N/A* | `+` | `+` |
//! | `_` `~` | *absent* | `-` | `-` |
//! | `€` | `E` | `E` | `E` |
//!
//! `"`, `<` and `>` are marked *N/A* with no prose rule, since XML escaping
//! handles them at the syntax level. They are still not SEPA-legal, so they map
//! to the nearest legal punctuation (`'`, `(`, `)`).
//!
//! ## Examples
//!
//! ```
//! use sepa::charset::{is_sepa_char, transliterate, Transliteration};
//!
//! assert!(is_sepa_char('A'));
//! assert!(is_sepa_char('+'));
//! assert!(!is_sepa_char('ü'));
//!
//! assert_eq!(transliterate("Müller & Söhne GmbH", Transliteration::German),
//!            "Mueller + Soehne GmbH");
//! assert_eq!(transliterate("Müller & Söhne GmbH", Transliteration::Epc),
//!            "Muller + Sohne GmbH");
//! ```

use crate::charset_table::EPC_CONVERSION_TABLE;

/// Which transliteration table [`transliterate`] applies.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Transliteration {
    /// German banking convention: `ä→ae`, `ö→oe`, `ü→ue`, `ß→ss` (default).
    ///
    /// Sanctioned by DFÜ-Abkommen Anlage 3 as an accepted alternative to the
    /// EPC table. Preserves the German reading of names.
    #[default]
    German,
    /// Strict EPC217-08 one-to-one mapping: `ä→a`, `ö→o`, `ü→u`, `ß→s`.
    ///
    /// This is the table as published: strictly one character to one character
    /// for Latin, including the ligatures (`Æ→A`, not `AE`). The only entries
    /// that lengthen are 26 Greek and Cyrillic letters with a genuine
    /// multi-character romanisation (`Θ→TH`, `Щ→SHT`, `Я→YA`).
    Epc,
}

/// Returns `true` if `c` is in the SEPA Basic Latin character set.
///
/// # Examples
///
/// ```
/// use sepa::charset::is_sepa_char;
///
/// assert!(is_sepa_char('Z'));
/// assert!(is_sepa_char('7'));
/// assert!(is_sepa_char(' '));
/// assert!(is_sepa_char('\''));
/// assert!(!is_sepa_char('ß'));
/// assert!(!is_sepa_char('&'));
/// ```
#[inline]
#[must_use]
pub const fn is_sepa_char(c: char) -> bool {
    matches!(c,
        'a'..='z' | 'A'..='Z' | '0'..='9'
        | '/' | '-' | '?' | ':' | '(' | ')' | '.' | ',' | '\'' | '+' | ' '
    )
}

/// Returns `true` if every character of `s` is in the SEPA character set.
///
/// # Examples
///
/// ```
/// use sepa::charset::is_sepa_text;
///
/// assert!(is_sepa_text("Rechnung 2026-07 (Teil 1)"));
/// assert!(!is_sepa_text("Zahlung für Müller"));
/// ```
#[inline]
#[must_use]
pub fn is_sepa_text(s: &str) -> bool {
    s.chars().all(is_sepa_char)
}

/// Returns the first character of `s` that is not in the SEPA character set.
///
/// # Examples
///
/// ```
/// use sepa::charset::first_invalid_char;
///
/// assert_eq!(first_invalid_char("Müller"), Some('ü'));
/// assert_eq!(first_invalid_char("Mueller"), None);
/// ```
#[inline]
#[must_use]
pub fn first_invalid_char(s: &str) -> Option<char> {
    s.chars().find(|c| !is_sepa_char(*c))
}

/// Convert `s` into the SEPA Basic Latin character set.
///
/// Characters already in the set pass through untouched. Everything else is
/// mapped per `style` (see the [module docs](self)); anything unmapped becomes
/// `.`.
///
/// Returns `Cow::Borrowed` when no conversion was needed, so the common
/// all-ASCII case costs nothing.
///
/// # Examples
///
/// ```
/// use sepa::charset::{transliterate, Transliteration};
///
/// // Untouched — already valid
/// assert_eq!(transliterate("Rechnung 12/2026", Transliteration::German),
///            "Rechnung 12/2026");
///
/// // German style keeps the pronunciation
/// assert_eq!(transliterate("Größe", Transliteration::German), "Groesse");
/// // EPC style is strictly one-to-one, so ß becomes a single 's'
/// assert_eq!(transliterate("Größe", Transliteration::Epc), "Grose");
///
/// // Accents fold to their base letter in both styles
/// assert_eq!(transliterate("Café Ångström", Transliteration::Epc), "Cafe Angstrom");
///
/// // Symbols map to SEPA-legal equivalents
/// assert_eq!(transliterate("A & B @ 100€", Transliteration::Epc), "A + B . 100E");
/// ```
#[must_use]
pub fn transliterate(s: &str, style: Transliteration) -> std::borrow::Cow<'_, str> {
    if is_sepa_text(s) {
        return std::borrow::Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if is_sepa_char(c) {
            out.push(c);
        } else {
            out.push_str(map_char(c, style));
        }
    }
    std::borrow::Cow::Owned(out)
}

/// Map a single out-of-set character to its SEPA replacement.
///
/// Looks the character up in the published [EPC217-08 conversion
/// table](crate::charset). Characters absent from the table have no defined
/// mapping and become `.`, per EPC217-08 §5.
fn map_char(c: char, style: Transliteration) -> &'static str {
    // The German style overrides exactly seven characters. EPC217-08 maps these
    // one-to-one (ä→a, ß→s); the DFÜ-Abkommen lists the digraph forms in its
    // "Alternativ auch zulässig" column, and they are what German banks and
    // their customers expect to see on a statement.
    if style == Transliteration::German {
        match c {
            'ä' => return "ae",
            'ö' => return "oe",
            'ü' => return "ue",
            'Ä' => return "Ae",
            'Ö' => return "Oe",
            'Ü' => return "Ue",
            'ß' | 'ẞ' => return "ss",
            _ => {}
        }
    }

    // EPC217-08 §6.2 states these in prose. `&` is marked "N/A" in the
    // spreadsheet (XML escaping handles it at the syntax level) and `_`/`~` are
    // absent from it entirely, so the prose rule is the only guidance. Where the
    // spreadsheet *does* carry a value it wins — see the module docs on the `@`
    // discrepancy.
    match c {
        '&' => return "+",
        '_' | '~' => return "-",
        // No EPC rule: these are not SEPA-legal, and mapping them to the
        // nearest legal punctuation preserves more than a bare '.' would.
        '"' => return "'",
        '<' => return "(",
        '>' => return ")",
        _ => {}
    }

    if let Ok(i) = EPC_CONVERSION_TABLE.binary_search_by_key(&(c as u32), |(cp, _)| *cp) {
        // `binary_search_by_key` returned an in-range index.
        if let Some((_, replacement)) = EPC_CONVERSION_TABLE.get(i) {
            return replacement;
        }
    }

    // Not in the table: collapse whitespace to a space, everything else to '.'.
    if c.is_whitespace() { " " } else { "." }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permitted_set_is_exactly_73_characters() {
        let count = (0u32..=0x10_FFFF)
            .filter_map(char::from_u32)
            .filter(|c| is_sepa_char(*c))
            .count();
        assert_eq!(count, 73, "SEPA Basic Latin set must hold exactly 73 chars");
    }

    #[test]
    fn punctuation_membership() {
        for c in "/-?:().,'+ ".chars() {
            assert!(is_sepa_char(c), "{c:?} must be permitted");
        }
        for c in "&@#$%*!\"<>[]{}=;_~\\|".chars() {
            assert!(!is_sepa_char(c), "{c:?} must be rejected");
        }
    }

    #[test]
    fn valid_text_is_borrowed_not_copied() {
        let s = "Rechnung 2026-07 (Teil 1)";
        assert!(is_sepa_text(s));
        assert!(matches!(
            transliterate(s, Transliteration::German),
            std::borrow::Cow::Borrowed(_)
        ));
    }

    #[test]
    fn german_style_preserves_pronunciation() {
        assert_eq!(
            transliterate("Müller & Söhne GmbH", Transliteration::German),
            "Mueller + Soehne GmbH"
        );
        assert_eq!(
            transliterate("Ärzte Öl Übung Straße", Transliteration::German),
            "Aerzte Oel Uebung Strasse"
        );
    }

    #[test]
    fn epc_maps_ligatures_one_to_one() {
        // EPC217-08 is strictly one character to one character for Latin — the
        // ligatures do NOT expand. A generic transliterator produces "AE"/"OE",
        // which is a defensible reading but is not what the EPC table says.
        for (input, expected) in [
            ('Æ', "A"),
            ('æ', "a"),
            ('Œ', "O"),
            ('œ', "o"),
            ('Ĳ', "I"),
            ('ĳ', "i"),
        ] {
            assert_eq!(
                transliterate(&input.to_string(), Transliteration::Epc),
                expected,
                "EPC217-08 maps {input:?} one-to-one"
            );
        }
    }

    #[test]
    fn greek_and_cyrillic_use_the_published_romanisation() {
        // 26 letters have a real multi-character romanisation in EPC217-08
        // (ISO 843 / ISO 9). Collapsing them to '.' loses the name entirely.
        for (input, expected) in [
            ('Θ', "TH"),
            ('Χ', "CH"),
            ('Ψ', "PS"),
            ('θ', "th"),
            ('Ж', "ZH"),
            ('Ц', "TS"),
            ('Щ', "SHT"),
            ('Я', "YA"),
            ('ж', "zh"),
            ('я', "ya"),
        ] {
            for style in [Transliteration::German, Transliteration::Epc] {
                assert_eq!(
                    transliterate(&input.to_string(), style),
                    expected,
                    "{input:?} must romanise per EPC217-08"
                );
            }
        }
        // A whole word, to show it stays readable rather than becoming dots.
        // Ψ→PS, υ→y, χ→ch, ή→i per the published table.
        assert_eq!(transliterate("Ψυχή", Transliteration::Epc), "PSychi");
    }

    #[test]
    fn table_is_sorted_for_binary_search() {
        let table = crate::charset_table::EPC_CONVERSION_TABLE;
        assert!(
            table.windows(2).all(|w| w[0].0 < w[1].0),
            "EPC table must be strictly sorted by code point"
        );
        assert!(table.len() > 1000, "table looks truncated");
    }

    #[test]
    fn epc_style_is_one_to_one() {
        assert_eq!(
            transliterate("Müller & Söhne GmbH", Transliteration::Epc),
            "Muller + Sohne GmbH"
        );
        // Length is preserved for everything but the ligatures above.
        let input = "Ärzte Öl Übung Straße";
        let out = transliterate(input, Transliteration::Epc);
        assert_eq!(out, "Arzte Ol Ubung Strase");
        assert_eq!(out.chars().count(), input.chars().count());
    }

    #[test]
    fn non_umlaut_accents_fold_identically_in_both_styles() {
        // The two styles differ only on ä/ö/ü/ß; every other accent folds the same.
        for style in [Transliteration::German, Transliteration::Epc] {
            assert_eq!(transliterate("Café", style), "Cafe");
            assert_eq!(transliterate("Señor Niño", style), "Senor Nino");
            assert_eq!(transliterate("Łódź", style), "Lodz");
            assert_eq!(transliterate("Français", style), "Francais");
        }
    }

    #[test]
    fn styles_differ_only_on_umlauts_and_eszett() {
        // "Ångström" contains 'ö', so the styles legitimately diverge here.
        assert_eq!(
            transliterate("Ångström", Transliteration::German),
            "Angstroem"
        );
        assert_eq!(transliterate("Ångström", Transliteration::Epc), "Angstrom");
    }

    #[test]
    fn symbols_map_to_legal_equivalents() {
        assert_eq!(
            transliterate("A & B @ 100€", Transliteration::Epc),
            "A + B . 100E"
        );
        assert_eq!(transliterate("a_b~c", Transliteration::Epc), "a-b-c");
        assert_eq!(
            transliterate("\"quoted\"", Transliteration::Epc),
            "'quoted'"
        );
        assert_eq!(transliterate("x[1]{2}", Transliteration::Epc), "x(1)(2)");
    }

    #[test]
    fn unmapped_characters_become_a_dot() {
        assert_eq!(transliterate("日本語", Transliteration::Epc), "...");
        assert_eq!(transliterate("emoji 🎉", Transliteration::Epc), "emoji .");
    }

    #[test]
    fn control_and_exotic_whitespace_becomes_a_space() {
        assert_eq!(transliterate("a\tb\nc", Transliteration::Epc), "a b c");
        assert_eq!(transliterate("a\u{00A0}b", Transliteration::Epc), "a b");
    }

    #[test]
    fn transliteration_output_is_always_valid() {
        // The whole point: whatever goes in, what comes out is SEPA-legal.
        let nasty = "Müller\tÖl 🎉 €100 «señor» 日本 \\|<>{}[]!@#$%^&*_~;=`";
        for style in [Transliteration::German, Transliteration::Epc] {
            let out = transliterate(nasty, style);
            assert!(
                is_sepa_text(&out),
                "{style:?} produced non-SEPA output: {out:?}"
            );
        }
    }

    #[test]
    fn first_invalid_char_reports_the_offender() {
        assert_eq!(first_invalid_char("Mueller"), None);
        assert_eq!(first_invalid_char("Müller"), Some('ü'));
        assert_eq!(first_invalid_char("A&B"), Some('&'));
    }

    #[test]
    fn empty_input() {
        assert!(is_sepa_text(""));
        assert_eq!(transliterate("", Transliteration::German), "");
    }
}
