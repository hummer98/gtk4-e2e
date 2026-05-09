//! Local discovery registry.
//!
//! Each running instance writes `${runtime_dir}/instance-${pid}.json` so that
//! external SDKs (e.g. `packages/client`) can enumerate active processes via
//! `E2EClient.discover()`. Files are written atomically (tmp file + rename)
//! and removed on shutdown (best effort).

use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Subdirectory created beneath `$XDG_RUNTIME_DIR` (or `temp_dir`).
pub const REGISTRY_SUBDIR: &str = "gtk4-e2e";

/// JSON payload written to `instance-${pid}.json`.
///
/// `token` is only present when `$GTK4_E2E_TOKEN` is set; the `null` case is
/// elided from the JSON to keep the SDK side typed as `token?: string`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct InstanceFile {
    pub pid: u32,
    pub port: u16,
    pub app_name: String,
    pub app_version: String,
    pub started_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

/// Resolve the directory where instance files live, creating it if missing.
///
/// Honors `$XDG_RUNTIME_DIR` when set and existing; otherwise falls back to
/// `std::env::temp_dir()` (covers macOS, where `XDG_RUNTIME_DIR` is unset).
pub fn runtime_dir() -> io::Result<PathBuf> {
    let base = match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(p) => {
            let p = PathBuf::from(p);
            if p.is_dir() {
                p
            } else {
                std::env::temp_dir()
            }
        }
        None => std::env::temp_dir(),
    };
    let dir = base.join(REGISTRY_SUBDIR);
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Write `instance-${pid}.json` atomically into `dir`.
///
/// Strategy: write to `instance-${pid}.json.tmp.${uuid}`, then rename to the
/// target. Concurrent readers therefore never see a partially written file.
pub fn write_instance_file(dir: &Path, info: &InstanceFile) -> io::Result<PathBuf> {
    fs::create_dir_all(dir)?;
    let target = dir.join(format!("instance-{}.json", info.pid));
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let tmp = dir.join(format!("instance-{}.json.tmp.{suffix}", info.pid));
    {
        let f = fs::File::create(&tmp)?;
        serde_json::to_writer_pretty(&f, info).map_err(io::Error::other)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, &target)?;
    Ok(target)
}

/// Remove an instance file; never panics, never logs.
pub fn delete_instance_file(path: &Path) {
    let _ = fs::remove_file(path);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Tests that mutate the process-wide environment must hold this lock.
    /// `std::env::{set_var, remove_var}` is `unsafe` since Rust 1.84 due to
    /// data-race risk against concurrent `getenv` callers.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn sample(pid: u32) -> InstanceFile {
        InstanceFile {
            pid,
            port: 19042,
            app_name: "gtk4-e2e-app".into(),
            app_version: "0.1.0".into(),
            started_at: "2026-05-10T00:00:00Z".into(),
            token: None,
        }
    }

    #[test]
    fn write_then_read_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let info = sample(12345);
        let path = write_instance_file(tmp.path(), &info).unwrap();
        assert_eq!(path, tmp.path().join("instance-12345.json"));

        let bytes = fs::read(&path).unwrap();
        let read: InstanceFile = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(read, info);

        let entries: Vec<_> = fs::read_dir(tmp.path())
            .unwrap()
            .filter_map(Result::ok)
            .collect();
        assert_eq!(
            entries.len(),
            1,
            "tmp file should be renamed away, not left behind"
        );
    }

    #[test]
    fn token_none_is_omitted() {
        let s = serde_json::to_string(&sample(1)).unwrap();
        assert!(!s.contains("token"), "expected no token key, got {s}");
    }

    #[test]
    fn token_some_round_trips() {
        let mut info = sample(2);
        info.token = Some("secret".into());
        let s = serde_json::to_string(&info).unwrap();
        assert!(s.contains("\"token\":\"secret\""), "got {s}");
        let back: InstanceFile = serde_json::from_str(&s).unwrap();
        assert_eq!(back, info);
    }

    #[test]
    fn runtime_dir_uses_xdg_when_set_and_falls_back_when_unset() {
        let _guard = ENV_LOCK.lock().unwrap();
        let original = std::env::var_os("XDG_RUNTIME_DIR");

        let xdg = tempfile::tempdir().unwrap();
        // SAFETY: serialized via ENV_LOCK; tests in this module never read
        // env in parallel with this mutation.
        unsafe {
            std::env::set_var("XDG_RUNTIME_DIR", xdg.path());
        }
        let got = runtime_dir().unwrap();
        assert_eq!(got, xdg.path().join(REGISTRY_SUBDIR));
        assert!(got.is_dir());

        // SAFETY: same as above.
        unsafe {
            std::env::remove_var("XDG_RUNTIME_DIR");
        }
        let got = runtime_dir().unwrap();
        assert_eq!(got, std::env::temp_dir().join(REGISTRY_SUBDIR));

        // restore for any subsequent tests
        // SAFETY: same as above.
        unsafe {
            match original {
                Some(v) => std::env::set_var("XDG_RUNTIME_DIR", v),
                None => std::env::remove_var("XDG_RUNTIME_DIR"),
            }
        }
    }

    #[test]
    fn delete_is_best_effort() {
        let tmp = tempfile::tempdir().unwrap();
        // non-existent path must not panic
        delete_instance_file(&tmp.path().join("missing.json"));

        let present = tmp.path().join("present.json");
        fs::write(&present, b"hi").unwrap();
        delete_instance_file(&present);
        assert!(!present.exists());
    }
}
