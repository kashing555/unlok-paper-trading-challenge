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

/// Operator-only, destructive, deliberately blunt: back to the boot world.
/// Same broker seed, same instrument seeds — so a reset world replays the
/// walkthrough with identical numbers, which is the whole point of having it.
pub async fn reset(State(state): State<AppState>) -> Result<Json<Value>, AppError> {
    let mut app = state.lock().await;
    app.reset()?;
    let engine = app.engine();
    Ok(Json(json!({
        "status": "reset",
        "instruments": engine.instruments().count(),
        "participants": engine.participants().count(),
        "events": engine.seq(),
    })))
}

/// The journal, narrated: every event as a human sentence, so acting through
/// Swagger (or anything else) is watchable from the cockpit. This is not a
/// second log — it IS the log, read back with words on.
pub async fn events(
    State(state): State<AppState>,
    axum::extract::Query(q): axum::extract::Query<dto::EventsQuery>,
) -> Result<Json<Value>, AppError> {
    use engine::Event as E;
    let app = state.lock().await;
    let engine = app.engine();
    let rows: Vec<Value> = app
        .events_after(q.after)?
        .into_iter()
        .map(|j| {
            let summary = match &j.event {
                E::ParticipantCreated {
                    participant,
                    starting_cash,
                } => format!("participant {participant} created with {starting_cash}"),
                E::OrderSubmitted { order } => format!(
                    "cloid {}: {} {} {} {} @ {} submitted",
                    order.id,
                    order.participant,
                    match order.side {
                        Side::Buy => "buy",
                        Side::Sell => "sell",
                    },
                    order.qty,
                    order.symbol,
                    order.limit_px
                ),
                E::OrderAcknowledged { id, broker_id } => {
                    format!("cloid {id} acknowledged → oid {broker_id}")
                }
                E::OrderRejected { id, reason } => {
                    format!("cloid {id} REJECTED ({reason:?})")
                }
                E::OrderFilled {
                    id,
                    exec_id,
                    qty,
                    px,
                    fee,
                } => match engine.order(*id) {
                    Ok(o) => format!(
                        "tid {exec_id}: fill {qty} {} @ {px} on cloid {id} ({}), fee {fee}",
                        o.symbol, o.participant
                    ),
                    Err(_) => format!("tid {exec_id}: fill {qty} @ {px} on cloid {id}, fee {fee}"),
                },
                E::OrderCancelled { id } => format!("cloid {id} cancelled"),
                E::OrderReplaced {
                    original,
                    replacement,
                } => format!(
                    "cloid {original} replaced → cloid {}: {} @ {}",
                    replacement.id, replacement.qty, replacement.limit_px
                ),
                E::MarkUpdated { symbol, px } => format!("mark {symbol} → {px}"),
                E::InstrumentUpserted { spec } => format!(
                    "instrument {} listed: tick {}, lot {}",
                    spec.symbol, spec.tick, spec.lot
                ),
                E::InstrumentRemoved { symbol } => format!("instrument {symbol} delisted"),
                E::DayClosed { day, entries } => {
                    format!("day {day} closed — {} participants ranked", entries.len())
                }
            };
            json!({
                "seq": j.seq,
                "at": j.at.as_millis(),
                "kind": j.event.kind(),
                "summary": summary,
            })
        })
        .collect();
    Ok(Json(json!({ "events": rows })))
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
        .map(|o| dto::order_view(o, engine.fee_of(o.id), engine.broker_id_of(o.id)))
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
        .map(|o| dto::order_view(o, engine.fee_of(o.id), engine.broker_id_of(o.id)))
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
        Json(dto::order_view(
            engine.order(id)?,
            engine.fee_of(id),
            engine.broker_id_of(id),
        )),
    ))
}

pub async fn list_orders(State(state): State<AppState>) -> Json<Value> {
    let app = state.lock().await;
    let engine = app.engine();
    let orders: Vec<dto::OrderView> = engine
        .orders()
        .map(|o| dto::order_view(o, engine.fee_of(o.id), engine.broker_id_of(o.id)))
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
    Ok(Json(dto::order_view(
        engine.order(id)?,
        engine.fee_of(id),
        engine.broker_id_of(id),
    )))
}

pub async fn cancel_order(
    State(state): State<AppState>,
    Path(id): Path<u64>,
) -> Result<Json<dto::OrderView>, AppError> {
    let id = ClientOrderId::new(id);
    let mut app = state.lock().await;
    app.execute(now(), Command::CancelOrder { id })?;
    let engine = app.engine();
    Ok(Json(dto::order_view(
        engine.order(id)?,
        engine.fee_of(id),
        engine.broker_id_of(id),
    )))
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
    let original = dto::order_view(
        engine.order(id)?,
        engine.fee_of(id),
        engine.broker_id_of(id),
    );
    let replacement = dto::order_view(
        engine.order(replacement_id)?,
        engine.fee_of(replacement_id),
        engine.broker_id_of(replacement_id),
    );
    Ok(Json(
        json!({ "original": original, "replacement": replacement }),
    ))
}

/// The tape: every execution in the world, ExecID order (= chronological).
/// Keyed by the execution id — the brokerOrderId names the *order* at the
/// venue and is shared by all its fills; the ExecID names *each fill*, which
/// is what a blotter is a list of. cloid and oid ride along, so every row
/// carries the complete id trio.
pub async fn executions(State(state): State<AppState>) -> Json<Value> {
    let app = state.lock().await;
    let engine = app.engine();
    let rows: Vec<Value> = engine
        .all_executions()
        .into_iter()
        .filter_map(|(cloid, r)| {
            let order = engine.order(cloid).ok()?;
            Some(json!({
                "execId": r.exec_id.get(),
                "clientOrderId": cloid.get(),
                "brokerOrderId": engine.broker_id_of(cloid).map(domain::BrokerOrderId::get),
                "participant": order.participant.to_string(),
                "symbol": order.symbol.to_string(),
                "side": match order.side { Side::Buy => "buy", Side::Sell => "sell" },
                "qty": r.qty.get(),
                "px": r.px.to_string(),
                "fee": r.fee.to_string(),
            }))
        })
        .collect();
    Json(json!({ "executions": rows }))
}

/// The trade blotter for one order: every execution with the venue's ExecID
/// (FIX 17) — the third id of the trio, so each fill is addressable, exactly
/// as a dispute or a dedup would need it.
pub async fn order_executions(
    State(state): State<AppState>,
    Path(id): Path<u64>,
) -> Result<Json<Value>, AppError> {
    let id = ClientOrderId::new(id);
    let app = state.lock().await;
    let engine = app.engine();
    engine.order(id)?; // 404 for an unknown order, not an empty list
    let executions: Vec<Value> = engine
        .executions_of(id)
        .iter()
        .map(|row| {
            json!({
                "execId": row.exec_id.get(),
                "qty": row.qty.get(),
                "px": row.px.to_string(),
                "fee": row.fee.to_string(),
            })
        })
        .collect();
    Ok(Json(
        json!({ "clientOrderId": id.get(), "executions": executions }),
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
    let id = ClientOrderId::new(req.client_order_id);
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
    Ok(Json(dto::order_view(
        engine.order(id)?,
        engine.fee_of(id),
        engine.broker_id_of(id),
    )))
}

// ---- reference data ------------------------------------------------------

fn instrument_json(spec: &domain::InstrumentSpec) -> Value {
    json!({
        "symbol": spec.symbol.to_string(),
        "tick": spec.tick.to_string(),
        "lot": spec.lot.get(),
        "maxOrderQty": spec.max_order_qty.map(domain::Qty::get),
    })
}

fn parse_spec(sym: &str, body: &dto::InstrumentBody) -> Result<domain::InstrumentSpec, AppError> {
    Ok(domain::InstrumentSpec::new(
        symbol(sym)?,
        price(&body.tick)?,
        quantity(body.lot.unwrap_or(1))?,
        body.max_order_qty.map(quantity).transpose()?,
    )?)
}

/// The security master — the venue's reference data, so a client discovers
/// what it may trade by asking, not by being rejected. An empty list means
/// unrestricted: any well-formed symbol on permissive grids (tick 0.0001,
/// lot 1).
pub async fn instruments(State(state): State<AppState>) -> Json<Value> {
    let app = state.lock().await;
    let instruments: Vec<Value> = app.engine().instruments().map(instrument_json).collect();
    Json(json!({ "instruments": instruments }))
}

pub async fn get_instrument(
    State(state): State<AppState>,
    Path(sym): Path<String>,
) -> Result<Json<Value>, AppError> {
    let sym = symbol(&sym)?;
    let app = state.lock().await;
    app.engine()
        .instrument(&sym)
        .map(instrument_json)
        .map(Json)
        .ok_or(AppError::Engine(engine::EngineError::UnknownInstrument(
            sym,
        )))
}

pub async fn create_instrument(
    State(state): State<AppState>,
    Json(body): Json<dto::CreateInstrument>,
) -> Result<(StatusCode, Json<Value>), AppError> {
    let spec = parse_spec(&body.symbol, &body.spec)?;
    let mut app = state.lock().await;
    app.execute(now(), Command::CreateInstrument { spec: spec.clone() })?;
    Ok((StatusCode::CREATED, Json(instrument_json(&spec))))
}

pub async fn update_instrument(
    State(state): State<AppState>,
    Path(sym): Path<String>,
    Json(body): Json<dto::InstrumentBody>,
) -> Result<Json<Value>, AppError> {
    let spec = parse_spec(&sym, &body)?;
    let mut app = state.lock().await;
    app.execute(now(), Command::UpdateInstrument { spec: spec.clone() })?;
    Ok(Json(instrument_json(&spec)))
}

pub async fn remove_instrument(
    State(state): State<AppState>,
    Path(sym): Path<String>,
) -> Result<StatusCode, AppError> {
    let sym = symbol(&sym)?;
    let mut app = state.lock().await;
    app.execute(now(), Command::RemoveInstrument { symbol: sym })?;
    Ok(StatusCode::NO_CONTENT)
}

/// Current marks, readable — the counterpart of POSTing them, so a tester can
/// verify what the market is currently saying without opening a position.
pub async fn marks(State(state): State<AppState>) -> Json<Value> {
    let app = state.lock().await;
    let marks: Vec<Value> = app
        .engine()
        .marks()
        .iter()
        .map(|(symbol, px)| json!({ "symbol": symbol.to_string(), "px": px.to_string() }))
        .collect();
    Json(json!({ "marks": marks }))
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
