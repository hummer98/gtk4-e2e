//! Integration tests for `elements::walk_elements`.
//!
//! Auto-skips on display-less hosts (no `gtk::init()`), like `input_tap.rs`.
//! Plan T018 §11.1 row 2.

#![cfg(feature = "e2e")]

mod common;

use gtk4_e2e_server::elements::{walk_elements, ElementsError};
use gtk4_e2e_server::gtk;
use gtk4_e2e_server::gtk::prelude::*;

fn require_display() -> bool {
    if !common::ensure_gtk_init() {
        eprintln!("[skip] no GTK display available");
        return false;
    }
    true
}

/// Build a small fixture: window > vbox > [entry(.primary)#input1, label#label1].
fn build_fixture() -> (gtk::Application, gtk::ApplicationWindow) {
    let app = gtk::Application::builder()
        .application_id("dev.gtk4-e2e.elements-test")
        .build();
    let _ = app.register(None::<&gtk::gio::Cancellable>);

    let entry = gtk::Entry::builder().build();
    entry.set_widget_name("input1");
    entry.add_css_class("primary");
    let label = gtk::Label::new(Some("hi"));
    label.set_widget_name("label1");
    let vbox = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    vbox.append(&entry);
    vbox.append(&label);

    let window = gtk::ApplicationWindow::builder()
        .application(&app)
        .child(&vbox)
        .build();
    window.present();
    common::pump_glib(64);
    (app, window)
}

#[test]
fn full_tree_returns_one_root_per_window() {
    if !require_display() {
        return;
    }
    let (app, window) = build_fixture();
    let resp = walk_elements(&app, None, None, &[]).expect("walk should succeed");
    assert_eq!(resp.roots.len(), 1, "single window expected");
    assert!(
        resp.count > 1,
        "tree should have many nodes, got {}",
        resp.count
    );

    // count must be the recursive sum over roots.
    fn rec(info: &gtk4_e2e_server::proto::ElementInfo) -> u32 {
        1 + info.children.iter().map(rec).sum::<u32>()
    }
    let recomputed: u32 = resp.roots.iter().map(rec).sum();
    assert_eq!(
        recomputed, resp.count,
        "count must equal recursive node count"
    );

    window.close();
    common::pump_glib(32);
}

#[test]
fn selector_hit_by_name() {
    if !require_display() {
        return;
    }
    let (app, window) = build_fixture();
    let resp = walk_elements(&app, Some("#input1"), None, &[]).expect("walk should succeed");
    assert_eq!(resp.roots.len(), 1);
    assert_eq!(resp.roots[0].widget_name.as_deref(), Some("input1"));
    assert_eq!(resp.roots[0].kind, "GtkEntry");
    window.close();
    common::pump_glib(32);
}

#[test]
fn selector_hit_by_class() {
    if !require_display() {
        return;
    }
    let (app, window) = build_fixture();
    let resp = walk_elements(&app, Some(".primary"), None, &[]).expect("walk should succeed");
    assert_eq!(resp.roots.len(), 1);
    assert_eq!(resp.roots[0].widget_name.as_deref(), Some("input1"));
    assert!(resp.roots[0].css_classes.iter().any(|c| c == "primary"));
    window.close();
    common::pump_glib(32);
}

#[test]
fn selector_miss_returns_empty_roots() {
    if !require_display() {
        return;
    }
    let (app, window) = build_fixture();
    let resp = walk_elements(&app, Some("#nosuch"), None, &[]).expect("walk should succeed");
    assert!(resp.roots.is_empty());
    assert_eq!(resp.count, 0);
    window.close();
    common::pump_glib(32);
}

#[test]
fn max_depth_zero_drops_children() {
    if !require_display() {
        return;
    }
    let (app, window) = build_fixture();
    let resp = walk_elements(&app, None, Some(0), &[]).expect("walk should succeed");
    assert_eq!(resp.roots.len(), 1);
    assert!(
        resp.roots[0].children.is_empty(),
        "depth 0 must produce no children"
    );
    assert_eq!(resp.count, 1);
    window.close();
    common::pump_glib(32);
}

#[test]
fn max_depth_one_includes_only_immediate_children() {
    if !require_display() {
        return;
    }
    let (app, window) = build_fixture();
    let resp = walk_elements(&app, None, Some(1), &[]).expect("walk should succeed");
    let root = &resp.roots[0];
    // root + at least its immediate child.
    assert!(!root.children.is_empty(), "depth 1 must include children");
    for c in &root.children {
        assert!(
            c.children.is_empty(),
            "depth 1 must not descend past first level (got grandchildren under {:?})",
            c.kind
        );
    }
    window.close();
    common::pump_glib(32);
}

#[test]
fn no_active_window_yields_error() {
    if !require_display() {
        return;
    }
    let app = gtk::Application::builder()
        .application_id("dev.gtk4-e2e.elements-noactive")
        .build();
    let _ = app.register(None::<&gtk::gio::Cancellable>);

    let err = walk_elements(&app, None, None, &[]).expect_err("no windows -> error");
    assert!(matches!(err, ElementsError::NoActiveWindow));
}

#[test]
fn invalid_selector_yields_error() {
    if !require_display() {
        return;
    }
    let (app, window) = build_fixture();
    let err = walk_elements(&app, Some("@bad"), None, &[]).expect_err("invalid selector");
    assert!(matches!(err, ElementsError::InvalidSelector { .. }));
    window.close();
    common::pump_glib(32);
}

#[test]
fn empty_props_leaves_properties_field_off() {
    if !require_display() {
        return;
    }
    let (app, window) = build_fixture();
    let resp = walk_elements(&app, Some("#input1"), None, &[]).expect("walk should succeed");
    let node = &resp.roots[0];
    assert!(
        node.properties.is_none(),
        "empty props must omit the properties field (got {:?})",
        node.properties
    );
    window.close();
    common::pump_glib(32);
}

#[test]
fn props_reads_string_value_from_entry() {
    if !require_display() {
        return;
    }
    let (app, window) = build_fixture();

    // Drive the live (computed) text value rather than the static initial
    // so the test exercises read-through, not just the field declaration.
    let win = app.windows().into_iter().next().expect("window present");
    let entry = find_named(&win.upcast::<gtk::Widget>(), "input1").expect("entry found");
    entry
        .downcast::<gtk::Entry>()
        .expect("Entry downcast")
        .set_text("hello-props");
    common::pump_glib(32);

    let props = vec!["text".to_string()];
    let resp = walk_elements(&app, Some("#input1"), None, &props).expect("walk should succeed");
    let node = &resp.roots[0];
    let map = node
        .properties
        .as_ref()
        .expect("properties must be present when props non-empty");
    assert_eq!(
        map.get("text"),
        Some(&serde_json::Value::String("hello-props".to_string())),
        "text property should round-trip via read_property_as_json"
    );
    window.close();
    common::pump_glib(32);
}

#[test]
fn props_unknown_name_emits_missing_sentinel() {
    if !require_display() {
        return;
    }
    let (app, window) = build_fixture();
    let props = vec!["this-property-does-not-exist".to_string()];
    let resp = walk_elements(&app, Some("#input1"), None, &props).expect("walk should succeed");
    let node = &resp.roots[0];
    let map = node.properties.as_ref().expect("properties present");
    let entry = map
        .get("this-property-does-not-exist")
        .expect("entry present");
    assert_eq!(
        entry,
        &serde_json::json!({"$missing": true}),
        "missing properties should surface the $missing sentinel"
    );
    window.close();
    common::pump_glib(32);
}

#[test]
fn props_wildcard_enumerates_readable_gobject_properties() {
    // `props=["*"]` should expand to every readable GObject property
    // advertised by the widget. We don't assert the full set (it varies
    // with GTK4 minor versions) but we do require:
    //   - `name` is present and matches the static widget_name we set
    //   - several well-known GtkWidget-class properties are listed
    //   - explicit names listed alongside `*` are preserved (the wildcard
    //     must not stomp on an explicitly-supplied key).
    if !require_display() {
        return;
    }
    let (app, window) = build_fixture();
    let props = vec!["*".to_string(), "name".to_string()];
    let resp = walk_elements(&app, Some("#input1"), None, &props).expect("walk should succeed");
    let node = &resp.roots[0];
    let map = node
        .properties
        .as_ref()
        .expect("properties present when * requested");

    // `name` is a GObject property on GtkWidget — set by set_widget_name.
    assert_eq!(
        map.get("name"),
        Some(&serde_json::Value::String("input1".to_string())),
        "explicit + wildcard: name should equal the set widget_name"
    );

    // A handful of GtkWidget-class properties that are stable across the
    // gtk4-rs versions we support — used here as a sanity probe that the
    // wildcard expansion really enumerated the class, not just whatever
    // was named explicitly. We do NOT assert specific values for these,
    // only that they are listed (the value may be a sentinel for
    // unsupported types).
    for required in ["visible", "sensitive", "width-request", "height-request"] {
        assert!(
            map.contains_key(required),
            "wildcard expansion missing expected GtkWidget property {required:?}; got {:?}",
            map.keys().collect::<Vec<_>>()
        );
    }

    // `text` is the GtkEntry-specific property — proves we dispatched
    // off the actual widget class, not just GtkWidget.
    assert!(
        map.contains_key("text"),
        "wildcard expansion on GtkEntry should include `text`; got {:?}",
        map.keys().collect::<Vec<_>>()
    );

    window.close();
    common::pump_glib(32);
}

// ---------------------------------------------------------------------------
// Popover (cross-surface) bounds — ADR-0004. The popover content lives on its
// own GdkSurface (xdg_popup); these tests prove `compute_widget_bounds`
// composes it back into the parent-window coordinate space (basis =
// PopupComposed) and that same-surface widgets are unchanged (basis = None).
// ---------------------------------------------------------------------------

/// Window > vbox > [Button#popover-btn (parent of Popover with content
/// Label#popover-content), Label#anchor-probe]. Returns the window and the
/// popover so the caller can `popup()` it. Mirrors the demo wiring.
fn build_popover_fixture() -> (gtk::Application, gtk::ApplicationWindow, gtk::Popover) {
    let app = gtk::Application::builder()
        .application_id("dev.gtk4-e2e.popover-test")
        .build();
    let _ = app.register(None::<&gtk::gio::Cancellable>);

    let content = gtk::Label::new(Some("popover body text"));
    content.set_widget_name("popover-content");
    let popover = gtk::Popover::builder().child(&content).build();

    let popover_btn = gtk::Button::with_label("Open");
    popover_btn.set_widget_name("popover-btn");
    popover.set_parent(&popover_btn);

    let probe = gtk::Label::new(Some("probe"));
    probe.set_widget_name("anchor-probe");

    let vbox = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    vbox.append(&popover_btn);
    vbox.append(&probe);

    let window = gtk::ApplicationWindow::builder()
        .application(&app)
        .default_width(360)
        .default_height(300)
        .child(&vbox)
        .build();
    window.present();
    common::pump_glib(128);
    (app, window, popover)
}

/// Open the popover and pump until its surface is mapped.
fn open_popover(popover: &gtk::Popover) {
    popover.popup();
    common::pump_glib(128);
    common::pump_glib_for(std::time::Duration::from_millis(200));
}

/// Find the (single) `#popover-content` node anywhere in the response roots.
fn find_node<'a>(
    roots: &'a [gtk4_e2e_server::proto::ElementInfo],
    name: &str,
) -> Option<&'a gtk4_e2e_server::proto::ElementInfo> {
    for r in roots {
        if r.widget_name.as_deref() == Some(name) {
            return Some(r);
        }
        if let Some(hit) = find_node(&r.children, name) {
            return Some(hit);
        }
    }
    None
}

#[test]
fn popover_content_bounds_are_composed_in_window_space() {
    if !require_display() {
        return;
    }
    let (app, window, popover) = build_popover_fixture();
    open_popover(&popover);

    // Window root bounds (self-relative) = (0,0,W,H).
    let root_resp = walk_elements(&app, None, None, &[]).expect("walk ok");
    let root = &root_resp.roots[0];
    let root_b = root.bounds.expect("window has bounds");
    let (win_w, win_h) = (root_b.width, root_b.height);
    eprintln!("[measure] window WxH = {win_w} x {win_h}");

    let resp = walk_elements(&app, Some("#popover-content"), None, &[]).expect("walk ok");
    let node = find_node(&resp.roots, "popover-content").expect("popover-content found");
    let b = node
        .bounds
        .unwrap_or_else(|| panic!("popover content must have composed bounds; node = {node:?}"));
    eprintln!("[measure] popover-content bounds = {b:?}");

    // basis must mark this as cross-surface composed.
    assert_eq!(
        b.basis,
        Some(gtk4_e2e_server::proto::BoundsBasis::PopupComposed),
        "popover content bounds must carry basis=popup_composed"
    );

    // All four corners inside the parent window rectangle (AC2). On real
    // compositors xdg_popup is constrained on-screen, so a happy-path popover
    // is in-window; a sign error in (B)/(D) would push a corner out.
    assert!(b.x >= 0.0, "x>=0, got {}", b.x);
    assert!(b.y >= 0.0, "y>=0, got {}", b.y);
    assert!(
        b.x + b.width <= win_w,
        "x+w<=W, got {} > {win_w}",
        b.x + b.width
    );
    assert!(
        b.y + b.height <= win_h,
        "y+h<=H, got {} > {win_h}",
        b.y + b.height
    );

    // Numeric sanity (M3): width/height are finite and positive (natural label
    // size), not degenerate.
    assert!(
        b.width > 0.0 && b.height > 0.0,
        "w/h must be positive: {b:?}"
    );
    assert!(b.x.is_finite() && b.y.is_finite(), "x/y finite: {b:?}");

    window.close();
    common::pump_glib(32);
}

#[test]
fn popover_origin_aligns_with_anchor() {
    // M3 / plan step 4: pin the *sign* of the composition with an oracle that
    // is INDEPENDENT of the composition formula — the geometric relationship
    // between the popover content and its anchor button (both fetched via
    // walk, the button on the same surface with basis=None).
    //
    // GTK centres a default popover horizontally on its anchor, so the content
    // centre-x must equal the anchor centre-x (catches an x-sign flip), and
    // the content must sit vertically adjacent to the anchor — just below it,
    // or flipped just above when the anchor is near the bottom edge — never
    // deep inside or far from it (catches a y-sign flip). Measured on macOS
    // quartz: anchor y=716 (bottom) → content composed to y=680, i.e. flipped
    // above and centred, all corners in-window.
    if !require_display() {
        return;
    }
    let (app, window, popover) = build_popover_fixture();
    open_popover(&popover);

    let resp = walk_elements(&app, None, None, &[]).expect("walk ok");
    let anchor = find_node(&resp.roots, "popover-btn")
        .expect("anchor button found")
        .bounds
        .expect("anchor has bounds");
    let content = find_node(&resp.roots, "popover-content")
        .expect("content found")
        .bounds
        .expect("content has composed bounds");
    eprintln!("[measure] anchor={anchor:?} content={content:?}");

    let anchor_cx = anchor.x + anchor.width / 2.0;
    let content_cx = content.x + content.width / 2.0;
    // Horizontal centring: catches an x-sign error (which would offset the
    // content sideways by 2*position_x, well beyond this tolerance).
    assert!(
        (anchor_cx - content_cx).abs() <= 8.0,
        "popover content centre-x {content_cx} must align with anchor centre-x {anchor_cx}"
    );

    // Vertical adjacency: the content edge nearest the anchor must be within a
    // small gap of the anchor (popover padding/shadow), i.e. the popover hugs
    // the anchor either just below or just above it. A y-sign flip would push
    // it ~2*position_y away (hundreds of px), failing this bound.
    let gap_below = content.y - (anchor.y + anchor.height); // content under anchor
    let gap_above = anchor.y - (content.y + content.height); // content over anchor
    let adjacent = (-2.0..=48.0).contains(&gap_below) || (-2.0..=48.0).contains(&gap_above);
    assert!(
        adjacent,
        "popover content must hug the anchor vertically (gap_below={gap_below}, gap_above={gap_above})"
    );

    window.close();
    common::pump_glib(32);
}

#[test]
fn same_surface_widget_keeps_basis_none() {
    // Degrade-proof: a normal main-window widget still gets basis=None
    // (legacy payload unchanged, AC3).
    if !require_display() {
        return;
    }
    let (app, window, _popover) = build_popover_fixture();
    let resp = walk_elements(&app, Some("#anchor-probe"), None, &[]).expect("walk ok");
    let node = &resp.roots[0];
    let b = node.bounds.expect("probe has bounds");
    assert_eq!(b.basis, None, "same-surface widget must keep basis=None");
    window.close();
    common::pump_glib(32);
}

/// DFS helper for `props_reads_string_value_from_entry` — locate a named
/// widget without going through walk_elements (so the test can prepare
/// fixture state directly against the GTK object).
fn find_named(root: &gtk::Widget, name: &str) -> Option<gtk::Widget> {
    if root.widget_name() == name {
        return Some(root.clone());
    }
    let mut cur = root.first_child();
    while let Some(child) = cur {
        let next = child.next_sibling();
        if let Some(hit) = find_named(&child, name) {
            return Some(hit);
        }
        cur = next;
    }
    None
}
