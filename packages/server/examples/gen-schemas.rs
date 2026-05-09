//! Codegen entry point — emit `proto/schemas/{Info,Capability}.schema.json`.
//!
//! Invoked by `task gen:types` and the CI stale-check
//! (see ADR-0002 §Verification). The output directory is resolved relative to
//! `CARGO_MANIFEST_DIR` so the script behaves identically regardless of the
//! caller's cwd.
//!
//! Always run via:
//!     cargo run -p gtk4-e2e-server --example gen-schemas --features e2e

use std::path::PathBuf;
use std::process::ExitCode;

use gtk4_e2e_server::write_schemas;

fn main() -> ExitCode {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let out_dir = manifest_dir.join("proto").join("schemas");

    match write_schemas(&out_dir) {
        Ok(()) => {
            println!("wrote schemas to {}", out_dir.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("gen-schemas: failed to write {}: {e}", out_dir.display());
            ExitCode::FAILURE
        }
    }
}
