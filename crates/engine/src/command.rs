//! What a caller can ask for. A command is an **intent that may be refused**;
//! an [`crate::Event`] is a fact that already happened and cannot be.
//!
//! Timestamps are not in here — they are passed to
//! [`crate::Engine::execute`] by the caller, because nothing in this system
//! reads a clock (`.claude/principles.md` §6).

use domain::{ClientOrderId, Money, ParticipantId, Px, Qty, Side, Symbol};

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
}
