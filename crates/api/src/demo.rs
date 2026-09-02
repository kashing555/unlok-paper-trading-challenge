//! `ptc demo` — a full competition, start to finish, in one command.
//!
//! Drives the **same application layer the HTTP API drives** (`api::App`), not
//! the API itself: no server, no ports, nothing to leave running. Every
//! timestamp is supplied rather than read from a clock and the broker is
//! seeded, so **the output is byte-identical on every run** and can be diffed —
//! CI runs it twice and does exactly that.

use std::error::Error;

use api::App;
use broker::{FeeSchedule, FillPolicy, MockBroker};
use domain::{ClientOrderId, Money, ParticipantId, Px, Qty, Side, Symbol, Timestamp, TradingDay};
use engine::Command;
use store::SqliteLog;

const CASH: &str = "100000";

/// A return as a percentage, rounded for display.
///
/// `{:.4}` on a `Decimal` **truncates** rather than rounds — 0.14169 would
/// print as 0.1416, which is wrong by a digit in the place a reader checks.
fn pct(r: rust_decimal::Decimal) -> rust_decimal::Decimal {
    (r * rust_decimal::Decimal::ONE_HUNDRED).round_dp(4)
}
const SEED: u64 = 20_260_829;
const FEE_BPS: i64 = 5;

struct Demo {
    app: App,
    clock: i64,
}

impl Demo {
    fn new() -> Result<Self, Box<dyn Error>> {
        let make_broker = || {
            MockBroker::new(
                SEED,
                FillPolicy::Partial { max_slices: 3 },
                FeeSchedule { bps: FEE_BPS },
            )
        };
        Ok(Self {
            app: App::open(SqliteLog::in_memory()?, make_broker, vec![])?,
            clock: 1_772_000_000_000,
        })
    }

    fn run(&mut self, command: Command) -> Result<(), Box<dyn Error>> {
        self.clock += 1_000;
        self.app
            .execute(Timestamp::from_millis(self.clock), command)?;
        Ok(())
    }

    fn create(&mut self, name: &str) -> Result<(), Box<dyn Error>> {
        self.run(Command::CreateParticipant {
            participant: ParticipantId::parse(name)?,
            starting_cash: Money::parse(CASH)?,
        })
    }

    fn submit(
        &mut self,
        who: &str,
        symbol: &str,
        side: Side,
        qty: i64,
        px: &str,
    ) -> Result<ClientOrderId, Box<dyn Error>> {
        let id = self.app.mint_order_id();
        self.run(Command::SubmitOrder {
            id,
            participant: ParticipantId::parse(who)?,
            symbol: Symbol::parse(symbol)?,
            side,
            qty: Qty::new(qty)?,
            limit_px: Px::parse(px)?,
        })?;
        println!("    submit  #{id} {who:<6} {side:?} {qty:>4} {symbol:<5} @ {px}");
        Ok(id)
    }

    fn auto_fill(&mut self, id: ClientOrderId) -> Result<(), Box<dyn Error>> {
        while !self.app.engine().order(id)?.state.is_terminal() {
            self.run(Command::AutoExecute { id })?;
            let o = self.app.engine().order(id)?;
            println!(
                "    fill    #{id} tid {} -> {:<14} {:>4}/{:<4} cost {} fees {}",
                self.app
                    .engine()
                    .executions_of(id)
                    .last()
                    .map_or(0, |r| r.exec_id.get()),
                o.state.name(),
                o.state.filled(),
                o.qty,
                o.state.cost(),
                self.app.engine().fee_of(id)
            );
        }
        Ok(())
    }

    fn fill_once(&mut self, id: ClientOrderId, qty: i64, px: &str) -> Result<(), Box<dyn Error>> {
        self.run(Command::Execute {
            id,
            qty: Qty::new(qty)?,
            px: Px::parse(px)?,
        })?;
        let o = self.app.engine().order(id)?;
        println!(
            "    fill    #{id} tid {} -> {:<14} {:>4}/{:<4} cost {} fees {}",
            self.app
                .engine()
                .executions_of(id)
                .last()
                .map_or(0, |r| r.exec_id.get()),
            o.state.name(),
            o.state.filled(),
            o.qty,
            o.state.cost(),
            self.app.engine().fee_of(id)
        );
        Ok(())
    }

    fn cancel(&mut self, id: ClientOrderId) -> Result<(), Box<dyn Error>> {
        self.run(Command::CancelOrder { id })?;
        let o = self.app.engine().order(id)?;
        println!(
            "    cancel  #{id} -> {} keeping {} filled",
            o.state.name(),
            o.state.filled()
        );
        Ok(())
    }

    fn replace(
        &mut self,
        id: ClientOrderId,
        qty: i64,
        px: &str,
    ) -> Result<ClientOrderId, Box<dyn Error>> {
        let replacement_id = self.app.mint_order_id();
        self.run(Command::ReplaceOrder {
            id,
            replacement_id,
            qty: Qty::new(qty)?,
            limit_px: Px::parse(px)?,
        })?;
        println!(
            "    replace #{id} -> {} keeping {} filled; new #{replacement_id} for {qty} @ {px}",
            self.app.engine().order(id)?.state.name(),
            self.app.engine().order(id)?.state.filled(),
        );
        Ok(replacement_id)
    }

    fn mark(&mut self, symbol: &str, px: &str) -> Result<(), Box<dyn Error>> {
        self.run(Command::UpdateMark {
            symbol: Symbol::parse(symbol)?,
            px: Px::parse(px)?,
        })
    }

    fn portfolios(&self) -> Result<(), Box<dyn Error>> {
        println!(
            "    {:<8} {:>12} {:>12} {:>12} {:>10} {:>14}",
            "who", "cash", "realized", "unrealized", "fees", "total value"
        );
        for p in self.app.engine().participants() {
            let marks = self.app.engine().marks();
            println!(
                "    {:<8} {:>12} {:>12} {:>12} {:>10} {:>14}",
                p.participant().to_string(),
                p.cash().to_string(),
                p.realized_pnl().to_string(),
                p.unrealized_pnl(marks)?.to_string(),
                p.fees_paid().to_string(),
                p.total_value(marks)?.to_string(),
            );
            for pos in p.positions() {
                println!(
                    "      holds {:<5} {:>5} @ avg {}",
                    pos.symbol().to_string(),
                    pos.qty().to_string(),
                    pos.avg_cost().map_or("-".into(), |a| a.to_string()),
                );
            }
        }
        Ok(())
    }

    fn close(&mut self, day: &str) -> Result<(), Box<dyn Error>> {
        let d = TradingDay::parse(day)?;
        self.run(Command::CloseDay { day: d })?;
        let board = self.app.engine().leaderboard(d)?;

        println!("\n  LEADERBOARD {day}   (return % desc, then turnover asc, then id)");
        println!(
            "    {:<5} {:<8} {:>14} {:>12} {:>11} {:>12}  active",
            "rank", "who", "closing value", "daily P&L", "return", "turnover"
        );
        for row in &board.rows {
            let r = &row.result;
            println!(
                "    {:<5} {:<8} {:>14} {:>12} {:>10.4}% {:>12}  {}",
                row.rank,
                r.participant.to_string(),
                r.closing_value.to_string(),
                r.daily_pnl.to_string(),
                pct(r.daily_return),
                r.turnover.to_string(),
                if r.active { "yes" } else { "no" },
            );
        }
        Ok(())
    }

    fn ladder(&self) -> Result<(), Box<dyn Error>> {
        println!("\n  OVERALL LADDER   (geometric compound of daily returns)");
        println!(
            "    {:<5} {:<8} {:>12} {:>6} {:>7} {:>7}  eligible",
            "rank", "who", "cumulative", "wins", "active", "points"
        );
        for row in self.app.engine().ladder()? {
            println!(
                "    {:<5} {:<8} {:>11.4}% {:>6} {:>7} {:>7}  {}",
                row.rank.map_or("-".into(), |r| r.to_string()),
                row.participant.to_string(),
                pct(row.cumulative_return),
                row.daily_wins,
                row.active_days,
                row.points,
                if row.eligible {
                    "yes"
                } else {
                    "no (never traded)"
                },
            );
        }
        Ok(())
    }
}

pub fn run() -> Result<(), Box<dyn Error>> {
    let mut d = Demo::new()?;

    println!("PAPER TRADING COMPETITION — scripted demo");
    println!("  seed {SEED}, fees {FEE_BPS}bp, fixed clock: this output is reproducible\n");

    println!("SETUP  three participants, {CASH} each");
    for who in ["alice", "bob", "carol"] {
        d.create(who)?;
    }
    println!("    carol is created and never trades — watch the ladder at the end.\n");

    // ---- day one ---------------------------------------------------------
    println!("DAY 2026-08-28");
    let a1 = d.submit("alice", "AAPL", Side::Buy, 100, "10")?;
    d.fill_once(a1, 40, "10")?;
    d.cancel(a1)?;
    println!("      ^ cancelled after a partial: the 40 that executed stay booked.\n");

    let a2 = d.submit("alice", "MSFT", Side::Buy, 50, "20")?;
    d.auto_fill(a2)?;
    println!();

    let b1 = d.submit("bob", "AAPL", Side::Buy, 200, "12")?;
    let b2 = d.replace(b1, 100, "11")?;
    d.auto_fill(b2)?;
    println!();

    d.mark("AAPL", "11")?;
    d.mark("MSFT", "21")?;
    d.portfolios()?;
    d.close("2026-08-28")?;

    // ---- day two ---------------------------------------------------------
    println!("\nDAY 2026-08-29");
    let a3 = d.submit("alice", "AAPL", Side::Sell, 20, "11")?;
    d.auto_fill(a3)?;
    println!();

    d.mark("AAPL", "10.5000")?;
    d.mark("MSFT", "22.2500")?;
    d.portfolios()?;
    d.close("2026-08-29")?;

    d.ladder()?;
    println!("\n  carol never placed an order, so she is listed but unranked.");
    println!("  Staying flat on a day is a strategy and ranks; never competing is not.");
    println!("  See docs/ranking.md for the full argument.\n");
    Ok(())
}
