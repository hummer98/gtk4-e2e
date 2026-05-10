//! In-process e2e test server for GTK4 + Rust apps.
//!
//! Gated behind the `e2e` Cargo feature. With the feature disabled (the
//! default), this crate compiles to an effectively empty library and pulls
//! in none of the heavy runtime dependencies (`tokio`, `axum`, `gtk4`).
//!
//! See `docs/seed.md` §6 Step 1 and `docs/adr/0001-architecture.md`.

#[cfg(feature = "e2e")]
pub mod elements;
#[cfg(feature = "e2e")]
pub mod http;
#[cfg(feature = "e2e")]
pub mod input;
#[cfg(feature = "e2e")]
pub mod main_thread;
#[cfg(feature = "e2e")]
mod port;
#[cfg(feature = "e2e")]
pub mod proto;
#[cfg(feature = "e2e")]
mod registry;
#[cfg(feature = "e2e")]
mod schema_export;
#[cfg(feature = "e2e")]
pub mod snapshot;
#[cfg(feature = "e2e")]
pub mod state;
#[cfg(feature = "e2e")]
pub mod tree;
#[cfg(feature = "e2e")]
pub mod wait;
#[cfg(feature = "e2e")]
pub mod ws;

#[cfg(feature = "e2e")]
pub use gtk4 as gtk;

#[cfg(feature = "e2e")]
pub use crate::proto::{Capability, EventEnvelope, EventKind, Info};

#[cfg(feature = "e2e")]
pub use crate::registry::{runtime_dir, InstanceFile, REGISTRY_SUBDIR};

#[cfg(feature = "e2e")]
pub use crate::schema_export::write_schemas;

#[cfg(feature = "e2e")]
pub use crate::start_impl::{current_rfc3339, start, Handle};

#[cfg(feature = "e2e")]
mod start_impl {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::time::Duration;

    use crate::http::{router, AppState};
    use crate::main_thread::{install_app, spawn_receiver_loop, MainCmd};
    use crate::port::pick_free_listener;
    use crate::proto::{Capability, EventEnvelope, Info};
    use crate::registry::{delete_instance_file, runtime_dir, write_instance_file, InstanceFile};
    use crate::state::AppDefinedState;

    /// Live handle to a running in-process e2e server.
    ///
    /// Dropping the handle triggers graceful axum shutdown, removes the
    /// registry file, and joins the tokio runtime (with a short timeout).
    pub struct Handle {
        rt: Option<tokio::runtime::Runtime>,
        shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
        registry_path: Option<PathBuf>,
        info: Arc<Info>,
        event_tx: tokio::sync::broadcast::Sender<EventEnvelope>,
        state: AppDefinedState,
    }

    impl Handle {
        /// Snapshot of `GET /test/info` payload for this instance.
        pub fn info(&self) -> &Info {
            &self.info
        }

        /// Bound TCP port (also reachable via `info().port`).
        pub fn port(&self) -> u16 {
            self.info.port
        }

        /// Path of the registry file currently advertising this instance.
        pub fn registry_path(&self) -> Option<&Path> {
            self.registry_path.as_deref()
        }

        /// Push side of the event broadcast bus. Cloning a `Sender` is cheap.
        ///
        /// `broadcast::Sender::send(env)` returns:
        /// - `Ok(usize)` = number of currently-subscribed receivers (>= 1)
        /// - `Err(SendError(env))` when there are zero subscribers
        ///
        /// The `Ok(0)` case does **not** occur — the absence of subscribers
        /// manifests as `Err(SendError)`. Callers (e.g. the demo) typically
        /// discard the result with `let _ = handle.event_tx().send(env);`,
        /// dropping the event when no test client is attached.
        pub fn event_tx(&self) -> tokio::sync::broadcast::Sender<EventEnvelope> {
            self.event_tx.clone()
        }

        /// Replace the app-defined state snapshot exposed at `GET /test/state`.
        ///
        /// Subsequent `WaitCondition::AppStateEq { path, value }` polls observe
        /// the new snapshot. The semantics are *whole-snapshot replace* —
        /// callers that want to merge are responsible for assembling the full
        /// JSON before calling `set_state`. See `packages/demo/src/main.rs` for
        /// the accumulator pattern (T019).
        pub fn set_state(&self, value: serde_json::Value) {
            self.state.set(value);
        }
    }

    /// Spawn the in-process e2e server.
    ///
    /// Step 1 accepts the GTK `Application` to lock in the public signature
    /// from ADR-0001 §Decision but does not yet read it. Step 2 will populate
    /// `Info.app_name` from `app.application_id()`.
    ///
    /// Boot-time failures (port exhaustion, registry write, runtime build)
    /// panic — `docs/seed.md` §10 explicitly permits panics on server boot.
    pub fn start(app: &crate::gtk::Application) -> Handle {
        start_inner(app).expect("gtk4-e2e-server: boot failed")
    }

    fn start_inner(app: &crate::gtk::Application) -> std::io::Result<Handle> {
        let dir = runtime_dir()?;
        let (port, std_listener) = pick_free_listener()?;
        std_listener.set_nonblocking(true)?;

        let pid = std::process::id();
        let instance_id = uuid::Uuid::new_v4().simple().to_string();
        let app_name = "gtk4-e2e-app".to_string();
        let app_version = env!("CARGO_PKG_VERSION").to_string();
        let started_at = current_rfc3339();
        let token = std::env::var("GTK4_E2E_TOKEN").ok();

        let info = Arc::new(Info {
            instance_id,
            pid,
            port,
            app_name: app_name.clone(),
            app_version: app_version.clone(),
            capabilities: vec![
                Capability::Info,
                Capability::Tap,
                Capability::Wait,
                Capability::Screenshot,
                Capability::Events,
                Capability::Type,
                Capability::Swipe,
                Capability::Elements,
                Capability::State,
            ],
            token_required: token.as_ref().map(|_| true),
        });

        let instance = InstanceFile {
            pid,
            port,
            app_name,
            app_version,
            started_at,
            token,
        };
        let registry_path = write_instance_file(&dir, &instance)?;

        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .thread_name("gtk4-e2e-rt")
            .build()?;

        // Cross-runtime channel: tokio handlers → GLib main thread.
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::channel::<MainCmd>(64);
        install_app(app.clone());
        spawn_receiver_loop(cmd_rx);

        // Event bus for `WS /test/events`. Capacity 256 is a soft buffer:
        // SDK clients that lag past it observe `RecvError::Lagged` and skip
        // ahead. The receiver returned here is dropped immediately because
        // every connected WebSocket subscribes via `state.event_tx.subscribe()`
        // at upgrade time; with no live subscribers, `send` returns
        // `Err(SendError)` which the demo discards.
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel::<EventEnvelope>(256);

        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let app_state = AppDefinedState::default();
        let state = AppState {
            info: info.clone(),
            cmd_tx,
            event_tx: event_tx.clone(),
            state: app_state.clone(),
        };
        let app_router = router(state);

        rt.spawn(async move {
            let tokio_listener = match tokio::net::TcpListener::from_std(std_listener) {
                Ok(l) => l,
                Err(_) => return,
            };
            let _ = axum::serve(tokio_listener, app_router)
                .with_graceful_shutdown(async move {
                    let _ = shutdown_rx.await;
                })
                .await;
        });

        Ok(Handle {
            rt: Some(rt),
            shutdown_tx: Some(shutdown_tx),
            registry_path: Some(registry_path),
            info,
            event_tx,
            state: app_state,
        })
    }

    impl Drop for Handle {
        fn drop(&mut self) {
            if let Some(tx) = self.shutdown_tx.take() {
                let _ = tx.send(());
            }
            if let Some(p) = self.registry_path.take() {
                delete_instance_file(&p);
            }
            if let Some(rt) = self.rt.take() {
                rt.shutdown_timeout(Duration::from_secs(2));
            }
        }
    }

    /// RFC3339 UTC timestamp for `EventEnvelope.ts` and `InstanceFile.started_at`.
    ///
    /// Exposed publicly so the demo can stamp events without re-pulling `time`
    /// as a direct dependency. Returns the Unix epoch on formatter failure.
    pub fn current_rfc3339() -> String {
        time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
    }
}
