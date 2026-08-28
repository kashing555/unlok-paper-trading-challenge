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
    BrokerOrderId, ClientOrderId, DomainError, Money, NewOrder, ParticipantId, Px, Qty,
    RejectReason, Side, Symbol, Timestamp, TradingDay,
};
use engine::{Event, Journaled};
use scoring::DayInput;
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
pub(crate) struct WireDayInput {
    participant: String,
    closing_value: i64,
    prior_closing_value: i64,
    turnover: i64,
    active: bool,
}

/// Submission terms. **No state and no `replaces` field**: an order is always
/// `NEW` when submitted, and the replace link is the event's own `original`
/// field. Storing either would be storing a fact twice.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct WireOrder {
    id: u64,
    participant: String,
    symbol: String,
    side: WireSide,
    qty: i64,
    limit_px: i64,
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
    DayClosed {
        day: String,
        entries: Vec<WireDayInput>,
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

impl From<&NewOrder> for WireOrder {
    fn from(o: &NewOrder) -> Self {
        Self {
            id: o.id.get(),
            participant: o.participant.to_string(),
            symbol: o.symbol.to_string(),
            side: o.side.into(),
            qty: o.qty.get(),
            limit_px: o.limit_px.raw(),
            submitted_at: o.at.as_millis(),
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
                order: order.into(),
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
                replacement: replacement.into(),
            },
            Event::MarkUpdated { symbol, px } => Self::MarkUpdated {
                symbol: symbol.to_string(),
                px: px.raw(),
            },
            Event::DayClosed { day, entries } => Self::DayClosed {
                day: day.to_string(),
                entries: entries.iter().map(Into::into).collect(),
            },
        }
    }
}

impl From<&DayInput> for WireDayInput {
    fn from(d: &DayInput) -> Self {
        Self {
            participant: d.participant.to_string(),
            closing_value: d.closing_value.raw(),
            prior_closing_value: d.prior_closing_value.raw(),
            turnover: d.turnover.raw(),
            active: d.active,
        }
    }
}

impl TryFrom<WireDayInput> for DayInput {
    type Error = DomainError;

    fn try_from(d: WireDayInput) -> Result<Self, Self::Error> {
        Ok(Self {
            participant: ParticipantId::parse(&d.participant)?,
            closing_value: Money::from_raw(d.closing_value),
            prior_closing_value: Money::from_raw(d.prior_closing_value),
            turnover: Money::from_raw(d.turnover),
            active: d.active,
        })
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

impl TryFrom<WireOrder> for NewOrder {
    type Error = DomainError;

    fn try_from(o: WireOrder) -> Result<Self, Self::Error> {
        Ok(Self {
            id: ClientOrderId::new(o.id),
            participant: ParticipantId::parse(&o.participant)?,
            symbol: Symbol::parse(&o.symbol)?,
            side: o.side.into(),
            qty: Qty::new(o.qty)?,
            limit_px: Px::from_raw(o.limit_px)?,
            at: Timestamp::from_millis(o.submitted_at),
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
                order: order.try_into()?,
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
                replacement: replacement.try_into()?,
            },
            WireEvent::MarkUpdated { symbol, px } => Self::MarkUpdated {
                symbol: Symbol::parse(&symbol)?,
                px: Px::from_raw(px)?,
            },
            WireEvent::DayClosed { day, entries } => Self::DayClosed {
                day: TradingDay::parse(&day)?,
                entries: entries
                    .into_iter()
                    .map(TryInto::try_into)
                    .collect::<Result<_, DomainError>>()?,
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
