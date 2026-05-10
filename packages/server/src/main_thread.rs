//! Cross-runtime dispatch between tokio worker threads and the GLib main
//! context. Plan §Q9 / Phase 3 (TDD gate).
//!
//! gtk4-rs widgets are not `Send`, so they can only be touched from the GLib
//! main thread. HTTP handlers, however, run on tokio worker threads. We
//! bridge the two by:
//!
//! 1. Creating a `tokio::sync::mpsc::Sender<MainCmd>` shared via `AppState`.
//! 2. Spawning a single receiver loop via `glib::MainContext::spawn_local`,
//!    which is allowed to host `!Send` futures.
//! 3. Each `MainCmd` carries a `tokio::sync::oneshot::Sender` so the handler
//!    can reply asynchronously.
//!
//! The receiver loop wraps each command in `std::panic::catch_unwind` so a
//! widget-side panic kills the request rather than the entire server.

use std::cell::RefCell;
use std::panic;

use crate::gtk::glib;
use tokio::sync::mpsc;
use tokio::sync::oneshot;

use crate::elements::ElementsError;
use crate::input::{SwipeError, TapError, TypeError};
use crate::proto::{ElementsResponse, TapTarget, TypeRequest, WaitCondition, XY};
use crate::snapshot::ScreenshotError;

/// Result of evaluating a `WaitCondition` for one tick.
#[derive(Debug, Clone, PartialEq)]
pub enum WaitTickOutcome {
    Matched,
    NotYet,
}

/// Permanent failures during wait-condition evaluation. Plan §Q8 splits these
/// from `selector_not_found`, which is treated as a tick failure.
#[derive(Debug, Clone, PartialEq)]
pub enum WaitEvalError {
    InvalidSelector(String),
    UnsupportedPropertyType(String),
    Internal(String),
}

/// Outcome of one wait tick. Either an outcome (matched / not yet / not
/// found), or a permanent failure that should bail out of the polling loop.
#[derive(Debug, Clone, PartialEq)]
pub enum WaitTickResult {
    Outcome(WaitTickOutcome),
    SelectorNotFound,
    PermanentFailure(WaitEvalError),
}

/// Commands posted from tokio handlers to the GLib main thread.
pub enum MainCmd {
    /// Phase 3 smoke variant — replies with `()`. Also used by health checks.
    Echo { reply: oneshot::Sender<()> },
    /// Synthesize a tap.
    Tap {
        target: TapTarget,
        reply: oneshot::Sender<Result<(), TapError>>,
    },
    /// Evaluate a wait condition once.
    EvalWait {
        condition: WaitCondition,
        reply: oneshot::Sender<WaitTickResult>,
    },
    /// Capture the active window as PNG bytes.
    Screenshot {
        reply: oneshot::Sender<Result<Vec<u8>, ScreenshotError>>,
    },
    /// Insert text into a widget (Step 9, T013).
    Type {
        request: TypeRequest,
        reply: oneshot::Sender<Result<(), TypeError>>,
    },
    /// Synthesize a swipe over `duration_ms` (T014, plan §5.4).
    Swipe {
        from: XY,
        to: XY,
        duration_ms: u64,
        reply: oneshot::Sender<Result<(), SwipeError>>,
    },
    /// Walk the widget tree (Step 14, T018).
    Elements {
        selector: Option<String>,
        max_depth: Option<u32>,
        reply: oneshot::Sender<Result<ElementsResponse, ElementsError>>,
    },
}

thread_local! {
    /// Holds the currently active `gtk::Application` for the GLib main thread.
    /// Set by `start()` before spawning the receiver loop so widget-side
    /// handlers can read it without juggling closures.
    pub(crate) static APP: RefCell<Option<crate::gtk::Application>> = const { RefCell::new(None) };
}

/// Install the active `gtk::Application` for the current (GLib main) thread.
pub fn install_app(app: crate::gtk::Application) {
    APP.with(|slot| {
        slot.borrow_mut().replace(app);
    });
}

/// Read the currently installed `gtk::Application` (if any) from this thread.
pub(crate) fn with_app<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&crate::gtk::Application) -> R,
{
    APP.with(|slot| slot.borrow().as_ref().map(f))
}

/// Spawn the receiver loop on the default GLib main context.
///
/// `spawn_local` accepts `!Send` futures, which is required because the
/// receiver future borrows `mpsc::Receiver` (a `Send` type) and resolves
/// commands that ultimately touch `!Send` GTK widgets.
pub fn spawn_receiver_loop(mut cmd_rx: mpsc::Receiver<MainCmd>) {
    let ctx = glib::MainContext::default();
    ctx.spawn_local(async move {
        while let Some(cmd) = cmd_rx.recv().await {
            // Per-command catch_unwind so a panic during widget access kills
            // the request only (not the receiver loop). plan §Q9 / Risk-5.
            let _ = panic::catch_unwind(panic::AssertUnwindSafe(|| handle_cmd(cmd)));
        }
    });
}

fn handle_cmd(cmd: MainCmd) {
    match cmd {
        MainCmd::Echo { reply } => {
            let _ = reply.send(());
        }
        MainCmd::Tap { target, reply } => {
            let outcome = with_app(|app| crate::wait::dispatch_tap(app, &target))
                .unwrap_or(Err(TapError::NoActiveWindow));
            let _ = reply.send(outcome);
        }
        MainCmd::EvalWait { condition, reply } => {
            let outcome = with_app(|app| crate::wait::eval_condition_in_app(app, &condition))
                .unwrap_or(WaitTickResult::PermanentFailure(WaitEvalError::Internal(
                    "no_active_window".into(),
                )));
            let _ = reply.send(outcome);
        }
        MainCmd::Screenshot { reply } => {
            let outcome = with_app(crate::snapshot::render_active_window)
                .unwrap_or(Err(ScreenshotError::NoActiveWindow));
            let _ = reply.send(outcome);
        }
        MainCmd::Type { request, reply } => {
            let outcome = with_app(|app| crate::wait::dispatch_type(app, &request))
                .unwrap_or(Err(TypeError::NoActiveWindow));
            let _ = reply.send(outcome);
        }
        MainCmd::Swipe {
            from,
            to,
            duration_ms,
            reply,
        } => {
            // Plan T014 §5.4: APP.with directly so the `Some/None` arms can
            // each consume `reply` without borrow-checker contortions inside
            // a `with_app` closure.
            APP.with(|slot| match slot.borrow().as_ref() {
                Some(app) => {
                    crate::wait::dispatch_swipe(app, from, to, duration_ms, reply);
                }
                None => {
                    let _ = reply.send(Err(SwipeError::NoActiveWindow));
                }
            });
        }
        MainCmd::Elements {
            selector,
            max_depth,
            reply,
        } => {
            let outcome = with_app(|app| {
                crate::elements::walk_elements(app, selector.as_deref(), max_depth)
            })
            .unwrap_or(Err(ElementsError::NoActiveWindow));
            let _ = reply.send(outcome);
        }
    }
}
