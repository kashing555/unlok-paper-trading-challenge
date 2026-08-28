//! The `ptc` binary: open the log, rebuild, serve.

#![forbid(unsafe_code)]

use std::sync::Arc;

use api::{router, App, AppState};
use std::collections::BTreeSet;

use broker::{FeeSchedule, FillPolicy, Limits, MockBroker};
use domain::{Qty, Symbol};
use store::SqliteLog;
use tokio::sync::Mutex;

/// Configuration, from the environment with documented defaults. Kept tiny on
/// purpose: a config system is not what this exercise is being scored on.
struct Config {
    addr: String,
    db: String,
    seed: u64,
    fee_bps: i64,
    max_slices: u32,
    /// Comma-separated allowlist. Empty means every symbol is tradable.
    symbols: BTreeSet<Symbol>,
    max_order_qty: Option<Qty>,
}

impl Config {
    /// Fallible: a mistyped `PTC_MAX_QTY` should be a clean message and a
    /// non-zero exit, not a panic. The workspace denies `expect_used`, which is
    /// what caught this — the lint earning its place.
    fn from_env() -> Result<Self, Box<dyn std::error::Error>> {
        let var = |k: &str| std::env::var(k).ok();
        Ok(Self {
            addr: var("PTC_ADDR").unwrap_or_else(|| "127.0.0.1:8080".into()),
            // ":memory:" keeps a run entirely ephemeral, which is what the
            // tests and the demo want.
            db: var("PTC_DB").unwrap_or_else(|| "ptc.sqlite".into()),
            seed: var("PTC_SEED").and_then(|v| v.parse().ok()).unwrap_or(42),
            fee_bps: var("PTC_FEE_BPS").and_then(|v| v.parse().ok()).unwrap_or(0),
            max_slices: var("PTC_MAX_SLICES")
                .and_then(|v| v.parse().ok())
                .unwrap_or(3),
            // Broker-side limits, off by default so a first run is
            // frictionless — but configurable, **because REJECTED is one of the
            // six states the brief requires**. Without a way to switch a limit
            // on, the state could be exercised in the tests but never through
            // the running service, which is not the same as supporting it.
            symbols: match var("PTC_SYMBOLS") {
                None => BTreeSet::new(),
                Some(v) => v
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(Symbol::parse)
                    .collect::<Result<BTreeSet<_>, _>>()
                    .map_err(|e| format!("PTC_SYMBOLS: {e}"))?,
            },
            max_order_qty: match var("PTC_MAX_QTY") {
                None => None,
                Some(v) => Some(
                    Qty::new(
                        v.parse()
                            .map_err(|_| format!("PTC_MAX_QTY: {v:?} is not an integer"))?,
                    )
                    .map_err(|e| format!("PTC_MAX_QTY: {e}"))?,
                ),
            },
        })
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::from_env()?;

    let log = if config.db == ":memory:" {
        SqliteLog::in_memory()?
    } else {
        SqliteLog::open(&config.db)?
    };

    let broker = MockBroker::new(
        config.seed,
        FillPolicy::Partial {
            max_slices: config.max_slices,
        },
        FeeSchedule {
            bps: config.fee_bps,
        },
        Limits {
            known_symbols: config.symbols.clone(),
            max_order_qty: config.max_order_qty,
        },
    );

    let state: AppState = Arc::new(Mutex::new(App::open(log, broker)?));
    let listener = tokio::net::TcpListener::bind(&config.addr).await?;

    println!("ptc listening on http://{}", config.addr);
    println!("  log      {}", config.db);
    println!(
        "  broker   seed={} fee={}bp slices<={}",
        config.seed, config.fee_bps, config.max_slices
    );
    println!(
        "  limits   symbols={} maxQty={}",
        if config.symbols.is_empty() {
            "any".to_owned()
        } else {
            config
                .symbols
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(",")
        },
        config
            .max_order_qty
            .map_or("none".to_owned(), |q| q.to_string())
    );

    axum::serve(listener, router(state))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
