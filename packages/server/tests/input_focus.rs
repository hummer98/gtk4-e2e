//! Integration tests for `input::focus_widget` (issue #3).
//!
//! Like `input_type.rs` these tests need GTK initialised, so they auto-skip on
//! display-less hosts (CI without xvfb, headless macOS).

#![cfg(feature = "e2e")]

mod common;

use gtk4_e2e_server::gtk;
use gtk4_e2e_server::gtk::prelude::*;
use gtk4_e2e_server::input::{focus_widget, FocusError};

fn require_display() -> bool {
    if !common::ensure_gtk_init() {
        eprintln!("[skip] no GTK display available");
        return false;
    }
    true
}

#[test]
fn entry_grabs_focus() {
    if !require_display() {
        return;
    }
    let (window, entry, _tv, _label) = common::build_type_widgets().unwrap();
    window.present();
    common::pump_glib(64);

    let widget: gtk::Widget = entry.clone().upcast();
    focus_widget(&widget, Some("#input1")).expect("focus should succeed on visible entry");
    common::pump_glib(32);
    assert!(entry.has_focus(), "entry should hold keyboard focus");

    window.close();
    common::pump_glib(32);
}

#[test]
fn non_focusable_widget_is_rejected() {
    if !require_display() {
        return;
    }
    let (window, _entry, _tv, label) = common::build_type_widgets().unwrap();
    window.present();
    common::pump_glib(64);

    // A plain Label cannot take keyboard focus → grab_focus() returns false.
    let widget: gtk::Widget = label.clone().upcast();
    let err = focus_widget(&widget, Some("#label1")).unwrap_err();
    assert!(matches!(err, FocusError::FocusRejected { .. }));

    window.close();
    common::pump_glib(32);
}

#[test]
fn widget_not_visible_returns_error() {
    if !require_display() {
        return;
    }
    let (window, entry, _tv, _label) = common::build_type_widgets().unwrap();
    entry.set_visible(false);
    window.present();
    common::pump_glib(64);

    let widget: gtk::Widget = entry.clone().upcast();
    let err = focus_widget(&widget, Some("#input1")).unwrap_err();
    assert!(matches!(err, FocusError::WidgetNotVisible { .. }));

    window.close();
    common::pump_glib(32);
}

#[test]
fn widget_disabled_returns_error() {
    if !require_display() {
        return;
    }
    let (window, entry, _tv, _label) = common::build_type_widgets().unwrap();
    window.present();
    common::pump_glib(64);
    entry.set_sensitive(false);

    let widget: gtk::Widget = entry.clone().upcast();
    let err = focus_widget(&widget, Some("#input1")).unwrap_err();
    assert!(matches!(err, FocusError::WidgetDisabled { .. }));

    window.close();
    common::pump_glib(32);
}
