//! Integration tests for `notify::register` — GObject `notify::<prop>` signal
//! stream surfaced as `EventKind::Property` envelopes on the broadcast bus.
//!
//! Auto-skips on display-less hosts (no `gtk::init()`), like `elements_walk.rs`.
//! Plan T026 §C-1 / §C-2 / §C-3.

#![cfg(feature = "e2e")]

mod common;

use std::time::{Duration, Instant};

use gtk4_e2e_server::gtk;
use gtk4_e2e_server::gtk::prelude::*;
use gtk4_e2e_server::proto::{EventEnvelope, EventKind};

fn require_display() -> bool {
    if !common::ensure_gtk_init() {
        eprintln!("[skip] no GTK display available");
        return false;
    }
    true
}

#[test]
fn transition_detected_within_100ms() {
    if !require_display() {
        return;
    }

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();

    let (event_tx, _drop_rx) = tokio::sync::broadcast::channel::<EventEnvelope>(32);
    let clock_base = Instant::now();

    let stack = gtk::Stack::builder().build();
    stack.set_widget_name("stack1");
    let m1 = gtk::Label::new(Some("mode1"));
    let m2 = gtk::Label::new(Some("mode2"));
    stack.add_named(&m1, Some("mode1"));
    stack.add_named(&m2, Some("mode2"));

    let registered = gtk4_e2e_server::notify::register(&event_tx, clock_base, stack.upcast_ref());
    assert!(
        registered.iter().any(|p| p == "visible-child-name"),
        "expected visible-child-name to be registered, got {registered:?}"
    );

    let mut rx = event_tx.subscribe();

    common::pump_glib(8);
    stack.set_visible_child_name("mode2");

    let env = rt.block_on(async {
        tokio::time::timeout(Duration::from_millis(100), rx.recv())
            .await
            .expect("event did not arrive within 100ms")
            .expect("broadcast closed")
    });

    assert_eq!(env.kind, EventKind::Property);
    let data = env.data;
    assert_eq!(data["property"], "visible-child-name");
    assert_eq!(data["value"], "mode2");
    assert_eq!(data["widget_kind"], "GtkStack");
    let widget_id = data["widget_id"]
        .as_str()
        .expect("widget_id must be string");
    assert!(
        widget_id.starts_with('w'),
        "widget_id should start with 'w', got {widget_id}"
    );
    assert!(
        data["ts_ns"].as_u64().expect("ts_ns must be u64") > 0,
        "ts_ns must be positive"
    );
}

#[test]
fn non_allowlisted_widget_emits_no_event() {
    if !require_display() {
        return;
    }

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();

    let (event_tx, _drop_rx) = tokio::sync::broadcast::channel::<EventEnvelope>(32);
    let clock_base = Instant::now();

    let label = gtk::Label::new(Some("initial"));
    let registered = gtk4_e2e_server::notify::register(&event_tx, clock_base, label.upcast_ref());
    assert!(
        registered.is_empty(),
        "GtkLabel is not in the allowlist, expected empty registration, got {registered:?}"
    );

    let mut rx = event_tx.subscribe();
    common::pump_glib(8);
    label.set_label("changed");

    let result =
        rt.block_on(async { tokio::time::timeout(Duration::from_millis(150), rx.recv()).await });
    assert!(
        result.is_err(),
        "expected timeout (no event), got {result:?}"
    );
}

#[test]
fn overhead_under_50ms_for_1000_widgets() {
    // T026 §C-3: 1000 create + register + drop cycles. Linux/CI enforces the
    // <50ms acceptance limit; macOS relaxes it to a smoke 200ms because local
    // schedulers jitter through the 50ms cliff. Display-less hosts skip.
    if !require_display() {
        return;
    }

    let (event_tx, _drop_rx) = tokio::sync::broadcast::channel::<EventEnvelope>(32);
    let clock_base = Instant::now();

    let t0 = Instant::now();
    for _ in 0..1000 {
        let entry = gtk::Entry::builder().build();
        let _ = gtk4_e2e_server::notify::register(&event_tx, clock_base, entry.upcast_ref());
        drop(entry);
    }
    let elapsed = t0.elapsed();

    #[cfg(target_os = "linux")]
    let budget = Duration::from_millis(50);
    #[cfg(not(target_os = "linux"))]
    let budget = Duration::from_millis(200);

    assert!(
        elapsed < budget,
        "1000 register cycles took {elapsed:?}, budget {budget:?}"
    );
}
