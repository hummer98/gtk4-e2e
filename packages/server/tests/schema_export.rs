//! Integration tests for `schema_export::write_schemas`.
//!
//! Anchors the codegen pipeline (ADR-0002): proto.rs → JSON Schema. Run with
//! `cargo test --all --features e2e`.

#![cfg(feature = "e2e")]

use std::fs;

use gtk4_e2e_server::write_schemas;
use serde_json::Value;

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
fn schema_carries_provenance_comment() {
    let tmp = tempfile::tempdir().unwrap();
    write_schemas(tmp.path()).unwrap();

    for name in ["Info.schema.json", "Capability.schema.json"] {
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

    for name in ["Info.schema.json", "Capability.schema.json"] {
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
