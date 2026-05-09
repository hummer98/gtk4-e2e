//! Protocol types — single source of truth (SSOT) for both Rust handlers and
//! the TypeScript SDK. Step 3 will derive `*.gen.ts` from these types via
//! `schemars`. For Step 1 only `Info` and `Capabilities` are exercised.

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

/// Capability identifiers advertised in `Info.capabilities`. Step 1 only
/// surfaces `Info`; further variants land in subsequent steps.
#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    Info,
}
