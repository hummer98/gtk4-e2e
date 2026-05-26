//! GObject `notify::<property>` signal stream — push-side wiring for
//! `WS /test/events` (T026).
//!
//! Aplications opt-in by calling `Handle::register_property_stream(&widget)`
//! after building the widget tree. For each `(kind, property)` pair listed
//! in `ALLOWLIST` that matches the widget's `type_().name()`, this module
//! attaches a `connect_notify_local` handler that constructs a
//! `PropertyEventData` payload and broadcasts an `EventEnvelope` on the
//! server's `event_tx` bus.

use std::time::Instant;

use tokio::sync::broadcast;

use crate::gtk;
use crate::gtk::prelude::*;
use crate::proto::{EventEnvelope, EventKind, PropertyEventData};
use crate::start_impl::current_rfc3339;
use crate::wait::{sentinel_for, WidgetLike};

/// Static `(widget kind, property names)` allowlist consulted by
/// [`register`].
///
/// `kind` is the GObject type name (`widget.type_().name()`, e.g. `"GtkStack"`)
/// and each property entry is a GObject property name in its canonical
/// hyphen-separated form (e.g. `"visible-child-name"`).
///
/// The list intentionally tracks short-lived, user-driven UI state
/// transitions — pages, modes, toggles — rather than every readable
/// property of a widget. Extending it is a one-line change.
pub const ALLOWLIST: &[(&str, &[&str])] = &[
    ("GtkStack", &["visible-child-name", "visible-child"]),
    ("GtkSwitch", &["active", "state"]),
    ("GtkSpinner", &["spinning"]),
    ("GtkPicture", &["file", "paintable"]),
    ("GtkEntry", &["text"]),
    ("GtkNotebook", &["page"]),
    ("GtkRevealer", &["reveal-child", "child-revealed"]),
    ("GtkExpander", &["expanded"]),
];

/// `true` if `(kind, property)` is in [`ALLOWLIST`].
pub fn is_allowed(kind: &str, property: &str) -> bool {
    ALLOWLIST
        .iter()
        .any(|(k, props)| *k == kind && props.contains(&property))
}

/// Wire allowlisted `notify::<property>` signals on `widget` into `event_tx`.
///
/// Returns the list of property names that had a handler attached (empty
/// when `widget`'s `type_().name()` is not in [`ALLOWLIST`]).
///
/// **Visibility**: this is `pub` so integration tests in `tests/` can drive
/// the registration directly. Library users should prefer
/// [`Handle::register_property_stream`](crate::Handle::register_property_stream)
/// which threads the server's owned `event_tx` and `clock_base` for them.
///
/// **Thread**: must be called from the GLib main thread (the same thread
/// that ran `gtk::init()` / runs `app.run()`). The closure attached by
/// `connect_notify_local` fires synchronously on that thread.
///
/// **Idempotency**: not guaranteed. Calling `register` twice for the same
/// widget attaches two handlers and emits duplicate events. Callers must
/// register each widget at most once.
pub fn register(
    event_tx: &broadcast::Sender<EventEnvelope>,
    clock_base: Instant,
    widget: &gtk::Widget,
) -> Vec<String> {
    let kind = widget.type_().name().to_string();
    let mut registered = Vec::new();
    for (k, props) in ALLOWLIST {
        if *k != kind {
            continue;
        }
        for property in *props {
            let tx = event_tx.clone();
            let property_owned = property.to_string();
            let kind_owned = kind.clone();
            widget.connect_notify_local(Some(property), move |w, _pspec| {
                let value = w
                    .read_property_as_json(&property_owned)
                    .unwrap_or_else(sentinel_for);
                let data = PropertyEventData {
                    widget_id: format!("w{:x}", w.as_ptr() as usize),
                    widget_kind: kind_owned.clone(),
                    property: property_owned.clone(),
                    value,
                    ts_ns: clock_base.elapsed().as_nanos() as u64,
                };
                let env = EventEnvelope {
                    kind: EventKind::Property,
                    ts: current_rfc3339(),
                    data: serde_json::to_value(data).unwrap_or(serde_json::Value::Null),
                };
                let _ = tx.send(env);
            });
            registered.push(property.to_string());
        }
        break;
    }
    registered
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_allowed_recognises_known_pair() {
        assert!(is_allowed("GtkStack", "visible-child-name"));
        assert!(is_allowed("GtkSwitch", "active"));
    }

    #[test]
    fn is_allowed_rejects_unknown() {
        assert!(!is_allowed("GtkLabel", "label"));
        assert!(!is_allowed("GtkStack", "no-such-prop"));
    }
}
