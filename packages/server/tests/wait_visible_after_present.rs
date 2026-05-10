//! T006 Open Q-K — `SelectorVisible` evaluated immediately after `window.present()`.
//!
//! `is_mapped()` is the property that lags one frame behind `present()` on
//! some backends (notably macOS quartz). This test pins the contract that the
//! 100 ms long-poll tick gives mapping enough time to settle, so a 1000 ms
//! `wait` request returns 200 instead of 408 even when the GLib loop has not
//! been pumped before the request.
//!
//! This is a dedicated integration binary so the thread-local APP slot in
//! `tests/common/mod.rs::ensure_gtk_init` is process-isolated from the other
//! display-required suites (`http_routes.rs`, `input_tap.rs`).

#![cfg(feature = "e2e")]

mod common;

use std::sync::Arc;

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use serde_json::{json, Value};
use tower::ServiceExt;

use gtk4_e2e_server::gtk;
use gtk4_e2e_server::gtk::prelude::*;
use gtk4_e2e_server::http::{router, AppState};
use gtk4_e2e_server::main_thread::{install_app, spawn_receiver_loop, MainCmd};
use gtk4_e2e_server::proto::{Capability, Info};
use gtk4_e2e_server::state::AppDefinedState;

fn build_state(cmd_tx: tokio::sync::mpsc::Sender<MainCmd>) -> AppState {
    let info = Arc::new(Info {
        instance_id: "test".into(),
        pid: 0,
        port: 0,
        app_name: "test".into(),
        app_version: "0".into(),
        capabilities: vec![
            Capability::Info,
            Capability::Tap,
            Capability::Wait,
            Capability::Screenshot,
            Capability::Events,
            Capability::Type,
            Capability::Swipe,
            Capability::State,
        ],
        token_required: None,
    });
    let (event_tx, _event_rx) =
        tokio::sync::broadcast::channel::<gtk4_e2e_server::EventEnvelope>(8);
    AppState {
        info,
        cmd_tx,
        event_tx,
        state: AppDefinedState::default(),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn selector_visible_immediately_after_present_returns_200() {
    if !common::ensure_gtk_init() {
        eprintln!("[skip] no GTK display available");
        return;
    }

    // Build the application + window but do NOT pre-pump the GLib loop. We
    // want `wait` to be issued while `is_mapped()` is potentially still
    // false on backends that lag a frame.
    let app_gtk = gtk::Application::builder()
        .application_id("dev.gtk4-e2e.routetest-present-race")
        .build();
    let _ = app_gtk.register(None::<&gtk::gio::Cancellable>);

    let label = gtk::Label::new(Some("ok"));
    label.set_widget_name("label1");
    let window = gtk::ApplicationWindow::builder()
        .application(&app_gtk)
        .child(&label)
        .build();
    window.present();
    // No pump_glib here — the polling driver inside `wait` must give mapping
    // time to settle on its own.

    install_app(app_gtk);
    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::channel::<MainCmd>(8);
    spawn_receiver_loop(cmd_rx);

    let state = build_state(cmd_tx);
    let app = router(state);

    let body = json!({
        "condition": {"kind": "selector_visible", "selector": "#label1"},
        "timeout_ms": 1_000,
    });
    let req = Request::builder()
        .method("POST")
        .uri("/test/wait")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();

    // Drive the GLib loop concurrently so the long-poll's tick can observe
    // is_mapped flipping to true. Mirror of `http_routes::wait_times_out_408`.
    let resp = tokio::select! {
        r = app.oneshot(req) => r.unwrap(),
        _ = async {
            for _ in 0..200 {
                for _ in 0..16 {
                    if !gtk::glib::MainContext::default().iteration(false) {
                        break;
                    }
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        } => panic!("pump finished before wait request returned"),
    };

    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "selector_visible should match within timeout once present() settles"
    );
    let bytes = to_bytes(resp.into_body(), 1 << 20).await.unwrap();
    let v: Value = serde_json::from_slice(&bytes).unwrap();
    assert!(v.get("elapsed_ms").is_some(), "missing elapsed_ms");

    window.close();
    common::pump_glib(32);
}

#[tokio::test(flavor = "current_thread")]
async fn selector_visible_for_invisible_widget_times_out() {
    if !common::ensure_gtk_init() {
        eprintln!("[skip] no GTK display available");
        return;
    }

    let app_gtk = gtk::Application::builder()
        .application_id("dev.gtk4-e2e.routetest-present-race-invisible")
        .build();
    let _ = app_gtk.register(None::<&gtk::gio::Cancellable>);

    let label = gtk::Label::new(Some("hidden"));
    label.set_widget_name("label1");
    label.set_visible(false);
    let window = gtk::ApplicationWindow::builder()
        .application(&app_gtk)
        .child(&label)
        .build();
    window.present();

    install_app(app_gtk);
    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::channel::<MainCmd>(8);
    spawn_receiver_loop(cmd_rx);

    let state = build_state(cmd_tx);
    let app = router(state);

    let body = json!({
        "condition": {"kind": "selector_visible", "selector": "#label1"},
        "timeout_ms": 250,
    });
    let req = Request::builder()
        .method("POST")
        .uri("/test/wait")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();

    let resp = tokio::select! {
        r = app.oneshot(req) => r.unwrap(),
        _ = async {
            for _ in 0..400 {
                for _ in 0..16 {
                    if !gtk::glib::MainContext::default().iteration(false) {
                        break;
                    }
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        } => panic!("pump finished before wait request returned"),
    };

    assert_eq!(
        resp.status(),
        StatusCode::REQUEST_TIMEOUT,
        "invisible widget should time out (NotYet ticks)"
    );

    window.close();
    common::pump_glib(32);
}
