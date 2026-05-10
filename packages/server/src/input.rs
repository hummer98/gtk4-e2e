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
/// MVP supports `Button` only. All other widget kinds error with
/// `UnsupportedWidget`. Visibility / sensitivity gates run first so the
/// returned error matches the user's mental model.
pub fn tap_widget(widget: &gtk::Widget, selector: Option<&str>) -> Result<(), TapError> {
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
    if let Some(button) = widget.downcast_ref::<gtk::Button>() {
        button.emit_clicked();
        return Ok(());
    }
    Err(TapError::UnsupportedWidget {
        widget_type: widget.type_().name().to_string(),
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
        return Err(SwipeError::OutOfBounds { x: from.x, y: from.y });
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
