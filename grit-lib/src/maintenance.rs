//! Repository maintenance: repack, fsck, and garbage collection orchestration.
//!
//! These APIs wrap existing pack, prune, and fsck primitives for embedders and the
//! `grit` CLI. They never shell out to the system `git` binary.
//!
//! Library guide: [maintenance](https://grit-scm.com/docs/library/maintenance)
//!
//! ```rust
//! use grit_lib::repo::{InitOptions, Repository};
//! # fn main() -> grit_lib::error::Result<()> {
//! let dir = tempfile::tempdir()?;
//! let _repo = Repository::init(dir.path(), &InitOptions::default())?;
//! # Ok(())
//! # }
//! ```

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::commit_graph_file::CommitGraphChain;
use crate::commit_graph_write::{
    atomic_replace_commit_graph, build_commit_graph_bytes,
    collect_reachable_commit_oids_for_repository, load_commit_graph_commit_info,
    remove_commit_graph_chain_dir, BloomWriteStats,
};
use crate::config::ConfigSet;
use crate::diff::bloom::BloomFilterSettings;
use crate::environment::Environment;
use crate::error::{ConfigError, Error, Result};
use crate::fsck as repo_fsck;
use crate::gc::{prune_loose_unreachable, PruneStats};
use crate::git_date::approx::approxidate_careful_at;
use crate::index_pack::{install_pack_bytes, IngestPackOptions};
use crate::midx::{
    clear_pack_midx_state, write_multi_pack_index_with_options,
    MidxBitmapWriteOutcome, WriteMultiPackIndexOptions, WriteMultiPackIndexResult,
};
use crate::objects::{parse_commit, parse_tag, parse_tree, ObjectId, ObjectKind};
use crate::odb::Odb;
use crate::pack_bitmap::{PackBitmapWriteOptions, PackBitmapWriter};
use crate::pack_objects::{build_pack, PackBuildOptions};
use crate::progress::{NeverCancel, NullProgress};
use crate::prune_packed::{prune_packed_objects, PrunePackedOptions};
use crate::reflog::{
    expire_reflog_git, list_reflog_refs, load_gc_reflog_expire_config, ReflogExpireParams,
};
use crate::refs_fsck::{RefsFsckIssue, RefsFsckSeverity};
use crate::repo::Repository;
use repo_fsck::FsckIssue as RepoFsckIssue;

/// Options for [`repack_all()`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct RepackOptions {
    /// Report actions without writing packs or deleting files.
    pub dry_run: bool,
    /// After repack, write `objects/info/commit-graph` when the ODB is files-backed.
    pub write_commit_graph: bool,
    /// After repack, write a `.bitmap` sidecar for the new pack (files-backed only).
    pub write_bitmap: bool,
    /// Override pack-objects settings; `None` uses [`PackBuildOptions::for_local_copy`].
    pub pack_build: Option<PackBuildOptions>,
    /// Config/environment for ingest and pack threads (defaults to repo config).
    pub merged_config: Option<ConfigSet>,
    /// Process environment for config discovery and thread counts.
    pub environment: Environment,
}

impl Default for RepackOptions {
    fn default() -> Self {
        Self {
            dry_run: false,
            write_commit_graph: false,
            write_bitmap: false,
            pack_build: None,
            merged_config: None,
            environment: Environment::empty(),
        }
    }
}

impl RepackOptions {
    /// Start from [`Default::default`] with [`Self::dry_run`] set.
    #[must_use]
    pub fn with_dry_run(dry_run: bool) -> Self {
        Self {
            dry_run,
            ..Self::default()
        }
    }
}

/// Counters and paths produced by [`repack_all`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct RepackStats {
    /// Loose object files before repack.
    pub loose_before: usize,
    /// Pack files before repack.
    pub packs_before: usize,
    /// Objects written into the new pack (`None` when [`RepackOptions::dry_run`]).
    pub objects_packed: Option<usize>,
    /// Bytes in the new pack (`None` on dry run).
    pub pack_bytes: Option<u64>,
    /// Path to the installed pack (`None` on dry run or when nothing was packed).
    pub pack_path: Option<PathBuf>,
    /// Sidecar/index files removed from `objects/pack/`.
    pub removed_pack_files: usize,
    /// Loose objects removed because they were redundant with the new pack.
    pub pruned_loose: usize,
}

/// Options for [`Repository::write_commit_graph`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct CommitGraphOptions {
    /// When true, emit Bloom filter chunks for changed-path queries.
    pub changed_paths: bool,
}

impl CommitGraphOptions {
    /// Build options with an explicit changed-paths Bloom setting.
    #[must_use]
    pub fn with_changed_paths(changed_paths: bool) -> Self {
        Self {
            changed_paths,
            ..Self::default()
        }
    }
}

/// Result of writing `objects/info/commit-graph`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct CommitGraphWriteReport {
    /// Path to the monolithic commit-graph file.
    pub path: PathBuf,
    /// Commits encoded in the graph.
    pub commit_count: usize,
    /// Whether a split `commit-graphs/` chain was removed.
    pub removed_chain: bool,
    /// Bloom generation counters when [`CommitGraphOptions::changed_paths`] was set.
    pub bloom_stats: Option<BloomWriteStats>,
}

/// Options for [`Repository::write_midx`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct MidxOptions {
    /// Write a reachability `.bitmap` sidecar when the MIDX is a root file.
    pub bitmap: bool,
}

impl MidxOptions {
    /// Build MIDX write options with an explicit bitmap sidecar flag.
    #[must_use]
    pub fn with_bitmap(bitmap: bool) -> Self {
        Self {
            bitmap,
            ..Self::default()
        }
    }
}

/// Result of writing `objects/pack/multi-pack-index`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct MidxWriteReport {
    /// Path to the tip MIDX file.
    pub path: PathBuf,
    /// Pack indexes covered by the MIDX.
    pub pack_count: usize,
    /// Whether a reachability bitmap was written or skipped.
    pub bitmap: MidxBitmapWriteOutcome,
}

/// Options for [`References::pack`](crate::refs::References::pack).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct PackRefsOptions {
    /// When true, pack every packable ref (`git pack-refs --all`); otherwise only tags.
    pub all: bool,
}

impl PackRefsOptions {
    /// Build pack-refs options matching `git pack-refs` / `--all`.
    #[must_use]
    pub fn with_all(all: bool) -> Self {
        Self {
            all,
            ..Self::default()
        }
    }
}

/// Result of packing refs (files `packed-refs` or reftable compaction).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PackRefsReport {
    /// `packed-refs` path or the `reftable/` directory.
    pub path: PathBuf,
    /// Loose refs merged into the packed store.
    pub refs_packed: usize,
    /// Loose ref files removed (files backend) or tables merged (reftable).
    pub refs_pruned: usize,
}

/// Options for [`fsck()`].
#[derive(Debug, Clone, Copy, Default)]
#[non_exhaustive]
pub struct FsckOptions {
    /// Treat ref-fsck warnings as errors (Git `--strict` for references).
    pub strict: bool,
    /// Validate reachable object graph and object headers.
    pub check_objects: bool,
    /// Run [`refs_fsck()`](crate::refs_fsck::refs_fsck).
    pub check_refs: bool,
    /// Run [`verify_midx`] when a multi-pack-index is present.
    pub check_midx: bool,
}

/// Severity of one [`FsckIssue`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsckSeverity {
    /// Repository should be treated as broken.
    Error,
    /// Problem that may be acceptable depending on policy.
    Warning,
}

/// One fsck diagnostic (refs, objects, or MIDX).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FsckIssue {
    /// Whether this issue should fail the overall fsck run.
    pub severity: FsckSeverity,
    /// Ref name, object id hex, or `multi-pack-index`.
    pub path: String,
    /// CamelCase message id (Git fsck vocabulary where applicable).
    pub msg_id: String,
    /// Human-readable detail after the message id.
    pub detail: String,
}

/// Aggregate fsck result.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct FsckReport {
    /// True when no [`FsckSeverity::Error`] issues were recorded.
    pub ok: bool,
    /// All recorded fsck findings in discovery order.
    pub issues: Vec<FsckIssue>,
}

/// Format one issue like Git fsck / refs verify (`level: path: msgId: detail`).
#[must_use]
pub fn format_fsck_line(issue: &FsckIssue) -> String {
    let level = match issue.severity {
        FsckSeverity::Error => "error",
        FsckSeverity::Warning => "warning",
    };
    format!(
        "{}: {}: {}: {}",
        level, issue.path, issue.msg_id, issue.detail
    )
}

/// Options for [`gc()`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct GcOptions {
    /// Repack options applied before pruning unreachable objects.
    pub repack: RepackOptions,
    /// Expire reflogs using `gc.reflogExpire*` before repack/prune.
    pub expire_reflog: bool,
    /// Unix seconds for reflog expiry rules.
    pub now_unix: i64,
}

impl Default for GcOptions {
    fn default() -> Self {
        Self {
            repack: RepackOptions::default(),
            expire_reflog: true,
            now_unix: 0,
        }
    }
}

impl GcOptions {
    /// Run gc with repack dry-run control and an explicit wall-clock for reflog expiry.
    #[must_use]
    pub fn with_repack_dry_run(dry_run: bool, now_unix: i64) -> Self {
        Self {
            repack: RepackOptions::with_dry_run(dry_run),
            expire_reflog: true,
            now_unix,
        }
    }

    /// Default repack with explicit reflog expiry policy and wall-clock.
    #[must_use]
    pub fn with_reflog_policy(now_unix: i64, expire_reflog: bool) -> Self {
        Self {
            repack: RepackOptions::default(),
            expire_reflog,
            now_unix,
        }
    }
}

impl FsckOptions {
    /// Check objects, refs, and MIDX with optional strict ref policy.
    #[must_use]
    pub fn full_repository(strict: bool) -> Self {
        Self {
            strict,
            check_objects: true,
            check_refs: true,
            check_midx: true,
        }
    }

    /// Check objects and refs without validating MIDX.
    #[must_use]
    pub fn objects_and_refs(strict: bool) -> Self {
        Self {
            strict,
            check_objects: true,
            check_refs: true,
            check_midx: false,
        }
    }

    /// Validate only the multi-pack-index when present.
    #[must_use]
    pub fn midx_only(strict: bool) -> Self {
        Self {
            strict,
            check_objects: false,
            check_refs: false,
            check_midx: true,
        }
    }
}

/// Object ids that must stay reachable during repack, prune, and fsck walks.
///
/// Includes ref tips, symbolic `HEAD`, every reflog `old`/`new` oid, and every
/// blob/tree oid recorded in the index (including unmerged stages; gitlinks are excluded).
///
/// # Errors
///
/// Propagates ref, reflog, or index load failures.
pub fn collect_maintenance_object_roots(repo: &Repository) -> Result<Vec<ObjectId>> {
    repo_fsck::reachability_roots(repo)
}

pub(crate) fn bloom_settings_for_commit_graph_write(
    config: &ConfigSet,
    changed_paths: bool,
) -> BloomFilterSettings {
    let mut bloom = BloomFilterSettings::default();
    if !changed_paths {
        return bloom;
    }
    if let Some(v) = config
        .get("commitgraph.changedpathsversion")
        .and_then(|s| s.trim().parse::<i32>().ok())
    {
        if v == 2 {
            bloom.hash_version = 2;
        } else if v == 1 {
            bloom.hash_version = 1;
        }
    }
    bloom
}

pub(crate) fn commit_graph_writes_generation_data(config: &ConfigSet) -> bool {
    config
        .get("commitgraph.generationversion")
        .and_then(|s| s.trim().parse::<i32>().ok())
        .map(|v| v >= 2)
        .unwrap_or(true)
}

/// Write a monolithic reachable commit-graph under `objects/info/commit-graph`.
///
/// Reachable commits are discovered from [`Repository::refs`](crate::repo::Repository::refs)
/// (reftable, packed-refs, and linked-worktree visibility), not by scanning `git_dir/refs` alone.
///
/// # Errors
///
/// Returns [`Error::UnsupportedObjectStore`] for non-files ODB backends, or errors from graph
/// generation and atomic I/O.
pub fn write_commit_graph(
    repo: &Repository,
    opts: &CommitGraphOptions,
) -> Result<CommitGraphWriteReport> {
    let odb = repo.odb();
    odb.require_files_primary("commit-graph write")?;
    let objects_dir = odb
        .files_objects_dir()
        .ok_or(Error::UnsupportedObjectStore {
            operation: "commit-graph write",
        })?;
    let config = repo.config()?;
    let bloom = bloom_settings_for_commit_graph_write(&config, opts.changed_paths);
    let write_generation = commit_graph_writes_generation_data(&config);

    let chain = CommitGraphChain::try_load(objects_dir).ok().flatten();

    let commits = collect_reachable_commit_oids_for_repository(repo)?;
    let mut sorted: Vec<ObjectId> = commits.into_iter().collect();
    sorted.sort();
    let commit_count = sorted.len();

    let mut infos = HashMap::new();
    for oid in &sorted {
        infos.insert(*oid, load_commit_graph_commit_info(odb, *oid)?);
    }

    let mut existing_filters = HashMap::new();
    let mut upgraded_filters = HashMap::new();
    if opts.changed_paths {
        if let Some(ref chain) = chain {
            for oid in &sorted {
                if let Some(bytes) = chain.existing_filter_bytes(oid, &bloom) {
                    existing_filters.insert(*oid, bytes);
                } else if let Some(bytes) = chain.upgradable_filter_bytes(oid, &bloom) {
                    upgraded_filters.insert(*oid, bytes);
                }
            }
        }
    }

    let (bytes, bloom_stats) = build_commit_graph_bytes(
        &sorted,
        &infos,
        odb,
        opts.changed_paths,
        &bloom,
        None,
        &[],
        None,
        &existing_filters,
        &upgraded_filters,
        write_generation,
    )?;

    let path = atomic_replace_commit_graph(objects_dir, &bytes)?;
    let removed_chain = remove_commit_graph_chain_dir(objects_dir);

    Ok(CommitGraphWriteReport {
        path,
        commit_count,
        removed_chain,
        bloom_stats: opts.changed_paths.then_some(bloom_stats),
    })
}

/// Write or refresh `objects/pack/multi-pack-index` for the repository.
///
/// # Errors
///
/// Returns [`Error::UnsupportedObjectStore`] for non-files backends, or MIDX write failures.
pub fn write_midx(repo: &Repository, opts: &MidxOptions) -> Result<MidxWriteReport> {
    let odb = repo.odb();
    odb.require_files_primary("midx write")?;
    let objects_dir = odb
        .files_objects_dir()
        .ok_or(Error::UnsupportedObjectStore {
            operation: "midx write",
        })?;
    let pack_dir = objects_dir.join("pack");
    let pack_count = fs::read_dir(&pack_dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| e.path().extension().is_some_and(|x| x == "idx"))
                .filter(|e| e.path().with_extension("pack").is_file())
                .count()
        })
        .unwrap_or(0);

    let write_opts = WriteMultiPackIndexOptions {
        write_bitmap: opts.bitmap,
        ..WriteMultiPackIndexOptions::default()
    };
    let WriteMultiPackIndexResult { bitmap } =
        write_multi_pack_index_with_options(&pack_dir, &write_opts)?;
    let path = pack_dir.join("multi-pack-index");
    Ok(MidxWriteReport {
        path,
        pack_count,
        bitmap,
    })
}

/// Result of [`gc`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GcStats {
    /// Counters from the repack phase.
    pub repack: RepackStats,
    /// Counters from loose-object pruning after repack.
    pub prune: PruneStats,
    /// Reflog lines removed by expiry during gc.
    pub reflog_entries_pruned: usize,
}

/// Repack all reachable objects into a single pack and drop redundant storage.
///
/// Mirrors a minimal `git repack -ad`: one pack for ref roots, delete other packs,
/// then [`prune_packed_objects`] for loose objects already in the pack.
///
/// # Errors
///
/// Propagates pack build/install failures, unsupported custom ODB backends, and I/O errors.
pub fn repack_all(repo: &Repository, opts: &RepackOptions) -> Result<RepackStats> {
    let odb = repo.odb();
    odb.require_files_primary("repack")?;
    let objects_dir = odb
        .files_objects_dir()
        .ok_or(Error::UnsupportedObjectStore {
            operation: "repack",
        })?;
    let pack_dir = objects_dir.join("pack");

    let mut stats = RepackStats {
        loose_before: count_loose_objects(odb)?,
        packs_before: count_pack_files(&pack_dir),
        ..RepackStats::default()
    };

    let roots = collect_maintenance_object_roots(repo)?;
    if roots.is_empty() && stats.loose_before == 0 && stats.packs_before == 0 {
        return Ok(stats);
    }

    let cfg = opts
        .merged_config
        .clone()
        .or_else(|| repo.config().ok().map(|c| (*c).clone()));
    let pack_opts = opts
        .pack_build
        .unwrap_or_else(|| PackBuildOptions::for_local_copy(cfg.as_ref()));

    if opts.dry_run {
        stats.objects_packed = Some(estimate_reachable_count(odb, &roots)?);
        return Ok(stats);
    }

    let pack_bytes = build_pack(odb, &roots, &[], &pack_opts)?;
    if pack_bytes.len() <= 12 + odb.hash_algo().len() {
        return Ok(stats);
    }

    clear_pack_midx_state(&pack_dir)?;

    let ingest_opts = IngestPackOptions {
        fix_thin: false,
        merged_config: cfg.clone(),
        environment: opts.environment.clone(),
        ..IngestPackOptions::default()
    };
    let ingested = install_pack_bytes(pack_bytes, odb, &ingest_opts)?;
    stats.objects_packed = Some(ingested.object_ids.len());
    stats.pack_bytes = ingested.pack_path.metadata().ok().map(|m| m.len());
    stats.pack_path = Some(ingested.pack_path.clone());
    stats.removed_pack_files = remove_superseded_pack_files(&pack_dir, &ingested.pack_path)?;

    let pruned = prune_packed_objects(objects_dir, PrunePackedOptions::default())?;
    stats.pruned_loose = pruned.len();

    // After prune-packed reads pack indexes, drop listings so later reads (and external
    // `git gc` replacing packs) never reuse stale thread-local or catalog state.
    odb.invalidate_packs();

    if opts.write_commit_graph {
        write_commit_graph(repo, &CommitGraphOptions::with_changed_paths(true))?;
    }
    if opts.write_bitmap {
        let idx_path = ingested.pack_path.with_extension("idx");
        let bitmap_opts = PackBitmapWriteOptions::default();
        let _ = PackBitmapWriter::write(repo, &idx_path, &bitmap_opts, SystemTime::UNIX_EPOCH);
    }

    Ok(stats)
}

/// Check ref integrity, reachable object graph, and optional MIDX.
pub fn fsck(repo: &Repository, opts: &FsckOptions) -> Result<FsckReport> {
    let config = repo.config()?;
    let inner_opts = repo_fsck::FsckOptions {
        connectivity_only: !opts.check_objects,
        check_dangling: false,
        strict: opts.strict,
    };
    let inner = repo_fsck::fsck(repo, &inner_opts, &mut NullProgress, &NeverCancel)?;
    let mut issues = Vec::new();
    for issue in inner.issues {
        match &issue {
            RepoFsckIssue::BadRef(_) if !opts.check_refs => continue,
            RepoFsckIssue::MidxInvalid { .. } if !opts.check_midx => continue,
            _ => {}
        }
        issues.push(fsck_issue_from_repo(issue, &config, opts.strict));
    }
    let ok = !issues.iter().any(|i| i.severity == FsckSeverity::Error);
    Ok(FsckReport { ok, issues })
}

/// Run reflog expiry (optional), repack, and unreachable loose-object prune.
pub fn gc(repo: &Repository, opts: &GcOptions) -> Result<GcStats> {
    let mut stats = GcStats::default();
    let odb = repo.odb();
    odb.require_files_primary("gc")?;

    let now = gc_now_unix(opts.now_unix);
    let cfg = repo.config()?;
    let prune_policy = gc_loose_prune_policy(&cfg, now)?;

    if opts.expire_reflog {
        stats.reflog_entries_pruned = expire_reflogs_for_gc(repo, opts.repack.dry_run, now)?;
    }

    stats.repack = repack_all(repo, &opts.repack)?;

    if opts.repack.dry_run {
        return Ok(stats);
    }

    let roots = collect_maintenance_object_roots(repo)?;
    stats.prune = match prune_policy {
        GcLoosePrunePolicy::Disabled => PruneStats::default(),
        GcLoosePrunePolicy::Enabled { keep_newer_than } => {
            prune_loose_unreachable(odb, &roots, keep_newer_than)?
        }
    };
    Ok(stats)
}

/// How [`gc`] applies loose-object pruning for `gc.pruneExpire`.
enum GcLoosePrunePolicy {
    /// `gc.pruneExpire=never` — do not delete unreachable loose objects.
    Disabled,
    /// Prune unreachable loose objects not protected by the grace cutoff.
    Enabled {
        /// When `Some(t)`, only delete loose objects strictly older than `t` (Git grace window).
        keep_newer_than: Option<SystemTime>,
    },
}

fn gc_loose_prune_policy(config: &ConfigSet, now_unix: i64) -> Result<GcLoosePrunePolicy> {
    let spec = config
        .get("gc.pruneExpire")
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "2.weeks.ago".to_owned());
    if spec.eq_ignore_ascii_case("never") {
        return Ok(GcLoosePrunePolicy::Disabled);
    }
    let mut err = 0;
    let cutoff_sec = approxidate_careful_at(&spec, now_unix, Some(&mut err)) as i64;
    if err != 0 {
        return Err(Error::Config(ConfigError::Other(format!(
            "bad config value '{spec}' for 'gc.pruneExpire'"
        ))));
    }
    let keep_newer_than = SystemTime::UNIX_EPOCH
        .checked_add(std::time::Duration::from_secs(cutoff_sec.max(0) as u64));
    Ok(GcLoosePrunePolicy::Enabled { keep_newer_than })
}

fn gc_now_unix(now_unix: i64) -> i64 {
    if now_unix != 0 {
        return now_unix;
    }
    time::OffsetDateTime::now_utc().unix_timestamp()
}

fn expire_reflogs_for_gc(repo: &Repository, dry_run: bool, now: i64) -> Result<usize> {
    let cfg = repo.config()?;
    let gc_cfg = load_gc_reflog_expire_config(&cfg, now);
    let git_dir = repo.git_dir();
    let params = ReflogExpireParams {
        stale_fix: false,
        dry_run,
        verbose: false,
    };
    let mut total = 0usize;
    for refname in list_reflog_refs(git_dir)? {
        let result = expire_reflog_git(
            repo,
            git_dir,
            &refname,
            &params,
            None,
            None,
            &gc_cfg.patterns,
            gc_cfg.global_total,
            gc_cfg.global_unreachable,
            now,
        )?;
        total += result.pruned;
    }
    Ok(total)
}

fn fsck_issue_from_refs(issue: RefsFsckIssue) -> FsckIssue {
    FsckIssue {
        severity: match issue.severity {
            RefsFsckSeverity::Error => FsckSeverity::Error,
            RefsFsckSeverity::Warning => FsckSeverity::Warning,
        },
        path: issue.path,
        msg_id: issue.msg_id.to_owned(),
        detail: issue.detail,
    }
}

fn fsck_issue_from_repo(issue: RepoFsckIssue, config: &ConfigSet, strict: bool) -> FsckIssue {
    if let RepoFsckIssue::BadRef(r) = issue {
        return fsck_issue_from_refs(r);
    }
    let severity = repo_fsck::issue_severity(&issue, config, strict);
    let msg_id = repo_fsck::issue_msg_id(&issue).to_owned();
    let (path, detail) = match &issue {
        RepoFsckIssue::CorruptLoose { oid, path } => {
            (oid.to_hex(), format!("could not read {}", path.display()))
        }
        RepoFsckIssue::HashMismatch { oid, path } => (
            oid.to_hex(),
            format!("loose hash mismatch at {}", path.display()),
        ),
        RepoFsckIssue::BadObject { oid, error } => (oid.to_hex(), error.detail.clone()),
        RepoFsckIssue::PackChecksum { pack } => (
            pack.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("pack")
                .to_owned(),
            format!("pack checksum invalid for {}", pack.display()),
        ),
        RepoFsckIssue::IdxMismatch { pack } => (
            pack.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("pack")
                .to_owned(),
            format!("index checksum mismatch for {}", pack.display()),
        ),
        RepoFsckIssue::MissingObject { oid, referenced_by } => (
            oid.to_hex(),
            format!("referenced by {}", referenced_by.to_hex()),
        ),
        RepoFsckIssue::BrokenLink {
            tree,
            name,
            mode,
            target,
        } => (
            tree.to_hex(),
            format!("entry {name} mode {mode:o} -> {}", target.to_hex()),
        ),
        RepoFsckIssue::BadRef(_) => unreachable!(),
        RepoFsckIssue::MidxInvalid { details } => {
            ("multi-pack-index".to_owned(), details.join("; "))
        }
        RepoFsckIssue::CommitGraphInvalid { detail } => ("commit-graph".to_owned(), detail.clone()),
    };
    FsckIssue {
        severity: match severity {
            repo_fsck::FsckSeverity::Error => FsckSeverity::Error,
            repo_fsck::FsckSeverity::Warning => FsckSeverity::Warning,
        },
        path,
        msg_id,
        detail,
    }
}

fn count_loose_objects(odb: &Odb) -> Result<usize> {
    Ok(odb.enumerate_local_loose_objects()?.len())
}

fn count_pack_files(pack_dir: &Path) -> usize {
    if !pack_dir.is_dir() {
        return 0;
    }
    fs::read_dir(pack_dir)
        .ok()
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| e.path().extension().is_some_and(|ext| ext == "pack"))
                .count()
        })
        .unwrap_or(0)
}

fn estimate_reachable_count(odb: &Odb, roots: &[ObjectId]) -> Result<usize> {
    let mut seen: HashSet<ObjectId> = HashSet::new();
    let mut queue: VecDeque<ObjectId> = VecDeque::new();
    for &root in roots {
        if seen.insert(root) {
            queue.push_back(root);
        }
    }
    while let Some(oid) = queue.pop_front() {
        let obj = match odb.read(&oid) {
            Ok(o) => o,
            Err(_) => continue,
        };
        match obj.kind {
            ObjectKind::Commit => {
                let commit = parse_commit(&obj.data)?;
                for parent in commit.parents {
                    if seen.insert(parent) {
                        queue.push_back(parent);
                    }
                }
                if seen.insert(commit.tree) {
                    queue.push_back(commit.tree);
                }
            }
            ObjectKind::Tree => {
                for entry in parse_tree(&obj.data)? {
                    if entry.mode == 0o160000 {
                        continue;
                    }
                    if seen.insert(entry.oid) {
                        queue.push_back(entry.oid);
                    }
                }
            }
            ObjectKind::Tag => {
                let tag = parse_tag(&obj.data)?;
                if seen.insert(tag.object) {
                    queue.push_back(tag.object);
                }
            }
            ObjectKind::Blob => {}
        }
    }
    Ok(seen.len())
}

/// Remove pack artifacts in `pack_dir` except those belonging to `keep_pack`.
pub fn remove_superseded_pack_files(pack_dir: &Path, keep_pack: &Path) -> Result<usize> {
    let keep_stem = keep_pack
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| Error::Io(io::Error::new(io::ErrorKind::InvalidInput, "bad pack path")))?;
    if !pack_dir.is_dir() {
        return Ok(0);
    }
    let mut removed = 0usize;
    for entry in fs::read_dir(pack_dir).map_err(Error::Io)? {
        let entry = entry.map_err(Error::Io)?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str == keep_stem || name_str.starts_with(&format!("{keep_stem}.")) {
            continue;
        }
        if !(name_str.starts_with("pack-") || name_str.starts_with(".pack-")) {
            continue;
        }
        let path = entry.path();
        if path.is_file() {
            if pack_has_keep_marker(pack_dir, &path) {
                continue;
            }
            fs::remove_file(&path).map_err(Error::Io)?;
            removed += 1;
        }
    }
    Ok(removed)
}

fn pack_has_keep_marker(pack_dir: &Path, path: &Path) -> bool {
    let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
        return false;
    };
    pack_dir.join(format!("{stem}.keep")).is_file()
}

/// Pack loose refs into `packed-refs` (files backend) or compact reftable stacks.
///
/// # Errors
///
/// Propagates ref-store, locking, and I/O failures.
pub fn pack_refs(repo: &Repository, opts: &PackRefsOptions) -> Result<PackRefsReport> {
    let (path, refs_packed, refs_pruned) = crate::refs::pack_refs_for_repository(repo, opts.all)?;
    Ok(PackRefsReport {
        path,
        refs_packed,
        refs_pruned,
    })
}

impl Repository {
    /// Write a reachable commit-graph file for this repository.
    ///
    /// # Errors
    ///
    /// See [`write_commit_graph`].
    pub fn write_commit_graph(&self, opts: &CommitGraphOptions) -> Result<CommitGraphWriteReport> {
        write_commit_graph(self, opts)
    }

    /// Write or refresh the multi-pack index for this repository's pack directory.
    ///
    /// # Errors
    ///
    /// See [`write_midx`].
    pub fn write_midx(&self, opts: &MidxOptions) -> Result<MidxWriteReport> {
        write_midx(self, opts)
    }
}

/// Standalone object fsck helpers.
pub mod fsck {
    pub use crate::fsck_standalone::*;
}

/// Repository-wide fsck (object store, connectivity, refs).
pub mod repository_fsck {
    pub use crate::fsck::*;
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use crate::porcelain::add::stage_for_commit;
    use crate::porcelain::commit::{create_commit, CommitRequest};
    use crate::progress::{NeverCancel, NullProgress};
    use crate::repo::InitOptions;
    use tempfile::TempDir;

    #[test]
    fn fsck_clean_repo_ok() {
        let dir = TempDir::new().unwrap();
        let repo = Repository::init(dir.path(), &InitOptions::default()).expect("init");
        std::fs::write(repo.work_tree().unwrap().join("a"), b"x").unwrap();
        stage_for_commit(&repo, &mut NullProgress).expect("stage");
        let ident = "Test <t@example.com> 1 +0000".to_string();
        create_commit(
            &repo,
            &CommitRequest {
                message: "init".into(),
                author: ident.clone(),
                committer: ident,
                allow_empty: false,
                sign_override: None,
                amend: false,
            },
            &mut NullProgress,
        )
        .expect("commit");
        let report = fsck(
            &repo,
            &FsckOptions {
                strict: true,
                check_objects: true,
                check_refs: true,
                check_midx: true,
            },
        )
        .expect("fsck");
        assert!(report.ok, "{:?}", report.issues);
    }

    #[test]
    fn refs_fsck_line_roundtrip() {
        let issue = FsckIssue {
            severity: FsckSeverity::Error,
            path: "refs/heads/main".to_owned(),
            msg_id: "badSymref".to_owned(),
            detail: "symref target missing".to_owned(),
        };
        assert!(format_fsck_line(&issue).contains("badSymref"));
        let refs = RefsFsckIssue {
            severity: RefsFsckSeverity::Error,
            path: issue.path.clone(),
            msg_id: "badSymref",
            detail: issue.detail.clone(),
        };
        assert_eq!(
            format_fsck_line(&issue),
            crate::refs_fsck::format_refs_fsck_line(&refs)
        );
    }
}
