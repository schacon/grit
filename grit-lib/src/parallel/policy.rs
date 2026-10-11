//! Resolve worker counts and batch thresholds from Git config and explicit overrides.

use std::num::NonZeroUsize;

use crate::config::ConfigSet;
use crate::worktree_scan::PARALLEL_STAT_MIN_ENTRIES;

/// Minimum item count before parallel hash helpers use more than one thread.
const PAR_HASH_MIN_ITEMS: usize = 32;
/// Minimum total payload bytes for parallel hashing.
const PAR_HASH_MIN_TOTAL_BYTES: usize = 256 * 1024;

use super::Parallelism;

/// Git operation that selects which config keys apply to parallelism.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ParallelOp {
    /// Index stat preload and parallel worktree hashing (`index.threads`, `core.preloadIndex`).
    IndexRefresh,
    /// Parallel index load paths such as lazy name-hash init (`index.threads`, `core.preloadIndex`).
    IndexLoad,
    /// Checkout worker pool (`checkout.workers`, `checkout.thresholdForParallelism`).
    Checkout,
    /// `index-pack` and in-memory pack indexing (`pack.threads`).
    IndexPack,
    /// Pack writing / `pack-objects` threading (`pack.threads`).
    PackWrite,
    /// Rename/copy detection scoring (`index.threads` when preload is enabled).
    DiffRename,
}

/// Explicit CLI or embedder overrides layered on top of config resolution.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ThreadPolicy {
    /// When set, replaces config-derived thread counts (`0` = auto / all logical CPUs).
    pub threads_override: Option<usize>,
    /// When set, replaces the operation's default minimum item count before using a pool.
    pub min_items_override: Option<usize>,
}

impl ThreadPolicy {
    /// Override worker count (`None` = use config only; `Some(0)` = auto).
    #[must_use]
    pub fn with_threads_override(threads: Option<usize>) -> Self {
        Self {
            threads_override: threads,
            ..Self::default()
        }
    }

    /// Override minimum batch size (`Some(0)` forces the parallel path on tiny inputs).
    #[must_use]
    pub fn with_min_items_override(min_items: Option<usize>) -> Self {
        Self {
            min_items_override: min_items,
            ..Self::default()
        }
    }

    /// Set minimum batch override on an existing policy.
    #[must_use]
    pub fn min_items_override(mut self, min_items: Option<usize>) -> Self {
        self.min_items_override = min_items;
        self
    }

    /// Resolve pool options for `op` using merged `config` and this policy.
    #[must_use]
    pub fn resolve(&self, op: ParallelOp, config: &ConfigSet) -> ParallelOptions {
        let preload = core_preload_index(config);
        let threads = match op {
            ParallelOp::IndexRefresh | ParallelOp::IndexLoad | ParallelOp::DiffRename => {
                if !preload {
                    NonZeroUsize::MIN
                } else {
                    resolve_index_threads(config, self.threads_override)
                }
            }
            ParallelOp::IndexPack | ParallelOp::PackWrite => {
                resolve_pack_threads(config, self.threads_override)
            }
            ParallelOp::Checkout => resolve_checkout_workers(config, self.threads_override),
        };

        let (default_min_items, default_min_bytes) = match op {
            ParallelOp::IndexRefresh => (PAR_HASH_MIN_ITEMS, Some(PAR_HASH_MIN_TOTAL_BYTES)),
            ParallelOp::IndexLoad => (crate::index_name_hash_lazy::LAZY_THREAD_COST * 2, None),
            ParallelOp::Checkout => checkout_parallelism_threshold(config),
            ParallelOp::IndexPack | ParallelOp::PackWrite => (0, None),
            ParallelOp::DiffRename => (PAR_HASH_MIN_ITEMS, Some(PAR_HASH_MIN_TOTAL_BYTES)),
        };

        let min_items = self.min_items_override.unwrap_or(default_min_items);

        ParallelOptions {
            threads,
            min_items,
            min_total_bytes: default_min_bytes,
        }
    }

    /// Thread count for index stat preload (preserves the historical four-thread cap).
    #[must_use]
    pub fn index_stat_preload_threads(
        &self,
        config: &ConfigSet,
        blob_count: usize,
        stat_parallel_threads: Option<usize>,
    ) -> usize {
        const STAT_CAP: usize = 4;
        if !core_preload_index(config) || blob_count < PARALLEL_STAT_MIN_ENTRIES {
            return 1;
        }
        if let Some(n) = self.threads_override {
            if n <= 1 {
                return 1;
            }
            return n.min(STAT_CAP);
        }
        if let Some(requested) = stat_parallel_threads {
            if requested <= 1 {
                return 1;
            }
            return requested.min(STAT_CAP);
        }
        let cpus = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);
        cpus.clamp(1, STAT_CAP)
    }
}

/// Resolved worker count and serial-fallback thresholds for one parallel map call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParallelOptions {
    /// Worker threads (always at least 1).
    pub threads: NonZeroUsize,
    /// Minimum input length before spawning a pool (0 = always parallel when `threads` > 1).
    pub min_items: usize,
    /// Optional minimum summed payload bytes (used by object hashing).
    pub min_total_bytes: Option<usize>,
}

impl ParallelOptions {
    /// View as [`Parallelism`] for legacy hash helpers.
    #[must_use]
    pub fn parallelism(self) -> Parallelism {
        Parallelism::new(self.threads)
    }
}

#[must_use]
fn core_preload_index(config: &ConfigSet) -> bool {
    config
        .get_bool("core.preloadindex")
        .or_else(|| config.get_bool("core.preloadIndex"))
        .and_then(|r| r.ok())
        .unwrap_or(true)
}

/// Git `repo_config_get_index_threads` value semantics for `index.threads`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IndexThreadsSetting {
    /// Unset — Grit hash paths treat this as auto (all CPUs), matching prior behavior.
    UnsetAuto,
    Auto,
    Serial,
    Count(usize),
}

#[must_use]
fn index_threads_setting(config: &ConfigSet) -> IndexThreadsSetting {
    if let Some(Ok(n)) = config.get_i64("index.threads") {
        return match n {
            n if n <= 0 => IndexThreadsSetting::Auto,
            1 => IndexThreadsSetting::Serial,
            n => IndexThreadsSetting::Count(usize::try_from(n).unwrap_or(1)),
        };
    }
    if config
        .get("index.threads")
        .is_some_and(|v| v.trim() == "0")
    {
        return IndexThreadsSetting::Auto;
    }
    if let Some(result) = config.get_bool("index.threads") {
        return match result {
            Ok(true) => IndexThreadsSetting::Auto,
            Ok(false) => IndexThreadsSetting::Serial,
            Err(_) => IndexThreadsSetting::UnsetAuto,
        };
    }
    IndexThreadsSetting::UnsetAuto
}

#[must_use]
fn resolve_index_threads(config: &ConfigSet, override_threads: Option<usize>) -> NonZeroUsize {
    if let Some(n) = override_threads {
        return Parallelism::resolve(Some(n)).threads();
    }
    match index_threads_setting(config) {
        IndexThreadsSetting::UnsetAuto | IndexThreadsSetting::Auto => {
            Parallelism::resolve(None).threads()
        }
        IndexThreadsSetting::Serial => NonZeroUsize::MIN,
        IndexThreadsSetting::Count(n) => Parallelism::resolve(Some(n)).threads(),
    }
}

#[must_use]
fn resolve_pack_threads(config: &ConfigSet, override_threads: Option<usize>) -> NonZeroUsize {
    if let Some(n) = override_threads {
        return Parallelism::resolve(Some(n)).threads();
    }
    let requested = config
        .get_i64("pack.threads")
        .and_then(|r| r.ok())
        .map(|n| if n <= 0 { 0 } else { n as usize });
    Parallelism::resolve(requested).threads()
}

#[must_use]
fn resolve_checkout_workers(config: &ConfigSet, override_threads: Option<usize>) -> NonZeroUsize {
    if let Some(n) = override_threads {
        return Parallelism::resolve(Some(n)).threads();
    }
    match config.get_i64("checkout.workers") {
        None => Parallelism::resolve(None).threads(),
        Some(Ok(n)) if n < 1 => Parallelism::resolve(None).threads(),
        Some(Ok(n)) => Parallelism::resolve(Some(n as usize)).threads(),
        Some(Err(_)) => Parallelism::resolve(None).threads(),
    }
}

#[must_use]
fn checkout_parallelism_threshold(config: &ConfigSet) -> (usize, Option<usize>) {
    let threshold = config
        .get_i64("checkout.thresholdForParallelism")
        .and_then(|r| r.ok())
        .and_then(|n| usize::try_from(n).ok())
        .unwrap_or(100);
    (threshold, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ConfigScope, ConfigSet};
    use crate::environment::Environment;

    fn config_from(text: &str) -> ConfigSet {
        use crate::config::ConfigFile;
        use std::path::Path;
        let file = ConfigFile::parse(Path::new("test"), text, ConfigScope::Local).expect("parse");
        let mut set = ConfigSet::new();
        set.merge(&file);
        set
    }

    #[test]
    fn index_threads_bool_and_int_semantics() {
        let true_auto = config_from("[index]\n\tthreads = true\n");
        assert_eq!(index_threads_setting(&true_auto), IndexThreadsSetting::Auto);
        let false_serial = config_from("[index]\n\tthreads = false\n");
        assert_eq!(
            index_threads_setting(&false_serial),
            IndexThreadsSetting::Serial
        );
        let zero = config_from("[index]\n\tthreads = 0\n");
        assert_eq!(index_threads_setting(&zero), IndexThreadsSetting::Auto);
        let one = config_from("[index]\n\tthreads = 1\n");
        assert_eq!(index_threads_setting(&one), IndexThreadsSetting::Serial);
        let four = config_from("[index]\n\tthreads = 4\n");
        assert_eq!(index_threads_setting(&four), IndexThreadsSetting::Count(4));
        let neg = config_from("[index]\n\tthreads = -3\n");
        assert_eq!(index_threads_setting(&neg), IndexThreadsSetting::Auto);
    }

    #[test]
    fn preload_index_disables_index_refresh_workers() {
        let cfg = config_from("[core]\n\tpreloadIndex = false\n[index]\n\tthreads = 8\n");
        let policy = ThreadPolicy::default();
        let opts = policy.resolve(ParallelOp::IndexRefresh, &cfg);
        assert_eq!(opts.threads, NonZeroUsize::MIN);
    }

    #[test]
    fn thread_override_beats_config() {
        let cfg = config_from("[index]\n\tthreads = 2\n");
        let policy = ThreadPolicy::with_threads_override(Some(6));
        let opts = policy.resolve(ParallelOp::IndexRefresh, &cfg);
        assert_eq!(
            opts.threads.get(),
            6.min(
                std::thread::available_parallelism()
                    .map(|n| n.get())
                    .unwrap_or(1)
            )
        );
    }

    #[test]
    fn min_items_override_for_tests() {
        let cfg = ConfigSet::new();
        let policy = ThreadPolicy::with_min_items_override(Some(0));
        let opts = policy.resolve(ParallelOp::IndexPack, &cfg);
        assert_eq!(opts.min_items, 0);
    }

    #[test]
    fn pack_threads_zero_is_auto() {
        let cfg = config_from("[pack]\n\tthreads = 0\n");
        let opts = ThreadPolicy::default().resolve(ParallelOp::IndexPack, &cfg);
        assert!(opts.threads.get() > 1 || std::thread::available_parallelism().is_ok());
    }

    #[test]
    fn checkout_workers_below_one_is_auto() {
        let cfg = config_from("[checkout]\n\tworkers = 0\n");
        let opts = ThreadPolicy::default().resolve(ParallelOp::Checkout, &cfg);
        assert_eq!(
            opts.threads.get(),
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1)
        );
    }

    #[test]
    fn stat_preload_respects_cap_and_override() {
        let cfg = ConfigSet::new();
        let policy = ThreadPolicy::with_threads_override(Some(8));
        assert_eq!(policy.index_stat_preload_threads(&cfg, 5000, None), 4);
    }

    #[test]
    fn pack_index_parallelism_for_git_dir_matches_config() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("config"), "[pack]\n\tthreads = 3\n").expect("write config");
        let env = Environment::empty();
        let par = ConfigSet::pack_index_parallelism_for_git_dir(&env, Some(dir.path()));
        assert_eq!(par.threads().get(), 3);
    }
}
