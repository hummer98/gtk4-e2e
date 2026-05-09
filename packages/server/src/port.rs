//! Free TCP port acquisition for the in-process e2e server.
//!
//! Returns a successfully `bind`ed `std::net::TcpListener` together with its
//! port number so that the caller can hand the listener directly to axum
//! without re-binding (avoiding the classic check-then-bind race).

use rand::seq::SliceRandom;
use std::net::TcpListener;

/// Default port range reserved for the e2e server (per ADR-0001).
pub const DEFAULT_PORT_RANGE: std::ops::RangeInclusive<u16> = 19000..=19999;

/// Pick a free port in `19000..=19999` and return its `(port, TcpListener)`.
///
/// The listener is set to non-blocking so it can be passed to
/// `tokio::net::TcpListener::from_std`.
pub fn pick_free_listener() -> std::io::Result<(u16, TcpListener)> {
    pick_free_listener_in_range(DEFAULT_PORT_RANGE)
}

/// Same as [`pick_free_listener`] but with a caller-supplied port range.
/// Exposed primarily for testing.
pub fn pick_free_listener_in_range<I>(range: I) -> std::io::Result<(u16, TcpListener)>
where
    I: IntoIterator<Item = u16>,
{
    let mut ports: Vec<u16> = range.into_iter().collect();
    ports.shuffle(&mut rand::thread_rng());
    let mut last_err: Option<std::io::Error> = None;
    for p in ports {
        match TcpListener::bind(("127.0.0.1", p)) {
            Ok(l) => {
                l.set_nonblocking(true)?;
                return Ok((p, l));
            }
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::AddrNotAvailable,
            "no free port in range",
        )
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn picks_port_in_range() {
        let (port, listener) = pick_free_listener().expect("at least one free port");
        assert!(
            DEFAULT_PORT_RANGE.contains(&port),
            "port {port} not in {DEFAULT_PORT_RANGE:?}"
        );
        let local = listener.local_addr().expect("local addr");
        assert_eq!(local.ip().to_string(), "127.0.0.1");
        assert_eq!(local.port(), port);
    }

    #[test]
    fn skips_bound_port() {
        // Acquire two ephemeral ports, hold one, release the other.
        // The held port must be skipped; the released port must be returned.
        let held = TcpListener::bind(("127.0.0.1", 0)).expect("ephemeral 1");
        let held_port = held.local_addr().unwrap().port();
        let scratch = TcpListener::bind(("127.0.0.1", 0)).expect("ephemeral 2");
        let free_port = scratch.local_addr().unwrap().port();
        drop(scratch);

        let (got, _l) =
            pick_free_listener_in_range([held_port, free_port]).expect("free_port should bind");
        assert_eq!(got, free_port);
        assert_ne!(got, held_port);
    }

    #[test]
    fn empty_range_returns_err() {
        let err = pick_free_listener_in_range(std::iter::empty::<u16>())
            .expect_err("empty range must err");
        assert_eq!(err.kind(), std::io::ErrorKind::AddrNotAvailable);
    }
}
