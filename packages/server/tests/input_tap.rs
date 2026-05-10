//! Integration tests for `input::tap_widget` / `input::resolve_xy`.
//!
//! These need GTK initialised, so they auto-skip on display-less hosts (CI
//! without xvfb, headless macOS). The tests live in their own integration
//! binary so they're trivially excludable.

#![cfg(feature = "e2e")]

mod common;

use std::cell::Cell;
use std::rc::Rc;

use gtk4_e2e_server::gtk;
use gtk4_e2e_server::gtk::prelude::*;
use gtk4_e2e_server::input::{resolve_xy, tap_widget, TapError};

fn require_display() -> bool {
    if !common::ensure_gtk_init() {
        eprintln!("[skip] no GTK display available");
        return false;
    }
    true
}

#[test]
fn button_emit_clicked() {
    if !require_display() {
        return;
    }
    let (window, _entry, button, _label) = common::build_demo_widgets().unwrap();
    window.present();
    common::pump_glib(64);

    let fired = Rc::new(Cell::new(0u32));
    let fired_c = fired.clone();
    button.connect_clicked(move |_| fired_c.set(fired_c.get() + 1));

    let widget: gtk::Widget = button.clone().upcast();
    tap_widget(&widget, Some("#btn1")).expect("tap should succeed on visible button");
    assert_eq!(
        fired.get(),
        1,
        "clicked handler should have fired exactly once"
    );

    window.close();
    common::pump_glib(32);
}

#[test]
fn unsupported_widget_returns_422_code() {
    if !require_display() {
        return;
    }
    let (window, _entry, _button, label) = common::build_demo_widgets().unwrap();
    window.present();
    common::pump_glib(64);

    let widget: gtk::Widget = label.clone().upcast();
    let err = tap_widget(&widget, Some("#label1")).unwrap_err();
    assert!(matches!(err, TapError::UnsupportedWidget { .. }));

    window.close();
    common::pump_glib(32);
}

#[test]
fn xy_resolves_to_widget() {
    if !require_display() {
        return;
    }
    let (window, _entry, button, _label) = common::build_demo_widgets().unwrap();
    window.set_default_size(360, 200);
    window.present();
    common::pump_glib(64);

    let bounds = button
        .compute_bounds(&window)
        .expect("button should have bounds after present");
    let cx = (bounds.x() + bounds.width() / 2.0) as i32;
    let cy = (bounds.y() + bounds.height() / 2.0) as i32;

    // The deepest widget at the button center may be a private label child;
    // that's acceptable as long as resolve_xy returned something.
    let _hit = resolve_xy(&window, cx, cy).expect("xy at button center should resolve");

    window.close();
    common::pump_glib(32);
}

#[test]
fn xy_out_of_bounds_returns_error() {
    if !require_display() {
        return;
    }
    let (window, _entry, _button, _label) = common::build_demo_widgets().unwrap();
    window.set_default_size(360, 200);
    window.present();
    common::pump_glib(64);

    let err = resolve_xy(&window, -1, -1).unwrap_err();
    assert!(matches!(err, TapError::OutOfBounds { .. }));

    let err = resolve_xy(&window, 10_000, 10_000).unwrap_err();
    assert!(matches!(err, TapError::OutOfBounds { .. }));

    window.close();
    common::pump_glib(32);
}

#[test]
fn widget_not_visible_returns_error() {
    if !require_display() {
        return;
    }
    let (window, _entry, button, _label) = common::build_demo_widgets().unwrap();
    button.set_visible(false);
    window.present();
    common::pump_glib(64);

    let widget: gtk::Widget = button.clone().upcast();
    let err = tap_widget(&widget, Some("#btn1")).unwrap_err();
    assert!(matches!(err, TapError::WidgetNotVisible { .. }));

    window.close();
    common::pump_glib(32);
}

#[test]
fn widget_disabled_returns_error() {
    if !require_display() {
        return;
    }
    let (window, _entry, button, _label) = common::build_demo_widgets().unwrap();
    window.present();
    common::pump_glib(64);
    button.set_sensitive(false);

    let widget: gtk::Widget = button.clone().upcast();
    let err = tap_widget(&widget, Some("#btn1")).unwrap_err();
    assert!(matches!(err, TapError::WidgetDisabled { .. }));

    window.close();
    common::pump_glib(32);
}

// ----- T019: tap dispatch for Switch / CheckButton / ToggleButton -----

fn build_window_with<F>(widget_id: &str, build: F) -> (gtk::ApplicationWindow, gtk::Widget)
where
    F: FnOnce(&str) -> gtk::Widget,
{
    let app = gtk::Application::builder()
        .application_id("dev.gtk4-e2e.input-tap-toggle")
        .build();
    let _ = app.register(None::<&gtk::gio::Cancellable>);
    let widget = build(widget_id);
    let window = gtk::ApplicationWindow::builder()
        .application(&app)
        .child(&widget)
        .build();
    (window, widget)
}

#[test]
fn switch_tap_toggles_active() {
    if !require_display() {
        return;
    }
    let (window, widget) = build_window_with("switch1", |id| {
        let s = gtk::Switch::builder().active(false).build();
        s.set_widget_name(id);
        s.upcast()
    });
    window.present();
    common::pump_glib(64);

    let switch = widget.downcast_ref::<gtk::Switch>().unwrap();
    assert!(!switch.is_active(), "precondition: starts inactive");
    tap_widget(&widget, Some("#switch1")).expect("switch tap should succeed");
    common::pump_glib(32);
    assert!(switch.is_active(), "switch should toggle to active");

    tap_widget(&widget, Some("#switch1")).expect("switch tap should toggle back");
    common::pump_glib(32);
    assert!(!switch.is_active(), "second tap should toggle off");

    window.close();
    common::pump_glib(32);
}

#[test]
fn check_button_tap_toggles_active() {
    if !require_display() {
        return;
    }
    let (window, widget) = build_window_with("check1", |id| {
        let c = gtk::CheckButton::with_label("Subscribe");
        c.set_widget_name(id);
        c.upcast()
    });
    window.present();
    common::pump_glib(64);

    let check = widget.downcast_ref::<gtk::CheckButton>().unwrap();
    assert!(!check.is_active(), "precondition: starts inactive");
    tap_widget(&widget, Some("#check1")).expect("check tap should succeed");
    common::pump_glib(32);
    assert!(check.is_active(), "check button should toggle to active");

    window.close();
    common::pump_glib(32);
}

#[test]
fn toggle_button_tap_toggles_active() {
    if !require_display() {
        return;
    }
    let (window, widget) = build_window_with("toggle1", |id| {
        let t = gtk::ToggleButton::with_label("Toggle");
        t.set_widget_name(id);
        t.upcast()
    });
    window.present();
    common::pump_glib(64);

    let toggle = widget.downcast_ref::<gtk::ToggleButton>().unwrap();
    assert!(!toggle.is_active(), "precondition: starts inactive");
    tap_widget(&widget, Some("#toggle1")).expect("toggle tap should succeed");
    common::pump_glib(32);
    assert!(toggle.is_active(), "toggle button should toggle to active");

    window.close();
    common::pump_glib(32);
}
