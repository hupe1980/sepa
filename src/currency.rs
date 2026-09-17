//! ISO 4217 currency codes, as ISO 20022 constrains them.
//!
//! Four of the five EPC payment schemes are euro-only. The fifth — One-Leg Out
//! Instant Credit Transfer — is not, so an amount may be ordered in the
//! beneficiary's currency; see
//! [`CreditTransferKind::OneLegOutInstant`](crate::pain001::CreditTransferKind::OneLegOutInstant).
//!
//! ## No table of active codes
//!
//! `ActiveOrHistoricCurrencyCode` is `[A-Z]{3}` and nothing more: neither the
//! schema nor the EPC guidelines enumerate the codes. Which currencies a
//! payment can actually reach is the receiving PSP's question, not this
//! crate's, so there is no list here to go stale.
//!
//! What *is* refused is the placeholder class — codes that match the pattern
//! and name no money:
//!
//! | Code | Meaning |
//! |---|---|
//! | `XXX` | "no currency involved" |
//! | `XTS` | reserved for testing |
//! | `XAU` `XAG` `XPT` `XPD` | precious metals, priced per troy ounce |
//!
//! Those are the same hazard as `<BICFI>NOTPROVIDED</BICFI>`: a value that
//! passes every check and instructs nothing.
//!
//! ## Examples
//!
//! ```
//! use sepa::Currency;
//!
//! let usd: Currency = "USD".parse()?;
//! assert_eq!(usd.code(), "USD");
//! assert!(!usd.is_euro());
//! assert!(Currency::EUR.is_euro());
//!
//! // Case is normalised, as it is for every other identifier here.
//! assert_eq!("chf".parse::<Currency>()?, Currency::new("CHF")?);
//!
//! // Syntactically perfect, names no money.
//! assert!(Currency::new("XXX").is_err());
//! assert!(Currency::new("EURO").is_err());
//! # Ok::<(), sepa::CurrencyError>(())
//! ```

use std::fmt;
use std::str::FromStr;

/// Error returned when a string is not a usable ISO 4217 currency code.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum CurrencyError {
    /// Not three ASCII letters — the `[A-Z]{3}` pattern every ISO 20022
    /// currency element is typed with.
    #[error("{value:?} is not three letters, so it is not an ISO 4217 code")]
    Malformed {
        /// The rejected text.
        value: String,
    },

    /// A reserved ISO 4217 code that names no spendable currency.
    ///
    /// Distinct from [`Malformed`](Self::Malformed) because the caller's
    /// mistake is different: `XXX` is not a typo, it is a placeholder that
    /// reached a payment instruction.
    #[error("{code} is a reserved ISO 4217 code and names no currency")]
    NotSpendable {
        /// The rejected code, upper-cased.
        code: String,
    },
}

/// A validated ISO 4217 currency code.
///
/// The constructor is the only way in, so a `Currency` in hand is three upper
/// case letters that are not a reserved placeholder. See the [module
/// docs](self) for why there is no table of active codes behind it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Currency([u8; 3]);

// Hand-written, not derived, for the same two reasons as `Iban` and `Bic`.
// A derive over the `[u8; 3]` would serialise "USD" as `[85, 83, 68]`, and —
// the part that matters — would **reconstruct the newtype without running the
// constructor**, so a round trip through JSON could hand back a `Currency`
// holding `XXX` or three NUL bytes. A validated newtype whose deserialiser
// skips validation is not a validated newtype (P2).
#[cfg(feature = "serde")]
impl serde::Serialize for Currency {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.code())
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for Currency {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = <std::borrow::Cow<'de, str> as serde::Deserialize>::deserialize(d)?;
        Self::new(&s).map_err(serde::de::Error::custom)
    }
}

/// ISO 4217 codes that match `[A-Z]{3}` and name no spendable currency.
///
/// `XXX` and `XTS` are explicit placeholders. The four metals are priced per
/// troy ounce rather than in minor units, so an integer-cents amount against
/// one of them is meaningless — which is the same argument, one step on.
const NOT_SPENDABLE: [&[u8; 3]; 6] = [b"XAG", b"XAU", b"XPD", b"XPT", b"XTS", b"XXX"];

impl Currency {
    /// The euro — the currency of every message the four SEPA schemes carry.
    pub const EUR: Self = Self(*b"EUR");

    /// Validate `code` as an ISO 4217 currency, upper-casing it.
    ///
    /// # Errors
    ///
    /// [`CurrencyError::Malformed`] when `code` is not three ASCII letters, and
    /// [`CurrencyError::NotSpendable`] for a reserved placeholder such as
    /// `XXX`.
    ///
    /// # Examples
    ///
    /// ```
    /// use sepa::Currency;
    /// assert_eq!(Currency::new("usd")?.code(), "USD");
    /// assert!(Currency::new("US").is_err());
    /// assert!(Currency::new("XTS").is_err());
    /// # Ok::<(), sepa::CurrencyError>(())
    /// ```
    pub fn new(code: &str) -> Result<Self, CurrencyError> {
        let [a, b, c] = *code.as_bytes() else {
            return Err(CurrencyError::Malformed {
                value: code.to_owned(),
            });
        };
        if ![a, b, c].iter().all(u8::is_ascii_alphabetic) {
            return Err(CurrencyError::Malformed {
                value: code.to_owned(),
            });
        }
        let upper = [
            a.to_ascii_uppercase(),
            b.to_ascii_uppercase(),
            c.to_ascii_uppercase(),
        ];
        if NOT_SPENDABLE.binary_search(&&upper).is_ok() {
            return Err(CurrencyError::NotSpendable {
                // Three ASCII letters are always valid UTF-8.
                code: String::from_utf8_lossy(&upper).into_owned(),
            });
        }
        Ok(Self(upper))
    }

    /// The code as three upper-case letters.
    #[must_use]
    pub fn code(&self) -> &str {
        // Only constructible from three ASCII letters.
        std::str::from_utf8(&self.0).unwrap_or("???")
    }

    /// Whether this is the euro.
    #[must_use]
    pub fn is_euro(&self) -> bool {
        self.0 == *b"EUR"
    }
}

impl fmt::Display for Currency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

impl FromStr for Currency {
    type Err = CurrencyError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl TryFrom<&str> for Currency {
    type Error = CurrencyError;
    fn try_from(s: &str) -> Result<Self, Self::Error> {
        Self::new(s)
    }
}

impl Default for Currency {
    /// The euro, which is the currency of every SEPA-scheme message.
    fn default() -> Self {
        Self::EUR
    }
}

#[cfg(test)]
mod tests {
    use super::{Currency, CurrencyError, NOT_SPENDABLE};

    #[test]
    fn the_placeholder_table_is_sorted_for_binary_search() {
        assert!(
            NOT_SPENDABLE.windows(2).all(|w| w[0] < w[1]),
            "the table must stay sorted or the binary search silently misses"
        );
    }

    #[test]
    fn three_letters_in_any_case_are_accepted_and_normalised() {
        for s in ["USD", "usd", "Usd", "uSD"] {
            assert_eq!(Currency::new(s).unwrap().code(), "USD", "{s}");
        }
    }

    #[test]
    fn anything_that_is_not_three_ascii_letters_is_malformed() {
        // Includes multi-byte input: `code.as_bytes()` is three *bytes*, so a
        // three-character non-ASCII string must not slip through as one.
        for bad in ["", "US", "EURO", "US1", "€UR", "ÄÖÜ", " EU", "EU "] {
            assert!(
                matches!(Currency::new(bad), Err(CurrencyError::Malformed { .. })),
                "{bad:?} must be malformed"
            );
        }
    }

    #[test]
    fn reserved_codes_are_refused_with_their_own_error() {
        // The whole point of the separate variant: `XXX` is not a typo.
        for code in ["XXX", "xxx", "XTS", "XAU", "XAG", "XPT", "XPD"] {
            assert!(
                matches!(Currency::new(code), Err(CurrencyError::NotSpendable { .. })),
                "{code} must be refused as a placeholder"
            );
        }
    }

    #[test]
    fn euro_is_recognised_however_it_was_built() {
        assert!(Currency::EUR.is_euro());
        assert!(Currency::new("eur").unwrap().is_euro());
        assert!(Currency::default().is_euro());
        assert!(!Currency::new("USD").unwrap().is_euro());
        assert_eq!(Currency::EUR.to_string(), "EUR");
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_round_trips_as_a_string_and_still_validates() {
        let usd = Currency::new("USD").unwrap();
        let json = serde_json::to_string(&usd).unwrap();
        // A string, not the three bytes behind it.
        assert_eq!(json, "\"USD\"");
        assert_eq!(serde_json::from_str::<Currency>(&json).unwrap(), usd);

        // The constructor runs on the way back in, so the newtype cannot be
        // forged through a deserialiser. Regression: a `derive` here would
        // have accepted the placeholder and the raw byte array alike.
        for bad in ["\"XXX\"", "\"EURO\"", "\"\"", "[85,83,68]"] {
            assert!(
                serde_json::from_str::<Currency>(bad).is_err(),
                "{bad} must not deserialise into a Currency"
            );
        }
        // Case is still normalised through serde.
        assert_eq!(
            serde_json::from_str::<Currency>("\"chf\"").unwrap(),
            Currency::new("CHF").unwrap()
        );
    }

    #[test]
    fn a_three_letter_code_never_panics_on_display() {
        // `code()` unwraps a UTF-8 conversion; the constructor is what makes
        // that total, so sweep the whole constructible space.
        for a in b'A'..=b'Z' {
            for b in b'A'..=b'Z' {
                for c in b'A'..=b'Z' {
                    let s = String::from_utf8(vec![a, b, c]).unwrap();
                    if let Ok(cur) = Currency::new(&s) {
                        assert_eq!(cur.code(), s);
                    }
                }
            }
        }
    }
}
