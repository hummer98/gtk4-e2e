//! Synthetic tap input.
//!
//! Plan §Q3 (Review M3): MVP supports `Button` only via `emit_clicked()`. All
//! other widget kinds return `TapError::UnsupportedWidget`. xy → widget
//! resolution lives here too, scoped to the active `ApplicationWindow`.

use crate::gtk;
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
