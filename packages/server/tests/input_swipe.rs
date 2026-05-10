//! Integration tests for `input::swipe` / `input::validate` /
//! `SwipeAnimation::run`.
//!
//! Auto-skips on display-less hosts (no `gtk::init()`), like `input_tap.rs`.
//! Plan T014 §8.1.2.

#![cfg(feature = "e2e")]

mod common;

use std::time::Duration;

use gtk4_e2e_server::gtk;
use gtk4_e2e_server::gtk::prelude::*;
use gtk4_e2e_server::input::{swipe, SwipeError};
use gtk4_e2e_server::proto::XY;

fn require_display() -> bool {
    if !common::ensure_gtk_init() {
        eprintln!("[skip] no GTK display available");
        return false;
    }
    true
}

/// Build a window containing a vertically scrollable `ScrolledWindow` plus a
/// vbox of 30 rows so the viewport is comfortably smaller than the content.
fn build_scroll_window() -> (gtk::ApplicationWindow, gtk::ScrolledWindow) {
    let app = gtk::Application::builder()
        .application_id("dev.gtk4-e2e.swipe-test")
        .build();
    let _ = app.register(None::<&gtk::gio::Cancellable>);

    let listbox = gtk::ListBox::new();
    listbox.set_widget_name("list1");
    for i in 0..30 {
        let row = gtk::Label::new(Some(&format!("Row {i}")));
        row.set_widget_name(&format!("row-{i}"));
        listbox.append(&row);
    }

    let scrolled = gtk::ScrolledWindow::builder()
        .height_request(200)
        .min_content_height(200)
        .child(&listbox)
        .build();
    scrolled.set_widget_name("scroll1");
    scrolled.set_kinetic_scrolling(false);

    let window = gtk::ApplicationWindow::builder()
        .application(&app)
        .title("swipe-test")
        .child(&scrolled)
        .default_width(360)
        .default_height(480)
        .build();
    (window, scrolled)
}

/// Build a window with no `ScrolledWindow` for `no_scrollable` test cases.
fn build_plain_window() -> gtk::ApplicationWindow {
    let app = gtk::Application::builder()
        .application_id("dev.gtk4-e2e.swipe-plain")
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
fn swipe_zero_duration_returns_error() {
    if !require_display() {
        return;
    }
    let (window, _scrolled) = build_scroll_window();
    window.present();
    common::pump_glib(64);

    let err = swipe(&window, XY { x: 100, y: 200 }, XY { x: 100, y: 50 }, 0).unwrap_err();
    assert!(matches!(err, SwipeError::ZeroDuration), "got {err:?}");

    window.close();
    common::pump_glib(32);
}

#[test]
fn swipe_duration_too_long_returns_error() {
    if !require_display() {
        return;
    }
    let (window, _scrolled) = build_scroll_window();
    window.present();
    common::pump_glib(64);

    let err = swipe(
        &window,
        XY { x: 100, y: 200 },
        XY { x: 100, y: 50 },
        10_001,
    )
    .unwrap_err();
    assert!(
        matches!(err, SwipeError::DurationTooLong { duration_ms: 10_001 }),
        "got {err:?}"
    );

    window.close();
    common::pump_glib(32);
}

#[test]
fn swipe_out_of_bounds_returns_error() {
    if !require_display() {
        return;
    }
    let (window, _scrolled) = build_scroll_window();
    window.present();
    common::pump_glib(64);

    let err = swipe(&window, XY { x: -1, y: -1 }, XY { x: 100, y: 50 }, 100).unwrap_err();
    assert!(matches!(err, SwipeError::OutOfBounds { .. }), "got {err:?}");

    let err = swipe(
        &window,
        XY { x: 10_000, y: 10_000 },
        XY { x: 100, y: 50 },
        100,
    )
    .unwrap_err();
    assert!(matches!(err, SwipeError::OutOfBounds { .. }), "got {err:?}");

    window.close();
    common::pump_glib(32);
}

#[test]
fn swipe_no_scrollable_returns_error() {
    if !require_display() {
        return;
    }
    let window = build_plain_window();
    window.present();
    common::pump_glib(64);

    // Aim near the centre of the window where the label child resolves; no
    // ScrolledWindow ancestor exists so this should return NoScrollableAtPoint.
    let err = swipe(&window, XY { x: 50, y: 50 }, XY { x: 50, y: 20 }, 100).unwrap_err();
    assert!(
        matches!(err, SwipeError::NoScrollableAtPoint { .. }),
        "got {err:?}"
    );

    window.close();
    common::pump_glib(32);
}

#[test]
fn swipe_modifies_vadjustment_value() {
    if !require_display() {
        return;
    }
    let (window, scrolled) = build_scroll_window();
    window.present();
    common::pump_glib(64);

    let vadj = scrolled.vadjustment();
    let v_before = vadj.value();

    // Upward swipe: from (100, 400) to (100, 100) over 200 ms.
    // dy = 400 - 100 = 300 → vadjustment.value should increase by 300 (clamped
    // to upper - page_size).
    swipe(
        &window,
        XY { x: 100, y: 400 },
        XY { x: 100, y: 100 },
        200,
    )
    .expect("swipe should succeed for in-bounds ScrolledWindow target");

    common::pump_glib_for(Duration::from_millis(400));

    let v_after = vadj.value();
    let upper_minus_page = (vadj.upper() - vadj.page_size()).max(0.0);
    let expected = (v_before + 300.0).min(upper_minus_page);
    assert!(
        (v_after - expected).abs() < 1.5,
        "vadjustment.value = {v_after}, expected ~{expected} (upper={}, page={})",
        vadj.upper(),
        vadj.page_size()
    );

    window.close();
    common::pump_glib(32);
}
