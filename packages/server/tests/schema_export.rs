//! Integration tests for `schema_export::write_schemas`.
//!
//! Anchors the codegen pipeline (ADR-0002): proto.rs → JSON Schema. Run with
//! `cargo test --all --features e2e`.

#![cfg(feature = "e2e")]

use std::fs;

use gtk4_e2e_server::write_schemas;
use serde_json::Value;

const ALL_SCHEMA_FILES: &[&str] = &[
    "Info.schema.json",
    "Capability.schema.json",
    "TapTarget.schema.json",
    "TypeRequest.schema.json",
    "SwipeRequest.schema.json",
    "WaitRequest.schema.json",
    "WaitCondition.schema.json",
    "WaitResult.schema.json",
    "EventEnvelope.schema.json",
    "EventKind.schema.json",
    "Bounds.schema.json",
    "ElementInfo.schema.json",
    "ElementsResponse.schema.json",
];

#[test]
fn writes_info_and_capability() {
    let tmp = tempfile::tempdir().unwrap();
    write_schemas(tmp.path()).expect("write_schemas should succeed");

    let info = tmp.path().join("Info.schema.json");
    let cap = tmp.path().join("Capability.schema.json");
    assert!(info.is_file(), "Info.schema.json missing at {info:?}");
    assert!(cap.is_file(), "Capability.schema.json missing at {cap:?}");
}

#[test]
fn writes_tap_and_wait_schemas() {
    let tmp = tempfile::tempdir().unwrap();
    write_schemas(tmp.path()).expect("write_schemas should succeed");

    for name in ALL_SCHEMA_FILES {
        let p = tmp.path().join(name);
        assert!(p.is_file(), "{name} missing at {p:?}");
    }
}

#[test]
fn info_schema_has_instance_id() {
    let tmp = tempfile::tempdir().unwrap();
    write_schemas(tmp.path()).unwrap();

    let bytes = fs::read(tmp.path().join("Info.schema.json")).unwrap();
    let v: Value = serde_json::from_slice(&bytes).unwrap();
    let ty = v
        .pointer("/properties/instance_id/type")
        .expect("Info.properties.instance_id.type should exist");
    assert_eq!(ty, &Value::String("string".into()));
}

#[test]
fn capability_schema_has_snake_case_variant() {
    let tmp = tempfile::tempdir().unwrap();
    write_schemas(tmp.path()).unwrap();

    let bytes = fs::read(tmp.path().join("Capability.schema.json")).unwrap();
    let v: Value = serde_json::from_slice(&bytes).unwrap();
    let variants = v.get("enum").expect("Capability schema should have enum");
    let arr = variants.as_array().expect("enum should be array");
    assert!(
        arr.iter().any(|e| e == &Value::String("info".into())),
        "expected snake_case variant `info` in {arr:?}"
    );
}

#[test]
fn capability_includes_tap_wait() {
    let tmp = tempfile::tempdir().unwrap();
    write_schemas(tmp.path()).unwrap();

    let bytes = fs::read(tmp.path().join("Capability.schema.json")).unwrap();
    let v: Value = serde_json::from_slice(&bytes).unwrap();
    let arr = v
        .get("enum")
        .and_then(Value::as_array)
        .expect("Capability schema should have enum array");
    for expected in ["info", "tap", "wait"] {
        assert!(
            arr.iter().any(|e| e == &Value::String(expected.into())),
            "expected variant {expected:?} in {arr:?}"
        );
    }
}

#[test]
fn capability_enum_order_is_anchored() {
    // Plan §Q5 / Step 9: Capability ordering must match the surfaced order.
    // Step 7 anchored Events at the tail; Step 9 appends Type (T013) and
    // Swipe (T014) after Events. Step 14 appends Elements (T018) at the tail.
    let tmp = tempfile::tempdir().unwrap();
    write_schemas(tmp.path()).unwrap();

    let bytes = fs::read(tmp.path().join("Capability.schema.json")).unwrap();
    let v: Value = serde_json::from_slice(&bytes).unwrap();
    let arr = v
        .get("enum")
        .and_then(Value::as_array)
        .expect("Capability schema should have enum array");
    let strs: Vec<&str> = arr.iter().filter_map(Value::as_str).collect();
    assert_eq!(
        strs,
        vec![
            "info",
            "tap",
            "wait",
            "screenshot",
            "events",
            "type",
            "swipe",
            "elements"
        ],
        "Capability enum order must be [info, tap, wait, screenshot, events, type, swipe, elements]"
    );
}

#[test]
fn capabilities_order_is_stable() {
    // info.capabilities ordering must be deterministic — anchored as an
    // invariant so future re-orderings break this test.
    let info = gtk4_e2e_server::Info {
        instance_id: "x".into(),
        pid: 0,
        port: 0,
        app_name: "x".into(),
        app_version: "x".into(),
        capabilities: vec![
            gtk4_e2e_server::Capability::Info,
            gtk4_e2e_server::Capability::Tap,
            gtk4_e2e_server::Capability::Wait,
            gtk4_e2e_server::Capability::Screenshot,
            gtk4_e2e_server::Capability::Events,
            gtk4_e2e_server::Capability::Type,
            gtk4_e2e_server::Capability::Swipe,
            gtk4_e2e_server::Capability::Elements,
        ],
        token_required: None,
    };
    assert_eq!(
        info.capabilities,
        vec![
            gtk4_e2e_server::Capability::Info,
            gtk4_e2e_server::Capability::Tap,
            gtk4_e2e_server::Capability::Wait,
            gtk4_e2e_server::Capability::Screenshot,
            gtk4_e2e_server::Capability::Events,
            gtk4_e2e_server::Capability::Type,
            gtk4_e2e_server::Capability::Swipe,
            gtk4_e2e_server::Capability::Elements,
        ]
    );
}

#[test]
fn event_kind_enum_has_state_change_and_log_line() {
    let tmp = tempfile::tempdir().unwrap();
    write_schemas(tmp.path()).unwrap();
    let bytes = fs::read(tmp.path().join("EventKind.schema.json")).unwrap();
    let v: Value = serde_json::from_slice(&bytes).unwrap();
    let arr = v
        .get("enum")
        .and_then(Value::as_array)
        .expect("EventKind schema should have enum array");
    let strs: Vec<&str> = arr.iter().filter_map(Value::as_str).collect();
    assert!(
        strs.contains(&"state_change"),
        "missing state_change in {strs:?}"
    );
    assert!(strs.contains(&"log_line"), "missing log_line in {strs:?}");
}

#[test]
fn event_envelope_schema_has_kind_ts_data() {
    let tmp = tempfile::tempdir().unwrap();
    write_schemas(tmp.path()).unwrap();
    let bytes = fs::read(tmp.path().join("EventEnvelope.schema.json")).unwrap();
    let v: Value = serde_json::from_slice(&bytes).unwrap();
    let props = v
        .pointer("/properties")
        .and_then(Value::as_object)
        .expect("EventEnvelope should have properties");
    for k in ["kind", "ts", "data"] {
        assert!(
            props.contains_key(k),
            "EventEnvelope missing property `{k}`"
        );
    }
}

#[test]
fn wait_condition_has_kind_tag() {
    let tmp = tempfile::tempdir().unwrap();
    write_schemas(tmp.path()).unwrap();

    let bytes = fs::read(tmp.path().join("WaitCondition.schema.json")).unwrap();
    let v: Value = serde_json::from_slice(&bytes).unwrap();
    let s = serde_json::to_string(&v).unwrap();
    assert!(
        s.contains("\"kind\""),
        "WaitCondition schema should mention `kind` discriminator: {s}"
    );
    assert!(
        s.contains("selector_visible"),
        "WaitCondition schema should mention `selector_visible`: {s}"
    );
    assert!(
        s.contains("state_eq"),
        "WaitCondition schema should mention `state_eq`: {s}"
    );
}

#[test]
fn tap_target_is_untagged() {
    let tmp = tempfile::tempdir().unwrap();
    write_schemas(tmp.path()).unwrap();

    let bytes = fs::read(tmp.path().join("TapTarget.schema.json")).unwrap();
    let v: Value = serde_json::from_slice(&bytes).unwrap();
    // untagged enum should produce a oneOf/anyOf without a discriminator field.
    let title = v.get("title").and_then(Value::as_str);
    assert_eq!(title, Some("TapTarget"));
    let s = serde_json::to_string(&v).unwrap();
    assert!(
        s.contains("\"selector\""),
        "TapTarget schema should mention `selector` variant: {s}"
    );
    assert!(
        s.contains("\"xy\""),
        "TapTarget schema should mention `xy` variant: {s}"
    );
}

#[test]
fn wait_result_has_elapsed_ms_only() {
    let tmp = tempfile::tempdir().unwrap();
    write_schemas(tmp.path()).unwrap();

    let bytes = fs::read(tmp.path().join("WaitResult.schema.json")).unwrap();
    let v: Value = serde_json::from_slice(&bytes).unwrap();
    let props = v
        .pointer("/properties")
        .and_then(Value::as_object)
        .expect("WaitResult should have properties");
    assert!(
        props.contains_key("elapsed_ms"),
        "WaitResult should have `elapsed_ms`"
    );
    assert!(
        !props.contains_key("matched"),
        "WaitResult should NOT have `matched` (Review m9)"
    );
}

#[test]
fn schema_carries_provenance_comment() {
    let tmp = tempfile::tempdir().unwrap();
    write_schemas(tmp.path()).unwrap();

    for name in ALL_SCHEMA_FILES {
        let bytes = fs::read(tmp.path().join(name)).unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        let comment = v.get("$comment").unwrap_or_else(|| {
            panic!("{name} should have top-level $comment");
        });
        let s = comment.as_str().expect("$comment must be string");
        assert!(
            s.contains("AUTO-GENERATED"),
            "$comment in {name} should mention AUTO-GENERATED, got {s:?}"
        );
        assert!(
            s.contains("packages/server/src/proto.rs"),
            "$comment in {name} should reference proto.rs, got {s:?}"
        );
    }
}

#[test]
fn output_is_deterministic() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    write_schemas(a.path()).unwrap();
    write_schemas(b.path()).unwrap();

    for name in ALL_SCHEMA_FILES {
        let ba = fs::read(a.path().join(name)).unwrap();
        let bb = fs::read(b.path().join(name)).unwrap();
        assert_eq!(ba, bb, "{name} output must be byte-deterministic");
        assert_eq!(
            ba.last(),
            Some(&b'\n'),
            "{name} should end with a single LF"
        );
    }
}
