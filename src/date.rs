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
//! With the `time` or `chrono` feature, the corresponding date types convert in
//! both directions, so a caller that already has a typed date never formats a
//! string. The conversions are fallible in both directions: `time` and `chrono`
//! represent years this type deliberately does not.
//!
//! ```
//! # #[cfg(feature = "time")]
//! # fn demo() -> Result<(), Box<dyn std::error::Error>> {
//! use sepa::IsoDate;
//!
//! let from_time = time::Date::from_calendar_date(2026, time::Month::July, 20)?;
//! let date = IsoDate::try_from(from_time)?;
//! assert_eq!(date.to_string(), "2026-07-20");
//!
//! let back: time::Date = date.try_into()?;
//! assert_eq!(back, from_time);
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
        // Destructuring the exact byte pattern rejects every other length,
        // separator and non-digit without a single fallible index.
        let [y0, y1, y2, y3, b'-', m0, m1, b'-', d0, d1] = *s.as_bytes() else {
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
    /// posts by the day either way. [`parse`](Self::parse) stays strict,
    /// because a date a *builder* writes must be exactly an `xs:date`.
    ///
    /// # Errors
    ///
    /// [`DateError`] when the leading ten characters are not a real calendar
    /// date. Anything after them is ignored, not validated.
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
        // `get`, not a slice: `s` is bank-supplied and byte 10 can land inside
        // a multi-byte character.
        let head = s.get(..10).unwrap_or(s);
        Self::parse(head)
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
    #[must_use]
    pub fn today() -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        // A u64 second count divided by 86_400 cannot exceed i64 range in any
        // clock this program could observe, and the epoch day is always valid.
        let days = i64::try_from(secs / 86_400).unwrap_or(i64::MAX);
        Self::from_epoch_days(days).unwrap_or(Self::MIN)
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

const fn is_leap(year: u16) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
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
/// # Ok::<(), sepa::DateTimeError>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
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
        let date = IsoDate::from_epoch_days(days).unwrap_or(IsoDate::MIN);
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
        let (time_part, offset_minutes) = if let Some(head) = rest.strip_suffix('Z') {
            (head, Some(0i16))
        } else if rest.len() >= 6 {
            // The offset, if present, is the final `±hh:mm`.
            let (head, tail) = rest.split_at(rest.len() - 6);
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
    if hours > 23 || minutes > 59 {
        return None;
    }
    let total = hours * 60 + minutes;
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
    use super::{DateError, IsoDate, IsoDateTime};

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
        fn try_from(t: time::PrimitiveDateTime) -> Result<Self, Self::Error> {
            let date = IsoDate::try_from(t.date())?;
            // `time` guarantees the components are in range, so `new` cannot fail.
            Ok(Self::from(date).with_time(t.hour(), t.minute(), t.second()))
        }
    }
}

// ── `chrono` interop ──────────────────────────────────────────────────────────

#[cfg(feature = "chrono")]
mod chrono_interop {
    use super::{DateError, IsoDate, IsoDateTime};
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
        fn try_from(t: chrono::NaiveDateTime) -> Result<Self, Self::Error> {
            let date = IsoDate::try_from(t.date())?;
            // `chrono` guarantees the components are in range.
            #[allow(clippy::cast_possible_truncation)]
            Ok(Self::from(date).with_time(t.hour() as u8, t.minute() as u8, t.second() as u8))
        }
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

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
