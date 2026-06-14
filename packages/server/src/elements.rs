//! Widget tree query — backs `GET /test/elements` (Step 14).
//!
//! `walk_elements` is the GTK-bound entry point invoked from the GLib main
//! thread via `MainCmd::Elements`. With no selector it dumps every active
//! window's subtree. With a selector it runs a DFS pre-order match across
//! the tree and returns one `ElementInfo` per matching widget; nested matches
//! inside an outer match are not duplicated as separate roots (§5.2 P-2).

use crate::gtk;
use crate::gtk::gdk;
use crate::gtk::prelude::*;
use crate::proto::{Bounds, BoundsBasis, ElementInfo, ElementsResponse};
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
    let bounds = compute_widget_bounds(widget, window_root);
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

/// Compute a widget's bounds relative to the parent `GtkWindow` root widget,
/// dispatching on **surface (GtkNative) identity** rather than on whether
/// `compute_bounds` happens to return `None` (M1 / ADR-0004).
///
/// - Same native as `window_root` (the common case, and the only case for
///   plain main-window widgets and other toplevels) → the legacy
///   `compute_bounds(window_root)` path with `basis = None`. Unrealized
///   same-surface widgets fall here too and yield `None`, exactly as before —
///   no behavioural change.
/// - Different native (the widget lives on a separate `GdkSurface`, i.e. a
///   `GtkPopover`'s `xdg_popup`) → composed back to the window origin via
///   `compose_popover_bounds`, tagged `basis = Some(PopupComposed)`.
///   Composition failure (not a `GdkPopup`, position unavailable) returns
///   `None`, i.e. the same `bounds: null` the caller saw before this change.
///
/// Why native identity: `widget.native()` returns the single `GtkNative`
/// (= one surface) that hosts the widget, so "different surface?" is decided
/// deterministically and is **independent** of whether the underlying
/// `gtk_widget_compute_transform` returns `None` or an (incorrect) `Some` on a
/// given backend (X11 vs Wayland). That removes the risk of the fix being
/// silently disabled while CI stays green.
fn compute_widget_bounds(widget: &gtk::Widget, window_root: &gtk::Widget) -> Option<Bounds> {
    let same_surface = match (widget.native(), window_root.native()) {
        (Some(wn), Some(rn)) => {
            // GObject instance identity: the same surface is hosted by the
            // same `GtkNative` instance, so compare by upcast `==` (mr1).
            wn.upcast::<gtk::glib::Object>() == rn.upcast::<gtk::glib::Object>()
        }
        // Either side has no native (fully unrealized): treat as "not a
        // cross-surface popover". The legacy path below returns `None`,
        // preserving the previous `bounds: null`.
        _ => false,
    };

    if same_surface {
        return widget.compute_bounds(window_root).map(|r| Bounds {
            x: r.x() as f64,
            y: r.y() as f64,
            width: r.width() as f64,
            height: r.height() as f64,
            basis: None,
        });
    }

    compose_popover_bounds(widget, window_root)
}

/// Compose the bounds of a widget living on a separate `GdkPopup` surface
/// (a `GtkPopover`) back into the parent `GtkWindow` root-widget coordinate
/// space. Returns `None` (→ `bounds: null`) when the surface is not a
/// `GdkPopup` or its negotiated position is unavailable (e.g. the popover is
/// closed / unmapped).
///
/// The parent-window-relative origin of widget `w` is the sum of four terms
/// (ADR-0004 §Decision); each is read from a stable gtk4-rs `v4_6` API:
///
/// ```text
///   origin = (A) w within the popover widget         compute_bounds(&popover)
///          − (B) popover widget → popover surface     popover.surface_transform()
///          + (C) popover surface → parent surface      popup.position_{x,y}()
///          − (D) parent surface → window root widget   window.surface_transform()
/// ```
///
/// The signs of (B)/(D) were fixed by direct measurement (plan step 4): a
/// `GtkNative` surface transform is the offset **from the surface origin to
/// the widget origin** (CSS shadow / margin), so the widget origin in surface
/// space is `widget_coord − transform`. (C) is already parent-surface
/// relative and post-negotiation (flip/slide applied), so it adds directly.
fn compose_popover_bounds(widget: &gtk::Widget, window_root: &gtk::Widget) -> Option<Bounds> {
    let widget_native = widget.native()?;
    let popover = widget_native.downcast_ref::<gtk::Popover>()?;
    let popover_widget = popover.upcast_ref::<gtk::Widget>();

    // (A) widget rect relative to the popover widget origin — same surface, so
    //     this succeeds. w/h (allocation size) also come from here.
    let local = widget.compute_bounds(popover_widget)?;

    // (B) popover widget origin → popover surface origin.
    let (pop_tx, pop_ty) = popover.surface_transform();

    // (C) popover surface origin → parent surface origin (windowing-system
    //     negotiated; reflects any edge flip/slide).
    let surface = popover.surface()?;
    let popup = surface.downcast_ref::<gdk::Popup>()?;
    let pos_x = popup.position_x() as f64;
    let pos_y = popup.position_y() as f64;

    // (D) parent surface origin → parent window root widget origin.
    let root_native = window_root.native()?;
    let (win_tx, win_ty) = root_native.surface_transform();

    let x = local.x() as f64 - pop_tx + pos_x - win_tx;
    let y = local.y() as f64 - pop_ty + pos_y - win_ty;

    Some(Bounds {
        x,
        y,
        width: local.width() as f64,
        height: local.height() as f64,
        basis: Some(BoundsBasis::PopupComposed),
    })
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
