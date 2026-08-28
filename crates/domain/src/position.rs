//! A long position and its cost basis.
//!
//! **Average cost is never stored.** The position holds `qty` and `cost` — the
//! total basis of what is held — and the average is derived only for display.
//! Storing a rounded average and re-multiplying it drifts, and the drift is
//! invisible until a reconciliation fails days later
//! (`.claude/code-style.md`).
//!
//! **Fees are capitalised on buy and expensed on sell.** One convention, both
//! sides. The trap is applying it on one side only, which leaks a fee per round
//! trip into unrealized P&L.

use thiserror::Error;

use crate::{DomainError, Money, Px, Qty, Symbol};

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PositionError {
    #[error("cannot sell {sell} of {symbol}: only {held} held")]
    InsufficientPosition {
        symbol: Symbol,
        held: i64,
        sell: i64,
    },

    #[error(transparent)]
    Domain(#[from] DomainError),
}

/// What a sell realized, so the caller can book it without recomputing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Realized {
    /// `proceeds − cost_removed − fee`.
    pub pnl: Money,
    /// Cash received: `proceeds − fee`.
    pub proceeds_net: Money,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Position {
    symbol: Symbol,
    qty: Qty,
    cost: Money,
}

impl Position {
    pub fn flat(symbol: Symbol) -> Self {
        Self {
            symbol,
            qty: Qty::ZERO,
            cost: Money::ZERO,
        }
    }

    pub fn symbol(&self) -> &Symbol {
        &self.symbol
    }

    pub const fn qty(&self) -> Qty {
        self.qty
    }

    /// Total basis of the held quantity, fees included.
    pub const fn cost(&self) -> Money {
        self.cost
    }

    pub const fn is_flat(&self) -> bool {
        self.qty.is_zero()
    }

    /// Average cost per share — **derived for display, never stored**.
    /// `None` when flat, because there is no average of nothing.
    pub fn avg_cost(&self) -> Option<Px> {
        if self.qty.is_zero() {
            return None;
        }
        Px::from_raw(self.cost.raw() / self.qty.get()).ok()
    }

    /// Buy: cash out is the caller's business, the basis is ours.
    pub fn buy(&mut self, qty: Qty, px: Px, fee: Money) -> Result<Money, PositionError> {
        let outlay = px.notional(qty)?.checked_add(fee)?;
        self.qty = self.qty.checked_add(qty)?;
        self.cost = self.cost.checked_add(outlay)?;
        Ok(outlay)
    }

    /// Sell, realizing against the running basis.
    ///
    /// Realized P&L books against the basis **at the moment of the sell**, not
    /// against an average recomputed later: a participant who buys 100 @ 10,
    /// sells 50 @ 12, then buys 100 @ 14 has realized a real +100, and
    /// re-deriving the basis afterwards would retroactively change a number
    /// already published on a closed day's leaderboard.
    pub fn sell(&mut self, qty: Qty, px: Px, fee: Money) -> Result<Realized, PositionError> {
        if qty > self.qty {
            return Err(PositionError::InsufficientPosition {
                symbol: self.symbol.clone(),
                held: self.qty.get(),
                sell: qty.get(),
            });
        }

        let removed = cost_removed(self.cost, qty, self.qty)?;
        let proceeds = px.notional(qty)?;
        let proceeds_net = proceeds.checked_sub(fee)?;
        let pnl = proceeds.checked_sub(removed)?.checked_sub(fee)?;

        self.qty = self.qty.checked_sub(qty)?;
        self.cost = self.cost.checked_sub(removed)?;

        // Closing the position must leave no basis behind. `cost_removed`
        // guarantees it (a final sale has sold == held, so the division is
        // exact), and this is the invariant that catches it if that ever stops
        // being true — loudly, per `principles.md` §6.
        debug_assert!(
            !self.qty.is_zero() || self.cost == Money::ZERO,
            "closed position left {} of basis behind",
            self.cost
        );

        Ok(Realized { pnl, proceeds_net })
    }

    /// Market value at a mark: `qty × mark`.
    pub fn market_value(&self, mark: Px) -> Result<Money, PositionError> {
        Ok(mark.notional(self.qty)?)
    }

    /// Unrealized P&L: `market value − basis`.
    ///
    /// Computed as `qty × mark − cost`, **not** `qty × (mark − avg)`. They are
    /// algebraically identical, but the second form needs the average and so
    /// needs a division; this one is exact.
    pub fn unrealized(&self, mark: Px) -> Result<Money, PositionError> {
        Ok(self.market_value(mark)?.checked_sub(self.cost)?)
    }
}

/// The share of basis leaving with a sale: `cost × sold / held`.
///
/// Widened to `i128` so the multiply cannot overflow before the divide.
/// Truncating, and **it does not accumulate residue**: the final sale of a
/// position has `sold == held`, so the division is exact and takes the whole
/// remaining basis with it. Intermediate sales may round by up to one raw unit,
/// which the close then absorbs.
fn cost_removed(cost: Money, sold: Qty, held: Qty) -> Result<Money, DomainError> {
    debug_assert!(!held.is_zero(), "cost_removed called on a flat position");
    let n = i128::from(cost.raw()) * i128::from(sold.get()) / i128::from(held.get());
    i64::try_from(n)
        .map(Money::from_raw)
        .map_err(|_| DomainError::Overflow("cost basis removal"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sym() -> Symbol {
        Symbol::parse("AAPL").unwrap()
    }
    fn qty(n: i64) -> Qty {
        Qty::new(n).unwrap()
    }
    fn px(s: &str) -> Px {
        Px::parse(s).unwrap()
    }
    fn money(s: &str) -> Money {
        Money::parse(s).unwrap()
    }

    #[test]
    fn buying_capitalises_the_fee_into_the_basis() {
        let mut p = Position::flat(sym());
        let outlay = p.buy(qty(100), px("10"), money("5")).unwrap();

        assert_eq!(outlay, money("1005"));
        assert_eq!(p.qty(), qty(100));
        // Basis is 1005, not 1000: the fee was part of what the shares cost.
        assert_eq!(p.cost(), money("1005"));
        assert_eq!(p.avg_cost(), Some(px("10.05")));
    }

    #[test]
    fn selling_expenses_the_fee_and_realizes_against_the_running_basis() {
        let mut p = Position::flat(sym());
        p.buy(qty(100), px("10"), money("0")).unwrap();

        let realized = p.sell(qty(40), px("12"), money("5")).unwrap();

        // proceeds 480, basis removed 400, fee 5 -> 75
        assert_eq!(realized.pnl, money("75"));
        assert_eq!(realized.proceeds_net, money("475"));
        assert_eq!(p.qty(), qty(60));
        assert_eq!(p.cost(), money("600"));
        // The average is untouched by a sale — only the quantity shrank.
        assert_eq!(p.avg_cost(), Some(px("10")));
    }

    #[test]
    fn selling_more_than_is_held_is_rejected() {
        // The long-only guarantee. Enforced here rather than by a check the
        // caller might forget.
        let mut p = Position::flat(sym());
        p.buy(qty(10), px("10"), Money::ZERO).unwrap();

        assert_eq!(
            p.sell(qty(11), px("12"), Money::ZERO),
            Err(PositionError::InsufficientPosition {
                symbol: sym(),
                held: 10,
                sell: 11
            })
        );
        // and nothing moved
        assert_eq!(p.qty(), qty(10));
    }

    #[test]
    fn closing_a_position_leaves_no_basis_behind() {
        // A basis that does not divide evenly: 31.0000 over 3 shares. Each
        // partial sale truncates, but the final sale has sold == held, so the
        // division is exact and takes the residue with it.
        let mut p = Position::flat(sym());
        p.buy(qty(3), px("10"), money("1")).unwrap();
        assert_eq!(p.cost(), money("31"));

        p.sell(qty(1), px("11"), Money::ZERO).unwrap();
        assert_eq!(p.cost(), money("20.6667")); // 31 - 10.3333, truncated

        p.sell(qty(2), px("11"), Money::ZERO).unwrap();
        assert!(p.is_flat());
        assert_eq!(p.cost(), Money::ZERO, "residue left in a closed position");
    }

    #[test]
    fn unrealized_is_market_value_less_basis() {
        let mut p = Position::flat(sym());
        p.buy(qty(100), px("10"), money("5")).unwrap();

        assert_eq!(p.market_value(px("12")).unwrap(), money("1200"));
        // 1200 - 1005: the fee is already in the basis, so it is not paid twice.
        assert_eq!(p.unrealized(px("12")).unwrap(), money("195"));
        assert_eq!(p.unrealized(px("10")).unwrap(), money("-5"));
    }

    #[test]
    fn a_flat_position_has_no_average() {
        assert_eq!(Position::flat(sym()).avg_cost(), None);
    }
}
