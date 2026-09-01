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
    response::Html,
    routing::{delete, get, post, put},
    Router,
};
use broker::MockBroker;
use domain::{ClientOrderId, InstrumentSpec, Timestamp};
use engine::{Command, Engine};
use store::{EventLog, SqliteLog};
use tokio::sync::Mutex;

pub use error::AppError;

type BrokerFactory = Box<dyn Fn() -> MockBroker + Send + Sync>;

pub struct App {
    engine: Engine<MockBroker>,
    log: SqliteLog,
    next_order_id: u64,
    /// Rebuilds the broker on reset — a fresh RNG stream and id counter, so a
    /// reset world is as deterministic as a booted one. Reusing the old broker
    /// would leak the previous world's randomness into the new one.
    make_broker: BrokerFactory,
    /// Instrument seeds re-applied (as fresh journalled events) after a reset,
    /// so reset behaves exactly like delete-the-file-and-reboot, self-served.
    seeds: Vec<InstrumentSpec>,
}

impl App {
    /// Open a log and rebuild state from it. An empty log gives an empty
    /// competition; a populated one continues exactly where it left off.
    pub fn open(
        log: SqliteLog,
        make_broker: impl Fn() -> MockBroker + Send + Sync + 'static,
        seeds: Vec<InstrumentSpec>,
    ) -> Result<Self, AppError> {
        let entries = log.read_all()?;
        let engine = Engine::replay(make_broker(), entries)?;
        let next_order_id = engine.next_order_id();
        let mut app = Self {
            engine,
            log,
            next_order_id,
            make_broker: Box::new(make_broker),
            seeds,
        };
        app.apply_seeds()?;
        Ok(app)
    }

    /// Seeds land only in an empty registry — the log is the authority, and
    /// neither a restart nor a reset may fight what the API has since edited.
    /// After a reset there is nothing to fight, so they land fresh.
    fn apply_seeds(&mut self) -> Result<(), AppError> {
        if self.engine.instruments().next().is_some() {
            return Ok(());
        }
        for spec in self.seeds.clone() {
            self.execute(now(), Command::CreateInstrument { spec })?;
        }
        Ok(())
    }

    /// Destroy the world and restore the boot state: empty log, reborn engine
    /// and broker, seeds re-applied as fresh events. Every participant, order,
    /// mark and closed day is gone. The log is append-only *within* a
    /// competition; the competition itself is the operator's to discard.
    pub fn reset(&mut self) -> Result<(), AppError> {
        self.log.clear()?;
        self.engine = Engine::new((self.make_broker)());
        self.next_order_id = 1;
        self.apply_seeds()
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

/// The contract, hand-written and served by the API itself. Hand-written
/// rather than derived (utoipa et al.) because the spec is a *decision
/// document* — descriptions carry the semantics (idempotent close, fail-closed
/// valuation, strict parsing) that no derive macro knows — and a test guards
/// its path set against this router so the two cannot drift silently.
const OPENAPI: &str = include_str!("../openapi.json");

/// Swagger UI shell. The UI assets load from a CDN — a dev-tool page, not the
/// API: the service itself has no external dependency, and `/docs` degrades to
/// "spec still readable at /openapi.json" when offline.
const DOCS: &str = r#"<!doctype html><html><head>
<meta charset="utf-8"><title>Paper Trading Challenge — API docs</title>
<link rel="stylesheet" href="https://unpkg.com/swagger-ui-dist@5/swagger-ui.css">
</head><body>
<div id="ui"></div>
<script src="https://unpkg.com/swagger-ui-dist@5/swagger-ui-bundle.js"></script>
<script>window.onload=()=>{SwaggerUIBundle({url:'/openapi.json',dom_id:'#ui',tryItOutEnabled:true,defaultModelsExpandDepth:0})}</script>
</body></html>"#;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(routes::health))
        .route("/reset", post(routes::reset))
        .route(
            "/openapi.json",
            get(|| async {
                (
                    [(axum::http::header::CONTENT_TYPE, "application/json")],
                    OPENAPI,
                )
            }),
        )
        .route("/docs", get(|| async { Html(DOCS) }))
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
        .route("/orders/{id}/executions", get(routes::order_executions))
        .route("/orders/{id}", delete(routes::cancel_order))
        .route("/orders/{id}", put(routes::replace_order))
        .route("/broker/executions", post(routes::execute_order))
        .route(
            "/instruments",
            get(routes::instruments).post(routes::create_instrument),
        )
        .route(
            "/instruments/{symbol}",
            get(routes::get_instrument)
                .put(routes::update_instrument)
                .delete(routes::remove_instrument),
        )
        .route(
            "/market/prices",
            post(routes::update_marks).get(routes::marks),
        )
        .route("/days/{day}/close", post(routes::close_day))
        .route("/days/{day}/leaderboard", get(routes::leaderboard))
        .route("/days", get(routes::closed_days))
        .route("/ladder", get(routes::ladder))
        .with_state(state)
}
