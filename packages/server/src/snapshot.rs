//! Capture the active window as a PNG.
//!
//! Plan §Q1/§Q2: `gsk::CairoRenderer` + `gdk::Texture::save_to_png_bytes()`
//! to keep PNG encoding in-tree without pulling in `image`/`png` crates.
//! Plan §Q7: render + encode happen on the GLib main thread.

use crate::gtk;
use gtk::gdk;
use gtk::graphene;
use gtk::gsk;
use gtk::prelude::*;

/// Domain errors surfaced by the screenshot pipeline.
///
/// Mapped to HTTP status codes in `http.rs::screenshot_error_response`
/// (plan §Q4):
///
/// | error               | http |
/// |---------------------|------|
/// | `NoActiveWindow`    | 422  |
/// | `InvalidSelector`   | 422  |
/// | `SelectorNotFound`  | 404  |
/// | `WindowOutOfRange`  | 422  |
/// | `UnrealizedTarget`  | 422  |
/// | `EmptyNode`         | 422  |
/// | `ZeroSize`          | 422  |
/// | `RenderRealize`     | 500  |
#[derive(Debug, Clone, PartialEq)]
pub enum ScreenshotError {
    NoActiveWindow,
    /// `?selector=` failed to parse (issue #7).
    InvalidSelector {
        reason: String,
    },
    /// `?selector=` parsed but matched no widget across `app.windows()`
    /// (issue #7). Carries the original selector text for the 404 body.
    SelectorNotFound {
        selector: String,
    },
    /// `?window=<idx>` is out of range for `app.windows()` (issue #7).
    WindowOutOfRange {
        index: usize,
        count: usize,
    },
    /// The resolved target exists in the widget tree but is not visible /
    /// mapped, so it has nothing to paint — e.g. a child of a collapsed
    /// `GtkRevealer` (`reveal-child=false`). Distinct from `NoActiveWindow`
    /// so the caller reads it as "make the target visible, then capture"
    /// rather than "there is no window" (issue #7 follow-up).
    UnrealizedTarget,
    EmptyNode,
    ZeroSize,
    RenderRealize(String),
}

impl std::fmt::Display for ScreenshotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScreenshotError::NoActiveWindow => write!(f, "no_active_window"),
            ScreenshotError::InvalidSelector { reason } => write!(f, "invalid_selector: {reason}"),
            ScreenshotError::SelectorNotFound { selector } => {
                write!(f, "selector_not_found: {selector}")
            }
            ScreenshotError::WindowOutOfRange { index, count } => {
                write!(f, "window_out_of_range: {index} (count {count})")
            }
            ScreenshotError::UnrealizedTarget => write!(f, "unrealized_target"),
            ScreenshotError::EmptyNode => write!(f, "empty_node"),
            ScreenshotError::ZeroSize => write!(f, "zero_size"),
            ScreenshotError::RenderRealize(msg) => write!(f, "render_failed: {msg}"),
        }
    }
}

impl std::error::Error for ScreenshotError {}

/// Render a screenshot target as PNG bytes (issue #7).
///
/// Target precedence — the first non-`None` argument wins:
///   1. `selector`: resolve a widget across **all** `app.windows()` (active or
///      not, including open popovers, which are children in the widget tree)
///      via the shared `parse_selector` + `find_first` used by `/test/elements`
///      and `/test/type`, then offscreen-render that widget with
///      `WidgetPaintable` — so non-active windows and separate popover surfaces
///      are captured (the active-window-only path could not reach them).
///   2. `window`: index into `app.windows()` (creation order) to capture a
///      specific toplevel.
///   3. neither: the active window — the historical default.
///
/// `selector` parse failure → `InvalidSelector`; no match → `SelectorNotFound`;
/// out-of-range index → `WindowOutOfRange`.
pub fn render_target(
    app: &gtk::Application,
    selector: Option<&str>,
    window: Option<usize>,
) -> Result<Vec<u8>, ScreenshotError> {
    use crate::tree::{find_first, parse_selector, GtkTree};

    if let Some(sel_str) = selector {
        let sel = parse_selector(sel_str).map_err(|e| ScreenshotError::InvalidSelector {
            reason: e.reason.to_string(),
        })?;
        let widget =
            find_first(GtkTree { app }, &sel).ok_or_else(|| ScreenshotError::SelectorNotFound {
                selector: sel_str.to_string(),
            })?;
        return capture_widget_png(&widget);
    }

    if let Some(index) = window {
        let windows = app.windows();
        let win = windows
            .get(index)
            .ok_or(ScreenshotError::WindowOutOfRange {
                index,
                count: windows.len(),
            })?;
        return capture_widget_png(win.upcast_ref::<gtk::Widget>());
    }

    render_active_window(app)
}

/// Render the active window of `app` as a PNG and return its bytes.
///
/// `Application::active_window()` returning `None` maps to
/// `ScreenshotError::NoActiveWindow`. Plan §Q3 / §Q6.
pub fn render_active_window(app: &gtk::Application) -> Result<Vec<u8>, ScreenshotError> {
    let window = app.active_window().ok_or(ScreenshotError::NoActiveWindow)?;
    capture_widget_png(window.upcast_ref::<gtk::Widget>())
}

/// Snapshot a widget tree and encode the resulting GSK render node to PNG
/// bytes via `gsk::CairoRenderer` + `gdk::Texture::save_to_png_bytes`.
///
/// Plan §Q2: realize a fresh CairoRenderer on each call (CPU-only, xvfb-safe),
/// render at the widget's local logical size, then `unrealize()` explicitly.
fn capture_widget_png(widget: &gtk::Widget) -> Result<Vec<u8>, ScreenshotError> {
    // A target in the tree but not visible/mapped (e.g. a collapsed
    // `GtkRevealer` child) has nothing to paint. Report it as `UnrealizedTarget`
    // so the caller knows to reveal it first — distinct from `NoActiveWindow`,
    // which means there is no window at all (issue #7 follow-up).
    if !widget.is_visible() || !widget.is_mapped() {
        return Err(ScreenshotError::UnrealizedTarget);
    }

    let width = widget.width() as f32;
    let height = widget.height() as f32;
    if width <= 0.0 || height <= 0.0 {
        return Err(ScreenshotError::ZeroSize);
    }

    let snapshot = gtk::Snapshot::new();
    let paintable = gtk::WidgetPaintable::new(Some(widget));
    paintable.snapshot(
        snapshot.upcast_ref::<gdk::Snapshot>(),
        width as f64,
        height as f64,
    );
    let node = snapshot.to_node().ok_or(ScreenshotError::EmptyNode)?;

    let renderer = gsk::CairoRenderer::new();
    renderer
        .realize(None)
        .map_err(|e| ScreenshotError::RenderRealize(e.to_string()))?;

    let viewport = graphene::Rect::new(0.0, 0.0, width, height);
    let texture: gdk::Texture = renderer.render_texture(&node, Some(&viewport));

    renderer.unrealize();

    let bytes = texture.save_to_png_bytes();
    Ok(bytes.to_vec())
}

/// Alternate path: borrow the native surface so `realize()` gets a concrete
/// `&gdk::Surface`. Kept around as a pre-staged fallback for Risk-2 in case
/// `realize(None)` proves flaky on xvfb (plan rev2 / M1).
///
/// To swap in, replace the `renderer.realize(None)` call in
/// `capture_widget_png` with the body of this function. Behaviour and error
/// taxonomy are identical for the supported callers.
#[allow(dead_code)]
fn capture_widget_png_via_native_surface(widget: &gtk::Widget) -> Result<Vec<u8>, ScreenshotError> {
    let native = widget.native().ok_or(ScreenshotError::NoActiveWindow)?;
    let surface = native.surface().ok_or(ScreenshotError::NoActiveWindow)?;

    let width = widget.width() as f32;
    let height = widget.height() as f32;
    if width <= 0.0 || height <= 0.0 {
        return Err(ScreenshotError::ZeroSize);
    }

    let snapshot = gtk::Snapshot::new();
    let paintable = gtk::WidgetPaintable::new(Some(widget));
    paintable.snapshot(
        snapshot.upcast_ref::<gdk::Snapshot>(),
        width as f64,
        height as f64,
    );
    let node = snapshot.to_node().ok_or(ScreenshotError::EmptyNode)?;

    let renderer = gsk::CairoRenderer::new();
    renderer
        .realize(Some(&surface))
        .map_err(|e| ScreenshotError::RenderRealize(e.to_string()))?;

    let viewport = graphene::Rect::new(0.0, 0.0, width, height);
    let texture: gdk::Texture = renderer.render_texture(&node, Some(&viewport));

    renderer.unrealize();

    Ok(texture.save_to_png_bytes().to_vec())
}
