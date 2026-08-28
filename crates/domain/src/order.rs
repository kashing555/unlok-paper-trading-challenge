//! The order itself: what was asked for, and where it has got to.
//!
//! Plain data plus total functions. The state machine lives in
//! [`crate::lifecycle`]; this is the record it advances.

use crate::{
    lifecycle::{self, OrderEvent, OrderState, TransitionError},
    ClientOrderId, DomainError, ParticipantId, Px, Qty, Symbol, Timestamp,
};

/// Long-only, so `Sell` may only reduce an existing position — enforced by the
/// portfolio, which is the only thing that knows what is held.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Buy,
    Sell,
}

/// A submission request, before it becomes an [`Order`].
///
/// Separate from `Order` so that the invariants checked at submission live in
/// one fallible constructor rather than being re-checked by every caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewOrder {
    pub id: ClientOrderId,
    pub participant: ParticipantId,
    pub symbol: Symbol,
    pub side: Side,
    pub qty: Qty,
    pub limit_px: Px,
    pub at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Order {
    pub id: ClientOrderId,
    pub participant: ParticipantId,
    pub symbol: Symbol,
    pub side: Side,
    /// The quantity asked for. Never changes — a modification produces a *new*
    /// order (see [`Order::replace`]), so this stays the terms the broker was
    /// actually working.
    pub qty: Qty,
    pub limit_px: Px,
    pub state: OrderState,
    /// Set when this order came from replacing another. FIX `OrigClOrdID`.
    pub replaces: Option<ClientOrderId>,
    pub submitted_at: Timestamp,
}

/// The result of a cancel-replace: both sides, so a caller cannot record one
/// without the other.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replacement {
    pub original: Order,
    pub replacement: Order,
}

impl Order {
    /// Submit an order.
    ///
    /// Zero quantity is rejected **here** rather than in `Qty`: zero is a
    /// legitimate quantity elsewhere (an unfilled order has `filled == 0`), but
    /// an order *for* nothing is meaningless. The rule belongs where it is true.
    pub fn submit(req: NewOrder) -> Result<Self, DomainError> {
        if req.qty.is_zero() {
            return Err(DomainError::NonPositiveQty(0));
        }
        Ok(Self {
            id: req.id,
            participant: req.participant,
            symbol: req.symbol,
            side: req.side,
            qty: req.qty,
            limit_px: req.limit_px,
            state: OrderState::New,
            replaces: None,
            submitted_at: req.at,
        })
    }

    /// Advance the order by one event. A thin shell over the pure transition
    /// function, which is where the logic and the tests live.
    pub fn apply(&mut self, event: &OrderEvent) -> Result<(), TransitionError> {
        self.state = lifecycle::apply(&self.state, self.qty, event)?;
        Ok(())
    }

    pub const fn remaining(&self) -> Result<Qty, DomainError> {
        self.qty.checked_sub_const(self.state.filled())
    }

    /// Cancel-replace, not in-place mutation (`docs/design.md` §5).
    ///
    /// The original is cancelled — keeping whatever it already filled — and a
    /// new order is minted pointing back at it. Three consequences, all
    /// deliberate:
    ///
    /// - **Filled quantity is never rewritten.** Replacing an order that is
    ///   40/100 filled withdraws the residual 60; the 40 stays booked against
    ///   the original.
    /// - **A replace can lose the race.** If the order completed first,
    ///   `FILLED` is terminal and this returns `Illegal` carrying that state,
    ///   rather than silently creating an unwanted second order.
    /// - **The chain is preserved**, because a leaderboard is downstream of it.
    pub fn replace(
        &self,
        id: ClientOrderId,
        qty: Qty,
        limit_px: Px,
        at: Timestamp,
    ) -> Result<Replacement, TransitionError> {
        let cancelled = lifecycle::apply(&self.state, self.qty, &OrderEvent::Cancelled)?;

        let replacement = Order::submit(NewOrder {
            id,
            participant: self.participant.clone(),
            symbol: self.symbol.clone(),
            side: self.side,
            qty,
            limit_px,
            at,
        })?;

        Ok(Replacement {
            original: Order {
                state: cancelled,
                ..self.clone()
            },
            replacement: Order {
                replaces: Some(self.id),
                ..replacement
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{lifecycle::RejectReason, BrokerOrderId, Money};

    fn req(id: u64, qty: i64) -> NewOrder {
        NewOrder {
            id: ClientOrderId::new(id),
            participant: ParticipantId::parse("alice").unwrap(),
            symbol: Symbol::parse("AAPL").unwrap(),
            side: Side::Buy,
            qty: Qty::new(qty).unwrap(),
            limit_px: Px::parse("10").unwrap(),
            at: Timestamp::from_millis(1_000),
        }
    }

    fn acked(qty: i64) -> Order {
        let mut order = Order::submit(req(1, qty)).unwrap();
        order
            .apply(&OrderEvent::Acknowledged {
                broker_id: BrokerOrderId::new(7),
            })
            .unwrap();
        order
    }

    #[test]
    fn submission_starts_new_and_unlinked() {
        let order = Order::submit(req(1, 100)).unwrap();
        assert_eq!(order.state, OrderState::New);
        assert_eq!(order.replaces, None);
        assert_eq!(order.remaining().unwrap(), Qty::new(100).unwrap());
    }

    #[test]
    fn an_order_for_zero_shares_is_rejected() {
        // Zero is a legal Qty — an unfilled order has filled == 0 — but an
        // order *for* nothing is meaningless, so the rule lives here.
        let mut r = req(1, 100);
        r.qty = Qty::ZERO;
        assert_eq!(Order::submit(r), Err(DomainError::NonPositiveQty(0)));
    }

    #[test]
    fn remaining_shrinks_as_fills_land() {
        let mut order = acked(100);
        order
            .apply(&OrderEvent::Fill {
                qty: Qty::new(40).unwrap(),
                px: Px::parse("10").unwrap(),
            })
            .unwrap();
        assert_eq!(order.remaining().unwrap(), Qty::new(60).unwrap());
    }

    #[test]
    fn replace_withdraws_the_residual_and_preserves_the_fill() {
        let mut order = acked(100);
        order
            .apply(&OrderEvent::Fill {
                qty: Qty::new(40).unwrap(),
                px: Px::parse("10").unwrap(),
            })
            .unwrap();

        let out = order
            .replace(
                ClientOrderId::new(2),
                Qty::new(200).unwrap(),
                Px::parse("11").unwrap(),
                Timestamp::from_millis(2_000),
            )
            .unwrap();

        // The 40 that executed stays booked against the original order. Only
        // the untraded residual is withdrawn.
        assert_eq!(out.original.state.filled(), Qty::new(40).unwrap());
        assert_eq!(out.original.state.cost(), Money::parse("400").unwrap());
        assert_eq!(out.original.state.name(), "CANCELLED");

        // The replacement is a fresh order at the new terms, chained back.
        assert_eq!(out.replacement.state, OrderState::New);
        assert_eq!(out.replacement.qty, Qty::new(200).unwrap());
        assert_eq!(out.replacement.limit_px, Px::parse("11").unwrap());
        assert_eq!(out.replacement.replaces, Some(ClientOrderId::new(1)));
        assert_eq!(out.replacement.participant, out.original.participant);
    }

    #[test]
    fn replace_loses_the_race_to_a_completed_fill() {
        let mut order = acked(100);
        order
            .apply(&OrderEvent::Fill {
                qty: Qty::new(100).unwrap(),
                px: Px::parse("10").unwrap(),
            })
            .unwrap();

        // FILLED is terminal, so the replace fails rather than quietly opening
        // a second position the participant never asked for. The error names
        // the state so the caller can decide what to do about it.
        assert_eq!(
            order.replace(
                ClientOrderId::new(2),
                Qty::new(200).unwrap(),
                Px::parse("11").unwrap(),
                Timestamp::from_millis(2_000),
            ),
            Err(TransitionError::Illegal {
                state: "FILLED",
                event: "a cancellation"
            })
        );
    }

    #[test]
    fn a_rejected_order_cannot_be_replaced() {
        let mut order = Order::submit(req(1, 100)).unwrap();
        order
            .apply(&OrderEvent::Rejected {
                reason: RejectReason::InsufficientCash,
            })
            .unwrap();
        assert!(order
            .replace(
                ClientOrderId::new(2),
                Qty::new(50).unwrap(),
                Px::parse("11").unwrap(),
                Timestamp::from_millis(2_000)
            )
            .is_err());
    }

    #[test]
    fn replacing_an_unacked_order_is_allowed() {
        // Same window as cancel-before-ack: our id exists, the broker's does not.
        let order = Order::submit(req(1, 100)).unwrap();
        let out = order
            .replace(
                ClientOrderId::new(2),
                Qty::new(50).unwrap(),
                Px::parse("11").unwrap(),
                Timestamp::from_millis(2_000),
            )
            .unwrap();
        assert_eq!(out.original.state.name(), "CANCELLED");
        assert_eq!(out.replacement.qty, Qty::new(50).unwrap());
    }
}
