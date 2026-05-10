//! Integration tests for `input::pinch` / `input::validate_pinch` /
//! `PinchAnimation::run`.
//!
//! Auto-skips on display-less hosts (no `gtk::init()`), like `input_swipe.rs`.
//! Plan T015 §11.1.

#![cfg(feature = "e2e")]

mod common;

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use gtk4_e2e_server::gtk;
use gtk4_e2e_server::gtk::prelude::*;
use gtk4_e2e_server::input::{pinch, PinchError};
use gtk4_e2e_server::proto::XY;

fn require_display() -> bool {
    if !common::ensure_gtk_init() {
        eprintln!("[skip] no GTK display available");
        return false;
    }
    true
}

/// Build a window containing a `DrawingArea` with an attached `GestureZoom`.
/// Returns the window plus a shared `f64` slot updated by
/// `connect_scale_changed`. Tests can read this back to confirm that the
/// animation's `emit_by_name` reaches the user-side handler.
fn build_zoom_window() -> (gtk::ApplicationWindow, Rc<RefCell<f64>>) {
    let app = gtk::Application::builder()
        .application_id("dev.gtk4-e2e.pinch-test")
        .build();
    let _ = app.register(None::<&gtk::gio::Cancellable>);

    let drawing_area = gtk::DrawingArea::builder()
        .content_width(160)
        .content_height(120)
        .build();
    drawing_area.set_widget_name("zoom1");

    let received: Rc<RefCell<f64>> = Rc::new(RefCell::new(1.0));
    let gesture = gtk::GestureZoom::new();
    {
        let received = received.clone();
        gesture.connect_scale_changed(move |_g, scale| {
            *received.borrow_mut() = scale;
        });
    }
    drawing_area.add_controller(gesture);

    let window = gtk::ApplicationWindow::builder()
        .application(&app)
        .title("pinch-test")
        .child(&drawing_area)
        .default_width(360)
        .default_height(480)
        .build();
    (window, received)
}

/// Build a plain (no `GestureZoom`) window for `no_pinchable` test cases.
fn build_plain_window() -> gtk::ApplicationWindow {
    let app = gtk::Application::builder()
        .application_id("dev.gtk4-e2e.pinch-plain")
        .build();
    let _ = app.register(None::<&gtk::gio::Cancellable>);

    let label = gtk::Label::new(Some("plain"));
    label.set_widget_name("label1");
    gtk::ApplicationWindow::builder()
        .application(&app)
        .title("plain")
        .child(&label)
        .default_width(360)
        .default_height(200)
        .build()
}

#[test]
fn pinch_zero_duration_returns_error() {
    if !require_display() {
        return;
    }
    let (window, _received) = build_zoom_window();
    window.present();
    common::pump_glib(64);

    let err = pinch(&window, XY { x: 100, y: 100 }, 1.5, 0).unwrap_err();
    assert!(matches!(err, PinchError::ZeroDuration), "got {err:?}");

    window.close();
    common::pump_glib(32);
}

#[test]
fn pinch_duration_too_long_returns_error() {
    if !require_display() {
        return;
    }
    let (window, _received) = build_zoom_window();
    window.present();
    common::pump_glib(64);

    let err = pinch(&window, XY { x: 100, y: 100 }, 1.5, 10_001).unwrap_err();
    assert!(
        matches!(err, PinchError::DurationTooLong { duration_ms: 10_001 }),
        "got {err:?}"
    );

    window.close();
    common::pump_glib(32);
}

#[test]
fn pinch_invalid_scale_nan() {
    if !require_display() {
        return;
    }
    let (window, _received) = build_zoom_window();
    window.present();
    common::pump_glib(64);

    let err = pinch(&window, XY { x: 100, y: 100 }, f32::NAN, 100).unwrap_err();
    assert!(
        matches!(err, PinchError::InvalidScale { reason: "nan" }),
        "got {err:?}"
    );

    window.close();
    common::pump_glib(32);
}

#[test]
fn pinch_invalid_scale_zero() {
    if !require_display() {
        return;
    }
    let (window, _received) = build_zoom_window();
    window.present();
    common::pump_glib(64);

    let err = pinch(&window, XY { x: 100, y: 100 }, 0.0, 100).unwrap_err();
    assert!(
        matches!(err, PinchError::InvalidScale { reason: "non_positive" }),
        "got {err:?}"
    );

    window.close();
    common::pump_glib(32);
}

#[test]
fn pinch_invalid_scale_negative() {
    if !require_display() {
        return;
    }
    let (window, _received) = build_zoom_window();
    window.present();
    common::pump_glib(64);

    let err = pinch(&window, XY { x: 100, y: 100 }, -1.0, 100).unwrap_err();
    assert!(
        matches!(err, PinchError::InvalidScale { reason: "non_positive" }),
        "got {err:?}"
    );

    window.close();
    common::pump_glib(32);
}

#[test]
fn pinch_invalid_scale_too_large() {
    if !require_display() {
        return;
    }
    let (window, _received) = build_zoom_window();
    window.present();
    common::pump_glib(64);

    let err = pinch(&window, XY { x: 100, y: 100 }, 100.0, 100).unwrap_err();
    assert!(
        matches!(err, PinchError::InvalidScale { reason: "too_large" }),
        "got {err:?}"
    );

    window.close();
    common::pump_glib(32);
}

#[test]
fn pinch_invalid_scale_too_small() {
    if !require_display() {
        return;
    }
    let (window, _received) = build_zoom_window();
    window.present();
    common::pump_glib(64);

    // 0.01 < 1 / MAX_PINCH_SCALE (= 1/50 = 0.02)
    let err = pinch(&window, XY { x: 100, y: 100 }, 0.01, 100).unwrap_err();
    assert!(
        matches!(err, PinchError::InvalidScale { reason: "too_small" }),
        "got {err:?}"
    );

    window.close();
    common::pump_glib(32);
}

#[test]
fn pinch_out_of_bounds_returns_error() {
    if !require_display() {
        return;
    }
    let (window, _received) = build_zoom_window();
    window.present();
    common::pump_glib(64);

    let err = pinch(&window, XY { x: -1, y: -1 }, 1.5, 100).unwrap_err();
    assert!(matches!(err, PinchError::OutOfBounds { .. }), "got {err:?}");

    let err = pinch(&window, XY { x: 10_000, y: 10_000 }, 1.5, 100).unwrap_err();
    assert!(matches!(err, PinchError::OutOfBounds { .. }), "got {err:?}");

    window.close();
    common::pump_glib(32);
}

#[test]
fn pinch_no_pinchable_returns_error() {
    if !require_display() {
        return;
    }
    let window = build_plain_window();
    window.present();
    common::pump_glib(64);

    let err = pinch(&window, XY { x: 50, y: 50 }, 1.5, 100).unwrap_err();
    assert!(
        matches!(err, PinchError::NoPinchableAtPoint { .. }),
        "got {err:?}"
    );

    window.close();
    common::pump_glib(32);
}

#[test]
fn pinch_emits_scale_changed_to_target() {
    if !require_display() {
        return;
    }
    let (window, received) = build_zoom_window();
    window.present();
    common::pump_glib(64);

    pinch(&window, XY { x: 80, y: 60 }, 1.5, 200)
        .expect("pinch should succeed for a window with GestureZoom");

    common::pump_glib_for(Duration::from_millis(400));

    let last = *received.borrow();
    assert!(
        (last - 1.5).abs() < 0.05,
        "expected last scale-changed value ~1.5, got {last}"
    );

    window.close();
    common::pump_glib(32);
}
