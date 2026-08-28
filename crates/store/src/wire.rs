//! The serialisation mirror.
//!
//! **Domain types carry no serde derives** (`.claude/rust.md`), so the on-disk
//! format is defined here and mapped explicitly. The cost is this file; what it
//! buys is that a rename inside `domain` cannot silently change the format of
//! data already written, and that the storage format is a decision made in one
//! place rather than a by-product of a struct definition.
//!
//! Every field crosses as its **raw scaled integer**, never a float — a JSON
//! number for money would be an IEEE double, and this file is the boundary
//! where that mistake would be permanent.

use domain::{
    BrokerOrderId, ClientOrderId, DomainError, Money, Order, OrderState, ParticipantId, Px, Qty,
    RejectReason, Side, Symbol, Timestamp,
};
use engine::{Event, Journaled};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WireSide {
    Buy,
    Sell,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WireReject {
    UnknownSymbol,
    InsufficientCash,
    InsufficientPosition,
    ExceedsSizeLimit,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub(crate) enum WireState {
    New,
    Acknowledged {
        broker_id: u64,
    },
    PartiallyFilled {
        broker_id: u64,
        filled: i64,
        cost: i64,
    },
    Filled {
        filled: i64,
        cost: i64,
    },
    Cancelled {
        filled: i64,
        cost: i64,
    },
    Rejected {
        reason: WireReject,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct WireOrder {
    id: u64,
    participant: String,
    symbol: String,
    side: WireSide,
    qty: i64,
    limit_px: i64,
    state: WireState,
    replaces: Option<u64>,
    submitted_at: i64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum WireEvent {
    ParticipantCreated {
        participant: String,
        starting_cash: i64,
    },
    OrderSubmitted {
        order: WireOrder,
    },
    OrderAcknowledged {
        id: u64,
        broker_id: u64,
    },
    OrderRejected {
        id: u64,
        reason: WireReject,
    },
    OrderFilled {
        id: u64,
        qty: i64,
        px: i64,
        fee: i64,
    },
    OrderCancelled {
        id: u64,
    },
    OrderReplaced {
        original: u64,
        replacement: WireOrder,
    },
    MarkUpdated {
        symbol: String,
        px: i64,
    },
}

// ---- domain -> wire ------------------------------------------------------

impl From<Side> for WireSide {
    fn from(s: Side) -> Self {
        match s {
            Side::Buy => Self::Buy,
            Side::Sell => Self::Sell,
        }
    }
}

impl From<RejectReason> for WireReject {
    fn from(r: RejectReason) -> Self {
        match r {
            RejectReason::UnknownSymbol => Self::UnknownSymbol,
            RejectReason::InsufficientCash => Self::InsufficientCash,
            RejectReason::InsufficientPosition => Self::InsufficientPosition,
            RejectReason::ExceedsSizeLimit => Self::ExceedsSizeLimit,
        }
    }
}

impl From<&OrderState> for WireState {
    fn from(s: &OrderState) -> Self {
        match s {
            OrderState::New => Self::New,
            OrderState::Acknowledged { broker_id } => Self::Acknowledged {
                broker_id: broker_id.get(),
            },
            OrderState::PartiallyFilled {
                broker_id,
                filled,
                cost,
            } => Self::PartiallyFilled {
                broker_id: broker_id.get(),
                filled: filled.get(),
                cost: cost.raw(),
            },
            OrderState::Filled { filled, cost } => Self::Filled {
                filled: filled.get(),
                cost: cost.raw(),
            },
            OrderState::Cancelled { filled, cost } => Self::Cancelled {
                filled: filled.get(),
                cost: cost.raw(),
            },
            OrderState::Rejected { reason } => Self::Rejected {
                reason: (*reason).into(),
            },
        }
    }
}

impl From<&Order> for WireOrder {
    fn from(o: &Order) -> Self {
        Self {
            id: o.id.get(),
            participant: o.participant.to_string(),
            symbol: o.symbol.to_string(),
            side: o.side.into(),
            qty: o.qty.get(),
            limit_px: o.limit_px.raw(),
            state: (&o.state).into(),
            replaces: o.replaces.map(ClientOrderId::get),
            submitted_at: o.submitted_at.as_millis(),
        }
    }
}

impl From<&Event> for WireEvent {
    fn from(e: &Event) -> Self {
        match e {
            Event::ParticipantCreated {
                participant,
                starting_cash,
            } => Self::ParticipantCreated {
                participant: participant.to_string(),
                starting_cash: starting_cash.raw(),
            },
            Event::OrderSubmitted { order } => Self::OrderSubmitted {
                order: (&**order).into(),
            },
            Event::OrderAcknowledged { id, broker_id } => Self::OrderAcknowledged {
                id: id.get(),
                broker_id: broker_id.get(),
            },
            Event::OrderRejected { id, reason } => Self::OrderRejected {
                id: id.get(),
                reason: (*reason).into(),
            },
            Event::OrderFilled { id, qty, px, fee } => Self::OrderFilled {
                id: id.get(),
                qty: qty.get(),
                px: px.raw(),
                fee: fee.raw(),
            },
            Event::OrderCancelled { id } => Self::OrderCancelled { id: id.get() },
            Event::OrderReplaced {
                original,
                replacement,
            } => Self::OrderReplaced {
                original: original.get(),
                replacement: (&**replacement).into(),
            },
            Event::MarkUpdated { symbol, px } => Self::MarkUpdated {
                symbol: symbol.to_string(),
                px: px.raw(),
            },
        }
    }
}

// ---- wire -> domain ------------------------------------------------------

impl From<WireSide> for Side {
    fn from(s: WireSide) -> Self {
        match s {
            WireSide::Buy => Self::Buy,
            WireSide::Sell => Self::Sell,
        }
    }
}

impl From<WireReject> for RejectReason {
    fn from(r: WireReject) -> Self {
        match r {
            WireReject::UnknownSymbol => Self::UnknownSymbol,
            WireReject::InsufficientCash => Self::InsufficientCash,
            WireReject::InsufficientPosition => Self::InsufficientPosition,
            WireReject::ExceedsSizeLimit => Self::ExceedsSizeLimit,
        }
    }
}

impl TryFrom<WireState> for OrderState {
    type Error = DomainError;

    fn try_from(s: WireState) -> Result<Self, Self::Error> {
        Ok(match s {
            WireState::New => Self::New,
            WireState::Acknowledged { broker_id } => Self::Acknowledged {
                broker_id: BrokerOrderId::new(broker_id),
            },
            WireState::PartiallyFilled {
                broker_id,
                filled,
                cost,
            } => Self::PartiallyFilled {
                broker_id: BrokerOrderId::new(broker_id),
                filled: Qty::new(filled)?,
                cost: Money::from_raw(cost),
            },
            WireState::Filled { filled, cost } => Self::Filled {
                filled: Qty::new(filled)?,
                cost: Money::from_raw(cost),
            },
            WireState::Cancelled { filled, cost } => Self::Cancelled {
                filled: Qty::new(filled)?,
                cost: Money::from_raw(cost),
            },
            WireState::Rejected { reason } => Self::Rejected {
                reason: reason.into(),
            },
        })
    }
}

impl TryFrom<WireOrder> for Order {
    type Error = DomainError;

    fn try_from(o: WireOrder) -> Result<Self, Self::Error> {
        Ok(Self {
            id: ClientOrderId::new(o.id),
            participant: ParticipantId::parse(&o.participant)?,
            symbol: Symbol::parse(&o.symbol)?,
            side: o.side.into(),
            qty: Qty::new(o.qty)?,
            limit_px: Px::from_raw(o.limit_px)?,
            state: o.state.try_into()?,
            replaces: o.replaces.map(ClientOrderId::new),
            submitted_at: Timestamp::from_millis(o.submitted_at),
        })
    }
}

impl TryFrom<WireEvent> for Event {
    type Error = DomainError;

    fn try_from(e: WireEvent) -> Result<Self, Self::Error> {
        Ok(match e {
            WireEvent::ParticipantCreated {
                participant,
                starting_cash,
            } => Self::ParticipantCreated {
                participant: ParticipantId::parse(&participant)?,
                starting_cash: Money::from_raw(starting_cash),
            },
            WireEvent::OrderSubmitted { order } => Self::OrderSubmitted {
                order: Box::new(order.try_into()?),
            },
            WireEvent::OrderAcknowledged { id, broker_id } => Self::OrderAcknowledged {
                id: ClientOrderId::new(id),
                broker_id: BrokerOrderId::new(broker_id),
            },
            WireEvent::OrderRejected { id, reason } => Self::OrderRejected {
                id: ClientOrderId::new(id),
                reason: reason.into(),
            },
            WireEvent::OrderFilled { id, qty, px, fee } => Self::OrderFilled {
                id: ClientOrderId::new(id),
                qty: Qty::new(qty)?,
                px: Px::from_raw(px)?,
                fee: Money::from_raw(fee),
            },
            WireEvent::OrderCancelled { id } => Self::OrderCancelled {
                id: ClientOrderId::new(id),
            },
            WireEvent::OrderReplaced {
                original,
                replacement,
            } => Self::OrderReplaced {
                original: ClientOrderId::new(original),
                replacement: Box::new(replacement.try_into()?),
            },
            WireEvent::MarkUpdated { symbol, px } => Self::MarkUpdated {
                symbol: Symbol::parse(&symbol)?,
                px: Px::from_raw(px)?,
            },
        })
    }
}

/// Encode one journal entry's payload.
pub(crate) fn encode(event: &Event) -> Result<String, serde_json::Error> {
    serde_json::to_string(&WireEvent::from(event))
}

/// Decode a payload back into an event.
pub(crate) fn decode(payload: &str) -> Result<Event, crate::StoreError> {
    let wire: WireEvent = serde_json::from_str(payload)?;
    Ok(wire.try_into()?)
}

/// Rebuild a journal entry from its stored columns.
pub(crate) fn journaled(seq: u64, at: i64, payload: &str) -> Result<Journaled, crate::StoreError> {
    Ok(Journaled {
        seq,
        at: Timestamp::from_millis(at),
        event: decode(payload)?,
    })
}
