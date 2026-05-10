//! Integration tests for `snapshot::render_active_window`.
//!
//! Plan §Phase 2 / §Q2: prove the GSK CairoRenderer + PNG encoding pipeline
//! produces a valid PNG when an Application has a presented window. Skipped
//! on display-less hosts (CI runs these under xvfb, not in the rust job).

#![cfg(feature = "e2e")]

mod common;

use gtk4_e2e_server::gtk;
use gtk4_e2e_server::gtk::prelude::*;
use gtk4_e2e_server::snapshot::{render_active_window, ScreenshotError};

fn require_display() -> bool {
    if !common::ensure_gtk_init() {
        eprintln!("[skip] no GTK display available");
        return false;
    }
    true
}

#[test]
fn render_active_window_returns_png() {
    if !require_display() {
        return;
    }

    let app = gtk::Application::builder()
        .application_id("dev.gtk4-e2e.snaprender1")
        .build();
    let _ = app.register(None::<&gtk::gio::Cancellable>);

    let label = gtk::Label::new(Some("snap"));
    label.set_widget_name("label1");
    let window = gtk::ApplicationWindow::builder()
        .application(&app)
        .child(&label)
        .default_width(64)
        .default_height(48)
        .build();
    window.present();
    common::pump_glib(64);

    let bytes = render_active_window(&app).expect("render should succeed when window is mapped");

    assert!(
        bytes.len() > 100,
        "PNG body suspiciously small: {} bytes",
        bytes.len()
    );
    assert_eq!(
        &bytes[..8],
        b"\x89PNG\r\n\x1a\n",
        "missing PNG signature in first 8 bytes"
    );
    // IHDR chunk: bytes 16..20 = width BE, 20..24 = height BE.
    let width = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
    let height = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
    assert!(width > 0, "PNG IHDR width should be > 0");
    assert!(height > 0, "PNG IHDR height should be > 0");

    window.close();
    common::pump_glib(32);
}

#[test]
fn render_no_active_window_errors() {
    if !require_display() {
        return;
    }

    // Build a fresh Application but never present a window. `active_window()`
    // should be `None`, so the render path returns NoActiveWindow.
    let app = gtk::Application::builder()
        .application_id("dev.gtk4-e2e.snaprender2")
        .build();
    let _ = app.register(None::<&gtk::gio::Cancellable>);
    common::pump_glib(8);

    let outcome = render_active_window(&app);
    assert_eq!(outcome, Err(ScreenshotError::NoActiveWindow));
}
