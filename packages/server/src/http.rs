//! HTTP layer (axum).
//!
//! Routes (plan §Q11 / §Q4 Step 6 / Step 9):
//!
//! | route                | method | success | failure |
//! |----------------------|--------|---------|---------|
//! | `/test/info`         | GET    | 200     | —       |
//! | `/test/tap`          | POST   | 200     | 400 / 422 / 404 / 500 |
//! | `/test/wait`         | POST   | 200     | 400 / 422 / 408 / 500 |
//! | `/test/screenshot`   | GET    | 200     | 422 / 500 |
//! | `/test/type`         | POST   | 200     | 400 / 422 / 404 / 500 |
//! | (any unknown)        | *      | —       | 501 (axum fallback) |
//!
//! Plan Review M2: 501 = capability missing. 404 = domain not-found (only
//! emitted by `tap` when a selector matches no widget).

use std::sync::Arc;

use axum::{
    body::Bytes,
    extract::{Request, State},
    http::{header, StatusCode},
    response::{IntoResponse, Json, Response},
    routing::{get, post},
    Router,
};
use serde_json::json;
use tokio::sync::{mpsc, oneshot};

use crate::input::{TapError, TypeError};
use crate::main_thread::{MainCmd, WaitEvalError};
use crate::proto::{EventEnvelope, Info, TapTarget, TypeRequest, WaitRequest};
use crate::snapshot::ScreenshotError;
use crate::tree::parse_selector;
use crate::wait::{poll_until, WaitError, MAX_TIMEOUT_MS};

/// Shared state injected into axum handlers.
#[derive(Clone)]
pub struct AppState {
    pub info: Arc<Info>,
    pub cmd_tx: mpsc::Sender<MainCmd>,
    /// Broadcast bus for `WS /test/events`. Cloned per-connection at upgrade
    /// time via `subscribe()`.
    pub event_tx: tokio::sync::broadcast::Sender<EventEnvelope>,
}

/// Build the router exposed by the in-process server.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/test/info", get(get_info))
        .route("/test/tap", post(post_tap))
        .route("/test/wait", post(post_wait))
        .route("/test/screenshot", get(get_screenshot))
        .route("/test/type", post(post_type))
        .route("/test/events", get(crate::ws::ws_events))
        .fallback(unimpl)
        .with_state(state)
}

async fn get_info(State(state): State<AppState>) -> Json<Info> {
    Json((*state.info).clone())
}

async fn unimpl(req: Request) -> Response {
    let cap = capability_from_path(req.uri().path());
    error_response(
        StatusCode::NOT_IMPLEMENTED,
        json!({ "error": "not_implemented", "capability": cap }),
    )
}

fn capability_from_path(path: &str) -> String {
    path.strip_prefix("/test/")
        .unwrap_or(path)
        .trim_end_matches('/')
        .split('/')
        .next()
        .unwrap_or("")
        .to_string()
}

async fn post_tap(State(state): State<AppState>, body: String) -> Response {
    let target: TapTarget = match serde_json::from_str(&body) {
        Ok(t) => t,
        Err(_) => return bad_request("malformed_body"),
    };

    // Pre-validate selectors before crossing into the GLib main thread, so
    // the parser error becomes 422 invalid_selector rather than 404.
    if let TapTarget::Selector { selector } = &target {
        if let Err(e) = parse_selector(selector) {
            return validation_error("invalid_selector", json!({"reason": e.reason}));
        }
    }

    let (tx, rx) = oneshot::channel();
    if state
        .cmd_tx
        .send(MainCmd::Tap { target, reply: tx })
        .await
        .is_err()
    {
        return server_error("main_thread channel closed");
    }
    let outcome = match rx.await {
        Ok(o) => o,
        Err(_) => return server_error("main_thread reply dropped"),
    };
    match outcome {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => tap_error_response(e),
    }
}

fn tap_error_response(e: TapError) -> Response {
    match e {
        TapError::SelectorNotFound { selector } => error_response(
            StatusCode::NOT_FOUND,
            json!({ "error": "selector_not_found", "selector": selector }),
        ),
        TapError::NoWidgetAtPoint { x, y } => error_response(
            StatusCode::NOT_FOUND,
            json!({ "error": "no_widget_at_point", "x": x, "y": y }),
        ),
        TapError::UnsupportedWidget { widget_type } => validation_error(
            "tap_unsupported_widget",
            json!({ "widget_type": widget_type }),
        ),
        TapError::WidgetNotVisible { selector } => {
            validation_error("widget_not_visible", json!({ "selector": selector }))
        }
        TapError::WidgetDisabled { selector } => {
            validation_error("widget_disabled", json!({ "selector": selector }))
        }
        TapError::OutOfBounds { x, y } => {
            validation_error("out_of_bounds", json!({ "x": x, "y": y }))
        }
        TapError::NoActiveWindow => validation_error("no_active_window", json!({})),
    }
}

async fn post_wait(State(state): State<AppState>, body: String) -> Response {
    let req: WaitRequest = match serde_json::from_str(&body) {
        Ok(r) => r,
        Err(_) => return bad_request("malformed_body"),
    };

    if req.timeout_ms == 0 {
        return validation_error("invalid_timeout", json!({"reason": "zero"}));
    }
    if req.timeout_ms > MAX_TIMEOUT_MS {
        return validation_error("invalid_timeout", json!({"reason": "excessive"}));
    }

    // Pre-validate selectors syntactically.
    let selector = match &req.condition {
        crate::proto::WaitCondition::SelectorVisible { selector } => selector,
        crate::proto::WaitCondition::StateEq { selector, .. } => selector,
    };
    if let Err(e) = parse_selector(selector) {
        return validation_error("invalid_selector", json!({"reason": e.reason}));
    }

    match poll_until(&state.cmd_tx, req.condition, req.timeout_ms).await {
        Ok(result) => Json(result).into_response(),
        Err(WaitError::Timeout) => error_response(
            StatusCode::REQUEST_TIMEOUT,
            json!({"error": "wait_timeout", "timeout_ms": req.timeout_ms}),
        ),
        Err(WaitError::InvalidTimeout(reason)) => {
            validation_error("invalid_timeout", json!({"reason": reason}))
        }
        Err(WaitError::Eval(WaitEvalError::InvalidSelector(reason))) => {
            validation_error("invalid_selector", json!({"reason": reason}))
        }
        Err(WaitError::Eval(WaitEvalError::UnsupportedPropertyType(t))) => {
            validation_error("unsupported_property_type", json!({ "widget_type": t }))
        }
        Err(WaitError::Eval(WaitEvalError::Internal(msg))) | Err(WaitError::Internal(msg)) => {
            server_error(&msg)
        }
    }
}

async fn get_screenshot(State(state): State<AppState>) -> Response {
    let (tx, rx) = oneshot::channel();
    if state
        .cmd_tx
        .send(MainCmd::Screenshot { reply: tx })
        .await
        .is_err()
    {
        return server_error("main_thread channel closed");
    }
    let outcome = match rx.await {
        Ok(o) => o,
        Err(_) => return server_error("main_thread reply dropped"),
    };
    match outcome {
        Ok(bytes) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "image/png")],
            Bytes::from(bytes),
        )
            .into_response(),
        Err(e) => screenshot_error_response(e),
    }
}

async fn post_type(State(state): State<AppState>, body: String) -> Response {
    let req: TypeRequest = match serde_json::from_str(&body) {
        Ok(r) => r,
        Err(_) => return bad_request("malformed_body"),
    };

    // Pre-validate selector before crossing into the GLib main thread, so the
    // parser error becomes 422 invalid_selector rather than 404. Mirror of
    // `post_tap`. `dispatch_type` defends against bypass with a 404 fallback,
    // see `wait.rs::dispatch_type`.
    if let Err(e) = parse_selector(&req.selector) {
        return validation_error("invalid_selector", json!({"reason": e.reason}));
    }

    let (tx, rx) = oneshot::channel();
    if state
        .cmd_tx
        .send(MainCmd::Type {
            request: req,
            reply: tx,
        })
        .await
        .is_err()
    {
        return server_error("main_thread channel closed");
    }
    let outcome = match rx.await {
        Ok(o) => o,
        Err(_) => return server_error("main_thread reply dropped"),
    };
    match outcome {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => type_error_response(e),
    }
}

fn type_error_response(e: TypeError) -> Response {
    match e {
        TypeError::SelectorNotFound { selector } => error_response(
            StatusCode::NOT_FOUND,
            json!({ "error": "selector_not_found", "selector": selector }),
        ),
        TypeError::UnsupportedWidget { widget_type } => validation_error(
            "type_unsupported_widget",
            json!({ "widget_type": widget_type }),
        ),
        TypeError::WidgetNotVisible { selector } => {
            validation_error("widget_not_visible", json!({ "selector": selector }))
        }
        TypeError::WidgetDisabled { selector } => {
            validation_error("widget_disabled", json!({ "selector": selector }))
        }
        TypeError::NoActiveWindow => validation_error("no_active_window", json!({})),
    }
}

fn screenshot_error_response(e: ScreenshotError) -> Response {
    match e {
        ScreenshotError::NoActiveWindow => validation_error("no_active_window", json!({})),
        ScreenshotError::EmptyNode => validation_error("empty_node", json!({})),
        ScreenshotError::ZeroSize => validation_error("zero_size", json!({})),
        ScreenshotError::RenderRealize(msg) => server_error(&format!("render_failed: {msg}")),
    }
}

fn bad_request(reason: &str) -> Response {
    error_response(
        StatusCode::BAD_REQUEST,
        json!({ "error": "bad_request", "reason": reason }),
    )
}

fn validation_error(code: &str, detail: serde_json::Value) -> Response {
    let mut body = serde_json::Map::new();
    body.insert("error".into(), serde_json::Value::String(code.into()));
    if let serde_json::Value::Object(extra) = detail {
        for (k, v) in extra {
            body.insert(k, v);
        }
    }
    error_response(
        StatusCode::UNPROCESSABLE_ENTITY,
        serde_json::Value::Object(body),
    )
}

fn server_error(msg: &str) -> Response {
    error_response(
        StatusCode::INTERNAL_SERVER_ERROR,
        json!({ "error": "internal", "message": msg }),
    )
}

fn error_response(status: StatusCode, body: serde_json::Value) -> Response {
    (status, Json(body)).into_response()
}
