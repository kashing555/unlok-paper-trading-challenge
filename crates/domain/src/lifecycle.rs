//! The order state machine.
//!
//! The six states are the brief's, verbatim. Transitions are a **total
//! function** over `(state, event)` with no wildcard arm anywhere: adding a
//! state breaks the outer match, adding an event breaks every inner one, and
//! both are exactly the mistake a wildcard would hide (`CLAUDE.md`, "Writing
//! Rust here").
//!
//! Each state carries **exactly its own data** — `Acknowledged` cannot exist
//! without a broker id, `Rejected` has no filled quantity because there is no
//! such thing. A struct with `filled`, `cancelled_at` and `reject_reason` as
//! optional fields would admit "cancelled and rejected with a fill", which no
//! order can be in and no test would cover.
//!
//! Fills accumulate `filled` and **`cost`, not an average price**: storing a
//! rounded average and re-multiplying it drifts, and the drift only surfaces
//! as a reconciliation that fails days later (`.claude/code-style.md`).

use thiserror::Error;

use crate::{BrokerOrderId, DomainError, Money, Px, Qty};

/// Why a broker refused an order. Closed rather than free text — a reject
/// reason is branched on, and a `String` cannot be matched exhaustively.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectReason {
    UnknownSymbol,
    InsufficientCash,
    InsufficientPosition,
    ExceedsSizeLimit,
}

/// The six states from the brief.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrderState {
    /// Submitted; the broker has not acked yet. **Cancellable** — this window is
    /// real because executions are driven separately from submission, and it is
    /// why we mint our own id before the broker gives us one.
    New,
    Acknowledged {
        broker_id: BrokerOrderId,
    },
    PartiallyFilled {
        broker_id: BrokerOrderId,
        filled: Qty,
        cost: Money,
    },
    /// Terminal. Carries no `broker_id`: it is needed while an order is *live*,
    /// to cancel it and to correlate reports against it. Once terminal that is
    /// history, and history lives in the event log.
    Filled {
        filled: Qty,
        cost: Money,
    },
    /// Terminal. `filled` may be non-zero — cancelling after a partial fill
    /// keeps the fills, which is the whole point of tracking it here.
    Cancelled {
        filled: Qty,
        cost: Money,
    },
    Rejected {
        reason: RejectReason,
    },
}

/// A fact that has already happened to an order.
///
/// `Fill` is singular — one execution report — and deliberately not named
/// `Filled`, which is the *state* reached only when the last one lands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrderEvent {
    Acknowledged { broker_id: BrokerOrderId },
    Rejected { reason: RejectReason },
    Fill { qty: Qty, px: Px },
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TransitionError {
    #[error("cannot apply {event} to an order in state {state}")]
    Illegal {
        state: &'static str,
        event: &'static str,
    },

    #[error("fill of {fill} would take filled quantity to {total}, over the ordered {ordered}")]
    Overfill { fill: i64, total: i64, ordered: i64 },

    #[error("a fill must be for a positive quantity")]
    EmptyFill,

    #[error(transparent)]
    Domain(#[from] DomainError),
}

impl OrderState {
    pub const fn name(&self) -> &'static str {
        match self {
            Self::New => "NEW",
            Self::Acknowledged { .. } => "ACKNOWLEDGED",
            Self::PartiallyFilled { .. } => "PARTIALLY_FILLED",
            Self::Filled { .. } => "FILLED",
            Self::Cancelled { .. } => "CANCELLED",
            Self::Rejected { .. } => "REJECTED",
        }
    }

    /// Terminal states accept no further events. A fill arriving on one is an
    /// error rather than a warning: it is a P&L bug that would otherwise be
    /// found during a reconciliation days later.
    pub const fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Filled { .. } | Self::Cancelled { .. } | Self::Rejected { .. }
        )
    }

    /// Quantity executed so far, in any state.
    pub const fn filled(&self) -> Qty {
        match self {
            Self::New | Self::Acknowledged { .. } | Self::Rejected { .. } => Qty::ZERO,
            Self::PartiallyFilled { filled, .. }
            | Self::Filled { filled, .. }
            | Self::Cancelled { filled, .. } => *filled,
        }
    }

    /// Gross notional executed so far, excluding fees — fees belong to the
    /// portfolio fold, not to the lifecycle.
    pub const fn cost(&self) -> Money {
        match self {
            Self::New | Self::Acknowledged { .. } | Self::Rejected { .. } => Money::ZERO,
            Self::PartiallyFilled { cost, .. }
            | Self::Filled { cost, .. }
            | Self::Cancelled { cost, .. } => *cost,
        }
    }

    pub const fn broker_id(&self) -> Option<BrokerOrderId> {
        match self {
            Self::Acknowledged { broker_id } | Self::PartiallyFilled { broker_id, .. } => {
                Some(*broker_id)
            }
            Self::New | Self::Filled { .. } | Self::Cancelled { .. } | Self::Rejected { .. } => {
                None
            }
        }
    }
}

impl OrderEvent {
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Acknowledged { .. } => "an acknowledgement",
            Self::Rejected { .. } => "a rejection",
            Self::Fill { .. } => "a fill",
            Self::Cancelled => "a cancellation",
        }
    }
}

/// Apply one event to one state.
///
/// `ordered` is the order's total quantity, needed to decide whether a fill
/// completes the order or leaves it partial.
pub fn apply(
    state: &OrderState,
    ordered: Qty,
    event: &OrderEvent,
) -> Result<OrderState, TransitionError> {
    use OrderEvent as E;
    use OrderState as S;

    let illegal = || TransitionError::Illegal {
        state: state.name(),
        event: event.name(),
    };

    match state {
        // Terminal states accept nothing. Listing the variants rather than
        // using `_` keeps the compiler's exhaustiveness check switched on.
        S::Filled { .. } | S::Cancelled { .. } | S::Rejected { .. } => Err(illegal()),

        S::New => match event {
            E::Acknowledged { broker_id } => Ok(S::Acknowledged {
                broker_id: *broker_id,
            }),
            E::Rejected { reason } => Ok(S::Rejected { reason: *reason }),
            E::Cancelled => Ok(S::Cancelled {
                filled: Qty::ZERO,
                cost: Money::ZERO,
            }),
            // A fill before the ack means the broker filled an order it never
            // acknowledged: report the contradiction rather than book it.
            E::Fill { .. } => Err(illegal()),
        },

        S::Acknowledged { broker_id } => match event {
            E::Fill { qty, px } => fill(*broker_id, Qty::ZERO, Money::ZERO, ordered, *qty, *px),
            E::Cancelled => Ok(S::Cancelled {
                filled: Qty::ZERO,
                cost: Money::ZERO,
            }),
            E::Acknowledged { .. } | E::Rejected { .. } => Err(illegal()),
        },

        S::PartiallyFilled {
            broker_id,
            filled,
            cost,
        } => match event {
            E::Fill { qty, px } => fill(*broker_id, *filled, *cost, ordered, *qty, *px),
            // The fills survive the cancel — cancelling withdraws the residual,
            // it does not undo what already executed.
            E::Cancelled => Ok(S::Cancelled {
                filled: *filled,
                cost: *cost,
            }),
            E::Acknowledged { .. } | E::Rejected { .. } => Err(illegal()),
        },
    }
}

fn fill(
    broker_id: BrokerOrderId,
    filled: Qty,
    cost: Money,
    ordered: Qty,
    qty: Qty,
    px: Px,
) -> Result<OrderState, TransitionError> {
    if qty.is_zero() {
        return Err(TransitionError::EmptyFill);
    }

    let total = filled.checked_add(qty)?;
    if total > ordered {
        return Err(TransitionError::Overfill {
            fill: qty.get(),
            total: total.get(),
            ordered: ordered.get(),
        });
    }

    let cost = cost.checked_add(px.notional(qty)?)?;

    Ok(if total == ordered {
        OrderState::Filled {
            filled: total,
            cost,
        }
    } else {
        OrderState::PartiallyFilled {
            broker_id,
            filled: total,
            cost,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORDERED: i64 = 100;

    fn broker() -> BrokerOrderId {
        BrokerOrderId::new(7)
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
    fn step(state: &OrderState, event: &OrderEvent) -> Result<OrderState, TransitionError> {
        apply(state, qty(ORDERED), event)
    }

    fn all_states() -> Vec<OrderState> {
        vec![
            OrderState::New,
            OrderState::Acknowledged {
                broker_id: broker(),
            },
            OrderState::PartiallyFilled {
                broker_id: broker(),
                filled: qty(40),
                cost: money("400"),
            },
            OrderState::Filled {
                filled: qty(ORDERED),
                cost: money("1000"),
            },
            OrderState::Cancelled {
                filled: qty(40),
                cost: money("400"),
            },
            OrderState::Rejected {
                reason: RejectReason::UnknownSymbol,
            },
        ]
    }

    fn all_events() -> Vec<OrderEvent> {
        vec![
            OrderEvent::Acknowledged {
                broker_id: broker(),
            },
            OrderEvent::Rejected {
                reason: RejectReason::InsufficientCash,
            },
            OrderEvent::Fill {
                qty: qty(10),
                px: px("10"),
            },
            OrderEvent::Cancelled,
        ]
    }

    /// The close condition for A1: **every** state x event pair is specified,
    /// with none left unasserted. The `expected` table is in the same order as
    /// `all_states() x all_events()`, and the length assertion is what stops a
    /// new state or event silently escaping coverage.
    #[test]
    fn every_state_event_pair_is_specified() {
        // NEW: ack ok, reject ok, fill illegal (not acked), cancel ok
        // ACKNOWLEDGED: re-ack no, late reject no, fill yes, cancel yes
        // PARTIALLY_FILLED: same as acknowledged
        // FILLED / CANCELLED / REJECTED: terminal, nothing is accepted
        #[rustfmt::skip]
        let expected = [
            true,  true,  false, true,
            false, false, true,  true,
            false, false, true,  true,
            false, false, false, false,
            false, false, false, false,
            false, false, false, false,
        ];

        let states = all_states();
        let events = all_events();
        assert_eq!(
            expected.len(),
            states.len() * events.len(),
            "the table must cover every pair"
        );

        let mut i = 0;
        for state in &states {
            for event in &events {
                let got = step(state, event);
                assert_eq!(
                    got.is_ok(),
                    expected[i],
                    "{} + {} gave {got:?}",
                    state.name(),
                    event.name()
                );
                i += 1;
            }
        }
    }

    #[test]
    fn acknowledgement_records_the_broker_id() {
        let acked = step(
            &OrderState::New,
            &OrderEvent::Acknowledged {
                broker_id: broker(),
            },
        )
        .unwrap();
        assert_eq!(acked.broker_id(), Some(broker()));
    }

    #[test]
    fn partial_fills_accumulate_then_close_to_filled() {
        let mut state = OrderState::Acknowledged {
            broker_id: broker(),
        };

        state = step(
            &state,
            &OrderEvent::Fill {
                qty: qty(30),
                px: px("10"),
            },
        )
        .unwrap();
        assert_eq!(state.name(), "PARTIALLY_FILLED");
        assert_eq!(state.filled(), qty(30));

        state = step(
            &state,
            &OrderEvent::Fill {
                qty: qty(50),
                px: px("10"),
            },
        )
        .unwrap();
        assert_eq!(state.filled(), qty(80));
        assert_eq!(state.name(), "PARTIALLY_FILLED");

        // The fill that completes the order closes it, without anyone deciding
        // separately that it is done.
        state = step(
            &state,
            &OrderEvent::Fill {
                qty: qty(20),
                px: px("10"),
            },
        )
        .unwrap();
        assert_eq!(state.name(), "FILLED");
        assert_eq!(state.filled(), qty(ORDERED));
        assert_eq!(state.cost(), money("1000"));
    }

    #[test]
    fn cost_accumulates_exactly_rather_than_averaging() {
        // Two fills at prices whose mean is not representable at any scale we
        // would round an average to. Tracking cost keeps it exact.
        let mut state = OrderState::Acknowledged {
            broker_id: broker(),
        };
        state = step(
            &state,
            &OrderEvent::Fill {
                qty: qty(1),
                px: px("10.0050"),
            },
        )
        .unwrap();
        state = step(
            &state,
            &OrderEvent::Fill {
                qty: qty(2),
                px: px("10.0150"),
            },
        )
        .unwrap();

        // 10.0050 + 2 x 10.0150 = 30.0350, exactly.
        assert_eq!(state.cost(), money("30.0350"));
        assert_eq!(state.filled(), qty(3));
    }

    #[test]
    fn cancel_before_acknowledgement_is_allowed() {
        // The window the client-minted id exists for: the order is live at our
        // end and the broker has not given us an id yet.
        let state = step(&OrderState::New, &OrderEvent::Cancelled).unwrap();
        assert_eq!(state.name(), "CANCELLED");
        assert_eq!(state.filled(), Qty::ZERO);
    }

    #[test]
    fn cancel_after_partial_fill_retains_filled_quantity() {
        let state = OrderState::PartiallyFilled {
            broker_id: broker(),
            filled: qty(40),
            cost: money("400"),
        };
        let cancelled = step(&state, &OrderEvent::Cancelled).unwrap();

        // Cancelling withdraws the residual 60. It does not undo the 40 that
        // executed — those are real shares that were really bought.
        assert_eq!(cancelled.name(), "CANCELLED");
        assert_eq!(cancelled.filled(), qty(40));
        assert_eq!(cancelled.cost(), money("400"));
    }

    #[test]
    fn a_fill_may_not_exceed_the_ordered_quantity() {
        let state = OrderState::PartiallyFilled {
            broker_id: broker(),
            filled: qty(90),
            cost: money("900"),
        };
        assert_eq!(
            step(
                &state,
                &OrderEvent::Fill {
                    qty: qty(20),
                    px: px("10")
                }
            ),
            Err(TransitionError::Overfill {
                fill: 20,
                total: 110,
                ordered: 100
            })
        );
    }

    #[test]
    fn an_empty_fill_is_rejected_rather_than_ignored() {
        let state = OrderState::Acknowledged {
            broker_id: broker(),
        };
        assert_eq!(
            step(
                &state,
                &OrderEvent::Fill {
                    qty: Qty::ZERO,
                    px: px("10")
                }
            ),
            Err(TransitionError::EmptyFill)
        );
    }

    #[test]
    fn a_fill_on_a_terminal_order_is_an_error_not_a_warning() {
        // The bug this prevents does not show up now — it shows up as a P&L
        // that fails to reconcile days later.
        let filled = OrderState::Filled {
            filled: qty(ORDERED),
            cost: money("1000"),
        };
        assert_eq!(
            step(
                &filled,
                &OrderEvent::Fill {
                    qty: qty(1),
                    px: px("10")
                }
            ),
            Err(TransitionError::Illegal {
                state: "FILLED",
                event: "a fill"
            })
        );
    }

    #[test]
    fn terminal_states_are_reported_as_terminal() {
        for state in all_states() {
            let expected = matches!(state.name(), "FILLED" | "CANCELLED" | "REJECTED");
            assert_eq!(state.is_terminal(), expected, "{}", state.name());
        }
    }
}
