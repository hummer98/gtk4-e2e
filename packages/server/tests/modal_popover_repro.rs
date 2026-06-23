//! Regression tests for issue #10: tapping a button inside an open
//! autohide/modal `GtkPopover` (a confirm-dialog pattern) and dismissing it via
//! `POST /test/key {"key":"Escape"}`.
//!
//! These need GTK initialised, so they auto-skip on display-less hosts (CI
//! without xvfb, headless macOS) AND on macOS even with a display, because
//! libtest runs each test on a spawned worker thread and macOS GTK refuses
//! off-main-thread init. The main-thread verification path is
//! `cargo run --example modal-popover-probe --features e2e`. On Linux/AGX with
//! a display these run as real assertions.

#![cfg(feature = "e2e")]

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use gtk4_e2e_server::gtk;
use gtk4_e2e_server::gtk::prelude::*;
use gtk4_e2e_server::input::{send_key, validate_tap, KeyError, TapPlan};
use gtk4_e2e_server::tree::{find_first, parse_selector, GtkTree};

fn require_display() -> bool {
    if !common::ensure_gtk_init() {
        eprintln!("[skip] no GTK display available");
        return false;
    }
    true
}

/// window > vbox > [trigger Button#open-popover] with an autohide Popover
/// (#confirm-popover) parented on the trigger, containing #popover-confirm /
/// #popover-cancel buttons. Both buttons pop the popover down synchronously in
/// their `clicked` handler (the grab-reentrant shape). The returned `Rc` holds
/// the last clicked button's label so tests can confirm the handler ran.
fn build_modal_popover_fixture() -> (
    gtk::Application,
    gtk::ApplicationWindow,
    gtk::Popover,
    Rc<RefCell<Option<&'static str>>>,
) {
    let app = gtk::Application::builder()
        .application_id("dev.gtk4-e2e.modal-popover-repro")
        .build();
    let _ = app.register(None::<&gtk::gio::Cancellable>);

    let result: Rc<RefCell<Option<&'static str>>> = Rc::new(RefCell::new(None));

    let confirm = gtk::Button::with_label("Delete");
    confirm.set_widget_name("popover-confirm");
    let cancel = gtk::Button::with_label("Cancel");
    cancel.set_widget_name("popover-cancel");

    let pop_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    pop_box.append(&confirm);
    pop_box.append(&cancel);

    let popover = gtk::Popover::builder().autohide(true).build();
    popover.set_widget_name("confirm-popover");
    popover.set_child(Some(&pop_box));

    let trigger = gtk::Button::with_label("Open");
    trigger.set_widget_name("open-popover");
    popover.set_parent(&trigger);

    {
        let popover = popover.clone();
        let result = result.clone();
        cancel.connect_clicked(move |_| {
            popover.popdown();
            *result.borrow_mut() = Some("cancelled");
        });
    }
    {
        let popover = popover.clone();
        let result = result.clone();
        confirm.connect_clicked(move |_| {
            popover.popdown();
            *result.borrow_mut() = Some("deleted");
        });
    }

    let vbox = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    vbox.append(&trigger);

    let window = gtk::ApplicationWindow::builder()
        .application(&app)
        .default_width(400)
        .default_height(300)
        .child(&vbox)
        .build();
    window.present();
    common::pump_glib(64);

    (app, window, popover, result)
}

/// `find_first` reaches a button inside an open modal popover (the popover is a
/// child widget in the GTK4 hierarchy, reachable via first_child/next_sibling).
#[test]
fn find_first_resolves_button_inside_open_modal_popover() {
    if !require_display() {
        return;
    }
    let (app, window, popover, _result) = build_modal_popover_fixture();
    popover.popup();
    common::pump_glib(64);

    let tree = GtkTree { app: &app };
    let sel = parse_selector("#popover-cancel").unwrap();
    let found = find_first(tree, &sel);
    assert!(
        found.is_some(),
        "#popover-cancel inside an open modal popover must be resolvable"
    );
    assert_eq!(
        found.unwrap().type_().name(),
        "GtkButton",
        "resolved widget should be the Cancel button"
    );

    window.close();
    common::pump_glib(32);
}

/// `validate_tap` returns a plan (no premature side-effect); firing the plan's
/// deferred action via an idle callback runs the click handler and pops the
/// popover down — the modal grab is torn down without re-entry stalling.
#[test]
fn deferred_tap_fires_click_and_dismisses_modal_popover() {
    if !require_display() {
        return;
    }
    let (app, window, popover, result) = build_modal_popover_fixture();
    popover.popup();
    common::pump_glib(64);
    assert!(popover.is_mapped(), "precondition: popover open");

    let tree = GtkTree { app: &app };
    let sel = parse_selector("#popover-cancel").unwrap();
    let widget = find_first(tree, &sel).expect("cancel button resolvable");

    let plan: TapPlan = validate_tap(&widget, Some("#popover-cancel"))
        .expect("cancel button is a supported (Button) tap target");

    // Validation alone must not have fired the click yet.
    assert!(
        result.borrow().is_none(),
        "validate_tap must not fire the action"
    );

    // Defer the action exactly as the dispatch path does, then pump so the idle
    // callback fires (and replies on its oneshot). The reply receiver is dropped
    // immediately — we assert the *effect* (widget state) rather than the wire
    // reply, since awaiting a oneshot off the GLib thread isn't available here.
    let (tx, rx) = tokio::sync::oneshot::channel::<Result<(), gtk4_e2e_server::input::TapError>>();
    plan.run_deferred(tx);
    common::pump_glib(128);
    drop(rx);

    assert_eq!(
        result.borrow().as_deref(),
        Some("cancelled"),
        "deferred tap should have fired the Cancel click handler"
    );
    assert!(
        !popover.is_mapped(),
        "modal popover should be dismissed after the deferred tap"
    );

    window.close();
    common::pump_glib(32);
}

/// `send_key(window, "Escape")` pops down an open autohide popover.
#[test]
fn send_key_escape_dismisses_modal_popover() {
    if !require_display() {
        return;
    }
    let (_app, window, popover, _result) = build_modal_popover_fixture();
    popover.popup();
    common::pump_glib(64);
    assert!(popover.is_mapped(), "precondition: popover open");

    send_key(window.upcast_ref::<gtk::Window>(), "Escape").expect("Escape should be supported");
    common::pump_glib(64);

    assert!(
        !popover.is_mapped(),
        "Escape via /test/key should dismiss the modal popover"
    );

    window.close();
    common::pump_glib(32);
}

/// `send_key` is a no-op success when no popover is open (cleanup-safe).
#[test]
fn send_key_escape_is_noop_when_no_popover() {
    if !require_display() {
        return;
    }
    let (_app, window, _popover, _result) = build_modal_popover_fixture();
    // popover NOT popped up.
    send_key(window.upcast_ref::<gtk::Window>(), "Escape")
        .expect("Escape with no open popover is a no-op success");

    window.close();
    common::pump_glib(32);
}

/// Unsupported key names are rejected with `UnsupportedKey`.
#[test]
fn send_key_rejects_unsupported_key() {
    if !require_display() {
        return;
    }
    let (_app, window, _popover, _result) = build_modal_popover_fixture();
    let err = send_key(window.upcast_ref::<gtk::Window>(), "Enter").unwrap_err();
    assert!(
        matches!(err, KeyError::UnsupportedKey { .. }),
        "non-Escape keys must be rejected, got {err:?}"
    );

    window.close();
    common::pump_glib(32);
}
