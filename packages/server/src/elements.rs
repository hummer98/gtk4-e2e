//! Widget tree query — backs `GET /test/elements` (Step 14).
//!
//! `walk_elements` is the GTK-bound entry point invoked from the GLib main
//! thread via `MainCmd::Elements`. With no selector it dumps every active
//! window's subtree. With a selector it runs a DFS pre-order match across
//! the tree and returns one `ElementInfo` per matching widget; nested matches
//! inside an outer match are not duplicated as separate roots (§5.2 P-2).

use crate::gtk;
use crate::gtk::prelude::*;
use crate::proto::{Bounds, ElementInfo, ElementsResponse};
use crate::tree::{parse_selector, Selector};

/// Domain errors surfaced by `walk_elements`.
///
/// Mapped to HTTP status codes in `http.rs::elements_error_response` — both
/// variants emit 422.
#[derive(Debug, Clone, PartialEq)]
pub enum ElementsError {
    InvalidSelector { reason: String },
    NoActiveWindow,
}

impl std::fmt::Display for ElementsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ElementsError::InvalidSelector { reason } => {
                write!(f, "invalid_selector: {reason}")
            }
            ElementsError::NoActiveWindow => write!(f, "no_active_window"),
        }
    }
}

impl std::error::Error for ElementsError {}

/// Walk `app.windows()` and assemble an `ElementsResponse`.
///
/// - selector `None`: one root per active window (typically 1).
/// - selector `Some(_)`: DFS pre-order over the tree; each match becomes a
///   root. Once a match is recorded its descendants are not re-scanned for
///   nested matches.
/// - `max_depth` caps subtree depth per root (`0` = root only, `None` =
///   unlimited).
pub fn walk_elements(
    app: &gtk::Application,
    selector: Option<&str>,
    max_depth: Option<u32>,
) -> Result<ElementsResponse, ElementsError> {
    let windows = app.windows();
    if windows.is_empty() {
        return Err(ElementsError::NoActiveWindow);
    }

    let parsed = match selector {
        Some(s) => Some(parse_selector(s).map_err(|e| ElementsError::InvalidSelector {
            reason: e.reason.to_string(),
        })?),
        None => None,
    };

    let mut roots: Vec<ElementInfo> = Vec::new();
    let mut counter: u32 = 0;

    for window in windows {
        let widget = window.upcast::<gtk::Widget>();
        match &parsed {
            None => {
                let info = to_element_info(&widget, &widget, 0, max_depth, &mut counter);
                roots.push(info);
            }
            Some(sel) => {
                collect_matches(&widget, &widget, sel, max_depth, &mut counter, &mut roots);
            }
        }
    }

    let count: u32 = roots.iter().map(node_count).sum();
    Ok(ElementsResponse { roots, count })
}

fn collect_matches(
    widget: &gtk::Widget,
    window_root: &gtk::Widget,
    sel: &Selector,
    max_depth: Option<u32>,
    counter: &mut u32,
    roots: &mut Vec<ElementInfo>,
) {
    if widget_matches(widget, sel) {
        let info = to_element_info(widget, window_root, 0, max_depth, counter);
        roots.push(info);
        return;
    }
    let mut cur = widget.first_child();
    while let Some(child) = cur {
        let next = child.next_sibling();
        collect_matches(&child, window_root, sel, max_depth, counter, roots);
        cur = next;
    }
}

fn widget_matches(widget: &gtk::Widget, sel: &Selector) -> bool {
    match sel {
        Selector::Name(target) => {
            let n = widget.widget_name();
            !n.is_empty() && n.as_str() == target.as_str()
        }
        Selector::Class(target) => widget
            .css_classes()
            .iter()
            .any(|c| c.as_str() == target.as_str()),
    }
}

fn to_element_info(
    widget: &gtk::Widget,
    window_root: &gtk::Widget,
    depth: u32,
    max_depth: Option<u32>,
    counter: &mut u32,
) -> ElementInfo {
    let id = format!("e{}", *counter);
    *counter += 1;

    let kind = widget.type_().name().to_string();
    let widget_name = {
        let n = widget.widget_name();
        if n.is_empty() {
            None
        } else {
            Some(n.to_string())
        }
    };
    let css_classes: Vec<String> = widget
        .css_classes()
        .into_iter()
        .map(|s| s.to_string())
        .collect();
    let visible = widget.is_visible();
    let sensitive = widget.is_sensitive();
    let bounds = widget.compute_bounds(window_root).map(|r| Bounds {
        x: r.x() as f64,
        y: r.y() as f64,
        width: r.width() as f64,
        height: r.height() as f64,
    });

    let mut children = Vec::new();
    if max_depth.map_or(true, |m| depth < m) {
        let mut cur = widget.first_child();
        while let Some(child) = cur {
            let next = child.next_sibling();
            children.push(to_element_info(
                &child,
                window_root,
                depth + 1,
                max_depth,
                counter,
            ));
            cur = next;
        }
    }

    ElementInfo {
        id,
        kind,
        widget_name,
        css_classes,
        visible,
        sensitive,
        bounds,
        children,
    }
}

fn node_count(info: &ElementInfo) -> u32 {
    1 + info.children.iter().map(node_count).sum::<u32>()
}
