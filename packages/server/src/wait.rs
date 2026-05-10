//! Long-polling `wait` evaluation. Plan §Q5/§Q6/§Q7/§Q8.
//!
//! Pure evaluation (`eval_condition`) is mock-tree friendly so the unit tests
//! don't need GTK. The GTK-bound wrapper `eval_condition_in_app` reads
//! widgets through `tree::GtkTree` and is invoked from the GLib main thread
//! via `MainCmd::EvalWait`.
//!
//! `dispatch_tap` is the GTK-bound entry point used by `MainCmd::Tap`.

use std::cmp::min;
use std::time::{Duration, Instant};

use serde_json::Value;
use tokio::sync::{mpsc, oneshot};

use crate::gtk::prelude::*;
use crate::input::{resolve_xy, tap_widget, type_text, TapError, TypeError};
use crate::main_thread::{MainCmd, WaitEvalError, WaitTickOutcome, WaitTickResult};
use crate::proto::{TapTarget, TypeRequest, WaitCondition, WaitResult};
use crate::tree::{find_first, parse_selector, GtkTree, WidgetTree};

/// Polling interval (plan §Q7: fixed 100 ms internal).
pub const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Errors returned to the HTTP layer for `/test/wait`. Plan §Q8.
#[derive(Debug, Clone, PartialEq)]
pub enum WaitError {
    InvalidTimeout(&'static str),
    Eval(WaitEvalError),
    Timeout,
    Internal(String),
}

impl std::fmt::Display for WaitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WaitError::InvalidTimeout(reason) => write!(f, "invalid_timeout: {reason}"),
            WaitError::Eval(e) => write!(f, "eval: {e:?}"),
            WaitError::Timeout => write!(f, "wait_timeout"),
            WaitError::Internal(msg) => write!(f, "internal: {msg}"),
        }
    }
}

impl std::error::Error for WaitError {}

/// Maximum allowed `timeout_ms` (plan §Q8 任意 m3: 10 minutes).
pub const MAX_TIMEOUT_MS: u64 = 600_000;

/// Polling driver. Round-trips through `cmd_tx` once per tick; returns 200's
/// `WaitResult { elapsed_ms }` on match, `WaitError::Timeout` on deadline.
pub async fn poll_until(
    cmd_tx: &mpsc::Sender<MainCmd>,
    condition: WaitCondition,
    timeout_ms: u64,
) -> Result<WaitResult, WaitError> {
    if timeout_ms == 0 {
        return Err(WaitError::InvalidTimeout("zero"));
    }
    if timeout_ms > MAX_TIMEOUT_MS {
        return Err(WaitError::InvalidTimeout("excessive"));
    }
    let started = Instant::now();
    let deadline = started + Duration::from_millis(timeout_ms);
    loop {
        let (tx, rx) = oneshot::channel();
        cmd_tx
            .send(MainCmd::EvalWait {
                condition: condition.clone(),
                reply: tx,
            })
            .await
            .map_err(|_| WaitError::Internal("main_thread channel closed".into()))?;
        let tick = rx
            .await
            .map_err(|_| WaitError::Internal("main_thread reply dropped".into()))?;
        match tick {
            WaitTickResult::Outcome(WaitTickOutcome::Matched) => {
                return Ok(WaitResult {
                    elapsed_ms: started.elapsed().as_millis() as u64,
                });
            }
            WaitTickResult::Outcome(WaitTickOutcome::NotYet) | WaitTickResult::SelectorNotFound => {
                // tick failure (plan Review C2): keep polling
            }
            WaitTickResult::PermanentFailure(e) => return Err(WaitError::Eval(e)),
        }
        let now = Instant::now();
        if now >= deadline {
            return Err(WaitError::Timeout);
        }
        let next = min(now + POLL_INTERVAL, deadline);
        tokio::time::sleep_until(next.into()).await;
    }
}

// ------------------------------------------------------------
// GTK-bound entry points (called from the GLib main thread via MainCmd)
// ------------------------------------------------------------

pub(crate) fn dispatch_tap(
    app: &crate::gtk::Application,
    target: &TapTarget,
) -> Result<(), TapError> {
    match target {
        TapTarget::Selector { selector } => {
            let sel = parse_selector(selector).map_err(|e| TapError::SelectorNotFound {
                // The HTTP layer already validates `invalid_selector`, but a
                // `tap` targeting an unparseable name should still surface as
                // 422. We cannot construct that variant here, so propagate
                // SelectorNotFound and let the validator catch the case
                // earlier in the request lifecycle.
                selector: format!("{}: {}", selector, e.reason),
            })?;
            let tree = GtkTree { app };
            let widget = find_first(tree, &sel).ok_or_else(|| TapError::SelectorNotFound {
                selector: selector.clone(),
            })?;
            tap_widget(&widget, Some(selector))
        }
        TapTarget::Xy { xy } => {
            use crate::gtk::prelude::*;
            let window = app
                .active_window()
                .ok_or(TapError::NoActiveWindow)?
                .downcast::<crate::gtk::ApplicationWindow>()
                .map_err(|_| TapError::NoActiveWindow)?;
            let widget = resolve_xy(&window, xy.x, xy.y)?;
            tap_widget(&widget, None)
        }
    }
}

/// GTK-bound entry point for `MainCmd::Type` (Step 9).
///
/// Mirrors `dispatch_tap` for the selector path: parse selector, resolve via
/// `GtkTree`, then run the visibility / sensitivity / kind checks inside
/// `type_text`. Selector-only — no xy variant for `type` in the MVP.
pub(crate) fn dispatch_type(
    app: &crate::gtk::Application,
    req: &TypeRequest,
) -> Result<(), TypeError> {
    // The HTTP layer pre-validates the selector and returns 422
    // `invalid_selector` before reaching this dispatch (see
    // `http.rs::post_type`). The `map_err` below is defensive: if that
    // pre-validation is ever removed or bypassed, surface the parse error
    // as `SelectorNotFound` (404) rather than panic. Mirror of
    // `dispatch_tap` (wait.rs:107-114).
    let sel = parse_selector(&req.selector).map_err(|e| TypeError::SelectorNotFound {
        selector: format!("{}: {}", req.selector, e.reason),
    })?;
    let tree = GtkTree { app };
    let widget = find_first(tree, &sel).ok_or_else(|| TypeError::SelectorNotFound {
        selector: req.selector.clone(),
    })?;
    type_text(&widget, &req.text, Some(&req.selector))
}

pub(crate) fn eval_condition_in_app(
    app: &crate::gtk::Application,
    condition: &WaitCondition,
) -> WaitTickResult {
    let tree = GtkTree { app };
    eval_condition(tree, condition)
}

/// Evaluate one tick of a `WaitCondition` against any `WidgetTree` impl.
///
/// Pure (no `gtk::init()` required) so this is the function exercised by the
/// mock-tree unit tests in `wait_unit::*`.
pub fn eval_condition<'a, T>(tree: T, condition: &WaitCondition) -> WaitTickResult
where
    T: WidgetTree<'a>,
    T::Node: WidgetLike,
{
    match condition {
        WaitCondition::SelectorVisible { selector } => {
            let parsed = match parse_selector(selector) {
                Ok(s) => s,
                Err(e) => {
                    return WaitTickResult::PermanentFailure(WaitEvalError::InvalidSelector(
                        e.reason.into(),
                    ));
                }
            };
            match find_first(tree, &parsed) {
                None => WaitTickResult::SelectorNotFound,
                Some(node) => {
                    if node.is_visible_and_mapped() {
                        WaitTickResult::Outcome(WaitTickOutcome::Matched)
                    } else {
                        WaitTickResult::Outcome(WaitTickOutcome::NotYet)
                    }
                }
            }
        }
        WaitCondition::StateEq {
            selector,
            property,
            value,
        } => {
            let parsed = match parse_selector(selector) {
                Ok(s) => s,
                Err(e) => {
                    return WaitTickResult::PermanentFailure(WaitEvalError::InvalidSelector(
                        e.reason.into(),
                    ));
                }
            };
            match find_first(tree, &parsed) {
                None => WaitTickResult::SelectorNotFound,
                Some(node) => match node.read_property_as_json(property) {
                    Ok(v) => {
                        if &v == value {
                            WaitTickResult::Outcome(WaitTickOutcome::Matched)
                        } else {
                            WaitTickResult::Outcome(WaitTickOutcome::NotYet)
                        }
                    }
                    Err(PropReadError::Unsupported(t)) => {
                        WaitTickResult::PermanentFailure(WaitEvalError::UnsupportedPropertyType(t))
                    }
                    Err(PropReadError::Missing) => WaitTickResult::Outcome(WaitTickOutcome::NotYet),
                },
            }
        }
    }
}

/// Errors returned by `WidgetLike::read_property_as_json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PropReadError {
    Unsupported(String),
    Missing,
}

/// Behaviour required from a tree node for `eval_condition` to operate on it.
///
/// GTK `Widget` implements this for production; mock fixtures in the unit
/// tests provide their own implementation.
pub trait WidgetLike {
    fn is_visible_and_mapped(&self) -> bool;
    fn read_property_as_json(&self, property: &str) -> Result<Value, PropReadError>;
}

impl WidgetLike for crate::gtk::Widget {
    fn is_visible_and_mapped(&self) -> bool {
        self.is_visible() && self.is_mapped()
    }
    fn read_property_as_json(&self, property: &str) -> Result<Value, PropReadError> {
        // GObject property lookup. The pspec discovery makes this safe: if the
        // widget exposes no such property we return Missing (tick failure),
        // and if its type is outside our MVP support we return Unsupported
        // (permanent 422). Plan §Q6: MVP types = String, bool, i32, f64.
        let pspec = match self.find_property(property) {
            Some(p) => p,
            None => return Err(PropReadError::Missing),
        };
        let v = self.property_value(property);
        let type_name = pspec.value_type().name().to_string();
        if let Ok(s) = v.get::<String>() {
            return Ok(Value::String(s));
        }
        if let Ok(b) = v.get::<bool>() {
            return Ok(Value::Bool(b));
        }
        if let Ok(i) = v.get::<i32>() {
            return Ok(Value::from(i));
        }
        if let Ok(f) = v.get::<f64>() {
            return serde_json::Number::from_f64(f)
                .map(Value::Number)
                .ok_or(PropReadError::Unsupported(type_name));
        }
        Err(PropReadError::Unsupported(type_name))
    }
}
