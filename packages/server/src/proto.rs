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
/// Step 7 appends `Events` for the `WS /test/events` channel.
/// Step 9 appends `Type` (T013) for `POST /test/type` and `Swipe` (T014) for `POST /test/swipe`.
#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    Info,
    Tap,
    Wait,
    Screenshot,
    Events,
    Type,
    Swipe,
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

/// Body of `POST /test/type` (Step 9).
///
/// MVP semantics: the server replaces the entire text content of the
/// resolved widget (Entry / Editable / TextView) with `text`. There is no
/// "insert at cursor" mode — see plan §2.2.
#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone, PartialEq, Eq)]
pub struct TypeRequest {
    pub selector: String,
    pub text: String,
}

/// Body of `POST /test/wait`.
#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone, PartialEq)]
pub struct WaitRequest {
    pub condition: WaitCondition,
    pub timeout_ms: u64,
}

/// Body of `POST /test/swipe`.
///
/// `from` / `to` are window-local pixel coordinates (top-left origin) of the
/// active window. `duration_ms = 0` is rejected with HTTP 422 (see
/// `SwipeError::ZeroDuration`); the upper bound is 10 000 ms.
#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone, PartialEq, Eq)]
pub struct SwipeRequest {
    pub from: XY,
    pub to: XY,
    pub duration_ms: u64,
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

/// Body of a single message sent over `WS /test/events` (Step 7).
///
/// Wire shape is internally tagged on `kind`. `data` is intentionally an
/// opaque JSON value so new event kinds can be added without renegotiating
/// the schema. SDK consumers narrow on `kind` and parse `data` per variant.
///
/// `ts` is RFC3339 UTC, mirroring `InstanceFile.started_at` (Step 1).
#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone, PartialEq)]
pub struct EventEnvelope {
    pub kind: EventKind,
    pub ts: String,
    pub data: serde_json::Value,
}

/// Discriminator for `EventEnvelope.kind`.
///
/// `LogLine` is reserved for a future tracing-layer integration (Step >= 8);
/// the variant exists today so filter strings are stable across versions.
/// Until that integration ships, the server never produces `EventEnvelope`s
/// with `kind = LogLine` — clients can pass `"log_line"` in the filter list
/// without error, but no frames will be delivered for that kind alone.
#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    StateChange,
    LogLine,
}
