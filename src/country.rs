//! ISO 3166-1 alpha-2 country codes, as ISO 20022 uses them.
//!
//! Three unrelated ISO 20022 fields are typed `CountryCode` and constrained to
//! nothing but `[A-Z]{2}`: characters 5–6 of a BIC, `PstlAdr/Ctry`, and
//! `CtryOfRes`. The pattern alone accepts `ZZ`, which addresses no country and
//! no bank, so this module is the shared answer to "is that two letters, or is
//! it a country".
//!
//! ## Examples
//!
//! ```
//! use sepa::country::is_country_code;
//!
//! assert!(is_country_code("DE"));
//! assert!(is_country_code("us")); // case-insensitive
//! assert!(!is_country_code("ZZ"));
//! assert!(!is_country_code("DEU"));
//! ```

/// Every country code ISO 20022 accepts: the 249 officially assigned
/// ISO 3166-1 alpha-2 codes, plus `XK`.
///
/// `XK` is not an ISO 3166 code — it is user-assigned — but SWIFT issues BICs
/// under it for Kosovo, and it is a registered IBAN country code, so rejecting
/// it would make a Kosovan BIC unusable against a Kosovan IBAN.
///
/// Sorted, so the lookup is a binary search.
static COUNTRY_CODES: [&[u8; 2]; 250] = [
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

/// Returns `true` when `code` is a country code ISO 20022 accepts.
///
/// Comparison is case-insensitive; anything that is not exactly two ASCII
/// letters is `false` rather than an error, so malformed input from a bank file
/// simply fails the test.
///
/// # Examples
///
/// ```
/// use sepa::country::is_country_code;
///
/// assert!(is_country_code("DE"));
/// assert!(is_country_code("XK")); // Kosovo — user-assigned, used by SWIFT
/// assert!(!is_country_code("ZZ"));
/// assert!(!is_country_code(""));
/// ```
#[must_use]
pub fn is_country_code(code: &str) -> bool {
    let [a, b] = code.as_bytes() else {
        return false;
    };
    let key = [a.to_ascii_uppercase(), b.to_ascii_uppercase()];
    COUNTRY_CODES.binary_search(&&key).is_ok()
}

#[cfg(test)]
mod tests {
    use super::{COUNTRY_CODES, is_country_code};

    #[test]
    fn the_table_is_sorted_for_binary_search() {
        assert!(
            COUNTRY_CODES.windows(2).all(|w| w[0] < w[1]),
            "the table must stay sorted or the binary search silently misses"
        );
    }

    #[test]
    fn lookup_is_case_insensitive_and_never_panics() {
        assert!(is_country_code("DE"));
        assert!(is_country_code("de"));
        assert!(is_country_code("dE"));
        for bad in ["", "D", "DEU", "Ü!", "12", "ZZ", "QQ"] {
            assert!(!is_country_code(bad), "{bad:?} must not be a country");
        }
    }

    #[test]
    fn every_iban_registry_country_is_a_country_code() {
        // A BIC and a postal address have to be usable alongside an IBAN from
        // the same country, so the registry must be a subset of this table.
        for a in b'A'..=b'Z' {
            for b in b'A'..=b'Z' {
                let cc = String::from_utf8(vec![a, b]).unwrap();
                if crate::iban::iban_bban_format(&cc).is_some() {
                    assert!(
                        is_country_code(&cc),
                        "{cc} is an IBAN country but not a country code"
                    );
                }
            }
        }
    }

    #[test]
    fn every_sepa_country_is_a_country_code() {
        for cc in ["DE", "GI", "XK", "MD", "RS", "VA", "SM", "MC", "AD"] {
            assert!(is_country_code(cc), "{cc}");
        }
    }
}
