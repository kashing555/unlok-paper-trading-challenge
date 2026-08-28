//! Pure domain vocabulary for the paper trading competition.
//!
//! This crate has no async runtime, no HTTP, no SQL, no clock and no RNG — see
//! `.claude/principles.md` §2. Everything here is a value type or a total
//! function over value types, and every test in it runs in microseconds.
//!
//! The types are *parsed, not validated* (§3): constructors are fallible and
//! fields are private, so an invalid `Qty` or `Symbol` cannot be constructed at
//! all rather than being constructed and checked later.

#![forbid(unsafe_code)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

mod error;
mod ids;
mod money;
mod time;

pub use error::DomainError;
pub use ids::{BrokerOrderId, ClientOrderId, ParticipantId, Symbol};
pub use money::{Cash, Px, Qty, SCALE, SCALE_DIGITS};
pub use time::{Timestamp, TradingDay};
