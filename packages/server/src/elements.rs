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
use crate::wait::{PropReadError, WidgetLike};
use std::collections::BTreeMap;

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
    props: &[String],
) -> Result<ElementsResponse, ElementsError> {
    let windows = app.windows();
    if windows.is_empty() {
        return Err(ElementsError::NoActiveWindow);
    }

    let parsed = match selector {
        Some(s) => Some(
            parse_selector(s).map_err(|e| ElementsError::InvalidSelector {
                reason: e.reason.to_string(),
            })?,
        ),
        None => None,
    };

    let mut roots: Vec<ElementInfo> = Vec::new();
    let mut counter: u32 = 0;

    for window in windows {
        let widget = window.upcast::<gtk::Widget>();
        match &parsed {
            None => {
                let info = to_element_info(&widget, &widget, 0, max_depth, props, &mut counter);
                roots.push(info);
            }
            Some(sel) => {
                collect_matches(
                    &widget,
                    &widget,
                    sel,
                    max_depth,
                    props,
                    &mut counter,
                    &mut roots,
                );
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
    props: &[String],
    counter: &mut u32,
    roots: &mut Vec<ElementInfo>,
) {
    if widget_matches(widget, sel) {
        let info = to_element_info(widget, window_root, 0, max_depth, props, counter);
        roots.push(info);
        return;
    }
    let mut cur = widget.first_child();
    while let Some(child) = cur {
        let next = child.next_sibling();
        collect_matches(&child, window_root, sel, max_depth, props, counter, roots);
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
    props: &[String],
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
    let properties = read_requested_properties(widget, props);

    let mut children = Vec::new();
    if max_depth.is_none_or(|m| depth < m) {
        let mut cur = widget.first_child();
        while let Some(child) = cur {
            let next = child.next_sibling();
            children.push(to_element_info(
                &child,
                window_root,
                depth + 1,
                max_depth,
                props,
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
        properties,
        children,
    }
}

/// Build the per-node `properties` map for an opt-in `props=` request.
///
/// Returns `None` when no properties were requested so the field stays
/// off the wire (see `ElementInfo` doc comment). Each requested name
/// is looked up via the shared `WidgetLike::read_property_as_json`
/// helper from `wait.rs`; missing properties and unsupported types
/// surface as the documented sentinel objects rather than dropping
/// the entry, so callers can tell "absent on this widget" apart from
/// "I forgot to ask for it".
///
/// The literal token `"*"` in `props` is the wildcard: it expands to
/// every readable GObject property advertised by this widget's class
/// (`list_properties()` filtered on `ParamFlags::READABLE`). Mixing
/// `"*"` with specific names is allowed — explicit names always win,
/// so a value already in `map` is not overwritten by the wildcard pass.
fn read_requested_properties(
    widget: &gtk::Widget,
    props: &[String],
) -> Option<BTreeMap<String, serde_json::Value>> {
    if props.is_empty() {
        return None;
    }
    let mut map: BTreeMap<String, serde_json::Value> = BTreeMap::new();
    let mut want_all = false;
    for name in props {
        if name == "*" {
            want_all = true;
            continue;
        }
        let entry = match widget.read_property_as_json(name) {
            Ok(v) => v,
            Err(PropReadError::Missing) => {
                serde_json::json!({ "$missing": true })
            }
            Err(PropReadError::Unsupported(type_name)) => {
                serde_json::json!({ "$unsupported": type_name })
            }
        };
        map.insert(name.clone(), entry);
    }
    if want_all {
        for pspec in widget.list_properties() {
            if !pspec.flags().contains(gtk::glib::ParamFlags::READABLE) {
                continue;
            }
            let name = pspec.name().to_string();
            if map.contains_key(&name) {
                // Explicit ask wins over the wildcard expansion.
                continue;
            }
            let entry = match widget.read_property_as_json(&name) {
                Ok(v) => v,
                Err(PropReadError::Missing) => {
                    // list_properties() advertised the name, so Missing
                    // here is the property layer disagreeing with itself —
                    // surface the sentinel so the gap is visible.
                    serde_json::json!({ "$missing": true })
                }
                Err(PropReadError::Unsupported(type_name)) => {
                    serde_json::json!({ "$unsupported": type_name })
                }
            };
            map.insert(name, entry);
        }
    }
    Some(map)
}

fn node_count(info: &ElementInfo) -> u32 {
    1 + info.children.iter().map(node_count).sum::<u32>()
}
