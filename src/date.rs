//! Typed ISO 8601 dates and timestamps for the ISO 20022 date fields.
//!
//! Every date a builder writes — `ReqdColltnDt`, `ReqdExctnDt`, `DtOfSgntr`,
//! `CreDtTm` — is an [`IsoDate`] or an [`IsoDateTime`], never a string. That is
//! the same treatment [`Iban`](crate::Iban), [`Bic`](crate::Bic) and
//! [`CreditorId`](crate::CreditorId) already get: the value is validated once,
//! at construction, and a malformed date is then unrepresentable in a batch
//! rather than something `build()` has to catch.
//!
//! ```
//! use sepa::IsoDate;
//!
//! let d: IsoDate = "2026-07-20".parse()?;
//! assert_eq!(d.year(), 2026);
//! assert_eq!(d.to_string(), "2026-07-20");
//!
//! // Impossible dates are rejected at construction, not at submission.
//! assert!("2026-02-30".parse::<IsoDate>().is_err());
//! assert!(IsoDate::new(2026, 13, 1).is_err());
//!
//! // Calendar arithmetic for collection-date offsets.
//! assert_eq!(d.plus_days(5)?.to_string(), "2026-07-25");
//! # Ok::<(), sepa::DateError>(())
//! ```
//!
//! ## Interop
//!
//! With the `time` or `chrono` feature, the matching types convert in **both**
//! directions, so a caller that already has a typed date never formats a
//! string. Every conversion is fallible, and each direction fails for its own
//! reason: `time` and `chrono` represent years this type deliberately does not,
//! and this type represents timestamps that name no instant.
//!
//! | This crate | `time` | `chrono` |
//! |---|---|---|
//! | [`IsoDate`] | `Date` | `NaiveDate` |
//! | [`IsoDateTime`] (offset dropped) | `PrimitiveDateTime` | `NaiveDateTime` |
//! | [`IsoDateTime`] (offset kept) | `OffsetDateTime` | `DateTime<FixedOffset>` |
//!
//! The last row is the one with an opinion. `OffsetDateTime` and `DateTime`
//! name an *instant*, and an [`IsoDateTime`] with no offset does not — so that
//! conversion returns [`ConversionError::NoOffset`] rather than assuming UTC.
//! Reach for [`IsoDateTime::in_utc`] when UTC really is the answer.
//!
//! ```
//! # #[cfg(feature = "time")]
//! # fn demo() -> Result<(), Box<dyn std::error::Error>> {
//! use sepa::{IsoDate, IsoDateTime, ConversionError};
//!
//! let from_time = time::Date::from_calendar_date(2026, time::Month::July, 20)?;
//! let date = IsoDate::try_from(from_time)?;
//! assert_eq!(date.to_string(), "2026-07-20");
//!
//! let back: time::Date = date.try_into()?;
//! assert_eq!(back, from_time);
//!
//! // A timestamp with an offset is an instant, and survives the round trip.
//! let stamped: IsoDateTime = "2026-07-20T13:00:00+02:00".parse()?;
//! let offset: time::OffsetDateTime = stamped.try_into()?;
//! assert_eq!(offset.unix_timestamp(), stamped.unix_seconds().unwrap());
//!
//! // One without an offset is not, and says so rather than guessing.
//! let bare: IsoDateTime = "2026-07-20T13:00:00".parse()?;
//! assert_eq!(
//!     time::OffsetDateTime::try_from(bare).unwrap_err(),
//!     ConversionError::NoOffset,
//! );
//! # Ok(())
//! # }
//! # #[cfg(feature = "time")]
//! # demo().unwrap();
//! ```

use std::fmt;
use std::str::FromStr;

// ── errors ────────────────────────────────────────────────────────────────────

/// Error returned when a value is not a valid ISO 8601 calendar date.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum DateError {
    /// The text is not of the form `YYYY-MM-DD`.
    #[error("{value:?} is not an ISO 8601 date of the form YYYY-MM-DD")]
    Malformed {
        /// The rejected text.
        value: String,
    },

    /// The components parse but do not name a real day — `2026-02-30`.
    #[error("{year:04}-{month:02}-{day:02} is not a real calendar date")]
    NotACalendarDate {
        /// Year component.
        year: i64,
        /// Month component.
        month: u32,
        /// Day component.
        day: u32,
    },

    /// The year lies outside 1–9999.
    ///
    /// `xs:date` has no year zero, and ISO 20022 dates are four-digit years, so
    /// the range is bounded on both sides.
    #[error("year {year} is outside the supported range 1-9999")]
    YearOutOfRange {
        /// The rejected year.
        year: i64,
    },

    /// A day count fell outside the range [`IsoDate::MIN`]–[`IsoDate::MAX`].
    ///
    /// Produced by [`IsoDate::from_epoch_days`] and by date arithmetic that
    /// walks off the end of the calendar.
    #[error("{days} days from the epoch is outside 0001-01-01 - 9999-12-31")]
    OutOfRange {
        /// The rejected day count, relative to 1970-01-01.
        days: i64,
    },
}

/// Error returned when a value is not a valid ISO 8601 timestamp.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum DateTimeError {
    /// The date part is invalid.
    #[error(transparent)]
    Date(#[from] DateError),

    /// The text is not of the form `YYYY-MM-DDThh:mm:ss[.fff][Z|±hh:mm]`.
    #[error("{value:?} is not an ISO 8601 timestamp of the form YYYY-MM-DDThh:mm:ss")]
    Malformed {
        /// The rejected text.
        value: String,
    },

    /// The time components parse but are out of range — `25:00:00`.
    #[error("{hour:02}:{minute:02}:{second:02} is not a valid time of day")]
    NotATimeOfDay {
        /// Hour component.
        hour: u32,
        /// Minute component.
        minute: u32,
        /// Second component.
        second: u32,
    },
}

/// Error returned when an [`IsoDateTime`] cannot be expressed in an external
/// date-time type.
///
/// Only produced by the `time` and `chrono` conversions, and only in the
/// direction that needs an instant. A timestamp with no UTC offset is a wall
/// clock somewhere, and `time::OffsetDateTime` / `chrono::DateTime` both
/// require the somewhere — so the conversion fails rather than assuming UTC.
#[cfg(any(feature = "time", feature = "chrono"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ConversionError {
    /// The timestamp carries no UTC offset, so it names no instant.
    #[error("timestamp has no UTC offset, so it names no instant — see IsoDateTime::in_utc")]
    NoOffset,

    /// The value falls outside the target type's representable range.
    #[error("the value is outside the target type's range")]
    OutOfRange,
}

// ── IsoDate ───────────────────────────────────────────────────────────────────

/// A validated ISO 8601 calendar date, rendered as `YYYY-MM-DD`.
///
/// Ordering is chronological, so a `Vec<IsoDate>` sorts the way a ledger
/// expects, and a collection run can be bucketed by date with a `BTreeMap`.
///
/// # Examples
///
/// ```
/// use sepa::IsoDate;
///
/// let a = IsoDate::new(2026, 7, 20)?;
/// let b: IsoDate = "2026-07-25".parse()?;
/// assert!(a < b);
/// assert_eq!(b.epoch_days() - a.epoch_days(), 5);
/// # Ok::<(), sepa::DateError>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IsoDate {
    // Field order fixes the derived `Ord` as year, then month, then day —
    // which is exactly chronological for a proleptic Gregorian calendar.
    year: u16,
    month: u8,
    day: u8,
}

impl IsoDate {
    /// The earliest representable date, `0001-01-01`.
    pub const MIN: Self = Self {
        year: 1,
        month: 1,
        day: 1,
    };

    /// The latest representable date, `9999-12-31`.
    pub const MAX: Self = Self {
        year: 9999,
        month: 12,
        day: 31,
    };

    /// Build a date from its components.
    ///
    /// # Errors
    ///
    /// [`DateError::YearOutOfRange`] outside 1–9999, or
    /// [`DateError::NotACalendarDate`] for a day that does not exist in that
    /// month — including 29 February in a common year.
    ///
    /// # Examples
    ///
    /// ```
    /// use sepa::IsoDate;
    /// assert!(IsoDate::new(2024, 2, 29).is_ok()); // 2024 is a leap year
    /// assert!(IsoDate::new(2023, 2, 29).is_err());
    /// ```
    pub fn new(year: u16, month: u8, day: u8) -> Result<Self, DateError> {
        if year == 0 {
            return Err(DateError::YearOutOfRange { year: 0 });
        }
        if !(1..=12).contains(&month) || day < 1 || day > days_in_month(year, month) {
            return Err(DateError::NotACalendarDate {
                year: i64::from(year),
                month: u32::from(month),
                day: u32::from(day),
            });
        }
        Ok(Self { year, month, day })
    }

    /// Parse a `YYYY-MM-DD` date.
    ///
    /// The shape is exact: no other separator, no two-digit year, no trailing
    /// time. This is deliberate — `ReqdColltnDt` is an `xs:date`, and a value a
    /// bank would reject must not be constructible.
    ///
    /// # Errors
    ///
    /// [`DateError::Malformed`] for anything that is not `YYYY-MM-DD`, or
    /// [`DateError::NotACalendarDate`] for an impossible day.
    ///
    /// # Examples
    ///
    /// ```
    /// use sepa::IsoDate;
    /// assert_eq!(IsoDate::parse("2026-07-20")?.day(), 20);
    /// assert!(IsoDate::parse("20.07.2026").is_err());
    /// assert!(IsoDate::parse("2026-07-20T00:00:00").is_err());
    /// # Ok::<(), sepa::DateError>(())
    /// ```
    pub fn parse(s: &str) -> Result<Self, DateError> {
        // `xs:date` carries an OPTIONAL timezone — `2026-07-20Z` and
        // `2026-07-20+02:00` are both schema-valid `ISODate` values, so
        // refusing them would make a conforming document unreadable. The zone
        // is accepted and dropped: the value denotes that calendar day, and
        // every date this crate writes is a bare `xs:date`.
        let (head, zone) = s.split_at_checked(10).unwrap_or((s, ""));
        if !is_xsd_timezone(zone) {
            return Err(DateError::Malformed {
                value: s.to_owned(),
            });
        }
        // Destructuring the exact byte pattern rejects every other length,
        // separator and non-digit without a single fallible index.
        let [y0, y1, y2, y3, b'-', m0, m1, b'-', d0, d1] = *head.as_bytes() else {
            return Err(DateError::Malformed {
                value: s.to_owned(),
            });
        };
        if ![y0, y1, y2, y3, m0, m1, d0, d1]
            .iter()
            .all(u8::is_ascii_digit)
        {
            return Err(DateError::Malformed {
                value: s.to_owned(),
            });
        }
        let d2 = |a: u8, b: u8| u16::from(a - b'0') * 10 + u16::from(b - b'0');
        let year = d2(y0, y1) * 100 + d2(y2, y3);
        #[allow(clippy::items_after_statements)]
        // `d2` yields at most 99, so both casts are lossless.
        #[allow(clippy::cast_possible_truncation)]
        Self::new(year, d2(m0, m1) as u8, d2(d0, d1) as u8)
    }

    /// Parse the **date part** of an ISO 8601 date or date-time.
    ///
    /// Accepts `2026-07-20` and `2026-07-20T12:30:00Z` alike, returning the
    /// date in both cases.
    ///
    /// This is what bank files need. ISO 20022 types a booking date as a
    /// `DateAndDateTimeChoice`, so the same field arrives as a bare date from
    /// one bank and as a timestamp from the next, and a reconciliation routine
    /// posts by the day either way.
    ///
    /// The accepted set is exactly `xs:date` ∪ `xs:dateTime` — the two members
    /// of that choice — and nothing else, so a value with trailing junk is an
    /// error rather than a confident date.
    ///
    /// # Errors
    ///
    /// [`DateError`] when the value is neither an `xs:date` nor an
    /// `xs:dateTime` whose date part is a real calendar day.
    ///
    /// # Examples
    ///
    /// ```
    /// use sepa::IsoDate;
    ///
    /// let d = IsoDate::new(2026, 7, 20)?;
    /// assert_eq!(IsoDate::parse_date_part("2026-07-20")?, d);
    /// assert_eq!(IsoDate::parse_date_part("2026-07-20T12:30:00Z")?, d);
    /// assert!(IsoDate::parse_date_part("2026-02-30T00:00:00").is_err());
    /// # Ok::<(), sepa::DateError>(())
    /// ```
    pub fn parse_date_part(s: &str) -> Result<Self, DateError> {
        // `split_at_checked`, not a slice: `s` is bank-supplied and byte 10 can
        // land inside a multi-byte character.
        let Some((head, rest)) = s.split_at_checked(10) else {
            return Self::parse(s);
        };
        // Either the choice's `xs:date` member — which `parse` now validates
        // including its optional timezone — or its `xs:dateTime` member, whose
        // time part must itself be well formed. Trailing anything else is not a
        // date with noise after it; it is a value this crate cannot read, and
        // saying so beats returning a day nobody wrote.
        match rest.as_bytes().first() {
            Some(b'T') => {
                IsoDateTime::parse(s).map_err(|_| DateError::Malformed {
                    value: s.to_owned(),
                })?;
                Self::parse(head)
            }
            _ => Self::parse(s),
        }
    }

    /// The year, 1–9999.
    #[inline]
    #[must_use]
    pub const fn year(self) -> u16 {
        self.year
    }

    /// The month, 1–12.
    #[inline]
    #[must_use]
    pub const fn month(self) -> u8 {
        self.month
    }

    /// The day of the month, 1–31.
    #[inline]
    #[must_use]
    pub const fn day(self) -> u8 {
        self.day
    }

    /// Whether this date's year is a Gregorian leap year.
    #[inline]
    #[must_use]
    pub const fn is_leap_year(self) -> bool {
        is_leap(self.year)
    }

    /// Today's date in UTC, from the system clock.
    ///
    /// UTC rather than local time: a collection date is a banking-calendar day
    /// agreed with the bank, and deriving it from an ambient timezone makes the
    /// same code emit different files on different machines.
    ///
    /// **Nothing in this crate calls it.** No builder defaults a payment date,
    /// because "today", "today + 5" and every other clock-derived answer is a
    /// banking-calendar question the crate cannot answer — it depends on the
    /// scheme, the sequence type, TARGET2 and the bank's cut-off. This is here
    /// for callers who have already decided that today is the right answer.
    #[must_use]
    pub fn today() -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        // Both saturations are toward the end of the calendar the clock is
        // actually past, never toward its start. `unwrap_or(Self::MIN)` here
        // turned a clock set past year 9999 into **0001-01-01** — a date that
        // is not merely wrong but wrong in the opposite direction, and one that
        // reads as a plausible sentinel rather than as a broken clock.
        let days = i64::try_from(secs / 86_400).unwrap_or(i64::MAX);
        Self::from_epoch_days(days).unwrap_or(Self::MAX)
    }

    /// Days since 1970-01-01, negative before it.
    ///
    /// # Examples
    ///
    /// ```
    /// use sepa::IsoDate;
    /// assert_eq!(IsoDate::new(1970, 1, 1)?.epoch_days(), 0);
    /// assert_eq!(IsoDate::new(1969, 12, 31)?.epoch_days(), -1);
    /// # Ok::<(), sepa::DateError>(())
    /// ```
    // `doy`, `doe` and `yoe` are the reference algorithm's own names — day of
    // year, day of era, year of era. Renaming them to satisfy `similar_names`
    // would make the code harder to check against the published version.
    #[allow(
        clippy::similar_names,
        reason = "names taken from the source algorithm"
    )]
    #[must_use]
    pub const fn epoch_days(self) -> i64 {
        // Howard Hinnant's `days_from_civil`.
        // https://howardhinnant.github.io/date_algorithms.html
        let y = self.year as i64 - if self.month <= 2 { 1 } else { 0 };
        let era = if y >= 0 { y } else { y - 399 } / 400;
        let yoe = y - era * 400; // 0..=399
        let m = self.month as i64;
        let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + self.day as i64 - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    /// The date `days` after 1970-01-01.
    ///
    /// # Errors
    ///
    /// [`DateError::OutOfRange`] when the day count falls outside
    /// `0001-01-01`–`9999-12-31`.
    // `doy`, `doe` and `yoe` are the reference algorithm's own names — day of
    // year, day of era, year of era. Renaming them to satisfy `similar_names`
    // would make the code harder to check against the published version.
    #[allow(
        clippy::similar_names,
        reason = "names taken from the source algorithm"
    )]
    pub fn from_epoch_days(days: i64) -> Result<Self, DateError> {
        // Reject well outside the representable range first: the algorithm
        // below shifts the input, and `i64::MAX + 719_468` would overflow.
        const MIN_DAYS: i64 = -719_162; // 0001-01-01
        const MAX_DAYS: i64 = 2_932_896; // 9999-12-31
        if !(MIN_DAYS..=MAX_DAYS).contains(&days) {
            return Err(DateError::OutOfRange { days });
        }

        // Howard Hinnant's `civil_from_days`.
        let z = days + 719_468;
        let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
        let doe = z - era * 146_097; // 0..=146_096
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = y + i64::from(m <= 2);

        // `m` is 1..=12 and `d` is 1..=31 by construction of the algorithm, and
        // the range guard above bounds `y` — but convert fallibly rather than
        // casting, so a future change to the bounds cannot silently truncate.
        let year = u16::try_from(y).map_err(|_| DateError::YearOutOfRange { year: y })?;
        let month = u8::try_from(m).map_err(|_| DateError::YearOutOfRange { year: y })?;
        let day = u8::try_from(d).map_err(|_| DateError::YearOutOfRange { year: y })?;
        Self::new(year, month, day)
    }

    /// The date `days` later — negative values move backwards.
    ///
    /// This is calendar arithmetic, not banking-day arithmetic: it knows
    /// nothing about weekends or TARGET2 holidays.
    ///
    /// # Errors
    ///
    /// [`DateError::OutOfRange`] when the result leaves
    /// `0001-01-01`–`9999-12-31`.
    ///
    /// # Examples
    ///
    /// ```
    /// use sepa::IsoDate;
    /// let d = IsoDate::new(2026, 2, 26)?;
    /// assert_eq!(d.plus_days(3)?.to_string(), "2026-03-01");
    /// assert_eq!(d.plus_days(-26)?.to_string(), "2026-01-31");
    /// # Ok::<(), sepa::DateError>(())
    /// ```
    pub fn plus_days(self, days: i64) -> Result<Self, DateError> {
        // Saturating rather than checked: an overflowing offset is out of range
        // in exactly the direction it saturated towards, and `from_epoch_days`
        // reports that with the day count the caller asked for.
        Self::from_epoch_days(self.epoch_days().saturating_add(days))
    }

    /// Write the `YYYY-MM-DD` form without allocating.
    ///
    /// # Errors
    ///
    /// Propagates the writer's error.
    pub(crate) fn write_to<W: fmt::Write>(self, w: &mut W) -> fmt::Result {
        write!(w, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

impl fmt::Display for IsoDate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.write_to(f)
    }
}

impl FromStr for IsoDate {
    type Err = DateError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl TryFrom<&str> for IsoDate {
    type Error = DateError;
    fn try_from(s: &str) -> Result<Self, Self::Error> {
        Self::parse(s)
    }
}

/// Whether `s` is an empty string or a valid `xs:date`/`xs:dateTime` timezone.
///
/// `Z`, `+hh:mm` or `-hh:mm`, with `hh` ≤ 14 and `mm` ≤ 59 — the range XML
/// Schema permits.
fn is_xsd_timezone(s: &str) -> bool {
    match s.as_bytes() {
        [] | [b'Z'] => true,
        [b'+' | b'-', h0, h1, b':', m0, m1] => {
            // The digit check has to short-circuit: `b - b'0'` underflows for
            // any byte below '0', and this runs on bank-supplied text. Computing
            // first and validating after is how a parser panics on a file.
            if ![h0, h1, m0, m1].iter().all(|b| b.is_ascii_digit()) {
                return false;
            }
            let hours = i16::from(h0 - b'0') * 10 + i16::from(h1 - b'0');
            let minutes = i16::from(m0 - b'0') * 10 + i16::from(m1 - b'0');
            hours <= 14 && minutes <= 59 && (hours < 14 || minutes == 0)
        }
        _ => false,
    }
}

const fn is_leap(year: u16) -> bool {
    year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
}

const fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(year) => 29,
        2 => 28,
        _ => 0,
    }
}

// ── IsoDateTime ───────────────────────────────────────────────────────────────

/// A validated ISO 8601 timestamp, rendered as `YYYY-MM-DDThh:mm:ss`.
///
/// This is the type of `GrpHdr/CreDtTm`. Seconds are whole: ISO 20022 permits
/// fractional seconds, but no scheme uses them and dropping them keeps a
/// regenerated file byte-identical to the submitted one.
///
/// A trailing `Z` or `±hh:mm` offset is preserved when parsed and reproduced on
/// output, so a timestamp read back from a bank file round-trips. Timestamps
/// built by [`IsoDateTime::now`] carry no offset, which is the form the EPC and
/// DK examples use.
///
/// ## Deliberately not `Ord`
///
/// A timestamp with no offset does not name an instant — `12:30:00` is a wall
/// clock somewhere, and which somewhere is not in the value. Comparing the
/// written fields would therefore be wrong in the one case that matters:
/// `2026-07-20T13:00:00+02:00` is an hour *earlier* than
/// `2026-07-20T12:00:00Z`, and any field-order comparison puts it later. Rather
/// than ship an ordering that is right for same-offset values and silently
/// wrong for mixed ones, this type has none. Compare
/// [`unix_seconds`](Self::unix_seconds), which is `None` for exactly the values
/// that cannot be compared.
///
/// Equality is on the written form, for the same reason: two spellings of one
/// instant are different `CreDtTm` values, and a message must reproduce the one
/// it was given. [`IsoDate`], which is what every SEPA *payment* date is, has
/// no offset and so is fully `Ord`.
///
/// # Examples
///
/// ```
/// use sepa::IsoDateTime;
///
/// let t: IsoDateTime = "2026-07-20T12:30:00".parse()?;
/// assert_eq!(t.to_string(), "2026-07-20T12:30:00");
/// assert_eq!(t.date().month(), 7);
///
/// // An offset survives the round trip.
/// let z: IsoDateTime = "2026-07-20T12:30:00Z".parse()?;
/// assert_eq!(z.to_string(), "2026-07-20T12:30:00Z");
///
/// // Offsets are honoured when instants are compared.
/// let berlin: IsoDateTime = "2026-07-20T13:00:00+02:00".parse()?;
/// assert!(berlin.unix_seconds() < z.unix_seconds());
/// # Ok::<(), sepa::DateTimeError>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IsoDateTime {
    date: IsoDate,
    hour: u8,
    minute: u8,
    second: u8,
    /// Offset from UTC in minutes; `Some(0)` renders as `Z`.
    offset_minutes: Option<i16>,
}

impl IsoDateTime {
    /// Build a timestamp from a date and a time of day, with no UTC offset.
    ///
    /// # Errors
    ///
    /// [`DateTimeError::NotATimeOfDay`] outside `00:00:00`–`23:59:59`. Leap
    /// seconds are not accepted; no ISO 20022 field uses them.
    pub fn new(date: IsoDate, hour: u8, minute: u8, second: u8) -> Result<Self, DateTimeError> {
        if hour > 23 || minute > 59 || second > 59 {
            return Err(DateTimeError::NotATimeOfDay {
                hour: u32::from(hour),
                minute: u32::from(minute),
                second: u32::from(second),
            });
        }
        Ok(Self {
            date,
            hour,
            minute,
            second,
            offset_minutes: None,
        })
    }

    /// The same instant tagged as UTC, so it renders with a trailing `Z`.
    #[must_use]
    pub const fn in_utc(mut self) -> Self {
        self.offset_minutes = Some(0);
        self
    }

    /// The current UTC time from the system clock, without an offset suffix.
    #[must_use]
    pub fn now() -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let days = i64::try_from(secs / 86_400).unwrap_or(i64::MAX);
        // See `IsoDate::today`: saturate forward, never back to year 1.
        let date = IsoDate::from_epoch_days(days).unwrap_or(IsoDate::MAX);
        // Each component is bounded well inside u8 before the cast.
        #[allow(clippy::cast_possible_truncation)]
        Self {
            date,
            hour: ((secs / 3600) % 24) as u8,
            minute: ((secs / 60) % 60) as u8,
            second: (secs % 60) as u8,
            offset_minutes: None,
        }
    }

    /// Parse `YYYY-MM-DDThh:mm:ss`, optionally with fractional seconds and a
    /// `Z` or `±hh:mm` offset.
    ///
    /// Fractional seconds are accepted and discarded — banks send them, and no
    /// SEPA field is specified to the sub-second.
    ///
    /// # Errors
    ///
    /// [`DateTimeError`] describing which part failed.
    pub fn parse(s: &str) -> Result<Self, DateTimeError> {
        let malformed = || DateTimeError::Malformed {
            value: s.to_owned(),
        };
        let (date_part, rest) = s.split_once('T').ok_or_else(malformed)?;
        let date = IsoDate::parse(date_part)?;

        // Split off the offset before touching the time, so `+` inside an
        // offset is never mistaken for part of the seconds field.
        //
        // `split_at_checked`, not `split_at`: the input is untrusted, and byte
        // `len - 6` can land inside a multi-byte character — `"…T€€a"` is
        // seven bytes, and splitting it at index 1 would panic.
        let offset_split = rest
            .len()
            .checked_sub(6)
            .and_then(|at| rest.split_at_checked(at));
        let (time_part, offset_minutes) = if let Some(head) = rest.strip_suffix('Z') {
            (head, Some(0i16))
        } else if let Some((head, tail)) = offset_split {
            // The offset, if present, is the final `±hh:mm`.
            match parse_offset(tail) {
                Some(off) => (head, Some(off)),
                None => (rest, None),
            }
        } else {
            (rest, None)
        };

        // Fractional seconds are legal and irrelevant.
        let time_part = time_part
            .split_once('.')
            .map_or(time_part, |(head, _)| head);
        let [h0, h1, b':', m0, m1, b':', s0, s1] = *time_part.as_bytes() else {
            return Err(malformed());
        };
        if ![h0, h1, m0, m1, s0, s1].iter().all(u8::is_ascii_digit) {
            return Err(malformed());
        }
        let d2 = |a: u8, b: u8| (a - b'0') * 10 + (b - b'0');
        let mut out = Self::new(date, d2(h0, h1), d2(m0, m1), d2(s0, s1))?;
        out.offset_minutes = offset_minutes;
        Ok(out)
    }

    /// The calendar date part.
    #[inline]
    #[must_use]
    pub const fn date(self) -> IsoDate {
        self.date
    }

    /// The hour, 0–23.
    #[inline]
    #[must_use]
    pub const fn hour(self) -> u8 {
        self.hour
    }

    /// The minute, 0–59.
    #[inline]
    #[must_use]
    pub const fn minute(self) -> u8 {
        self.minute
    }

    /// The second, 0–59.
    #[inline]
    #[must_use]
    pub const fn second(self) -> u8 {
        self.second
    }

    /// The UTC offset in minutes, when the timestamp carried one.
    #[inline]
    #[must_use]
    pub const fn offset_minutes(self) -> Option<i16> {
        self.offset_minutes
    }

    /// Seconds since 1970-01-01T00:00:00Z, or `None` without a UTC offset.
    ///
    /// This is the only sound way to order two timestamps: it is `None` for
    /// exactly the values that do not name an instant, so a comparison cannot
    /// quietly assume a timezone. See the [type docs](Self) for why
    /// [`IsoDateTime`] is not `Ord`.
    ///
    /// # Examples
    ///
    /// ```
    /// use sepa::IsoDateTime;
    ///
    /// let utc: IsoDateTime = "2026-07-20T12:00:00Z".parse()?;
    /// let berlin: IsoDateTime = "2026-07-20T13:00:00+02:00".parse()?;
    /// assert_eq!(berlin.unix_seconds(), Some(utc.unix_seconds().unwrap() - 3600));
    ///
    /// // A timestamp with no offset is not an instant.
    /// assert_eq!("2026-07-20T12:00:00".parse::<IsoDateTime>()?.unix_seconds(), None);
    /// # Ok::<(), sepa::DateTimeError>(())
    /// ```
    #[must_use]
    pub const fn unix_seconds(self) -> Option<i64> {
        let Some(offset) = self.offset_minutes else {
            return None;
        };
        let local = self.date.epoch_days() * 86_400
            + self.hour as i64 * 3600
            + self.minute as i64 * 60
            + self.second as i64;
        Some(local - offset as i64 * 60)
    }

    /// The same instant re-expressed at UTC, or `None` without a UTC offset.
    ///
    /// Rendering the result gives the `Z` form, so two timestamps written at
    /// different offsets can be compared, stored or logged in one spelling.
    ///
    /// # Examples
    ///
    /// ```
    /// use sepa::IsoDateTime;
    ///
    /// let berlin: IsoDateTime = "2026-07-20T13:00:00+02:00".parse()?;
    /// assert_eq!(berlin.to_utc().unwrap().to_string(), "2026-07-20T11:00:00Z");
    /// assert_eq!("2026-07-20T13:00:00".parse::<IsoDateTime>()?.to_utc(), None);
    /// # Ok::<(), sepa::DateTimeError>(())
    /// ```
    #[must_use]
    pub fn to_utc(self) -> Option<Self> {
        let secs = self.unix_seconds()?;
        let days = secs.div_euclid(86_400);
        let rest = secs.rem_euclid(86_400);
        let date = IsoDate::from_epoch_days(days).ok()?;
        // `rem_euclid` gives 0..=86_399, so every component is non-negative
        // and well inside u8 — but say so with a fallible conversion rather
        // than an `as` cast, since this is arithmetic on parsed input.
        Some(Self {
            date,
            hour: u8::try_from(rest / 3600).ok()?,
            minute: u8::try_from((rest / 60) % 60).ok()?,
            second: u8::try_from(rest % 60).ok()?,
            offset_minutes: Some(0),
        })
    }

    /// Replace the time of day, with components already known to be in range.
    ///
    /// Used only by the `time` / `chrono` conversions, whose source types
    /// guarantee the range.
    #[cfg(any(feature = "time", feature = "chrono"))]
    const fn with_time(mut self, hour: u8, minute: u8, second: u8) -> Self {
        self.hour = hour;
        self.minute = minute;
        self.second = second;
        self
    }
}

fn parse_offset(tail: &str) -> Option<i16> {
    let bytes = tail.as_bytes();
    let [sign, h0, h1, b':', m0, m1] = *bytes else {
        return None;
    };
    if !matches!(sign, b'+' | b'-') || ![h0, h1, m0, m1].iter().all(u8::is_ascii_digit) {
        return None;
    }
    let hours = i16::from(h0 - b'0') * 10 + i16::from(h1 - b'0');
    let minutes = i16::from(m0 - b'0') * 10 + i16::from(m1 - b'0');
    // `xs:dateTime` bounds the offset at ±14:00; anything beyond it names no
    // real timezone, and accepting it would let a bogus tail be read as an
    // offset instead of rejecting the timestamp.
    let total = hours * 60 + minutes;
    if minutes > 59 || total > 14 * 60 {
        return None;
    }
    Some(if sign == b'-' { -total } else { total })
}

impl fmt::Display for IsoDateTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.date.write_to(f)?;
        write!(f, "T{:02}:{:02}:{:02}", self.hour, self.minute, self.second)?;
        match self.offset_minutes {
            None => Ok(()),
            Some(0) => f.write_str("Z"),
            Some(off) => {
                let sign = if off < 0 { '-' } else { '+' };
                let abs = off.unsigned_abs();
                write!(f, "{sign}{:02}:{:02}", abs / 60, abs % 60)
            }
        }
    }
}

impl FromStr for IsoDateTime {
    type Err = DateTimeError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl TryFrom<&str> for IsoDateTime {
    type Error = DateTimeError;
    fn try_from(s: &str) -> Result<Self, Self::Error> {
        Self::parse(s)
    }
}

impl From<IsoDate> for IsoDateTime {
    /// Midnight on that date, with no offset.
    fn from(date: IsoDate) -> Self {
        Self {
            date,
            hour: 0,
            minute: 0,
            second: 0,
            offset_minutes: None,
        }
    }
}

// ── serde ─────────────────────────────────────────────────────────────────────

#[cfg(feature = "serde")]
mod serde_impls {
    use super::{IsoDate, IsoDateTime};
    use serde::de::{Error as _, Unexpected};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    impl Serialize for IsoDate {
        fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
            s.collect_str(self)
        }
    }

    impl<'de> Deserialize<'de> for IsoDate {
        fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            // `String`, not `&str`: a borrowed deserialiser is not always
            // available — `from_reader` and any escaped string need an owned
            // value, and asking for a borrow there fails at runtime.
            let raw = String::deserialize(d)?;
            Self::parse(&raw)
                .map_err(|_| D::Error::invalid_value(Unexpected::Str(&raw), &"a YYYY-MM-DD date"))
        }
    }

    impl Serialize for IsoDateTime {
        fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
            s.collect_str(self)
        }
    }

    impl<'de> Deserialize<'de> for IsoDateTime {
        fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            let raw = String::deserialize(d)?;
            Self::parse(&raw).map_err(|_| {
                D::Error::invalid_value(Unexpected::Str(&raw), &"a YYYY-MM-DDThh:mm:ss timestamp")
            })
        }
    }
}

// ── `time` interop ────────────────────────────────────────────────────────────

#[cfg(feature = "time")]
mod time_interop {
    use super::{ConversionError, DateError, IsoDate, IsoDateTime};

    impl TryFrom<time::Date> for IsoDate {
        type Error = DateError;
        fn try_from(d: time::Date) -> Result<Self, Self::Error> {
            let year = d.year();
            let year = u16::try_from(year).map_err(|_| DateError::YearOutOfRange {
                year: i64::from(year),
            })?;
            Self::new(year, u8::from(d.month()), d.day())
        }
    }

    impl TryFrom<IsoDate> for time::Date {
        type Error = time::error::ComponentRange;
        fn try_from(d: IsoDate) -> Result<Self, Self::Error> {
            Self::from_calendar_date(
                i32::from(d.year()),
                time::Month::try_from(d.month())?,
                d.day(),
            )
        }
    }

    impl TryFrom<time::PrimitiveDateTime> for IsoDateTime {
        type Error = DateError;
        /// The timestamp with **no** offset — `PrimitiveDateTime` carries none,
        /// so claiming UTC would invent one.
        fn try_from(t: time::PrimitiveDateTime) -> Result<Self, Self::Error> {
            let date = IsoDate::try_from(t.date())?;
            // `time` guarantees the components are in range, so `new` cannot fail.
            Ok(Self::from(date).with_time(t.hour(), t.minute(), t.second()))
        }
    }

    impl TryFrom<IsoDateTime> for time::PrimitiveDateTime {
        type Error = time::error::ComponentRange;
        /// Drops the UTC offset, if any: `PrimitiveDateTime` has nowhere to put
        /// it. Convert to [`time::OffsetDateTime`] instead when the offset is
        /// the point.
        fn try_from(t: IsoDateTime) -> Result<Self, Self::Error> {
            Ok(Self::new(
                time::Date::try_from(t.date())?,
                time::Time::from_hms(t.hour(), t.minute(), t.second())?,
            ))
        }
    }

    impl TryFrom<time::OffsetDateTime> for IsoDateTime {
        type Error = DateError;
        /// Keeps the offset, so the timestamp still names an instant.
        fn try_from(t: time::OffsetDateTime) -> Result<Self, Self::Error> {
            let date = IsoDate::try_from(t.date())?;
            let mut out = Self::from(date).with_time(t.hour(), t.minute(), t.second());
            // `time` bounds an offset at ±25:59:59 and ISO 20022 at ±14:00;
            // beyond that there is no `xs:dateTime` to write, so the value is
            // rendered at UTC rather than with an offset no schema accepts.
            // `time` already types whole minutes as `i16`; ISO 20022 bounds an
            // `xs:dateTime` offset at ±14:00, and beyond that there is no
            // spelling to write — so re-express the same instant at UTC rather
            // than emit an offset no schema accepts.
            let minutes = t.offset().whole_minutes();
            if minutes.abs() <= 14 * 60 {
                out.offset_minutes = Some(minutes);
                Ok(out)
            } else {
                let utc = t.to_offset(time::UtcOffset::UTC);
                let date = IsoDate::try_from(utc.date())?;
                Ok(Self::from(date)
                    .with_time(utc.hour(), utc.minute(), utc.second())
                    .in_utc())
            }
        }
    }

    impl TryFrom<IsoDateTime> for time::OffsetDateTime {
        type Error = ConversionError;
        /// # Errors
        ///
        /// [`ConversionError::NoOffset`] when the timestamp carries no UTC
        /// offset: it names a wall clock, not an instant, and picking one would
        /// be the invention this crate refuses to make. See
        /// [`IsoDateTime::unix_seconds`].
        fn try_from(t: IsoDateTime) -> Result<Self, Self::Error> {
            let minutes = t.offset_minutes().ok_or(ConversionError::NoOffset)?;
            let offset = time::UtcOffset::from_whole_seconds(i32::from(minutes) * 60)
                .map_err(|_| ConversionError::OutOfRange)?;
            let naive =
                time::PrimitiveDateTime::try_from(t).map_err(|_| ConversionError::OutOfRange)?;
            Ok(naive.assume_offset(offset))
        }
    }
}

// ── `chrono` interop ──────────────────────────────────────────────────────────

#[cfg(feature = "chrono")]
mod chrono_interop {
    use super::{ConversionError, DateError, IsoDate, IsoDateTime};
    use chrono::{Datelike, Timelike};

    impl TryFrom<chrono::NaiveDate> for IsoDate {
        type Error = DateError;
        fn try_from(d: chrono::NaiveDate) -> Result<Self, Self::Error> {
            let year = d.year();
            let year = u16::try_from(year).map_err(|_| DateError::YearOutOfRange {
                year: i64::from(year),
            })?;
            // `chrono` months and days are 1-based u32 within calendar range.
            #[allow(clippy::cast_possible_truncation)]
            Self::new(year, d.month() as u8, d.day() as u8)
        }
    }

    impl TryFrom<IsoDate> for chrono::NaiveDate {
        type Error = DateError;
        fn try_from(d: IsoDate) -> Result<Self, Self::Error> {
            Self::from_ymd_opt(
                i32::from(d.year()),
                u32::from(d.month()),
                u32::from(d.day()),
            )
            .ok_or(DateError::NotACalendarDate {
                year: i64::from(d.year()),
                month: u32::from(d.month()),
                day: u32::from(d.day()),
            })
        }
    }

    impl TryFrom<chrono::NaiveDateTime> for IsoDateTime {
        type Error = DateError;
        /// The timestamp with **no** offset — `NaiveDateTime` carries none, so
        /// claiming UTC would invent one.
        fn try_from(t: chrono::NaiveDateTime) -> Result<Self, Self::Error> {
            let date = IsoDate::try_from(t.date())?;
            // `chrono` guarantees the components are in range.
            #[allow(clippy::cast_possible_truncation)]
            Ok(Self::from(date).with_time(t.hour() as u8, t.minute() as u8, t.second() as u8))
        }
    }

    impl TryFrom<IsoDateTime> for chrono::NaiveDateTime {
        type Error = DateError;
        /// Drops the UTC offset, if any: `NaiveDateTime` has nowhere to put it.
        /// Convert to `chrono::DateTime<FixedOffset>` when the offset matters.
        fn try_from(t: IsoDateTime) -> Result<Self, Self::Error> {
            let date = chrono::NaiveDate::try_from(t.date())?;
            date.and_hms_opt(
                u32::from(t.hour()),
                u32::from(t.minute()),
                u32::from(t.second()),
            )
            .ok_or(DateError::NotACalendarDate {
                year: i64::from(t.date().year()),
                month: u32::from(t.date().month()),
                day: u32::from(t.date().day()),
            })
        }
    }

    impl<Tz: chrono::TimeZone> TryFrom<chrono::DateTime<Tz>> for IsoDateTime {
        type Error = DateError;
        /// Keeps the offset, so the timestamp still names an instant.
        fn try_from(t: chrono::DateTime<Tz>) -> Result<Self, Self::Error> {
            use chrono::Offset as _;
            let fixed = t.offset().fix();
            let naive = t.naive_local();
            let mut out = Self::try_from(naive)?;
            // ISO 20022 bounds `xs:dateTime` offsets at ±14:00; a zone outside
            // that has no spelling, so fall back to UTC rather than write one.
            out.offset_minutes = i16::try_from(fixed.local_minus_utc() / 60)
                .ok()
                .filter(|m| m.abs() <= 14 * 60);
            if out.offset_minutes.is_none() {
                out = Self::try_from(t.naive_utc())?.in_utc();
            }
            Ok(out)
        }
    }

    impl TryFrom<IsoDateTime> for chrono::DateTime<chrono::FixedOffset> {
        type Error = ConversionError;
        /// # Errors
        ///
        /// [`ConversionError::NoOffset`] when the timestamp carries no UTC
        /// offset — it names a wall clock, not an instant. See
        /// [`IsoDateTime::unix_seconds`].
        fn try_from(t: IsoDateTime) -> Result<Self, Self::Error> {
            let minutes = t.offset_minutes().ok_or(ConversionError::NoOffset)?;
            let offset = chrono::FixedOffset::east_opt(i32::from(minutes) * 60)
                .ok_or(ConversionError::OutOfRange)?;
            let naive =
                chrono::NaiveDateTime::try_from(t).map_err(|_| ConversionError::OutOfRange)?;
            naive
                .and_local_timezone(offset)
                .single()
                .ok_or(ConversionError::OutOfRange)
        }
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "time")]
    #[test]
    fn time_timestamps_convert_in_both_directions() {
        // Regression: only `time::PrimitiveDateTime -> IsoDateTime` existed,
        // while the module docs promised both directions for every type.
        let bare: IsoDateTime = "2026-07-20T13:00:00".parse().unwrap();
        let primitive = time::PrimitiveDateTime::try_from(bare).unwrap();
        assert_eq!(IsoDateTime::try_from(primitive).unwrap(), bare);

        // An offset is an instant, and the instant is what must survive.
        let berlin: IsoDateTime = "2026-07-20T13:00:00+02:00".parse().unwrap();
        let offset = time::OffsetDateTime::try_from(berlin).unwrap();
        assert_eq!(offset.unix_timestamp(), berlin.unix_seconds().unwrap());
        assert_eq!(IsoDateTime::try_from(offset).unwrap(), berlin);

        // Without one there is no instant to hand over, and none is invented.
        assert_eq!(
            time::OffsetDateTime::try_from(bare),
            Err(ConversionError::NoOffset)
        );
    }

    #[cfg(feature = "chrono")]
    #[test]
    fn chrono_timestamps_convert_in_both_directions() {
        let bare: IsoDateTime = "2026-07-20T13:00:00".parse().unwrap();
        let naive = chrono::NaiveDateTime::try_from(bare).unwrap();
        assert_eq!(IsoDateTime::try_from(naive).unwrap(), bare);

        let berlin: IsoDateTime = "2026-07-20T13:00:00+02:00".parse().unwrap();
        let dt = chrono::DateTime::<chrono::FixedOffset>::try_from(berlin).unwrap();
        assert_eq!(dt.timestamp(), berlin.unix_seconds().unwrap());
        assert_eq!(IsoDateTime::try_from(dt).unwrap(), berlin);

        assert_eq!(
            chrono::DateTime::<chrono::FixedOffset>::try_from(bare),
            Err(ConversionError::NoOffset)
        );

        // A UTC `DateTime` renders as `Z`, which is the spelling ISO 20022 uses.
        let utc = chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(naive, chrono::Utc);
        assert_eq!(
            IsoDateTime::try_from(utc).unwrap().to_string(),
            "2026-07-20T13:00:00Z"
        );
    }

    #[test]
    fn timestamps_compare_as_instants_not_as_written_fields() {
        // Regression: `IsoDateTime` derived `Ord` over (date, h, m, s, offset),
        // so `13:00:00+02:00` — an hour *earlier* than `12:00:00Z` — sorted
        // after it. The type no longer has an ordering; `unix_seconds` does,
        // and it is `None` for exactly the values that name no instant.
        let utc: IsoDateTime = "2026-07-20T12:00:00Z".parse().unwrap();
        let berlin: IsoDateTime = "2026-07-20T13:00:00+02:00".parse().unwrap();
        let naive: IsoDateTime = "2026-07-20T12:00:00".parse().unwrap();

        assert!(berlin.unix_seconds() < utc.unix_seconds());
        assert_eq!(
            utc.unix_seconds().unwrap() - berlin.unix_seconds().unwrap(),
            3600
        );
        assert_eq!(naive.unix_seconds(), None);

        // Equality stays on the written form: a `CreDtTm` must round-trip the
        // spelling it was given.
        assert_ne!(utc, berlin);
        assert_eq!(
            berlin.to_utc().unwrap(),
            "2026-07-20T11:00:00Z".parse().unwrap()
        );
        assert_eq!(naive.to_utc(), None);
        assert_eq!(utc.to_utc(), Some(utc));
    }

    #[test]
    fn a_utc_offset_beyond_the_xsd_range_is_not_an_offset() {
        // `xs:dateTime` bounds the offset at ±14:00. Anything past it names no
        // timezone, so it must not be read as one — the tail then fails to
        // parse as a time and the whole value is rejected.
        assert!("2026-07-20T12:00:00+14:00".parse::<IsoDateTime>().is_ok());
        assert_eq!(
            "2026-07-20T12:00:00-14:00"
                .parse::<IsoDateTime>()
                .unwrap()
                .offset_minutes(),
            Some(-840)
        );
        for bad in [
            "2026-07-20T12:00:00+14:01",
            "2026-07-20T12:00:00+15:00",
            "2026-07-20T12:00:00+23:59",
        ] {
            assert!(
                bad.parse::<IsoDateTime>().is_err(),
                "{bad} must be rejected"
            );
        }
    }

    #[test]
    fn unix_seconds_agrees_with_the_epoch_across_the_calendar() {
        for (text, secs) in [
            ("1970-01-01T00:00:00Z", 0i64),
            ("1970-01-01T00:00:01Z", 1),
            ("1969-12-31T23:59:59Z", -1),
            ("2026-07-20T12:34:56Z", 1_784_550_896),
        ] {
            let t: IsoDateTime = text.parse().unwrap();
            assert_eq!(t.unix_seconds(), Some(secs), "{text}");
            assert_eq!(t.to_utc().unwrap().to_string(), text, "{text} round trip");
        }
    }

    #[test]
    fn parses_and_renders_the_iso_form() {
        let d = IsoDate::parse("2026-07-20").unwrap();
        assert_eq!((d.year(), d.month(), d.day()), (2026, 7, 20));
        assert_eq!(d.to_string(), "2026-07-20");
    }

    #[test]
    fn rejects_everything_that_is_not_a_bare_iso_date() {
        for bad in [
            "2026-7-20",
            "20260720",
            "2026/07/20",
            "26-07-20",
            "",
            "not-a-date",
            "2026-07-20T00:00:00",
            "2026-07-20 ",
            " 2026-07-20",
        ] {
            assert!(
                matches!(IsoDate::parse(bad), Err(DateError::Malformed { .. })),
                "{bad:?} must be malformed"
            );
        }
    }

    #[test]
    fn rejects_impossible_calendar_days() {
        for (bad, why) in [
            ("2026-02-30", "February has 28 days in 2026"),
            ("2023-02-29", "2023 is not a leap year"),
            ("1900-02-29", "1900 is a century non-leap year"),
            ("2026-13-01", "there is no month 13"),
            ("2026-00-10", "there is no month 0"),
            ("2026-07-00", "there is no day 0"),
            ("2026-07-32", "July has 31 days"),
        ] {
            assert!(
                matches!(IsoDate::parse(bad), Err(DateError::NotACalendarDate { .. })),
                "{bad:?} must be rejected: {why}"
            );
        }
        // The leap-year rules that the naive `% 4` test gets wrong.
        assert!(IsoDate::parse("2000-02-29").is_ok());
        assert!(IsoDate::parse("2024-02-29").is_ok());
    }

    #[test]
    fn the_date_part_of_a_timestamp_parses_where_a_strict_date_would_not() {
        let d = IsoDate::new(2026, 7, 20).unwrap();
        for raw in [
            "2026-07-20",
            "2026-07-20T00:00:00",
            "2026-07-20T12:30:00.123Z",
            "2026-07-20+02:00",
        ] {
            assert_eq!(IsoDate::parse_date_part(raw), Ok(d), "{raw}");
        }
        // An impossible day is still impossible with a time attached.
        assert!(IsoDate::parse_date_part("2026-02-30T00:00:00").is_err());
        assert!(IsoDate::parse_date_part("20.07.2026").is_err());
        // Byte 10 landing inside a multi-byte character must not panic.
        for bad in ["2026-07-2€", "2026-07-€0T00", "ü"] {
            assert!(IsoDate::parse_date_part(bad).is_err(), "{bad:?}");
        }
        // The strict parser still refuses everything but a bare date.
        assert!(IsoDate::parse("2026-07-20T00:00:00").is_err());
    }

    #[test]
    fn year_zero_is_rejected() {
        // `xs:date` has no year zero, and year 0 would otherwise test as leap.
        assert!(matches!(
            IsoDate::parse("0000-01-01"),
            Err(DateError::YearOutOfRange { year: 0 })
        ));
        assert!(IsoDate::parse("0001-01-01").is_ok());
    }

    #[test]
    fn epoch_days_round_trip_across_the_whole_range() {
        for date in [
            IsoDate::MIN,
            IsoDate::new(1969, 12, 31).unwrap(),
            IsoDate::new(1970, 1, 1).unwrap(),
            IsoDate::new(2000, 2, 29).unwrap(),
            IsoDate::new(2026, 7, 20).unwrap(),
            IsoDate::MAX,
        ] {
            let days = date.epoch_days();
            assert_eq!(IsoDate::from_epoch_days(days).unwrap(), date, "{date}");
        }
        assert_eq!(IsoDate::new(1970, 1, 1).unwrap().epoch_days(), 0);
        assert_eq!(IsoDate::new(1969, 12, 31).unwrap().epoch_days(), -1);
    }

    #[test]
    fn epoch_days_are_dense_and_monotonic_over_a_leap_year() {
        let mut d = IsoDate::new(2024, 1, 1).unwrap();
        let start = d.epoch_days();
        let mut count = 0i64;
        while d < IsoDate::new(2025, 1, 1).unwrap() {
            assert_eq!(d.epoch_days(), start + count);
            d = d.plus_days(1).unwrap();
            count += 1;
        }
        assert_eq!(count, 366, "2024 is a leap year");
    }

    #[test]
    fn arithmetic_crosses_month_and_year_boundaries() {
        let d = IsoDate::new(2026, 2, 26).unwrap();
        assert_eq!(d.plus_days(3).unwrap().to_string(), "2026-03-01");
        assert_eq!(d.plus_days(-57).unwrap().to_string(), "2025-12-31");
        assert_eq!(
            IsoDate::new(2024, 2, 28).unwrap().plus_days(1).unwrap(),
            IsoDate::new(2024, 2, 29).unwrap()
        );
    }

    #[test]
    fn arithmetic_past_the_representable_range_errors_rather_than_wrapping() {
        assert!(matches!(
            IsoDate::MAX.plus_days(1),
            Err(DateError::OutOfRange { .. })
        ));
        assert!(matches!(
            IsoDate::MIN.plus_days(-1),
            Err(DateError::OutOfRange { .. })
        ));
        // Saturating arithmetic: these must report, not wrap around.
        assert!(matches!(
            IsoDate::MAX.plus_days(i64::MAX),
            Err(DateError::OutOfRange { .. })
        ));
        assert!(matches!(
            IsoDate::MIN.plus_days(i64::MIN),
            Err(DateError::OutOfRange { .. })
        ));
        assert!(matches!(
            IsoDate::from_epoch_days(i64::MAX),
            Err(DateError::OutOfRange { .. })
        ));
        assert!(matches!(
            IsoDate::from_epoch_days(i64::MIN),
            Err(DateError::OutOfRange { .. })
        ));
    }

    #[test]
    fn ordering_is_chronological() {
        let mut dates = [
            IsoDate::new(2026, 1, 31).unwrap(),
            IsoDate::new(2025, 12, 1).unwrap(),
            IsoDate::new(2026, 1, 2).unwrap(),
        ];
        dates.sort_unstable();
        assert_eq!(
            dates.iter().map(ToString::to_string).collect::<Vec<_>>(),
            ["2025-12-01", "2026-01-02", "2026-01-31"]
        );
    }

    #[test]
    fn today_is_a_real_date() {
        let today = IsoDate::today();
        assert!(today > IsoDate::new(2020, 1, 1).unwrap());
        assert_eq!(IsoDate::parse(&today.to_string()).unwrap(), today);
    }

    #[test]
    fn timestamps_parse_render_and_keep_their_offset() {
        for (raw, rendered) in [
            ("2026-07-20T12:30:00", "2026-07-20T12:30:00"),
            ("2026-07-20T12:30:00Z", "2026-07-20T12:30:00Z"),
            ("2026-07-20T12:30:00.123", "2026-07-20T12:30:00"),
            ("2026-07-20T12:30:00.123456Z", "2026-07-20T12:30:00Z"),
            ("2026-07-20T12:30:00+02:00", "2026-07-20T12:30:00+02:00"),
            ("2026-07-20T12:30:00-05:30", "2026-07-20T12:30:00-05:30"),
        ] {
            assert_eq!(
                IsoDateTime::parse(raw).unwrap().to_string(),
                rendered,
                "{raw}"
            );
        }
    }

    #[test]
    fn malformed_timestamps_are_rejected() {
        for bad in [
            "2026-07-20",
            "2026-07-20T12:30",
            "2026-07-20 12:30:00",
            "2026-07-20T25:00:00",
            "2026-07-20T12:60:00",
            "2026-02-30T12:00:00",
            "",
        ] {
            assert!(IsoDateTime::parse(bad).is_err(), "{bad:?} must be rejected");
        }
    }

    #[test]
    fn a_multibyte_time_part_is_rejected_rather_than_panicking() {
        // Regression: the offset was split off with `rest.split_at(len - 6)`,
        // and for `"€€a"` — seven bytes — index 1 falls inside the first '€'.
        // `IsoDateTime` parses untrusted text, so that was a reachable panic.
        for bad in [
            "2026-07-20T€€a",
            "2026-07-20T€€",
            "2026-07-20T12:30:0€",
            "2026-07-20T€€€€€€€",
            "2026-07-20T12:30:00+0€:00",
        ] {
            assert!(IsoDateTime::parse(bad).is_err(), "{bad:?} must be rejected");
        }
    }

    #[test]
    fn now_has_no_offset_and_in_utc_adds_one() {
        let now = IsoDateTime::now();
        assert_eq!(now.offset_minutes(), None);
        assert!(!now.to_string().ends_with('Z'));
        assert!(now.in_utc().to_string().ends_with('Z'));
        assert_eq!(IsoDateTime::parse(&now.to_string()).unwrap(), now);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_round_trips_through_the_iso_string() {
        let d = IsoDate::new(2026, 7, 20).unwrap();
        let json = serde_json::to_string(&d).unwrap();
        assert_eq!(json, "\"2026-07-20\"");
        assert_eq!(serde_json::from_str::<IsoDate>(&json).unwrap(), d);
        assert!(serde_json::from_str::<IsoDate>("\"2026-02-30\"").is_err());

        let t = IsoDateTime::parse("2026-07-20T12:30:00Z").unwrap();
        let json = serde_json::to_string(&t).unwrap();
        assert_eq!(json, "\"2026-07-20T12:30:00Z\"");
        assert_eq!(serde_json::from_str::<IsoDateTime>(&json).unwrap(), t);

        // A non-borrowing deserialiser must work too: `from_reader` cannot hand
        // out a `&str`, so asking for one would fail at runtime.
        assert_eq!(
            serde_json::from_reader::<_, IsoDate>(br#""2026-07-20""#.as_slice()).unwrap(),
            d
        );
    }

    #[cfg(feature = "time")]
    #[test]
    fn time_types_convert_both_ways() {
        let d = IsoDate::new(2026, 7, 20).unwrap();
        let t: time::Date = d.try_into().unwrap();
        assert_eq!(t.year(), 2026);
        assert_eq!(IsoDate::try_from(t).unwrap(), d);
    }

    #[cfg(feature = "chrono")]
    #[test]
    fn chrono_types_convert_both_ways() {
        let d = IsoDate::new(2026, 7, 20).unwrap();
        let c: chrono::NaiveDate = d.try_into().unwrap();
        assert_eq!(c.to_string(), "2026-07-20");
        assert_eq!(IsoDate::try_from(c).unwrap(), d);
    }
}
