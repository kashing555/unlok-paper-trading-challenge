//! What a caller can ask for. A command is an **intent that may be refused**;
//! an [`crate::Event`] is a fact that already happened and cannot be.
//!
//! Timestamps are not in here — they are passed to
//! [`crate::Engine::execute`] by the caller, because nothing in this system
//! reads a clock (`.claude/principles.md` §6).

use domain::{
    ClientOrderId, InstrumentSpec, Money, ParticipantId, Px, Qty, Side, Symbol, TradingDay,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    CreateParticipant {
        participant: ParticipantId,
        starting_cash: Money,
    },
    SubmitOrder {
        id: ClientOrderId,
        participant: ParticipantId,
        symbol: Symbol,
        side: Side,
        qty: Qty,
        limit_px: Px,
    },
    CancelOrder {
        id: ClientOrderId,
    },
    ReplaceOrder {
        id: ClientOrderId,
        replacement_id: ClientOrderId,
        qty: Qty,
        limit_px: Px,
    },
    /// An execution report with terms the caller chose — the brief's "generate
    /// mock executions", driven explicitly.
    Execute {
        id: ClientOrderId,
        qty: Qty,
        px: Px,
    },
    /// Let the broker's seeded policy choose the terms instead.
    AutoExecute {
        id: ClientOrderId,
    },
    UpdateMark {
        symbol: Symbol,
        px: Px,
    },
    /// Register a tradable instrument. While the registry is empty every
    /// well-formed symbol trades on permissive defaults; the first
    /// registration switches the venue to allowlist mode.
    CreateInstrument {
        spec: InstrumentSpec,
    },
    /// Replace an instrument's spec. Working orders were validated against the
    /// spec at submission and are not retroactively re-checked — the venue
    /// changed its rules, it did not unwind your order.
    UpdateInstrument {
        spec: InstrumentSpec,
    },
    /// Delist. Refused while any working order or open position exists on the
    /// symbol: delisting must not strand what it cannot unwind.
    RemoveInstrument {
        symbol: Symbol,
    },
    /// Snapshot the day and publish its results. Idempotent: closing an
    /// already-closed day produces no events and changes nothing, because a
    /// published ranking that silently recomputes is worse than a stale one.
    CloseDay {
        day: TradingDay,
    },
}
