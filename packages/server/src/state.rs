//! App-defined state snapshot exposed at `GET /test/state` (T019).
//!
//! `AppDefinedState` is a thin `Arc<RwLock<serde_json::Value>>` wrapper. The
//! demo / consumer pushes whole-snapshot replacements via `Handle::set_state`,
//! and `WaitCondition::AppStateEq { path, value }` resolves `path` as a JSON
//! Pointer (RFC 6901) inside the snapshot.
//!
//! The wrapper is `Send + Sync` and holds no GTK references, so it can be read
//! from the tokio HTTP handler without the GLib main-loop round-trip
//! (`MainCmd::EvalWait`) that GTK widget queries require.

use std::sync::{Arc, RwLock};

use serde_json::Value;

#[derive(Clone, Default)]
pub struct AppDefinedState(Arc<RwLock<Value>>);

impl AppDefinedState {
    /// Clone of the current snapshot. Returns `Value::Null` before any `set`.
    pub fn snapshot(&self) -> Value {
        self.0
            .read()
            .expect("AppDefinedState lock poisoned")
            .clone()
    }

    /// Replace the entire snapshot. Subsequent `pointer` / `snapshot` reads
    /// observe the new value.
    pub fn set(&self, value: Value) {
        *self.0.write().expect("AppDefinedState lock poisoned") = value;
    }

    /// Resolve `path` as a JSON Pointer (RFC 6901) inside the current
    /// snapshot. Returns `None` when the path does not resolve (intermediate
    /// node missing, array index out of range, …).
    pub fn pointer(&self, path: &str) -> Option<Value> {
        self.0
            .read()
            .expect("AppDefinedState lock poisoned")
            .pointer(path)
            .cloned()
    }
}
