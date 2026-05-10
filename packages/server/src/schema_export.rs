//! JSON Schema exporter — proto.rs SSOT → `*.schema.json` artifacts.
//!
//! Backs ADR-0002: schemars output is committed and acts as the SSOT diff
//! anchor; TS types are derived from these files via
//! `packages/client/scripts/gen-types.ts`. Feature-gated (`e2e`) so default
//! builds remain free of `schemars`/`serde_json`.

use std::fs;
use std::io;
use std::path::Path;

use schemars::{schema_for, JsonSchema};
use serde_json::Value;

use crate::proto::{
    Bounds, Capability, ElementInfo, ElementsResponse, EventEnvelope, EventKind, Info,
    PinchRequest, SwipeRequest, TapTarget, TypeRequest, WaitCondition, WaitRequest, WaitResult,
};

const PROVENANCE: &str = "AUTO-GENERATED FROM packages/server/src/proto.rs — do not edit by hand";

/// Emit `*.schema.json` files for every SSOT proto type into `out_dir`.
///
/// Each file is pretty-printed JSON (2-space indent), terminates with a single
/// LF, and carries a top-level `$comment` referencing the SSOT.
pub fn write_schemas(out_dir: &Path) -> io::Result<()> {
    fs::create_dir_all(out_dir)?;
    write_one::<Info>(out_dir, "Info")?;
    write_one::<Capability>(out_dir, "Capability")?;
    write_one::<TapTarget>(out_dir, "TapTarget")?;
    write_one::<TypeRequest>(out_dir, "TypeRequest")?;
    write_one::<SwipeRequest>(out_dir, "SwipeRequest")?;
    write_one::<PinchRequest>(out_dir, "PinchRequest")?;
    write_one::<WaitRequest>(out_dir, "WaitRequest")?;
    write_one::<WaitCondition>(out_dir, "WaitCondition")?;
    write_one::<WaitResult>(out_dir, "WaitResult")?;
    write_one::<EventEnvelope>(out_dir, "EventEnvelope")?;
    write_one::<EventKind>(out_dir, "EventKind")?;
    write_one::<Bounds>(out_dir, "Bounds")?;
    write_one::<ElementInfo>(out_dir, "ElementInfo")?;
    write_one::<ElementsResponse>(out_dir, "ElementsResponse")?;
    Ok(())
}

fn write_one<T: JsonSchema>(out_dir: &Path, name: &str) -> io::Result<()> {
    let schema = schema_for!(T);
    let mut value = serde_json::to_value(&schema).map_err(io::Error::other)?;
    if let Value::Object(ref mut map) = value {
        map.insert("$comment".into(), Value::String(PROVENANCE.into()));
    } else {
        return Err(io::Error::other(format!(
            "schema for {name} was not a JSON object"
        )));
    }

    let mut bytes = serde_json::to_vec_pretty(&value).map_err(io::Error::other)?;
    bytes.push(b'\n');

    let path = out_dir.join(format!("{name}.schema.json"));
    fs::write(&path, bytes)?;
    Ok(())
}
