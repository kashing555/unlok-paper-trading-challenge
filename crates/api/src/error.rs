//! Errors as `application/problem+json` ([RFC 7807]).
//!
//! Each variant carries a **stable machine-readable `type`**, so a client can
//! branch on the failure without parsing prose. An illegal state transition
//! returns `409` **with the order's current state in the body** — the caller
//! needs to know what it actually is, not merely that the request failed.
//!
//! [RFC 7807]: https://datatracker.ietf.org/doc/html/rfc7807

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use domain::{DomainError, PortfolioError, PositionError, TransitionError};
use engine::EngineError;
use serde_json::{json, Value};
use store::StoreError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error(transparent)]
    Engine(#[from] EngineError),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Domain(#[from] DomainError),
    #[error("{0}")]
    BadRequest(String),
}

const BASE: &str = "https://unlok-ptc.invalid/errors";

impl AppError {
    /// `(status, slug, extra members)`.
    fn classify(&self) -> (StatusCode, &'static str, Option<Value>) {
        use EngineError as E;
        match self {
            Self::BadRequest(_) => (StatusCode::BAD_REQUEST, "malformed-request", None),
            Self::Domain(_) => (StatusCode::BAD_REQUEST, "invalid-value", None),

            Self::Engine(E::UnknownParticipant(_)) => {
                (StatusCode::NOT_FOUND, "unknown-participant", None)
            }
            Self::Engine(E::UnknownOrder(_)) => (StatusCode::NOT_FOUND, "unknown-order", None),
            Self::Engine(E::DayNotClosed(_)) => (StatusCode::NOT_FOUND, "day-not-closed", None),

            Self::Engine(E::DayOutOfOrder { day, latest }) => (
                StatusCode::CONFLICT,
                "day-out-of-order",
                Some(json!({ "day": day.to_string(), "latestClosed": latest.to_string() })),
            ),

            Self::Engine(E::DuplicateParticipant(_)) => {
                (StatusCode::CONFLICT, "duplicate-participant", None)
            }
            Self::Engine(E::DuplicateOrder(_)) => (StatusCode::CONFLICT, "duplicate-order", None),

            // The state is in the body on purpose: "you cannot cancel that" is
            // not actionable, "it is already FILLED" is.
            Self::Engine(E::Transition(TransitionError::Illegal { state, event })) => (
                StatusCode::CONFLICT,
                "illegal-transition",
                Some(json!({ "currentState": state, "attempted": event })),
            ),
            Self::Engine(E::Transition(TransitionError::Overfill {
                fill,
                total,
                ordered,
            })) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "overfill",
                Some(json!({ "fill": fill, "wouldTotal": total, "ordered": ordered })),
            ),
            Self::Engine(E::Transition(_)) => {
                (StatusCode::UNPROCESSABLE_ENTITY, "invalid-execution", None)
            }

            Self::Engine(E::InsufficientAvailableCash { need, available }) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "insufficient-cash",
                Some(json!({ "need": need.to_string(), "available": available.to_string() })),
            ),
            Self::Engine(E::InsufficientAvailablePosition {
                symbol,
                want,
                available,
            }) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "insufficient-position",
                Some(json!({ "symbol": symbol.to_string(), "want": want, "available": available })),
            ),
            Self::Engine(E::Portfolio(PortfolioError::MissingMark { symbol })) => (
                StatusCode::CONFLICT,
                "missing-mark",
                Some(json!({ "symbol": symbol.to_string() })),
            ),
            Self::Engine(E::Portfolio(PortfolioError::Position(
                PositionError::InsufficientPosition { symbol, held, sell },
            ))) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "insufficient-position",
                Some(json!({ "symbol": symbol.to_string(), "held": held, "sell": sell })),
            ),
            Self::Engine(E::NothingToExecute(_)) => {
                (StatusCode::CONFLICT, "nothing-to-execute", None)
            }
            Self::Engine(E::Scoring(_)) => (StatusCode::CONFLICT, "cannot-rank", None),

            // Anything left is a bug or a disk problem, not the caller's fault.
            Self::Engine(_) | Self::Store(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "internal", None)
            }
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, slug, extra) = self.classify();
        let mut body = json!({
            "type":   format!("{BASE}/{slug}"),
            "title":  slug.replace('-', " "),
            "status": status.as_u16(),
            "detail": self.to_string(),
        });
        if let (Some(Value::Object(more)), Some(obj)) = (extra, body.as_object_mut()) {
            obj.extend(more);
        }
        let mut response = (status, Json(body)).into_response();
        response.headers_mut().insert(
            axum::http::header::CONTENT_TYPE,
            axum::http::HeaderValue::from_static("application/problem+json"),
        );
        response
    }
}
