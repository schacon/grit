//! Integration coverage for [`grit_lib::parallel`] public helpers.

use grit_lib::config::{ConfigFile, ConfigScope, ConfigSet};
use grit_lib::parallel::{par_map, ParallelOp, ParallelOptions, Parallelism, ThreadPolicy};
use grit_test_support::forced_parallel_policy;
use std::num::NonZeroUsize;
use std::path::Path;

fn config_from(text: &str) -> ConfigSet {
    let file = ConfigFile::parse(Path::new("test"), text, ConfigScope::Local).expect("parse");
    let mut set = ConfigSet::new();
    set.merge(&file);
    set
}

#[test]
fn forced_parallel_policy_enables_tiny_batches() {
    let policy = forced_parallel_policy();
    let opts = policy.resolve(ParallelOp::IndexPack, &ConfigSet::new());
    assert_eq!(opts.min_items, 0);
    assert!(opts.threads.get() >= 1);
}

#[test]
fn par_map_public_api_preserves_order_with_forced_policy() {
    let policy = forced_parallel_policy();
    let opts = policy.resolve(ParallelOp::IndexRefresh, &ConfigSet::new());
    let items: Vec<usize> = (0..128).collect();
    let got = par_map(&items, opts, 0, |&x| x * 3);
    let expect: Vec<_> = items.iter().map(|x| x * 3).collect();
    assert_eq!(got, expect);
}

#[test]
fn index_threads_false_is_serial_for_refresh() {
    let cfg = config_from("[index]\n\tthreads = false\n");
    let opts = ThreadPolicy::default().resolve(ParallelOp::IndexRefresh, &cfg);
    assert_eq!(opts.threads, NonZeroUsize::MIN);
}

#[test]
fn parallelism_resolve_matches_documented_auto() {
    let avail = std::thread::available_parallelism()
        .map(NonZeroUsize::get)
        .unwrap_or(1);
    assert_eq!(Parallelism::resolve(Some(0)).threads().get(), avail);
}
