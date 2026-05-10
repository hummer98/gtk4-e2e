//! Minimal GTK4 demo embedding `gtk4-e2e-server` behind the `e2e` feature.
//!
//! See `docs/seed.md` §6 Step 2 and `docs/adr/0001-architecture.md`.

#[cfg(feature = "e2e")]
use std::cell::RefCell;
#[cfg(feature = "e2e")]
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{
    Align, Application, ApplicationWindow, Box as GtkBox, Button, CheckButton, Entry, Label,
    ListBox, Orientation, ScrolledWindow, Switch, ToggleButton,
};
#[cfg(feature = "e2e")]
use serde_json::{json, Value};

const APP_ID: &str = "dev.gtk4-e2e.demo";

fn main() {
    let app = Application::builder().application_id(APP_ID).build();

    // Hold the server `Handle` in a slot reachable from `main()`'s stack.
    // `connect_activate` requires `Fn`, so the closure cannot own the Handle
    // directly. The outer `Rc` keeps the slot alive across `app.run()`; when
    // `main` returns, the slot drops and `Handle::Drop` runs graceful
    // shutdown + registry cleanup.
    #[cfg(feature = "e2e")]
    let server_slot: Rc<RefCell<Option<gtk4_e2e_server::Handle>>> = Rc::new(RefCell::new(None));

    {
        #[cfg(feature = "e2e")]
        let server_slot = server_slot.clone();
        app.connect_activate(move |app| {
            #[cfg(feature = "e2e")]
            build_ui(app, server_slot.clone());
            #[cfg(not(feature = "e2e"))]
            build_ui(app);

            #[cfg(feature = "e2e")]
            {
                // `activate` may fire multiple times (e.g. primary instance
                // handover). Guard so we only spawn the server once.
                if server_slot.borrow().is_none() {
                    let handle = gtk4_e2e_server::start(app);
                    eprintln!(
                        "[gtk4-e2e-demo] server up on http://127.0.0.1:{}/test/info",
                        handle.port()
                    );
                    *server_slot.borrow_mut() = Some(handle);
                }
            }
        });
    }

    app.run();
}

fn build_ui(
    app: &Application,
    #[cfg(feature = "e2e")] server_slot: Rc<RefCell<Option<gtk4_e2e_server::Handle>>>,
) {
    // Plan §Q13 / Review M6: Entry initial text must be `"hello"` so the
    // scenario expectation (`state_eq label1.label = "hello"`) becomes true
    // as soon as the button is tapped.
    let entry = Entry::builder().text("hello").build();
    entry.set_widget_name("entry1");
    entry.add_css_class("primary");
    // Step 9: secondary Entry for `type` capability scenarios (initial empty).
    // No signal handlers — type.spec.ts asserts directly via `state_eq #input1.text`.
    let input1 = Entry::builder().build();
    input1.set_widget_name("input1");
    let label = Label::builder().label("waiting...").build();
    label.set_widget_name("label1");
    let button = Button::with_label("Apply");
    button.set_widget_name("btn1");

    // T019: Activatable / toggleable widgets so scenarios can drive
    // tap → app_state_eq end-to-end.
    let switch1 = Switch::builder().active(false).build();
    switch1.set_widget_name("switch1");
    let check1 = CheckButton::with_label("Subscribe");
    check1.set_widget_name("check1");
    let toggle1 = ToggleButton::with_label("Toggle");
    toggle1.set_widget_name("toggle1");

    // Accumulator pattern (plan §3.B.2): demo holds the canonical state
    // snapshot, each callback partially updates it, then pushes the full
    // snapshot via `Handle::set_state`. `Handle::set_state` keeps whole-
    // snapshot replace semantics on the server side — accumulating across
    // widgets is the demo's responsibility.
    #[cfg(feature = "e2e")]
    let demo_state: Rc<RefCell<Value>> = Rc::new(RefCell::new(json!({})));
    #[cfg(feature = "e2e")]
    let click_count: Rc<RefCell<u64>> = Rc::new(RefCell::new(0));

    {
        let entry = entry.clone();
        let label = label.clone();
        #[cfg(feature = "e2e")]
        let server_slot = server_slot.clone();
        #[cfg(feature = "e2e")]
        let demo_state = demo_state.clone();
        #[cfg(feature = "e2e")]
        let click_count = click_count.clone();
        button.connect_clicked(move |_| {
            let new_text = entry.text();
            label.set_text(new_text.as_str());

            // Step 7: surface the state change as an `EventEnvelope` for any
            // SDK client subscribed to `WS /test/events`. `Sender::send`
            // returns `Err(SendError)` when no client is attached — that is
            // expected and silently dropped.
            #[cfg(feature = "e2e")]
            {
                if let Some(handle) = server_slot.borrow().as_ref() {
                    let env = gtk4_e2e_server::EventEnvelope {
                        kind: gtk4_e2e_server::EventKind::StateChange,
                        ts: gtk4_e2e_server::current_rfc3339(),
                        data: json!({
                            "selector": "#label1",
                            "property": "label",
                            "value": new_text.as_str(),
                        }),
                    };
                    let _ = handle.event_tx().send(env);
                }

                // T019: app-defined state snapshot push. `/session/mode`,
                // `/label1/text`, and `/click_count` accumulate across taps so
                // scenarios can `wait { app_state_eq, path = "/click_count",
                // value = N }` after multiple Apply presses.
                {
                    let mut count = click_count.borrow_mut();
                    *count += 1;
                }
                set_at(&demo_state, "/session/mode", json!("applied"));
                set_at(&demo_state, "/label1/text", json!(new_text.as_str()));
                set_at(&demo_state, "/click_count", json!(*click_count.borrow()));
                push_state(&server_slot, &demo_state);
            }
        });
    }

    #[cfg(feature = "e2e")]
    {
        let server_slot = server_slot.clone();
        let demo_state = demo_state.clone();
        switch1.connect_active_notify(move |s| {
            set_at(&demo_state, "/switch1/active", json!(s.is_active()));
            set_at(
                &demo_state,
                "/session/mode",
                json!(if s.is_active() { "on" } else { "off" }),
            );
            push_state(&server_slot, &demo_state);
        });
    }
    #[cfg(feature = "e2e")]
    {
        let server_slot = server_slot.clone();
        let demo_state = demo_state.clone();
        check1.connect_active_notify(move |c| {
            set_at(&demo_state, "/check1/active", json!(c.is_active()));
            push_state(&server_slot, &demo_state);
        });
    }
    #[cfg(feature = "e2e")]
    {
        let server_slot = server_slot.clone();
        let demo_state = demo_state.clone();
        toggle1.connect_active_notify(move |t| {
            set_at(&demo_state, "/toggle1/active", json!(t.is_active()));
            push_state(&server_slot, &demo_state);
        });
    }

    // Step 9 (T014): ScrolledWindow + ListBox so `swipe` has something to
    // scroll, plus a `#scroll-pos` mirror label that surfaces
    // `vadjustment.value` as a string for `state_eq` assertions.
    let listbox = ListBox::new();
    listbox.set_widget_name("list1");
    // Plan §7.1 calls for 30 rows; observed `vadjustment.upper - page_size`
    // tops out at ~244 px on quartz / xvfb when `vexpand(true)` lets the
    // viewport eat the listbox real estate. We bump to 80 rows so a
    // dy=300 swipe stays comfortably inside the scrollable range — without
    // this the final value would clamp short and `state_eq label="300"`
    // never matches.
    for i in 0..80 {
        let row = Label::new(Some(&format!("Row {i}")));
        row.set_widget_name(&format!("row-{i}"));
        listbox.append(&row);
    }

    let scrolled = ScrolledWindow::builder()
        .height_request(200)
        .min_content_height(200)
        .vexpand(true)
        .child(&listbox)
        .build();
    scrolled.set_widget_name("scroll1");
    // Adjustment::set_value (used by `input::SwipeAnimation`) does not trigger
    // kinetic scrolling, but disabling it is a small defence against future
    // manual-drag tests bleeding momentum into deterministic assertions.
    scrolled.set_kinetic_scrolling(false);

    let scroll_pos = Label::new(Some("0"));
    scroll_pos.set_widget_name("scroll-pos");
    {
        let scroll_pos = scroll_pos.clone();
        scrolled.vadjustment().connect_value_changed(move |a| {
            scroll_pos.set_text(&format!("{}", a.value() as i32));
        });
    }

    let vbox = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(8)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .halign(Align::Fill)
        .build();
    vbox.append(&entry);
    vbox.append(&input1);
    vbox.append(&button);
    vbox.append(&label);
    vbox.append(&switch1);
    vbox.append(&check1);
    vbox.append(&toggle1);
    vbox.append(&scrolled);
    vbox.append(&scroll_pos);

    let window = ApplicationWindow::builder()
        .application(app)
        .title("gtk4-e2e demo")
        .default_width(360)
        .default_height(480)
        .child(&vbox)
        .build();

    window.present();
}

/// Set `value` at JSON Pointer `path` (e.g. `/switch1/active`) in `state`,
/// auto-creating intermediate object segments. Array index segments are not
/// supported — the demo only nests objects.
#[cfg(feature = "e2e")]
fn set_at(state: &Rc<RefCell<Value>>, path: &str, value: Value) {
    let mut s = state.borrow_mut();
    if path.is_empty() {
        *s = value;
        return;
    }
    if !s.is_object() {
        *s = json!({});
    }
    let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    let mut cur: &mut Value = &mut s;
    for seg in &parts[..parts.len() - 1] {
        let key = (*seg).to_string();
        let map = cur.as_object_mut().expect("intermediate must be object");
        let entry = map.entry(key).or_insert_with(|| json!({}));
        if !entry.is_object() {
            *entry = json!({});
        }
        cur = entry;
    }
    let last = parts.last().expect("path is non-empty");
    cur.as_object_mut()
        .expect("leaf parent must be object")
        .insert((*last).to_string(), value);
}

#[cfg(feature = "e2e")]
fn push_state(
    server_slot: &Rc<RefCell<Option<gtk4_e2e_server::Handle>>>,
    state: &Rc<RefCell<Value>>,
) {
    if let Some(handle) = server_slot.borrow().as_ref() {
        handle.set_state(state.borrow().clone());
    }
}
