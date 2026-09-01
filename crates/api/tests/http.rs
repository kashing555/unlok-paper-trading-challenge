//! Stage C1's close condition: every endpoint reachable, and an illegal
//! transition answered with `409` **carrying the current state** — the caller
//! needs to know what it actually is, not merely that the request failed.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use api::{router, App, AppState};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use broker::MockBroker;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use store::SqliteLog;
use tokio::sync::Mutex;
use tower::ServiceExt;

fn app() -> AppState {
    Arc::new(Mutex::new(
        App::open(
            SqliteLog::in_memory().unwrap(),
            || MockBroker::simple(7),
            vec![],
        )
        .unwrap(),
    ))
}

async fn call(
    state: &AppState,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let request = Request::builder().method(method).uri(uri);
    let request = match body {
        Some(b) => request
            .header("content-type", "application/json")
            .body(Body::from(b.to_string()))
            .unwrap(),
        None => request.body(Body::empty()).unwrap(),
    };

    let response = router(state.clone()).oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, value)
}

async fn get(state: &AppState, uri: &str) -> (StatusCode, Value) {
    call(state, "GET", uri, None).await
}
async fn post(state: &AppState, uri: &str, body: Value) -> (StatusCode, Value) {
    call(state, "POST", uri, Some(body)).await
}

async fn create(state: &AppState, id: &str, cash: &str) {
    let (s, _) = post(
        state,
        "/participants",
        json!({"id": id, "startingCash": cash}),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED);
}

async fn submit(state: &AppState, who: &str, side: &str, qty: i64, px: &str) -> u64 {
    let (s, body) = post(
        state,
        "/orders",
        json!({"participant": who, "symbol": "AAPL", "side": side, "qty": qty, "limitPx": px}),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{body}");
    body["clientOrderId"].as_u64().unwrap()
}

async fn mark(state: &AppState, px: &str) {
    let (s, _) = post(
        state,
        "/market/prices",
        json!([{"symbol": "AAPL", "px": px}]),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
}

#[tokio::test]
async fn a_competition_runs_end_to_end_over_http() {
    let state = app();
    create(&state, "alice", "100000").await;
    create(&state, "bob", "100000").await;

    // alice buys 100 @ 10 and it is filled by the broker's policy.
    let id = submit(&state, "alice", "buy", 100, "10").await;
    let (s, order) = post(&state, "/broker/executions", json!({"clientOrderId": id})).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(order["state"], "FILLED");
    assert_eq!(order["filledQty"], 100);
    assert_eq!(order["filledCost"], "1000.0000");
    assert_eq!(order["fees"], "0.0000");
    assert!(
        order["brokerOrderId"].is_null(),
        "terminal orders drop the broker id"
    );

    mark(&state, "12").await;

    let (s, p) = get(&state, "/participants/alice/portfolio").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(p["cash"], "99000.0000");
    assert_eq!(p["feesPaid"], "0.0000");
    assert_eq!(p["positions"][0]["qty"], 100);
    assert_eq!(p["positions"][0]["avgPrice"], "10.0000");
    assert_eq!(p["unrealizedPnl"], "200.0000");
    assert_eq!(p["totalValue"], "100200.0000");
    assert_eq!(p["activeOrders"].as_array().unwrap().len(), 0);

    // Close the day and read the published board.
    let (s, board) = post(&state, "/days/2026-08-28/close", Value::Null).await;
    assert_eq!(s, StatusCode::OK, "{board}");
    assert_eq!(board["day"], "2026-08-28");
    assert_eq!(board["rankedBy"], "dailyReturn descending");
    assert_eq!(board["rows"][0]["participant"], "alice");
    assert_eq!(board["rows"][0]["dailyReturn"], "0.002");
    assert_eq!(board["rows"][1]["participant"], "bob");
    assert_eq!(board["rows"][1]["active"], false);

    // The ladder places alice and lists bob unranked — bob never traded.
    let (s, ladder) = get(&state, "/ladder").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(ladder["rows"][0]["participant"], "alice");
    assert_eq!(ladder["rows"][0]["rank"], 1);
    assert_eq!(ladder["rows"][1]["participant"], "bob");
    assert!(ladder["rows"][1]["rank"].is_null());
    assert_eq!(ladder["rows"][1]["eligible"], false);

    let (_, days) = get(&state, "/days").await;
    assert_eq!(days["closedDays"], json!(["2026-08-28"]));
}

#[tokio::test]
async fn cancel_and_replace_are_reachable_and_preserve_fills() {
    let state = app();
    create(&state, "alice", "100000").await;

    // Partially fill 40 of 100, then replace: the 40 stays on the original.
    let id = submit(&state, "alice", "buy", 100, "10").await;
    let (s, _) = post(
        &state,
        "/broker/executions",
        json!({"clientOrderId": id, "qty": 40, "px": "10"}),
    )
    .await;
    assert_eq!(s, StatusCode::OK);

    let (s, body) = call(
        &state,
        "PUT",
        &format!("/orders/{id}"),
        Some(json!({"qty": 50, "limitPx": "11"})),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(body["original"]["state"], "CANCELLED");
    assert_eq!(body["original"]["filledQty"], 40);
    assert_eq!(body["replacement"]["qty"], 50);
    assert_eq!(body["replacement"]["replaces"], id);
    assert_eq!(body["replacement"]["state"], "ACKNOWLEDGED");

    let new_id = body["replacement"]["clientOrderId"].as_u64().unwrap();
    let (s, cancelled) = call(&state, "DELETE", &format!("/orders/{new_id}"), None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(cancelled["state"], "CANCELLED");
}

#[tokio::test]
async fn an_illegal_transition_returns_409_naming_the_current_state() {
    let state = app();
    create(&state, "alice", "100000").await;
    let id = submit(&state, "alice", "buy", 10, "10").await;
    post(&state, "/broker/executions", json!({"clientOrderId": id})).await;

    let (status, problem) = call(&state, "DELETE", &format!("/orders/{id}"), None).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(
        problem["type"],
        "https://unlok-ptc.invalid/errors/illegal-transition"
    );
    assert_eq!(problem["status"], 409);
    // The actionable part: not "that failed", but "it is already FILLED".
    assert_eq!(problem["currentState"], "FILLED");
    assert_eq!(problem["attempted"], "a cancellation");
}

#[tokio::test]
async fn refusals_carry_the_numbers_that_explain_them() {
    let state = app();
    create(&state, "alice", "100").await;

    let (status, problem) = post(
        &state,
        "/orders",
        json!({"participant": "alice", "symbol": "AAPL", "side": "buy", "qty": 100, "limitPx": "10"}),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        problem["type"],
        "https://unlok-ptc.invalid/errors/insufficient-cash"
    );
    assert_eq!(problem["need"], "1000.0000");
    assert_eq!(problem["available"], "100.0000");
}

#[tokio::test]
async fn unknown_things_are_404_and_duplicates_are_409() {
    let state = app();
    create(&state, "alice", "100000").await;

    assert_eq!(
        get(&state, "/participants/nobody/portfolio").await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(get(&state, "/orders/999").await.0, StatusCode::NOT_FOUND);
    assert_eq!(
        get(&state, "/days/2026-08-28/leaderboard").await.0,
        StatusCode::NOT_FOUND
    );

    let (status, problem) = post(
        &state,
        "/participants",
        json!({"id": "alice", "startingCash": "1"}),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(
        problem["type"],
        "https://unlok-ptc.invalid/errors/duplicate-participant"
    );
}

#[tokio::test]
async fn malformed_input_is_rejected_rather_than_repaired() {
    let state = app();
    create(&state, "alice", "100000").await;

    let bad = |field: Value| json!({"participant": "alice", "symbol": "AAPL", "side": "buy", "qty": 1, "limitPx": field});

    // Excess precision is an error, not a truncation.
    assert_eq!(
        post(&state, "/orders", bad(json!("10.123456"))).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        post(&state, "/orders", bad(json!("abc"))).await.0,
        StatusCode::BAD_REQUEST
    );

    // A lower-case symbol is rejected, not upper-cased: two spellings of one
    // key is how executions get filed twice.
    let (s, _) = post(
        &state,
        "/orders",
        json!({"participant": "alice", "symbol": "aapl", "side": "buy", "qty": 1, "limitPx": "10"}),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    let (s, problem) = post(
        &state,
        "/orders",
        json!({"participant": "alice", "symbol": "AAPL", "side": "BUY", "qty": 1, "limitPx": "10"}),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(problem["detail"].as_str().unwrap().contains("buy"));

    // Half a manual execution is refused rather than half-guessed.
    let id = submit(&state, "alice", "buy", 10, "10").await;
    let (s, _) = post(
        &state,
        "/broker/executions",
        json!({"clientOrderId": id, "qty": 5}),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn a_day_cannot_close_while_a_held_symbol_has_no_mark() {
    let state = app();
    create(&state, "alice", "100000").await;
    let id = submit(&state, "alice", "buy", 10, "10").await;
    post(&state, "/broker/executions", json!({"clientOrderId": id})).await;

    let (status, problem) = post(&state, "/days/2026-08-28/close", Value::Null).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(
        problem["type"],
        "https://unlok-ptc.invalid/errors/missing-mark"
    );
    assert_eq!(problem["symbol"], "AAPL");

    // The portfolio read still works — fail closed means never a wrong number,
    // not never a response.
    let (s, p) = get(&state, "/participants/alice/portfolio").await;
    assert_eq!(s, StatusCode::OK);
    assert!(p["totalValue"].is_null());
    assert!(p["valuationError"].as_str().unwrap().contains("AAPL"));
}

#[tokio::test]
async fn closing_a_day_twice_returns_the_same_published_board() {
    let state = app();
    create(&state, "alice", "100000").await;
    let id = submit(&state, "alice", "buy", 100, "10").await;
    post(&state, "/broker/executions", json!({"clientOrderId": id})).await;
    mark(&state, "12").await;

    let (_, first) = post(&state, "/days/2026-08-28/close", Value::Null).await;
    mark(&state, "50").await; // the book moves...
    let (status, again) = post(&state, "/days/2026-08-28/close", Value::Null).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(again, first, "a published day must not silently recompute");
}

#[tokio::test]
async fn state_survives_a_restart_because_the_log_does() {
    let path = std::env::temp_dir().join(format!("ptc-api-{}.sqlite", std::process::id()));
    let _ = std::fs::remove_file(&path);

    let first = Arc::new(Mutex::new(
        App::open(
            SqliteLog::open(&path).unwrap(),
            || MockBroker::simple(7),
            vec![],
        )
        .unwrap(),
    ));
    create(&first, "alice", "100000").await;
    let id = submit(&first, "alice", "buy", 100, "10").await;
    post(&first, "/broker/executions", json!({"clientOrderId": id})).await;
    mark(&first, "12").await;
    let (_, before) = get(&first, "/participants/alice/portfolio").await;
    drop(first);

    // New process, different broker seed: everything comes back from the log.
    let second = Arc::new(Mutex::new(
        App::open(
            SqliteLog::open(&path).unwrap(),
            || MockBroker::simple(9999),
            vec![],
        )
        .unwrap(),
    ));
    let (status, after) = get(&second, "/participants/alice/portfolio").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(after, before);

    // And the id minter resumed past the ids already in the log.
    let next = submit(&second, "alice", "buy", 1, "10").await;
    assert!(next > id, "a restart must not re-issue a used order id");

    std::fs::remove_file(&path).unwrap();
}

#[tokio::test]
async fn the_openapi_contract_matches_the_router() {
    let state = app();

    let (status, spec) = get(&state, "/openapi.json").await;
    assert_eq!(status, StatusCode::OK);

    // Every path+method the spec claims must exist, and every route the
    // router serves must be documented — hand-written specs drift, so the
    // drift is a test failure instead of a discovery.
    let mut documented: Vec<String> = spec["paths"]
        .as_object()
        .unwrap()
        .iter()
        .flat_map(|(path, ops)| {
            ops.as_object()
                .unwrap()
                .keys()
                .map(move |m| format!("{} {path}", m.to_uppercase()))
        })
        .collect();
    documented.sort();

    let mut served = vec![
        "GET /health",
        "POST /reset",
        "POST /participants",
        "GET /participants",
        "GET /participants/{participantId}/portfolio",
        "GET /participants/{participantId}/orders",
        "POST /orders",
        "GET /orders",
        "GET /orders/{clientOrderId}",
        "GET /orders/{clientOrderId}/executions",
        "DELETE /orders/{clientOrderId}",
        "PUT /orders/{clientOrderId}",
        "POST /broker/executions",
        "GET /instruments",
        "POST /instruments",
        "GET /instruments/{symbol}",
        "PUT /instruments/{symbol}",
        "DELETE /instruments/{symbol}",
        "GET /market/prices",
        "POST /market/prices",
        "GET /days",
        "POST /days/{day}/close",
        "GET /days/{day}/leaderboard",
        "GET /ladder",
    ]
    .into_iter()
    .map(String::from)
    .collect::<Vec<_>>();
    served.sort();

    assert_eq!(documented, served, "openapi.json and the router disagree");

    let (status, _) = get(&state, "/docs").await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn the_security_master_is_crud_and_governs_submissions() {
    let state = app();

    // Empty registry = unrestricted: list is empty, any symbol trades.
    let (status, body) = get(&state, "/instruments").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["instruments"].as_array().unwrap().len(), 0);

    // List AAPL at a penny tick with a size cap.
    let (status, created) = post(
        &state,
        "/instruments",
        json!({"symbol":"AAPL","tick":"0.01","maxOrderQty":500}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(created["tick"], "0.0100");
    assert_eq!(created["lot"], 1);
    assert_eq!(created["maxOrderQty"], 500);

    // Duplicate listing is a conflict; reading it back works.
    assert_eq!(
        post(
            &state,
            "/instruments",
            json!({"symbol":"AAPL","tick":"0.01"})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(get(&state, "/instruments/AAPL").await.0, StatusCode::OK);
    assert_eq!(
        get(&state, "/instruments/TSLA").await.0,
        StatusCode::NOT_FOUND
    );

    create(&state, "alice", "100000").await;

    // Venue-style rejections, recorded as REJECTED orders — not refused
    // commands. Off the list:
    let (status, order) = post(
        &state,
        "/orders",
        json!({"participant":"alice","symbol":"TSLA","side":"buy","qty":10,"limitPx":"10"}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(order["state"], "REJECTED");

    // Off the tick grid — $10.0050 on a penny stock, the Reg NMS case:
    let (_, order) = post(
        &state,
        "/orders",
        json!({"participant":"alice","symbol":"AAPL","side":"buy","qty":10,"limitPx":"10.0050"}),
    )
    .await;
    assert_eq!(order["state"], "REJECTED");

    // Over the cap:
    let (_, order) = post(
        &state,
        "/orders",
        json!({"participant":"alice","symbol":"AAPL","side":"buy","qty":501,"limitPx":"10"}),
    )
    .await;
    assert_eq!(order["state"], "REJECTED");

    // On-grid passes and fills.
    let id = submit(&state, "alice", "buy", 100, "10").await;
    post(&state, "/broker/executions", json!({"clientOrderId": id})).await;

    // Delisting is blocked while a position exists — with the counts.
    let (status, problem) = call(&state, "DELETE", "/instruments/AAPL", None).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(
        problem["type"],
        "https://unlok-ptc.invalid/errors/instrument-in-use"
    );
    assert_eq!(problem["positions"], 1);

    // Widen the tick by PUT; the earlier off-tick price is now legal.
    let (status, _) = call(
        &state,
        "PUT",
        "/instruments/AAPL",
        Some(json!({"tick":"0.0050"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, order) = post(
        &state,
        "/orders",
        json!({"participant":"alice","symbol":"AAPL","side":"buy","qty":10,"limitPx":"10.0050"}),
    )
    .await;
    assert_eq!(order["state"], "ACKNOWLEDGED");

    // Marks remain readable back, sorted, exactly as posted.
    post(
        &state,
        "/market/prices",
        json!([{"symbol":"MSFT","px":"21"},{"symbol":"AAPL","px":"11.5"}]),
    )
    .await;
    let (_, marks) = get(&state, "/market/prices").await;
    assert_eq!(
        marks["marks"],
        json!([{"symbol":"AAPL","px":"11.5000"},{"symbol":"MSFT","px":"21.0000"}])
    );
}

#[tokio::test]
async fn reset_restores_the_boot_world_deterministically() {
    let state = app();
    create(&state, "alice", "100000").await;
    let id = submit(&state, "alice", "buy", 100, "10").await;
    post(&state, "/broker/executions", json!({"clientOrderId": id})).await;
    mark(&state, "12").await;
    post(&state, "/days/2026-08-28/close", Value::Null).await;

    let (status, body) = post(&state, "/reset", Value::Null).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["participants"], 0);
    assert_eq!(body["events"], 0);

    // The world is gone: no participants, no orders, no closed days.
    let (_, h) = get(&state, "/health").await;
    assert_eq!(h["orders"], 0);
    assert_eq!(h["closedDays"], 0);
    assert_eq!(
        get(&state, "/participants/alice/portfolio").await.0,
        StatusCode::NOT_FOUND
    );

    // And it is the BOOT world, not merely an empty one: ids restart at 1 and
    // the reborn broker mints from 1 again — a fresh RNG stream, so the reset
    // world is exactly as deterministic as a rebooted one.
    create(&state, "alice", "100000").await;
    let (s2, order) = post(
        &state,
        "/orders",
        json!({"participant":"alice","symbol":"AAPL","side":"buy","qty":10,"limitPx":"10"}),
    )
    .await;
    assert_eq!(s2, StatusCode::CREATED);
    assert_eq!(order["clientOrderId"], 1);
    assert_eq!(order["brokerOrderId"], 1);
}

#[tokio::test]
async fn every_fill_carries_the_venues_exec_id() {
    let state = app();
    create(&state, "alice", "100000").await;
    let id = submit(&state, "alice", "buy", 100, "10").await;

    // One explicit, one auto: both are booked at the venue, one id sequence.
    post(
        &state,
        "/broker/executions",
        json!({"clientOrderId": id, "qty": 40, "px": "10"}),
    )
    .await;
    post(&state, "/broker/executions", json!({"clientOrderId": id})).await;

    let (status, blotter) = get(&state, &format!("/orders/{id}/executions")).await;
    assert_eq!(status, StatusCode::OK);
    let rows = blotter["executions"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["execId"], 1);
    assert_eq!(rows[0]["qty"], 40);
    assert_eq!(rows[1]["execId"], 2);
    assert_eq!(
        rows.iter().map(|r| r["qty"].as_i64().unwrap()).sum::<i64>(),
        100,
        "the blotter must reconcile to the order"
    );

    assert_eq!(
        get(&state, "/orders/999/executions").await.0,
        StatusCode::NOT_FOUND
    );
}
