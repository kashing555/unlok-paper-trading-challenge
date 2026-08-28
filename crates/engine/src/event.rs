//! Facts, in the order they happened.
//!
//! The event log is the system of record: participants, orders, positions,
//! cash and P&L are all folds over this (`CLAUDE.md` rule 1). Two consequences
//! shape what an event has to carry.
//!
//! **Events carry the broker's output, not the instruction to ask it.** An
//! acknowledgement records the broker id that was minted; a fill records the
//! terms that were chosen. Replay therefore never consults the broker, never
//! advances its RNG, and reproduces exactly what happened rather than what
//! would happen if it ran again.
//!
//! **Events carry submission terms, not order state.** An order is always
//! `NEW` at the moment it is submitted, so recording its state would be storing
//! a constant — and a redundant field is a field that can disagree. The
//! lifecycle is derived by folding the later events, which is the same rule as
//! positions and P&L: keep the facts, derive the rest.

use domain::{
    BrokerOrderId, ClientOrderId, Money, NewOrder, ParticipantId, Px, Qty, RejectReason, Symbol,
    Timestamp, TradingDay,
};
use scoring::DayInput;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    ParticipantCreated {
        participant: ParticipantId,
        starting_cash: Money,
    },
    OrderSubmitted {
        order: NewOrder,
    },
    OrderAcknowledged {
        id: ClientOrderId,
        broker_id: BrokerOrderId,
    },
    OrderRejected {
        id: ClientOrderId,
        reason: RejectReason,
    },
    OrderFilled {
        id: ClientOrderId,
        qty: Qty,
        px: Px,
        fee: Money,
    },
    OrderCancelled {
        id: ClientOrderId,
    },
    /// The original is cancelled and the replacement submitted, as one fact —
    /// so a reader cannot see half of a cancel-replace.
    OrderReplaced {
        original: ClientOrderId,
        replacement: NewOrder,
    },
    MarkUpdated {
        symbol: Symbol,
        px: Px,
    },
    /// A day's closing facts, per participant.
    ///
    /// The **facts** are stored, not the computed leaderboard: closing value,
    /// prior close, turnover and activity are what happened, and they cannot
    /// change. The ranking is recomputed from them on demand, which keeps the
    /// event small and means a stored board can never drift from the facts it
    /// was built on. The trade is that changing the ranking rules would change
    /// historical boards — in production that needs a migration, and it is
    /// listed as such in the README.
    DayClosed {
        day: TradingDay,
        entries: Vec<DayInput>,
    },
}

/// An event with its position in the log.
///
/// `seq` is the total order the single-writer loop guarantees; `at` is the
/// caller's timestamp. Together they are what makes a replay reproducible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Journaled {
    pub seq: u64,
    pub at: Timestamp,
    pub event: Event,
}

impl Event {
    /// Short stable tag, for logs and for the store's `kind` column.
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::ParticipantCreated { .. } => "participant_created",
            Self::OrderSubmitted { .. } => "order_submitted",
            Self::OrderAcknowledged { .. } => "order_acknowledged",
            Self::OrderRejected { .. } => "order_rejected",
            Self::OrderFilled { .. } => "order_filled",
            Self::OrderCancelled { .. } => "order_cancelled",
            Self::OrderReplaced { .. } => "order_replaced",
            Self::MarkUpdated { .. } => "mark_updated",
            Self::DayClosed { .. } => "day_closed",
        }
    }
}
