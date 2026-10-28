//! Localhost networking helpers for tests that start servers.

use std::net::TcpListener;
use std::sync::Mutex;

/// Reserve a localhost TCP port that no other caller in this process has been given.
///
/// The port is found by binding `127.0.0.1:0` and dropping the listener, so a
/// server started afterwards can bind it. Ports are remembered process-wide:
/// test modules compiled into one binary share the list, so two concurrently
/// starting servers are never handed the same port. Returns `None` when no
/// listener can be bound or no fresh port turns up after 200 tries.
#[must_use]
pub fn free_port() -> Option<u16> {
    static HANDED_OUT: Mutex<Vec<u16>> = Mutex::new(Vec::new());
    let mut handed_out = HANDED_OUT.lock().unwrap_or_else(|e| e.into_inner());
    for _ in 0..200 {
        let port = TcpListener::bind(("127.0.0.1", 0))
            .ok()?
            .local_addr()
            .ok()?
            .port();
        if !handed_out.contains(&port) {
            handed_out.push(port);
            return Some(port);
        }
    }
    None
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::free_port;

    #[test]
    fn free_port_never_repeats_within_a_process() {
        let ports: Vec<u16> = (0..32).map(|_| free_port().expect("port")).collect();
        let mut unique = ports.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), ports.len(), "{ports:?}");
    }

    #[test]
    fn free_port_is_bindable() {
        let port = free_port().expect("port");
        std::net::TcpListener::bind(("127.0.0.1", port)).expect("bind reserved port");
    }
}
