//! Integration tests for `input::set_value_widget` / `input::set_value_at` /
//! `input::value_from_coord` / `input::find_range_ancestor`.
//!
//! Auto-skips on display-less hosts (no `gtk::init()`), like `input_press.rs`.
//! Drives a `GtkScale` (a `GtkRange`) through the set-value pipeline.

#![cfg(feature = "e2e")]

mod common;

use gtk4_e2e_server::gtk;
use gtk4_e2e_server::gtk::prelude::*;
use gtk4_e2e_server::input::{
    find_range_ancestor, set_value_at, set_value_widget, value_from_coord, SetValueError,
};
use gtk4_e2e_server::proto::XY;

fn require_display() -> bool {
    if !common::ensure_gtk_init() {
        eprintln!("[skip] no GTK display available");
        return false;
    }
    true
}

/// Build a window whose sole child is a horizontal `GtkScale` (`#scale1`,
/// range 0..=100). Returns the window and the scale.
fn build_scale_window() -> (gtk::ApplicationWindow, gtk::Scale) {
    let app = gtk::Application::builder()
        .application_id("dev.gtk4-e2e.set-value-test")
        .build();
    let _ = app.register(None::<&gtk::gio::Cancellable>);

    let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 100.0, 1.0);
    scale.set_widget_name("scale1");
    scale.set_hexpand(true);
    scale.set_vexpand(true);
    scale.set_draw_value(false);

    let window = gtk::ApplicationWindow::builder()
        .application(&app)
        .title("set-value-test")
        .child(&scale)
        .default_width(400)
        .default_height(120)
        .build();
    (window, scale)
}

#[test]
fn set_value_widget_applies_value() {
    if !require_display() {
        return;
    }
    let (window, scale) = build_scale_window();
    window.present();
    common::pump_glib(64);

    let widget: gtk::Widget = scale.clone().upcast();
    let set = set_value_widget(&widget, "#scale1", 42.0).expect("set should succeed");
    assert_eq!(set, 42.0);
    assert_eq!(scale.value(), 42.0);

    window.close();
    common::pump_glib(32);
}

#[test]
fn set_value_widget_clamps_to_range() {
    if !require_display() {
        return;
    }
    let (window, scale) = build_scale_window();
    window.present();
    common::pump_glib(64);

    let widget: gtk::Widget = scale.clone().upcast();
    // Above upper bound clamps to 100; below lower clamps to 0.
    let high = set_value_widget(&widget, "#scale1", 999.0).expect("set should succeed");
    assert_eq!(high, 100.0);
    let low = set_value_widget(&widget, "#scale1", -50.0).expect("set should succeed");
    assert_eq!(low, 0.0);

    window.close();
    common::pump_glib(32);
}

#[test]
fn set_value_widget_rejects_non_finite() {
    if !require_display() {
        return;
    }
    let (window, scale) = build_scale_window();
    window.present();
    common::pump_glib(64);

    let widget: gtk::Widget = scale.clone().upcast();
    let err = set_value_widget(&widget, "#scale1", f64::NAN).unwrap_err();
    assert!(
        matches!(err, SetValueError::InvalidValue { .. }),
        "got {err:?}"
    );
    let err = set_value_widget(&widget, "#scale1", f64::INFINITY).unwrap_err();
    assert!(
        matches!(err, SetValueError::InvalidValue { .. }),
        "got {err:?}"
    );

    window.close();
    common::pump_glib(32);
}

#[test]
fn set_value_widget_rejects_non_range() {
    if !require_display() {
        return;
    }
    if !common::ensure_gtk_init() {
        return;
    }
    let label = gtk::Label::new(Some("not a range"));
    label.set_widget_name("label1");
    let widget: gtk::Widget = label.upcast();
    let err = set_value_widget(&widget, "#label1", 10.0).unwrap_err();
    assert!(
        matches!(err, SetValueError::NoRangeForSelector { .. }),
        "got {err:?}"
    );
}

#[test]
fn set_value_widget_rejects_disabled() {
    if !require_display() {
        return;
    }
    let (window, scale) = build_scale_window();
    scale.set_sensitive(false);
    window.present();
    common::pump_glib(64);

    let widget: gtk::Widget = scale.clone().upcast();
    let err = set_value_widget(&widget, "#scale1", 10.0).unwrap_err();
    assert!(
        matches!(err, SetValueError::WidgetDisabled { .. }),
        "got {err:?}"
    );

    window.close();
    common::pump_glib(32);
}

#[test]
fn set_value_at_explicit_value() {
    if !require_display() {
        return;
    }
    let (window, scale) = build_scale_window();
    window.present();
    common::pump_glib(64);

    let root: gtk::Widget = window.clone().upcast();
    let rect = scale
        .compute_bounds(&root)
        .expect("scale should have bounds after present");
    let cx = (rect.x() + rect.width() / 2.0) as i32;
    let cy = (rect.y() + rect.height() / 2.0) as i32;

    let set = set_value_at(&window, XY { x: cx, y: cy }, Some(50.0)).expect("set should succeed");
    assert_eq!(set, 50.0);
    assert_eq!(scale.value(), 50.0);

    window.close();
    common::pump_glib(32);
}

#[test]
fn set_value_at_derives_from_coordinate() {
    if !require_display() {
        return;
    }
    let (window, scale) = build_scale_window();
    window.present();
    common::pump_glib(64);

    let root: gtk::Widget = window.clone().upcast();
    let rect = scale
        .compute_bounds(&root)
        .expect("scale should have bounds after present");
    let cy = (rect.y() + rect.height() / 2.0) as i32;
    let left_x = (rect.x() + 2.0) as i32;
    let right_x = (rect.x() + rect.width() - 3.0) as i32;

    // No explicit value → derived from x position along the trough.
    let left = set_value_at(&window, XY { x: left_x, y: cy }, None).expect("left set");
    assert!(left < 25.0, "expected near-lower value, got {left}");

    let right = set_value_at(&window, XY { x: right_x, y: cy }, None).expect("right set");
    assert!(right > 75.0, "expected near-upper value, got {right}");

    window.close();
    common::pump_glib(32);
}

#[test]
fn value_from_coord_maps_midpoint() {
    if !require_display() {
        return;
    }
    let (window, scale) = build_scale_window();
    window.present();
    common::pump_glib(64);

    let root: gtk::Widget = window.clone().upcast();
    let rect = scale.compute_bounds(&root).expect("bounds");
    let mid_x = (rect.x() + rect.width() / 2.0) as i32;
    let cy = (rect.y() + rect.height() / 2.0) as i32;

    let range = find_range_ancestor(&scale.clone().upcast()).expect("scale is a range");
    let v = value_from_coord(&range, &root, XY { x: mid_x, y: cy });
    assert!(
        (40.0..=60.0).contains(&v),
        "midpoint should be ~50, got {v}"
    );

    window.close();
    common::pump_glib(32);
}

#[test]
fn set_value_at_out_of_bounds() {
    if !require_display() {
        return;
    }
    let (window, _scale) = build_scale_window();
    window.present();
    common::pump_glib(64);

    let err = set_value_at(&window, XY { x: -5, y: -5 }, Some(10.0)).unwrap_err();
    assert!(
        matches!(err, SetValueError::OutOfBounds { .. }),
        "got {err:?}"
    );

    window.close();
    common::pump_glib(32);
}

#[test]
fn find_range_ancestor_walks_up() {
    if !require_display() {
        return;
    }
    let (window, scale) = build_scale_window();
    window.present();
    common::pump_glib(64);

    // The scale itself resolves to a range.
    assert!(find_range_ancestor(&scale.clone().upcast()).is_some());
    // A non-range widget with no range ancestor resolves to None.
    let label = gtk::Label::new(Some("x"));
    assert!(find_range_ancestor(&label.upcast()).is_none());

    window.close();
    common::pump_glib(32);
}
