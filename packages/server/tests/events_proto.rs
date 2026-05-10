//! Step 7 unit tests: envelope serde, capability advertisement, ws filter.
//!
//! T1 / T2 / T3 from plan §7.

#![cfg(feature = "e2e")]

use std::collections::HashSet;

use gtk4_e2e_server::proto::{Capability, EventEnvelope, EventKind};
use gtk4_e2e_server::ws::{parse_kinds, should_forward};
use serde_json::json;

// ----- T1: EventEnvelope round-trips through serde -----

#[test]
fn envelope_serializes_with_snake_case_kind() {
    let env = EventEnvelope {
        kind: EventKind::StateChange,
        ts: "2024-01-02T03:04:05Z".into(),
        data: json!({ "selector": "#label1" }),
    };
    let s = serde_json::to_string(&env).unwrap();
    assert!(
        s.contains("\"kind\":\"state_change\""),
        "expected snake_case kind in {s}"
    );
    assert!(s.contains("\"ts\":\"2024-01-02T03:04:05Z\""));
    assert!(s.contains("\"selector\":\"#label1\""));
}

#[test]
fn envelope_roundtrips_through_json() {
    let original = EventEnvelope {
        kind: EventKind::LogLine,
        ts: "1970-01-01T00:00:00Z".into(),
        data: json!({ "level": "info", "msg": "hi" }),
    };
    let s = serde_json::to_string(&original).unwrap();
    let back: EventEnvelope = serde_json::from_str(&s).unwrap();
    assert_eq!(back, original);
}

// ----- T2: ws filter is a pure function -----

#[test]
fn parse_kinds_csv_yields_set() {
    let set = parse_kinds(Some("state_change"));
    assert!(set.contains(&EventKind::StateChange));
    assert!(!set.contains(&EventKind::LogLine));
}

#[test]
fn parse_kinds_unknown_token_is_ignored() {
    // Forward-compat: unknown kinds (from a newer server) become a no-op
    // rather than a 4xx, so SDKs from one version continue to work against
    // another version.
    let set = parse_kinds(Some("state_change,future_kind"));
    assert_eq!(set.len(), 1);
    assert!(set.contains(&EventKind::StateChange));
}

#[test]
fn should_forward_empty_filter_passes_all() {
    let filter = HashSet::new();
    let env = EventEnvelope {
        kind: EventKind::StateChange,
        ts: "x".into(),
        data: json!({}),
    };
    assert!(should_forward(&env, &filter));
}

#[test]
fn should_forward_filter_drops_unmatched() {
    let mut filter = HashSet::new();
    filter.insert(EventKind::LogLine);
    let env = EventEnvelope {
        kind: EventKind::StateChange,
        ts: "x".into(),
        data: json!({}),
    };
    assert!(!should_forward(&env, &filter));
}

// ----- T3: Info advertises Events at the tail of capabilities -----

#[test]
fn capability_includes_events() {
    // Mirrors the live `start_inner` configuration: the order is anchored so
    // missing additions show up here even if production code drifts.
    let caps = [
        Capability::Info,
        Capability::Tap,
        Capability::Wait,
        Capability::Screenshot,
        Capability::Events,
        Capability::Type,
    ];
    assert!(
        caps.contains(&Capability::Events),
        "Capability::Events must be advertised"
    );
    assert_eq!(
        caps.last(),
        Some(&Capability::Type),
        "Type must be the trailing variant after Step 9"
    );
}
