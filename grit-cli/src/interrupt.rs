//! Ctrl+C as a library [`Cancel`] signal.
//!
//! Without a handler `SIGINT` kills the process mid-write, so library rollbacks never run.
//! While an [`InterruptCancel`] is installed the first `SIGINT` only asks the running
//! operation to stop (it then undoes its partial work and returns `Error::Cancelled`). A
//! second Ctrl+C terminates the process as usual, unless it arrives within
//! [`DUPLICATE_WINDOW_NS`] of the first: tools such as `timeout` signal both the child and
//! its process group, and that duplicate must not cut the rollback short.

use std::sync::atomic::{AtomicBool, Ordering};

use grit_lib::progress::Cancel;

static INTERRUPTED: AtomicBool = AtomicBool::new(false);

/// Monotonic time of the first `SIGINT`, in nanoseconds.
#[cfg(unix)]
static FIRST_INTERRUPT_NS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// A further `SIGINT` this soon after the first is the same interrupt delivered twice.
#[cfg(unix)]
const DUPLICATE_WINDOW_NS: u64 = 100_000_000;

/// Cancels the running library operation on the first Ctrl+C; restores the previous handler
/// when dropped.
pub struct InterruptCancel {
    #[cfg(unix)]
    previous: libc::sigaction,
}

#[cfg(unix)]
fn monotonic_ns() -> u64 {
    // SAFETY: `clock_gettime` is async-signal-safe and only writes the provided struct.
    let ts = unsafe {
        let mut ts: libc::timespec = std::mem::zeroed();
        libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts);
        ts
    };
    u64::try_from(ts.tv_sec).unwrap_or(0) * 1_000_000_000 + u64::try_from(ts.tv_nsec).unwrap_or(0)
}

#[cfg(unix)]
extern "C" fn on_interrupt(_signal: libc::c_int) {
    let now = monotonic_ns();
    if !INTERRUPTED.swap(true, Ordering::Relaxed) {
        FIRST_INTERRUPT_NS.store(now, Ordering::Relaxed);
        return;
    }
    if now.saturating_sub(FIRST_INTERRUPT_NS.load(Ordering::Relaxed)) < DUPLICATE_WINDOW_NS {
        return;
    }
    // SAFETY: `signal` and `raise` are async-signal-safe. SIGINT stays blocked until this
    // handler returns, so the default action then terminates the process.
    unsafe {
        libc::signal(libc::SIGINT, libc::SIG_DFL);
        libc::raise(libc::SIGINT);
    }
}

impl InterruptCancel {
    /// Route the next `SIGINT` to [`Cancel::is_cancelled`] instead of terminating.
    #[cfg(unix)]
    pub fn install() -> Self {
        INTERRUPTED.store(false, Ordering::Relaxed);
        // SAFETY: the handler only touches atomics and async-signal-safe libc calls, and
        // both `sigaction` structs are fully initialized before the call.
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = on_interrupt as extern "C" fn(libc::c_int) as libc::sighandler_t;
            libc::sigemptyset(&mut action.sa_mask);
            let mut previous: libc::sigaction = std::mem::zeroed();
            libc::sigaction(libc::SIGINT, &action, &mut previous);
            Self { previous }
        }
    }

    /// Ctrl+C keeps its default meaning on platforms without `SIGINT` handling here.
    #[cfg(not(unix))]
    pub fn install() -> Self {
        Self {}
    }
}

impl Cancel for InterruptCancel {
    fn is_cancelled(&self) -> bool {
        INTERRUPTED.load(Ordering::Relaxed)
    }
}

impl Drop for InterruptCancel {
    fn drop(&mut self) {
        #[cfg(unix)]
        // SAFETY: restores the action saved by `install`.
        unsafe {
            libc::sigaction(libc::SIGINT, &self.previous, std::ptr::null_mut());
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn current_sigint_handler() -> libc::sighandler_t {
        // SAFETY: querying the current action with a null new action.
        unsafe {
            let mut current: libc::sigaction = std::mem::zeroed();
            libc::sigaction(libc::SIGINT, std::ptr::null(), &mut current);
            current.sa_sigaction
        }
    }

    #[test]
    fn first_sigint_cancels_a_duplicate_is_absorbed_and_drop_restores_the_handler() {
        let before = current_sigint_handler();
        let cancel = InterruptCancel::install();
        assert!(!cancel.is_cancelled());
        // SAFETY: the installed handler only sets atomic flags for these two signals.
        unsafe {
            libc::raise(libc::SIGINT);
        }
        assert!(cancel.is_cancelled());
        // SAFETY: as above; a duplicate right after the first is absorbed.
        unsafe {
            libc::raise(libc::SIGINT);
        }
        assert!(cancel.is_cancelled());
        let ours = on_interrupt as extern "C" fn(libc::c_int) as libc::sighandler_t;
        assert_eq!(
            current_sigint_handler(),
            ours,
            "the handler stays installed so a later Ctrl+C can terminate"
        );
        drop(cancel);
        assert_eq!(current_sigint_handler(), before);
    }
}
