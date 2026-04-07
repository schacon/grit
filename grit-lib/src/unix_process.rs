//! Unix-only process helpers (FFI).

/// Returns whether process `pid` exists (same semantics as `kill(pid, 0)`).
///
/// On success of `kill`, the process exists (or we lack permission; treated as alive).
#[must_use]
pub fn pid_is_alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
}
