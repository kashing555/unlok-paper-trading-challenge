//! Handlers. They translate and delegate — every decision is in `engine` or
//! `domain`, so there is nothing here worth unit-testing in isolation
//! (`docs/build-order.md` C1).

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use domain::{ClientOrderId, Money, ParticipantId, Px, Qty, Side, Symbol, TradingDay};
use engine::Command;
use serde_json::{json, Value};

use crate::{dto, now, AppError, AppState};

// ---- parsing at the edge -------------------------------------------------
// Strict: a malformed field is rejected, never repaired into a guess
// (`principles.md` §7 — Postel's law is declined on input paths).

fn participant(s: &str) -> Result<ParticipantId, AppError> {
    Ok(ParticipantId::parse(s)?)
}
fn symbol(s: &str) -> Result<Symbol, AppError> {
    Ok(Symbol::parse(s)?)
}
fn price(s: &str) -> Result<Px, AppError> {
    Ok(Px::parse(s)?)
}
fn money(s: &str) -> Result<Money, AppError> {
    Ok(Money::parse(s)?)
}
fn quantity(n: i64) -> Result<Qty, AppError> {
    Ok(Qty::new(n)?)
}
fn day(s: &str) -> Result<TradingDay, AppError> {
    Ok(TradingDay::parse(s)?)
}

fn side(s: &str) -> Result<Side, AppError> {
    match s {
        "buy" => Ok(Side::Buy),
        "sell" => Ok(Side::Sell),
        other => Err(AppError::BadRequest(format!(
            r#"side must be "buy" or "sell", got {other:?}"#
        ))),
    }
}

// ---- health --------------------------------------------------------------

pub async fn health(State(state): State<AppState>) -> Json<Value> {
    let app = state.lock().await;
    Json(json!({
        "status": "ok",
        "participants": app.engine().participants().count(),
        "orders": app.engine().orders().count(),
        "events": app.engine().seq(),
        "closedDays": app.engine().closed_days().count(),
    }))
}

// ---- participants --------------------------------------------------------

pub async fn create_participant(
    State(state): State<AppState>,
    Json(req): Json<dto::CreateParticipant>,
) -> Result<(StatusCode, Json<Value>), AppError> {
    let id = participant(&req.id)?;
    let starting_cash = money(&req.starting_cash)?;
    if starting_cash <= Money::ZERO {
        return Err(AppError::BadRequest(
            "startingCash must be greater than zero".into(),
        ));
    }

    let mut app = state.lock().await;
    app.execute(
        now(),
        Command::CreateParticipant {
            participant: id.clone(),
            starting_cash,
        },
    )?;
    Ok((
        StatusCode::CREATED,
        Json(json!({ "id": id.to_string(), "startingCash": starting_cash.to_string() })),
    ))
}

pub async fn list_participants(State(state): State<AppState>) -> Json<Value> {
    let app = state.lock().await;
    let ids: Vec<String> = app
        .engine()
        .participants()
        .map(|p| p.participant().to_string())
        .collect();
    Json(json!({ "participants": ids }))
}

pub async fn portfolio(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<dto::PortfolioView>, AppError> {
    let id = participant(&id)?;
    let app = state.lock().await;
    let engine = app.engine();

    let orders = engine
        .working_orders_of(&id)
        .map(|o| dto::order_view(o, engine.fee_of(o.id)))
        .collect();
    Ok(Json(dto::portfolio_view(
        engine.portfolio(&id)?,
        engine.marks(),
        orders,
    )))
}

pub async fn participant_orders(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    let id = participant(&id)?;
    let app = state.lock().await;
    let engine = app.engine();
    engine.portfolio(&id)?; // 404 for an unknown participant, not an empty list

    let orders: Vec<dto::OrderView> = engine
        .orders_of(&id)
        .map(|o| dto::order_view(o, engine.fee_of(o.id)))
        .collect();
    Ok(Json(json!({ "orders": orders })))
}

// ---- orders --------------------------------------------------------------

pub async fn submit_order(
    State(state): State<AppState>,
    Json(req): Json<dto::SubmitOrder>,
) -> Result<(StatusCode, Json<dto::OrderView>), AppError> {
    let participant = participant(&req.participant)?;
    let symbol = symbol(&req.symbol)?;
    let side = side(&req.side)?;
    let qty = quantity(req.qty)?;
    let limit_px = price(&req.limit_px)?;

    let mut app = state.lock().await;
    let id = app.mint_order_id();
    app.execute(
        now(),
        Command::SubmitOrder {
            id,
            participant,
            symbol,
            side,
            qty,
            limit_px,
        },
    )?;
    let engine = app.engine();
    Ok((
        StatusCode::CREATED,
        Json(dto::order_view(engine.order(id)?, engine.fee_of(id))),
    ))
}

pub async fn list_orders(State(state): State<AppState>) -> Json<Value> {
    let app = state.lock().await;
    let engine = app.engine();
    let orders: Vec<dto::OrderView> = engine
        .orders()
        .map(|o| dto::order_view(o, engine.fee_of(o.id)))
        .collect();
    Json(json!({ "orders": orders }))
}

pub async fn get_order(
    State(state): State<AppState>,
    Path(id): Path<u64>,
) -> Result<Json<dto::OrderView>, AppError> {
    let app = state.lock().await;
    let engine = app.engine();
    let id = ClientOrderId::new(id);
    Ok(Json(dto::order_view(engine.order(id)?, engine.fee_of(id))))
}

pub async fn cancel_order(
    State(state): State<AppState>,
    Path(id): Path<u64>,
) -> Result<Json<dto::OrderView>, AppError> {
    let id = ClientOrderId::new(id);
    let mut app = state.lock().await;
    app.execute(now(), Command::CancelOrder { id })?;
    let engine = app.engine();
    Ok(Json(dto::order_view(engine.order(id)?, engine.fee_of(id))))
}

/// Cancel-replace. Returns **both** sides, because a caller that saw only the
/// new order would not know what happened to the quantity already filled on the
/// old one.
pub async fn replace_order(
    State(state): State<AppState>,
    Path(id): Path<u64>,
    Json(req): Json<dto::ReplaceOrder>,
) -> Result<Json<Value>, AppError> {
    let id = ClientOrderId::new(id);
    let qty = quantity(req.qty)?;
    let limit_px = price(&req.limit_px)?;

    let mut app = state.lock().await;
    let replacement_id = app.mint_order_id();
    app.execute(
        now(),
        Command::ReplaceOrder {
            id,
            replacement_id,
            qty,
            limit_px,
        },
    )?;

    let engine = app.engine();
    let original = dto::order_view(engine.order(id)?, engine.fee_of(id));
    let replacement = dto::order_view(engine.order(replacement_id)?, engine.fee_of(replacement_id));
    Ok(Json(
        json!({ "original": original, "replacement": replacement }),
    ))
}

// ---- broker --------------------------------------------------------------

/// Generate an execution. With `qty` and `px` the caller chooses the terms;
/// with neither, the broker's seeded policy does. Supplying only one is
/// rejected rather than guessed at.
pub async fn execute_order(
    State(state): State<AppState>,
    Json(req): Json<dto::Execute>,
) -> Result<Json<dto::OrderView>, AppError> {
    let id = ClientOrderId::new(req.order_id);
    let command = match (req.qty, req.px.as_deref()) {
        (Some(qty), Some(px)) => Command::Execute {
            id,
            qty: quantity(qty)?,
            px: price(px)?,
        },
        (None, None) => Command::AutoExecute { id },
        _ => {
            return Err(AppError::BadRequest(
                "supply both qty and px, or neither to let the broker decide".into(),
            ))
        }
    };

    let mut app = state.lock().await;
    app.execute(now(), command)?;
    let engine = app.engine();
    Ok(Json(dto::order_view(engine.order(id)?, engine.fee_of(id))))
}

// ---- market data ---------------------------------------------------------

pub async fn update_marks(
    State(state): State<AppState>,
    Json(req): Json<Vec<dto::MarkUpdate>>,
) -> Result<Json<Value>, AppError> {
    // Parse every entry before applying any, so a batch with one malformed
    // price updates nothing. The boundary is parsing: past it, each mark is
    // its own command, so a store failure mid-batch could still land a prefix
    // — acceptable for marks (each is independently true), not for fills.
    let parsed = req
        .iter()
        .map(|m| Ok((symbol(&m.symbol)?, price(&m.px)?)))
        .collect::<Result<Vec<_>, AppError>>()?;

    let mut app = state.lock().await;
    for (symbol, px) in parsed {
        app.execute(now(), Command::UpdateMark { symbol, px })?;
    }
    Ok(Json(json!({ "updated": req.len() })))
}

// ---- competition ---------------------------------------------------------

pub async fn close_day(
    State(state): State<AppState>,
    Path(d): Path<String>,
) -> Result<Json<dto::LeaderboardView>, AppError> {
    let d = day(&d)?;
    let mut app = state.lock().await;
    // Idempotent: closing a closed day is a no-op that returns the published
    // board rather than recomputing it.
    app.execute(now(), Command::CloseDay { day: d })?;
    Ok(Json((&app.engine().leaderboard(d)?).into()))
}

pub async fn leaderboard(
    State(state): State<AppState>,
    Path(d): Path<String>,
) -> Result<Json<dto::LeaderboardView>, AppError> {
    let d = day(&d)?;
    let app = state.lock().await;
    Ok(Json((&app.engine().leaderboard(d)?).into()))
}

pub async fn closed_days(State(state): State<AppState>) -> Json<Value> {
    let app = state.lock().await;
    let days: Vec<String> = app.engine().closed_days().map(|d| d.to_string()).collect();
    Json(json!({ "closedDays": days }))
}

pub async fn ladder(State(state): State<AppState>) -> Result<Json<dto::LadderView>, AppError> {
    let app = state.lock().await;
    Ok(Json(dto::LadderView::new(&app.engine().ladder()?)))
}
