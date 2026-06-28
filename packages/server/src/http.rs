//! HTTP layer (axum).
//!
//! Routes (plan §Q11 / §Q4 Step 6 / Step 9 — T013 adds /test/type, T014 adds
//! /test/swipe, T015 adds /test/pinch, T018 adds /test/elements, issue #3 adds
//! /test/focus, Task 029 adds /test/press):
//!
//! | route                | method | success | failure |
//! |----------------------|--------|---------|---------|
//! | `/test/info`         | GET    | 200     | —       |
//! | `/test/tap`          | POST   | 200     | 400 / 422 / 404 / 500 |
//! | `/test/swipe`        | POST   | 200     | 400 / 422 / 404 / 500 |
//! | `/test/pinch`        | POST   | 200     | 400 / 422 / 404 / 500 |
//! | `/test/press`        | POST   | 200     | 400 / 422 / 404 / 500 |
//! | `/test/key`          | POST   | 200     | 400 / 422 / 500 |
//! | `/test/wait`         | POST   | 200     | 400 / 422 / 408 / 500 |
//! | `/test/screenshot`   | GET    | 200     | 422 / 500 |
//! | `/test/type`         | POST   | 200     | 400 / 422 / 404 / 500 |
//! | `/test/focus`        | POST   | 200     | 400 / 422 / 404 / 500 |
//! | `/test/elements`     | GET    | 200     | 422 / 500 |
//! | (any unknown)        | *      | —       | 501 (axum fallback) |
//!
//! Plan Review M2: 501 = capability missing. 404 = domain not-found (only
//! emitted by `tap` when a selector matches no widget, and by `swipe` when
//! the `from` point is not contained in any `ScrolledWindow`).
//! `/test/elements` returns 200 with `roots: []` on selector miss (Playwright
//! `.all()` parity), so it never raises 404.

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

use crate::elements::ElementsError;
use crate::input::{
    FocusError, KeyError, PinchError, PressError, SetValueError, SwipeError, TapError,
    TouchDragError, TypeError, MAX_PRESS_HOLD_MS, MAX_TOUCH_DRAG_HOLD_MS, MAX_TOUCH_DRAG_WAYPOINTS,
};
use crate::main_thread::{MainCmd, WaitEvalError};
use crate::proto::{
    EventEnvelope, FocusRequest, Info, KeyRequest, PinchRequest, PressRequest, SetValueRequest,
    SwipeRequest, TapTarget, TouchDragRequest, TypeRequest, WaitRequest,
};
use crate::snapshot::ScreenshotError;
use crate::state::AppDefinedState;
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
    /// App-defined state snapshot exposed via `GET /test/state` and read by
    /// `WaitCondition::AppStateEq`.
    pub state: AppDefinedState,
}

/// Build the router exposed by the in-process server.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/test/info", get(get_info))
        .route("/test/tap", post(post_tap))
        .route("/test/swipe", post(post_swipe))
        .route("/test/pinch", post(post_pinch))
        .route("/test/press", post(post_press))
        .route("/test/key", post(post_key))
        .route("/test/set-value", post(post_set_value))
        .route("/test/touch-drag", post(post_touch_drag))
        .route("/test/wait", post(post_wait))
        .route("/test/screenshot", get(get_screenshot))
        .route("/test/type", post(post_type))
        .route("/test/focus", post(post_focus))
        .route("/test/elements", get(get_elements))
        .route("/test/events", get(crate::ws::ws_events))
        .route("/test/state", get(get_state))
        .fallback(unimpl)
        .with_state(state)
}

async fn get_info(State(state): State<AppState>) -> Json<Info> {
    Json((*state.info).clone())
}

async fn get_state(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(state.state.snapshot())
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

async fn post_swipe(State(state): State<AppState>, body: String) -> Response {
    let req: SwipeRequest = match serde_json::from_str(&body) {
        Ok(r) => r,
        Err(_) => return bad_request("malformed_body"),
    };

    let (tx, rx) = oneshot::channel();
    if state
        .cmd_tx
        .send(MainCmd::Swipe {
            from: req.from,
            to: req.to,
            duration_ms: req.duration_ms,
            reply: tx,
        })
        .await
        .is_err()
    {
        return server_error("main_thread channel closed");
    }
    match rx.await {
        Ok(Ok(())) => StatusCode::OK.into_response(),
        Ok(Err(e)) => swipe_error_response(e),
        Err(_) => server_error("main_thread reply dropped"),
    }
}

fn swipe_error_response(e: SwipeError) -> Response {
    match e {
        SwipeError::OutOfBounds { x, y } => {
            validation_error("out_of_bounds", json!({ "x": x, "y": y }))
        }
        SwipeError::NoActiveWindow => validation_error("no_active_window", json!({})),
        SwipeError::NoScrollableAtPoint { x, y } => error_response(
            StatusCode::NOT_FOUND,
            json!({ "error": "no_scrollable_at_point", "x": x, "y": y }),
        ),
        SwipeError::ZeroDuration => {
            validation_error("invalid_duration", json!({ "reason": "zero" }))
        }
        SwipeError::DurationTooLong { duration_ms } => validation_error(
            "invalid_duration",
            json!({ "reason": "too_long", "duration_ms": duration_ms }),
        ),
    }
}

async fn post_pinch(State(state): State<AppState>, body: String) -> Response {
    let req: PinchRequest = match serde_json::from_str(&body) {
        Ok(r) => r,
        Err(_) => return bad_request("malformed_body"),
    };

    let (tx, rx) = oneshot::channel();
    if state
        .cmd_tx
        .send(MainCmd::Pinch {
            center: req.center,
            scale: req.scale,
            duration_ms: req.duration_ms,
            reply: tx,
        })
        .await
        .is_err()
    {
        return server_error("main_thread channel closed");
    }
    match rx.await {
        Ok(Ok(())) => StatusCode::OK.into_response(),
        Ok(Err(e)) => pinch_error_response(e),
        Err(_) => server_error("main_thread reply dropped"),
    }
}

fn pinch_error_response(e: PinchError) -> Response {
    match e {
        PinchError::OutOfBounds { x, y } => {
            validation_error("out_of_bounds", json!({ "x": x, "y": y }))
        }
        PinchError::NoActiveWindow => validation_error("no_active_window", json!({})),
        PinchError::NoPinchableAtPoint { x, y } => error_response(
            StatusCode::NOT_FOUND,
            json!({ "error": "no_pinchable_at_point", "x": x, "y": y }),
        ),
        PinchError::ZeroDuration => {
            validation_error("invalid_duration", json!({ "reason": "zero" }))
        }
        PinchError::DurationTooLong { duration_ms } => validation_error(
            "invalid_duration",
            json!({ "reason": "too_long", "duration_ms": duration_ms }),
        ),
        PinchError::InvalidScale { reason } => {
            validation_error("invalid_scale", json!({ "reason": reason }))
        }
    }
}

async fn post_press(State(state): State<AppState>, body: String) -> Response {
    let req: PressRequest = match serde_json::from_str(&body) {
        Ok(r) => r,
        Err(_) => return bad_request("malformed_body"),
    };

    // R1: pre-validate on the pure (non-GLib) path before crossing the channel.
    // 1. Exactly one of selector / xy must be set.
    match (&req.selector, &req.xy) {
        (Some(_), None) | (None, Some(_)) => {}
        _ => {
            return validation_error(
                "invalid_target",
                json!({ "reason": "exactly one of selector / xy required" }),
            );
        }
    }
    // 2. hold_ms bounds (zero / too_long).
    if req.hold_ms == 0 {
        return validation_error("invalid_hold", json!({ "reason": "zero" }));
    }
    if req.hold_ms > MAX_PRESS_HOLD_MS {
        return validation_error(
            "invalid_hold",
            json!({ "reason": "too_long", "hold_ms": req.hold_ms }),
        );
    }
    // 3. selector syntax (mirror of post_focus / post_type): a parser error is
    //    422 invalid_selector here rather than 404 from the dispatch fallback.
    if let Some(selector) = &req.selector {
        if let Err(e) = parse_selector(selector) {
            return validation_error("invalid_selector", json!({ "reason": e.reason }));
        }
    }

    let (tx, rx) = oneshot::channel();
    if state
        .cmd_tx
        .send(MainCmd::Press {
            selector: req.selector,
            xy: req.xy,
            hold_ms: req.hold_ms,
            reply: tx,
        })
        .await
        .is_err()
    {
        return server_error("main_thread channel closed");
    }
    match rx.await {
        Ok(Ok(())) => StatusCode::OK.into_response(),
        Ok(Err(e)) => press_error_response(e),
        Err(_) => server_error("main_thread reply dropped"),
    }
}

fn press_error_response(e: PressError) -> Response {
    match e {
        PressError::ZeroHold => validation_error("invalid_hold", json!({ "reason": "zero" })),
        PressError::HoldTooLong { hold_ms } => validation_error(
            "invalid_hold",
            json!({ "reason": "too_long", "hold_ms": hold_ms }),
        ),
        PressError::OutOfBounds { x, y } => {
            validation_error("out_of_bounds", json!({ "x": x, "y": y }))
        }
        PressError::NoActiveWindow => validation_error("no_active_window", json!({})),
        PressError::NoLongPressableAtPoint { x, y } => error_response(
            StatusCode::NOT_FOUND,
            json!({ "error": "no_long_pressable_at_point", "x": x, "y": y }),
        ),
        PressError::SelectorNotFound { selector } => error_response(
            StatusCode::NOT_FOUND,
            json!({ "error": "selector_not_found", "selector": selector }),
        ),
        PressError::NoLongPressableForSelector { selector } => error_response(
            StatusCode::NOT_FOUND,
            json!({ "error": "no_long_pressable_for_selector", "selector": selector }),
        ),
        PressError::InvalidTarget { reason } => {
            validation_error("invalid_target", json!({ "reason": reason }))
        }
    }
}

async fn post_key(State(state): State<AppState>, body: String) -> Response {
    let req: KeyRequest = match serde_json::from_str(&body) {
        Ok(r) => r,
        Err(_) => return bad_request("malformed_body"),
    };

    let (tx, rx) = oneshot::channel();
    if state
        .cmd_tx
        .send(MainCmd::Key {
            key: req.key,
            reply: tx,
        })
        .await
        .is_err()
    {
        return server_error("main_thread channel closed");
    }
    match rx.await {
        Ok(Ok(())) => StatusCode::OK.into_response(),
        Ok(Err(e)) => key_error_response(e),
        Err(_) => server_error("main_thread reply dropped"),
    }
}

fn key_error_response(e: KeyError) -> Response {
    match e {
        KeyError::UnsupportedKey { key } => {
            validation_error("unsupported_key", json!({ "key": key }))
        }
        KeyError::NoActiveWindow => validation_error("no_active_window", json!({})),
    }
}

async fn post_set_value(State(state): State<AppState>, body: String) -> Response {
    let req: SetValueRequest = match serde_json::from_str(&body) {
        Ok(r) => r,
        Err(_) => return bad_request("malformed_body"),
    };

    // R1: pre-validate on the pure (non-GLib) path before crossing the channel.
    // 1. Exactly one of selector / xy must be set.
    match (&req.selector, &req.xy) {
        (Some(_), None) | (None, Some(_)) => {}
        _ => {
            return validation_error(
                "invalid_target",
                json!({ "reason": "exactly one of selector / xy required" }),
            );
        }
    }
    // 2. Selector mode requires an explicit value (no coordinate to derive one).
    if req.selector.is_some() && req.value.is_none() {
        return validation_error(
            "invalid_target",
            json!({ "reason": "value is required when targeting by selector" }),
        );
    }
    // 3. value must be finite when present.
    if let Some(v) = req.value {
        if !v.is_finite() {
            return validation_error("invalid_value", json!({ "reason": "not_finite" }));
        }
    }
    // 4. selector syntax (mirror of post_press): a parser error is 422
    //    invalid_selector here rather than 404 from the dispatch fallback.
    if let Some(selector) = &req.selector {
        if let Err(e) = parse_selector(selector) {
            return validation_error("invalid_selector", json!({ "reason": e.reason }));
        }
    }

    let (tx, rx) = oneshot::channel();
    if state
        .cmd_tx
        .send(MainCmd::SetValue {
            selector: req.selector,
            xy: req.xy,
            value: req.value,
            reply: tx,
        })
        .await
        .is_err()
    {
        return server_error("main_thread channel closed");
    }
    match rx.await {
        Ok(Ok(())) => StatusCode::OK.into_response(),
        Ok(Err(e)) => set_value_error_response(e),
        Err(_) => server_error("main_thread reply dropped"),
    }
}
fn set_value_error_response(e: SetValueError) -> Response {
    match e {
        SetValueError::InvalidTarget { reason } => {
            validation_error("invalid_target", json!({ "reason": reason }))
        }
        SetValueError::InvalidValue { reason } => {
            validation_error("invalid_value", json!({ "reason": reason }))
        }
        SetValueError::OutOfBounds { x, y } => {
            validation_error("out_of_bounds", json!({ "x": x, "y": y }))
        }
        SetValueError::WidgetNotVisible { selector } => match selector {
            Some(s) => validation_error("widget_not_visible", json!({ "selector": s })),
            None => validation_error("widget_not_visible", json!({})),
        },
        SetValueError::WidgetDisabled { selector } => match selector {
            Some(s) => validation_error("widget_disabled", json!({ "selector": s })),
            None => validation_error("widget_disabled", json!({})),
        },
        SetValueError::NoActiveWindow => validation_error("no_active_window", json!({})),
        SetValueError::SelectorNotFound { selector } => error_response(
            StatusCode::NOT_FOUND,
            json!({ "error": "selector_not_found", "selector": selector }),
        ),
        SetValueError::NoRangeAtPoint { x, y } => error_response(
            StatusCode::NOT_FOUND,
            json!({ "error": "no_range_at_point", "x": x, "y": y }),
        ),
        SetValueError::NoRangeForSelector { selector } => error_response(
            StatusCode::NOT_FOUND,
            json!({ "error": "no_range_for_selector", "selector": selector }),
        ),
    }
}
async fn post_touch_drag(State(state): State<AppState>, body: String) -> Response {
    let req: TouchDragRequest = match serde_json::from_str(&body) {
        Ok(r) => r,
        Err(_) => return bad_request("malformed_body"),
    };

    // R1: pre-validate on the pure (non-GLib) path before crossing the channel,
    // mirroring `post_press`.
    // 1. Exactly one of selector / xy must be set.
    match (&req.selector, &req.xy) {
        (Some(_), None) | (None, Some(_)) => {}
        _ => {
            return validation_error(
                "invalid_target",
                json!({ "reason": "exactly one of selector / xy required" }),
            );
        }
    }
    // 2. hold_ms bounds (zero / too_long).
    if req.hold_ms == 0 {
        return validation_error("invalid_hold", json!({ "reason": "zero" }));
    }
    if req.hold_ms > MAX_TOUCH_DRAG_HOLD_MS {
        return validation_error(
            "invalid_hold",
            json!({ "reason": "too_long", "hold_ms": req.hold_ms }),
        );
    }
    // 3. waypoint count bound.
    if req.waypoints.len() > MAX_TOUCH_DRAG_WAYPOINTS {
        return validation_error(
            "too_many_waypoints",
            json!({ "count": req.waypoints.len(), "max": MAX_TOUCH_DRAG_WAYPOINTS }),
        );
    }
    // 4. selector syntax (mirror of post_press): a parser error is 422
    //    invalid_selector here rather than 404 from the dispatch fallback.
    if let Some(selector) = &req.selector {
        if let Err(e) = parse_selector(selector) {
            return validation_error("invalid_selector", json!({ "reason": e.reason }));
        }
    }

    let (tx, rx) = oneshot::channel();
    if state
        .cmd_tx
        .send(MainCmd::TouchDrag {
            selector: req.selector,
            xy: req.xy,
            hold_ms: req.hold_ms,
            waypoints: req.waypoints,
            release: req.release,
            reply: tx,
        })
        .await
        .is_err()
    {
        return server_error("main_thread channel closed");
    }
    // Timer-based (hold + per-waypoint steps): like press / swipe, do NOT apply
    // the bounded-dispatch timeout — the sequence legitimately spans up to
    // `hold_ms` + waypoints × step.
    match rx.await {
        Ok(Ok(())) => StatusCode::OK.into_response(),
        Ok(Err(e)) => touch_drag_error_response(e),
        Err(_) => server_error("main_thread reply dropped"),
    }
}
fn touch_drag_error_response(e: TouchDragError) -> Response {
    match e {
        TouchDragError::ZeroHold => validation_error("invalid_hold", json!({ "reason": "zero" })),
        TouchDragError::HoldTooLong { hold_ms } => validation_error(
            "invalid_hold",
            json!({ "reason": "too_long", "hold_ms": hold_ms }),
        ),
        TouchDragError::TooManyWaypoints { count } => validation_error(
            "too_many_waypoints",
            json!({ "count": count, "max": MAX_TOUCH_DRAG_WAYPOINTS }),
        ),
        TouchDragError::OutOfBounds { x, y } => {
            validation_error("out_of_bounds", json!({ "x": x, "y": y }))
        }
        TouchDragError::NoActiveWindow => validation_error("no_active_window", json!({})),
        TouchDragError::NoDraggableAtPoint { x, y } => error_response(
            StatusCode::NOT_FOUND,
            json!({ "error": "no_draggable_at_point", "x": x, "y": y }),
        ),
        TouchDragError::SelectorNotFound { selector } => error_response(
            StatusCode::NOT_FOUND,
            json!({ "error": "selector_not_found", "selector": selector }),
        ),
        TouchDragError::NoDraggableForSelector { selector } => error_response(
            StatusCode::NOT_FOUND,
            json!({ "error": "no_draggable_for_selector", "selector": selector }),
        ),
        TouchDragError::InvalidTarget { reason } => {
            validation_error("invalid_target", json!({ "reason": reason }))
        }
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

    // Pre-validate selector / path before crossing into the GLib main thread.
    match &req.condition {
        crate::proto::WaitCondition::SelectorVisible { selector }
        | crate::proto::WaitCondition::StateEq { selector, .. } => {
            if let Err(e) = parse_selector(selector) {
                return validation_error("invalid_selector", json!({"reason": e.reason}));
            }
        }
        crate::proto::WaitCondition::AppStateEq { path, .. } => {
            // RFC 6901: an empty string ("") matches the document root, and a
            // non-empty pointer must start with `/`. `serde_json::Value::pointer`
            // silently returns `None` for malformed paths, which would otherwise
            // make a client typo look like an indefinite tick failure. Surface
            // the obvious case ("foo" without a leading `/`) as 422 invalid_path
            // up-front so callers fail fast.
            if !path.is_empty() && !path.starts_with('/') {
                return validation_error(
                    "invalid_path",
                    json!({"reason": "missing_leading_slash"}),
                );
            }
        }
    }

    match poll_until(
        &state.cmd_tx,
        state.state.clone(),
        req.condition,
        req.timeout_ms,
    )
    .await
    {
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

async fn get_screenshot(
    State(state): State<AppState>,
    axum::extract::RawQuery(raw): axum::extract::RawQuery,
) -> Response {
    let q = match parse_screenshot_query(raw.as_deref().unwrap_or("")) {
        Ok(q) => q,
        Err(resp) => return resp,
    };

    // Pre-validate the selector before crossing into the GLib main thread so a
    // parse error surfaces as 422 invalid_selector rather than a 404 (mirror of
    // `get_elements` / `post_type`). `render_target` re-parses defensively.
    if let Some(selector) = q.selector.as_deref() {
        if let Err(e) = parse_selector(selector) {
            return validation_error("invalid_selector", json!({"reason": e.reason}));
        }
    }

    let (tx, rx) = oneshot::channel();
    if state
        .cmd_tx
        .send(MainCmd::Screenshot {
            selector: q.selector,
            window: q.window,
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

async fn post_focus(State(state): State<AppState>, body: String) -> Response {
    let req: FocusRequest = match serde_json::from_str(&body) {
        Ok(r) => r,
        Err(_) => return bad_request("malformed_body"),
    };

    // Pre-validate selector before crossing into the GLib main thread, so the
    // parser error becomes 422 invalid_selector rather than 404. Mirror of
    // `post_type`; `dispatch_focus` defends against bypass with a 404 fallback.
    if let Err(e) = parse_selector(&req.selector) {
        return validation_error("invalid_selector", json!({"reason": e.reason}));
    }

    let (tx, rx) = oneshot::channel();
    if state
        .cmd_tx
        .send(MainCmd::Focus {
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
        Err(e) => focus_error_response(e),
    }
}

fn focus_error_response(e: FocusError) -> Response {
    match e {
        FocusError::SelectorNotFound { selector } => error_response(
            StatusCode::NOT_FOUND,
            json!({ "error": "selector_not_found", "selector": selector }),
        ),
        FocusError::FocusRejected { selector } => {
            validation_error("focus_rejected", json!({ "selector": selector }))
        }
        FocusError::WidgetNotVisible { selector } => {
            validation_error("widget_not_visible", json!({ "selector": selector }))
        }
        FocusError::WidgetDisabled { selector } => {
            validation_error("widget_disabled", json!({ "selector": selector }))
        }
        FocusError::NoActiveWindow => validation_error("no_active_window", json!({})),
    }
}

async fn get_elements(
    State(state): State<AppState>,
    axum::extract::RawQuery(raw): axum::extract::RawQuery,
) -> Response {
    let q = match parse_elements_query(raw.as_deref().unwrap_or("")) {
        Ok(q) => q,
        Err(resp) => return resp,
    };

    if let Some(selector) = q.selector.as_deref() {
        if let Err(e) = parse_selector(selector) {
            return validation_error("invalid_selector", json!({"reason": e.reason}));
        }
    }

    let (tx, rx) = oneshot::channel();
    if state
        .cmd_tx
        .send(MainCmd::Elements {
            selector: q.selector,
            max_depth: q.max_depth,
            props: q.props,
            reply: tx,
        })
        .await
        .is_err()
    {
        return server_error("main_thread channel closed");
    }
    match rx.await {
        Ok(Ok(resp)) => Json(resp).into_response(),
        Ok(Err(e)) => elements_error_response(e),
        Err(_) => server_error("main_thread reply dropped"),
    }
}

struct ElementsQuery {
    selector: Option<String>,
    max_depth: Option<u32>,
    /// Opt-in list of GObject property names to read per matched widget.
    /// Empty when `props=` is absent or set to the empty string.
    props: Vec<String>,
}

#[allow(clippy::result_large_err)] // axum Response is the standard error path here; boxing it would force every caller to unbox.
fn parse_elements_query(raw: &str) -> Result<ElementsQuery, Response> {
    let mut selector: Option<String> = None;
    let mut max_depth: Option<u32> = None;
    let mut props: Vec<String> = Vec::new();
    if !raw.is_empty() {
        for pair in raw.split('&') {
            if pair.is_empty() {
                continue;
            }
            let (k, v) = match pair.split_once('=') {
                Some((k, v)) => (k, v),
                None => (pair, ""),
            };
            let value = match decode_query_component(v) {
                Some(s) => s,
                None => {
                    return Err(validation_error(
                        "invalid_query",
                        json!({ "reason": "malformed_percent_encoding", "key": k }),
                    ));
                }
            };
            match k {
                "selector" => selector = Some(value),
                "max_depth" => match value.parse::<u32>() {
                    Ok(n) => max_depth = Some(n),
                    Err(_) => {
                        return Err(validation_error(
                            "invalid_max_depth",
                            json!({ "reason": "non_integer" }),
                        ));
                    }
                },
                // `props=text,placeholder-text` — comma-separated GObject
                // property names. Repeated `props=` keys also accumulate
                // (so `?props=text&props=label` works). Empty segments
                // (`props=` alone, `props=,foo`, trailing comma) are
                // silently dropped; we don't reject because the empty
                // form is the natural "no props" default.
                "props" => {
                    for name in value.split(',') {
                        let name = name.trim();
                        if !name.is_empty() {
                            props.push(name.to_string());
                        }
                    }
                }
                _ => {}
            }
        }
    }
    Ok(ElementsQuery {
        selector,
        max_depth,
        props,
    })
}

fn decode_query_component(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hi = (bytes[i + 1] as char).to_digit(16)?;
                let lo = (bytes[i + 2] as char).to_digit(16)?;
                out.push(((hi << 4) | lo) as u8);
                i += 3;
            }
            b'%' => return None,
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

fn elements_error_response(e: ElementsError) -> Response {
    match e {
        ElementsError::InvalidSelector { reason } => {
            validation_error("invalid_selector", json!({ "reason": reason }))
        }
        ElementsError::NoActiveWindow => validation_error("no_active_window", json!({})),
    }
}

fn screenshot_error_response(e: ScreenshotError) -> Response {
    match e {
        ScreenshotError::NoActiveWindow => validation_error("no_active_window", json!({})),
        ScreenshotError::InvalidSelector { reason } => {
            validation_error("invalid_selector", json!({ "reason": reason }))
        }
        ScreenshotError::SelectorNotFound { selector } => error_response(
            StatusCode::NOT_FOUND,
            json!({ "error": "selector_not_found", "selector": selector }),
        ),
        ScreenshotError::WindowOutOfRange { index, count } => validation_error(
            "window_out_of_range",
            json!({ "index": index, "count": count }),
        ),
        ScreenshotError::UnrealizedTarget => validation_error("unrealized_target", json!({})),
        ScreenshotError::EmptyNode => validation_error("empty_node", json!({})),
        ScreenshotError::ZeroSize => validation_error("zero_size", json!({})),
        ScreenshotError::RenderRealize(msg) => server_error(&format!("render_failed: {msg}")),
    }
}

struct ScreenshotQuery {
    selector: Option<String>,
    window: Option<usize>,
}

#[allow(clippy::result_large_err)] // mirrors `parse_elements_query`: axum Response is the error path.
fn parse_screenshot_query(raw: &str) -> Result<ScreenshotQuery, Response> {
    let mut selector: Option<String> = None;
    let mut window: Option<usize> = None;
    if !raw.is_empty() {
        for pair in raw.split('&') {
            if pair.is_empty() {
                continue;
            }
            let (k, v) = match pair.split_once('=') {
                Some((k, v)) => (k, v),
                None => (pair, ""),
            };
            let value = match decode_query_component(v) {
                Some(s) => s,
                None => {
                    return Err(validation_error(
                        "invalid_query",
                        json!({ "reason": "malformed_percent_encoding", "key": k }),
                    ));
                }
            };
            match k {
                "selector" => selector = Some(value),
                "window" => match value.parse::<usize>() {
                    Ok(n) => window = Some(n),
                    Err(_) => {
                        return Err(validation_error(
                            "invalid_window",
                            json!({ "reason": "non_integer" }),
                        ));
                    }
                },
                _ => {}
            }
        }
    }
    Ok(ScreenshotQuery { selector, window })
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
