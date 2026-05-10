//! HTTP route integration tests using `tower::ServiceExt::oneshot`.
//!
//! Tap / wait routes that need real widget interaction depend on GTK init;
//! those tests skip on display-less hosts. Routes that exercise pure 4xx /
//! 501 paths (validation, fallback) run unconditionally.

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

fn make_state() -> (AppState, tokio::sync::mpsc::Sender<MainCmd>) {
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
        ],
        token_required: None,
    });
    let (cmd_tx, _cmd_rx) = tokio::sync::mpsc::channel::<MainCmd>(8);
    let (event_tx, _event_rx) =
        tokio::sync::broadcast::channel::<gtk4_e2e_server::EventEnvelope>(8);
    (
        AppState {
            info,
            cmd_tx: cmd_tx.clone(),
            event_tx,
        },
        cmd_tx,
    )
}

async fn body_json(resp: axum::response::Response) -> Value {
    let bytes = to_bytes(resp.into_body(), 1 << 20).await.unwrap();
    serde_json::from_slice(&bytes).unwrap_or(Value::Null)
}

// ----- 501 / 4xx paths (no GTK needed) -----

#[tokio::test]
async fn unknown_route_returns_501() {
    let (state, _tx) = make_state();
    let app = router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/test/blooper")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_IMPLEMENTED);
}

#[tokio::test]
async fn tap_invalid_selector_422() {
    let (state, _tx) = make_state();
    let app = router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/test/tap")
                .header("content-type", "application/json")
                .body(Body::from("{\"selector\":\".bad\"}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let v = body_json(resp).await;
    assert_eq!(
        v.get("error").and_then(Value::as_str),
        Some("invalid_selector")
    );
}

#[tokio::test]
async fn tap_malformed_body_400() {
    let (state, _tx) = make_state();
    let app = router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/test/tap")
                .header("content-type", "application/json")
                .body(Body::from("not json"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let v = body_json(resp).await;
    assert_eq!(v.get("error").and_then(Value::as_str), Some("bad_request"));
}

#[tokio::test]
async fn wait_invalid_timeout_zero_422() {
    let (state, _tx) = make_state();
    let app = router(state);
    let body = json!({
        "condition": {"kind": "selector_visible", "selector": "#x"},
        "timeout_ms": 0,
    });
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/test/wait")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let v = body_json(resp).await;
    assert_eq!(
        v.get("error").and_then(Value::as_str),
        Some("invalid_timeout")
    );
}

#[tokio::test]
async fn wait_invalid_timeout_excessive_422() {
    let (state, _tx) = make_state();
    let app = router(state);
    let body = json!({
        "condition": {"kind": "selector_visible", "selector": "#x"},
        "timeout_ms": 600_001,
    });
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/test/wait")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn wait_invalid_selector_422() {
    let (state, _tx) = make_state();
    let app = router(state);
    let body = json!({
        "condition": {"kind": "selector_visible", "selector": ".bad"},
        "timeout_ms": 100,
    });
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/test/wait")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let v = body_json(resp).await;
    assert_eq!(
        v.get("error").and_then(Value::as_str),
        Some("invalid_selector")
    );
}

// ----- GTK-bound routes (display required) -----

fn require_display() -> bool {
    if !common::ensure_gtk_init() {
        eprintln!("[skip] no GTK display available");
        return false;
    }
    true
}

#[tokio::test(flavor = "current_thread")]
async fn tap_endpoint_returns_200() {
    if !require_display() {
        return;
    }
    let (mut state, _tx) = make_state();
    let app_gtk = gtk::Application::builder()
        .application_id("dev.gtk4-e2e.routetest1")
        .build();
    let _ = app_gtk.register(None::<&gtk::gio::Cancellable>);

    let button = gtk::Button::with_label("press");
    button.set_widget_name("btn1");
    let label = gtk::Label::new(Some("waiting"));
    label.set_widget_name("label1");
    {
        let label = label.clone();
        button.connect_clicked(move |_| label.set_text("hello"));
    }
    let vbox = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    vbox.append(&button);
    vbox.append(&label);
    let window = gtk::ApplicationWindow::builder()
        .application(&app_gtk)
        .child(&vbox)
        .build();
    window.present();
    common::pump_glib(64);

    install_app(app_gtk);

    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::channel::<MainCmd>(8);
    spawn_receiver_loop(cmd_rx);
    state.cmd_tx = cmd_tx;

    let app = router(state);

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/test/tap")
                .header("content-type", "application/json")
                .body(Body::from("{\"selector\":\"#btn1\"}"))
                .unwrap(),
        )
        .await
        .unwrap();
    common::pump_glib(64);
    assert_eq!(resp.status(), StatusCode::OK);

    common::pump_glib(64);
    assert_eq!(label.text().as_str(), "hello");

    window.close();
    common::pump_glib(32);
}

#[tokio::test(flavor = "current_thread")]
async fn tap_selector_not_found_404() {
    if !require_display() {
        return;
    }
    let (mut state, _tx) = make_state();
    let app_gtk = gtk::Application::builder()
        .application_id("dev.gtk4-e2e.routetest2")
        .build();
    let _ = app_gtk.register(None::<&gtk::gio::Cancellable>);
    let window = gtk::ApplicationWindow::builder()
        .application(&app_gtk)
        .build();
    window.present();
    common::pump_glib(64);
    install_app(app_gtk);

    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::channel::<MainCmd>(8);
    spawn_receiver_loop(cmd_rx);
    state.cmd_tx = cmd_tx;

    let app = router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/test/tap")
                .header("content-type", "application/json")
                .body(Body::from("{\"selector\":\"#missing\"}"))
                .unwrap(),
        )
        .await
        .unwrap();
    common::pump_glib(64);
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let v = body_json(resp).await;
    assert_eq!(
        v.get("error").and_then(Value::as_str),
        Some("selector_not_found")
    );

    window.close();
    common::pump_glib(32);
}

#[tokio::test(flavor = "current_thread")]
async fn wait_endpoint_returns_result() {
    if !require_display() {
        return;
    }
    let (mut state, _tx) = make_state();
    let app_gtk = gtk::Application::builder()
        .application_id("dev.gtk4-e2e.routetest3")
        .build();
    let _ = app_gtk.register(None::<&gtk::gio::Cancellable>);

    let label = gtk::Label::new(Some("hello"));
    label.set_widget_name("label1");
    let window = gtk::ApplicationWindow::builder()
        .application(&app_gtk)
        .child(&label)
        .build();
    window.present();
    common::pump_glib(64);
    install_app(app_gtk);

    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::channel::<MainCmd>(8);
    spawn_receiver_loop(cmd_rx);
    state.cmd_tx = cmd_tx;

    let app = router(state);
    let body = json!({
        "condition": {
            "kind": "state_eq",
            "selector": "#label1",
            "property": "label",
            "value": "hello"
        },
        "timeout_ms": 1000,
    });
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/test/wait")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    common::pump_glib(64);
    assert_eq!(resp.status(), StatusCode::OK);
    let v = body_json(resp).await;
    assert!(v.get("elapsed_ms").is_some());

    window.close();
    common::pump_glib(32);
}

#[tokio::test(flavor = "current_thread")]
async fn wait_times_out_408() {
    if !require_display() {
        return;
    }
    let (mut state, _tx) = make_state();
    let app_gtk = gtk::Application::builder()
        .application_id("dev.gtk4-e2e.routetest4")
        .build();
    let _ = app_gtk.register(None::<&gtk::gio::Cancellable>);
    let window = gtk::ApplicationWindow::builder()
        .application(&app_gtk)
        .build();
    window.present();
    common::pump_glib(64);
    install_app(app_gtk);

    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::channel::<MainCmd>(8);
    spawn_receiver_loop(cmd_rx);
    state.cmd_tx = cmd_tx;

    let app = router(state);
    let body = json!({
        "condition": {"kind": "selector_visible", "selector": "#never"},
        "timeout_ms": 250,
    });
    // The handler awaits; we need to drive the GLib loop concurrently. Do
    // that by calling oneshot() in a tokio::join with a glib pump.
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
        } => panic!("pump finished before request returned"),
    };
    assert_eq!(resp.status(), StatusCode::REQUEST_TIMEOUT);
    let v = body_json(resp).await;
    assert_eq!(v.get("error").and_then(Value::as_str), Some("wait_timeout"));

    window.close();
    common::pump_glib(32);
}

#[tokio::test(flavor = "current_thread")]
async fn screenshot_endpoint_returns_png() {
    if !require_display() {
        return;
    }
    let (mut state, _tx) = make_state();
    let app_gtk = gtk::Application::builder()
        .application_id("dev.gtk4-e2e.routetest5")
        .build();
    let _ = app_gtk.register(None::<&gtk::gio::Cancellable>);

    let label = gtk::Label::new(Some("scr"));
    label.set_widget_name("label1");
    let window = gtk::ApplicationWindow::builder()
        .application(&app_gtk)
        .child(&label)
        .default_width(64)
        .default_height(48)
        .build();
    window.present();
    common::pump_glib(64);
    install_app(app_gtk);

    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::channel::<MainCmd>(8);
    spawn_receiver_loop(cmd_rx);
    state.cmd_tx = cmd_tx;

    let app = router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/test/screenshot")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    common::pump_glib(64);

    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok()),
        Some("image/png")
    );
    let bytes = to_bytes(resp.into_body(), 1 << 20).await.unwrap();
    assert!(
        bytes.len() > 100,
        "PNG body suspiciously small: {} bytes",
        bytes.len()
    );
    assert_eq!(
        &bytes[..8],
        b"\x89PNG\r\n\x1a\n",
        "missing PNG signature in first 8 bytes"
    );

    window.close();
    common::pump_glib(32);
}

#[tokio::test(flavor = "current_thread")]
async fn screenshot_no_active_window_returns_422() {
    if !require_display() {
        return;
    }
    let (mut state, _tx) = make_state();
    let app_gtk = gtk::Application::builder()
        .application_id("dev.gtk4-e2e.routetest6")
        .build();
    let _ = app_gtk.register(None::<&gtk::gio::Cancellable>);
    install_app(app_gtk);

    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::channel::<MainCmd>(8);
    spawn_receiver_loop(cmd_rx);
    state.cmd_tx = cmd_tx;

    let app = router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/test/screenshot")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    common::pump_glib(32);

    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let v = body_json(resp).await;
    assert_eq!(
        v.get("error").and_then(Value::as_str),
        Some("no_active_window")
    );
}
