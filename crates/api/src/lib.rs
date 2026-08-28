//! HTTP over the engine.
//!
//! **The single writer is one mutex around the whole application.** Every
//! command and every read takes it, so mutations form one total order and a
//! read never sees a half-applied command. `docs/design.md` §12 originally
//! proposed a channel-and-actor for this; the mutex delivers the same property
//! — the property being *ordering*, not throughput — with a fraction of the
//! machinery, and a competition's order volume is nowhere near a core. Building
//! the actor anyway would be the speculative complexity `principles.md` §7
//! declines. Noted in the README as a production delta.
//!
//! **Events are durable before they are visible.** `App::execute` plans, writes
//! to the log, and only then applies. Applying first would leave a state the
//! log cannot reproduce if the process died in between.

#![forbid(unsafe_code)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod dto;
mod error;
mod routes;

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    routing::{delete, get, post, put},
    Router,
};
use broker::MockBroker;
use domain::{ClientOrderId, Timestamp};
use engine::{Command, Engine};
use store::{EventLog, SqliteLog};
use tokio::sync::Mutex;

pub use error::AppError;

pub struct App {
    engine: Engine<MockBroker>,
    log: SqliteLog,
    next_order_id: u64,
}

impl App {
    /// Open a log and rebuild state from it. An empty log gives an empty
    /// competition; a populated one continues exactly where it left off.
    pub fn open(log: SqliteLog, broker: MockBroker) -> Result<Self, AppError> {
        let entries = log.read_all()?;
        let engine = Engine::replay(broker, entries)?;
        let next_order_id = engine.next_order_id();
        Ok(Self {
            engine,
            log,
            next_order_id,
        })
    }

    /// Write ahead: plan, persist, then apply.
    pub fn execute(&mut self, at: Timestamp, command: Command) -> Result<(), AppError> {
        let entries = self.engine.plan(at, command)?;
        self.log.append(&entries)?;
        self.engine.commit(&entries)?;
        Ok(())
    }

    /// Mint the next client order id.
    ///
    /// Server-side because the id is **ours** (FIX `ClOrdID`) — the client is
    /// not in a position to guarantee uniqueness. Resumed past the highest id
    /// in the log on startup, so a restart cannot re-issue an id that is
    /// already resting.
    pub fn mint_order_id(&mut self) -> ClientOrderId {
        let id = ClientOrderId::new(self.next_order_id);
        self.next_order_id += 1;
        id
    }

    pub const fn engine(&self) -> &Engine<MockBroker> {
        &self.engine
    }
}

pub type AppState = Arc<Mutex<App>>;

/// Wall clock, read **here and nowhere else**. `domain` and `engine` take
/// timestamps as arguments; the shell is the only layer allowed to know what
/// time it is (`principles.md` §6).
pub fn now() -> Timestamp {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
    Timestamp::from_millis(millis)
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(routes::health))
        .route(
            "/participants",
            post(routes::create_participant).get(routes::list_participants),
        )
        .route("/participants/{id}/portfolio", get(routes::portfolio))
        .route("/participants/{id}/orders", get(routes::participant_orders))
        .route(
            "/orders",
            post(routes::submit_order).get(routes::list_orders),
        )
        .route("/orders/{id}", get(routes::get_order))
        .route("/orders/{id}", delete(routes::cancel_order))
        .route("/orders/{id}", put(routes::replace_order))
        .route("/broker/executions", post(routes::execute_order))
        .route("/market/prices", post(routes::update_marks))
        .route("/days/{day}/close", post(routes::close_day))
        .route("/days/{day}/leaderboard", get(routes::leaderboard))
        .route("/days", get(routes::closed_days))
        .route("/ladder", get(routes::ladder))
        .with_state(state)
}
