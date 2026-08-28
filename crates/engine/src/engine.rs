//! The engine: commands in, events out, state as a fold over the events.
//!
//! **`apply` is the only thing that mutates state**, and it is the same code
//! path a replay uses. Anything `decide` learns that is not in an event — a
//! broker id, a chosen fill size — would be lost on replay, so it is put in the
//! event rather than applied directly. That single rule is what makes
//! "rebuild from the log and get the same state" true by construction rather
//! than by discipline.
//!
//! **`decide` validates everything before any mutation happens.** A command
//! that would advance the order but be refused by the book is refused whole:
//! the portfolio effect is checked against a clone first. A half-applied
//! command is a position that disagrees with the cash that paid for it.

use std::collections::BTreeMap;

use broker::{Broker, BrokerError};
use domain::{
    lifecycle, ClientOrderId, DomainError, Fill, Marks, Money, NewOrder, Order, OrderEvent,
    ParticipantId, Portfolio, PortfolioError, Px, Qty, Side, Symbol, Timestamp, TradingDay,
    TransitionError,
};
use scoring::{ladder, leaderboard, DayInput, LadderRow, Leaderboard, ScoringError};
use thiserror::Error;

use crate::{Command, Event, Journaled};

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum EngineError {
    #[error("no such participant: {0}")]
    UnknownParticipant(ParticipantId),

    #[error("participant already exists: {0}")]
    DuplicateParticipant(ParticipantId),

    #[error("no such order: {0}")]
    UnknownOrder(ClientOrderId),

    #[error("order id already used: {0}")]
    DuplicateOrder(ClientOrderId),

    #[error("insufficient available cash: need {need}, {available} free after working orders")]
    InsufficientAvailableCash { need: Money, available: Money },

    #[error("insufficient available {symbol}: want {want}, {available} free after working orders")]
    InsufficientAvailablePosition {
        symbol: Symbol,
        want: i64,
        available: i64,
    },

    #[error("nothing to execute on order {0}")]
    NothingToExecute(ClientOrderId),

    #[error("day {0} has not been closed")]
    DayNotClosed(TradingDay),

    #[error(transparent)]
    Scoring(#[from] ScoringError),

    #[error(transparent)]
    Transition(#[from] TransitionError),
    #[error(transparent)]
    Portfolio(#[from] PortfolioError),
    #[error(transparent)]
    Broker(#[from] BrokerError),
    #[error(transparent)]
    Domain(#[from] DomainError),
}

pub struct Engine<B: Broker> {
    broker: B,
    participants: BTreeMap<ParticipantId, Portfolio>,
    orders: BTreeMap<ClientOrderId, Order>,
    marks: Marks,
    seq: u64,
    /// Gross notional traded since the last close, per participant. A
    /// projection of the fills, reset by `DayClosed` — not a counter kept
    /// alongside them.
    day_turnover: BTreeMap<ParticipantId, Money>,
    day_fills: BTreeMap<ParticipantId, u32>,
    /// Last published closing value; the baseline the next day's return is
    /// measured from. Absent means the participant has not closed a day yet,
    /// and their starting cash is the baseline.
    last_close: BTreeMap<ParticipantId, Money>,
    closed: BTreeMap<TradingDay, Vec<DayInput>>,
}

impl<B: Broker> Engine<B> {
    pub fn new(broker: B) -> Self {
        Self {
            broker,
            participants: BTreeMap::new(),
            orders: BTreeMap::new(),
            marks: Marks::new(),
            seq: 0,
            day_turnover: BTreeMap::new(),
            day_fills: BTreeMap::new(),
            last_close: BTreeMap::new(),
            closed: BTreeMap::new(),
        }
    }

    // ---- reads -----------------------------------------------------------

    pub fn portfolio(&self, id: &ParticipantId) -> Result<&Portfolio, EngineError> {
        self.participants
            .get(id)
            .ok_or_else(|| EngineError::UnknownParticipant(id.clone()))
    }

    pub fn participants(&self) -> impl Iterator<Item = &Portfolio> {
        self.participants.values()
    }

    pub fn order(&self, id: ClientOrderId) -> Result<&Order, EngineError> {
        self.orders.get(&id).ok_or(EngineError::UnknownOrder(id))
    }

    /// Every order, in id order — deterministic, because a `BTreeMap` orders by
    /// key rather than by hash seed.
    pub fn orders(&self) -> impl Iterator<Item = &Order> {
        self.orders.values()
    }

    pub fn orders_of<'a>(&'a self, id: &'a ParticipantId) -> impl Iterator<Item = &'a Order> + 'a {
        self.orders.values().filter(move |o| &o.participant == id)
    }

    /// The brief's "active orders": submitted and not yet terminal.
    pub fn working_orders_of<'a>(
        &'a self,
        id: &'a ParticipantId,
    ) -> impl Iterator<Item = &'a Order> + 'a {
        self.orders_of(id).filter(|o| !o.state.is_terminal())
    }

    pub const fn marks(&self) -> &Marks {
        &self.marks
    }

    pub const fn seq(&self) -> u64 {
        self.seq
    }

    // ---- pre-trade availability -----------------------------------------

    /// Cash not already committed to working buy orders.
    ///
    /// Derived from the working orders every time rather than cached: a
    /// reserved-cash counter kept alongside the orders is a second copy of a
    /// fact the orders already hold, and the two would eventually disagree.
    ///
    /// `excluding` is the order a replace is about to cancel — its reservation
    /// is released by the same command that takes the new one.
    fn available_cash(
        &self,
        id: &ParticipantId,
        excluding: Option<ClientOrderId>,
    ) -> Result<Money, EngineError> {
        let mut reserved = Money::ZERO;
        for order in self.working_orders_of(id) {
            if order.side != Side::Buy || Some(order.id) == excluding {
                continue;
            }
            let notional = order.limit_px.notional(order.remaining()?)?;
            reserved = reserved
                .checked_add(notional)?
                .checked_add(self.broker.fee_on(notional)?)?;
        }
        Ok(self.portfolio(id)?.cash().checked_sub(reserved)?)
    }

    /// Shares not already committed to working sell orders.
    fn available_qty(
        &self,
        id: &ParticipantId,
        symbol: &Symbol,
        excluding: Option<ClientOrderId>,
    ) -> Result<Qty, EngineError> {
        let held = self
            .portfolio(id)?
            .position(symbol)
            .map_or(Qty::ZERO, domain::Position::qty);

        let mut reserved = Qty::ZERO;
        for order in self.working_orders_of(id) {
            if order.side != Side::Sell || &order.symbol != symbol || Some(order.id) == excluding {
                continue;
            }
            reserved = reserved.checked_add(order.remaining()?)?;
        }
        Ok(held.checked_sub(reserved)?)
    }

    /// The account-side pre-trade check.
    ///
    /// Deliberately here and not in the broker: a broker does not know what a
    /// participant holds. Rejecting *before* the broker sees the order also
    /// means the reject reason is the true one rather than a guess.
    fn check_affordable(
        &self,
        order: &Order,
        excluding: Option<ClientOrderId>,
    ) -> Result<(), EngineError> {
        match order.side {
            Side::Buy => {
                let notional = order.limit_px.notional(order.qty)?;
                let need = notional.checked_add(self.broker.fee_on(notional)?)?;
                let available = self.available_cash(&order.participant, excluding)?;
                if need > available {
                    return Err(EngineError::InsufficientAvailableCash { need, available });
                }
            }
            Side::Sell => {
                let available = self.available_qty(&order.participant, &order.symbol, excluding)?;
                if order.qty > available {
                    return Err(EngineError::InsufficientAvailablePosition {
                        symbol: order.symbol.clone(),
                        want: order.qty.get(),
                        available: available.get(),
                    });
                }
            }
        }
        Ok(())
    }

    // ---- execute ---------------------------------------------------------

    /// Decide, journal, apply. The only entry point for change.
    pub fn execute(
        &mut self,
        at: Timestamp,
        command: Command,
    ) -> Result<Vec<Journaled>, EngineError> {
        let events = self.decide(at, command)?;

        let mut journaled = Vec::with_capacity(events.len());
        for event in events {
            self.seq += 1;
            journaled.push(Journaled {
                seq: self.seq,
                at,
                event,
            });
        }

        for entry in &journaled {
            self.apply(&entry.event)?;
        }
        Ok(journaled)
    }

    fn decide(&mut self, at: Timestamp, command: Command) -> Result<Vec<Event>, EngineError> {
        match command {
            Command::CreateParticipant {
                participant,
                starting_cash,
            } => {
                if self.participants.contains_key(&participant) {
                    return Err(EngineError::DuplicateParticipant(participant));
                }
                Ok(vec![Event::ParticipantCreated {
                    participant,
                    starting_cash,
                }])
            }

            Command::SubmitOrder {
                id,
                participant,
                symbol,
                side,
                qty,
                limit_px,
            } => {
                if self.orders.contains_key(&id) {
                    return Err(EngineError::DuplicateOrder(id));
                }
                // Existence check before anything else, so an unknown
                // participant is reported as such rather than as "no cash".
                self.portfolio(&participant)?;

                let order = Order::submit(NewOrder {
                    id,
                    participant,
                    symbol,
                    side,
                    qty,
                    limit_px,
                    at,
                })?;
                self.check_affordable(&order, None)?;

                let response = self.broker.on_submit(&order);
                Ok(vec![
                    Event::OrderSubmitted {
                        order: Box::new(order),
                    },
                    broker_response(id, response)?,
                ])
            }

            Command::CancelOrder { id } => {
                let order = self.order(id)?;
                // Validate the transition here so a cancel of a terminal order
                // fails as a command rather than during apply.
                lifecycle::apply(&order.state, order.qty, &OrderEvent::Cancelled)?;
                Ok(vec![Event::OrderCancelled { id }])
            }

            Command::ReplaceOrder {
                id,
                replacement_id,
                qty,
                limit_px,
            } => {
                if self.orders.contains_key(&replacement_id) {
                    return Err(EngineError::DuplicateOrder(replacement_id));
                }
                let original = self.order(id)?;
                let outcome = original.replace(replacement_id, qty, limit_px, at)?;

                // The original's reservation is released by this same command,
                // so it must not count against the replacement.
                self.check_affordable(&outcome.replacement, Some(id))?;

                let response = self.broker.on_submit(&outcome.replacement);
                Ok(vec![
                    Event::OrderReplaced {
                        original: id,
                        replacement: Box::new(outcome.replacement),
                    },
                    broker_response(replacement_id, response)?,
                ])
            }

            Command::Execute { id, qty, px } => {
                let fee = self.broker.fee_on(px.notional(qty)?)?;
                self.validated_fill(id, qty, px, fee)
            }

            Command::AutoExecute { id } => {
                let order = self.order(id)?.clone();
                let execution = self
                    .broker
                    .next_execution(&order)?
                    .ok_or(EngineError::NothingToExecute(id))?;
                self.validated_fill(id, execution.qty, execution.px, execution.fee)
            }

            Command::UpdateMark { symbol, px } => Ok(vec![Event::MarkUpdated { symbol, px }]),

            Command::CloseDay { day } => {
                // Idempotent: a published ranking that silently recomputes is
                // worse than a stale one, so re-closing does nothing at all.
                if self.closed.contains_key(&day) {
                    return Ok(vec![]);
                }
                Ok(vec![Event::DayClosed {
                    day,
                    entries: self.day_entries()?,
                }])
            }
        }
    }

    /// The facts each participant closes the day on.
    ///
    /// **Fails closed** if any held symbol lacks a mark: `total_value`
    /// propagates the error rather than valuing at zero, and a wrong closing
    /// value corrupts a leaderboard that is then immutable.
    fn day_entries(&self) -> Result<Vec<DayInput>, EngineError> {
        let mut entries = Vec::with_capacity(self.participants.len());
        for portfolio in self.participants.values() {
            let id = portfolio.participant();
            let closing_value = portfolio.total_value(&self.marks)?;
            let fills = self.day_fills.get(id).copied().unwrap_or(0);

            entries.push(DayInput {
                participant: id.clone(),
                closing_value,
                // First day measures from what the participant was given.
                prior_closing_value: self
                    .last_close
                    .get(id)
                    .copied()
                    .unwrap_or_else(|| portfolio.starting_cash()),
                turnover: self.day_turnover.get(id).copied().unwrap_or(Money::ZERO),
                // Active if they traded today, or still hold something. A
                // participant who held at open and sold out has fills > 0, so
                // both halves of "traded or was exposed" are covered.
                active: fills > 0 || portfolio.positions().next().is_some(),
            });
        }
        Ok(entries)
    }

    /// Check a fill against **both** the order's lifecycle and the book, and
    /// only then emit it.
    ///
    /// The book is checked against a clone. Cloning one portfolio is cheap and
    /// it is the difference between "the command was refused" and "the order
    /// advanced but the cash did not".
    fn validated_fill(
        &self,
        id: ClientOrderId,
        qty: Qty,
        px: Px,
        fee: Money,
    ) -> Result<Vec<Event>, EngineError> {
        let order = self.order(id)?;
        lifecycle::apply(&order.state, order.qty, &OrderEvent::Fill { qty, px })?;

        let mut probe = self.portfolio(&order.participant)?.clone();
        probe.apply(&Fill {
            symbol: order.symbol.clone(),
            side: order.side,
            qty,
            px,
            fee,
        })?;

        Ok(vec![Event::OrderFilled { id, qty, px, fee }])
    }

    // ---- apply -----------------------------------------------------------

    /// Fold one event into state.
    ///
    /// The single mutator, and the one a replay uses. Errors here are invariant
    /// breaks rather than refusals — `decide` already established that the
    /// event is legal — so they are surfaced, never swallowed.
    pub fn apply(&mut self, event: &Event) -> Result<(), EngineError> {
        match event {
            Event::ParticipantCreated {
                participant,
                starting_cash,
            } => {
                self.participants.insert(
                    participant.clone(),
                    Portfolio::open(participant.clone(), *starting_cash),
                );
            }

            Event::OrderSubmitted { order } => {
                self.orders.insert(order.id, (**order).clone());
            }

            Event::OrderAcknowledged { id, broker_id } => {
                self.order_mut(*id)?.apply(&OrderEvent::Acknowledged {
                    broker_id: *broker_id,
                })?;
            }

            Event::OrderRejected { id, reason } => {
                self.order_mut(*id)?
                    .apply(&OrderEvent::Rejected { reason: *reason })?;
            }

            Event::OrderCancelled { id } => {
                self.order_mut(*id)?.apply(&OrderEvent::Cancelled)?;
            }

            Event::OrderReplaced {
                original,
                replacement,
            } => {
                self.order_mut(*original)?.apply(&OrderEvent::Cancelled)?;
                self.orders.insert(replacement.id, (**replacement).clone());
            }

            Event::OrderFilled { id, qty, px, fee } => {
                let order = self.order_mut(*id)?;
                order.apply(&OrderEvent::Fill { qty: *qty, px: *px })?;
                let fill = Fill {
                    symbol: order.symbol.clone(),
                    side: order.side,
                    qty: *qty,
                    px: *px,
                    fee: *fee,
                };
                let participant = order.participant.clone();
                self.participants
                    .get_mut(&participant)
                    .ok_or(EngineError::UnknownParticipant(participant.clone()))?
                    .apply(&fill)?;

                // Turnover is gross notional, both sides — it measures how much
                // trading was done, not what it netted to.
                let notional = px.notional(*qty)?;
                let running = self
                    .day_turnover
                    .entry(participant.clone())
                    .or_insert(Money::ZERO);
                *running = running.checked_add(notional)?;
                *self.day_fills.entry(participant).or_insert(0) += 1;
            }

            Event::MarkUpdated { symbol, px } => {
                self.marks.set(symbol.clone(), *px);
            }

            Event::DayClosed { day, entries } => {
                for entry in entries {
                    self.last_close
                        .insert(entry.participant.clone(), entry.closing_value);
                }
                self.closed.insert(*day, entries.clone());
                // The day's counters belong to the day that just ended.
                self.day_turnover.clear();
                self.day_fills.clear();
            }
        }
        Ok(())
    }

    /// The published leaderboard for a closed day, recomputed from the stored
    /// facts. Errors rather than inventing one for a day that is still open.
    pub fn leaderboard(&self, day: TradingDay) -> Result<Leaderboard, EngineError> {
        let entries = self
            .closed
            .get(&day)
            .ok_or(EngineError::DayNotClosed(day))?;
        Ok(leaderboard(day, scoring::daily_results(entries.clone())?)?)
    }

    pub fn closed_days(&self) -> impl Iterator<Item = TradingDay> + '_ {
        self.closed.keys().copied()
    }

    /// The overall ladder across every closed day, oldest first.
    pub fn ladder(&self) -> Result<Vec<LadderRow>, EngineError> {
        let boards: Vec<Leaderboard> = self
            .closed_days()
            .map(|d| self.leaderboard(d))
            .collect::<Result<_, _>>()?;
        Ok(ladder(&boards))
    }

    /// Rebuild state from a log. Never consults the broker — everything it
    /// decided is already recorded in the events.
    pub fn replay(
        broker: B,
        log: impl IntoIterator<Item = Journaled>,
    ) -> Result<Self, EngineError> {
        let mut engine = Self::new(broker);
        for entry in log {
            engine.apply(&entry.event)?;
            engine.seq = entry.seq;
        }
        Ok(engine)
    }

    fn order_mut(&mut self, id: ClientOrderId) -> Result<&mut Order, EngineError> {
        self.orders
            .get_mut(&id)
            .ok_or(EngineError::UnknownOrder(id))
    }
}

fn broker_response(id: ClientOrderId, response: OrderEvent) -> Result<Event, EngineError> {
    match response {
        OrderEvent::Acknowledged { broker_id } => Ok(Event::OrderAcknowledged { id, broker_id }),
        OrderEvent::Rejected { reason } => Ok(Event::OrderRejected { id, reason }),
        // The port's contract is ack-or-reject. Anything else is a broker bug,
        // and it fails here rather than being folded into the log.
        other => Err(EngineError::Transition(TransitionError::Illegal {
            state: "NEW",
            event: other.name(),
        })),
    }
}
