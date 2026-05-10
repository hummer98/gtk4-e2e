//! Protocol types — single source of truth (SSOT) for both Rust handlers and
//! the TypeScript SDK. `*.gen.ts` is derived from these via schemars; see
//! ADR-0002 / plan §Q10.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Response payload of `GET /test/info`.
#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone, PartialEq, Eq)]
pub struct Info {
    pub instance_id: String,
    pub pid: u32,
    pub port: u16,
    pub app_name: String,
    pub app_version: String,
    pub capabilities: Vec<Capability>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_required: Option<bool>,
}

/// Capability identifiers advertised in `Info.capabilities`.
///
/// Variants are appended in the order in which they are surfaced. Step 6
/// extends the deterministic ordering to `[Info, Tap, Wait, Screenshot]`.
#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    Info,
    Tap,
    Wait,
    Screenshot,
}

/// Window-local pixel coordinates (top-left origin).
#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone, Copy, PartialEq, Eq)]
pub struct XY {
    pub x: i32,
    pub y: i32,
}

/// Body of `POST /test/tap`.
///
/// Untagged so the wire shape is `{ "selector": "#btn1" }` or
/// `{ "xy": { "x": 1, "y": 2 } }`. Plan §Q10 explicitly forbids
/// `pub type TapRequest = TapTarget` aliasing — `TapTarget` is the single name
/// used end-to-end (schema title → TS type → SDK method signature).
#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone, PartialEq, Eq)]
#[serde(untagged)]
pub enum TapTarget {
    Selector { selector: String },
    Xy { xy: XY },
}

/// Body of `POST /test/wait`.
#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone, PartialEq)]
pub struct WaitRequest {
    pub condition: WaitCondition,
    pub timeout_ms: u64,
}

/// Condition long-polled by `/test/wait`.
///
/// Tagged on `kind` so SDK consumers can narrow the union by discriminator.
#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WaitCondition {
    SelectorVisible {
        selector: String,
    },
    StateEq {
        selector: String,
        property: String,
        value: serde_json::Value,
    },
}

/// Success body of `/test/wait`.
///
/// Plan §Q10 / Review m9: 200 always implies match, so `matched` would be
/// redundant. Timeout is signalled by HTTP 408, not a body flag.
#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone, Copy, PartialEq, Eq)]
pub struct WaitResult {
    pub elapsed_ms: u64,
}
