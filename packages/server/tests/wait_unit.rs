//! Unit tests for the pure parts of `wait::*`:
//!
//! * `eval_condition` against a mock tree
//! * `poll_until` driving a mock `mpsc::Receiver` consumer
//!
//! No GTK init required. The GTK-bound entry points (`eval_condition_in_app`,
//! `dispatch_tap`) are exercised via the HTTP route integration tests.

#![cfg(feature = "e2e")]

use std::cell::RefCell;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::json;

use gtk4_e2e_server::main_thread::{MainCmd, WaitEvalError, WaitTickOutcome, WaitTickResult};
use gtk4_e2e_server::proto::{WaitCondition, WaitResult};
use gtk4_e2e_server::state::AppDefinedState;
use gtk4_e2e_server::tree::WidgetTree;
use gtk4_e2e_server::wait::{
    eval_app_state_eq, eval_condition, poll_until, PropReadError, WaitError, WidgetLike,
};

// ---------------------------------------------------------------
// Mock tree
// ---------------------------------------------------------------

#[derive(Debug, Clone)]
struct MockNode {
    name: Option<String>,
    visible: bool,
    mapped: bool,
    props: Vec<(String, serde_json::Value)>,
    children: Vec<MockNode>,
}

impl MockNode {
    fn named(name: &str) -> Self {
        Self {
            name: Some(name.into()),
            visible: true,
            mapped: true,
            props: vec![],
            children: vec![],
        }
    }
    fn with_prop(mut self, k: &str, v: serde_json::Value) -> Self {
        self.props.push((k.into(), v));
        self
    }
    fn invisible(mut self) -> Self {
        self.visible = false;
        self
    }
}

impl WidgetLike for &MockNode {
    fn is_visible_and_mapped(&self) -> bool {
        self.visible && self.mapped
    }
    fn read_property_as_json(&self, property: &str) -> Result<serde_json::Value, PropReadError> {
        if property == "unsupported_type" {
            return Err(PropReadError::Unsupported("ColorRGBA".into()));
        }
        for (k, v) in &self.props {
            if k == property {
                return Ok(v.clone());
            }
        }
        Err(PropReadError::Missing)
    }
}

#[derive(Clone, Copy)]
struct MockTreeRef<'a>(&'a Vec<MockNode>);

impl<'a> WidgetTree<'a> for MockTreeRef<'a> {
    type Node = &'a MockNode;
    type Roots = std::vec::IntoIter<&'a MockNode>;
    type Children = std::vec::IntoIter<&'a MockNode>;
    fn roots(self) -> Self::Roots {
        self.0.iter().collect::<Vec<_>>().into_iter()
    }
    fn children(self, node: Self::Node) -> Self::Children {
        node.children.iter().collect::<Vec<_>>().into_iter()
    }
    fn name(self, node: Self::Node) -> Option<String> {
        node.name.clone()
    }
}

// ---------------------------------------------------------------
// eval_condition
// ---------------------------------------------------------------

#[test]
fn mock_visible_widget_matches() {
    let roots = vec![MockNode::named("btn1")];
    let tree = MockTreeRef(&roots);
    let cond = WaitCondition::SelectorVisible {
        selector: "#btn1".into(),
    };
    assert_eq!(
        eval_condition(tree, &cond),
        WaitTickResult::Outcome(WaitTickOutcome::Matched)
    );
}

#[test]
fn mock_invisible_widget_not_yet() {
    let roots = vec![MockNode::named("btn1").invisible()];
    let tree = MockTreeRef(&roots);
    let cond = WaitCondition::SelectorVisible {
        selector: "#btn1".into(),
    };
    assert_eq!(
        eval_condition(tree, &cond),
        WaitTickResult::Outcome(WaitTickOutcome::NotYet)
    );
}

#[test]
fn matches_after_widget_appears() {
    // First tick: empty tree → SelectorNotFound. Later tick: widget appears
    // and we want Matched. Both should be handled by the polling loop without
    // a permanent failure (Review C2).
    let empty: Vec<MockNode> = vec![];
    let cond = WaitCondition::SelectorVisible {
        selector: "#btn1".into(),
    };
    let r1 = eval_condition(MockTreeRef(&empty), &cond);
    assert_eq!(r1, WaitTickResult::SelectorNotFound);

    let later = vec![MockNode::named("btn1")];
    let r2 = eval_condition(MockTreeRef(&later), &cond);
    assert_eq!(r2, WaitTickResult::Outcome(WaitTickOutcome::Matched));
}

#[test]
fn label_property_matches() {
    let roots = vec![MockNode::named("label1").with_prop("label", json!("hello"))];
    let tree = MockTreeRef(&roots);
    let cond = WaitCondition::StateEq {
        selector: "#label1".into(),
        property: "label".into(),
        value: json!("hello"),
    };
    assert_eq!(
        eval_condition(tree, &cond),
        WaitTickResult::Outcome(WaitTickOutcome::Matched)
    );
}

#[test]
fn label_property_not_yet_when_value_differs() {
    let roots = vec![MockNode::named("label1").with_prop("label", json!("waiting"))];
    let tree = MockTreeRef(&roots);
    let cond = WaitCondition::StateEq {
        selector: "#label1".into(),
        property: "label".into(),
        value: json!("hello"),
    };
    assert_eq!(
        eval_condition(tree, &cond),
        WaitTickResult::Outcome(WaitTickOutcome::NotYet)
    );
}

#[test]
fn unsupported_property_type_errors() {
    let roots = vec![MockNode::named("label1")];
    let tree = MockTreeRef(&roots);
    let cond = WaitCondition::StateEq {
        selector: "#label1".into(),
        property: "unsupported_type".into(),
        value: json!(null),
    };
    match eval_condition(tree, &cond) {
        WaitTickResult::PermanentFailure(WaitEvalError::UnsupportedPropertyType(_)) => {}
        other => panic!("expected UnsupportedPropertyType, got {other:?}"),
    }
}

#[test]
fn invalid_selector_is_permanent_failure() {
    let roots: Vec<MockNode> = vec![];
    let tree = MockTreeRef(&roots);
    let cond = WaitCondition::SelectorVisible {
        selector: "@bad".into(),
    };
    match eval_condition(tree, &cond) {
        WaitTickResult::PermanentFailure(WaitEvalError::InvalidSelector(_)) => {}
        other => panic!("expected InvalidSelector, got {other:?}"),
    }
}

// ---------------------------------------------------------------
// poll_until — drive a stand-in `mpsc::Receiver` and verify the timing model.
// ---------------------------------------------------------------

fn spawn_responder(
    mut rx: tokio::sync::mpsc::Receiver<MainCmd>,
    script: Arc<Mutex<Vec<WaitTickResult>>>,
    log: Arc<Mutex<u32>>,
) {
    tokio::spawn(async move {
        while let Some(cmd) = rx.recv().await {
            match cmd {
                MainCmd::EvalWait { reply, .. } => {
                    let mut guard = script.lock().unwrap();
                    let next = if guard.is_empty() {
                        WaitTickResult::Outcome(WaitTickOutcome::NotYet)
                    } else {
                        guard.remove(0)
                    };
                    drop(guard);
                    *log.lock().unwrap() += 1;
                    let _ = reply.send(next);
                }
                _ => unreachable!("only EvalWait expected in these tests"),
            }
        }
    });
}

#[tokio::test(flavor = "current_thread")]
async fn matches_within_timeout() {
    let (tx, rx) = tokio::sync::mpsc::channel::<MainCmd>(8);
    let script = Arc::new(Mutex::new(vec![
        WaitTickResult::Outcome(WaitTickOutcome::NotYet),
        WaitTickResult::Outcome(WaitTickOutcome::Matched),
    ]));
    let log = Arc::new(Mutex::new(0u32));
    spawn_responder(rx, script.clone(), log.clone());

    let result: Result<WaitResult, WaitError> = poll_until(
        &tx,
        AppDefinedState::default(),
        WaitCondition::SelectorVisible {
            selector: "#btn1".into(),
        },
        2_000,
    )
    .await;

    let r = result.expect("should match within timeout");
    assert!(r.elapsed_ms <= 2_000);
    assert!(*log.lock().unwrap() >= 2, "expected at least two ticks");
}

#[tokio::test(flavor = "current_thread")]
async fn times_out() {
    let (tx, rx) = tokio::sync::mpsc::channel::<MainCmd>(8);
    let script = Arc::new(Mutex::new(vec![])); // always NotYet
    let log = Arc::new(Mutex::new(0u32));
    spawn_responder(rx, script, log);

    let err = poll_until(
        &tx,
        AppDefinedState::default(),
        WaitCondition::SelectorVisible {
            selector: "#btn1".into(),
        },
        300,
    )
    .await
    .expect_err("should time out");
    assert!(matches!(err, WaitError::Timeout));
}

#[tokio::test(flavor = "current_thread")]
async fn selector_not_found_treated_as_tick_failure() {
    let (tx, rx) = tokio::sync::mpsc::channel::<MainCmd>(8);
    // First two ticks return SelectorNotFound, then Matched.
    let script = Arc::new(Mutex::new(vec![
        WaitTickResult::SelectorNotFound,
        WaitTickResult::SelectorNotFound,
        WaitTickResult::Outcome(WaitTickOutcome::Matched),
    ]));
    let log = Arc::new(Mutex::new(0u32));
    spawn_responder(rx, script, log.clone());

    let r = poll_until(
        &tx,
        AppDefinedState::default(),
        WaitCondition::SelectorVisible {
            selector: "#btn1".into(),
        },
        2_000,
    )
    .await
    .expect("should eventually match");
    assert!(r.elapsed_ms <= 2_000);
    assert!(*log.lock().unwrap() >= 3);
}

#[tokio::test(flavor = "current_thread")]
async fn rejects_zero_timeout() {
    let (tx, _rx) = tokio::sync::mpsc::channel::<MainCmd>(1);
    let err = poll_until(
        &tx,
        AppDefinedState::default(),
        WaitCondition::SelectorVisible {
            selector: "#btn1".into(),
        },
        0,
    )
    .await
    .expect_err("zero timeout should be rejected");
    assert!(matches!(err, WaitError::InvalidTimeout("zero")));
}

#[tokio::test(flavor = "current_thread")]
async fn rejects_excessive_timeout() {
    let (tx, _rx) = tokio::sync::mpsc::channel::<MainCmd>(1);
    let err = poll_until(
        &tx,
        AppDefinedState::default(),
        WaitCondition::SelectorVisible {
            selector: "#btn1".into(),
        },
        600_001,
    )
    .await
    .expect_err("excessive timeout should be rejected");
    assert!(matches!(err, WaitError::InvalidTimeout("excessive")));
}

#[tokio::test(flavor = "current_thread")]
async fn permanent_failure_propagates_immediately() {
    let (tx, rx) = tokio::sync::mpsc::channel::<MainCmd>(8);
    let script = Arc::new(Mutex::new(vec![WaitTickResult::PermanentFailure(
        WaitEvalError::UnsupportedPropertyType("ColorRGBA".into()),
    )]));
    let log = Arc::new(Mutex::new(0u32));
    spawn_responder(rx, script, log);

    let err = poll_until(
        &tx,
        AppDefinedState::default(),
        WaitCondition::StateEq {
            selector: "#label1".into(),
            property: "color".into(),
            value: json!(null),
        },
        2_000,
    )
    .await
    .expect_err("permanent failure should bubble up");
    match err {
        WaitError::Eval(WaitEvalError::UnsupportedPropertyType(_)) => {}
        other => panic!("expected UnsupportedPropertyType, got {other:?}"),
    }
}

// ---------------------------------------------------------------
// T019: app-defined state — eval_app_state_eq + poll_until via AppStateEq.
// ---------------------------------------------------------------

#[test]
fn app_state_eq_root_pointer_matches() {
    // §3.A.4 case (1): empty path resolves to the entire snapshot.
    let state = AppDefinedState::default();
    state.set(json!({"hello": "world"}));
    let r = eval_app_state_eq(&state, "", &json!({"hello": "world"}));
    assert_eq!(r, WaitTickResult::Outcome(WaitTickOutcome::Matched));
}

#[test]
fn app_state_eq_matches_when_path_resolves() {
    // §3.A.4 補強: deep pointer match.
    let state = AppDefinedState::default();
    state.set(json!({"session": {"mode": "applied"}}));
    let r = eval_app_state_eq(&state, "/session/mode", &json!("applied"));
    assert_eq!(r, WaitTickResult::Outcome(WaitTickOutcome::Matched));
}

#[test]
fn app_state_eq_not_yet_when_value_differs() {
    // §3.A.4 case (2): path resolves but the value differs.
    let state = AppDefinedState::default();
    state.set(json!({"session": {"mode": "applied"}}));
    let r = eval_app_state_eq(&state, "/session/mode", &json!("pending"));
    assert_eq!(r, WaitTickResult::Outcome(WaitTickOutcome::NotYet));
}

#[test]
fn app_state_eq_not_yet_when_path_missing() {
    // §3.A.4 case (3): path does not resolve. Treated as a tick failure
    // so transient app-side schema drift surfaces as 408 timeout.
    let state = AppDefinedState::default();
    state.set(json!({"session": {"mode": "applied"}}));
    let r = eval_app_state_eq(&state, "/no/such/key", &json!("anything"));
    assert_eq!(r, WaitTickResult::Outcome(WaitTickOutcome::NotYet));
}

#[tokio::test(flavor = "current_thread")]
async fn poll_until_app_state_eq_matches_after_set() {
    // poll_until short-circuits AppStateEq and reads `app_state` directly,
    // so the `MainCmd` channel is never touched. Verify a delayed `set`
    // still trips the matcher within the deadline.
    let (tx, _rx) = tokio::sync::mpsc::channel::<MainCmd>(1);
    let state = AppDefinedState::default();
    let state_clone = state.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(120)).await;
        state_clone.set(json!({"session": {"mode": "applied"}}));
    });
    let r = poll_until(
        &tx,
        state,
        WaitCondition::AppStateEq {
            path: "/session/mode".into(),
            value: json!("applied"),
        },
        2_000,
    )
    .await
    .expect("should match within timeout once state is set");
    assert!(r.elapsed_ms <= 2_000);
}

// silence unused warnings on Duration / RefCell when test attrs strip them
#[allow(dead_code)]
fn _ergonomics() {
    let _ = Duration::from_millis(1);
    let _ = RefCell::new(0);
}
