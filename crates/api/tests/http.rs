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
        App::open(SqliteLog::in_memory().unwrap(), MockBroker::simple(7)).unwrap(),
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
    body["id"].as_u64().unwrap()
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
    let (s, order) = post(&state, "/broker/executions", json!({"orderId": id})).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(order["state"], "FILLED");
    assert_eq!(order["filledQty"], 100);
    assert_eq!(order["filledCost"], "1000.0000");
    assert!(
        order["brokerOrderId"].is_null(),
        "terminal orders drop the broker id"
    );

    mark(&state, "12").await;

    let (s, p) = get(&state, "/participants/alice/portfolio").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(p["cash"], "99000.0000");
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
        json!({"orderId": id, "qty": 40, "px": "10"}),
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

    let new_id = body["replacement"]["id"].as_u64().unwrap();
    let (s, cancelled) = call(&state, "DELETE", &format!("/orders/{new_id}"), None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(cancelled["state"], "CANCELLED");
}

#[tokio::test]
async fn an_illegal_transition_returns_409_naming_the_current_state() {
    let state = app();
    create(&state, "alice", "100000").await;
    let id = submit(&state, "alice", "buy", 10, "10").await;
    post(&state, "/broker/executions", json!({"orderId": id})).await;

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
        json!({"orderId": id, "qty": 5}),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn a_day_cannot_close_while_a_held_symbol_has_no_mark() {
    let state = app();
    create(&state, "alice", "100000").await;
    let id = submit(&state, "alice", "buy", 10, "10").await;
    post(&state, "/broker/executions", json!({"orderId": id})).await;

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
    post(&state, "/broker/executions", json!({"orderId": id})).await;
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
        App::open(SqliteLog::open(&path).unwrap(), MockBroker::simple(7)).unwrap(),
    ));
    create(&first, "alice", "100000").await;
    let id = submit(&first, "alice", "buy", 100, "10").await;
    post(&first, "/broker/executions", json!({"orderId": id})).await;
    mark(&first, "12").await;
    let (_, before) = get(&first, "/participants/alice/portfolio").await;
    drop(first);

    // New process, different broker seed: everything comes back from the log.
    let second = Arc::new(Mutex::new(
        App::open(SqliteLog::open(&path).unwrap(), MockBroker::simple(9999)).unwrap(),
    ));
    let (status, after) = get(&second, "/participants/alice/portfolio").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(after, before);

    // And the id minter resumed past the ids already in the log.
    let next = submit(&second, "alice", "buy", 1, "10").await;
    assert!(next > id, "a restart must not re-issue a used order id");

    std::fs::remove_file(&path).unwrap();
}
