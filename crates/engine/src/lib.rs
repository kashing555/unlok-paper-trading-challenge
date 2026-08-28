//! Command → decide → events → apply, over the pure domain.
//!
//! This crate is the imperative shell (`.claude/principles.md` §2): it owns
//! state and sequencing, and every decision it makes is a call into `domain`
//! or `broker`. It has no HTTP, no database and no clock — timestamps arrive as
//! arguments.

#![forbid(unsafe_code)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

mod command;
mod engine;
mod event;

pub use command::Command;
pub use engine::{Engine, EngineError};
pub use event::{Event, Journaled};
