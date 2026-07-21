//! Minimal GTK4 demo embedding `gtk4-e2e-server` behind the `e2e` feature.
//!
//! See `docs/seed.md` §6 Step 2 and `docs/adr/0001-architecture.md`.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{
    Align, Application, ApplicationWindow, Box as GtkBox, Button, CheckButton, DrawingArea, Entry,
    GestureDrag, GestureLongPress, GestureZoom, Label, ListBox, Orientation, Scale, ScrolledWindow,
    Stack, StackSwitcher, Switch, ToggleButton,
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
            {
                // `activate` may fire multiple times (e.g. primary instance
                // handover). Start the server on the first activation so
                // `build_ui` can wire `register_property_stream` (T026) on
                // freshly constructed widgets.
                if server_slot.borrow().is_none() {
                    let handle = gtk4_e2e_server::start(app);
                    eprintln!(
                        "[gtk4-e2e-demo] server up on http://127.0.0.1:{}/test/info",
                        handle.port()
                    );
                    *server_slot.borrow_mut() = Some(handle);
                }
            }

            #[cfg(feature = "e2e")]
            build_ui(app, server_slot.clone());
            #[cfg(not(feature = "e2e"))]
            build_ui(app);
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

    // Step 9 (c) (T015): DrawingArea + GestureZoom for `pinch` capability.
    // `current_scale` is shared between the gesture handler and the draw_func.
    // The clamp range below is intentionally wider than the server-side
    // `MAX_PINCH_SCALE = 50` — it acts as a 2nd line of defence against
    // accidental UI-side blow-up if a future caller bypasses validation.
    let current_scale: Rc<RefCell<f64>> = Rc::new(RefCell::new(1.0));

    let drawing_area = DrawingArea::builder()
        .content_width(160)
        .content_height(120)
        .hexpand(false)
        .vexpand(false)
        .build();
    drawing_area.set_widget_name("zoom1");
    {
        let current_scale = current_scale.clone();
        drawing_area.set_draw_func(move |_da, cr, w, h| {
            let s = *current_scale.borrow();
            let cx = w as f64 / 2.0;
            let cy = h as f64 / 2.0;
            cr.translate(cx, cy);
            cr.scale(s, s);
            cr.set_source_rgb(0.2, 0.4, 0.9);
            cr.arc(0.0, 0.0, 30.0, 0.0, std::f64::consts::TAU);
            let _ = cr.fill();
        });
    }

    let zoom_pos = Label::new(Some("1.00"));
    zoom_pos.set_widget_name("zoom-pos");

    // T026: GtkStack so `WS /test/events` (kind=property) has a default
    // transition source to demonstrate. Each named page is a Label; flipping
    // the visible page via the toggle button below fires `notify::visible-
    // child-name`, which `register_property_stream` forwards to the bus.
    let mode_stack = Stack::builder().build();
    mode_stack.set_widget_name("mode-stack");
    let mode1_page = Label::new(Some("mode1 content"));
    let mode2_page = Label::new(Some("mode2 content"));
    mode_stack.add_named(&mode1_page, Some("mode1"));
    mode_stack.add_named(&mode2_page, Some("mode2"));

    // issue #12: a StackSwitcher driving `mode_stack`. Its tabs are
    // auto-generated, *unnamed* `GtkToggleButton`s whose content is a
    // `GtkBox` + `GtkLabel` — the exact composite that defeats both selector
    // taps (no widget_name) and naive xy taps (leaf is the GtkBox/GtkLabel).
    // With the nearest-activatable-ancestor fallback, an xy tap on a tab centre
    // retargets to the ToggleButton and switches the page (observable via the
    // existing `mode-stack` `visible-child-name` property stream).
    let mode_switcher = StackSwitcher::builder().stack(&mode_stack).build();
    mode_switcher.set_widget_name("mode-switcher");

    let mode_toggle = Button::with_label("Toggle mode");
    mode_toggle.set_widget_name("mode-toggle");
    {
        let mode_stack = mode_stack.clone();
        mode_toggle.connect_clicked(move |_| {
            let next = match mode_stack.visible_child_name().as_deref() {
                Some("mode2") => "mode1",
                _ => "mode2",
            };
            mode_stack.set_visible_child_name(next);
        });
    }

    // T026: surface `notify::visible-child-name` on the bus as
    // `EventKind::Property`. `register_property_stream` is a no-op when no
    // server is attached (e.g. demo built without `--features e2e`).
    #[cfg(feature = "e2e")]
    if let Some(handle) = server_slot.borrow().as_ref() {
        handle.register_property_stream(mode_stack.upcast_ref());
        handle.register_property_stream(switch1.upcast_ref());
    }

    let gesture = GestureZoom::new();
    {
        let current_scale = current_scale.clone();
        let zoom_pos = zoom_pos.clone();
        let drawing_area_w = drawing_area.clone();
        gesture.connect_scale_changed(move |_g, scale| {
            // 2-stage defence: server validates `MAX_PINCH_SCALE = 50`, the
            // demo additionally clamps in case anything bypasses validation.
            let clamped = scale.clamp(0.05, 100.0);
            *current_scale.borrow_mut() = clamped;
            zoom_pos.set_text(&format!("{:.2}", clamped));
            drawing_area_w.queue_draw();
        });
    }
    drawing_area.add_controller(gesture);

    // Task 029 (T029): DrawingArea + GestureLongPress for the `press`
    // capability. `connect_pressed` fires when the server injects a
    // press → hold → release sequence; the handler bumps an app-state counter
    // and a `fired` flag so scenarios can `wait { app_state_eq,
    // path = "/longpress1/fired", value = true }`. Mirror of the `#zoom1`
    // pinch widget above. The emit coordinates are informational only.
    let longpress_area = DrawingArea::builder()
        .content_width(160)
        .content_height(80)
        .hexpand(false)
        .vexpand(false)
        .build();
    longpress_area.set_widget_name("longpress1");

    let longpress_gesture = GestureLongPress::new();
    #[cfg(feature = "e2e")]
    {
        let server_slot = server_slot.clone();
        let demo_state = demo_state.clone();
        let press_count: Rc<RefCell<u64>> = Rc::new(RefCell::new(0));
        longpress_gesture.connect_pressed(move |_g, _x, _y| {
            {
                let mut count = press_count.borrow_mut();
                *count += 1;
            }
            set_at(&demo_state, "/longpress1/fired", json!(true));
            set_at(
                &demo_state,
                "/longpress1/count",
                json!(*press_count.borrow()),
            );
            push_state(&server_slot, &demo_state);
        });
    }
    longpress_area.add_controller(longpress_gesture);

    // touch-drag capability (issue #13): a DrawingArea with a GtkGestureDrag,
    // standing in for a radial / pie menu. `POST /test/touch-drag` drives the
    // gesture's `drag-begin` → `drag-update`×N → `drag-end` as one sequence.
    // Handlers record the phase + last cumulative offset into app-state so
    // scenarios can `wait { app_state_eq, path = "/touchdrag1/phase",
    // value = "end" }` and assert the release direction via
    // `/touchdrag1/offset_{x,y}`.
    let touchdrag_area = DrawingArea::builder()
        .content_width(160)
        .content_height(120)
        .hexpand(false)
        .vexpand(false)
        .build();
    touchdrag_area.set_widget_name("touchdrag1");

    let drag_gesture = GestureDrag::new();
    #[cfg(feature = "e2e")]
    {
        let server_slot = server_slot.clone();
        let demo_state = demo_state.clone();
        drag_gesture.connect_drag_begin(move |_g, x, y| {
            set_at(&demo_state, "/touchdrag1/phase", json!("begin"));
            set_at(&demo_state, "/touchdrag1/start_x", json!(x));
            set_at(&demo_state, "/touchdrag1/start_y", json!(y));
            set_at(&demo_state, "/touchdrag1/update_count", json!(0));
            push_state(&server_slot, &demo_state);
        });
    }
    #[cfg(feature = "e2e")]
    {
        let server_slot = server_slot.clone();
        let demo_state = demo_state.clone();
        let update_count: Rc<RefCell<u64>> = Rc::new(RefCell::new(0));
        drag_gesture.connect_drag_update(move |_g, ox, oy| {
            {
                let mut c = update_count.borrow_mut();
                *c += 1;
            }
            set_at(&demo_state, "/touchdrag1/phase", json!("update"));
            set_at(&demo_state, "/touchdrag1/offset_x", json!(ox));
            set_at(&demo_state, "/touchdrag1/offset_y", json!(oy));
            set_at(
                &demo_state,
                "/touchdrag1/update_count",
                json!(*update_count.borrow()),
            );
            push_state(&server_slot, &demo_state);
        });
    }
    #[cfg(feature = "e2e")]
    {
        let server_slot = server_slot.clone();
        let demo_state = demo_state.clone();
        drag_gesture.connect_drag_end(move |_g, ox, oy| {
            set_at(&demo_state, "/touchdrag1/phase", json!("end"));
            set_at(&demo_state, "/touchdrag1/offset_x", json!(ox));
            set_at(&demo_state, "/touchdrag1/offset_y", json!(oy));
            push_state(&server_slot, &demo_state);
        });
    }
    touchdrag_area.add_controller(drag_gesture);

    // set-value capability: a horizontal GtkScale (0..100) so `POST
    // /test/set-value` has a GtkRange to drive. `connect_value_changed` mirrors
    // the current value into `#scale-pos` (for `state_eq` assertions) and into
    // the app-state snapshot at `/scale1/value`, so scenarios can
    // `wait { app_state_eq, path = "/scale1/value", value = N }`.
    let scale1 = Scale::with_range(Orientation::Horizontal, 0.0, 100.0, 1.0);
    scale1.set_widget_name("scale1");
    scale1.set_hexpand(true);
    scale1.set_draw_value(false);

    let scale_pos = Label::new(Some("0"));
    scale_pos.set_widget_name("scale-pos");
    {
        let scale_pos = scale_pos.clone();
        #[cfg(feature = "e2e")]
        let server_slot = server_slot.clone();
        #[cfg(feature = "e2e")]
        let demo_state = demo_state.clone();
        scale1.connect_value_changed(move |s| {
            let v = s.value();
            scale_pos.set_text(&format!("{}", v as i32));
            #[cfg(feature = "e2e")]
            {
                set_at(&demo_state, "/scale1/value", json!(v));
                push_state(&server_slot, &demo_state);
            }
        });
    }

    // issue #10: autohide (modal) Popover mimicking the Brainship delete-confirm
    // dialog. Tapping `#open-popover` pops it up; it grabs modally. Inside are
    // `#popover-confirm` / `#popover-cancel` buttons. Both pop the popover down
    // *synchronously inside their `clicked` handler* — the exact pattern that,
    // on Wayland, used to re-enter the modal grab and stall the e2e tap reply.
    // The server now defers the tap action to a GLib idle callback so the reply
    // is always sent and the grab tears down cleanly.
    let confirm_btn = Button::with_label("Delete");
    confirm_btn.set_widget_name("popover-confirm");
    let cancel_btn = Button::with_label("Cancel");
    cancel_btn.set_widget_name("popover-cancel");

    let popover_box = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(8)
        .build();
    popover_box.append(&Label::new(Some("Delete this item?")));
    popover_box.append(&confirm_btn);
    popover_box.append(&cancel_btn);

    let confirm_popover = gtk4::Popover::builder().autohide(true).build();
    confirm_popover.set_widget_name("confirm-popover");
    confirm_popover.set_child(Some(&popover_box));

    let open_popover_btn = Button::with_label("Open confirm");
    open_popover_btn.set_widget_name("open-popover");
    confirm_popover.set_parent(&open_popover_btn);
    {
        let confirm_popover = confirm_popover.clone();
        open_popover_btn.connect_clicked(move |_| {
            confirm_popover.popup();
        });
    }
    {
        let confirm_popover = confirm_popover.clone();
        #[cfg(feature = "e2e")]
        let server_slot = server_slot.clone();
        #[cfg(feature = "e2e")]
        let demo_state = demo_state.clone();
        confirm_btn.connect_clicked(move |_| {
            // Synchronous popdown inside the handler — the grab-reentrant shape.
            confirm_popover.popdown();
            #[cfg(feature = "e2e")]
            {
                set_at(&demo_state, "/confirm/result", json!("deleted"));
                push_state(&server_slot, &demo_state);
            }
        });
    }
    {
        let confirm_popover = confirm_popover.clone();
        #[cfg(feature = "e2e")]
        let server_slot = server_slot.clone();
        #[cfg(feature = "e2e")]
        let demo_state = demo_state.clone();
        cancel_btn.connect_clicked(move |_| {
            confirm_popover.popdown();
            #[cfg(feature = "e2e")]
            {
                set_at(&demo_state, "/confirm/result", json!("cancelled"));
                push_state(&server_slot, &demo_state);
            }
        });
    }

    // ADR-0004: a Popover anchored near the *top* of the window so
    // `GET /test/elements`'s cross-surface bounds composition can be verified in
    // CI. It must be top-anchored: the demo window is taller than the CI xvfb
    // screen (720px), so a bottom-anchored popover (e.g. `#open-popover` above)
    // would open off-screen and never map, leaving the composition path
    // uncovered. Plain Button (not MenuButton) so a scenario can
    // `tap("#bounds-popover-btn")` to open it; the scenario only taps once (to
    // open) and then reads bounds over HTTP, so the modal grab never dismisses.
    //
    // Autohide is left ON (the default). A non-autohide popover did NOT yield
    // composed bounds under xvfb/X11 — its surface is not realized as a
    // GdkPopup, so `popover_root_frame` returns None and `bounds` comes back
    // null (it composed fine on macOS/quartz, masking the gap). A modal popover
    // is realized as a proper popup surface on both backends. See ADR-0004 m7.
    //
    // Starts closed, so it is absent from the visual-regression baseline frame —
    // but its trigger button shifts the layout, hence the baseline is
    // regenerated alongside this change.
    let bounds_popover_label = Label::new(Some("bounds probe"));
    bounds_popover_label.set_widget_name("bounds-popover-content");
    let bounds_popover = gtk4::Popover::builder()
        .child(&bounds_popover_label)
        .build();
    bounds_popover.set_widget_name("bounds-popover");
    let bounds_popover_btn = Button::with_label("Bounds probe");
    bounds_popover_btn.set_widget_name("bounds-popover-btn");
    bounds_popover.set_parent(&bounds_popover_btn);
    {
        let bounds_popover = bounds_popover.clone();
        bounds_popover_btn.connect_clicked(move |_| bounds_popover.popup());
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
    vbox.append(&bounds_popover_btn);
    vbox.append(&entry);
    vbox.append(&input1);
    vbox.append(&button);
    vbox.append(&label);
    vbox.append(&switch1);
    vbox.append(&check1);
    vbox.append(&toggle1);
    vbox.append(&scrolled);
    vbox.append(&scroll_pos);
    vbox.append(&drawing_area);
    vbox.append(&zoom_pos);
    vbox.append(&longpress_area);
    vbox.append(&touchdrag_area);
    vbox.append(&scale1);
    vbox.append(&scale_pos);
    vbox.append(&mode_switcher);
    vbox.append(&mode_stack);
    vbox.append(&mode_toggle);
    vbox.append(&open_popover_btn);

    let window = ApplicationWindow::builder()
        .application(app)
        .title("gtk4-e2e demo")
        .default_width(360)
        // Step 9 (c) (T015): the pinch DrawingArea + zoom-pos Label add ~140 px
        // below the ScrolledWindow. Bump default_height from 480 to 700 so
        // ScrolledWindow keeps enough headroom for swipe scenarios that
        // address y=400 (scroll widget would otherwise clamp to its
        // min_content_height and y=400 would land in the pinch widgets).
        .default_height(700)
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
