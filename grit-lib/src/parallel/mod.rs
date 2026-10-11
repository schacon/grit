//! Ordered scoped-thread parallelism and Git config-driven thread policy.
//!
//! There is no global pool and no environment reads in this module. Embedders and
//! the CLI pass an explicit [`ThreadPolicy`] (for example `--threads`) when opening
//! a [`crate::repo::Repository`].

mod map;
mod policy;
mod pool;

pub use map::{
    par_map, parallel_worthwhile, try_par_map, try_par_map_force, try_par_map_with_local,
};
pub use policy::{ParallelOp, ParallelOptions, ThreadPolicy};

use std::num::NonZeroUsize;

/// Failure from fallible parallel helpers when a closure returns an error.
///
/// If a worker panics, partial results are dropped and the panic is resumed on the
/// caller thread after workers join.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParallelHashError<E> {
    /// The first error returned by a worker (additional concurrent errors are discarded).
    Task(E),
}

impl<E: std::fmt::Display> std::fmt::Display for ParallelHashError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Task(e) => write!(f, "{e}"),
        }
    }
}

impl<E: std::fmt::Debug + std::fmt::Display> std::error::Error for ParallelHashError<E> {}

/// Resolved worker thread count for parallel helpers.
///
/// Config keys are interpreted by [`ThreadPolicy::resolve`]; this type only clamps
/// an optional request to [`std::thread::available_parallelism`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Parallelism {
    threads: NonZeroUsize,
}

impl Parallelism {
    /// Use exactly `threads` workers (must be at least 1).
    #[must_use]
    pub fn new(threads: NonZeroUsize) -> Self {
        Self { threads }
    }

    /// Resolve a thread count from config (`None` or `Some(0)` → all logical CPUs).
    #[must_use]
    pub fn resolve(requested: Option<usize>) -> Self {
        let avail = std::thread::available_parallelism()
            .map(NonZeroUsize::get)
            .unwrap_or(1);
        let n = match requested {
            None | Some(0) => avail,
            Some(n) => n.min(avail),
        };
        Self {
            threads: NonZeroUsize::new(n.max(1)).unwrap_or(NonZeroUsize::MIN),
        }
    }

    /// Explicit worker count passed to parallel entry points.
    #[must_use]
    pub fn threads(self) -> NonZeroUsize {
        self.threads
    }
}

/// Worker count for index preload / parallel worktree hashing from config alone.
#[must_use]
pub fn index_parallelism_from_config(config: &crate::config::ConfigSet) -> Parallelism {
    ThreadPolicy::default()
        .resolve(ParallelOp::IndexRefresh, config)
        .parallelism()
}

/// Lazy name-hash thread count from config and Git's `LAZY_THREAD_COST` thresholds.
#[must_use]
pub fn index_load_lazy_thread_count(
    policy: &ThreadPolicy,
    config: &crate::config::ConfigSet,
    cache_nr: usize,
    try_threaded: bool,
) -> usize {
    if !try_threaded {
        return 0;
    }
    let opts = policy.resolve(ParallelOp::IndexLoad, config);
    if opts.threads.get() <= 1 {
        return 0;
    }
    if cache_nr < 2 * crate::index_name_hash_lazy::LAZY_THREAD_COST {
        return 0;
    }
    let mut nr = opts.threads.get();
    if cache_nr < nr * crate::index_name_hash_lazy::LAZY_THREAD_COST {
        nr = cache_nr / crate::index_name_hash_lazy::LAZY_THREAD_COST;
    }
    nr
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    fn threads(n: usize) -> NonZeroUsize {
        NonZeroUsize::new(n).expect("non-zero threads")
    }

    #[test]
    fn try_par_map_preserves_order() {
        let items: Vec<usize> = (0..256).collect();
        let opts = ParallelOptions {
            threads: threads(4),
            min_items: 0,
            min_total_bytes: None,
        };
        let got = par_map(&items, opts, items.len(), |&x| x * 2);
        let expect: Vec<_> = items.iter().map(|x| x * 2).collect();
        assert_eq!(got, expect);
    }

    #[test]
    fn try_par_map_surfaces_task_error() {
        let items: Vec<i32> = (0..128).collect();
        let opts = ParallelOptions {
            threads: threads(4),
            min_items: 0,
            min_total_bytes: None,
        };
        let err = try_par_map(&items, opts, items.len(), |&x| {
            if x == 64 {
                Err("boom")
            } else {
                Ok(x)
            }
        })
        .unwrap_err();
        assert_eq!(err, ParallelHashError::Task("boom"));
    }

    #[test]
    fn try_par_map_with_local_runs() {
        let items: Vec<i32> = (0..64).collect();
        let opts = ParallelOptions {
            threads: threads(4),
            min_items: 0,
            min_total_bytes: None,
        };
        let counter = Arc::new(AtomicUsize::new(0));
        let got = try_par_map_with_local(
            &items,
            opts,
            items.len(),
            || 0_i32,
            |state, &x| -> Result<i32, ()> {
                *state += 1;
                Ok(x + *state)
            },
        )
        .expect("map");
        assert_eq!(got.len(), items.len());
        assert!(counter.load(Ordering::Relaxed) <= got.len());
    }
}
