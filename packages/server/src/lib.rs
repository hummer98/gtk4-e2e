//! In-process e2e test server for GTK4 + Rust apps.
//!
//! Gated behind the `e2e` Cargo feature. With the feature disabled (the
//! default), this crate compiles to an effectively empty library and pulls
//! in none of the heavy runtime dependencies (`tokio`, `axum`, `gtk4`).
//!
//! See `docs/seed.md` §6 Step 1 and `docs/adr/0001-architecture.md`.

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
pub mod tree;
#[cfg(feature = "e2e")]
pub mod wait;

#[cfg(feature = "e2e")]
pub use gtk4 as gtk;

#[cfg(feature = "e2e")]
pub use crate::proto::{Capability, Info};

#[cfg(feature = "e2e")]
pub use crate::registry::{runtime_dir, InstanceFile, REGISTRY_SUBDIR};

#[cfg(feature = "e2e")]
pub use crate::schema_export::write_schemas;

#[cfg(feature = "e2e")]
pub use crate::start_impl::{start, Handle};

#[cfg(feature = "e2e")]
mod start_impl {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::time::Duration;

    use crate::http::{router, AppState};
    use crate::main_thread::{install_app, spawn_receiver_loop, MainCmd};
    use crate::port::pick_free_listener;
    use crate::proto::{Capability, Info};
    use crate::registry::{delete_instance_file, runtime_dir, write_instance_file, InstanceFile};

    /// Live handle to a running in-process e2e server.
    ///
    /// Dropping the handle triggers graceful axum shutdown, removes the
    /// registry file, and joins the tokio runtime (with a short timeout).
    pub struct Handle {
        rt: Option<tokio::runtime::Runtime>,
        shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
        registry_path: Option<PathBuf>,
        info: Arc<Info>,
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

        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let state = AppState {
            info: info.clone(),
            cmd_tx,
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

    fn current_rfc3339() -> String {
        time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
    }
}
