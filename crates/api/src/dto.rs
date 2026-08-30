//! Request and response shapes.
//!
//! **These are the reason `domain` has no serde derives.** The wire format is a
//! contract with the outside world; deriving it onto domain types would couple
//! the two and turn an internal rename into a breaking API change
//! (`.claude/rust.md`). The `From` impls below are the seam, and they are the
//! only place the JSON shape is decided.
//!
//! **Money and returns cross as decimal strings, never JSON numbers.** A JSON
//! number is an IEEE double; `0.1 + 0.2` on the far side is where a cent goes
//! missing, and this is the boundary where that would become permanent.

use domain::{Order, Portfolio, Position};
use scoring::{LadderRow, Leaderboard};
use serde::{Deserialize, Serialize};

// ---- requests ------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateParticipant {
    pub id: String,
    pub starting_cash: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubmitOrder {
    pub participant: String,
    pub symbol: String,
    /// `"buy"` or `"sell"`.
    pub side: String,
    pub qty: i64,
    pub limit_px: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReplaceOrder {
    pub qty: i64,
    pub limit_px: String,
}

/// Drive an execution. Omit both fields to let the broker's seeded policy
/// choose the terms — the brief's "generate mock executions", either way.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Execute {
    pub order_id: u64,
    #[serde(default)]
    pub qty: Option<i64>,
    #[serde(default)]
    pub px: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MarkUpdate {
    pub symbol: String,
    pub px: String,
}

// ---- responses -----------------------------------------------------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderView {
    pub id: u64,
    pub participant: String,
    pub symbol: String,
    pub side: &'static str,
    pub qty: i64,
    pub limit_px: String,
    pub state: &'static str,
    pub filled_qty: i64,
    /// Gross notional executed, fees excluded — the lifecycle tracks what
    /// executed, not what it cost to execute.
    pub filled_cost: String,
    /// Fees accrued on this order across its fills — the engine's projection
    /// over the same events, reported beside the gross figure, never inside it.
    pub fees: String,
    pub remaining_qty: i64,
    pub broker_order_id: Option<u64>,
    /// The order this one replaced. FIX `OrigClOrdID`.
    pub replaces: Option<u64>,
    pub submitted_at: i64,
}

/// Built with the engine-held fee figure — an explicit argument rather than a
/// `From` impl, so no call site can forget it and silently render zero.
pub fn order_view(o: &Order, fees: domain::Money) -> OrderView {
    OrderView {
        id: o.id.get(),
        participant: o.participant.to_string(),
        symbol: o.symbol.to_string(),
        side: match o.side {
            domain::Side::Buy => "buy",
            domain::Side::Sell => "sell",
        },
        qty: o.qty.get(),
        limit_px: o.limit_px.to_string(),
        state: o.state.name(),
        filled_qty: o.state.filled().get(),
        filled_cost: o.state.cost().to_string(),
        fees: fees.to_string(),
        remaining_qty: o.remaining().map_or(0, |q| q.get()),
        broker_order_id: o.state.broker_id().map(domain::BrokerOrderId::get),
        replaces: o.replaces.map(domain::ClientOrderId::get),
        submitted_at: o.submitted_at.as_millis(),
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PositionView {
    pub symbol: String,
    pub qty: i64,
    /// The brief's "average position price". Derived from cost and quantity for
    /// display; never stored, so it cannot drift.
    pub avg_price: Option<String>,
    pub cost_basis: String,
    pub mark: Option<String>,
    pub market_value: Option<String>,
    pub unrealized_pnl: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortfolioView {
    pub participant: String,
    pub starting_cash: String,
    pub cash: String,
    pub realized_pnl: String,
    /// Separate from the basis it was capitalised into — the record keeps
    /// price and fee apart, so the report does too.
    pub fees_paid: String,
    /// `null` when a held symbol has no mark. **Fail closed means never a wrong
    /// number, not never a response** — the read succeeds, the valuation does
    /// not, and `valuationError` says which symbol is missing.
    pub unrealized_pnl: Option<String>,
    pub total_value: Option<String>,
    pub valuation_error: Option<String>,
    pub positions: Vec<PositionView>,
    pub active_orders: Vec<OrderView>,
}

pub fn portfolio_view(
    portfolio: &Portfolio,
    marks: &domain::Marks,
    active_orders: Vec<OrderView>,
) -> PortfolioView {
    let positions = portfolio
        .positions()
        .map(|p| position_view(p, marks))
        .collect();
    let valuation = portfolio
        .unrealized_pnl(marks)
        .and_then(|u| Ok((u, portfolio.total_value(marks)?)));

    let (unrealized_pnl, total_value, valuation_error) = match valuation {
        Ok((u, t)) => (Some(u.to_string()), Some(t.to_string()), None),
        Err(e) => (None, None, Some(e.to_string())),
    };

    PortfolioView {
        participant: portfolio.participant().to_string(),
        starting_cash: portfolio.starting_cash().to_string(),
        cash: portfolio.cash().to_string(),
        realized_pnl: portfolio.realized_pnl().to_string(),
        fees_paid: portfolio.fees_paid().to_string(),
        unrealized_pnl,
        total_value,
        valuation_error,
        positions,
        active_orders,
    }
}

fn position_view(p: &Position, marks: &domain::Marks) -> PositionView {
    let mark = marks.get(p.symbol());
    PositionView {
        symbol: p.symbol().to_string(),
        qty: p.qty().get(),
        avg_price: p.avg_cost().map(|a| a.to_string()),
        cost_basis: p.cost().to_string(),
        mark: mark.map(|m| m.to_string()),
        market_value: mark
            .and_then(|m| p.market_value(m).ok())
            .map(|v| v.to_string()),
        unrealized_pnl: mark
            .and_then(|m| p.unrealized(m).ok())
            .map(|v| v.to_string()),
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LeaderboardRowView {
    pub rank: u32,
    pub participant: String,
    pub closing_value: String,
    pub daily_pnl: String,
    /// A fraction, not a percentage: `0.02` is +2%.
    pub daily_return: String,
    pub turnover: String,
    pub active: bool,
    pub bust: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LeaderboardView {
    pub day: String,
    /// Stated in the payload so a consumer never has to infer the rules.
    pub ranked_by: &'static str,
    pub tiebreaks: [&'static str; 2],
    pub rows: Vec<LeaderboardRowView>,
}

impl From<&Leaderboard> for LeaderboardView {
    fn from(b: &Leaderboard) -> Self {
        Self {
            day: b.day.to_string(),
            ranked_by: "dailyReturn descending",
            tiebreaks: ["turnover ascending", "participantId ascending"],
            rows: b
                .rows
                .iter()
                .map(|r| LeaderboardRowView {
                    rank: r.rank,
                    participant: r.result.participant.to_string(),
                    closing_value: r.result.closing_value.to_string(),
                    daily_pnl: r.result.daily_pnl.to_string(),
                    daily_return: r.result.daily_return.to_string(),
                    turnover: r.result.turnover.to_string(),
                    active: r.result.active,
                    bust: r.result.bust,
                })
                .collect(),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LadderRowView {
    /// `null` for a participant who has never been active — listed, but not
    /// placed. See `docs/ranking.md` §4.
    pub rank: Option<u32>,
    pub participant: String,
    pub cumulative_return: String,
    pub daily_wins: u32,
    pub active_days: u32,
    /// Formula-1 style points. A **secondary view**, never the sort key.
    pub points: u32,
    pub eligible: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LadderView {
    pub ranked_by: &'static str,
    pub tiebreaks: [&'static str; 3],
    pub eligibility: &'static str,
    pub rows: Vec<LadderRowView>,
}

impl LadderView {
    pub fn new(rows: &[LadderRow]) -> Self {
        Self {
            ranked_by: "cumulativeReturn descending (geometric compound of daily returns)",
            tiebreaks: [
                "dailyWins descending",
                "activeDays descending",
                "participantId ascending",
            ],
            eligibility:
                "at least one active day; ineligible participants are listed with rank null",
            rows: rows
                .iter()
                .map(|r| LadderRowView {
                    rank: r.rank,
                    participant: r.participant.to_string(),
                    cumulative_return: r.cumulative_return.to_string(),
                    daily_wins: r.daily_wins,
                    active_days: r.active_days,
                    points: r.points,
                    eligible: r.eligible,
                })
                .collect(),
        }
    }
}
