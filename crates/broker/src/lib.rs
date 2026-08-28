//! A mock broker that generates execution reports.
//!
//! **Deterministic by construction.** The RNG is seeded and owned; `thread_rng`
//! appears nowhere. The same seed and the same order sequence produce
//! byte-identical executions on every run and every machine, which is what
//! makes the tests downstream of it mean anything.
//!
//! **What it does not model,** deliberately: an order book, queue position,
//! price-time priority, or price improvement. Fills are marketable-limit at the
//! order's own limit price. Those are a different exercise and the brief does
//! not ask for them (`docs/design.md` §6).

#![forbid(unsafe_code)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

use std::collections::BTreeSet;

use domain::{BrokerOrderId, DomainError, Money, Order, OrderEvent, Px, Qty, RejectReason, Symbol};
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
    /// Seeded partials: each execution takes a random slice of what is left,
    /// completing the order on the `max_slices`-th one so it always terminates.
    Partial { max_slices: u32 },
}

/// Broker-side limits. Account-side rejections (cash, position) are **not**
/// here — the broker does not know what a participant holds, and the engine
/// that does checks them before an order ever reaches this.
#[derive(Debug, Clone, Default)]
pub struct Limits {
    /// Empty means every symbol is tradable.
    pub known_symbols: BTreeSet<Symbol>,
    pub max_order_qty: Option<Qty>,
}

impl Limits {
    fn reject_reason(&self, order: &Order) -> Option<RejectReason> {
        if !self.known_symbols.is_empty() && !self.known_symbols.contains(&order.symbol) {
            return Some(RejectReason::UnknownSymbol);
        }
        if self.max_order_qty.is_some_and(|max| order.qty > max) {
            return Some(RejectReason::ExceedsSizeLimit);
        }
        None
    }
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
    limits: Limits,
    slices_done: u32,
}

impl MockBroker {
    pub fn new(seed: u64, policy: FillPolicy, fees: FeeSchedule, limits: Limits) -> Self {
        Self {
            rng: ChaCha8Rng::seed_from_u64(seed),
            next_id: 1,
            policy,
            fees,
            limits,
            slices_done: 0,
        }
    }

    /// A broker that acks everything and fills completely, with no fees — the
    /// default for tests that are about something else.
    pub fn simple(seed: u64) -> Self {
        Self::new(
            seed,
            FillPolicy::Complete,
            FeeSchedule::FREE,
            Limits::default(),
        )
    }

    fn mint_id(&mut self) -> BrokerOrderId {
        let id = BrokerOrderId::new(self.next_id);
        self.next_id += 1;
        id
    }
}

impl Broker for MockBroker {
    fn on_submit(&mut self, order: &Order) -> OrderEvent {
        match self.limits.reject_reason(order) {
            Some(reason) => OrderEvent::Rejected { reason },
            None => OrderEvent::Acknowledged {
                broker_id: self.mint_id(),
            },
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
                self.slices_done += 1;
                if self.slices_done >= max_slices || remaining.get() == 1 {
                    // Always terminates: the last permitted slice takes the
                    // rest, so an order cannot be left working forever.
                    self.slices_done = 0;
                    remaining
                } else {
                    Qty::new(self.rng.random_range(1..=remaining.get()))?
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
    use domain::{ClientOrderId, NewOrder, ParticipantId, Side, Timestamp};

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
        let a = run_to_fill(
            &mut MockBroker::new(42, policy, FeeSchedule::FREE, Limits::default()),
            100,
        );
        let b = run_to_fill(
            &mut MockBroker::new(42, policy, FeeSchedule::FREE, Limits::default()),
            100,
        );
        assert_eq!(a, b, "same seed must replay identically");
        assert!(
            a.len() > 1,
            "partial policy should produce more than one fill"
        );
    }

    #[test]
    fn a_different_seed_produces_different_executions() {
        let policy = FillPolicy::Partial { max_slices: 4 };
        let a = run_to_fill(
            &mut MockBroker::new(1, policy, FeeSchedule::FREE, Limits::default()),
            1000,
        );
        let b = run_to_fill(
            &mut MockBroker::new(2, policy, FeeSchedule::FREE, Limits::default()),
            1000,
        );
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
                    Limits::default(),
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
    fn an_unknown_symbol_is_rejected() {
        let mut limits = Limits::default();
        limits.known_symbols.insert(Symbol::parse("AAPL").unwrap());
        let mut broker = MockBroker::new(0, FillPolicy::Complete, FeeSchedule::FREE, limits);

        assert_eq!(
            broker.on_submit(&order(10, "TSLA")),
            OrderEvent::Rejected {
                reason: RejectReason::UnknownSymbol
            }
        );
        assert!(matches!(
            broker.on_submit(&order(10, "AAPL")),
            OrderEvent::Acknowledged { .. }
        ));
    }

    #[test]
    fn an_oversized_order_is_rejected() {
        let limits = Limits {
            max_order_qty: Some(Qty::new(100).unwrap()),
            ..Limits::default()
        };
        let mut broker = MockBroker::new(0, FillPolicy::Complete, FeeSchedule::FREE, limits);

        assert_eq!(
            broker.on_submit(&order(101, "AAPL")),
            OrderEvent::Rejected {
                reason: RejectReason::ExceedsSizeLimit
            }
        );
        assert!(matches!(
            broker.on_submit(&order(100, "AAPL")),
            OrderEvent::Acknowledged { .. }
        ));
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
