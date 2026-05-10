//! Minimal GTK4 demo embedding `gtk4-e2e-server` behind the `e2e` feature.
//!
//! See `docs/seed.md` §6 Step 2 and `docs/adr/0001-architecture.md`.

#[cfg(feature = "e2e")]
use std::cell::RefCell;
#[cfg(feature = "e2e")]
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{
    Align, Application, ApplicationWindow, Box as GtkBox, Button, Entry, Label, Orientation,
};

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

fn build_ui(app: &Application) {
    // Plan §Q13 / Review M6: Entry initial text must be `"hello"` so the
    // scenario expectation (`state_eq label1.label = "hello"`) becomes true
    // as soon as the button is tapped.
    let entry = Entry::builder().text("hello").build();
    entry.set_widget_name("entry1");
    let label = Label::builder().label("waiting...").build();
    label.set_widget_name("label1");
    let button = Button::with_label("Apply");
    button.set_widget_name("btn1");

    {
        let entry = entry.clone();
        let label = label.clone();
        button.connect_clicked(move |_| {
            label.set_text(entry.text().as_str());
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
    vbox.append(&button);
    vbox.append(&label);

    let window = ApplicationWindow::builder()
        .application(app)
        .title("gtk4-e2e demo")
        .default_width(360)
        .default_height(200)
        .child(&vbox)
        .build();

    window.present();
}
