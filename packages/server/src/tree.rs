//! Selector parser + widget tree walker abstraction.
//!
//! Plan §Q1/§Q2: MVP supports only `#name` (widget_name exact match,
//! `[A-Za-z_][A-Za-z0-9_-]{0,63}`). The walker is generic over a `WidgetTree`
//! trait so non-gtk fixtures can drive it in unit tests.
//!
//! GTK-bound impl lives in `tree::gtk`.

use std::fmt;

/// Parsed selector value.
///
/// Step 14 introduces `Class(_)` (`.css_class`) alongside `Name(_)` (`#name`).
/// Hierarchical / pseudo-class chains remain out of scope and would slot in
/// here as new variants when needed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selector {
    Name(String),
    Class(String),
}

/// Selector parse error returned to HTTP handlers as `422 invalid_selector`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidSelector {
    pub input: String,
    pub reason: &'static str,
}

impl fmt::Display for InvalidSelector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid selector {:?}: {}", self.input, self.reason)
    }
}

impl std::error::Error for InvalidSelector {}

const MAX_NAME_LEN: usize = 64;

/// Parse a selector string.
///
/// Grammar (plan §Q1, extended in Step 14):
///
/// ```text
/// selector   := ("#" | ".") identifier
/// identifier := [A-Za-z_][A-Za-z0-9_-]{0,63}
/// ```
///
/// `#name` is `widget_name` exact match; `.class` is `widget.css_classes()`
/// membership match.
pub fn parse_selector(input: &str) -> Result<Selector, InvalidSelector> {
    let (kind, rest) = if let Some(rest) = input.strip_prefix('#') {
        (SelectorKind::Name, rest)
    } else if let Some(rest) = input.strip_prefix('.') {
        (SelectorKind::Class, rest)
    } else {
        return Err(InvalidSelector {
            input: input.to_string(),
            reason: "selector must start with `#` or `.`",
        });
    };
    validate_identifier(input, rest)?;
    Ok(match kind {
        SelectorKind::Name => Selector::Name(rest.to_string()),
        SelectorKind::Class => Selector::Class(rest.to_string()),
    })
}

#[derive(Debug, Clone, Copy)]
enum SelectorKind {
    Name,
    Class,
}

fn validate_identifier(input: &str, rest: &str) -> Result<(), InvalidSelector> {
    if rest.is_empty() {
        return Err(InvalidSelector {
            input: input.to_string(),
            reason: "name must not be empty",
        });
    }
    if rest.len() > MAX_NAME_LEN {
        return Err(InvalidSelector {
            input: input.to_string(),
            reason: "name exceeds 64 characters",
        });
    }
    let mut chars = rest.chars();
    let first = chars.next().expect("non-empty checked above");
    if !(first.is_ascii_alphabetic() || first == '_') {
        return Err(InvalidSelector {
            input: input.to_string(),
            reason: "first char must be ASCII letter or underscore",
        });
    }
    for c in chars {
        if !(c.is_ascii_alphanumeric() || c == '_' || c == '-') {
            return Err(InvalidSelector {
                input: input.to_string(),
                reason: "name may contain only [A-Za-z0-9_-]",
            });
        }
    }
    Ok(())
}

/// Abstraction over a widget tree, decoupling the walker from gtk4-rs so it
/// can be unit-tested with mock fixtures.
///
/// The trait takes `self` (not `&self`) so callers can pass either a `&Tree`
/// (gtk impl) or a `&MockTree` (test impl) without lifetime juggling.
pub trait WidgetTree<'a>: Copy {
    type Node: Clone + 'a;
    type Roots: Iterator<Item = Self::Node>;
    type Children: Iterator<Item = Self::Node>;

    fn roots(self) -> Self::Roots;
    fn children(self, node: Self::Node) -> Self::Children;
    fn name(self, node: Self::Node) -> Option<String>;

    /// CSS classes attached to `node`. Default returns an empty vector so
    /// pre-existing fixtures keep compiling; impls that want to support
    /// `.class` selectors override it.
    fn classes(self, _node: Self::Node) -> Vec<String> {
        Vec::new()
    }
}

/// Free helper — returns true iff `node` matches `sel`.
///
/// Lives at module scope (not on the trait) so call sites pattern-match
/// `Selector` once and the trait stays minimal.
pub fn selector_matches<'a, T: WidgetTree<'a>>(tree: T, node: T::Node, sel: &Selector) -> bool {
    match sel {
        Selector::Name(target) => tree.name(node).as_deref() == Some(target.as_str()),
        Selector::Class(target) => tree.classes(node).iter().any(|c| c == target),
    }
}

/// Find the first node whose attributes match `selector`, in DFS pre-order
/// (root → first child → its first child → ...).
pub fn find_first<'a, T: WidgetTree<'a>>(tree: T, selector: &Selector) -> Option<T::Node> {
    for r in tree.roots() {
        if let Some(hit) = walk(tree, r, selector) {
            return Some(hit);
        }
    }
    None
}

fn walk<'a, T: WidgetTree<'a>>(tree: T, node: T::Node, sel: &Selector) -> Option<T::Node> {
    if selector_matches(tree, node.clone(), sel) {
        return Some(node);
    }
    for c in tree.children(node.clone()) {
        if let Some(hit) = walk(tree, c, sel) {
            return Some(hit);
        }
    }
    None
}

// ------------------------------------------------------------
// GTK-bound implementation
// ------------------------------------------------------------

/// GTK widget tree adapter, scoped to a single `gtk::Application`.
pub struct GtkTree<'a> {
    pub app: &'a crate::gtk::Application,
}

impl<'a> Clone for GtkTree<'a> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<'a> Copy for GtkTree<'a> {}

impl<'a> WidgetTree<'a> for GtkTree<'a> {
    type Node = crate::gtk::Widget;
    type Roots = std::vec::IntoIter<crate::gtk::Widget>;
    type Children = std::vec::IntoIter<crate::gtk::Widget>;

    fn roots(self) -> Self::Roots {
        use crate::gtk::prelude::*;
        let windows = self.app.windows();
        let mut out: Vec<crate::gtk::Widget> = Vec::with_capacity(windows.len());
        for w in windows {
            // The window itself participates in name matching. Children are
            // descended via `children()`.
            out.push(w.upcast::<crate::gtk::Widget>());
        }
        out.into_iter()
    }

    fn children(self, node: Self::Node) -> Self::Children {
        use crate::gtk::prelude::*;
        let mut out: Vec<crate::gtk::Widget> = Vec::new();
        let mut cur = node.first_child();
        while let Some(child) = cur {
            cur = child.next_sibling();
            out.push(child);
        }
        out.into_iter()
    }

    fn name(self, node: Self::Node) -> Option<String> {
        use crate::gtk::prelude::*;
        let n = node.widget_name();
        if n.is_empty() {
            None
        } else {
            Some(n.to_string())
        }
    }

    fn classes(self, node: Self::Node) -> Vec<String> {
        use crate::gtk::prelude::*;
        node.css_classes()
            .into_iter()
            .map(|s| s.to_string())
            .collect()
    }
}
