//! Synthetic tap input.
//!
//! Plan §Q3 (Review M3): MVP supports `Button` only via `emit_clicked()`. All
//! other widget kinds return `TapError::UnsupportedWidget`. xy → widget
//! resolution lives here too, scoped to the active `ApplicationWindow`.
//!
//! Plan T014 §4 adds `swipe`: validation + `SwipeAnimation` planning
//! (`validate`) and a GLib-timer-based animator (`SwipeAnimation::run`).
//! Motion event synthesis is impossible from gtk4-rs safe APIs, so swipe is
//! implemented by linearly animating the nearest ancestor `ScrolledWindow`'s
//! `vadjustment` / `hadjustment` — same pragmatic shortcut as `tap` using
//! `emit_clicked()` instead of synthesizing a button-press event.

use crate::gtk;
use crate::proto::XY;
use gtk::glib;
use gtk::prelude::*;

/// Domain errors surfaced by the tap pipeline.
///
/// Mapped to HTTP status codes in `http.rs` (see plan §Q4):
///
/// | error                           | http |
/// |---------------------------------|------|
/// | `SelectorNotFound`              | 404  |
/// | `NoWidgetAtPoint`               | 404  |
/// | `UnsupportedWidget`             | 422  |
/// | `WidgetNotVisible`              | 422  |
/// | `WidgetDisabled`                | 422  |
/// | `OutOfBounds`                   | 422  |
/// | `NoActiveWindow`                | 422  |
#[derive(Debug, Clone, PartialEq)]
pub enum TapError {
    SelectorNotFound { selector: String },
    NoWidgetAtPoint { x: i32, y: i32 },
    UnsupportedWidget { widget_type: String },
    WidgetNotVisible { selector: Option<String> },
    WidgetDisabled { selector: Option<String> },
    OutOfBounds { x: i32, y: i32 },
    NoActiveWindow,
}

impl std::fmt::Display for TapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TapError::SelectorNotFound { selector } => {
                write!(f, "selector_not_found: {selector}")
            }
            TapError::NoWidgetAtPoint { x, y } => {
                write!(f, "no_widget_at_point: ({x}, {y})")
            }
            TapError::UnsupportedWidget { widget_type } => {
                write!(f, "tap_unsupported_widget: {widget_type}")
            }
            TapError::WidgetNotVisible { selector } => match selector {
                Some(s) => write!(f, "widget_not_visible: {s}"),
                None => write!(f, "widget_not_visible"),
            },
            TapError::WidgetDisabled { selector } => match selector {
                Some(s) => write!(f, "widget_disabled: {s}"),
                None => write!(f, "widget_disabled"),
            },
            TapError::OutOfBounds { x, y } => write!(f, "out_of_bounds: ({x}, {y})"),
            TapError::NoActiveWindow => write!(f, "no_active_window"),
        }
    }
}

impl std::error::Error for TapError {}

/// Synthesize a tap on the given widget.
///
/// Supports `Button` (`emit_clicked`) plus the active-stateful trio
/// `Switch` / `CheckButton` / `ToggleButton` via `set_active(!active)` —
/// `Activatable` was removed in GTK4 so we toggle the per-widget property
/// directly, which fires `notify::active` for app-side observers (T019).
/// Other widget kinds error with `UnsupportedWidget`. Visibility / sensitivity
/// gates run first so the returned error matches the user's mental model.
///
/// Dispatch order is derived → base: `gtk::CheckButton` is *not* a `gtk::Button`
/// in GTK4 (unlike GTK3), but `gtk::ToggleButton` *is*, so we check
/// `ToggleButton` before `Button`.
pub fn tap_widget(widget: &gtk::Widget, selector: Option<&str>) -> Result<(), TapError> {
    let plan = validate_tap(widget, selector)?;
    plan.fire();
    Ok(())
}

/// A validated tap, split from its side-effect so the action can either fire
/// synchronously ([`TapPlan::fire`], used by direct callers / unit tests) or be
/// deferred to a GLib idle callback ([`TapPlan::run_deferred`], used by the
/// HTTP dispatch path).
///
/// Issue #10: firing `emit_clicked()` / `set_active()` synchronously from
/// inside the GLib command-receiver loop is fine for normal widgets, but when
/// the target lives inside an **autohide (modal) `GtkPopover`**, the app's
/// `clicked` handler typically pops the popover down — and doing that
/// *synchronously while the popover's modal grab is still active* re-enters the
/// grab teardown on Wayland and can stall the receiver loop before its
/// `reply.send`. The HTTP caller then never gets a response (curl hangs) and
/// the broken grab leaves the UI unresponsive. Deferring the action to an idle
/// callback lets the current dispatch unwind first, so the reply is always sent
/// and the grab tears down cleanly on the next loop iteration.
pub struct TapPlan {
    widget: gtk::Widget,
}

impl TapPlan {
    /// Perform the tap action now (synchronous). Used by [`tap_widget`] and the
    /// integration tests that assert the handler fired immediately.
    pub fn fire(&self) {
        let widget = &self.widget;
        if let Some(switch) = widget.downcast_ref::<gtk::Switch>() {
            switch.set_active(!switch.is_active());
        } else if let Some(check) = widget.downcast_ref::<gtk::CheckButton>() {
            check.set_active(!check.is_active());
        } else if let Some(toggle) = widget.downcast_ref::<gtk::ToggleButton>() {
            toggle.set_active(!toggle.is_active());
        } else if let Some(button) = widget.downcast_ref::<gtk::Button>() {
            button.emit_clicked();
        }
        // `validate_tap` already rejected unsupported kinds, so the final
        // `else` is unreachable; nothing to do.
    }

    /// Schedule the tap action on a GLib idle callback and send `reply` once it
    /// has fired. Must be called on the GLib main thread. The reply carries
    /// `Ok(())` because validation already succeeded; the idle callback only
    /// performs the (possibly grab-reentrant) side-effect. See [`TapPlan`].
    pub fn run_deferred(self, reply: tokio::sync::oneshot::Sender<Result<(), TapError>>) {
        let mut reply_slot = Some(reply);
        glib::idle_add_local_once(move || {
            self.fire();
            if let Some(reply) = reply_slot.take() {
                let _ = reply.send(Ok(()));
            }
        });
    }
}

/// Run the synchronous tap guards (visibility / sensitivity / kind) and return
/// a [`TapPlan`] for the resolved action, or a [`TapError`] when a guard fails.
///
/// Pure with respect to the widget: it reads state and downcasts but does not
/// fire `emit_clicked()` / `set_active()`. Splitting validation from the action
/// lets the HTTP path reply synchronously on error yet defer the side-effect
/// (issue #10). Supported kinds mirror the historical `tap_widget` dispatch
/// order (`Switch` / `CheckButton` / `ToggleButton` before `Button`, since a
/// `ToggleButton` *is* a `Button` in GTK4).
pub fn validate_tap(widget: &gtk::Widget, selector: Option<&str>) -> Result<TapPlan, TapError> {
    if !widget.is_visible() || !widget.is_mapped() {
        return Err(TapError::WidgetNotVisible {
            selector: selector.map(str::to_string),
        });
    }
    if !widget.is_sensitive() {
        return Err(TapError::WidgetDisabled {
            selector: selector.map(str::to_string),
        });
    }
    let supported = widget.downcast_ref::<gtk::Switch>().is_some()
        || widget.downcast_ref::<gtk::CheckButton>().is_some()
        || widget.downcast_ref::<gtk::ToggleButton>().is_some()
        || widget.downcast_ref::<gtk::Button>().is_some();
    if !supported {
        return Err(TapError::UnsupportedWidget {
            widget_type: widget.type_().name().to_string(),
        });
    }
    Ok(TapPlan {
        widget: widget.clone(),
    })
}

/// Locate a widget at window-local coordinates inside `window`.
///
/// Returns the deepest widget whose bounds contain `(x, y)`. Returns
/// `OutOfBounds` if `(x, y)` is not inside the window itself.
pub fn resolve_xy(
    window: &gtk::ApplicationWindow,
    x: i32,
    y: i32,
) -> Result<gtk::Widget, TapError> {
    let w_alloc = window.allocation();
    if x < 0 || y < 0 || x >= w_alloc.width() || y >= w_alloc.height() {
        return Err(TapError::OutOfBounds { x, y });
    }
    let root: gtk::Widget = window.clone().upcast();
    let hit = pick_at(&root, &root, x as f64, y as f64);
    hit.ok_or(TapError::NoWidgetAtPoint { x, y })
}

fn pick_at(parent: &gtk::Widget, widget: &gtk::Widget, x: f64, y: f64) -> Option<gtk::Widget> {
    let mut deepest: Option<gtk::Widget> = None;
    if widget_contains_point(parent, widget, x, y) {
        deepest = Some(widget.clone());
        let mut cur = widget.first_child();
        while let Some(child) = cur {
            if let Some(hit) = pick_at(parent, &child, x, y) {
                deepest = Some(hit);
            }
            cur = child.next_sibling();
        }
    }
    deepest
}

fn widget_contains_point(parent: &gtk::Widget, widget: &gtk::Widget, x: f64, y: f64) -> bool {
    if let Some(rect) = widget.compute_bounds(parent) {
        let ox = rect.x() as f64;
        let oy = rect.y() as f64;
        let w = rect.width() as f64;
        let h = rect.height() as f64;
        x >= ox && y >= oy && x < ox + w && y < oy + h
    } else {
        false
    }
}

// ---------------------------------------------------------------------------
// Type (T013, plan §2)
// ---------------------------------------------------------------------------

/// Domain errors surfaced by the `type` pipeline (Step 9).
///
/// Mapped to HTTP status codes in `http.rs`:
///
/// | error              | http |
/// |--------------------|------|
/// | `SelectorNotFound` | 404  |
/// | `UnsupportedWidget`| 422  |
/// | `WidgetNotVisible` | 422  |
/// | `WidgetDisabled`   | 422  |
/// | `NoActiveWindow`   | 422  |
#[derive(Debug, Clone, PartialEq)]
pub enum TypeError {
    SelectorNotFound { selector: String },
    UnsupportedWidget { widget_type: String },
    WidgetNotVisible { selector: Option<String> },
    WidgetDisabled { selector: Option<String> },
    NoActiveWindow,
}

impl std::fmt::Display for TypeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TypeError::SelectorNotFound { selector } => {
                write!(f, "selector_not_found: {selector}")
            }
            TypeError::UnsupportedWidget { widget_type } => {
                write!(f, "type_unsupported_widget: {widget_type}")
            }
            TypeError::WidgetNotVisible { selector } => match selector {
                Some(s) => write!(f, "widget_not_visible: {s}"),
                None => write!(f, "widget_not_visible"),
            },
            TypeError::WidgetDisabled { selector } => match selector {
                Some(s) => write!(f, "widget_disabled: {s}"),
                None => write!(f, "widget_disabled"),
            },
            TypeError::NoActiveWindow => write!(f, "no_active_window"),
        }
    }
}

impl std::error::Error for TypeError {}

/// Replace the text content of an `Editable` (`Entry` / `SearchEntry` /
/// `PasswordEntry` / `SpinButton` / `Text`) or `TextView` widget with `text`.
///
/// MVP semantics (plan §2.2): full replacement, not "insert at cursor".
/// Visibility and sensitivity guards run before kind dispatch so the error
/// surface matches `tap_widget`'s mental model.
pub fn type_text(
    widget: &gtk::Widget,
    text: &str,
    selector: Option<&str>,
) -> Result<(), TypeError> {
    if !widget.is_visible() || !widget.is_mapped() {
        return Err(TypeError::WidgetNotVisible {
            selector: selector.map(str::to_string),
        });
    }
    if !widget.is_sensitive() {
        return Err(TypeError::WidgetDisabled {
            selector: selector.map(str::to_string),
        });
    }
    if let Some(editable) = widget.dynamic_cast_ref::<gtk::Editable>() {
        editable.set_text(text);
        return Ok(());
    }
    if let Some(tv) = widget.downcast_ref::<gtk::TextView>() {
        tv.buffer().set_text(text);
        return Ok(());
    }
    Err(TypeError::UnsupportedWidget {
        widget_type: widget.type_().name().to_string(),
    })
}

// ---------------------------------------------------------------------------
// Focus (issue #3)
// ---------------------------------------------------------------------------

/// Domain errors surfaced by the `focus` pipeline (issue #3).
///
/// Mapped to HTTP status codes in `http.rs` (mirror of `TypeError`):
///
/// | error              | http |
/// |--------------------|------|
/// | `SelectorNotFound` | 404  |
/// | `FocusRejected`    | 422  |
/// | `WidgetNotVisible` | 422  |
/// | `WidgetDisabled`   | 422  |
/// | `NoActiveWindow`   | 422  |
#[derive(Debug, Clone, PartialEq)]
pub enum FocusError {
    SelectorNotFound { selector: String },
    FocusRejected { selector: Option<String> },
    WidgetNotVisible { selector: Option<String> },
    WidgetDisabled { selector: Option<String> },
    NoActiveWindow,
}

impl std::fmt::Display for FocusError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FocusError::SelectorNotFound { selector } => {
                write!(f, "selector_not_found: {selector}")
            }
            FocusError::FocusRejected { selector } => match selector {
                Some(s) => write!(f, "focus_rejected: {s}"),
                None => write!(f, "focus_rejected"),
            },
            FocusError::WidgetNotVisible { selector } => match selector {
                Some(s) => write!(f, "widget_not_visible: {s}"),
                None => write!(f, "widget_not_visible"),
            },
            FocusError::WidgetDisabled { selector } => match selector {
                Some(s) => write!(f, "widget_disabled: {s}"),
                None => write!(f, "widget_disabled"),
            },
            FocusError::NoActiveWindow => write!(f, "no_active_window"),
        }
    }
}

impl std::error::Error for FocusError {}

/// Move keyboard focus to `widget` via `grab_focus()`, enabling `:focus` /
/// `:focus-within` dependent CSS to render for deterministic screenshot
/// verification (issue #3).
///
/// Visibility and sensitivity guards run before the grab so the error surface
/// matches `tap_widget` / `type_text`. `grab_focus()` returns `false` when the
/// widget cannot take focus (e.g. a `Label`, or a widget with
/// `can_focus == false`); that becomes `FocusRejected` (422) rather than a
/// silent no-op, so callers learn the target is not focusable.
pub fn focus_widget(widget: &gtk::Widget, selector: Option<&str>) -> Result<(), FocusError> {
    if !widget.is_visible() || !widget.is_mapped() {
        return Err(FocusError::WidgetNotVisible {
            selector: selector.map(str::to_string),
        });
    }
    if !widget.is_sensitive() {
        return Err(FocusError::WidgetDisabled {
            selector: selector.map(str::to_string),
        });
    }
    if widget.grab_focus() {
        Ok(())
    } else {
        Err(FocusError::FocusRejected {
            selector: selector.map(str::to_string),
        })
    }
}

// ---------------------------------------------------------------------------
// Swipe (T014, plan §4)
// ---------------------------------------------------------------------------

/// Domain errors surfaced by the swipe pipeline.
///
/// Mapped to HTTP status codes in `http.rs` (see plan T014 §5):
///
/// | error                    | http |
/// |--------------------------|------|
/// | `OutOfBounds`            | 422  |
/// | `NoActiveWindow`         | 422  |
/// | `NoScrollableAtPoint`    | 404  |
/// | `ZeroDuration`           | 422  |
/// | `DurationTooLong`        | 422  |
#[derive(Debug, Clone, PartialEq)]
pub enum SwipeError {
    OutOfBounds { x: i32, y: i32 },
    NoActiveWindow,
    NoScrollableAtPoint { x: i32, y: i32 },
    ZeroDuration,
    DurationTooLong { duration_ms: u64 },
}

impl std::fmt::Display for SwipeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SwipeError::OutOfBounds { x, y } => write!(f, "out_of_bounds: ({x}, {y})"),
            SwipeError::NoActiveWindow => write!(f, "no_active_window"),
            SwipeError::NoScrollableAtPoint { x, y } => {
                write!(f, "no_scrollable_at_point: ({x}, {y})")
            }
            SwipeError::ZeroDuration => write!(f, "invalid_duration: zero"),
            SwipeError::DurationTooLong { duration_ms } => {
                write!(f, "invalid_duration: too_long ({duration_ms} ms)")
            }
        }
    }
}

impl std::error::Error for SwipeError {}

/// Upper bound on `duration_ms`. Plan §10 R-4: short by design — a single
/// gesture that takes longer than 10 s in tests is almost certainly wrong.
pub const MAX_SWIPE_DURATION_MS: u64 = 10_000;

const SWIPE_TARGET_FPS: u64 = 60;

/// Target frame rate for `PinchAnimation`. Mirrors `SWIPE_TARGET_FPS` so the
/// `frames` / `frame_interval_ms` arithmetic stays parallel between the two
/// animators.
const PINCH_TARGET_FPS: u64 = 60;

/// Plan output from [`validate`]. Holds adjustment refs and the precomputed
/// animation parameters; calling [`SwipeAnimation::run`] schedules the GLib
/// timer and consumes `self`. Pure (no widget mutation, no timer scheduled
/// yet) so HTTP handlers can examine validation outcomes off the GLib thread.
pub struct SwipeAnimation {
    vadj: gtk::Adjustment,
    hadj: gtk::Adjustment,
    v_start: f64,
    h_start: f64,
    dx: f64,
    dy: f64,
    frames: u64,
    frame_interval_ms: u64,
}

impl SwipeAnimation {
    /// Schedule the animation via `glib::timeout_add_local`. `on_complete` is
    /// called once after the final frame fires. Must be called on the GLib
    /// main thread.
    pub fn run<F: FnOnce() + 'static>(self, on_complete: F) {
        let SwipeAnimation {
            vadj,
            hadj,
            v_start,
            h_start,
            dx,
            dy,
            frames,
            frame_interval_ms,
        } = self;
        let mut current = 0u64;
        let mut on_complete_slot = Some(on_complete);
        glib::timeout_add_local(
            std::time::Duration::from_millis(frame_interval_ms),
            move || {
                current += 1;
                let t = (current as f64 / frames as f64).min(1.0);
                vadj.set_value((v_start + dy * t).round());
                hadj.set_value((h_start + dx * t).round());
                if current >= frames {
                    if let Some(cb) = on_complete_slot.take() {
                        cb();
                    }
                    glib::ControlFlow::Break
                } else {
                    glib::ControlFlow::Continue
                }
            },
        );
    }
}

/// Validate a swipe request and prepare a [`SwipeAnimation`].
///
/// Pure: does not schedule a GLib timer or mutate any widget. The returned
/// animation must be `run` on the GLib main thread.
pub fn validate(
    window: &gtk::ApplicationWindow,
    from: XY,
    to: XY,
    duration_ms: u64,
) -> Result<SwipeAnimation, SwipeError> {
    if duration_ms == 0 {
        return Err(SwipeError::ZeroDuration);
    }
    if duration_ms > MAX_SWIPE_DURATION_MS {
        return Err(SwipeError::DurationTooLong { duration_ms });
    }

    // gtk's `Widget::allocation()` lags one frame behind the toplevel becoming
    // mapped on some backends (notably macOS quartz). When that happens we fall
    // back to the window's default size — still a reasonable bounds check
    // because `resolve_xy` further restricts the point to a hit-tested widget.
    let alloc = window.allocation();
    let (mut w, mut h) = (alloc.width(), alloc.height());
    if w <= 0 || h <= 0 {
        let (dw, dh) = window.default_size();
        if dw > 0 {
            w = dw;
        }
        if dh > 0 {
            h = dh;
        }
    }
    if from.x < 0 || from.y < 0 || from.x >= w || from.y >= h {
        return Err(SwipeError::OutOfBounds {
            x: from.x,
            y: from.y,
        });
    }

    let leaf = match resolve_xy(window, from.x, from.y) {
        Ok(w) => w,
        Err(TapError::OutOfBounds { x, y }) => return Err(SwipeError::OutOfBounds { x, y }),
        Err(_) => {
            return Err(SwipeError::NoScrollableAtPoint {
                x: from.x,
                y: from.y,
            })
        }
    };

    // First-choice: gtk4-rs `Widget::ancestor(Type)` returns the nearest
    // ancestor of the given GType. Fall back to a hand-written walker when the
    // first-choice misses (e.g. when the widget itself is the ScrolledWindow).
    let scrolled = leaf
        .ancestor(gtk::ScrolledWindow::static_type())
        .and_then(|w| w.downcast::<gtk::ScrolledWindow>().ok())
        .or_else(|| find_scrolled_ancestor(&leaf))
        .ok_or(SwipeError::NoScrollableAtPoint {
            x: from.x,
            y: from.y,
        })?;

    let dx = (from.x - to.x) as f64;
    let dy = (from.y - to.y) as f64;

    let vadj = scrolled.vadjustment();
    let hadj = scrolled.hadjustment();
    let v_start = vadj.value();
    let h_start = hadj.value();

    let frames = ((duration_ms * SWIPE_TARGET_FPS) / 1000).max(1);
    let frame_interval_ms = std::cmp::max(duration_ms / frames, 1);

    Ok(SwipeAnimation {
        vadj,
        hadj,
        v_start,
        h_start,
        dx,
        dy,
        frames,
        frame_interval_ms,
    })
}

/// Synthesize a swipe over `duration_ms`, returning once the animation is
/// scheduled (not when it completes). Provided for tests / fire-and-forget
/// callers; HTTP path uses [`validate`] + [`SwipeAnimation::run`] directly so
/// it can reply on completion.
pub fn swipe(
    window: &gtk::ApplicationWindow,
    from: XY,
    to: XY,
    duration_ms: u64,
) -> Result<(), SwipeError> {
    let anim = validate(window, from, to, duration_ms)?;
    anim.run(|| {});
    Ok(())
}

/// Walk parents from `widget` looking for the nearest `gtk::ScrolledWindow`.
/// Plan T014 §4.2: parent extraction must precede `downcast` because the
/// latter consumes `self`.
pub fn find_scrolled_ancestor(widget: &gtk::Widget) -> Option<gtk::ScrolledWindow> {
    let mut cur = Some(widget.clone());
    while let Some(w) = cur {
        let parent = w.parent();
        if let Ok(sw) = w.downcast::<gtk::ScrolledWindow>() {
            return Some(sw);
        }
        cur = parent;
    }
    None
}

// ---------------------------------------------------------------------------
// Pinch (T015, plan §3 / §5)
// ---------------------------------------------------------------------------

/// Domain errors surfaced by the pinch pipeline.
///
/// Mapped to HTTP status codes in `http.rs` (see plan T015 §6.1):
///
/// | error                    | http |
/// |--------------------------|------|
/// | `OutOfBounds`            | 422  |
/// | `NoActiveWindow`         | 422  |
/// | `NoPinchableAtPoint`     | 404  |
/// | `ZeroDuration`           | 422  |
/// | `DurationTooLong`        | 422  |
/// | `InvalidScale { reason }`| 422  |
#[derive(Debug, Clone, PartialEq)]
pub enum PinchError {
    OutOfBounds { x: i32, y: i32 },
    NoActiveWindow,
    NoPinchableAtPoint { x: i32, y: i32 },
    ZeroDuration,
    DurationTooLong { duration_ms: u64 },
    InvalidScale { reason: &'static str },
}

impl std::fmt::Display for PinchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PinchError::OutOfBounds { x, y } => write!(f, "out_of_bounds: ({x}, {y})"),
            PinchError::NoActiveWindow => write!(f, "no_active_window"),
            PinchError::NoPinchableAtPoint { x, y } => {
                write!(f, "no_pinchable_at_point: ({x}, {y})")
            }
            PinchError::ZeroDuration => write!(f, "invalid_duration: zero"),
            PinchError::DurationTooLong { duration_ms } => {
                write!(f, "invalid_duration: too_long ({duration_ms} ms)")
            }
            PinchError::InvalidScale { reason } => write!(f, "invalid_scale: {reason}"),
        }
    }
}

impl std::error::Error for PinchError {}

/// Upper bound on `duration_ms` (parallel to `MAX_SWIPE_DURATION_MS`).
pub const MAX_PINCH_DURATION_MS: u64 = 10_000;

/// Upper / lower bound on `scale` (symmetric: `1 / scale` is also bounded).
/// 50× zoom is comfortably outside any realistic UI test; rejecting beyond
/// this acts as a safety valve against integer overflow / draw-time pathology.
pub const MAX_PINCH_SCALE: f32 = 50.0;

/// Plan output from [`validate_pinch`]. Holds the resolved `GestureZoom` plus
/// the precomputed animation parameters; calling [`PinchAnimation::run`]
/// schedules the GLib timer and consumes `self`. Pure (no widget mutation, no
/// timer scheduled yet) so HTTP handlers can inspect validation outcomes off
/// the GLib thread before scheduling.
pub struct PinchAnimation {
    gesture: gtk::GestureZoom,
    target_scale: f64,
    frames: u64,
    frame_interval_ms: u64,
}

impl PinchAnimation {
    /// Schedule the animation via `glib::timeout_add_local`. `on_complete` is
    /// called once after the final frame fires. Must be called on the GLib
    /// main thread.
    ///
    /// Plan §3 Q1: emits `scale-changed` only (no `begin` / `end`). Each frame
    /// linearly interpolates from `1.0` (gesture identity) to `target_scale`
    /// in line with the `gdouble` cumulative-scale convention of GestureZoom.
    pub fn run<F: FnOnce() + 'static>(self, on_complete: F) {
        let PinchAnimation {
            gesture,
            target_scale,
            frames,
            frame_interval_ms,
        } = self;
        let mut current = 0u64;
        let mut on_complete_slot = Some(on_complete);
        glib::timeout_add_local(
            std::time::Duration::from_millis(frame_interval_ms),
            move || {
                current += 1;
                let t = (current as f64 / frames as f64).min(1.0);
                let cur_scale: f64 = 1.0 + (target_scale - 1.0) * t;
                // gtk4-rs ObjectExt::emit_by_name::<R>: signal returns void so
                // the turbofish must be `<()>`. Args are `&[&dyn ToValue]`;
                // borrowing a stack value here is fine — emit is synchronous.
                gesture.emit_by_name::<()>("scale-changed", &[&cur_scale]);
                if current >= frames {
                    if let Some(cb) = on_complete_slot.take() {
                        cb();
                    }
                    glib::ControlFlow::Break
                } else {
                    glib::ControlFlow::Continue
                }
            },
        );
    }
}

/// Validate a pinch request and prepare a [`PinchAnimation`].
///
/// Pure: does not schedule a GLib timer or mutate any widget. The returned
/// animation must be `run` on the GLib main thread.
///
/// Order of checks (plan §5.2):
/// 1. `scale` validation (NaN / Inf / non_positive / too_large / too_small).
/// 2. `duration_ms` validation (zero / too_long).
/// 3. window bounds (with `default_size()` fallback for pre-mapped quartz).
/// 4. xy resolution (leaf widget at `center`).
/// 5. ancestor walk for `GestureZoom`.
pub fn validate_pinch(
    window: &gtk::ApplicationWindow,
    center: XY,
    scale: f32,
    duration_ms: u64,
) -> Result<PinchAnimation, PinchError> {
    if scale.is_nan() {
        return Err(PinchError::InvalidScale { reason: "nan" });
    }
    if scale.is_infinite() {
        return Err(PinchError::InvalidScale { reason: "infinite" });
    }
    if scale <= 0.0 {
        return Err(PinchError::InvalidScale {
            reason: "non_positive",
        });
    }
    if scale > MAX_PINCH_SCALE {
        return Err(PinchError::InvalidScale {
            reason: "too_large",
        });
    }
    // Reciprocal bound: `1 / scale > MAX` ⇒ scale below the symmetric floor.
    if scale < 1.0 / MAX_PINCH_SCALE {
        return Err(PinchError::InvalidScale {
            reason: "too_small",
        });
    }

    if duration_ms == 0 {
        return Err(PinchError::ZeroDuration);
    }
    if duration_ms > MAX_PINCH_DURATION_MS {
        return Err(PinchError::DurationTooLong { duration_ms });
    }

    let alloc = window.allocation();
    let (mut w, mut h) = (alloc.width(), alloc.height());
    if w <= 0 || h <= 0 {
        let (dw, dh) = window.default_size();
        if dw > 0 {
            w = dw;
        }
        if dh > 0 {
            h = dh;
        }
    }
    if center.x < 0 || center.y < 0 || center.x >= w || center.y >= h {
        return Err(PinchError::OutOfBounds {
            x: center.x,
            y: center.y,
        });
    }

    let leaf = match resolve_xy(window, center.x, center.y) {
        Ok(w) => w,
        Err(TapError::OutOfBounds { x, y }) => return Err(PinchError::OutOfBounds { x, y }),
        Err(_) => {
            return Err(PinchError::NoPinchableAtPoint {
                x: center.x,
                y: center.y,
            });
        }
    };

    let gesture = find_zoom_gesture_ancestor(&leaf).ok_or(PinchError::NoPinchableAtPoint {
        x: center.x,
        y: center.y,
    })?;

    let frames = ((duration_ms * PINCH_TARGET_FPS) / 1000).max(1);
    let frame_interval_ms = std::cmp::max(duration_ms / frames, 1);

    Ok(PinchAnimation {
        gesture,
        target_scale: scale as f64,
        frames,
        frame_interval_ms,
    })
}

/// Synthesize a pinch over `duration_ms`, returning once the animation is
/// scheduled (not when it completes). Mirror of [`swipe`] for fire-and-forget
/// callers; the HTTP path uses [`validate_pinch`] + [`PinchAnimation::run`]
/// directly so it can reply on completion.
pub fn pinch(
    window: &gtk::ApplicationWindow,
    center: XY,
    scale: f32,
    duration_ms: u64,
) -> Result<(), PinchError> {
    let anim = validate_pinch(window, center, scale, duration_ms)?;
    anim.run(|| {});
    Ok(())
}

/// Walk parents from `widget` looking for a widget with an attached
/// `gtk::GestureZoom`. Returns the gesture so the caller can re-target it
/// without a second walk.
///
/// Plan §3 Q2: `observe_controllers()` returns a `gio::ListModel` of
/// `EventController`s. We iterate, downcasting each item to `GestureZoom`.
/// Parent extraction must precede `downcast` (consumes `self`) — same
/// pitfall as `find_scrolled_ancestor`.
pub fn find_zoom_gesture_ancestor(widget: &gtk::Widget) -> Option<gtk::GestureZoom> {
    let mut cur = Some(widget.clone());
    while let Some(w) = cur {
        let parent = w.parent();
        let controllers = w.observe_controllers();
        let n = controllers.n_items();
        for i in 0..n {
            if let Some(item) = controllers.item(i) {
                if let Ok(gz) = item.downcast::<gtk::GestureZoom>() {
                    return Some(gz);
                }
            }
        }
        cur = parent;
    }
    None
}

// ---------------------------------------------------------------------------
// Press (Task 029, T029) — GestureLongPress
// ---------------------------------------------------------------------------

/// Domain errors surfaced by the press (long-press) pipeline.
///
/// Mapped to HTTP status codes in `http.rs` (see plan §4):
///
/// | error                          | http |
/// |--------------------------------|------|
/// | `ZeroHold`                     | 422  |
/// | `HoldTooLong`                  | 422  |
/// | `OutOfBounds`                  | 422  |
/// | `NoActiveWindow`               | 422  |
/// | `InvalidTarget`                | 422  |
/// | `NoLongPressableAtPoint`       | 404  |
/// | `SelectorNotFound`             | 404  |
/// | `NoLongPressableForSelector`   | 404  |
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PressError {
    ZeroHold,
    HoldTooLong { hold_ms: u64 },
    OutOfBounds { x: i32, y: i32 },
    NoActiveWindow,
    NoLongPressableAtPoint { x: i32, y: i32 },
    SelectorNotFound { selector: String },
    NoLongPressableForSelector { selector: String },
    InvalidTarget { reason: &'static str },
}

impl std::fmt::Display for PressError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PressError::ZeroHold => write!(f, "invalid_hold: zero"),
            PressError::HoldTooLong { hold_ms } => {
                write!(f, "invalid_hold: too_long ({hold_ms} ms)")
            }
            PressError::OutOfBounds { x, y } => write!(f, "out_of_bounds: ({x}, {y})"),
            PressError::NoActiveWindow => write!(f, "no_active_window"),
            PressError::NoLongPressableAtPoint { x, y } => {
                write!(f, "no_long_pressable_at_point: ({x}, {y})")
            }
            PressError::SelectorNotFound { selector } => {
                write!(f, "selector_not_found: {selector}")
            }
            PressError::NoLongPressableForSelector { selector } => {
                write!(f, "no_long_pressable_for_selector: {selector}")
            }
            PressError::InvalidTarget { reason } => write!(f, "invalid_target: {reason}"),
        }
    }
}

impl std::error::Error for PressError {}

/// Upper bound on `hold_ms`. Plan §4: a single long-press that holds longer
/// than 10 s in a test is almost certainly a mistake. Single source of truth;
/// the HTTP layer references this for its R1 pre-validation.
pub const MAX_PRESS_HOLD_MS: u64 = 10_000;

/// Plan output from [`validate_press`] / [`validate_press_widget`]. Holds the
/// resolved `GestureLongPress`, the emit coordinates, and the hold delay.
/// Calling [`LongPressAnimation::run`] schedules a single-shot GLib timer and
/// consumes `self`. Pure (no widget mutation, no timer yet) so HTTP handlers
/// can inspect validation outcomes off the GLib thread before scheduling.
pub struct LongPressAnimation {
    gesture: gtk::GestureLongPress,
    x: f64,
    y: f64,
    hold_ms: u64,
}

impl LongPressAnimation {
    /// Schedule the press via a single-shot `glib::timeout_add_local` that fires
    /// once after `hold_ms`, emits the `pressed` signal at `(x, y)`, calls
    /// `on_complete`, and returns `ControlFlow::Break`. Must be called on the
    /// GLib main thread.
    ///
    /// Plan §4: `GestureLongPress::pressed` is `(gesture, x: f64, y: f64)`. As
    /// with `PinchAnimation`, the `emit_by_name` turbofish is `<()>` because the
    /// signal returns void.
    pub fn run<F: FnOnce() + 'static>(self, on_complete: F) {
        let LongPressAnimation {
            gesture,
            x,
            y,
            hold_ms,
        } = self;
        let mut on_complete_slot = Some(on_complete);
        glib::timeout_add_local(std::time::Duration::from_millis(hold_ms), move || {
            gesture.emit_by_name::<()>("pressed", &[&x, &y]);
            if let Some(cb) = on_complete_slot.take() {
                cb();
            }
            glib::ControlFlow::Break
        });
    }
}

/// Validate `hold_ms` against the shared bounds. Shared by both the xy and
/// selector entry points so the double-defense (HTTP R1 + GLib-side) checks
/// stay identical.
fn validate_hold(hold_ms: u64) -> Result<(), PressError> {
    if hold_ms == 0 {
        return Err(PressError::ZeroHold);
    }
    if hold_ms > MAX_PRESS_HOLD_MS {
        return Err(PressError::HoldTooLong { hold_ms });
    }
    Ok(())
}

/// Validate an xy-targeted press and prepare a [`LongPressAnimation`].
///
/// Pure: does not schedule a GLib timer or mutate any widget. Order of checks:
/// 1. `hold_ms` validation (zero / too_long).
/// 2. window bounds (with `default_size()` fallback for pre-mapped quartz).
/// 3. xy resolution (leaf widget at `center`).
/// 4. ancestor walk for `GestureLongPress`.
pub fn validate_press(
    window: &gtk::ApplicationWindow,
    center: XY,
    hold_ms: u64,
) -> Result<LongPressAnimation, PressError> {
    validate_hold(hold_ms)?;

    let alloc = window.allocation();
    let (mut w, mut h) = (alloc.width(), alloc.height());
    if w <= 0 || h <= 0 {
        let (dw, dh) = window.default_size();
        if dw > 0 {
            w = dw;
        }
        if dh > 0 {
            h = dh;
        }
    }
    if center.x < 0 || center.y < 0 || center.x >= w || center.y >= h {
        return Err(PressError::OutOfBounds {
            x: center.x,
            y: center.y,
        });
    }

    let leaf = match resolve_xy(window, center.x, center.y) {
        Ok(w) => w,
        Err(TapError::OutOfBounds { x, y }) => return Err(PressError::OutOfBounds { x, y }),
        Err(_) => {
            return Err(PressError::NoLongPressableAtPoint {
                x: center.x,
                y: center.y,
            });
        }
    };

    let gesture =
        find_long_press_gesture_ancestor(&leaf).ok_or(PressError::NoLongPressableAtPoint {
            x: center.x,
            y: center.y,
        })?;

    Ok(LongPressAnimation {
        gesture,
        x: center.x as f64,
        y: center.y as f64,
        hold_ms,
    })
}

/// Validate a selector-targeted press against an already-resolved `widget` and
/// prepare a [`LongPressAnimation`].
///
/// Pure. The `GestureLongPress` is searched on the widget or an ancestor
/// (N1: not found ⇒ `NoLongPressableForSelector`). Emit coordinates are the
/// widget centre via `widget.compute_bounds(widget)` (`(w/2, h/2)`, falling
/// back to `(0.0, 0.0)`); the coordinates are informational for
/// `GestureLongPress`, which fires regardless of the exact point.
pub fn validate_press_widget(
    widget: &gtk::Widget,
    selector: &str,
    hold_ms: u64,
) -> Result<LongPressAnimation, PressError> {
    validate_hold(hold_ms)?;

    let gesture = find_long_press_gesture_ancestor(widget).ok_or_else(|| {
        PressError::NoLongPressableForSelector {
            selector: selector.to_string(),
        }
    })?;

    let (x, y) = widget
        .compute_bounds(widget)
        .map(|r| ((r.width() / 2.0) as f64, (r.height() / 2.0) as f64))
        .unwrap_or((0.0, 0.0));

    Ok(LongPressAnimation {
        gesture,
        x,
        y,
        hold_ms,
    })
}

/// Synthesize an xy-targeted press, returning once the press is scheduled (not
/// when it completes). Mirror of [`pinch`] for fire-and-forget callers; the
/// HTTP path uses [`validate_press`] + [`LongPressAnimation::run`] directly so
/// it can reply on completion.
pub fn press(window: &gtk::ApplicationWindow, center: XY, hold_ms: u64) -> Result<(), PressError> {
    let anim = validate_press(window, center, hold_ms)?;
    anim.run(|| {});
    Ok(())
}

/// Walk parents from `widget` looking for a widget with an attached
/// `gtk::GestureLongPress`. Returns the gesture so the caller can re-target it
/// without a second walk. Mirror of [`find_zoom_gesture_ancestor`].
pub fn find_long_press_gesture_ancestor(widget: &gtk::Widget) -> Option<gtk::GestureLongPress> {
    let mut cur = Some(widget.clone());
    while let Some(w) = cur {
        let parent = w.parent();
        let controllers = w.observe_controllers();
        let n = controllers.n_items();
        for i in 0..n {
            if let Some(item) = controllers.item(i) {
                if let Ok(glp) = item.downcast::<gtk::GestureLongPress>() {
                    return Some(glp);
                }
            }
        }
        cur = parent;
    }
    None
}

// ---------------------------------------------------------------------------
// Key (issue #10 companion: POST /test/key)
// ---------------------------------------------------------------------------

/// Domain errors surfaced by the `key` pipeline.
///
/// Mapped to HTTP status codes in `http.rs`:
///
/// | error            | http |
/// |------------------|------|
/// | `UnsupportedKey` | 422  |
/// | `NoActiveWindow` | 422  |
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyError {
    UnsupportedKey { key: String },
    NoActiveWindow,
}

impl std::fmt::Display for KeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KeyError::UnsupportedKey { key } => write!(f, "unsupported_key: {key}"),
            KeyError::NoActiveWindow => write!(f, "no_active_window"),
        }
    }
}

impl std::error::Error for KeyError {}

/// Deliver a single key to the active window (MVP: `"Escape"` only).
///
/// Issue #10: `/test/key` is the safe escape hatch for dismissing a modal
/// `GtkPopover` from a test without clicking a child widget (which can re-enter
/// the modal grab). For `Escape` we reproduce the documented popover behaviour
/// directly — pop down the top-most open autohide popover under the active
/// window — rather than synthesizing a `GdkEvent` (GTK4 removed public event
/// construction, and emitting `key-pressed` on a controller does not trigger
/// GTK's default Escape-to-close handling).
///
/// `popdown()` is the documented effect of Escape on an autohide popover and is
/// grab-safe: GTK tears the grab down internally. When no popover is open the
/// call is a no-op success (the modal is already gone), so callers can issue
/// `/test/key Escape` unconditionally as cleanup.
///
/// Returns `UnsupportedKey` for any key name other than `"Escape"` /
/// `"escape"`; the MVP intentionally does not synthesize arbitrary keystrokes.
pub fn send_key(window: &gtk::Window, key: &str) -> Result<(), KeyError> {
    if !key.eq_ignore_ascii_case("escape") {
        return Err(KeyError::UnsupportedKey {
            key: key.to_string(),
        });
    }
    if let Some(popover) = find_open_autohide_popover(&window.clone().upcast()) {
        popover.popdown();
    }
    Ok(())
}

/// DFS for the first mapped autohide `GtkPopover` in `root`'s widget subtree.
///
/// A `GtkPopover` is a child widget of the toplevel in the GTK4 hierarchy (it
/// presents on its own surface but is still reachable via
/// `first_child` / `next_sibling`), so the same walk used by `find_first`
/// reaches it. We only return popovers that are both mapped (open) and
/// autohide (modal) — those are the ones an Escape would dismiss.
pub fn find_open_autohide_popover(root: &gtk::Widget) -> Option<gtk::Popover> {
    if let Some(popover) = root.downcast_ref::<gtk::Popover>() {
        if popover.is_mapped() && popover.is_autohide() {
            return Some(popover.clone());
        }
    }
    let mut cur = root.first_child();
    while let Some(child) = cur {
        if let Some(found) = find_open_autohide_popover(&child) {
            return Some(found);
        }
        cur = child.next_sibling();
    }
    None
}
