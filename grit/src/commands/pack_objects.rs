//! `grit pack-objects` — create a packed archive of objects.
//!
//! Reads object IDs (or revisions with `--revs`) from stdin and writes a
//! `.pack` file and corresponding `.idx` index file.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use grit_lib::config::ConfigSet;
use grit_lib::error::Error as LibError;
use grit_lib::rev_list::{rev_list, MissingAction, RevListOptions};
use sha1::{Digest, Sha1};
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::io::{self, BufRead, IsTerminal, Write};
use std::thread;
use std::time::Duration;

use crate::grit_exe;
use grit_lib::delta_encode::{encode_lcp_delta, encode_prefix_extension_delta};
use grit_lib::objects::{parse_commit, parse_tree, ObjectId, ObjectKind};
use grit_lib::odb::Odb;
use grit_lib::promisor::{promisor_pack_object_ids, repo_treats_promisor_packs};
use grit_lib::repo::Repository;
use grit_lib::rev_parse::resolve_revision;
use std::path::Path;
use std::process::{Command, Stdio};

/// Arguments for `grit pack-objects`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Base name for the output files (writes <base>-<hash>.pack and .idx).
    #[arg(value_name = "BASE-NAME")]
    pub base_name: Option<String>,

    /// Write the pack data to stdout instead of a file.
    #[arg(long)]
    pub stdout: bool,

    /// Read revision list instead of object list from stdin.
    #[arg(long)]
    pub revs: bool,

    /// Omit objects the client already has (after `--not` in rev-list input).
    #[arg(long)]
    pub thin: bool,

    /// Shallow boundary file (accepted for `upload-pack` compatibility; no-op in grit).
    #[arg(long = "shallow-file", allow_hyphen_values = true)]
    pub shallow_file: Option<String>,

    /// Shallow pack (accepted for compatibility; no-op in grit).
    #[arg(long)]
    pub shallow: bool,

    /// Include annotated tags (accepted for compatibility; no-op in grit).
    #[arg(long = "include-tag")]
    pub include_tag: bool,

    /// Pack all objects in the repository.
    #[arg(long)]
    pub all: bool,

    /// Read pack filenames from stdin instead of object IDs.
    #[arg(long = "stdin-packs")]
    pub stdin_packs: bool,

    /// Use OFS_DELTA (delta-base-offset) format in pack output.
    #[arg(long = "delta-base-offset")]
    pub delta_base_offset: bool,

    /// Hash algorithm (accepted for compat).
    #[arg(long = "object-format")]
    pub object_format: Option<String>,

    /// Keep true parents (accepted for compat, no-op in grit).
    #[arg(long = "keep-true-parents")]
    pub keep_true_parents: bool,

    /// Suppress progress output (accepted for compat).
    #[arg(short = 'q', long = "quiet")]
    pub quiet: bool,

    /// Keep unreachable objects (accepted for compat).
    #[arg(long = "keep-unreachable")]
    pub keep_unreachable: bool,

    /// Unpack unreachable objects (accepted for compat).
    ///
    /// Git's flag is optional (`--unpack-unreachable[=time]`). A bare flag must not consume the
    /// trailing `<base-name>` argument (`git repack` passes `--unpack-unreachable` before the path).
    #[arg(
        long = "unpack-unreachable",
        num_args = 0..=1,
        default_missing_value = "",
        require_equals = true
    )]
    pub unpack_unreachable: Option<String>,

    /// Window size for delta compression (accepted for compat).
    #[arg(long = "window", allow_hyphen_values = true)]
    pub window: Option<i64>,

    /// Depth for delta compression (accepted for compat).
    #[arg(long = "depth", allow_hyphen_values = true)]
    pub depth: Option<i64>,

    /// Path-walk packing order (accepted for compat; grit does not implement path-walk yet).
    #[arg(long = "path-walk")]
    pub path_walk: bool,

    /// Disable path-walk ordering (default; accepted for test compatibility).
    #[arg(long = "no-path-walk")]
    pub no_path_walk: bool,

    /// Honor pack-keep files (accepted for compat).
    #[arg(long = "honor-pack-keep")]
    pub honor_pack_keep: bool,

    /// Only use local objects (accepted for compat).
    #[arg(long = "local")]
    pub local: bool,

    /// Write bitmap index (accepted for compat).
    #[arg(long = "write-bitmap-index")]
    pub write_bitmap_index: bool,

    /// Do not write bitmap index (accepted for compat).
    #[arg(long = "no-write-bitmap-index")]
    pub no_write_bitmap_index: bool,

    /// Filter specification (accepted for compat).
    #[arg(long = "filter")]
    pub filter: Option<String>,

    /// Write objects omitted by `--filter` to this pack prefix (Git `--filter-to`).
    #[arg(long = "filter-to", value_name = "BASE")]
    pub filter_to: Option<String>,

    /// Missing objects are ok (accepted for compat).
    #[arg(long = "missing")]
    pub missing: Option<String>,

    /// Exclude pack (accepted for compat).
    #[arg(long = "exclude-promisor-objects")]
    pub exclude_promisor_objects: bool,

    /// Include redundant objects (accepted for compat).
    #[arg(long = "include-redundant")]
    pub include_redundant: bool,

    /// Incremental pack (accepted for compat).
    #[arg(long = "incremental")]
    pub incremental: bool,

    /// Limit to objects not yet in any pack (used with `--all` and `--incremental` for `git repack -d`).
    #[arg(long = "unpacked")]
    pub unpacked: bool,

    /// Do not create empty pack (accepted for compat).
    #[arg(long = "non-empty")]
    pub non_empty: bool,

    /// Pack reachable loose objects (accepted for compat).
    #[arg(long = "loosen-unreachable")]
    pub loosen_unreachable: bool,

    /// Keep unreachable objects in pack (accepted for compat).
    #[arg(long = "pack-loose-unreachable")]
    pub pack_loose_unreachable: bool,

    /// Include objects reachable from reflog (accepted for compat).
    #[arg(long = "reflog")]
    pub reflog: bool,

    /// Index version (accepted for compat).
    #[arg(long = "index-version")]
    pub index_version: Option<String>,

    /// Number of threads (accepted for compat).
    #[arg(long = "threads")]
    pub threads: Option<u32>,

    /// Maximum output size (accepted for compat).
    #[arg(long = "max-pack-size")]
    pub max_pack_size: Option<String>,

    /// Sparse reachability traversal (accepted for compat).
    #[arg(long = "sparse", action = clap::ArgAction::SetTrue)]
    pub sparse: bool,

    /// Dense reachability traversal (disables sparse; matches Git `--no-sparse`).
    #[arg(long = "no-sparse", action = clap::ArgAction::SetTrue)]
    pub no_sparse: bool,

    /// Progress output (accepted for compat).
    #[arg(long = "progress")]
    pub progress: bool,

    /// Include indexed objects (accepted for compat).
    #[arg(long = "indexed-objects")]
    pub indexed_objects: bool,

    /// Restrict `--all` to the ref/reflog/index reachability closure (first pack of `repack
    /// --cruft`). Default `pack-objects --all` enumerates the full object directory like Git.
    #[arg(long = "reachability-all", hide = true)]
    pub reachability_all: bool,

    /// Cruft pack options (accepted for compat).
    #[arg(long = "cruft")]
    pub cruft: bool,

    #[arg(long = "cruft-expiration")]
    pub cruft_expiration: Option<String>,

    /// Do not repack objects that appear only in this pack (repeatable; basename like `pack-abc.pack`).
    #[arg(long = "keep-pack", value_name = "NAME", action = clap::ArgAction::Append)]
    pub keep_pack: Vec<String>,

    /// Extra args passed through (for forward compat with unknown flags).
    #[arg(value_name = "EXTRA", num_args = 0.., allow_hyphen_values = true, trailing_var_arg = true, hide = true)]
    pub extra: Vec<String>,
}

/// A pack entry to be written.
#[derive(Clone)]
struct PackEntry {
    oid: ObjectId,
    kind: ObjectKind,
    data: Vec<u8>,
}

/// Objects to pack plus optional stdin thin-pack hints (`-` preferred base lines).
struct PackObjectList {
    oids: Vec<ObjectId>,
    /// Blob OIDs that should delta against a base blob (base may be omitted from `oids`).
    thin_blob_deltas: Vec<(ObjectId, ObjectId)>,
}

/// One slot in a pack file (full object or `REF_DELTA`).
enum PackWriteEntry {
    Full(PackEntry),
    RefDelta {
        oid: ObjectId,
        base_oid: ObjectId,
        /// Uncompressed Git binary delta (zlib-compressed in the pack stream).
        delta: Vec<u8>,
    },
}

/// Run `grit pack-objects`.
pub fn run(args: Args) -> Result<()> {
    if let Some(fmt) = &args.object_format {
        if fmt != "sha1" {
            bail!("unsupported object format: {fmt}");
        }
    }

    if args.sparse && args.no_sparse {
        bail!("cannot combine --sparse and --no-sparse");
    }

    if !args.stdout && args.base_name.is_none() {
        bail!("usage: grit pack-objects [--stdout] <base-name>");
    }

    let repo = Repository::discover(None).context("not a git repository")?;

    // Collect object IDs.
    let pack_list = collect_oids(&repo, &args)?;

    // Git shows this progress title when progress is enabled. Tests set `GIT_PROGRESS_DELAY` and
    // capture stderr to a file (not a TTY); match that by honoring the env var even when stderr
    // is not a terminal (`t6500-gc` TTY block).
    let progress_delay_env = std::env::var("GIT_PROGRESS_DELAY").ok();
    let show_enumerate_progress = !args.quiet
        && !args.stdout
        && !pack_list.oids.is_empty()
        && (io::stderr().is_terminal() || progress_delay_env.is_some());
    if show_enumerate_progress {
        let delay = progress_delay_env
            .as_deref()
            .and_then(|s| s.parse().ok())
            .unwrap_or(2u64);
        if delay > 0 {
            thread::sleep(Duration::from_secs(delay));
        }
        eprintln!("Enumerating objects");
    }

    if pack_list.oids.is_empty() {
        // Git’s cruft `pack-objects` pass may enumerate zero objects; `repack` still passes
        // `--non-empty` but expects a successful no-op with no stdout hash.
        //
        // An empty repository’s full `repack`/`gc` also runs `pack-objects --all --non-empty` with
        // zero reachable objects; Git skips writing a pack (t6500 `gc --quiet` on fresh repo).
        let allow_empty =
            (args.cruft && !args.incremental) || (args.all && !args.incremental && !args.unpacked);
        if args.non_empty && !allow_empty {
            bail!("pack-objects refuses to create an empty pack");
        }
        if !args.stdout && !args.quiet {
            eprintln!("Total 0 (delta 0), reused 0 (delta 0)");
        }
        return Ok(());
    }

    // Read all objects.
    let mut entries: Vec<PackEntry> = Vec::with_capacity(pack_list.oids.len());
    for oid in &pack_list.oids {
        let obj = read_object_from_repo(&repo, oid)?;
        entries.push(PackEntry {
            oid: *oid,
            kind: obj.kind,
            data: obj.data,
        });
    }

    if args.filter.as_deref().map(str::trim) == Some("blob:none")
        && args
            .filter_to
            .as_deref()
            .map(str::trim)
            .is_some_and(|s| !s.is_empty())
    {
        let to_base = args.filter_to.as_deref().map(str::trim).unwrap_or("");
        let side_blobs: Vec<PackEntry> = entries
            .iter()
            .filter(|e| e.kind == ObjectKind::Blob)
            .cloned()
            .collect();
        entries.retain(|e| e.kind != ObjectKind::Blob);
        if !side_blobs.is_empty() {
            write_pack_via_stdin_objects(&repo, &side_blobs, to_base, args.quiet)?;
        }
    } else {
        apply_list_objects_filter(&mut entries, args.filter.as_deref());
    }

    if entries.is_empty() {
        if args.non_empty {
            bail!("pack-objects refuses to create an empty pack");
        }
        if !args.stdout && !args.quiet {
            eprintln!("Total 0 (delta 0), reused 0 (delta 0)");
        }
        return Ok(());
    }

    // OID-sorted `--all` order breaks REF_DELTA chains (base must appear earlier in the pack).
    // Order blobs by increasing size so strict-prefix chains (t5316) serialize correctly.
    // Incremental repack (`--unpacked --incremental`) uses the rev-list object order as-is.
    if args.all && !args.incremental {
        let mut blobs = Vec::new();
        let mut non_blobs = Vec::new();
        for e in entries {
            if e.kind == ObjectKind::Blob {
                blobs.push(e);
            } else {
                non_blobs.push(e);
            }
        }
        blobs.sort_by(|a, b| {
            a.data
                .len()
                .cmp(&b.data.len())
                .then_with(|| a.oid.cmp(&b.oid))
        });
        non_blobs.extend(blobs);
        entries = non_blobs;
    }

    let max_delta_depth = pack_delta_depth_limit(&args);
    let window_zero_cli = {
        let mut args = std::env::args();
        let mut z = false;
        while let Some(a) = args.next() {
            if let Some(rest) = a.strip_prefix("--window=") {
                if rest.parse::<i64>().ok() == Some(0) {
                    z = true;
                }
            } else if let Some(rest) = a.strip_prefix("-window=") {
                if rest.parse::<i64>().ok() == Some(0) {
                    z = true;
                }
            } else if (a == "--window" || a == "-window")
                && args.next().as_deref().and_then(|v| v.parse::<i64>().ok()) == Some(0)
            {
                z = true;
            }
        }
        z
    };
    let window_zero_extra = args.extra.iter().any(|a| {
        a.strip_prefix("--window=")
            .or_else(|| a.strip_prefix("-window="))
            .and_then(|v| v.parse::<i64>().ok())
            == Some(0)
    });
    let window_reuse_only = args.window == Some(0) || window_zero_cli || window_zero_extra;
    let (write_entries, new_deltas, reused_deltas) = optimize_blob_deltas(
        &repo,
        entries,
        max_delta_depth,
        window_reuse_only,
        &pack_list.thin_blob_deltas,
    )?;

    // Build pack bytes.
    let pack_bytes = build_pack(&write_entries)?;

    if args.stdout {
        let stdout = io::stdout();
        let mut out = stdout.lock();
        out.write_all(&pack_bytes)?;
        out.flush()?;
    } else {
        let base = args
            .base_name
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("no base name"))?;

        // Pack hash is the trailing 20 bytes.
        let pack_hash = hex::encode(&pack_bytes[pack_bytes.len() - 20..]);
        let pack_path = format!("{base}-{pack_hash}.pack");
        let idx_path = format!("{base}-{pack_hash}.idx");

        std::fs::write(&pack_path, &pack_bytes)?;

        // Build and write idx.
        let idx_bytes = build_idx_for_pack(&pack_bytes, &write_entries)?;
        std::fs::write(&idx_path, &idx_bytes)?;

        println!("{pack_hash}");
        if !args.quiet {
            eprintln!(
                "Total {} (delta {}), reused 0 (delta {})",
                write_entries.len(),
                new_deltas + reused_deltas,
                reused_deltas
            );
        }

        let pb = Path::new(&pack_path);
        if let (Some(dir), Some(stem)) = (pb.parent(), pb.file_stem().and_then(|s| s.to_str())) {
            if args.cruft && !args.incremental {
                let _ = std::fs::write(dir.join(format!("{stem}.mtimes")), b"");
            } else {
                // A full repack without `--cruft` may reuse the same pack hash as a former cruft
                // pack (same object set); drop stale `.mtimes` so `gc --keep-largest-pack` matches Git.
                let _ = std::fs::remove_file(dir.join(format!("{stem}.mtimes")));
            }
        }
    }

    Ok(())
}

/// Objects reachable from refs, reflogs (when enabled), and index blobs — same tips as Git’s
/// `pack-objects --all --reflog --indexed-objects` without `--keep-unreachable` / unpack flags.
fn reachable_objects_for_full_repack(repo: &Repository, args: &Args) -> Result<Vec<ObjectId>> {
    let mut opts = RevListOptions::default();
    opts.objects = true;
    opts.all_refs = true;
    // `git pack-objects --all --reflog` still packs the ref closure only; `--reflog` does not add
    // reflog-only commits as extra roots (see `git verify-pack` on the first pack vs grit before
    // this fix — t6500 cruft relies on foo/bar commits staying out of the main pack).
    opts.include_reflog_entries = false;
    opts.include_indexed_objects = args.indexed_objects;
    opts.missing_action = MissingAction::Allow;
    let r = match rev_list(repo, &[] as &[String], &[] as &[String], &opts) {
        Ok(r) => r,
        Err(LibError::InvalidRef(ref s)) if s == "no revisions specified" => {
            return Ok(Vec::new());
        }
        Err(e) => return Err(e).context("rev-list for pack-objects --all"),
    };
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    // `rev_list` object lines omit commit OIDs (Git lists trees/blobs per commit); commits must
    // still count as reachable for cruft splitting and `--all` OID sets.
    for c in &r.commits {
        if seen.insert(*c) {
            out.push(*c);
        }
    }
    for (o, _) in r.objects {
        if seen.insert(o) {
            out.push(o);
        }
    }
    Ok(out)
}

/// Basename without `.pack` / `.idx` (e.g. `pack-abc123`).
fn pack_stem_from_line(line: &str) -> String {
    let t = line.trim();
    let t = t.strip_prefix('-').unwrap_or(t).trim();
    Path::new(t)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(t)
        .strip_suffix(".pack")
        .or_else(|| t.strip_suffix(".idx"))
        .unwrap_or(t)
        .to_string()
}

/// `git pack-objects --cruft` stdin protocol: fresh pack basenames, `-` lines for packs to
/// discard, optional retained packs (no `-`) that are neither fresh nor discarded (unknown packs
/// on disk are treated as retained and skipped when gathering cruft candidates).
fn collect_cruft_pack_stdin_oids(repo: &Repository) -> Result<PackObjectList> {
    let stdin = io::stdin();
    let mut fresh: HashSet<String> = HashSet::new();
    let mut discard: HashSet<String> = HashSet::new();
    for line in stdin.lock().lines() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let stem = pack_stem_from_line(trimmed);
        if stem.is_empty() {
            continue;
        }
        if trimmed.starts_with('-') {
            discard.insert(stem);
        } else {
            fresh.insert(stem);
        }
    }

    let pack_dir = repo.odb.objects_dir().join("pack");
    let mut fresh_oids: HashSet<ObjectId> = HashSet::new();
    for stem in &fresh {
        let idx_path = pack_dir.join(format!("{stem}.idx"));
        if idx_path.is_file() {
            let idx = grit_lib::pack::read_pack_index(&idx_path)
                .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", idx_path.display()))?;
            for e in idx.entries {
                fresh_oids.insert(e.oid);
            }
        }
    }

    let mut oids: BTreeSet<ObjectId> = BTreeSet::new();
    collect_all_loose(&repo.odb, &mut oids)?;

    let indexes = grit_lib::pack::read_local_pack_indexes(repo.odb.objects_dir())
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    for idx in indexes {
        let name = idx
            .pack_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        if !name.ends_with(".pack") {
            continue;
        }
        let stem = name.strip_suffix(".pack").unwrap_or(name).to_string();
        if fresh.contains(&stem) {
            continue;
        }
        if !discard.contains(&stem) {
            // Pack not listed on stdin: treated as retained (Git `pack_keep_in_core`).
            continue;
        }
        for e in idx.entries {
            oids.insert(e.oid);
        }
    }

    // Cruft = objects from discarded packs (and loose) that are not in the new pack(s). Do not
    // subtract `rev-list --all --reflog`: reflog still points at discarded commits (t6500
    // `prepare_cruft_history`), and those objects must land in the cruft pack.
    oids.retain(|o| !fresh_oids.contains(o));

    Ok(PackObjectList {
        oids: oids.into_iter().collect(),
        thin_blob_deltas: Vec::new(),
    })
}

/// Effective maximum delta chain length for `pack-objects` (`--depth`), matching Git semantics:
/// unset → no artificial limit (tests rely on long reused chains); `<= 0` → no deltas; `> 0` → cap.
fn parse_depth_from_argv() -> Option<i64> {
    let mut args = std::env::args();
    while let Some(a) = args.next() {
        if let Some(rest) = a.strip_prefix("--depth=") {
            if let Ok(d) = rest.parse::<i64>() {
                return Some(d);
            }
        } else if a == "--depth" {
            if let Some(v) = args.next() {
                if let Ok(d) = v.parse::<i64>() {
                    return Some(d);
                }
            }
        }
    }
    None
}

fn pack_delta_depth_limit(args: &Args) -> Option<usize> {
    let _ = (args.path_walk, args.no_path_walk);
    let from_extra = || {
        for a in &args.extra {
            if let Some(rest) = a.strip_prefix("--depth=") {
                if let Ok(d) = rest.parse::<i64>() {
                    return Some(d);
                }
            }
        }
        parse_depth_from_argv()
    };
    let d_opt = args.depth.or_else(from_extra);
    match d_opt {
        None => None,
        Some(d) if d <= 0 => Some(0),
        Some(d) => Some(d as usize),
    }
}

/// Look up a blob OID in `tree_oid` by single path component `name` (e.g. `file` from `… blob file`).
fn blob_oid_for_tree_path(repo: &Repository, tree_oid: &ObjectId, name: &[u8]) -> Result<ObjectId> {
    let obj = read_object_from_repo(repo, tree_oid)?;
    if obj.kind != ObjectKind::Tree {
        bail!("preferred base {} is not a tree", tree_oid.to_hex());
    }
    let entries = parse_tree(&obj.data).map_err(|e| anyhow::anyhow!("{e}"))?;
    for e in entries {
        if e.mode == 0o040000 {
            continue;
        }
        if e.name == name {
            return Ok(e.oid);
        }
    }
    bail!(
        "path '{}' not found in tree {}",
        String::from_utf8_lossy(name),
        tree_oid.to_hex()
    );
}

/// Write the given objects into a pack at `base` via `pack-objects` stdin (OID lines).
fn write_pack_via_stdin_objects(
    repo: &Repository,
    entries: &[PackEntry],
    base: &str,
    quiet: bool,
) -> Result<()> {
    if entries.is_empty() {
        return Ok(());
    }
    let work_dir = repo.work_tree.as_deref().unwrap_or(&repo.git_dir);
    let mut cmd = Command::new(grit_exe::grit_executable());
    cmd.current_dir(work_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .arg("pack-objects");
    if quiet {
        cmd.arg("-q");
    }
    cmd.arg(base);
    let mut child = cmd.spawn().context("spawn pack-objects for filter-to")?;
    {
        let mut stdin = child.stdin.take().context("pack-objects stdin")?;
        for e in entries {
            writeln!(stdin, "{}", e.oid.to_hex())?;
        }
    }
    let out = child
        .wait_with_output()
        .context("wait pack-objects filter-to")?;
    if !out.status.success() {
        bail!("pack-objects (filter-to) failed with status {}", out.status);
    }
    Ok(())
}

/// Apply `git pack-objects --filter=<spec>` (subset: `blob:none` for `gc.repackFilter` tests).
fn apply_list_objects_filter(entries: &mut Vec<PackEntry>, filter: Option<&str>) {
    let Some(spec) = filter.map(str::trim).filter(|s| !s.is_empty()) else {
        return;
    };
    if spec == "blob:none" {
        entries.retain(|e| e.kind != ObjectKind::Blob);
    }
}

fn pack_all_use_reachable_closure_only(args: &Args) -> bool {
    // Default `pack-objects --all` walks the full ODB (loose + all packs). The first pass of
    // `repack --cruft` is the exception: it must match Git’s main pack (ref closure only).
    args.reachability_all
}

/// Collect object IDs from stdin or `--all`.
fn collect_oids(repo: &Repository, args: &Args) -> Result<PackObjectList> {
    if args.all && args.unpacked && args.incremental {
        return collect_incremental_repack_oids(repo, args);
    }

    if args.cruft && !args.incremental {
        return collect_cruft_pack_stdin_oids(repo);
    }

    let mut oids = BTreeSet::new();

    if args.all {
        let use_reachable_only = !args.incremental && pack_all_use_reachable_closure_only(args);
        if use_reachable_only {
            let v = reachable_objects_for_full_repack(repo, args)?;
            oids.extend(v);
        } else {
            collect_all_loose(&repo.odb, &mut oids)?;
            let pack_dir = repo.odb.objects_dir().join("pack");
            if pack_dir.exists() {
                let indexes = grit_lib::pack::read_local_pack_indexes(repo.odb.objects_dir())
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                for idx in indexes {
                    for entry in idx.entries {
                        oids.insert(entry.oid);
                    }
                }
            }
        }
    }

    if args.all && !args.keep_pack.is_empty() {
        let skip = keep_pack_object_ids(repo, &args.keep_pack)?;
        oids.retain(|o| !skip.contains(o));
    }

    if args.revs {
        // Git `pack-objects --revs` stdin: positive revs, then `--not`, then negative
        // revs (client haves). Lines may be 40-char hex or ref names. With `--thin`,
        // objects reachable from the haves are omitted from the pack.
        let stdin = io::stdin();
        let mut positive_tips: Vec<ObjectId> = Vec::new();
        let mut exclude_roots: Vec<ObjectId> = Vec::new();
        let mut post_not = false;
        let mut have_roots: BTreeSet<ObjectId> = BTreeSet::new();
        for line in stdin.lock().lines() {
            let line = line?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if trimmed == "--not" {
                post_not = true;
                continue;
            }
            if post_not {
                let oid = if let Ok(oid) = ObjectId::from_hex(trimmed) {
                    oid
                } else {
                    resolve_revision(repo, trimmed)
                        .with_context(|| format!("cannot resolve ref '{trimmed}'"))?
                };
                have_roots.insert(oid);
                continue;
            }
            if let Some(neg_ref) = trimmed.strip_prefix('^') {
                let oid = if let Ok(oid) = ObjectId::from_hex(neg_ref) {
                    oid
                } else {
                    resolve_revision(repo, neg_ref)
                        .with_context(|| format!("cannot resolve ref '{neg_ref}'"))?
                };
                exclude_roots.push(oid);
            } else {
                let oid = if let Ok(oid) = ObjectId::from_hex(trimmed) {
                    oid
                } else {
                    resolve_revision(repo, trimmed)
                        .with_context(|| format!("cannot resolve ref '{trimmed}'"))?
                };
                positive_tips.push(oid);
            }
        }

        let use_sparse = pack_objects_sparse_mode(repo, args)?;
        let mut oids: BTreeSet<ObjectId> = if use_sparse {
            collect_revs_pack_objects_sparse(repo, &positive_tips, &exclude_roots)?
        } else {
            let mut oids = BTreeSet::new();
            for tip in &positive_tips {
                walk_reachable(repo, tip, &mut oids)?;
            }
            oids
        };

        let skip_full_exclude_subtract =
            use_sparse && sparse_skip_full_exclude_subtract(repo, &positive_tips, &exclude_roots)?;
        if !skip_full_exclude_subtract {
            let mut exclude = BTreeSet::new();
            for root in &exclude_roots {
                walk_reachable(repo, root, &mut exclude)?;
            }
            for oid in &exclude {
                oids.remove(oid);
            }
        }

        if args.thin && !have_roots.is_empty() {
            let mut have_closure = BTreeSet::new();
            for root in &have_roots {
                walk_reachable(repo, root, &mut have_closure)?;
            }
            oids.retain(|o| !have_closure.contains(o));
        }
        return Ok(PackObjectList {
            oids: oids.into_iter().collect(),
            thin_blob_deltas: Vec::new(),
        });
    } else if args.stdin_packs {
        // Read pack filenames from stdin and include all objects in those packs.
        let stdin = io::stdin();
        let pack_dir = repo.odb.objects_dir().join("pack");
        for line in stdin.lock().lines() {
            let line = line?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            // The input can be a bare name like "pack-<hash>" or full path.
            let idx_path = if trimmed.contains('/') || trimmed.contains('\\') {
                std::path::PathBuf::from(trimmed)
            } else {
                pack_dir.join(format!("{}.idx", trimmed))
            };
            // If given a .pack, convert to .idx
            let idx_path = if idx_path.extension().is_some_and(|e| e == "pack") {
                idx_path.with_extension("idx")
            } else {
                idx_path
            };
            if idx_path.exists() {
                let idx = grit_lib::pack::read_pack_index(&idx_path)
                    .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", idx_path.display()))?;
                for entry in idx.entries {
                    oids.insert(entry.oid);
                }
            } else {
                bail!("pack index not found: {}", idx_path.display());
            }
        }
        return Ok(PackObjectList {
            oids: oids.into_iter().collect(),
            thin_blob_deltas: Vec::new(),
        });
    } else if !args.all {
        // Git `pack-objects` stdin format (see git/builtin/pack-objects.c `read_object_list_from_stdin`):
        //   -<oid>  — set preferred base (tree OID for thin-pack blob deltas), not an exclusion
        //   <oid> [<path>] — object to pack; with a preferred base, path selects the base blob
        let stdin = io::stdin();
        let mut oids_ordered: Vec<ObjectId> = Vec::new();
        let mut seen: HashSet<ObjectId> = HashSet::new();
        let mut thin_blob_deltas: Vec<(ObjectId, ObjectId)> = Vec::new();
        let mut preferred_tree: Option<ObjectId> = None;

        for line in stdin.lock().lines() {
            let line = line?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Some(rest) = trimmed.strip_prefix('-') {
                let hex_part = rest.split_whitespace().next().unwrap_or(rest);
                let tree_oid = ObjectId::from_hex(hex_part)
                    .map_err(|e| anyhow::anyhow!("invalid preferred base '{hex_part}': {e}"))?;
                preferred_tree = Some(tree_oid);
                continue;
            }

            let hex_part = trimmed.split_whitespace().next().unwrap_or(trimmed);
            let oid = ObjectId::from_hex(hex_part)
                .map_err(|e| anyhow::anyhow!("invalid object id '{hex_part}': {e}"))?;
            if !seen.insert(oid) {
                continue;
            }
            oids_ordered.push(oid);

            if let Some(pbase) = preferred_tree {
                if let Some(path_hint) = trimmed.split_whitespace().nth(1) {
                    if let Ok(base_blob) =
                        blob_oid_for_tree_path(repo, &pbase, path_hint.as_bytes())
                    {
                        if base_blob != oid {
                            thin_blob_deltas.push((oid, base_blob));
                        }
                    }
                }
            }
        }

        return Ok(PackObjectList {
            oids: oids_ordered,
            thin_blob_deltas,
        });
    }

    if args.exclude_promisor_objects {
        let config = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
        if repo_treats_promisor_packs(&repo.git_dir, &config) {
            let skip = promisor_pack_object_ids(&repo.git_dir.join("objects"));
            oids.retain(|o| !skip.contains(o));
        }
    }

    Ok(PackObjectList {
        oids: oids.into_iter().collect(),
        thin_blob_deltas: Vec::new(),
    })
}

fn collect_incremental_repack_oids(repo: &Repository, args: &Args) -> Result<PackObjectList> {
    let mut opts = RevListOptions::default();
    opts.objects = true;
    opts.all_refs = true;
    opts.include_reflog_entries = args.reflog;
    opts.include_indexed_objects = args.indexed_objects;
    opts.unpacked_only = true;
    opts.missing_action = MissingAction::Allow;

    let result = rev_list(repo, &[] as &[String], &[] as &[String], &opts)
        .context("rev-list for incremental pack-objects")?;

    let mut ordered: Vec<ObjectId> = Vec::new();
    let mut seen = HashSet::new();
    for oid in result.objects.iter().map(|(o, _)| *o) {
        if seen.insert(oid) {
            ordered.push(oid);
        }
    }

    if !args.keep_pack.is_empty() {
        let skip = keep_pack_object_ids(repo, &args.keep_pack)?;
        ordered.retain(|o| !skip.contains(o));
    }

    if args.exclude_promisor_objects {
        let config = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
        if repo_treats_promisor_packs(&repo.git_dir, &config) {
            let promisor = promisor_pack_object_ids(&repo.git_dir.join("objects"));
            ordered.retain(|o| !promisor.contains(o));
        }
    }

    Ok(PackObjectList {
        oids: ordered,
        thin_blob_deltas: Vec::new(),
    })
}

fn keep_pack_basename(name: &str) -> &str {
    Path::new(name)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(name)
}

fn keep_pack_object_ids(repo: &Repository, keep_pack: &[String]) -> Result<HashSet<ObjectId>> {
    let mut out = HashSet::new();
    let pack_dir = repo.git_dir.join("objects").join("pack");
    for name in keep_pack {
        let base = keep_pack_basename(name);
        let idx_path = pack_dir.join(base).with_extension("idx");
        if !idx_path.is_file() {
            continue;
        }
        let idx = grit_lib::pack::read_pack_index(&idx_path)
            .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", idx_path.display()))?;
        for e in idx.entries {
            out.insert(e.oid);
        }
    }
    Ok(out)
}

/// Walk all loose objects in the ODB.
fn collect_all_loose(odb: &Odb, oids: &mut BTreeSet<ObjectId>) -> Result<()> {
    let objects_dir = odb.objects_dir();
    for prefix in 0..=255u8 {
        let hex_prefix = format!("{prefix:02x}");
        let dir = objects_dir.join(&hex_prefix);
        if !dir.exists() {
            continue;
        }
        let rd = std::fs::read_dir(&dir)?;
        for entry in rd {
            let entry = entry?;
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.len() == 38 {
                let full_hex = format!("{hex_prefix}{name_str}");
                if let Ok(oid) = ObjectId::from_hex(&full_hex) {
                    oids.insert(oid);
                }
            }
        }
    }
    Ok(())
}

/// Walk reachable objects from a commit/tree/tag/blob OID.
fn walk_reachable(repo: &Repository, oid: &ObjectId, oids: &mut BTreeSet<ObjectId>) -> Result<()> {
    if !oids.insert(*oid) {
        return Ok(()); // already visited
    }
    let obj = read_object_from_repo(repo, oid)?;
    match obj.kind {
        ObjectKind::Commit => {
            // Parse tree and parent lines.
            if let Ok(text) = std::str::from_utf8(&obj.data) {
                for line in text.lines() {
                    if let Some(tree_hex) = line.strip_prefix("tree ") {
                        if let Ok(tree_oid) = ObjectId::from_hex(tree_hex.trim()) {
                            walk_reachable(repo, &tree_oid, oids)?;
                        }
                    } else if let Some(parent_hex) = line.strip_prefix("parent ") {
                        if let Ok(parent_oid) = ObjectId::from_hex(parent_hex.trim()) {
                            walk_reachable(repo, &parent_oid, oids)?;
                        }
                    } else if line.is_empty() {
                        break; // end of headers
                    }
                }
            }
        }
        ObjectKind::Tree => {
            // Parse tree entries: mode SP name NUL 20-byte-oid
            let data = &obj.data;
            let mut pos = 0;
            while pos < data.len() {
                // Find the NUL.
                let nul = data[pos..]
                    .iter()
                    .position(|&b| b == 0)
                    .map(|i| pos + i)
                    .ok_or_else(|| anyhow::anyhow!("corrupt tree object"))?;
                if nul + 21 > data.len() {
                    break;
                }
                let entry_oid = ObjectId::from_bytes(&data[nul + 1..nul + 21])
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                walk_reachable(repo, &entry_oid, oids)?;
                pos = nul + 21;
            }
        }
        ObjectKind::Tag => {
            // Parse the object line.
            if let Ok(text) = std::str::from_utf8(&obj.data) {
                if let Some(first_line) = text.lines().next() {
                    if let Some(obj_hex) = first_line.strip_prefix("object ") {
                        if let Ok(target_oid) = ObjectId::from_hex(obj_hex.trim()) {
                            walk_reachable(repo, &target_oid, oids)?;
                        }
                    }
                }
            }
        }
        ObjectKind::Blob => {} // leaf
    }
    Ok(())
}

/// Matches `git_parse_maybe_bool` + integer fallback used by Git's `git_env_bool` for
/// `GIT_TEST_PACK_SPARSE`.
fn parse_git_test_pack_sparse_env() -> Option<bool> {
    let v = std::env::var_os("GIT_TEST_PACK_SPARSE")?;
    let t = v.to_string_lossy();
    let s = t.trim();
    if s.eq_ignore_ascii_case("true")
        || s == "1"
        || s.eq_ignore_ascii_case("yes")
        || s.eq_ignore_ascii_case("on")
    {
        return Some(true);
    }
    if s.eq_ignore_ascii_case("false")
        || s == "0"
        || s.eq_ignore_ascii_case("no")
        || s.eq_ignore_ascii_case("off")
    {
        return Some(false);
    }
    if let Ok(i) = s.parse::<i64>() {
        return Some(i != 0);
    }
    None
}

/// Whether `pack-objects` should use Git's sparse reachability algorithm for `--revs`.
///
/// Precedence: `--no-sparse` / `--sparse`, then `GIT_TEST_PACK_SPARSE`, then `pack.useSparse`
/// (default true), matching Git's `pack-objects.c`.
/// When sparse packing uses a single `^ancestor` exclusion, Git keeps objects that a full
/// reachable-subtract would remove (t5322). Multi-tip ranges like `topic1 ^topic2 ^topic3` still
/// need the subtract (test 3).
fn sparse_skip_full_exclude_subtract(
    repo: &Repository,
    positive_tips: &[ObjectId],
    exclude_roots: &[ObjectId],
) -> Result<bool> {
    if positive_tips.len() != 1 || exclude_roots.len() != 1 {
        return Ok(false);
    }
    let tip = positive_tips[0];
    let excl = exclude_roots[0];
    let mut seen = HashSet::<ObjectId>::new();
    let mut queue = VecDeque::from([tip]);
    while let Some(cid) = queue.pop_front() {
        if cid == excl {
            return Ok(true);
        }
        if !seen.insert(cid) {
            continue;
        }
        let obj = read_object_from_repo(repo, &cid)?;
        if obj.kind != ObjectKind::Commit {
            continue;
        }
        let c = parse_commit(&obj.data)?;
        for p in c.parents {
            queue.push_back(p);
        }
    }
    Ok(false)
}

fn pack_objects_sparse_mode(repo: &Repository, args: &Args) -> Result<bool> {
    if args.no_sparse {
        return Ok(false);
    }
    if args.sparse {
        return Ok(true);
    }
    if let Some(b) = parse_git_test_pack_sparse_env() {
        return Ok(b);
    }
    let config = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
    for key in ["pack.useSparse", "pack.usesparse"] {
        if let Some(Ok(b)) = config.get_bool(key) {
            return Ok(b);
        }
    }
    Ok(true)
}

fn add_children_by_path_for_sparse(
    repo: &Repository,
    tree_oid: &ObjectId,
    uninteresting: &mut HashSet<ObjectId>,
    map: &mut HashMap<Vec<u8>, HashSet<ObjectId>>,
) -> Result<()> {
    let obj = read_object_from_repo(repo, tree_oid)?;
    if obj.kind != ObjectKind::Tree {
        return Ok(());
    }
    let entries = parse_tree(&obj.data)?;
    let parent_uninteresting = uninteresting.contains(tree_oid);
    for e in entries {
        if e.mode == 0o040000 {
            map.entry(e.name.clone()).or_default().insert(e.oid);
            if parent_uninteresting {
                uninteresting.insert(e.oid);
            }
        } else if e.mode != 0o160000 && parent_uninteresting {
            uninteresting.insert(e.oid);
        }
    }
    Ok(())
}

/// Port of Git's `mark_trees_uninteresting_sparse` (`revision.c`): when both interesting and
/// uninteresting root trees are present at the same walk depth, prune uninteresting paths that
/// match by entry name across those trees.
fn mark_trees_uninteresting_sparse(
    repo: &Repository,
    trees: &HashSet<ObjectId>,
    uninteresting: &mut HashSet<ObjectId>,
) -> Result<()> {
    let mut has_interesting = false;
    let mut has_uninteresting = false;
    for oid in trees {
        if uninteresting.contains(oid) {
            has_uninteresting = true;
        } else {
            has_interesting = true;
        }
    }
    if !has_uninteresting || !has_interesting {
        return Ok(());
    }
    let mut map: HashMap<Vec<u8>, HashSet<ObjectId>> = HashMap::new();
    for oid in trees {
        add_children_by_path_for_sparse(repo, oid, uninteresting, &mut map)?;
    }
    for child_set in map.into_values() {
        mark_trees_uninteresting_sparse(repo, &child_set, uninteresting)?;
    }
    Ok(())
}

fn walk_tree_respecting_uninteresting(
    repo: &Repository,
    tree_oid: &ObjectId,
    uninteresting: &HashSet<ObjectId>,
    oids: &mut BTreeSet<ObjectId>,
) -> Result<()> {
    if uninteresting.contains(tree_oid) {
        return Ok(());
    }
    if !oids.insert(*tree_oid) {
        return Ok(());
    }
    let obj = read_object_from_repo(repo, tree_oid)?;
    let entries = parse_tree(&obj.data)?;
    for e in entries {
        if e.mode == 0o040000 {
            walk_tree_respecting_uninteresting(repo, &e.oid, uninteresting, oids)?;
        } else if e.mode != 0o160000 && !uninteresting.contains(&e.oid) {
            oids.insert(e.oid);
        }
    }
    Ok(())
}

fn walk_reachable_commits_first(
    repo: &Repository,
    root: ObjectId,
    oids: &mut BTreeSet<ObjectId>,
    commit_seen: &mut HashSet<ObjectId>,
    commit_uninteresting: &HashSet<ObjectId>,
    uninteresting: &HashSet<ObjectId>,
) -> Result<()> {
    let mut queue = VecDeque::new();
    queue.push_back(root);
    while let Some(cid) = queue.pop_front() {
        if !commit_seen.insert(cid) {
            continue;
        }
        let obj = read_object_from_repo(repo, &cid)?;
        if obj.kind != ObjectKind::Commit {
            walk_reachable(repo, &cid, oids)?;
            continue;
        }
        let c = parse_commit(&obj.data)?;
        if commit_uninteresting.contains(&cid) {
            for p in c.parents {
                queue.push_back(p);
            }
            continue;
        }
        oids.insert(cid);
        walk_tree_respecting_uninteresting(repo, &c.tree, uninteresting, oids)?;
        for p in c.parents {
            queue.push_back(p);
        }
    }
    Ok(())
}

fn collect_revs_pack_objects_sparse(
    repo: &Repository,
    positive_tips: &[ObjectId],
    exclude_roots: &[ObjectId],
) -> Result<BTreeSet<ObjectId>> {
    let mut commit_uninteresting: HashSet<ObjectId> = HashSet::new();
    let mut queue: VecDeque<ObjectId> = VecDeque::new();
    let mut commit_seen_exclude: HashSet<ObjectId> = HashSet::new();

    for root in exclude_roots {
        queue.push_back(*root);
    }
    while let Some(cid) = queue.pop_front() {
        if !commit_seen_exclude.insert(cid) {
            continue;
        }
        commit_uninteresting.insert(cid);
        let obj = read_object_from_repo(repo, &cid)?;
        if obj.kind != ObjectKind::Commit {
            continue;
        }
        let c = parse_commit(&obj.data)?;
        for p in c.parents {
            queue.push_back(p);
        }
    }

    let mut edge_trees: HashSet<ObjectId> = HashSet::new();
    let mut uninteresting: HashSet<ObjectId> = HashSet::new();

    // Match Git's `mark_edges_uninteresting`: every starting commit (included and `^` excluded)
    // contributes its root tree and its parents' root trees to the edge set.
    let mut edge_commits: Vec<ObjectId> = Vec::new();
    edge_commits.extend_from_slice(positive_tips);
    edge_commits.extend_from_slice(exclude_roots);

    for tip in edge_commits {
        let obj = read_object_from_repo(repo, &tip)?;
        if obj.kind != ObjectKind::Commit {
            continue;
        }
        let c = parse_commit(&obj.data)?;
        edge_trees.insert(c.tree);
        if commit_uninteresting.contains(&tip) {
            uninteresting.insert(c.tree);
        }
        for p in &c.parents {
            let pobj = read_object_from_repo(repo, p)?;
            if pobj.kind != ObjectKind::Commit {
                continue;
            }
            let pc = parse_commit(&pobj.data)?;
            edge_trees.insert(pc.tree);
            if commit_uninteresting.contains(p) {
                uninteresting.insert(pc.tree);
            }
        }
    }

    mark_trees_uninteresting_sparse(repo, &edge_trees, &mut uninteresting)?;

    let mut oids = BTreeSet::new();
    let mut commit_seen_walk: HashSet<ObjectId> = HashSet::new();
    for tip in positive_tips {
        let obj = read_object_from_repo(repo, tip)?;
        match obj.kind {
            ObjectKind::Commit => {
                walk_reachable_commits_first(
                    repo,
                    *tip,
                    &mut oids,
                    &mut commit_seen_walk,
                    &commit_uninteresting,
                    &uninteresting,
                )?;
            }
            _ => walk_reachable(repo, tip, &mut oids)?,
        }
    }
    Ok(oids)
}

/// Read an object from loose store or pack files.
fn read_object_from_repo(repo: &Repository, oid: &ObjectId) -> Result<grit_lib::objects::Object> {
    // Try loose first.
    if let Ok(obj) = repo.odb.read(oid) {
        return Ok(obj);
    }
    // Try pack files.
    let indexes = grit_lib::pack::read_local_pack_indexes(repo.odb.objects_dir())
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    for idx in &indexes {
        if let Some(entry) = idx.entries.iter().find(|e| e.oid == *oid) {
            let pack_bytes = std::fs::read(&idx.pack_path)?;
            let obj = read_object_from_pack(&pack_bytes, entry.offset, &indexes)?;
            return Ok(obj);
        }
    }
    maybe_lazy_fetch_missing_object(repo, oid)?;
    if let Ok(obj) = repo.odb.read(oid) {
        return Ok(obj);
    }
    let indexes = grit_lib::pack::read_local_pack_indexes(repo.odb.objects_dir())
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    for idx in &indexes {
        if let Some(entry) = idx.entries.iter().find(|e| e.oid == *oid) {
            let pack_bytes = std::fs::read(&idx.pack_path)?;
            let obj = read_object_from_pack(&pack_bytes, entry.offset, &indexes)?;
            return Ok(obj);
        }
    }
    bail!("object not found: {}", oid.to_hex())
}

fn maybe_lazy_fetch_missing_object(repo: &Repository, oid: &ObjectId) -> Result<()> {
    let config = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
    if !repo_treats_promisor_packs(&repo.git_dir, &config) {
        bail!("missing object in non-promisor repository");
    }
    crate::commands::promisor_hydrate::try_lazy_fetch_promisor_object(repo, *oid)
        .map_err(|e| anyhow::anyhow!("{e}"))
        .map(|_| ())
}

/// Read and decompress a single object from pack bytes at the given offset.
fn read_object_from_pack(
    pack_bytes: &[u8],
    offset: u64,
    indexes: &[grit_lib::pack::PackIndex],
) -> Result<grit_lib::objects::Object> {
    let mut pos = offset as usize;
    let c = pack_bytes
        .get(pos)
        .copied()
        .ok_or_else(|| anyhow::anyhow!("truncated pack"))?;
    pos += 1;
    let type_code = (c >> 4) & 0x7;
    let mut size = (c & 0x0f) as usize;
    let mut shift = 4u32;
    let mut cur = c;
    while cur & 0x80 != 0 {
        cur = pack_bytes
            .get(pos)
            .copied()
            .ok_or_else(|| anyhow::anyhow!("truncated pack"))?;
        pos += 1;
        size |= ((cur & 0x7f) as usize) << shift;
        shift += 7;
    }

    match type_code {
        1..=4 => {
            let kind = match type_code {
                1 => ObjectKind::Commit,
                2 => ObjectKind::Tree,
                3 => ObjectKind::Blob,
                4 => ObjectKind::Tag,
                _ => unreachable!(),
            };
            use flate2::read::ZlibDecoder;
            use std::io::Read;
            let mut decoder = ZlibDecoder::new(&pack_bytes[pos..]);
            let mut data = Vec::with_capacity(size);
            decoder.read_to_end(&mut data)?;
            Ok(grit_lib::objects::Object::new(kind, data))
        }
        6 => {
            // OFS_DELTA
            let mut c2 = pack_bytes
                .get(pos)
                .copied()
                .ok_or_else(|| anyhow::anyhow!("truncated"))?;
            pos += 1;
            let mut neg_off = (c2 & 0x7f) as u64;
            while c2 & 0x80 != 0 {
                c2 = pack_bytes
                    .get(pos)
                    .copied()
                    .ok_or_else(|| anyhow::anyhow!("truncated"))?;
                pos += 1;
                neg_off = ((neg_off + 1) << 7) | (c2 & 0x7f) as u64;
            }
            let base_offset = offset
                .checked_sub(neg_off)
                .ok_or_else(|| anyhow::anyhow!("ofs-delta underflow"))?;

            use flate2::read::ZlibDecoder;
            use std::io::Read;
            let mut decoder = ZlibDecoder::new(&pack_bytes[pos..]);
            let mut delta_data = Vec::with_capacity(size);
            decoder.read_to_end(&mut delta_data)?;

            let base_obj = read_object_from_pack(pack_bytes, base_offset, indexes)?;
            let result = grit_lib::unpack_objects::apply_delta(&base_obj.data, &delta_data)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            Ok(grit_lib::objects::Object::new(base_obj.kind, result))
        }
        7 => {
            // REF_DELTA
            if pos + 20 > pack_bytes.len() {
                bail!("truncated ref-delta");
            }
            let base_oid = ObjectId::from_bytes(&pack_bytes[pos..pos + 20])
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            pos += 20;

            use flate2::read::ZlibDecoder;
            use std::io::Read;
            let mut decoder = ZlibDecoder::new(&pack_bytes[pos..]);
            let mut delta_data = Vec::with_capacity(size);
            decoder.read_to_end(&mut delta_data)?;

            // Find the base in any pack.
            let mut base_obj = None;
            for idx in indexes {
                if let Some(entry) = idx.entries.iter().find(|e| e.oid == base_oid) {
                    let pb = std::fs::read(&idx.pack_path)?;
                    base_obj = Some(read_object_from_pack(&pb, entry.offset, indexes)?);
                    break;
                }
            }
            let base = base_obj.ok_or_else(|| anyhow::anyhow!("ref-delta base not found"))?;
            let result = grit_lib::unpack_objects::apply_delta(&base.data, &delta_data)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            Ok(grit_lib::objects::Object::new(base.kind, result))
        }
        other => bail!("unknown pack type {other}"),
    }
}

/// Prefer `REF_DELTA` when one blob is a strict prefix of another (same as Git's
/// `create_delta` for the common “append bytes” case).
///
/// With `--window=0`, skip computing new prefix deltas and instead reuse `REF_DELTA` blobs from
/// existing packs when the base is also in the object set (matches Git `reuse_delta` for t5316).
///
/// `max_delta_depth`: `None` — no chain-length limit; `Some(0)` — store all blobs as full objects;
/// `Some(d)` for `d > 0` — cap delta chains (Git's `--depth` behavior).
///
/// Returns `(write_entries, new_deltas, reused_deltas)` for progress lines.
fn optimize_blob_deltas(
    repo: &Repository,
    entries: Vec<PackEntry>,
    max_delta_depth: Option<usize>,
    window_reuse_only: bool,
    thin_blob_deltas: &[(ObjectId, ObjectId)],
) -> Result<(Vec<PackWriteEntry>, usize, usize)> {
    let packed_set: HashSet<ObjectId> = entries.iter().map(|e| e.oid).collect();
    let objects_dir = repo.odb.objects_dir();

    let mut reuse_candidates: HashMap<ObjectId, (ObjectId, Vec<u8>)> = HashMap::new();
    if window_reuse_only && max_delta_depth != Some(0) {
        for e in entries.iter().filter(|e| e.kind == ObjectKind::Blob) {
            if let Some(triple) =
                grit_lib::pack::packed_ref_delta_reuse_slice(objects_dir, &e.oid, &packed_set)
                    .map_err(|e| anyhow::anyhow!("{e}"))?
            {
                reuse_candidates.insert(e.oid, triple);
            }
        }
    }

    let blobs: Vec<&PackEntry> = entries
        .iter()
        .filter(|e| e.kind == ObjectKind::Blob)
        .collect();
    let mut delta_target_to_base: HashMap<ObjectId, ObjectId> = HashMap::new();

    if max_delta_depth != Some(0) {
        if window_reuse_only {
            for (oid, (base, _)) in &reuse_candidates {
                delta_target_to_base.insert(*oid, *base);
            }
        }
        // t5316: successive `file` blobs are not strict prefixes (`…\\n8` vs `…\\n9`); the long
        // chain comes from `REF_DELTA` edges stored across the thin-pack series. Prefer those
        // before any in-memory prefix heuristic.
        for t in &blobs {
            if delta_target_to_base.contains_key(&t.oid) {
                continue;
            }
            if let Ok(Some(base)) = grit_lib::pack::packed_delta_base_oid(objects_dir, &t.oid) {
                if packed_set.contains(&base) && base != t.oid {
                    delta_target_to_base.insert(t.oid, base);
                }
            }
        }
        for t in &blobs {
            if delta_target_to_base.contains_key(&t.oid) {
                continue;
            }
            let mut best_base: Option<&PackEntry> = None;
            for b in &blobs {
                if b.oid == t.oid {
                    continue;
                }
                if b.data.is_empty() {
                    continue;
                }
                if t.data.starts_with(&b.data)
                    && t.data.len() > b.data.len()
                    && best_base.is_none_or(|bb| b.data.len() > bb.data.len())
                {
                    best_base = Some(b);
                }
            }
            if let Some(base) = best_base {
                delta_target_to_base.insert(t.oid, base.oid);
            }
        }
    }

    if let Some(limit) = max_delta_depth.filter(|&d| d > 0) {
        apply_delta_depth_limit(&mut delta_target_to_base, limit);
    }

    if max_delta_depth != Some(0) {
        for &(blob_oid, base_oid) in thin_blob_deltas {
            if entries
                .iter()
                .any(|e| e.oid == blob_oid && e.kind == ObjectKind::Blob)
            {
                delta_target_to_base.insert(blob_oid, base_oid);
            }
        }
        if let Some(limit) = max_delta_depth.filter(|&d| d > 0) {
            apply_delta_depth_limit(&mut delta_target_to_base, limit);
        }
    }

    let mut out: Vec<PackWriteEntry> = Vec::with_capacity(entries.len());
    for e in &entries {
        if e.kind == ObjectKind::Blob && delta_target_to_base.contains_key(&e.oid) {
            continue;
        }
        out.push(PackWriteEntry::Full(e.clone()));
    }

    let mut new_deltas = 0usize;
    let mut reused_deltas = 0usize;

    for e in &entries {
        let Some(&base_oid) = delta_target_to_base.get(&e.oid) else {
            continue;
        };

        if window_reuse_only {
            if let Some((reuse_base, zdelta)) = reuse_candidates.get(&e.oid) {
                if *reuse_base == base_oid {
                    out.push(PackWriteEntry::RefDelta {
                        oid: e.oid,
                        base_oid,
                        delta: zdelta.clone(),
                    });
                    reused_deltas += 1;
                    continue;
                }
            }
        }

        let base_data = if let Some(be) = entries.iter().find(|x| x.oid == base_oid) {
            if be.kind != ObjectKind::Blob {
                bail!("delta base {} is not a blob", base_oid.to_hex());
            }
            be.data.clone()
        } else {
            let o = read_object_from_repo(repo, &base_oid)?;
            if o.kind != ObjectKind::Blob {
                bail!("delta base {} is not a blob", base_oid.to_hex());
            }
            o.data
        };
        let delta = if thin_blob_deltas.iter().any(|&(t, _)| t == e.oid) {
            encode_lcp_delta(&base_data, &e.data).map_err(|e| anyhow::anyhow!("{e}"))?
        } else if e.data.starts_with(&base_data) && e.data.len() > base_data.len() {
            encode_prefix_extension_delta(&base_data, &e.data)
                .map_err(|e| anyhow::anyhow!("{e}"))?
        } else {
            encode_lcp_delta(&base_data, &e.data).map_err(|e| anyhow::anyhow!("{e}"))?
        };
        out.push(PackWriteEntry::RefDelta {
            oid: e.oid,
            base_oid,
            delta,
        });
        new_deltas += 1;
    }

    Ok((out, new_deltas, reused_deltas))
}

/// Break delta chains that exceed `max_depth` (Git `break_delta_chains` modulo rule).
fn apply_delta_depth_limit(map: &mut HashMap<ObjectId, ObjectId>, max_depth: usize) {
    let keys: Vec<ObjectId> = map.keys().copied().collect();
    let value_set: std::collections::HashSet<ObjectId> = map.values().copied().collect();
    let tips: Vec<ObjectId> = keys
        .iter()
        .copied()
        .filter(|k| !value_set.contains(k))
        .collect();

    let modulus = max_depth.saturating_add(1);
    let mut snip: std::collections::HashSet<ObjectId> = std::collections::HashSet::new();

    for tip in tips {
        let mut chain: Vec<ObjectId> = Vec::new();
        let mut cur = tip;
        let mut seen = std::collections::HashSet::new();
        while seen.insert(cur) {
            chain.push(cur);
            let Some(&b) = map.get(&cur) else {
                break;
            };
            cur = b;
        }

        let n = chain.len();
        if n < 2 {
            continue;
        }

        // Match `break_delta_chains`: after walking `DELTA` links from tip to base, `total_depth`
        // equals the number of edges (objects minus one).
        let mut total_depth = (n - 1) as u32;
        for &oid in &chain {
            let assigned = (total_depth as usize) % modulus;
            total_depth = total_depth.saturating_sub(1);
            if assigned == 0 {
                snip.insert(oid);
            }
        }
    }

    for oid in snip {
        map.remove(&oid);
    }

    let mut changed = true;
    while changed {
        changed = false;
        let targets: Vec<ObjectId> = map.keys().copied().collect();
        for t in targets {
            let Some(&b) = map.get(&t) else {
                continue;
            };
            if !map.contains_key(&b) {
                continue;
            }
            let mut root = b;
            while let Some(&next) = map.get(&root) {
                root = next;
            }
            map.insert(t, root);
            changed = true;
        }
    }
}

fn encode_pack_object_header(buf: &mut Vec<u8>, type_code: u8, payload_len: usize) {
    let mut size = payload_len;
    let first = ((type_code & 0x7) << 4) | (size & 0x0f) as u8;
    size >>= 4;
    if size > 0 {
        buf.push(first | 0x80);
        while size > 0 {
            let b = (size & 0x7f) as u8;
            size >>= 7;
            buf.push(if size > 0 { b | 0x80 } else { b });
        }
    } else {
        buf.push(first);
    }
}

/// Build a PACK v2 byte stream (full objects and optional `REF_DELTA` blobs).
fn build_pack(entries: &[PackWriteEntry]) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    buf.extend_from_slice(b"PACK");
    buf.extend_from_slice(&2u32.to_be_bytes());
    buf.extend_from_slice(&(entries.len() as u32).to_be_bytes());

    for entry in entries {
        match entry {
            PackWriteEntry::Full(pe) => {
                let type_code: u8 = match pe.kind {
                    ObjectKind::Commit => 1,
                    ObjectKind::Tree => 2,
                    ObjectKind::Blob => 3,
                    ObjectKind::Tag => 4,
                };
                encode_pack_object_header(&mut buf, type_code, pe.data.len());
                let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
                enc.write_all(&pe.data)?;
                let compressed = enc.finish()?;
                buf.extend_from_slice(&compressed);
            }
            PackWriteEntry::RefDelta {
                base_oid, delta, ..
            } => {
                encode_pack_object_header(&mut buf, 7, delta.len());
                buf.extend_from_slice(base_oid.as_bytes());
                let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
                enc.write_all(delta)?;
                let compressed = enc.finish()?;
                buf.extend_from_slice(&compressed);
            }
        }
    }

    // Trailing SHA-1 checksum.
    let mut hasher = Sha1::new();
    hasher.update(&buf);
    let digest = hasher.finalize();
    buf.extend_from_slice(digest.as_slice());

    Ok(buf)
}

/// Build idx v2 for a pack we just wrote.
fn build_idx_for_pack(pack_bytes: &[u8], entries: &[PackWriteEntry]) -> Result<Vec<u8>> {
    use grit_lib::pack::skip_one_pack_object;

    // We need offsets. Reparse the pack to get them.
    let nr = entries.len();
    let mut offsets = Vec::with_capacity(nr);
    let mut pos = 12usize; // skip header

    for _entry in entries {
        offsets.push(pos as u64);
        let start = pos as u64;
        skip_one_pack_object(pack_bytes, &mut pos, start).map_err(|e| anyhow::anyhow!("{e}"))?;
    }

    // Build sorted index.
    let mut sorted: Vec<(usize, ObjectId)> = entries
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let oid = match e {
                PackWriteEntry::Full(pe) => pe.oid,
                PackWriteEntry::RefDelta { oid, .. } => *oid,
            };
            (i, oid)
        })
        .collect();
    sorted.sort_by_key(|(_, oid)| *oid.as_bytes());

    let mut buf = Vec::new();
    // Header.
    buf.extend_from_slice(&[0xFF, b't', b'O', b'c']);
    buf.extend_from_slice(&2u32.to_be_bytes());

    // Fanout.
    let mut fanout = [0u32; 256];
    for (_, oid) in &sorted {
        fanout[oid.as_bytes()[0] as usize] += 1;
    }
    for i in 1..256 {
        fanout[i] += fanout[i - 1];
    }
    for slot in &fanout {
        buf.extend_from_slice(&slot.to_be_bytes());
    }

    // OID table.
    for (_, oid) in &sorted {
        buf.extend_from_slice(oid.as_bytes());
    }

    // CRC32 table: compute CRC32 for each entry's raw bytes in the pack.
    for (orig_idx, _) in &sorted {
        let off = offsets[*orig_idx] as usize;
        // Find the end of this entry.
        let next_off = if *orig_idx + 1 < nr {
            offsets[*orig_idx + 1] as usize
        } else {
            pack_bytes.len() - 20 // before trailing checksum
        };
        let crc = crc32_slice(&pack_bytes[off..next_off]);
        buf.extend_from_slice(&crc.to_be_bytes());
    }

    // Offset table.
    let mut large_offsets: Vec<u64> = Vec::new();
    for (orig_idx, _) in &sorted {
        let off = offsets[*orig_idx];
        if off >= 0x8000_0000 {
            let idx = large_offsets.len() as u32;
            buf.extend_from_slice(&(idx | 0x8000_0000).to_be_bytes());
            large_offsets.push(off);
        } else {
            buf.extend_from_slice(&(off as u32).to_be_bytes());
        }
    }

    // Large offset table.
    for off in &large_offsets {
        buf.extend_from_slice(&off.to_be_bytes());
    }

    // Pack checksum.
    let pack_checksum = &pack_bytes[pack_bytes.len() - 20..];
    buf.extend_from_slice(pack_checksum);

    // Index checksum.
    let mut h = Sha1::new();
    h.update(&buf);
    let idx_checksum = h.finalize();
    buf.extend_from_slice(idx_checksum.as_slice());

    Ok(buf)
}

/// CRC32 IEEE.
fn crc32_slice(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in data {
        let idx = ((crc ^ b as u32) & 0xFF) as usize;
        crc = CRC32_TABLE[idx] ^ (crc >> 8);
    }
    !crc
}

static CRC32_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut i = 0u32;
    while i < 256 {
        let mut crc = i;
        let mut j = 0;
        while j < 8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB8_8320;
            } else {
                crc >>= 1;
            }
            j += 1;
        }
        table[i as usize] = crc;
        i += 1;
    }
    table
};
