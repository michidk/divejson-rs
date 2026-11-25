//! The scalar value kinds of spec §5.2 and §5.3: identifiers, dates and date-times.
//!
//! Each one parses the exact grammar the specification and schema allow and keeps what a
//! generic type would lose: a date-time keeps the text it was written as, so `Z`, `z`,
//! `+00:00` and the number of fractional digits all survive a round trip, and a local
//! date-time stays a wall clock with no offset invented for it.

use alloc::string::{String, ToString};
use core::fmt;
use core::str::FromStr;

use core::fmt::Write as _;

use jiff::Timestamp;
use jiff::civil::{self, Time};
use jiff::tz::Offset;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

/// Why a string is not a DiveJSON uuid, date or date-time.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{value:?} is not a DiveJSON {kind}")]
pub struct ValueError {
    kind: &'static str,
    value: String,
}

impl ValueError {
    fn new(kind: &'static str, value: &str) -> Self {
        Self {
            kind,
            value: value.to_string(),
        }
    }
}

/// An [RFC 9562] UUID in the canonical lowercase hyphenated form §5.3 requires.
///
/// Uppercase, braced, URN and unhyphenated spellings are rejected on parse.
///
/// [RFC 9562]: https://www.rfc-editor.org/rfc/rfc9562
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Uuid(pub uuid::Uuid);

impl Uuid {
    /// The wrapped [`uuid::Uuid`].
    #[must_use]
    pub const fn get(self) -> uuid::Uuid {
        self.0
    }
}

impl From<uuid::Uuid> for Uuid {
    fn from(value: uuid::Uuid) -> Self {
        Self(value)
    }
}

impl From<Uuid> for uuid::Uuid {
    fn from(value: Uuid) -> Self {
        value.0
    }
}

impl FromStr for Uuid {
    type Err = ValueError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let canonical = s.len() == 36
            && s.bytes().enumerate().all(|(i, b)| match i {
                8 | 13 | 18 | 23 => b == b'-',
                _ => b.is_ascii_digit() || (b'a'..=b'f').contains(&b),
            });
        if !canonical {
            return Err(ValueError::new("uuid", s));
        }
        uuid::Uuid::try_parse(s)
            .map(Self)
            .map_err(|_| ValueError::new("uuid", s))
    }
}

impl fmt::Display for Uuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0.hyphenated(), f)
    }
}

/// A calendar date, `YYYY-MM-DD` (§5.2): a calendar fact, not an instant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date(civil::Date);

impl Date {
    /// The date as a [`jiff::civil::Date`].
    #[must_use]
    pub const fn get(self) -> civil::Date {
        self.0
    }
}

impl TryFrom<civil::Date> for Date {
    type Error = ValueError;

    /// Fails for a year outside `0000`–`9999`, which the four-digit grammar cannot write.
    fn try_from(value: civil::Date) -> Result<Self, Self::Error> {
        if (0..=9999).contains(&value.year()) {
            Ok(Self(value))
        } else {
            Err(ValueError::new("date", &value.to_string()))
        }
    }
}

impl FromStr for Date {
    type Err = ValueError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse_date(s)
            .map(Self)
            .ok_or_else(|| ValueError::new("date", s))
    }
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let date = self.0;
        write!(
            f,
            "{:04}-{:02}-{:02}",
            date.year(),
            date.month(),
            date.day()
        )
    }
}

/// A date-time (§5.2): a wall clock with its UTC offset, or a *local* date-time without
/// one when the source recorded none.
///
/// The original text is kept and written back unchanged; equality compares that text, so
/// `…Z` and `…+00:00` are equal instants but unequal values. [`DateTime::offset`] is `None`
/// for a local date-time, which readers must not convert to any zone.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DateTime {
    text: String,
    local: civil::DateTime,
    offset: Option<Offset>,
}

impl DateTime {
    /// The wall clock as written.
    #[must_use]
    pub const fn local(&self) -> civil::DateTime {
        self.local
    }

    /// The UTC offset, or `None` for a local date-time.
    #[must_use]
    pub const fn offset(&self) -> Option<Offset> {
        self.offset
    }

    /// The instant, when the value carries an offset.
    #[must_use]
    pub fn to_timestamp(&self) -> Option<Timestamp> {
        self.offset
            .and_then(|offset| offset.to_timestamp(self.local).ok())
    }

    /// The text exactly as it was parsed or written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// A local date-time: a wall clock whose offset the source did not record.
    ///
    /// # Errors
    /// When the year is outside `0000`–`9999`.
    pub fn from_local(local: civil::DateTime) -> Result<Self, ValueError> {
        format_date_time(local, None).parse()
    }

    /// A wall clock with its UTC offset, written numerically (`+00:00`, never `Z`).
    ///
    /// # Errors
    /// When the year is outside `0000`–`9999`, or the offset is not whole minutes.
    pub fn with_offset(local: civil::DateTime, offset: Offset) -> Result<Self, ValueError> {
        format_date_time(local, Some(offset)).parse()
    }
}

impl From<DateTime> for String {
    fn from(value: DateTime) -> Self {
        value.text
    }
}

impl FromStr for DateTime {
    type Err = ValueError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse_date_time(s).ok_or_else(|| ValueError::new("date-time", s))
    }
}

impl fmt::Display for DateTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

/// A dive's `started_at` (§6.2): a date-time, or a date alone when the source recorded
/// the day and not the time of day. Readers must not place a date-only dive at midnight.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum StartedAt {
    /// The day only.
    Date(Date),
    /// A wall clock, with its offset when the source recorded one.
    DateTime(DateTime),
}

impl From<Date> for StartedAt {
    fn from(value: Date) -> Self {
        Self::Date(value)
    }
}

impl From<DateTime> for StartedAt {
    fn from(value: DateTime) -> Self {
        Self::DateTime(value)
    }
}

impl FromStr for StartedAt {
    type Err = ValueError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() == 10 {
            s.parse().map(Self::Date)
        } else {
            s.parse().map(Self::DateTime)
        }
        .map_err(|_| ValueError::new("date-time or date", s))
    }
}

impl fmt::Display for StartedAt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Date(date) => date.fmt(f),
            Self::DateTime(date_time) => date_time.fmt(f),
        }
    }
}

macro_rules! string_serde {
    ($($ty:ty),*) => {$(
        impl Serialize for $ty {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.collect_str(self)
            }
        }

        impl<'de> Deserialize<'de> for $ty {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                struct Visitor;
                impl de::Visitor<'_> for Visitor {
                    type Value = $ty;
                    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                        f.write_str(stringify!($ty))
                    }
                    fn visit_str<E: de::Error>(self, v: &str) -> Result<$ty, E> {
                        v.parse().map_err(E::custom)
                    }
                }
                deserializer.deserialize_str(Visitor)
            }
        }
    )*};
}

string_serde!(Uuid, Date, DateTime, StartedAt);

fn digits(s: &str) -> Option<u32> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

fn parse_date(s: &str) -> Option<civil::Date> {
    let (year, rest) = s.split_at_checked(4)?;
    let rest = rest.strip_prefix('-')?;
    let (month, rest) = rest.split_at_checked(2)?;
    let day = rest.strip_prefix('-')?;
    if day.len() != 2 {
        return None;
    }
    civil::Date::new(
        i16::try_from(digits(year)?).ok()?,
        i8::try_from(digits(month)?).ok()?,
        i8::try_from(digits(day)?).ok()?,
    )
    .ok()
}

fn two(s: &str) -> Option<i8> {
    if s.len() == 2 {
        i8::try_from(digits(s)?).ok()
    } else {
        None
    }
}

/// `YYYY-MM-DDTHH:MM:SS(.f+)?(Z|z|±HH:MM)?`, every field a real calendar value.
fn parse_date_time(s: &str) -> Option<DateTime> {
    let (date, rest) = s.split_at_checked(10)?;
    let rest = rest.strip_prefix('T')?;
    let (clock, mut rest) = rest.split_at_checked(8)?;
    let mut fields = clock.split(':');
    let (hour, minute, second) = (
        two(fields.next()?)?,
        two(fields.next()?)?,
        two(fields.next()?)?,
    );
    if fields.next().is_some() {
        return None;
    }
    let mut nanos = 0;
    if let Some(fraction) = rest.strip_prefix('.') {
        let end = fraction
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(fraction.len());
        let (frac_digits, after) = fraction.split_at_checked(end)?;
        if frac_digits.is_empty() {
            return None;
        }
        // Nanoseconds are the finest grain jiff keeps; the text keeps every digit.
        let mut padded: String = frac_digits.chars().take(9).collect();
        while padded.len() < 9 {
            padded.push('0');
        }
        nanos = digits(&padded)?;
        rest = after;
    }
    let offset = match rest {
        "" => None,
        "Z" | "z" => Some(Offset::UTC),
        _ => {
            let (sign, hm) = rest.split_at_checked(1)?;
            let (oh, om) = hm.split_once(':')?;
            let (oh, om) = (i32::from(two(oh)?), i32::from(two(om)?));
            if oh > 23 || om > 59 {
                return None;
            }
            let seconds = oh.checked_mul(3600)?.checked_add(om.checked_mul(60)?)?;
            match sign {
                "+" => Some(Offset::from_seconds(seconds).ok()?),
                "-" => Some(Offset::from_seconds(seconds.checked_neg()?).ok()?),
                _ => return None,
            }
        }
    };
    let time = Time::new(hour, minute, second, i32::try_from(nanos).ok()?).ok()?;
    Some(DateTime {
        text: s.to_string(),
        local: civil::DateTime::from_parts(parse_date(date)?, time),
        offset,
    })
}

/// The shortest text for a value: fractional digits only as far as they are non-zero.
fn format_date_time(local: civil::DateTime, offset: Option<Offset>) -> String {
    let (date, time) = (local.date(), local.time());
    let mut text = String::new();
    let _ = write!(
        text,
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        date.year(),
        date.month(),
        date.day(),
        time.hour(),
        time.minute(),
        time.second()
    );
    if time.subsec_nanosecond() != 0 {
        let fraction = alloc::format!("{:09}", time.subsec_nanosecond());
        let _ = write!(text, ".{}", fraction.trim_end_matches('0'));
    }
    if let Some(offset) = offset {
        let seconds = offset.seconds();
        let sign = if seconds < 0 { '-' } else { '+' };
        let seconds = seconds.unsigned_abs();
        // Seconds the grammar cannot write make the result fail to parse back.
        let rest = if seconds % 60 == 0 { "" } else { ":" };
        let _ = write!(
            text,
            "{sign}{:02}:{:02}{rest}",
            seconds / 3600,
            seconds % 3600 / 60
        );
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_times() {
        for ok in [
            "2026-04-17T11:49:23+02:00",
            "2026-04-17T11:49:23Z",
            "2026-04-17T11:49:23.5z",
            "2026-04-17T11:49:23.123456789123-12:45",
            "2026-04-17T11:49:23",
        ] {
            let parsed: DateTime = ok.parse().unwrap();
            assert_eq!(parsed.as_str(), ok);
        }
        for bad in [
            "2026-04-17T11:49:23Z\n",
            "2026-04-17 11:49:23Z",
            "2026-02-30T11:49:23Z",
            "2026-04-17T24:00:00Z",
            "2026-04-17T11:49:60Z",
            "2026-04-17T11:49:23.Z",
            "2026-04-17T11:49:23+2:00",
            "2026-04-17T11:49:23+24:00",
            "2026-04-17t11:49:23",
            "2026-04-17",
        ] {
            assert!(bad.parse::<DateTime>().is_err(), "{bad:?}");
        }
        let utc: DateTime = "2026-04-17T11:49:23+00:00".parse().unwrap();
        assert_eq!(utc.offset(), Some(Offset::UTC));
        let west: DateTime = "2026-04-17T11:49:23-03:30".parse().unwrap();
        assert_eq!(west.offset().map(Offset::seconds), Some(-12600));
        let local: DateTime = "2026-04-17T11:49:23".parse().unwrap();
        assert!(local.offset().is_none());
    }

    #[test]
    fn constructors_round_trip() {
        let local = civil::date(2026, 4, 17).at(11, 49, 23, 500_000_000);
        assert_eq!(
            DateTime::from_local(local).unwrap().as_str(),
            "2026-04-17T11:49:23.5"
        );
        let offset = Offset::from_seconds(-12600).unwrap();
        let value = DateTime::with_offset(local, offset).unwrap();
        assert_eq!(value.as_str(), "2026-04-17T11:49:23.5-03:30");
        assert_eq!(value.to_timestamp(), offset.to_timestamp(local).ok());
        assert!(DateTime::with_offset(local, Offset::from_seconds(30).unwrap()).is_err());
    }

    #[test]
    fn dates_and_uuids() {
        assert!("2024-02-29".parse::<Date>().is_ok());
        assert!("2023-02-29".parse::<Date>().is_err());
        assert!("2023-1-01".parse::<Date>().is_err());
        assert!(matches!("2024-02-29".parse(), Ok(StartedAt::Date(_))));
        let id = "0b0e6a3c-4c1e-4b8e-9b8e-1f2a3b4c5d6e";
        assert_eq!(id.parse::<Uuid>().unwrap().to_string(), id);
        assert!(id.to_uppercase().parse::<Uuid>().is_err());
        assert!(id.replace('-', "").parse::<Uuid>().is_err());
    }
}
