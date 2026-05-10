//! Shared GTK init helper for integration tests.
//!
//! `gtk::init()` must be called once before any widget construction, and only
//! from the main thread. We use `std::sync::Once` so multiple integration
//! tests can each call `ensure_gtk_init()` defensively.
//!
//! Risk-1 (plan §6): if init has already happened from a different test, the
//! second call is a no-op.

#![cfg(feature = "e2e")]
#![allow(dead_code)] // helpers are wired up per integration test, not by this module.

use std::panic;
use std::sync::OnceLock;

use gtk4_e2e_server::gtk;
use gtk4_e2e_server::gtk::prelude::*;

static INIT: OnceLock<bool> = OnceLock::new();

pub fn ensure_gtk_init() -> bool {
    *INIT.get_or_init(|| {
        // gtk::init() may panic on display-less hosts (no DISPLAY /
        // WAYLAND_DISPLAY). Catch so the *outcome* (initialised or not) is
        // recorded once and subsequent callers see a clean boolean rather
        // than a poisoned synchronisation primitive.
        let result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
            let _ = gtk::init();
        }));
        result.is_ok() && gtk::is_initialized()
    })
}

/// Build a tiny ApplicationWindow with `entry1` / `btn1` / `label1` widget
/// names so input/wait tests can target it. Caller is responsible for
/// `window.present()` and pumping the main loop.
pub fn build_demo_widgets() -> Option<(gtk::ApplicationWindow, gtk::Entry, gtk::Button, gtk::Label)>
{
    if !gtk::is_initialized() {
        return None;
    }
    let app = gtk::Application::builder()
        .application_id("dev.gtk4-e2e.test")
        .build();
    let _ = app.register(None::<&gtk::gio::Cancellable>);

    let entry = gtk::Entry::builder().text("hello").build();
    entry.set_widget_name("entry1");
    let label = gtk::Label::new(Some("waiting..."));
    label.set_widget_name("label1");
    let button = gtk::Button::with_label("Apply");
    button.set_widget_name("btn1");

    {
        let entry_c = entry.clone();
        let label_c = label.clone();
        button.connect_clicked(move |_| {
            label_c.set_text(entry_c.text().as_str());
        });
    }

    let vbox = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    vbox.append(&entry);
    vbox.append(&button);
    vbox.append(&label);

    let window = gtk::ApplicationWindow::builder()
        .application(&app)
        .title("test")
        .child(&vbox)
        .build();

    Some((window, entry, button, label))
}

/// Build a window with widgets sized for `type` capability tests
/// (Step 9): an `Entry` (#input1, initially empty), a `TextView` (#tv1), and
/// a `Label` (#label1) for the unsupported-widget path. Kept separate from
/// `build_demo_widgets` so existing tap/wait tests do not need re-balancing.
pub fn build_type_widgets() -> Option<(
    gtk::ApplicationWindow,
    gtk::Entry,
    gtk::TextView,
    gtk::Label,
)> {
    if !gtk::is_initialized() {
        return None;
    }
    let app = gtk::Application::builder()
        .application_id("dev.gtk4-e2e.test")
        .build();
    let _ = app.register(None::<&gtk::gio::Cancellable>);

    let entry = gtk::Entry::builder().build();
    entry.set_widget_name("input1");
    let text_view = gtk::TextView::builder().build();
    text_view.set_widget_name("tv1");
    let label = gtk::Label::new(Some("not editable"));
    label.set_widget_name("label1");

    let vbox = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    vbox.append(&entry);
    vbox.append(&text_view);
    vbox.append(&label);

    let window = gtk::ApplicationWindow::builder()
        .application(&app)
        .title("test")
        .child(&vbox)
        .build();

    Some((window, entry, text_view, label))
}

/// Pump the GLib main loop a bounded number of iterations so synchronous
/// signal handlers fire and widget state stabilises. Returns when no work is
/// pending or when `max_iters` is exhausted.
pub fn pump_glib(max_iters: usize) {
    let ctx = gtk::glib::MainContext::default();
    for _ in 0..max_iters {
        if !ctx.iteration(false) {
            break;
        }
    }
}

/// Pump the GLib main loop for at least `duration` of wall-clock time. Used to
/// wait for `glib::timeout_add_local`-scheduled work to fire (real timers do
/// not advance with `ctx.iteration(false)` alone).
pub fn pump_glib_for(duration: std::time::Duration) {
    let ctx = gtk::glib::MainContext::default();
    let deadline = std::time::Instant::now() + duration;
    while std::time::Instant::now() < deadline {
        ctx.iteration(false);
        std::thread::sleep(std::time::Duration::from_millis(8));
    }
    for _ in 0..32 {
        if !ctx.iteration(false) {
            break;
        }
    }
}
