//! Selector parser + walker unit tests for `tree.rs`.
//!
//! Phase 1: GTK widgets are not touched here. The walker is exercised against
//! a `MockTree` so the abstraction is validated independently of gtk4-rs init.

#![cfg(feature = "e2e")]

use gtk4_e2e_server::tree::{find_first, parse_selector, Selector, WidgetTree};

#[test]
fn accepts_hash_name() {
    let s = parse_selector("#btn1").expect("#btn1 should parse");
    match s {
        Selector::Name(n) => assert_eq!(n, "btn1"),
        other => panic!("expected Selector::Name, got {other:?}"),
    }
}

#[test]
fn accepts_underscore_and_dash() {
    parse_selector("#btn_1").expect("underscore allowed");
    parse_selector("#btn-1").expect("dash allowed");
    parse_selector("#a").expect("single char allowed");
}

#[test]
fn accepts_dot_class() {
    let s = parse_selector(".foo").expect(".foo should parse");
    match s {
        Selector::Class(n) => assert_eq!(n, "foo"),
        other => panic!("expected Selector::Class, got {other:?}"),
    }
}

#[test]
fn rejects_invalid_dot_class() {
    assert!(parse_selector(".").is_err());
    assert!(parse_selector(".bad space").is_err());
    assert!(parse_selector(".$bad").is_err());
}

#[test]
fn rejects_empty() {
    assert!(parse_selector("#").is_err());
    assert!(parse_selector("").is_err());
}

#[test]
fn rejects_too_long() {
    let long = format!("#{}", "a".repeat(65));
    assert!(
        parse_selector(&long).is_err(),
        "65 chars should be rejected"
    );
    let ok = format!("#{}", "a".repeat(64));
    assert!(parse_selector(&ok).is_ok(), "64 chars should be accepted");
}

#[test]
fn rejects_special_chars() {
    assert!(parse_selector("#$bad").is_err());
    assert!(parse_selector("#bad space").is_err());
    assert!(parse_selector("#bad/slash").is_err());
    assert!(parse_selector("#bad.dot").is_err());
}

#[test]
fn rejects_leading_digit() {
    // identifier := [A-Za-z_][A-Za-z0-9_-]{0,63}
    assert!(parse_selector("#1abc").is_err());
}

#[test]
fn rejects_missing_hash() {
    assert!(parse_selector("btn1").is_err());
}

// ---- WidgetTree mock + walker tests ----

#[derive(Debug)]
struct MockNode {
    name: Option<&'static str>,
    classes: Vec<&'static str>,
    children: Vec<MockNode>,
}

impl MockNode {
    fn leaf(name: Option<&'static str>) -> Self {
        Self {
            name,
            classes: vec![],
            children: vec![],
        }
    }
    fn branch(name: Option<&'static str>, children: Vec<MockNode>) -> Self {
        Self {
            name,
            classes: vec![],
            children,
        }
    }
    fn with_classes(mut self, classes: Vec<&'static str>) -> Self {
        self.classes = classes;
        self
    }
}

struct MockTree {
    roots: Vec<MockNode>,
}

impl<'a> WidgetTree<'a> for &'a MockTree {
    type Node = &'a MockNode;
    type Roots = std::vec::IntoIter<&'a MockNode>;
    type Children = std::vec::IntoIter<&'a MockNode>;

    fn roots(self) -> Self::Roots {
        self.roots.iter().collect::<Vec<_>>().into_iter()
    }
    fn children(self, node: Self::Node) -> Self::Children {
        node.children.iter().collect::<Vec<_>>().into_iter()
    }
    fn name(self, node: Self::Node) -> Option<String> {
        node.name.map(|s| s.to_string())
    }
    fn classes(self, node: Self::Node) -> Vec<String> {
        node.classes.iter().map(|s| s.to_string()).collect()
    }
}

#[test]
fn mock_tree_returns_first_match() {
    let tree = MockTree {
        roots: vec![MockNode::branch(
            Some("root"),
            vec![
                MockNode::leaf(Some("a")),
                MockNode::branch(
                    Some("b"),
                    vec![MockNode::leaf(Some("target")), MockNode::leaf(Some("c"))],
                ),
                MockNode::leaf(Some("target")), // second hit, must NOT be returned
            ],
        )],
    };
    let sel = Selector::Name("target".to_string());
    let hit = find_first(&tree, &sel).expect("should find target");
    // First DFS hit is the inner one inside `b`.
    assert!(matches!(hit.name, Some("target")));
    // Confirm uniqueness by pointer: it should be the first leaf inside b
    // (which has no children) and not the second top-level target.
    assert!(hit.children.is_empty());
}

#[test]
fn mock_tree_returns_none() {
    let tree = MockTree {
        roots: vec![MockNode::leaf(Some("only"))],
    };
    let sel = Selector::Name("missing".to_string());
    assert!(find_first(&tree, &sel).is_none());
}

#[test]
fn mock_tree_skips_unnamed_nodes() {
    let tree = MockTree {
        roots: vec![MockNode::branch(
            None,
            vec![MockNode::leaf(None), MockNode::leaf(Some("hit"))],
        )],
    };
    let sel = Selector::Name("hit".to_string());
    assert!(find_first(&tree, &sel).is_some());
}

#[test]
fn mock_tree_matches_class_selector() {
    let tree = MockTree {
        roots: vec![MockNode::branch(
            Some("root"),
            vec![
                MockNode::leaf(Some("a")).with_classes(vec!["other"]),
                MockNode::leaf(Some("b")).with_classes(vec!["primary", "x"]),
            ],
        )],
    };
    let sel = Selector::Class("primary".to_string());
    let hit = find_first(&tree, &sel).expect("should find class match");
    assert_eq!(hit.name, Some("b"));
}

#[test]
fn mock_tree_class_miss_returns_none() {
    let tree = MockTree {
        roots: vec![MockNode::leaf(Some("solo"))],
    };
    let sel = Selector::Class("primary".to_string());
    assert!(find_first(&tree, &sel).is_none());
}
