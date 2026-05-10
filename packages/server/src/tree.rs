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
/// Only `Name(_)` is wired up for MVP. Future variants (e.g. `Class(_)`,
/// hierarchical chains) can be added without breaking call sites that pattern
/// match exhaustively — they will simply gain new arms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selector {
    Name(String),
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
/// Grammar (plan §Q1):
///
/// ```text
/// selector   := "#" identifier
/// identifier := [A-Za-z_][A-Za-z0-9_-]{0,63}
/// ```
pub fn parse_selector(input: &str) -> Result<Selector, InvalidSelector> {
    let rest = input.strip_prefix('#').ok_or(InvalidSelector {
        input: input.to_string(),
        reason: "selector must start with `#`",
    })?;
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
    Ok(Selector::Name(rest.to_string()))
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
}

/// Find the first node whose `widget_name` matches `selector`, in DFS order
/// (root → first child → its first child → ... pre-order).
pub fn find_first<'a, T: WidgetTree<'a>>(tree: T, selector: &Selector) -> Option<T::Node> {
    let target = match selector {
        Selector::Name(n) => n.as_str(),
    };
    for r in tree.roots() {
        if let Some(hit) = walk(tree, r, target) {
            return Some(hit);
        }
    }
    None
}

fn walk<'a, T: WidgetTree<'a>>(tree: T, node: T::Node, target: &str) -> Option<T::Node> {
    if let Some(name) = tree.name(node.clone()) {
        if name == target {
            return Some(node);
        }
    }
    for c in tree.children(node.clone()) {
        if let Some(hit) = walk(tree, c, target) {
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
}
