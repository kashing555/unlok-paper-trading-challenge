//! Exact fixed-point money. No float ever touches these values.
//!
//! **Scale.** All monetary quantities share one scale of four decimal places,
//! so a notional is `px.raw * qty` — an exact multiply with **no division and
//! no rounding anywhere in the core**. A cents scale would have forced
//! `px_raw * qty / 100`, and a price like `10.0050` × 1 share is 1000.5 cents:
//! representable only by rounding, on every fill, silently. Settlement rounding
//! to a real currency's minor unit is a production concern we do not model
//! (`docs/design.md` §16).
//!
//! **No `Add`/`Sub` impls, deliberately.** Arithmetic goes through the
//! `checked_*` methods so overflow is a `Result` the caller must handle rather
//! than a panic in release or a wrap in a ledger. Convenience here would be
//! paid for in a P&L that is wrong without saying so.

use std::fmt;

use crate::DomainError;

/// Fixed-point scale for every monetary quantity: four decimal places.
pub const SCALE: i64 = 10_000;
/// Number of decimal places implied by [`SCALE`].
pub const SCALE_DIGITS: usize = 4;

/// A signed amount of money: a cash balance, a notional, a fee, or a P&L.
///
/// Named `Money`, not `Cash`, because *cash* is one **use** of this type — the
/// participant's uninvested balance — not the type itself. A fee is not cash
/// and an unrealized P&L certainly is not, but all three are money.
///
/// Signed because P&L is, and because balance movements net in both directions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Money(i64);

/// A strictly positive price.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Px(i64);

/// A non-negative, whole number of shares. Unscaled — no fractional shares.
///
/// Zero is representable on purpose: an unfilled order has `filled == 0` and a
/// closed position has `qty == 0`. The stricter "an *order* must be for more
/// than zero shares" rule lives at the order constructor, not here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Qty(i64);

impl Money {
    pub const ZERO: Self = Self(0);

    /// Construct from raw scaled units. Any `i64` is a valid `Money`.
    pub const fn from_raw(raw: i64) -> Self {
        Self(raw)
    }

    pub const fn raw(self) -> i64 {
        self.0
    }

    pub fn parse(s: &str) -> Result<Self, DomainError> {
        parse_fixed("cash", s, true).map(Self)
    }

    pub fn checked_add(self, other: Self) -> Result<Self, DomainError> {
        self.0
            .checked_add(other.0)
            .map(Self)
            .ok_or(DomainError::Overflow("cash addition"))
    }

    pub fn checked_sub(self, other: Self) -> Result<Self, DomainError> {
        self.0
            .checked_sub(other.0)
            .map(Self)
            .ok_or(DomainError::Overflow("cash subtraction"))
    }
}

impl Px {
    /// Construct from raw scaled units, rejecting non-positive prices.
    pub fn from_raw(raw: i64) -> Result<Self, DomainError> {
        if raw > 0 {
            Ok(Self(raw))
        } else {
            Err(DomainError::NonPositivePx(raw))
        }
    }

    pub const fn raw(self) -> i64 {
        self.0
    }

    pub fn parse(s: &str) -> Result<Self, DomainError> {
        let raw = parse_fixed("price", s, false)?;
        Self::from_raw(raw)
    }

    /// `price × quantity`, exact.
    ///
    /// Both sides share [`SCALE`] and `Qty` is unscaled, so the product is
    /// already in `Money` raw units — no division, so no rounding.
    pub fn notional(self, qty: Qty) -> Result<Money, DomainError> {
        self.0
            .checked_mul(qty.0)
            .map(Money)
            .ok_or(DomainError::Overflow("notional"))
    }
}

impl Qty {
    pub const ZERO: Self = Self(0);

    pub fn new(shares: i64) -> Result<Self, DomainError> {
        if shares >= 0 {
            Ok(Self(shares))
        } else {
            Err(DomainError::NegativeQty(shares))
        }
    }

    pub const fn get(self) -> i64 {
        self.0
    }

    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }

    pub fn checked_add(self, other: Self) -> Result<Self, DomainError> {
        self.0
            .checked_add(other.0)
            .map(Self)
            .ok_or(DomainError::Overflow("quantity addition"))
    }

    /// Subtraction that cannot produce a short position — underflow past zero
    /// is a domain error, not a wrap.
    pub fn checked_sub(self, other: Self) -> Result<Self, DomainError> {
        let n = self
            .0
            .checked_sub(other.0)
            .ok_or(DomainError::Overflow("quantity subtraction"))?;
        Self::new(n)
    }
}

/// Parse a decimal string into raw scaled units.
///
/// **Strict on purpose** (`.claude/principles.md` §7 — Postel's law is rejected
/// on input paths): `1.23456` is an error rather than a silent truncation to
/// `1.2345`, `.5` and `1.` are errors rather than guesses, and whitespace
/// inside is an error. A price we had to interpret is a position we did not
/// mean to take.
fn parse_fixed(field: &'static str, s: &str, allow_negative: bool) -> Result<i64, DomainError> {
    let parse_err = || DomainError::ParseDecimal {
        field,
        input: s.to_owned(),
    };

    let body = s.trim();
    if body.is_empty() {
        return Err(parse_err());
    }

    let (negative, body) = match body.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, body.strip_prefix('+').unwrap_or(body)),
    };
    if negative && !allow_negative {
        return Err(DomainError::UnexpectedSign {
            field,
            input: s.to_owned(),
        });
    }

    let (int_part, frac_part) = match body.split_once('.') {
        // A trailing '.' with no digits after it ("1.") is ambiguous, and
        // `all(is_ascii_digit)` passes vacuously on an empty fraction — so it
        // has to be rejected here or it silently parses as 1.0000.
        Some((_, "")) => return Err(parse_err()),
        Some((int, frac)) => (int, frac),
        None => (body, ""),
    };

    // Both sides must be present and all-digits: rejects "", ".5", "1.", "1.2.3"
    // and "1 2" without any of them becoming a guess.
    if int_part.is_empty()
        || !int_part.bytes().all(|b| b.is_ascii_digit())
        || !frac_part.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(parse_err());
    }
    if frac_part.len() > SCALE_DIGITS {
        return Err(DomainError::TooManyDecimals {
            field,
            max: SCALE_DIGITS,
            input: s.to_owned(),
        });
    }

    let units: i64 = int_part.parse().map_err(|_| parse_err())?;
    let mut frac = frac_part.to_owned();
    while frac.len() < SCALE_DIGITS {
        frac.push('0');
    }
    let frac: i64 = frac.parse().map_err(|_| parse_err())?;

    let raw = units
        .checked_mul(SCALE)
        .and_then(|u| u.checked_add(frac))
        .ok_or(DomainError::Overflow("decimal parse"))?;

    Ok(if negative { -raw } else { raw })
}

/// Render raw scaled units as a decimal string with exactly [`SCALE_DIGITS`]
/// places. `unsigned_abs` rather than `abs` so `i64::MIN` formats instead of
/// panicking.
fn format_fixed(raw: i64) -> String {
    let sign = if raw < 0 { "-" } else { "" };
    let abs = raw.unsigned_abs();
    let scale = SCALE as u64;
    format!(
        "{sign}{}.{:0width$}",
        abs / scale,
        abs % scale,
        width = SCALE_DIGITS
    )
}

impl fmt::Display for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&format_fixed(self.0))
    }
}

impl fmt::Display for Px {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&format_fixed(self.0))
    }
}

impl fmt::Display for Qty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn parses_and_renders_with_full_precision() {
        assert_eq!(Money::parse("0").unwrap().raw(), 0);
        assert_eq!(Money::parse("1").unwrap().raw(), 10_000);
        assert_eq!(Money::parse("1.5").unwrap().raw(), 15_000);
        assert_eq!(Money::parse("10.0050").unwrap().raw(), 100_050);
        assert_eq!(Money::parse("-2.25").unwrap().raw(), -22_500);
        assert_eq!(Money::from_raw(100_050).to_string(), "10.0050");
        assert_eq!(Money::from_raw(-22_500).to_string(), "-2.2500");
        assert_eq!(Money::ZERO.to_string(), "0.0000");
    }

    #[test]
    fn rejects_excess_precision_rather_than_truncating() {
        // The Postel's-law rejection, made concrete: 1.23456 is not silently
        // 1.2345. A price we had to reinterpret is a position we did not mean.
        assert!(matches!(
            Money::parse("1.23456"),
            Err(DomainError::TooManyDecimals { max: 4, .. })
        ));
    }

    #[test]
    fn rejects_ambiguous_input_rather_than_guessing() {
        for bad in ["", "   ", ".5", "1.", "1.2.3", "abc", "1 2", "1,5", "--1"] {
            assert!(Money::parse(bad).is_err(), "should have rejected {bad:?}");
        }
    }

    #[test]
    fn price_and_quantity_reject_impossible_values() {
        assert!(matches!(
            Px::from_raw(0),
            Err(DomainError::NonPositivePx(0))
        ));
        assert!(matches!(
            Px::parse("-1"),
            Err(DomainError::UnexpectedSign { .. })
        ));
        assert!(matches!(Qty::new(-1), Err(DomainError::NegativeQty(-1))));
        // Zero is a legal Qty — an unfilled order and a closed position both
        // need it. The "an order must be for > 0" rule lives on the order.
        assert_eq!(Qty::new(0).unwrap(), Qty::ZERO);
    }

    #[test]
    fn notional_is_exact_at_sub_cent_prices() {
        // $10.0050 x 3 = $30.0150. On a cents scale this is 3001.5 cents and
        // could only be stored by rounding — on every fill, silently.
        let px = Px::parse("10.0050").unwrap();
        let notional = px.notional(Qty::new(3).unwrap()).unwrap();
        assert_eq!(notional, Money::parse("30.0150").unwrap());
        assert_eq!(notional.to_string(), "30.0150");
    }

    #[test]
    fn overflow_is_an_error_not_a_wrap() {
        let px = Px::from_raw(i64::MAX).unwrap();
        assert!(matches!(
            px.notional(Qty::new(2).unwrap()),
            Err(DomainError::Overflow("notional"))
        ));
        assert!(matches!(
            Money::from_raw(i64::MAX).checked_add(Money::from_raw(1)),
            Err(DomainError::Overflow("cash addition"))
        ));
    }

    #[test]
    fn quantity_cannot_go_short() {
        let held = Qty::new(100).unwrap();
        assert_eq!(held.checked_sub(Qty::new(100).unwrap()).unwrap(), Qty::ZERO);
        assert!(matches!(
            held.checked_sub(Qty::new(101).unwrap()),
            Err(DomainError::NegativeQty(-1))
        ));
    }

    #[test]
    fn extreme_values_format_without_panicking() {
        // unsigned_abs, not abs: -i64::MIN does not fit in an i64.
        assert!(Money::from_raw(i64::MIN).to_string().starts_with('-'));
    }

    proptest! {
        #[test]
        fn display_then_parse_round_trips(raw in (i64::MIN / 2)..(i64::MAX / 2)) {
            let cash = Money::from_raw(raw);
            prop_assert_eq!(Money::parse(&cash.to_string()).unwrap(), cash);
        }

        #[test]
        fn notional_matches_wider_arithmetic(
            px_raw in 1i64..1_000_000_000,
            shares in 0i64..1_000_000,
        ) {
            let px = Px::from_raw(px_raw).unwrap();
            let got = px.notional(Qty::new(shares).unwrap()).unwrap();
            // i128 reference: no scale juggling, so a scale bug shows up here.
            prop_assert_eq!(i128::from(got.raw()), i128::from(px_raw) * i128::from(shares));
        }
    }
}
