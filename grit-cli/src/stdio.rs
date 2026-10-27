//! Stdout behavior when the reader closes early (e.g. `grit log | head`).
//!
//! Rust ignores `SIGPIPE` by default, so writes to a closed pipe return
//! `std::io::ErrorKind::BrokenPipe` and `println!` panics. Git restores the default
//! handler so the process terminates with signal 13 (exit 141) instead of
//! printing a backtrace.

#[cfg(unix)]
pub fn configure() {
    // SAFETY: `signal(SIGPIPE, SIG_DFL)` is async-signal-safe on POSIX.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

#[cfg(not(unix))]
pub fn configure() {}

/// Run `f` with `SIGPIPE` ignored, so a write to a pipe whose reader is gone (a `pre-push`
/// hook that exits without reading its stdin, a transport helper that hangs up) fails with
/// `BrokenPipe` instead of killing the process. Git ignores `SIGPIPE` while pushing and while
/// talking to credential helpers too; `main` runs every command that reaches a remote this way.
#[cfg(unix)]
pub fn with_sigpipe_ignored<T>(f: impl FnOnce() -> T) -> T {
    // SAFETY: `signal` only swaps this process's `SIGPIPE` disposition; the previous one is
    // restored before returning.
    let previous = unsafe { libc::signal(libc::SIGPIPE, libc::SIG_IGN) };
    let out = f();
    unsafe {
        libc::signal(libc::SIGPIPE, previous);
    }
    out
}

/// Run `f`; there is no `SIGPIPE` to ignore on this platform.
#[cfg(not(unix))]
pub fn with_sigpipe_ignored<T>(f: impl FnOnce() -> T) -> T {
    f()
}

/// Treat a closed stdout pipe as success (used for `--json` and flush paths).
pub(crate) fn io_result(result: std::io::Result<()>) -> std::io::Result<()> {
    match result {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        Err(e) => Err(e),
    }
}
