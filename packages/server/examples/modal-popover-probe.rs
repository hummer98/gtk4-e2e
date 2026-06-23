//! Main-thread end-to-end verification for issue #10 (macOS-friendly).
//!
//! libtest runs each `#[test]` on a spawned worker thread; macOS GTK refuses to
//! init off the main thread, so the integration tests auto-skip locally. This
//! `example` runs on the real main thread, drives the real HTTP server against
//! a modal-popover fixture (with grab-reentrant `popdown()` click handlers like
//! the demo / Brainship confirm dialog), and verifies both:
//!   1. `POST /test/tap` on `#popover-cancel` returns 200 (deferred dispatch).
//!   2. `POST /test/key {"key":"Escape"}` pops the popover down.
//!
//! Run: cargo run -p gtk4-e2e-server --example modal-popover-probe --features e2e

use std::cell::RefCell;
use std::rc::Rc;

use gtk4_e2e_server::gtk;
use gtk4_e2e_server::gtk::prelude::*;

fn pump(max_iters: usize) {
    let ctx = gtk::glib::MainContext::default();
    for _ in 0..max_iters {
        if !ctx.iteration(false) {
            break;
        }
    }
}

/// Fire an HTTP POST on a background thread while the main thread keeps pumping
/// the GLib loop. Returns `Some(http_code)` or `None` if it hung past `~8s`.
fn http_post(port: u16, path: &str, body: &'static str) -> Option<String> {
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    let url = format!("http://127.0.0.1:{port}{path}");
    std::thread::spawn(move || {
        let out = std::process::Command::new("curl")
            .args([
                "-s",
                "-m",
                "5",
                "-o",
                "/dev/null",
                "-w",
                "%{http_code}",
                "-X",
                "POST",
                "-H",
                "content-type: application/json",
                "-d",
                body,
                &url,
            ])
            .output();
        let msg = match out {
            Ok(o) => String::from_utf8_lossy(&o.stdout).to_string(),
            Err(e) => format!("curl-err:{e}"),
        };
        let _ = tx.send(msg);
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
    while std::time::Instant::now() < deadline {
        pump(8);
        if let Ok(msg) = rx.try_recv() {
            return Some(msg);
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    None
}

fn main() {
    if gtk::init().is_err() {
        eprintln!("[skip] gtk init failed (no display)");
        return;
    }

    let app = gtk::Application::builder()
        .application_id("dev.gtk4-e2e.modal-popover-probe")
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

    // Grab-reentrant handlers: synchronous popdown inside `clicked`.
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
    pump(64);

    let handle = gtk4_e2e_server::start(&app);
    let port = handle.port();
    println!("[probe] server up on port {port}");
    pump(16);

    // ---- Scenario 1: tap #popover-cancel inside a live modal grab ----
    popover.popup();
    pump(64);
    println!(
        "[probe] (1) popover open before tap: mapped={}",
        popover.is_mapped()
    );

    match http_post(port, "/test/tap", "{\"selector\":\"#popover-cancel\"}") {
        Some(code) => println!("[probe] (1) POST /test/tap -> http {code}"),
        None => println!("[probe] (1) POST /test/tap HUNG (>8s) <-- issue #10 repro"),
    }
    pump(64);
    println!(
        "[probe] (1) after tap: popover mapped={} result={:?}",
        popover.is_mapped(),
        result.borrow()
    );

    // ---- Scenario 2: /test/key Escape closes the popover ----
    *result.borrow_mut() = None;
    popover.popup();
    pump(64);
    println!(
        "[probe] (2) popover reopened: mapped={}",
        popover.is_mapped()
    );

    match http_post(port, "/test/key", "{\"key\":\"Escape\"}") {
        Some(code) => println!("[probe] (2) POST /test/key Escape -> http {code}"),
        None => println!("[probe] (2) POST /test/key HUNG (>8s)"),
    }
    pump(64);
    println!(
        "[probe] (2) after Escape: popover mapped={}",
        popover.is_mapped()
    );

    // ---- Scenario 3: unsupported key -> 422 ----
    match http_post(port, "/test/key", "{\"key\":\"Enter\"}") {
        Some(code) => println!("[probe] (3) POST /test/key Enter -> http {code} (expect 422)"),
        None => println!("[probe] (3) POST /test/key Enter HUNG"),
    }

    drop(handle);
    window.close();
    pump(32);
    println!("[probe] done");
}
