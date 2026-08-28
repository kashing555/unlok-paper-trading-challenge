//! Time as data.
//!
//! Nothing here reads a clock, and nothing in this crate can: `chrono` is
//! declared with `default-features = false`, which drops the `clock` feature
//! that provides `Utc::now()`. Timestamps arrive as values from the caller
//! (`.claude/principles.md` §6), which is what makes every test reproducible
//! and lets a competition day be closed at a boundary rather than "whenever
//! the job happened to run".

use std::fmt;

use chrono::NaiveDate;

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
        assert_eq!(
            TradingDay::parse("2026-08-29").unwrap().to_string(),
            "2026-08-29"
        );
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
    fn days_order_chronologically() {
        // The engine relies on this: days close in ascending order, and the
        // ladder compounds them in the order a BTreeMap yields.
        let mut days: Vec<_> = ["2027-01-01", "2026-08-28", "2026-12-31"]
            .iter()
            .map(|d| TradingDay::parse(d).unwrap())
            .collect();
        days.sort();
        let sorted: Vec<_> = days.iter().map(ToString::to_string).collect();
        assert_eq!(sorted, ["2026-08-28", "2026-12-31", "2027-01-01"]);
    }

    #[test]
    fn timestamps_are_values_not_readings() {
        let t = Timestamp::from_millis(1_772_000_000_000);
        assert_eq!(t.as_millis(), 1_772_000_000_000);
        assert!(Timestamp::from_millis(1) < Timestamp::from_millis(2));
    }
}
