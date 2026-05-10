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
    let resp = walk_elements(&app, None, None).expect("walk should succeed");
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
    let resp = walk_elements(&app, Some("#input1"), None).expect("walk should succeed");
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
    let resp = walk_elements(&app, Some(".primary"), None).expect("walk should succeed");
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
    let resp = walk_elements(&app, Some("#nosuch"), None).expect("walk should succeed");
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
    let resp = walk_elements(&app, None, Some(0)).expect("walk should succeed");
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
    let resp = walk_elements(&app, None, Some(1)).expect("walk should succeed");
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

    let err = walk_elements(&app, None, None).expect_err("no windows -> error");
    assert!(matches!(err, ElementsError::NoActiveWindow));
}

#[test]
fn invalid_selector_yields_error() {
    if !require_display() {
        return;
    }
    let (app, window) = build_fixture();
    let err = walk_elements(&app, Some("@bad"), None).expect_err("invalid selector");
    assert!(matches!(err, ElementsError::InvalidSelector { .. }));
    window.close();
    common::pump_glib(32);
}
