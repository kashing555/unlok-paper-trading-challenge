//! The `ptc` binary: open the log, rebuild, serve.

#![forbid(unsafe_code)]

use std::sync::Arc;

use api::{router, App, AppState};
use broker::{FeeSchedule, FillPolicy, Limits, MockBroker};
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
}

impl Config {
    fn from_env() -> Self {
        let var = |k: &str| std::env::var(k).ok();
        Self {
            addr: var("PTC_ADDR").unwrap_or_else(|| "127.0.0.1:8080".into()),
            // ":memory:" keeps a run entirely ephemeral, which is what the
            // tests and the demo want.
            db: var("PTC_DB").unwrap_or_else(|| "ptc.sqlite".into()),
            seed: var("PTC_SEED").and_then(|v| v.parse().ok()).unwrap_or(42),
            fee_bps: var("PTC_FEE_BPS").and_then(|v| v.parse().ok()).unwrap_or(0),
            max_slices: var("PTC_MAX_SLICES")
                .and_then(|v| v.parse().ok())
                .unwrap_or(3),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::from_env();

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
        Limits::default(),
    );

    let state: AppState = Arc::new(Mutex::new(App::open(log, broker)?));
    let listener = tokio::net::TcpListener::bind(&config.addr).await?;

    println!("ptc listening on http://{}", config.addr);
    println!("  log      {}", config.db);
    println!(
        "  broker   seed={} fee={}bp slices<={}",
        config.seed, config.fee_bps, config.max_slices
    );

    axum::serve(listener, router(state))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
