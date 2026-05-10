//! WebSocket layer for `/test/events` (Step 7).
//!
//! Each connection subscribes to a tokio `broadcast` bus owned by the server.
//! Subscribers may filter by event kind via the `?kinds=csv` query parameter;
//! an unset / empty filter forwards every kind. Filter evaluation is done
//! server-side so lagging clients don't waste bandwidth on dropped frames.
//!
//! Plan §4.2 / §4.6.

use std::collections::HashSet;

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    response::Response,
};
use futures_util::{sink::SinkExt, stream::StreamExt};
use serde::Deserialize;
use tokio::sync::broadcast;

use crate::http::AppState;
use crate::proto::{EventEnvelope, EventKind};

/// Query parameters accepted by `GET /test/events` upgrade.
#[derive(Debug, Default, Deserialize)]
pub struct EventsQuery {
    /// Comma-separated list of `EventKind` snake_case names to subscribe to.
    /// Unset = subscribe to every kind. Unknown tokens are ignored with a
    /// warning (forward-compatible with future server versions adding kinds).
    pub kinds: Option<String>,
}

/// Parse the CSV `?kinds=` parameter into a HashSet.
///
/// Empty / `None` produces an empty set, which means "no filter" — see
/// [`should_forward`].
pub fn parse_kinds(s: Option<&str>) -> HashSet<EventKind> {
    let mut set = HashSet::new();
    let Some(s) = s else { return set };
    for tok in s.split(',') {
        let tok = tok.trim();
        if tok.is_empty() {
            continue;
        }
        match serde_json::from_value::<EventKind>(serde_json::Value::String(tok.to_string())) {
            Ok(k) => {
                set.insert(k);
            }
            Err(_) => {
                eprintln!("[gtk4-e2e ws] ignoring unknown event kind: {tok}");
            }
        }
    }
    set
}

/// Server-side filter check. Returns `true` when the envelope should be
/// forwarded to a subscriber whose filter is `filter`.
///
/// Empty `filter` means "subscribe to every kind", matching the unset-query
/// case in [`parse_kinds`].
pub fn should_forward(env: &EventEnvelope, filter: &HashSet<EventKind>) -> bool {
    filter.is_empty() || filter.contains(&env.kind)
}

/// `GET /test/events` handler — upgrades to WebSocket and streams envelopes.
pub async fn ws_events(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Query(params): Query<EventsQuery>,
) -> Response {
    let filter = parse_kinds(params.kinds.as_deref());
    let rx = state.event_tx.subscribe();
    ws.on_upgrade(move |socket| handle_socket(socket, filter, rx))
}

async fn handle_socket(
    socket: WebSocket,
    filter: HashSet<EventKind>,
    mut rx: broadcast::Receiver<EventEnvelope>,
) {
    let (mut sender, mut receiver) = socket.split();
    loop {
        tokio::select! {
            recv_result = rx.recv() => match recv_result {
                Ok(env) => {
                    if !should_forward(&env, &filter) {
                        continue;
                    }
                    let json = match serde_json::to_string(&env) {
                        Ok(s) => s,
                        Err(e) => {
                            eprintln!("[gtk4-e2e ws] serialize failed: {e}");
                            continue;
                        }
                    };
                    if sender.send(Message::Text(json)).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    eprintln!("[gtk4-e2e ws] subscriber lagged {n} events");
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => break,
            },
            client_msg = receiver.next() => match client_msg {
                Some(Ok(Message::Close(_))) | None => break,
                Some(Ok(_)) => continue,
                Some(Err(_)) => break,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn env(kind: EventKind) -> EventEnvelope {
        EventEnvelope {
            kind,
            ts: "1970-01-01T00:00:00Z".into(),
            data: json!({}),
        }
    }

    #[test]
    fn parse_kinds_handles_empty_and_none() {
        assert!(parse_kinds(None).is_empty());
        assert!(parse_kinds(Some("")).is_empty());
        assert!(parse_kinds(Some("  ")).is_empty());
    }

    #[test]
    fn parse_kinds_accepts_known_csv() {
        let set = parse_kinds(Some("state_change,log_line"));
        assert!(set.contains(&EventKind::StateChange));
        assert!(set.contains(&EventKind::LogLine));
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn parse_kinds_ignores_unknown_tokens() {
        let set = parse_kinds(Some("state_change,bogus,,log_line"));
        assert!(set.contains(&EventKind::StateChange));
        assert!(set.contains(&EventKind::LogLine));
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn should_forward_empty_filter_passes_all() {
        let filter = HashSet::new();
        assert!(should_forward(&env(EventKind::StateChange), &filter));
        assert!(should_forward(&env(EventKind::LogLine), &filter));
    }

    #[test]
    fn should_forward_drops_unmatched_kinds() {
        let mut filter = HashSet::new();
        filter.insert(EventKind::LogLine);
        assert!(!should_forward(&env(EventKind::StateChange), &filter));
        assert!(should_forward(&env(EventKind::LogLine), &filter));
    }
}
