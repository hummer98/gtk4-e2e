//! Integration tests for popover `bounds` synthesis in `walk_elements`
//! (task 028-a, plan §4.2 / §4.3).
//!
//! Two guarantees:
//!   - non-popover widgets keep their existing `compute_bounds` value even
//!     while a popover is open (regression / R6).
//!   - an open popover node and its children get real x/y/w/h bounds composed
//!     from GdkPopup geometry, *when* the host realizes the popup surface.
//!
//! Auto-skips on display-less hosts (`gtk::init()` fails). The popover-bounds
//! assertions further skip when the compositor does not realize the popup
//! surface (headless), printing a `[skip]` line — see plan §4.5 / R4.

#![cfg(feature = "e2e")]

mod common;

use gtk4_e2e_server::elements::walk_elements;
use gtk4_e2e_server::gtk;
use gtk4_e2e_server::gtk::prelude::*;
use gtk4_e2e_server::proto::ElementInfo;

/// Local display gate (plan §8.1: `require_display` is not a shared helper —
/// only `common::pump_glib` is shared).
fn require_display() -> bool {
    if !common::ensure_gtk_init() {
        eprintln!("[skip] no GTK display available");
        return false;
    }
    true
}

/// Build: window > vbox > [MenuButton#menu-btn] with a Popover (#nav-popover)
/// containing a Label#popover-child. The popover is popped up and the main
/// loop pumped so the surface has a chance to realize.
fn build_popover_fixture() -> (gtk::Application, gtk::ApplicationWindow, gtk::Popover) {
    let app = gtk::Application::builder()
        .application_id("dev.gtk4-e2e.popover-bounds-test")
        .build();
    let _ = app.register(None::<&gtk::gio::Cancellable>);

    let child = gtk::Label::new(Some("popover body"));
    child.set_widget_name("popover-child");

    let popover = gtk::Popover::builder().build();
    popover.set_widget_name("nav-popover");
    popover.set_child(Some(&child));

    let menu_btn = gtk::MenuButton::builder().build();
    menu_btn.set_widget_name("menu-btn");
    menu_btn.set_popover(Some(&popover));

    let vbox = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    vbox.append(&menu_btn);

    let window = gtk::ApplicationWindow::builder()
        .application(&app)
        .default_width(400)
        .default_height(300)
        .child(&vbox)
        .build();
    window.present();
    common::pump_glib(64);

    popover.popup();
    common::pump_glib(64);

    (app, window, popover)
}

/// True when the popover surface realized in this environment. When false the
/// GdkPopup geometry is unavailable and bounds assertions must be skipped.
fn popover_realized(popover: &gtk::Popover) -> bool {
    popover.is_mapped() && popover.native().and_then(|n| n.surface()).is_some()
}

/// Find a node by widget_name in a walk result subtree.
fn find_node<'a>(node: &'a ElementInfo, name: &str) -> Option<&'a ElementInfo> {
    if node.widget_name.as_deref() == Some(name) {
        return Some(node);
    }
    node.children.iter().find_map(|c| find_node(c, name))
}

#[test]
fn non_popover_widget_keeps_compute_bounds_while_popover_open() {
    if !require_display() {
        return;
    }
    let (app, window, _popover) = build_popover_fixture();

    // The MenuButton lives in the toplevel surface, so its bounds come from the
    // unchanged compute_bounds path regardless of the open popover (R6: the
    // fallback must not pollute same-surface widgets).
    let resp = walk_elements(&app, Some("#menu-btn"), None, &[]).expect("walk should succeed");
    assert_eq!(resp.roots.len(), 1, "menu-btn should match exactly once");
    let btn = &resp.roots[0];
    assert_eq!(btn.kind, "GtkMenuButton");
    let b = btn
        .bounds
        .as_ref()
        .expect("menu-btn is a realized toplevel-surface widget; bounds must be Some");
    assert!(
        b.width > 0.0 && b.height > 0.0,
        "menu-btn bounds should be a real rect, got {b:?}"
    );

    window.close();
    common::pump_glib(32);
}

#[test]
fn open_popover_node_and_child_get_real_bounds() {
    if !require_display() {
        return;
    }
    let (app, window, popover) = build_popover_fixture();

    if !popover_realized(&popover) {
        eprintln!(
            "[skip] popover surface not realized in this environment; \
             GdkPopup geometry unavailable (headless compositor) — \
             popover bounds assertions skipped (plan §4.5 / R4)"
        );
        window.close();
        common::pump_glib(32);
        return;
    }

    // Full-tree walk so the popover root is detected during DFS and its origin
    // propagates to the child (plan §6.1: selector-direct-on-child is out of
    // scope; full-tree walk is the supported path for C1).
    let resp = walk_elements(&app, None, None, &[]).expect("walk should succeed");
    let root = &resp.roots[0];

    let popover_node = find_node(root, "nav-popover").expect("popover node present in tree");
    let pb = popover_node
        .bounds
        .as_ref()
        .expect("open popover must have synthesized bounds (C1)");
    assert!(
        pb.width > 0.0 && pb.height > 0.0,
        "popover bounds must be a real rect, got {pb:?}"
    );
    assert!(
        pb.x >= 0.0 && pb.y >= 0.0,
        "popover origin should be in-window, got {pb:?}"
    );

    let child_node = find_node(root, "popover-child").expect("popover child present in tree");
    let cb = child_node
        .bounds
        .as_ref()
        .expect("popover child must have composed bounds (C1)");
    assert!(
        cb.width > 0.0 && cb.height > 0.0,
        "popover child bounds must be a real rect, got {cb:?}"
    );
    // Loose containment: the child sits within (or at) the popover frame, give
    // or take CSD/padding slack (R1).
    let eps = 4.0;
    assert!(
        cb.x >= pb.x - eps && cb.y >= pb.y - eps,
        "child {cb:?} should sit inside popover {pb:?}"
    );

    eprintln!("[observed] popover bounds = {pb:?}; child bounds = {cb:?}");

    window.close();
    common::pump_glib(32);
}
