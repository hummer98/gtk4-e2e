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
/// | error             | http |
/// |-------------------|------|
/// | `NoActiveWindow`  | 422  |
/// | `EmptyNode`       | 422  |
/// | `ZeroSize`        | 422  |
/// | `RenderRealize`   | 500  |
#[derive(Debug, Clone, PartialEq)]
pub enum ScreenshotError {
    NoActiveWindow,
    EmptyNode,
    ZeroSize,
    RenderRealize(String),
}

impl std::fmt::Display for ScreenshotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScreenshotError::NoActiveWindow => write!(f, "no_active_window"),
            ScreenshotError::EmptyNode => write!(f, "empty_node"),
            ScreenshotError::ZeroSize => write!(f, "zero_size"),
            ScreenshotError::RenderRealize(msg) => write!(f, "render_failed: {msg}"),
        }
    }
}

impl std::error::Error for ScreenshotError {}

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
    if !widget.is_visible() || !widget.is_mapped() {
        return Err(ScreenshotError::NoActiveWindow);
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
