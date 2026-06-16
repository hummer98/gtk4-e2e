//! Integration tests for `input::press` / `input::validate_press` /
//! `LongPressAnimation::run`.
//!
//! Auto-skips on display-less hosts (no `gtk::init()`), like `input_pinch.rs`.
//! Task 029 (T029).

#![cfg(feature = "e2e")]

mod common;

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use gtk4_e2e_server::gtk;
use gtk4_e2e_server::gtk::prelude::*;
use gtk4_e2e_server::input::{press, PressError};
use gtk4_e2e_server::proto::XY;

fn require_display() -> bool {
    if !common::ensure_gtk_init() {
        eprintln!("[skip] no GTK display available");
        return false;
    }
    true
}

/// Build a window containing a `DrawingArea` with an attached
/// `GestureLongPress`. Returns the window plus a shared counter incremented by
/// `connect_pressed`. Tests read this back to confirm the animation's
/// `emit_by_name("pressed", ...)` reaches the user-side handler.
fn build_long_press_window() -> (gtk::ApplicationWindow, Rc<Cell<u32>>) {
    let app = gtk::Application::builder()
        .application_id("dev.gtk4-e2e.press-test")
        .build();
    let _ = app.register(None::<&gtk::gio::Cancellable>);

    let drawing_area = gtk::DrawingArea::builder()
        .content_width(160)
        .content_height(120)
        .build();
    drawing_area.set_widget_name("longpress1");

    let count: Rc<Cell<u32>> = Rc::new(Cell::new(0));
    let gesture = gtk::GestureLongPress::new();
    {
        let count = count.clone();
        gesture.connect_pressed(move |_g, _x, _y| {
            count.set(count.get() + 1);
        });
    }
    drawing_area.add_controller(gesture);

    let window = gtk::ApplicationWindow::builder()
        .application(&app)
        .title("press-test")
        .child(&drawing_area)
        .default_width(360)
        .default_height(480)
        .build();
    (window, count)
}

/// Build a plain (no `GestureLongPress`) window for `no_long_pressable` cases.
fn build_plain_window() -> gtk::ApplicationWindow {
    let app = gtk::Application::builder()
        .application_id("dev.gtk4-e2e.press-plain")
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
fn press_zero_hold_returns_error() {
    if !require_display() {
        return;
    }
    let (window, _count) = build_long_press_window();
    window.present();
    common::pump_glib(64);

    let err = press(&window, XY { x: 80, y: 60 }, 0).unwrap_err();
    assert!(matches!(err, PressError::ZeroHold), "got {err:?}");

    window.close();
    common::pump_glib(32);
}

#[test]
fn press_hold_too_long_returns_error() {
    if !require_display() {
        return;
    }
    let (window, _count) = build_long_press_window();
    window.present();
    common::pump_glib(64);

    let err = press(&window, XY { x: 80, y: 60 }, 10_001).unwrap_err();
    assert!(
        matches!(err, PressError::HoldTooLong { hold_ms: 10_001 }),
        "got {err:?}"
    );

    window.close();
    common::pump_glib(32);
}

#[test]
fn press_out_of_bounds_returns_error() {
    if !require_display() {
        return;
    }
    let (window, _count) = build_long_press_window();
    window.present();
    common::pump_glib(64);

    let err = press(&window, XY { x: -1, y: -1 }, 100).unwrap_err();
    assert!(matches!(err, PressError::OutOfBounds { .. }), "got {err:?}");

    let err = press(
        &window,
        XY {
            x: 10_000,
            y: 10_000,
        },
        100,
    )
    .unwrap_err();
    assert!(matches!(err, PressError::OutOfBounds { .. }), "got {err:?}");

    window.close();
    common::pump_glib(32);
}

#[test]
fn press_no_long_pressable_returns_error() {
    if !require_display() {
        return;
    }
    let window = build_plain_window();
    window.present();
    common::pump_glib(64);

    let err = press(&window, XY { x: 50, y: 50 }, 100).unwrap_err();
    assert!(
        matches!(err, PressError::NoLongPressableAtPoint { .. }),
        "got {err:?}"
    );

    window.close();
    common::pump_glib(32);
}

#[test]
fn press_emits_pressed_to_target() {
    if !require_display() {
        return;
    }
    let (window, count) = build_long_press_window();
    window.present();
    common::pump_glib(64);

    press(&window, XY { x: 80, y: 60 }, 100)
        .expect("press should succeed for a window with GestureLongPress");

    common::pump_glib_for(Duration::from_millis(300));

    assert_eq!(
        count.get(),
        1,
        "expected `pressed` to fire exactly once, got {}",
        count.get()
    );

    window.close();
    common::pump_glib(32);
}

/// Completion condition 3 (traceability, N3): a press is a single-point hold —
/// it carries no `from`/`to` delta, so it structurally never reaches the
/// zero-distance error path that `swipe` guards against. This test documents
/// that by asserting a zero-movement press at a single coordinate still fires
/// `pressed` exactly once.
#[test]
fn press_zero_distance_hold_fires() {
    if !require_display() {
        return;
    }
    let (window, count) = build_long_press_window();
    window.present();
    common::pump_glib(64);

    press(&window, XY { x: 80, y: 60 }, 150)
        .expect("zero-distance press should succeed for a window with GestureLongPress");

    common::pump_glib_for(Duration::from_millis(350));

    assert_eq!(
        count.get(),
        1,
        "expected single-point hold to fire `pressed` once, got {}",
        count.get()
    );

    window.close();
    common::pump_glib(32);
}
