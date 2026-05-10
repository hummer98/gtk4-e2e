//! Integration tests for `input::type_text` (Step 9).
//!
//! Like `input_tap.rs` these tests need GTK initialised, so they auto-skip on
//! display-less hosts (CI without xvfb, headless macOS).

#![cfg(feature = "e2e")]

mod common;

use gtk4_e2e_server::gtk;
use gtk4_e2e_server::gtk::prelude::*;
use gtk4_e2e_server::input::{type_text, TypeError};

fn require_display() -> bool {
    if !common::ensure_gtk_init() {
        eprintln!("[skip] no GTK display available");
        return false;
    }
    true
}

#[test]
fn entry_set_text_replaces() {
    if !require_display() {
        return;
    }
    let (window, entry, _tv, _label) = common::build_type_widgets().unwrap();
    window.present();
    common::pump_glib(64);

    let widget: gtk::Widget = entry.clone().upcast();
    type_text(&widget, "world", Some("#input1")).expect("type should succeed on visible entry");
    assert_eq!(entry.text().as_str(), "world");

    window.close();
    common::pump_glib(32);
}

#[test]
fn text_view_set_text_replaces() {
    if !require_display() {
        return;
    }
    let (window, _entry, tv, _label) = common::build_type_widgets().unwrap();
    window.present();
    common::pump_glib(64);

    let widget: gtk::Widget = tv.clone().upcast();
    type_text(&widget, "abc", Some("#tv1")).expect("type should succeed on TextView");
    let buf = tv.buffer();
    let (start, end) = buf.bounds();
    assert_eq!(buf.text(&start, &end, false).as_str(), "abc");

    window.close();
    common::pump_glib(32);
}

#[test]
fn unsupported_widget_returns_422_code() {
    if !require_display() {
        return;
    }
    let (window, _entry, _tv, label) = common::build_type_widgets().unwrap();
    window.present();
    common::pump_glib(64);

    let widget: gtk::Widget = label.clone().upcast();
    let err = type_text(&widget, "x", Some("#label1")).unwrap_err();
    assert!(matches!(err, TypeError::UnsupportedWidget { .. }));

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
    let err = type_text(&widget, "x", Some("#input1")).unwrap_err();
    assert!(matches!(err, TypeError::WidgetNotVisible { .. }));

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
    let err = type_text(&widget, "x", Some("#input1")).unwrap_err();
    assert!(matches!(err, TypeError::WidgetDisabled { .. }));

    window.close();
    common::pump_glib(32);
}

#[test]
fn empty_text_is_allowed() {
    if !require_display() {
        return;
    }
    let (window, entry, _tv, _label) = common::build_type_widgets().unwrap();
    entry.set_text("preset");
    window.present();
    common::pump_glib(64);

    let widget: gtk::Widget = entry.clone().upcast();
    type_text(&widget, "", Some("#input1")).expect("empty text replacement should be allowed");
    assert_eq!(entry.text().as_str(), "");

    window.close();
    common::pump_glib(32);
}
