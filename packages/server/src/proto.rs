//! Protocol types — single source of truth (SSOT) for both Rust handlers and
//! the TypeScript SDK. `*.gen.ts` is derived from these via schemars; see
//! ADR-0002 / plan §Q10.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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
/// Step 14 appends `Elements` (T018) for `GET /test/elements`.
/// T019 appends `State` for `GET /test/state` (app-defined state snapshot).
/// Step 9 (c) appends `Pinch` (T015) for `POST /test/pinch`.
/// Task 029 appends `Press` (T029) for `POST /test/press` (GestureLongPress).
/// `Key` (issue #10) appends `POST /test/key`. `SetValue` appends
/// `POST /test/set-value` (GtkRange / GtkScale). `TouchDrag` (issue #13) appends
/// `POST /test/touch-drag` (held GtkGestureDrag sequence).
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
    Elements,
    State,
    Pinch,
    Focus,
    Press,
    Key,
    SetValue,
    TouchDrag,
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

/// Body of `POST /test/focus` (issue #3).
///
/// Selector-only — mirrors `TypeRequest` minus `text`. The server resolves the
/// widget and calls `grab_focus()` so `:focus` / `:focus-within` dependent CSS
/// renders for screenshot verification.
#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone, PartialEq, Eq)]
pub struct FocusRequest {
    pub selector: String,
}

/// Body of `POST /test/set-value`.
///
/// Drives a `GtkRange` (GtkScale / GtkScrollbar) to a target value via
/// `gtk::Range::set_value`, which clamps to the adjustment range and fires
/// `value-changed` / `notify::value` for app-side observers — the same
/// "achieve the semantic effect through the most reliable API" shortcut used by
/// `tap` (`emit_clicked`) and `swipe` (animated adjustment), since GTK4's safe
/// APIs cannot synthesise the click+drag a user would perform on the trough.
///
/// Exactly one of `selector` / `xy` selects the target widget (HTTP 422
/// otherwise):
///   - `selector`: resolve the `GtkRange` by `#name` / `.class`. `value` is
///     **required** in this mode (there is no coordinate to derive it from).
///   - `xy`: window-local pixel coordinates; the widget under the point (or its
///     nearest `GtkRange` ancestor) is the target.
///
/// `value` resolution:
///   - present: used directly (clamped server-side to the adjustment range).
///   - absent: requires `xy` — derived from the point's position along the
///     trough (orientation- and `inverted`-aware, best-effort).
///
/// `value` is rejected when non-finite (NaN / ±Inf) with HTTP 422
/// `invalid_value`.
///
/// `Eq` is intentionally omitted: `Option<f64>` is not `Eq`.
#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone, PartialEq)]
pub struct SetValueRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub xy: Option<XY>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
}

/// One waypoint of a `POST /test/touch-drag` path (issue #13).
///
/// `dx` / `dy` are pixel offsets **from the drag start point** (not from the
/// previous waypoint), matching `gtk::GestureDrag`'s `drag-update` offset
/// semantics so wedge / direction logic that reads the cumulative offset sees
/// exactly these values. A single waypoint `{dx: 0, dy: -120}` drags 120 px up.
///
/// `Eq` is intentionally omitted: `f64` is not `Eq` (parity with `PinchRequest`).
#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone, Copy, PartialEq)]
pub struct Waypoint {
    pub dx: f64,
    pub dy: f64,
}

/// Body of `POST /test/touch-drag` (issue #13).
///
/// Drives a held touch sequence — press-down → hold(`hold_ms`) → drag through
/// `waypoints` → optional release — as a **single `gtk::GestureDrag` sequence**:
/// the server emits `drag-begin` at the start point, waits `hold_ms` of real
/// wall-clock (so an app-side long-press timer crosses its threshold without
/// movement), then emits `drag-update` for each waypoint, and finally
/// `drag-end` when `release` is true. This mirrors the `tap`/`swipe`/`press`
/// pragmatism: GTK4's safe APIs cannot synthesise raw motion events, so we drive
/// the gesture's signals directly — the app's `connect_drag_begin` /
/// `connect_drag_update` / `connect_drag_end` handlers see one coherent
/// sequence.
///
/// Exactly one of `selector` / `xy` selects the drag origin (HTTP 422
/// otherwise). The target widget or an ancestor must carry a `GtkGestureDrag`
/// (or subclass); otherwise `no_draggable_*` (404).
///
/// `hold_ms` is bounded to 1..=10000 (`invalid_hold`). `waypoints` may be empty
/// (a pure press-hold-release) and is capped (`too_many_waypoints`).
///
/// `release` defaults to `true`. `release: false` leaves the gesture mid-drag
/// (no `drag-end`) so the highlighted / armed state can be screenshot-verified
/// before a follow-up call releases it.
///
/// `Eq` is intentionally omitted: `Waypoint` carries `f64`.
#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone, PartialEq)]
pub struct TouchDragRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub xy: Option<XY>,
    pub hold_ms: u64,
    #[serde(default)]
    pub waypoints: Vec<Waypoint>,
    #[serde(default = "default_release")]
    pub release: bool,
}

/// Default for `TouchDragRequest::release` — a touch-drag releases at its
/// endpoint unless the caller opts to hold (`release: false`).
fn default_release() -> bool {
    true
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

/// Body of `POST /test/pinch` (Step 9 (c), T015).
///
/// `center` is window-local pixel coordinates of the pinch focal point.
/// `scale > 1.0` zooms in, `scale < 1.0` zooms out, `scale = 1.0` is a no-op.
/// `duration_ms = 0` is rejected with HTTP 422 (`PinchError::ZeroDuration`);
/// the upper bound is 10 000 ms (mirror of swipe).
///
/// `Eq` is intentionally omitted: `f32` is not `Eq` (NaN is not reflexive).
#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone, PartialEq)]
pub struct PinchRequest {
    pub center: XY,
    pub scale: f32,
    pub duration_ms: u64,
}

/// Body of `POST /test/press` (Task 029, T029).
///
/// Injects a press → hold(`hold_ms`) → release sequence to fire a
/// `GtkGestureLongPress` (the `pressed` signal). Exactly one of `selector` /
/// `xy` must be provided (HTTP 422 otherwise):
///   - `selector`: resolve the widget by `#name` / `.class`, find a
///     `GestureLongPress` on it (or an ancestor), and emit at the widget centre.
///   - `xy`: window-local pixel coordinates; the widget under the point and its
///     ancestors are searched for a `GestureLongPress`.
///
/// `hold_ms = 0` is rejected (`PressError::ZeroHold`); the upper bound is
/// 10 000 ms (`PressError::HoldTooLong`). Independent fields (option-b) rather
/// than an untagged enum so the optionality of both targets is explicit.
///
/// `Eq` is derivable: every field type (`Option<String>`, `Option<XY>`, `u64`)
/// is `Eq` (unlike `PinchRequest`, which carries `f32`).
#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone, PartialEq, Eq)]
pub struct PressRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub xy: Option<XY>,
    pub hold_ms: u64,
}

/// Body of `POST /test/key` (issue #10 companion).
///
/// Delivers a single key to the active window so tests can dismiss modal UI
/// (an autohide `GtkPopover` confirm dialog) without relying on a click that
/// might re-enter the modal grab. MVP supports `key = "Escape"`; the active
/// window's open autohide popover is popped down (the documented Escape
/// behaviour). Unknown key names are rejected with 422 `unsupported_key`.
#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone, PartialEq, Eq)]
pub struct KeyRequest {
    pub key: String,
}

/// Condition long-polled by `/test/wait`.
///
/// Tagged on `kind` so SDK consumers can narrow the union by discriminator.
///
/// T019 appends `AppStateEq` for app-defined state snapshots: `path` is a
/// JSON Pointer (RFC 6901, e.g. `""` for root or `/foo/bar`) into the state
/// pushed via `Handle::set_state`. Path resolution failure is treated as a
/// tick failure so that schema drift on the app side surfaces as 408 timeout
/// rather than a permanent 422 (HTTP layer still rejects leading-`/`-missing
/// paths up-front via static validation).
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
    AppStateEq {
        path: String,
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
    Property,
}

/// Payload carried by `EventEnvelope.data` when `kind = Property`.
///
/// Emitted from a GObject `notify::<property>` signal hook (T026). The full
/// envelope on the wire is:
///
/// ```json
/// {
///   "kind": "property",
///   "ts": "2026-05-27T03:14:15Z",
///   "data": {
///     "widget_id": "w7f8b1c…",
///     "widget_kind": "GtkStack",
///     "property": "visible-child-name",
///     "value": "mode2",
///     "ts_ns": 1234567890
///   }
/// }
/// ```
///
/// `value` follows the same typing as `ElementInfo.properties`: String /
/// bool / i32 / f64, GEnum as nick string, GFlags as array of nick strings,
/// with the `{"$unsupported": "GTypeName"}` sentinel for
/// property types outside that set. Missing properties cannot occur here
/// (notify only fires for properties the widget actually exposes), so there
/// is no `$missing` sentinel.
///
/// `widget_id` is the GObject pointer formatted as `"w<hex>"`; it is stable
/// for the lifetime of the widget but distinct from `ElementInfo.id` (which
/// is a walk-local DFS index). The two ID spaces are intentionally separate
/// in the MVP — see plan §A-2.
///
/// `ts_ns` is a monotonic-clock nanosecond stamp relative to the server's
/// `Handle::start` time, so receivers can compute deterministic relative
/// latencies without RFC3339 parsing. The absolute value is not a UNIX epoch
/// stamp.
///
/// `Eq` is intentionally omitted: `serde_json::Value` may contain `f64`,
/// which does not implement `Eq` (parity with `PinchRequest`).
#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone, PartialEq)]
pub struct PropertyEventData {
    pub widget_id: String,
    pub widget_kind: String,
    pub property: String,
    pub value: serde_json::Value,
    pub ts_ns: u64,
}

/// Window-local widget bounds in CSS pixels (top-left origin).
///
/// Source: `gtk::Widget::compute_bounds(window_root)` for widgets in the
/// toplevel surface. The graphene `Rect` returns `f32`; we widen to `f64` here
/// so JSON consumers don't need to reason about precision quirks of the
/// float ↔ JSON round-trip.
///
/// Widgets on a separate native surface (an open `GtkPopover` and its
/// descendants) have no `compute_bounds` value across the surface boundary, so
/// their bounds are instead **synthesized into the same toplevel-widget
/// coordinate system**: the popover root from its `GdkPopup` geometry
/// (`position_x/y` + surface size) translated by the toplevel
/// `surface_transform`, and each descendant by offsetting its
/// popover-root-relative rect by that origin. The popup surface size may
/// include CSD shadow margin, so the popover rect can read slightly larger
/// than the visible content.
#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// One node of the widget tree returned by `GET /test/elements`.
///
/// `id` is a process-stable DFS pre-order index of the form `"e0"`,
/// `"e1"`, ... assigned during a single walk. It is **not** stable across
/// walks or app restarts — selectors (`#name` / `.class`) are the
/// primary refer mechanism. `id` exists for response triage / log lines
/// where a temporary handle is enough.
///
/// `widget_name` mirrors `widget.widget_name()`. Empty strings are
/// normalised to `None` (matches `tree::GtkTree::name()` semantics —
/// tree.rs:169-176).
///
/// `bounds` is `None` for unrealized / unmapped widgets. An **open** popover
/// (and its children) carries real synthesized bounds; a closed / unrealized
/// popover stays `None` (see `Bounds` for the coordinate synthesis).
///
/// `text` carries the human-visible text of text-bearing widgets
/// (issue #17): `GtkLabel` (displayed text), `GtkEditable` implementors
/// such as `GtkEntry` (current content), and `GtkTextView` (full buffer
/// content, hidden characters excluded). `None` — and absent on the
/// wire — for every other widget kind, so existing consumers are
/// unaffected.
///
/// `properties` is populated only when the caller asks for it via the
/// `props=` query parameter on `GET /test/elements` (opt-in; absent in
/// the response when empty). Each entry maps the requested GObject
/// property name to its current value, JSON-encoded by
/// `wait::read_property_as_json`. Supported types: String, bool, i32,
/// f64 (MVP set), plus GEnum values as their nick string (e.g.
/// `"wrap-mode": "word-char"`) and GFlags values as an array of nick
/// strings (issue #17). Sentinels are used for failure modes so the
/// response stays a flat JSON object instead of a tagged union:
///
/// - `{"$missing": true}` — widget exposes no such property.
/// - `{"$unsupported": "GTypeName"}` — property exists but its value
///   type is outside the supported set (e.g. boxed types like GdkRGBA).
///
/// These sentinels are stable wire contract; SDKs may decode them into
/// a richer typed result. Key ordering is deterministic (BTreeMap).
#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone, PartialEq)]
pub struct ElementInfo {
    pub id: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub widget_name: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub css_classes: Vec<String>,
    pub visible: bool,
    pub sensitive: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bounds: Option<Bounds>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub properties: Option<BTreeMap<String, serde_json::Value>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<ElementInfo>,
}

/// Body of `GET /test/elements`.
///
/// `roots` contains:
///   - selector unset: one entry per active window (typically 1).
///   - selector set: one entry per matching widget, in DFS pre-order
///     (outer matches before nested ones; nested matches inside an outer
///     match are not duplicated as separate roots).
///
/// Empty `roots` is **not** an error; HTTP returns 200 with `count: 0`.
///
/// `count` is the total number of `ElementInfo` nodes across all roots
/// (recursive sum of root + children).
#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone, PartialEq)]
pub struct ElementsResponse {
    pub roots: Vec<ElementInfo>,
    pub count: u32,
}
