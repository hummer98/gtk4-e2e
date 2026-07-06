//! Widget tree query — backs `GET /test/elements` (Step 14).
//!
//! `walk_elements` is the GTK-bound entry point invoked from the GLib main
//! thread via `MainCmd::Elements`. With no selector it dumps every active
//! window's subtree. With a selector it runs a DFS pre-order match across
//! the tree and returns one `ElementInfo` per matching widget; nested matches
//! inside an outer match are not duplicated as separate roots (§5.2 P-2).

use crate::gtk;
use crate::gtk::gdk;
use crate::gtk::gdk::prelude::*;
use crate::gtk::prelude::*;
use crate::proto::{Bounds, ElementInfo, ElementsResponse};
use crate::tree::{parse_selector, Selector};
use crate::wait::WidgetLike;
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
                let info =
                    to_element_info(&widget, &widget, 0, max_depth, props, &mut counter, None);
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
        let info = to_element_info(widget, window_root, 0, max_depth, props, counter, None);
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

/// Carries a detected popover root's toplevel-widget origin plus the root
/// widget itself, so descendant bounds can be composed without re-resolving
/// the `native → surface → popup → surface_transform` chain per child
/// (plan §1.1 / §8.1).
///
/// The root is held as an **owned** `gtk::Widget`. GObject `.clone()` is a
/// refcount bump only (cheap), and owning it sidesteps the borrow-checker
/// conflict that storing a `&gtk::Widget` reference would create against the
/// DFS lifetimes. Descendants receive `Option<&PopoverFrame>` by reference so
/// no per-child clone happens.
struct PopoverFrame {
    /// Toplevel-widget-space origin `(x, y)` of the popover root.
    origin: (f64, f64),
    /// The popover root widget; the `compute_bounds` target for descendants.
    root: gtk::Widget,
}

/// Compose a popover root's bounds in toplevel-widget coordinates from its
/// GdkPopup surface geometry. GTK-free so it is unit-testable (plan §1.2).
///
/// `position_*` are the popup's offset within the parent (toplevel) *surface*;
/// `toplevel_transform` is the toplevel `surface_transform` translating surface
/// → widget coordinates, so `widget = surface + transform` (research §4.1; R3
/// fixes the sign as addition). The popup surface size is carried through
/// unchanged (may include CSD shadow margin — R1).
fn compose_popover_root_bounds(
    position_x: f64,
    position_y: f64,
    surface_w: f64,
    surface_h: f64,
    toplevel_transform: (f64, f64),
) -> Bounds {
    Bounds {
        x: position_x + toplevel_transform.0,
        y: position_y + toplevel_transform.1,
        width: surface_w,
        height: surface_h,
    }
}

/// Compose a popover descendant's bounds: its popover-root-relative local rect
/// offset by the popover root's toplevel-widget origin (plan §1.2). The origin
/// affects only `x`/`y`; the size comes from the local rect.
fn compose_child_bounds(
    origin: (f64, f64),
    local_x: f64,
    local_y: f64,
    local_w: f64,
    local_h: f64,
) -> Bounds {
    Bounds {
        x: origin.0 + local_x,
        y: origin.1 + local_y,
        width: local_w,
        height: local_h,
    }
}

/// Collection layer (plan §1.4, research §4.1): derive a popover root's
/// rectangle and toplevel-widget origin from its GdkPopup surface geometry.
///
/// Returns `None` — so the caller keeps the unchanged `None` bounds — when the
/// popover is not realized, has no surface, or the surface is not a `GdkPopup`
/// (e.g. a regular `GdkToplevel`). The `dynamic_cast_ref::<gdk::Popup>()?`
/// short-circuit absorbs R6 (non-popover surfaces are never mis-composed).
fn popover_root_frame(
    popover: &gtk::Widget,
    toplevel: &gtk::Widget,
) -> Option<(Bounds, (f64, f64))> {
    let native = popover.native()?;
    let surface = native.surface()?;
    let popup = surface.dynamic_cast_ref::<gdk::Popup>()?;
    let px = popup.position_x() as f64;
    let py = popup.position_y() as f64;
    let w = surface.width() as f64;
    let h = surface.height() as f64;
    let (tx, ty) = toplevel.native()?.surface_transform();
    let bounds = compose_popover_root_bounds(px, py, w, h, (tx, ty));
    Some((bounds, (bounds.x, bounds.y)))
}

fn to_element_info(
    widget: &gtk::Widget,
    window_root: &gtk::Widget,
    depth: u32,
    max_depth: Option<u32>,
    props: &[String],
    counter: &mut u32,
    popover_origin: Option<&PopoverFrame>,
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
    let text = widget_display_text(widget);

    // Bounds decision — 4 independent branches (plan §1.1). The popover-root
    // test (branch 2) is evaluated independently of `popover_origin`: a popover
    // root reaches DFS with `popover_origin = None` (it is the origin *setter*),
    // so nesting it under the `Some` branch would drop the first root to None.
    //
    // `child_frame` is `Some` only when this widget is a freshly detected
    // popover root (branch 2); `propagate_incoming` marks a popover descendant
    // (branch 3) that should reuse the inherited frame.
    let mut child_frame: Option<PopoverFrame> = None;
    let mut propagate_incoming = false;
    let bounds = match widget.compute_bounds(window_root) {
        // Branch 1: same-surface widget — existing behaviour, untouched.
        // Children carry no popover frame.
        Some(r) => Some(Bounds {
            x: r.x() as f64,
            y: r.y() as f64,
            width: r.width() as f64,
            height: r.height() as f64,
        }),
        None => {
            if widget.dynamic_cast_ref::<gtk::Popover>().is_some() {
                // Branch 2: popover root crossing into a new native surface.
                // Synthesise its frame from GdkPopup geometry and propagate.
                match popover_root_frame(widget, window_root) {
                    Some((b, origin)) => {
                        child_frame = Some(PopoverFrame {
                            origin,
                            root: widget.clone(),
                        });
                        Some(b)
                    }
                    None => None,
                }
            } else if let Some(frame) = popover_origin {
                // Branch 3: descendant of a detected popover. Measure relative
                // to the popover root (same surface → succeeds) and offset by
                // the root origin. Reuse the inherited frame for children.
                propagate_incoming = true;
                widget.compute_bounds(&frame.root).map(|local| {
                    compose_child_bounds(
                        frame.origin,
                        local.x() as f64,
                        local.y() as f64,
                        local.width() as f64,
                        local.height() as f64,
                    )
                })
            } else {
                // Branch 4: unrealized / unsupported — unchanged (None).
                None
            }
        }
    };

    // Frame handed to children: a new root's frame (branch 2), the inherited
    // frame for popover descendants (branch 3), or none (branch 1 / 4).
    let frame_for_children: Option<&PopoverFrame> = if let Some(f) = child_frame.as_ref() {
        Some(f)
    } else if propagate_incoming {
        popover_origin
    } else {
        None
    };

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
                frame_for_children,
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
        text,
        bounds,
        properties,
        children,
    }
}

/// Human-visible text for text-bearing widgets (issue #17), or `None` for
/// widgets that carry no display text (the field then stays off the wire).
///
/// Covered widgets:
/// - `GtkLabel` — `label.text()`, i.e. the displayed string after mnemonic /
///   markup processing.
/// - `GtkEditable` implementors (`GtkEntry`, `GtkText`, `GtkSearchEntry`,
///   `GtkSpinButton`, ...) — the current editable content.
/// - `GtkTextView` — the full buffer content between start and end iters,
///   excluding hidden (invisible-tagged) characters.
///
/// The `GtkLabel` arm runs first only for clarity; the three widget classes
/// are disjoint so the order does not change the result.
fn widget_display_text(widget: &gtk::Widget) -> Option<String> {
    if let Some(label) = widget.dynamic_cast_ref::<gtk::Label>() {
        return Some(label.text().to_string());
    }
    if let Some(editable) = widget.dynamic_cast_ref::<gtk::Editable>() {
        return Some(editable.text().to_string());
    }
    if let Some(view) = widget.dynamic_cast_ref::<gtk::TextView>() {
        let buffer = view.buffer();
        return Some(
            buffer
                .text(&buffer.start_iter(), &buffer.end_iter(), false)
                .to_string(),
        );
    }
    None
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
        let entry = widget
            .read_property_as_json(name)
            .unwrap_or_else(crate::wait::sentinel_for);
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
            // list_properties() advertised the name, so a Missing here means
            // the property layer disagrees with itself — `sentinel_for` keeps
            // the gap visible via the `$missing` sentinel.
            let entry = widget
                .read_property_as_json(&name)
                .unwrap_or_else(crate::wait::sentinel_for);
            map.insert(name, entry);
        }
    }
    Some(map)
}

fn node_count(info: &ElementInfo) -> u32 {
    1 + info.children.iter().map(node_count).sum::<u32>()
}

#[cfg(test)]
mod tests {
    //! Deterministic, GTK-free unit tests for the coordinate-composition pure
    //! functions (plan §4.1). These pin the arithmetic for R1 (size carried
    //! through), R2 (single logical-px unit) and R3 (transform addition sign)
    //! without depending on a live GdkPopup, so they run in headless CI.
    use super::{compose_child_bounds, compose_popover_root_bounds};

    #[test]
    fn popover_root_zero_transform_is_identity() {
        let b = compose_popover_root_bounds(200.0, 50.0, 160.0, 120.0, (0.0, 0.0));
        assert_eq!((b.x, b.y, b.width, b.height), (200.0, 50.0, 160.0, 120.0));
    }

    #[test]
    fn popover_root_nonzero_transform_is_added() {
        // R3: widget = surface + transform (addition, not subtraction).
        let b = compose_popover_root_bounds(200.0, 50.0, 160.0, 120.0, (8.0, 8.0));
        assert_eq!((b.x, b.y), (208.0, 58.0));
        // The transform must not perturb the surface size.
        assert_eq!((b.width, b.height), (160.0, 120.0));
    }

    #[test]
    fn child_offsets_by_origin() {
        let b = compose_child_bounds((200.0, 50.0), 10.0, 12.0, 80.0, 20.0);
        assert_eq!((b.x, b.y, b.width, b.height), (210.0, 62.0, 80.0, 20.0));
    }

    #[test]
    fn child_size_comes_from_local_origin_only_shifts_xy() {
        // Composition-direction guard: origin shifts x/y only; width/height are
        // taken verbatim from the local (popover-root-relative) rect.
        let b = compose_child_bounds((1000.0, 2000.0), 0.0, 0.0, 33.0, 44.0);
        assert_eq!((b.x, b.y), (1000.0, 2000.0));
        assert_eq!((b.width, b.height), (33.0, 44.0));
    }
}
