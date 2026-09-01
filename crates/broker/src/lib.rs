//! A mock broker that generates execution reports.
//!
//! **Deterministic by construction.** The RNG is seeded and owned; `thread_rng`
//! appears nowhere. `ChaCha8Rng` specifically, not `StdRng`: `StdRng` is
//! documented as free to change algorithm between `rand` releases, which would
//! silently break same-seed replay on an upgrade — ChaCha8's stream is pinned. The same seed and the same order sequence produce
//! byte-identical executions on every run and every machine, which is what
//! makes the tests downstream of it mean anything.
//!
//! **What it does not model,** deliberately: an order book, queue position,
//! price-time priority, or price improvement. Fills are marketable-limit at the
//! order's own limit price. Those are a different exercise and the brief does
//! not ask for them (`docs/design.md` §6).

#![forbid(unsafe_code)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

use domain::{BrokerOrderId, DomainError, Money, Order, OrderEvent, Px, Qty};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BrokerError {
    #[error("order is not working: nothing to execute")]
    NotWorking,

    #[error(transparent)]
    Domain(#[from] DomainError),
}

/// One execution report's worth of terms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Execution {
    pub qty: Qty,
    pub px: Px,
    pub fee: Money,
}

/// Commission in basis points of notional, rounded down.
///
/// Rounding down rather than to nearest is arbitrary but **stated**: the
/// alternative is a fee that depends on a rounding rule nobody wrote down.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FeeSchedule {
    pub bps: i64,
}

impl FeeSchedule {
    pub const FREE: Self = Self { bps: 0 };

    pub fn of(&self, notional: Money) -> Result<Money, DomainError> {
        if self.bps == 0 {
            return Ok(Money::ZERO);
        }
        let raw = i128::from(notional.raw()) * i128::from(self.bps) / 10_000;
        i64::try_from(raw)
            .map(Money::from_raw)
            .map_err(|_| DomainError::Overflow("fee"))
    }
}

/// How much of an order a single execution takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FillPolicy {
    /// One execution for the whole remaining quantity.
    Complete,
    /// Seeded partials: each execution takes a random slice of at least
    /// `1/max_slices` of the order's original quantity, so **any one order**
    /// completes in at most `max_slices` executions. Stateless — the bound
    /// comes from the order, so two orders worked at once cannot interfere.
    Partial { max_slices: u32 },
}

/// The port the engine depends on. Small on purpose: an engine that can name
/// every method of its broker is an engine coupled to this one.
pub trait Broker {
    /// Ack or reject a freshly submitted order.
    fn on_submit(&mut self, order: &Order) -> OrderEvent;

    /// The next execution for a working order, or `None` when it is done.
    fn next_execution(&mut self, order: &Order) -> Result<Option<Execution>, BrokerError>;

    /// The commission on a notional, for executions driven explicitly rather
    /// than generated here.
    fn fee_on(&self, notional: Money) -> Result<Money, DomainError>;
}

pub struct MockBroker {
    rng: ChaCha8Rng,
    next_id: u64,
    policy: FillPolicy,
    fees: FeeSchedule,
}

impl MockBroker {
    pub fn new(seed: u64, policy: FillPolicy, fees: FeeSchedule) -> Self {
        Self {
            rng: ChaCha8Rng::seed_from_u64(seed),
            next_id: 1,
            policy,
            fees,
        }
    }

    /// A broker that acks everything and fills completely, with no fees — the
    /// default for tests that are about something else.
    pub fn simple(seed: u64) -> Self {
        Self::new(seed, FillPolicy::Complete, FeeSchedule::FREE)
    }

    fn mint_id(&mut self) -> BrokerOrderId {
        let id = BrokerOrderId::new(self.next_id);
        self.next_id += 1;
        id
    }
}

impl Broker for MockBroker {
    fn on_submit(&mut self, order: &Order) -> OrderEvent {
        // Reference data moved to the engine's security master; by the time an
        // order reaches the broker it has already passed the venue's grids, so
        // the mock's only job here is to acknowledge with a minted id.
        let _ = order;
        OrderEvent::Acknowledged {
            broker_id: self.mint_id(),
        }
    }

    fn next_execution(&mut self, order: &Order) -> Result<Option<Execution>, BrokerError> {
        if order.state.is_terminal() {
            return Err(BrokerError::NotWorking);
        }

        let remaining = order.remaining()?;
        if remaining.is_zero() {
            return Ok(None);
        }

        let qty = match self.policy {
            FillPolicy::Complete => remaining,
            FillPolicy::Partial { max_slices } => {
                // Every execution takes at least 1/max_slices of the order's
                // **original** quantity, so it completes in at most
                // `max_slices` executions — a bound derived from the order
                // itself rather than from a counter on the broker.
                //
                // A counter here was a bug: it was shared across every order
                // the broker was working, so interleaving two orders let one
                // spend the other's budget and complete in a single fill.
                let slices = i64::from(max_slices.max(1));
                let chunk = (order.qty.get() + slices - 1) / slices; // ceil; both positive
                if remaining.get() <= chunk {
                    remaining
                } else {
                    Qty::new(self.rng.random_range(chunk..=remaining.get()))?
                }
            }
        };

        // Marketable limit: filled at the order's own price. No improvement is
        // modelled, so a fill price is never better than the participant asked
        // for and never worse.
        let px = order.limit_px;
        let fee = self.fees.of(px.notional(qty)?)?;
        Ok(Some(Execution { qty, px, fee }))
    }

    fn fee_on(&self, notional: Money) -> Result<Money, DomainError> {
        self.fees.of(notional)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::{ClientOrderId, NewOrder, ParticipantId, Side, Symbol, Timestamp};

    fn order(qty: i64, symbol: &str) -> Order {
        Order::submit(NewOrder {
            id: ClientOrderId::new(1),
            participant: ParticipantId::parse("alice").unwrap(),
            symbol: Symbol::parse(symbol).unwrap(),
            side: Side::Buy,
            qty: Qty::new(qty).unwrap(),
            limit_px: Px::parse("10").unwrap(),
            at: Timestamp::from_millis(0),
        })
        .unwrap()
    }

    fn acked(qty: i64, broker: &mut MockBroker) -> Order {
        let mut o = order(qty, "AAPL");
        let event = broker.on_submit(&o);
        o.apply(&event).unwrap();
        o
    }

    /// Drive an order to completion, returning the slice sizes.
    fn run_to_fill(broker: &mut MockBroker, qty: i64) -> Vec<i64> {
        let mut o = acked(qty, broker);
        let mut slices = Vec::new();
        while let Some(exec) = broker.next_execution(&o).unwrap() {
            slices.push(exec.qty.get());
            o.apply(&OrderEvent::Fill {
                qty: exec.qty,
                px: exec.px,
            })
            .unwrap();
            if o.state.is_terminal() {
                break;
            }
        }
        slices
    }

    #[test]
    fn the_same_seed_produces_the_same_executions() {
        let policy = FillPolicy::Partial { max_slices: 4 };
        let a = run_to_fill(&mut MockBroker::new(42, policy, FeeSchedule::FREE), 100);
        let b = run_to_fill(&mut MockBroker::new(42, policy, FeeSchedule::FREE), 100);
        assert_eq!(a, b, "same seed must replay identically");
        assert!(
            a.len() > 1,
            "partial policy should produce more than one fill"
        );
    }

    #[test]
    fn a_different_seed_produces_different_executions() {
        let policy = FillPolicy::Partial { max_slices: 4 };
        let a = run_to_fill(&mut MockBroker::new(1, policy, FeeSchedule::FREE), 1000);
        let b = run_to_fill(&mut MockBroker::new(2, policy, FeeSchedule::FREE), 1000);
        assert_ne!(a, b);
    }

    #[test]
    fn partials_sum_exactly_to_the_ordered_quantity() {
        // No rounding residue: an order that is 100 filled 4 ways is still 100.
        for seed in 0..25 {
            let slices = run_to_fill(
                &mut MockBroker::new(
                    seed,
                    FillPolicy::Partial { max_slices: 5 },
                    FeeSchedule::FREE,
                ),
                997,
            );
            assert_eq!(slices.iter().sum::<i64>(), 997, "seed {seed}");
        }
    }

    #[test]
    fn a_complete_policy_fills_in_one_execution() {
        assert_eq!(run_to_fill(&mut MockBroker::simple(0), 100), vec![100]);
    }

    #[test]
    fn broker_ids_are_minted_once_each() {
        let mut broker = MockBroker::simple(0);
        let ids: Vec<_> = (0..3)
            .map(|_| match broker.on_submit(&order(1, "AAPL")) {
                OrderEvent::Acknowledged { broker_id } => broker_id.get(),
                other => panic!("expected an ack, got {other:?}"),
            })
            .collect();
        assert_eq!(ids, vec![1, 2, 3]);
    }

    #[test]
    fn fees_are_basis_points_of_notional() {
        let fees = FeeSchedule { bps: 10 }; // 10bp
        assert_eq!(
            fees.of(Money::parse("1000").unwrap()).unwrap(),
            Money::parse("1").unwrap()
        );
        assert_eq!(fees.of(Money::ZERO).unwrap(), Money::ZERO);
        assert_eq!(
            FeeSchedule::FREE.of(Money::parse("1000").unwrap()).unwrap(),
            Money::ZERO
        );
    }

    #[test]
    fn executing_a_terminal_order_is_an_error() {
        let mut broker = MockBroker::simple(0);
        let mut o = acked(10, &mut broker);
        o.apply(&OrderEvent::Cancelled).unwrap();
        assert_eq!(broker.next_execution(&o), Err(BrokerError::NotWorking));
    }
}

#[cfg(test)]
mod slicing_tests {
    use super::*;
    use domain::{ClientOrderId, NewOrder, ParticipantId, Side, Symbol, Timestamp};

    fn working(id: u64, qty: i64, broker: &mut MockBroker) -> Order {
        let mut o = Order::submit(NewOrder {
            id: ClientOrderId::new(id),
            participant: ParticipantId::parse("alice").unwrap(),
            symbol: Symbol::parse("AAPL").unwrap(),
            side: Side::Buy,
            qty: Qty::new(qty).unwrap(),
            limit_px: Px::parse("10").unwrap(),
            at: Timestamp::from_millis(0),
        })
        .unwrap();
        let ack = broker.on_submit(&o);
        o.apply(&ack).unwrap();
        o
    }

    /// Two orders worked at the same time must not share a slice budget.
    ///
    /// The policy is documented as "at most `max_slices` executions **per
    /// order**". A counter living on the broker instead of the order makes it
    /// "per broker", so interleaving two orders lets one exhaust the other's
    /// budget and complete in a single fill.
    #[test]
    fn interleaved_orders_do_not_share_a_slice_budget() {
        let mut broker =
            MockBroker::new(7, FillPolicy::Partial { max_slices: 3 }, FeeSchedule::FREE);

        let mut a = working(1, 1000, &mut broker);
        let mut b = working(2, 1000, &mut broker);

        // Work A twice, then start B. B is on its *first* execution and must
        // still be sliced, not completed outright.
        for _ in 0..2 {
            let e = broker.next_execution(&a).unwrap().unwrap();
            a.apply(&OrderEvent::Fill {
                qty: e.qty,
                px: e.px,
            })
            .unwrap();
        }

        let first_b = broker.next_execution(&b).unwrap().unwrap();
        b.apply(&OrderEvent::Fill {
            qty: first_b.qty,
            px: first_b.px,
        })
        .unwrap();

        assert!(
            !b.state.is_terminal(),
            "B completed on its first execution because A had spent the budget"
        );
    }

    /// The bound the policy promises, checked per order across many seeds.
    #[test]
    fn an_order_never_takes_more_than_max_slices_executions() {
        for seed in 0..40 {
            for max_slices in 1..=5 {
                let mut broker =
                    MockBroker::new(seed, FillPolicy::Partial { max_slices }, FeeSchedule::FREE);
                let mut o = working(1, 997, &mut broker);
                let mut fills = 0;
                while !o.state.is_terminal() {
                    let e = broker.next_execution(&o).unwrap().unwrap();
                    o.apply(&OrderEvent::Fill {
                        qty: e.qty,
                        px: e.px,
                    })
                    .unwrap();
                    fills += 1;
                    assert!(
                        fills <= max_slices,
                        "seed {seed} max {max_slices}: {fills} fills"
                    );
                }
                assert_eq!(o.state.filled(), Qty::new(997).unwrap());
            }
        }
    }
}
