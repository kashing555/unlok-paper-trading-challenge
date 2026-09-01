//! A participant's book: cash, positions, and the P&L split the brief asks for.
//!
//! The portfolio is a **fold over fills**, not a record kept in step with them
//! (`CLAUDE.md` rule 1). Nothing here is cached that the fills do not already
//! determine.
//!
//! Positions live in a `BTreeMap`, not a `HashMap`, so iteration order is a
//! property of the data rather than of a hash seed. Determinism is a
//! requirement of the brief and this is one of the places it would otherwise
//! leak away.

use std::collections::BTreeMap;

use thiserror::Error;

use crate::{
    position::{Position, PositionError},
    DomainError, Money, ParticipantId, Px, Qty, Side, Symbol,
};

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PortfolioError {
    #[error("insufficient cash: need {need}, have {have}")]
    InsufficientCash { need: Money, have: Money },

    #[error("no mark for {symbol}: refusing to value a position at a price we do not have")]
    MissingMark { symbol: Symbol },

    #[error(transparent)]
    Position(#[from] PositionError),

    #[error(transparent)]
    Domain(#[from] DomainError),
}

/// One execution, as the portfolio sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fill {
    pub symbol: Symbol,
    pub side: Side,
    pub qty: Qty,
    pub px: Px,
    pub fee: Money,
}

/// The prices positions are valued at. Supplied by the caller; nothing here
/// invents one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Marks(BTreeMap<Symbol, Px>);

impl Marks {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&mut self, symbol: Symbol, px: Px) {
        self.0.insert(symbol, px);
    }

    pub fn get(&self, symbol: &Symbol) -> Option<Px> {
        self.0.get(symbol).copied()
    }

    /// Every current mark, in symbol order (`BTreeMap`, so deterministic).
    pub fn iter(&self) -> impl Iterator<Item = (&Symbol, Px)> {
        self.0.iter().map(|(s, px)| (s, *px))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Portfolio {
    participant: ParticipantId,
    starting_cash: Money,
    cash: Money,
    realized_pnl: Money,
    /// Every fee ever charged, both sides. Reported separately because the
    /// record keeps price and fee apart (as FIX does) even though the basis
    /// convention capitalises buys — fees also being *in* the basis does not
    /// excuse them from being *visible*.
    fees_paid: Money,
    positions: BTreeMap<Symbol, Position>,
}

impl Portfolio {
    pub fn open(participant: ParticipantId, starting_cash: Money) -> Self {
        Self {
            participant,
            starting_cash,
            cash: starting_cash,
            realized_pnl: Money::ZERO,
            fees_paid: Money::ZERO,
            positions: BTreeMap::new(),
        }
    }

    pub fn participant(&self) -> &ParticipantId {
        &self.participant
    }

    pub const fn starting_cash(&self) -> Money {
        self.starting_cash
    }

    /// The brief's "cash balance".
    pub const fn cash(&self) -> Money {
        self.cash
    }

    /// The brief's "realized P&L" — booked at each sell, never re-derived.
    pub const fn realized_pnl(&self) -> Money {
        self.realized_pnl
    }

    /// Total fees charged across every fill, both sides.
    pub const fn fees_paid(&self) -> Money {
        self.fees_paid
    }

    /// The brief's "current positions": held ones only. A symbol traded and
    /// closed is history, and history lives in the event log.
    pub fn positions(&self) -> impl Iterator<Item = &Position> {
        self.positions.values().filter(|p| !p.is_flat())
    }

    pub fn position(&self, symbol: &Symbol) -> Option<&Position> {
        self.positions.get(symbol).filter(|p| !p.is_flat())
    }

    /// Book one fill.
    ///
    /// Validated **before** anything is mutated, so a rejected fill leaves the
    /// portfolio exactly as it was. A half-applied fill would be a position
    /// that disagrees with the cash that paid for it — the divergence this
    /// whole design exists to prevent.
    pub fn apply(&mut self, fill: &Fill) -> Result<(), PortfolioError> {
        match fill.side {
            Side::Buy => {
                let outlay = fill.px.notional(fill.qty)?.checked_add(fill.fee)?;
                if outlay > self.cash {
                    // Reachable only if the pre-trade check upstream failed to
                    // do its job. Loud, not silent: booking it would leave a
                    // negative balance that no later read could explain.
                    return Err(PortfolioError::InsufficientCash {
                        need: outlay,
                        have: self.cash,
                    });
                }

                let position = self
                    .positions
                    .entry(fill.symbol.clone())
                    .or_insert_with(|| Position::flat(fill.symbol.clone()));
                position.buy(fill.qty, fill.px, fill.fee)?;
                self.cash = self.cash.checked_sub(outlay)?;
                self.fees_paid = self.fees_paid.checked_add(fill.fee)?;
            }

            Side::Sell => {
                let position = self.positions.get_mut(&fill.symbol).ok_or_else(|| {
                    PositionError::InsufficientPosition {
                        symbol: fill.symbol.clone(),
                        held: 0,
                        sell: fill.qty.get(),
                    }
                })?;

                let realized = position.sell(fill.qty, fill.px, fill.fee)?;
                self.cash = self.cash.checked_add(realized.proceeds_net)?;
                self.realized_pnl = self.realized_pnl.checked_add(realized.pnl)?;
                self.fees_paid = self.fees_paid.checked_add(fill.fee)?;
            }
        }
        Ok(())
    }

    /// The brief's "unrealized P&L", summed across held positions.
    ///
    /// **Fails closed on a missing mark.** A held symbol with no price is an
    /// error, never a silent zero: a wrong portfolio value corrupts a
    /// leaderboard that is then immutable (`docs/design.md` §8).
    pub fn unrealized_pnl(&self, marks: &Marks) -> Result<Money, PortfolioError> {
        self.fold_positions(marks, Position::unrealized)
    }

    /// Aggregate market value of held positions.
    pub fn market_value(&self, marks: &Marks) -> Result<Money, PortfolioError> {
        self.fold_positions(marks, Position::market_value)
    }

    /// The brief's "total portfolio value": `cash + Σ(qty × mark)`.
    pub fn total_value(&self, marks: &Marks) -> Result<Money, PortfolioError> {
        Ok(self.cash.checked_add(self.market_value(marks)?)?)
    }

    fn fold_positions(
        &self,
        marks: &Marks,
        f: impl Fn(&Position, Px) -> Result<Money, PositionError>,
    ) -> Result<Money, PortfolioError> {
        let mut total = Money::ZERO;
        for position in self.positions() {
            let mark = marks
                .get(position.symbol())
                .ok_or_else(|| PortfolioError::MissingMark {
                    symbol: position.symbol().clone(),
                })?;
            total = total.checked_add(f(position, mark)?)?;
        }
        Ok(total)
    }
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

    fn book(side: Side, n: i64, price: &str, fee: &str) -> Fill {
        Fill {
            symbol: sym(),
            side,
            qty: qty(n),
            px: px(price),
            fee: money(fee),
        }
    }

    fn open() -> Portfolio {
        Portfolio::open(ParticipantId::parse("alice").unwrap(), money("100000"))
    }

    fn marks_at(price: &str) -> Marks {
        let mut m = Marks::new();
        m.set(sym(), px(price));
        m
    }

    /// The A2 close condition: average cost across buy → buy → sell → buy with
    /// fees on both sides, hand-computed, with realized and unrealized split.
    #[test]
    fn a_full_trading_sequence_matches_hand_computed_figures() {
        let mut p = open();

        // Buy 100 @ 10.00, fee 5.00
        p.apply(&book(Side::Buy, 100, "10", "5")).unwrap();
        assert_eq!(p.cash(), money("98995")); // 100000 - 1000 - 5
        assert_eq!(p.position(&sym()).unwrap().cost(), money("1005"));

        // Buy 100 @ 12.00, fee 5.00 -> avg (1005 + 1205) / 200 = 11.05
        p.apply(&book(Side::Buy, 100, "12", "5")).unwrap();
        assert_eq!(p.cash(), money("97790"));
        assert_eq!(p.position(&sym()).unwrap().cost(), money("2210"));
        assert_eq!(p.position(&sym()).unwrap().avg_cost(), Some(px("11.05")));

        // Sell 50 @ 15.00, fee 5.00
        //   basis out = 2210 x 50/200 = 552.50
        //   realized  = 750 - 552.50 - 5 = 192.50
        p.apply(&book(Side::Sell, 50, "15", "5")).unwrap();
        assert_eq!(p.realized_pnl(), money("192.50"));
        assert_eq!(p.cash(), money("98535")); // 97790 + 750 - 5
        assert_eq!(p.position(&sym()).unwrap().cost(), money("1657.50"));
        // The average survives the sale unchanged — the sale removed basis and
        // quantity in the same proportion.
        assert_eq!(p.position(&sym()).unwrap().avg_cost(), Some(px("11.05")));

        // Buy 100 @ 8.00, fee 5.00 -> avg (1657.50 + 805) / 250 = 9.85
        p.apply(&book(Side::Buy, 100, "8", "5")).unwrap();
        assert_eq!(p.cash(), money("97730"));
        assert_eq!(p.position(&sym()).unwrap().qty(), qty(250));
        assert_eq!(p.position(&sym()).unwrap().avg_cost(), Some(px("9.85")));

        // Mark at 10.00: value 2500, basis 2462.50
        let marks = marks_at("10");
        assert_eq!(p.market_value(&marks).unwrap(), money("2500"));
        assert_eq!(p.fees_paid(), money("20"));
        assert_eq!(p.unrealized_pnl(&marks).unwrap(), money("37.50"));
        assert_eq!(p.total_value(&marks).unwrap(), money("100230"));
    }

    /// The identity that catches almost any accounting slip: whatever the
    /// route, the book has to add up.
    #[test]
    fn total_value_equals_starting_cash_plus_realized_plus_unrealized() {
        let mut p = open();
        for fill in [
            book(Side::Buy, 100, "10", "5"),
            book(Side::Buy, 100, "12", "5"),
            book(Side::Sell, 50, "15", "5"),
            book(Side::Buy, 100, "8", "5"),
        ] {
            p.apply(&fill).unwrap();
        }

        let marks = marks_at("10");
        let expected = p
            .starting_cash()
            .checked_add(p.realized_pnl())
            .unwrap()
            .checked_add(p.unrealized_pnl(&marks).unwrap())
            .unwrap();

        assert_eq!(p.total_value(&marks).unwrap(), expected);
    }

    #[test]
    fn a_held_symbol_with_no_mark_is_an_error_not_a_zero() {
        // Fail closed. A wrong portfolio value corrupts a leaderboard that is
        // then immutable, so refusing to value is the cheaper failure.
        let mut p = open();
        p.apply(&book(Side::Buy, 100, "10", "0")).unwrap();

        assert_eq!(
            p.total_value(&Marks::new()),
            Err(PortfolioError::MissingMark { symbol: sym() })
        );
        assert_eq!(
            p.unrealized_pnl(&Marks::new()),
            Err(PortfolioError::MissingMark { symbol: sym() })
        );
    }

    #[test]
    fn an_empty_book_values_at_its_cash_with_no_marks_needed() {
        let p = open();
        assert_eq!(p.total_value(&Marks::new()).unwrap(), money("100000"));
        assert_eq!(p.positions().count(), 0);
    }

    #[test]
    fn selling_what_is_not_held_is_rejected_and_changes_nothing() {
        let mut p = open();
        let before = p.clone();

        assert!(matches!(
            p.apply(&book(Side::Sell, 1, "10", "0")),
            Err(PortfolioError::Position(
                PositionError::InsufficientPosition { .. }
            ))
        ));
        assert_eq!(p, before, "a rejected fill must leave the book untouched");
    }

    #[test]
    fn a_buy_beyond_available_cash_is_rejected_and_changes_nothing() {
        let mut p = Portfolio::open(ParticipantId::parse("bob").unwrap(), money("100"));
        let before = p.clone();

        assert_eq!(
            p.apply(&book(Side::Buy, 100, "10", "0")),
            Err(PortfolioError::InsufficientCash {
                need: money("1000"),
                have: money("100")
            })
        );
        // Validated before anything mutates: no position, no cash movement.
        assert_eq!(p, before);
    }

    #[test]
    fn a_closed_position_stops_being_a_current_position() {
        let mut p = open();
        p.apply(&book(Side::Buy, 100, "10", "0")).unwrap();
        assert_eq!(p.positions().count(), 1);

        p.apply(&book(Side::Sell, 100, "11", "0")).unwrap();
        assert_eq!(p.positions().count(), 0);
        assert_eq!(p.position(&sym()), None);
        assert_eq!(p.realized_pnl(), money("100"));
        // Closed and flat, so no mark is needed to value the book.
        assert_eq!(p.total_value(&Marks::new()).unwrap(), money("100100"));
    }

    #[test]
    fn positions_iterate_in_a_deterministic_order() {
        // BTreeMap, not HashMap: the order is a property of the symbols, not of
        // a hash seed that varies between runs.
        let mut p = open();
        for s in ["MSFT", "AAPL", "TSLA"] {
            p.apply(&Fill {
                symbol: Symbol::parse(s).unwrap(),
                side: Side::Buy,
                qty: qty(1),
                px: px("10"),
                fee: Money::ZERO,
            })
            .unwrap();
        }
        let order: Vec<_> = p.positions().map(|x| x.symbol().as_str()).collect();
        assert_eq!(order, ["AAPL", "MSFT", "TSLA"]);
    }
}
