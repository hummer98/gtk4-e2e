//! Phase 3 TDD gate: prove that a `tokio::sync::mpsc` round-trip can cross
//! from a tokio worker thread into a glib `MainContext::spawn_local` task and
//! back via `oneshot::channel`. Plan §Q9 / Review M1.
//!
//! If this test fails, the long-polling design is unworkable as planned and
//! the alternatives in plan §Q9 (`glib::ReceiverExt::attach`, `Rc<RefCell<...>>`)
//! must be revisited.

#![cfg(feature = "e2e")]

mod common;

use std::time::{Duration, Instant};

use gtk4_e2e_server::gtk::glib;
use gtk4_e2e_server::main_thread::{spawn_receiver_loop, MainCmd};

#[test]
fn oneshot_roundtrip_under_glib_and_tokio() {
    if !common::ensure_gtk_init() {
        eprintln!("[skip] no GTK display available");
        return;
    }

    // Channel over which the worker thread asks the GLib main context to do
    // something — here just an Echo handler.
    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::channel::<MainCmd>(8);

    // Receiver loop runs on the GLib main context (default = main thread).
    spawn_receiver_loop(cmd_rx);

    // Build a tokio runtime in *this* test thread; the test thread is also the
    // GLib main thread, so we drive both halves by alternating between
    // `rt.block_on(...)` and `MainContext::iteration(false)`.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();

    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel::<()>();
    let cmd_tx_clone = cmd_tx.clone();
    rt.spawn(async move {
        cmd_tx_clone
            .send(MainCmd::Echo { reply: reply_tx })
            .await
            .expect("worker → main_thread send should succeed");
    });

    // Pump both runtimes until the oneshot reply lands or the deadline hits.
    let deadline = Instant::now() + Duration::from_secs(5);
    let main_ctx = glib::MainContext::default();
    let mut reply_rx = reply_rx;
    loop {
        // Pump pending GLib work so spawn_local-ed futures get a chance to
        // process the queued MainCmd.
        for _ in 0..16 {
            if !main_ctx.iteration(false) {
                break;
            }
        }

        // Try to take the reply without blocking the test thread.
        let outcome = rt.block_on(async {
            tokio::time::timeout(Duration::from_millis(50), &mut reply_rx).await
        });
        match outcome {
            Ok(Ok(())) => return,
            Ok(Err(_)) => panic!("oneshot reply channel closed without sending"),
            Err(_) => {} // still waiting
        }

        if Instant::now() >= deadline {
            panic!("oneshot reply did not arrive within 5s — cross-runtime dispatch is broken");
        }
    }
}
