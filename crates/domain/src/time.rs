//! Time as data.
//!
//! Nothing here reads a clock, and nothing in this crate can: `chrono` is
//! declared with `default-features = false`, which drops the `clock` feature
//! that provides `Utc::now()`. Timestamps arrive as values from the caller
//! (`.claude/principles.md` §6), which is what makes every test reproducible
//! and lets a competition day be closed at a boundary rather than "whenever
//! the job happened to run".

use std::fmt;

use chrono::{Datelike, NaiveDate};

use crate::DomainError;

/// A point in time, milliseconds since the Unix epoch, supplied by the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(i64);

/// A competition day. The key daily results and leaderboards are filed under.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TradingDay(NaiveDate);

impl Timestamp {
    pub const fn from_millis(millis: i64) -> Self {
        Self(millis)
    }

    pub const fn as_millis(self) -> i64 {
        self.0
    }
}

impl TradingDay {
    /// Parses strict ISO-8601 `YYYY-MM-DD`.
    ///
    /// The explicit length check rejects `2026-8-1`, which `parse_from_str`
    /// would otherwise accept — two spellings of one day would key two
    /// leaderboards for the same date (see the note in `ids.rs`).
    pub fn parse(s: &str) -> Result<Self, DomainError> {
        if s.len() != 10 {
            return Err(DomainError::ParseDate(s.to_owned()));
        }
        NaiveDate::parse_from_str(s, "%Y-%m-%d")
            .map(Self)
            .map_err(|_| DomainError::ParseDate(s.to_owned()))
    }

    pub fn from_ymd(year: i32, month: u32, day: u32) -> Result<Self, DomainError> {
        NaiveDate::from_ymd_opt(year, month, day)
            .map(Self)
            .ok_or_else(|| DomainError::ParseDate(format!("{year:04}-{month:02}-{day:02}")))
    }

    /// The next calendar day. Not the next *trading* day — an exchange calendar
    /// is a production concern (`docs/design.md` §16); here a competition day is
    /// whatever the operator closes.
    pub fn succ(self) -> Result<Self, DomainError> {
        self.0
            .succ_opt()
            .map(Self)
            .ok_or_else(|| DomainError::ParseDate(self.to_string()))
    }

    pub fn year(self) -> i32 {
        self.0.year()
    }
}

impl fmt::Display for TradingDay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.format("%Y-%m-%d"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_renders_iso_dates() {
        let day = TradingDay::parse("2026-08-29").unwrap();
        assert_eq!(day.to_string(), "2026-08-29");
        assert_eq!(day.year(), 2026);
    }

    #[test]
    fn rejects_non_canonical_or_impossible_dates() {
        for bad in [
            "2026-8-1",
            "2026-08-29T00:00:00",
            "29-08-2026",
            "2026-02-30",
            "",
        ] {
            assert!(
                TradingDay::parse(bad).is_err(),
                "should have rejected {bad:?}"
            );
        }
        assert!(TradingDay::from_ymd(2026, 13, 1).is_err());
        // 2026 is not a leap year; 2024 was.
        assert!(TradingDay::from_ymd(2026, 2, 29).is_err());
        assert!(TradingDay::from_ymd(2024, 2, 29).is_ok());
    }

    #[test]
    fn days_order_and_advance_across_boundaries() {
        let dec31 = TradingDay::parse("2026-12-31").unwrap();
        assert_eq!(dec31.succ().unwrap().to_string(), "2027-01-01");
        assert!(TradingDay::parse("2026-08-28").unwrap() < dec31);
    }

    #[test]
    fn timestamps_are_values_not_readings() {
        let t = Timestamp::from_millis(1_772_000_000_000);
        assert_eq!(t.as_millis(), 1_772_000_000_000);
        assert!(Timestamp::from_millis(1) < Timestamp::from_millis(2));
    }
}
