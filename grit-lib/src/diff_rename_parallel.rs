//! Parallel blob loads and rename/copy score-matrix rows for [`crate::diff`].

use crate::config::ConfigSet;
use crate::odb::Odb;
use crate::parallel::{try_par_map, ParallelHashError, ParallelOp, ParallelOptions, ThreadPolicy};

/// Git's default `diff.renameLimit` when unset.
pub(crate) const DEFAULT_RENAME_LIMIT: usize = 1000;

/// Resolve pool options for rename/copy similarity work.
#[must_use]
pub(crate) fn diff_rename_parallel_options(
    config: &ConfigSet,
    thread_policy: &ThreadPolicy,
) -> ParallelOptions {
    thread_policy.resolve(ParallelOp::DiffRename, config)
}

/// Return `true` when Git would skip rename detection for this candidate count.
#[must_use]
pub(crate) fn exceeds_rename_detection_limits(
    deleted: usize,
    added: usize,
    config: &ConfigSet,
) -> bool {
    if deleted == 0 || added == 0 {
        return false;
    }
    let limit = rename_limit(config);
    deleted.max(added) > limit
}

#[must_use]
fn rename_limit(config: &ConfigSet) -> usize {
    config
        .get_i64("diff.renameLimit")
        .or_else(|| config.get_i64("diff.renamelimit"))
        .and_then(|r| r.ok())
        .and_then(|n| usize::try_from(n.max(0)).ok())
        .unwrap_or(DEFAULT_RENAME_LIMIT)
}

/// Load `count` blob payloads in parallel when policy thresholds allow.
pub(crate) fn parallel_load_optional_blobs<F>(
    odb: &Odb,
    count: usize,
    options: ParallelOptions,
    total_bytes_hint: usize,
    load_index: F,
) -> Vec<Option<Vec<u8>>>
where
    F: Fn(usize) -> Option<Vec<u8>> + Sync,
{
    if count == 0 {
        return Vec::new();
    }
    let indices: Vec<usize> = (0..count).collect();
    odb.with_pack_read_context(|| {
        match try_par_map(&indices, options, total_bytes_hint, |&i| Ok(load_index(i))) {
            Ok(rows) => rows,
            Err(ParallelHashError::Task(err)) => err,
        }
    })
}

/// Map `0..len` with the rename/copy parallel policy inside a pack read context.
pub(crate) fn parallel_map_indices<R, E, F>(
    odb: &Odb,
    len: usize,
    options: ParallelOptions,
    total_bytes_hint: usize,
    f: F,
) -> Result<Vec<R>, E>
where
    R: Send,
    E: Send + 'static,
    F: Fn(usize) -> Result<R, E> + Sync,
{
    if len == 0 {
        return Ok(Vec::new());
    }
    let indices: Vec<usize> = (0..len).collect();
    odb.with_pack_read_context(|| {
        try_par_map(&indices, options, total_bytes_hint, |&i| f(i))
            .map_err(|ParallelHashError::Task(err)| err)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rename_limit_defaults_to_git_value() {
        let cfg = ConfigSet::new();
        assert_eq!(rename_limit(&cfg), DEFAULT_RENAME_LIMIT);
    }

    #[test]
    fn rename_limit_blocks_large_side() {
        let cfg = ConfigSet::new();
        assert!(exceeds_rename_detection_limits(1001, 10, &cfg));
    }
}
