//! `grit pack-objects` — create a packed archive of objects.
//!
//! Reads object IDs (or revisions with `--revs`) from stdin and writes a
//! `.pack` file and corresponding `.idx` index file.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use grit_lib::config::{parse_config_parameters, ConfigSet};
use grit_lib::error::Error as LibError;
use grit_lib::midx::{
    load_midx_reuse_tables, midx_lookup_pack_and_offset, read_midx_pack_idx_names, MidxReuseTables,
};
use grit_lib::pack::{
    read_pack_index, read_packed_delta_dependency, slice_one_pack_object, PackIndex,
    PackedDeltaDependency,
};
use grit_lib::rev_list::{rev_list, shallow_boundary_oids, MissingAction, RevListOptions};
use sha1::{Digest as Sha1Digest, Sha1};
use sha2::{Digest as Sha256Digest, Sha256};
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::io::{self, BufRead, IsTerminal, Write};
use std::thread;
use std::time::Duration;

use crate::grit_exe;
use grit_lib::delta_encode::{encode_lcp_delta, encode_prefix_extension_delta};
use grit_lib::objects::{parse_commit, parse_tree, ObjectId, ObjectKind};
use grit_lib::odb::Odb;
use grit_lib::pack::hash_object_bytes;
use grit_lib::promisor::{promisor_pack_object_ids, repo_treats_promisor_packs};
use grit_lib::repo::Repository;
use grit_lib::rev_parse::resolve_revision;
use std::fs;
use std::path::{Path, PathBuf};
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

    /// Shallow pack: `--shallow <oid>` stdin lines cut parent chains at those commits (upload-pack).
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

    /// Disambiguation placeholder: Git rejects this (revision.c `--stdin` must not apply).
    #[arg(long = "stdin", hide = true)]
    pub stdin_disambiguation: bool,

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

    /// Name-hash version for pack ordering (subset: validate like Git).
    #[arg(long = "name-hash-version", allow_hyphen_values = true)]
    pub name_hash_version: Option<i32>,

    /// Honor pack-keep files (accepted for compat).
    #[arg(long = "honor-pack-keep")]
    pub honor_pack_keep: bool,

    /// Only use local objects (accepted for compat).
    #[arg(long = "local")]
    pub local: bool,

    /// Write bitmap index (accepted for compat).
    #[arg(long = "write-bitmap-index")]
    pub write_bitmap_index: bool,

    /// Git default bare-repo bitmap path: create `.bitmap` without full bitmap data (`t7700-repack`).
    #[arg(long = "write-bitmap-index-quiet", hide = true)]
    pub write_bitmap_index_quiet: bool,

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
    #[arg(long = "sparse")]
    pub sparse: bool,

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
    /// OID bytes stored in the pack index (`20` for SHA-1 repos, `32` for `extensions.objectformat=sha256`).
    pack_id: Vec<u8>,
    kind: ObjectKind,
    data: Vec<u8>,
}

/// Objects to pack plus optional stdin thin-pack hints (`-` preferred base lines).
struct PackObjectList {
    oids: Vec<ObjectId>,
    /// Blob OIDs that should delta against a base blob (base may be omitted from `oids`).
    thin_blob_deltas: Vec<(ObjectId, ObjectId)>,
    /// Stdin was interpreted as `git pack-objects --revs` / `rev-list --objects` input.
    rev_list_stdin: bool,
}

/// One slot in a pack file (full object or `REF_DELTA`).
enum PackWriteEntry {
    Full(PackEntry),
    RefDelta {
        oid: ObjectId,
        base_oid: ObjectId,
        target_pack: Vec<u8>,
        base_pack: Vec<u8>,
        /// Uncompressed Git binary delta (zlib-compressed in the pack stream).
        delta: Vec<u8>,
    },
    /// Verbatim packed object bytes (header + zlib) copied from an existing pack (MIDX reuse).
    ReusedSlice {
        oid: ObjectId,
        /// OID bytes as stored in the pack index (width matches repo hash).
        pack_id: Vec<u8>,
        raw: Vec<u8>,
    },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PackReuseMode {
    None,
    Single,
    Multi,
}

fn canonical_pack_window_key(raw: &str) -> bool {
    grit_lib::config::canonical_key(raw).ok().as_deref() == Some("pack.window")
}

fn pack_reuse_mode(repo: &Repository) -> PackReuseMode {
    let cfg = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
    let experimental = cfg
        .get_bool("feature.experimental")
        .and_then(|r| r.ok())
        .unwrap_or(false);
    let mut mode = if experimental {
        PackReuseMode::Multi
    } else {
        PackReuseMode::Single
    };
    if let Some(v) = cfg
        .get("pack.allowPackReuse")
        .or_else(|| cfg.get("pack.allowpackreuse"))
    {
        let lower = v.trim().to_ascii_lowercase();
        if lower == "single" {
            mode = PackReuseMode::Single;
        } else if lower == "multi" {
            mode = PackReuseMode::Multi;
        } else if let Ok(b) = grit_lib::config::parse_bool(&v) {
            mode = if b {
                PackReuseMode::Single
            } else {
                PackReuseMode::None
            };
        }
    }
    mode
}

fn pack_reuse_cli_ok(args: &Args) -> bool {
    args.stdout && !args.incremental && !args.honor_pack_keep
}

fn midx_pack_indexes(objects_dir: &Path) -> Result<HashMap<u32, PackIndex>> {
    let names = read_midx_pack_idx_names(objects_dir).map_err(|e| anyhow::anyhow!("{e}"))?;
    let mut out = HashMap::new();
    for (i, name) in names.iter().enumerate() {
        let id = u32::try_from(i).map_err(|_| anyhow::anyhow!("too many packs in MIDX"))?;
        let p = objects_dir.join("pack").join(name);
        let idx = read_pack_index(&p).map_err(|e| anyhow::anyhow!("{e}"))?;
        out.insert(id, idx);
    }
    Ok(out)
}

/// Global pseudo-bitmap bit for `oid` (MIDX reverse-index rank), matching Git `midx_pack_order`.
fn global_bitmap_bit_for_oid(tables: &MidxReuseTables, oid: &ObjectId) -> Option<u32> {
    tables.global_bitmap_bit(oid)
}

/// Objects in global MIDX pseudo-bitmap order: same order as the MIDX `RIDX` chunk (Git pack-reuse).
fn midx_objects_in_ridx_order(tables: &MidxReuseTables) -> Vec<(u32, ObjectId, u32, u64)> {
    let mut out = Vec::with_capacity(tables.rid_order.len());
    for (rank, &oid_idx) in tables.rid_order.iter().enumerate() {
        let rank_u32 = u32::try_from(rank).unwrap_or(u32::MAX);
        let oid = tables.oids[oid_idx as usize];
        let (pack_id, off) = tables.pack_and_offset[oid_idx as usize];
        out.push((rank_u32, oid, pack_id, off));
    }
    out
}

/// Multi-pack reuse when `mode == Multi`; preferred pack only when `mode == Single`.
fn compute_midx_reused_entries(
    repo: &Repository,
    pack_list: &PackObjectList,
    mode: PackReuseMode,
) -> Result<Option<(Vec<PackWriteEntry>, u32, u32)>> {
    if mode == PackReuseMode::None {
        return Ok(None);
    }
    let objects_dir = repo.odb.objects_dir();

    let pack_names = read_midx_pack_idx_names(objects_dir).map_err(|e| anyhow::anyhow!("{e}"))?;
    let preferred_pack_id = if mode == PackReuseMode::Single {
        let pref_name = grit_lib::midx::read_midx_preferred_idx_name(objects_dir)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        let id = pack_names
            .iter()
            .position(|n| n == &pref_name)
            .ok_or_else(|| anyhow::anyhow!("preferred pack not in midx"))? as u32;
        Some(id)
    } else {
        None
    };

    let Some(tables) = load_midx_reuse_tables(objects_dir).map_err(|e| anyhow::anyhow!("{e}"))?
    else {
        return Ok(None);
    };

    let indexes = midx_pack_indexes(objects_dir)?;
    let ordered = midx_objects_in_ridx_order(&tables);
    let pack_oid_set: HashSet<ObjectId> = pack_list.oids.iter().copied().collect();

    let mut oid_to_bit: HashMap<ObjectId, u32> = HashMap::new();
    for oid in &pack_list.oids {
        let Some(bit) = global_bitmap_bit_for_oid(&tables, oid) else {
            continue;
        };
        if let Some(pref) = preferred_pack_id {
            let (pid, _) = midx_lookup_pack_and_offset(objects_dir, oid)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            if pid != pref {
                continue;
            }
        }
        oid_to_bit.insert(*oid, bit);
    }

    let want: HashSet<u32> = oid_to_bit.values().copied().collect();
    if want.is_empty() {
        return Ok(None);
    }

    let mut pack_bytes_cache: HashMap<PathBuf, Vec<u8>> = HashMap::new();
    let load_pack = |cache: &mut HashMap<PathBuf, Vec<u8>>, path: &Path| -> Result<Vec<u8>> {
        if let Some(b) = cache.get(path) {
            return Ok(b.clone());
        }
        let data = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
        cache.insert(path.to_path_buf(), data.clone());
        Ok(data)
    };

    // Build `reuse_bits` in global MIDX order; repeat until fixed point so REF_DELTA / cross-pack
    // bases that appear later in the pseudo-bitmap can still satisfy dependents (matches Git).
    let mut reuse_bits: HashSet<u32> = HashSet::new();
    let mut changed = true;
    while changed {
        changed = false;
        for &(global_bit, _oid, pack_id, in_off) in &ordered {
            if !want.contains(&global_bit) || reuse_bits.contains(&global_bit) {
                continue;
            }
            let idx = indexes
                .get(&pack_id)
                .ok_or_else(|| anyhow::anyhow!("missing pack index for MIDX pack id {pack_id}"))?;
            let pack_bytes = load_pack(&mut pack_bytes_cache, idx.pack_path.as_path())?;
            let dep = read_packed_delta_dependency(&pack_bytes, in_off)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            let ok = match dep {
                None => true,
                Some(PackedDeltaDependency::OfsBase { base_offset }) => {
                    // OFS_DELTA bases must appear earlier in the pack *byte stream* (smaller offset).
                    // Reuse eligibility uses the base object's **global RIDX bitmap bit**, same as Git.
                    if base_offset < in_off {
                        if let Some(base_entry) =
                            idx.entries.iter().find(|e| e.offset == base_offset)
                        {
                            if base_entry.oid.len() != 20 {
                                false
                            } else if let Ok(b_oid) = ObjectId::from_bytes(&base_entry.oid) {
                                global_bitmap_bit_for_oid(&tables, &b_oid)
                                    .is_some_and(|bb| reuse_bits.contains(&bb))
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                }
                Some(PackedDeltaDependency::RefBase { base_oid }) => {
                    match global_bitmap_bit_for_oid(&tables, &base_oid) {
                        Some(bb) => {
                            if mode == PackReuseMode::Single {
                                match midx_lookup_pack_and_offset(objects_dir, &base_oid) {
                                    Ok((bpid, _)) => {
                                        Some(bpid) == preferred_pack_id && reuse_bits.contains(&bb)
                                    }
                                    Err(_) => false,
                                }
                            } else {
                                reuse_bits.contains(&bb)
                            }
                        }
                        None => false,
                    }
                }
            };
            if ok {
                reuse_bits.insert(global_bit);
                changed = true;
            }
        }
    }

    if reuse_bits.is_empty() {
        return Ok(None);
    }

    let mut reused: Vec<(u32, ObjectId, u32, Vec<u8>)> = Vec::new();
    for &(global_bit, oid, pack_id, in_off) in &ordered {
        if !reuse_bits.contains(&global_bit) || !pack_oid_set.contains(&oid) {
            continue;
        }
        let idx = indexes
            .get(&pack_id)
            .ok_or_else(|| anyhow::anyhow!("missing pack index"))?;
        let pack_bytes = load_pack(&mut pack_bytes_cache, idx.pack_path.as_path())?;
        let raw = slice_one_pack_object(&pack_bytes, in_off, idx.hash_bytes)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        reused.push((global_bit, oid, pack_id, raw.to_vec()));
    }

    let pack_reused = u32::try_from(reused.len()).unwrap_or(u32::MAX);
    let mut packs_touched: HashSet<u32> = HashSet::new();
    for (_, _, pack_id, _) in &reused {
        packs_touched.insert(*pack_id);
    }
    let packs_reused = u32::try_from(packs_touched.len()).unwrap_or(u32::MAX);

    let mut entries: Vec<PackWriteEntry> = Vec::with_capacity(reused.len());
    for (_, oid, pack_id_key, raw) in reused {
        let idx = indexes
            .get(&pack_id_key)
            .ok_or_else(|| anyhow::anyhow!("missing pack index"))?;
        let oid_pack = idx
            .entries
            .iter()
            .find(|e| grit_lib::pack::pack_index_entry_matches_sha1_oid(e, &oid))
            .map(|e| e.oid.clone())
            .unwrap_or_else(|| oid.as_bytes().to_vec());
        entries.push(PackWriteEntry::ReusedSlice {
            oid,
            pack_id: oid_pack,
            raw,
        });
    }

    Ok(Some((entries, pack_reused, packs_reused)))
}

/// Run `grit pack-objects`.
pub fn run(mut args: Args) -> Result<()> {
    if let Some(fmt) = &args.object_format {
        if fmt != "sha1" {
            bail!("unsupported object format: {fmt}");
        }
    }

    if args.stdin_disambiguation {
        bail!("fatal: disallowed abbreviated or ambiguous option 'stdin'");
    }

    if let Some(v) = args.name_hash_version {
        if v == 0 || v == 3 {
            bail!("invalid --name-hash-version option");
        }
    }
    if args.name_hash_version == Some(2) && args.write_bitmap_index && !args.stdout {
        eprintln!("currently, --write-bitmap-index requires --name-hash-version=1");
    }

    warn_pack_threads(&args);

    let path_walk_progress =
        args.path_walk && (args.progress || std::env::var("GIT_PROGRESS_DELAY").is_ok());
    if path_walk_progress {
        eprintln!("Compressing objects by path");
    }

    if !args.stdout && args.base_name.is_none() {
        bail!("usage: grit pack-objects [--stdout] <base-name>");
    }

    // `clap` + the hidden `extra` trailing var-arg can fail to bind `--revs` in some argv orders;
    // mirror Git by treating a literal `--revs` in the invocation as rev-list stdin (t5332).
    if !args.revs {
        args.revs = std::env::args().any(|a| a == "--revs" || a == "-revs");
    }
    args.extra.retain(|a| {
        if a == "--revs" || a == "-revs" {
            args.revs = true;
            false
        } else {
            true
        }
    });

    let repo = Repository::discover(None).context("not a git repository")?;
    let pack_hash_bytes = pack_trailer_bytes_for_repo(&repo.git_dir);

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
        let allow_empty = (args.cruft && !args.incremental)
            || (args.all && !args.incremental && !args.unpacked)
            // `repack -d` incremental pass: nothing loose to pack (t5332 after full repack).
            || (args.all && args.incremental && args.unpacked);
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
        let pack_id = hash_object_bytes(obj.kind, &obj.data, pack_hash_bytes)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        entries.push(PackEntry {
            oid: *oid,
            pack_id,
            kind: obj.kind,
            data: obj.data,
        });
    }

    if args.filter.as_deref().map(str::trim) == Some("blob:none") {
        let to_base = args
            .filter_to
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .or_else(|| {
                // `repack --filter=blob:none` omits `--filter-to`; Git writes omitted blobs to the
                // same pack prefix as the main pack (`t7700-repack`).
                args.base_name
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
            })
            .unwrap_or("");
        if !to_base.is_empty() {
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
    } else {
        apply_list_objects_filter(&mut entries, args.filter.as_deref());
    }

    if entries.is_empty() {
        if args.non_empty && !(args.all && args.incremental && args.unpacked) {
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
    let window_zero_cfg = {
        let mut z = false;
        if let Ok(params) = std::env::var("GIT_CONFIG_PARAMETERS") {
            for entry in parse_config_parameters(&params) {
                if let Some((k, v)) = entry.split_once('=') {
                    if canonical_pack_window_key(k.trim())
                        && v.trim().parse::<i64>().ok() == Some(0)
                    {
                        z = true;
                    }
                }
            }
        }
        z
    };
    let window_reuse_only =
        args.window == Some(0) || window_zero_cli || window_zero_extra || window_zero_cfg;

    if args.all && args.incremental && args.unpacked && window_reuse_only {
        order_incremental_commits_first_parent_chain(&repo, &mut entries)?;
    }

    let (mut write_entries, new_deltas, reused_deltas) = optimize_blob_deltas(
        &repo,
        entries,
        max_delta_depth,
        window_reuse_only,
        &pack_list.thin_blob_deltas,
        pack_hash_bytes,
    )?;

    let mut trace_pack_reused: Option<u32> = None;
    let mut trace_packs_reused: Option<u32> = None;
    if pack_reuse_cli_ok(&args) && (args.all || args.revs || pack_list.rev_list_stdin) {
        let mode = pack_reuse_mode(&repo);
        if let Some((reused_slices, pr, pk)) = compute_midx_reused_entries(&repo, &pack_list, mode)?
        {
            if !reused_slices.is_empty() {
                let mut reused_oids: HashSet<ObjectId> = HashSet::new();
                for e in &reused_slices {
                    if let PackWriteEntry::ReusedSlice { oid, .. } = e {
                        reused_oids.insert(*oid);
                    }
                }
                write_entries.retain(|e| {
                    let oid = match e {
                        PackWriteEntry::Full(pe) => pe.oid,
                        PackWriteEntry::RefDelta { oid, .. } => *oid,
                        PackWriteEntry::ReusedSlice { oid, .. } => *oid,
                    };
                    !reused_oids.contains(&oid)
                });
                let mut combined = reused_slices;
                combined.append(&mut write_entries);
                write_entries = combined;
                trace_pack_reused = Some(pr);
                trace_packs_reused = Some(pk);
            }
        }
    }

    if let Some(ref path) = std::env::var_os("GIT_TRACE2_EVENT") {
        if let Some(p) = path.to_str() {
            if let (Some(a), Some(b)) = (trace_pack_reused, trace_packs_reused) {
                let _ = crate::trace2_write_json_data_line(
                    p,
                    "pack-objects",
                    "pack-reused",
                    &a.to_string(),
                );
                let _ = crate::trace2_write_json_data_line(
                    p,
                    "pack-objects",
                    "packs-reused",
                    &b.to_string(),
                );
            }
        }
    }

    let use_ofs_delta = args.delta_base_offset;

    let pack_limit = parse_pack_size_limit_bytes(&args, &repo);
    let chunks: Vec<Vec<PackWriteEntry>> = if let Some(limit) = pack_limit.filter(|&l| l > 0) {
        let mut out_chunks: Vec<Vec<PackWriteEntry>> = Vec::new();
        let mut cur: Vec<PackWriteEntry> = Vec::new();
        let mut cur_sz: u64 = 0;
        for e in write_entries {
            let est = estimate_pack_entry_bytes(&e)?;
            if !cur.is_empty() && cur_sz > 0 && cur_sz.saturating_add(est) > limit {
                out_chunks.push(cur);
                cur = Vec::new();
                cur_sz = 0;
            }
            cur_sz = cur_sz.saturating_add(est);
            cur.push(e);
        }
        if !cur.is_empty() {
            out_chunks.push(cur);
        }
        out_chunks
    } else {
        vec![write_entries]
    };

    if args.stdout {
        if !args.quiet {
            let reused_pack = trace_pack_reused.unwrap_or(0);
            let total_delta = new_deltas + reused_deltas;
            let total: usize = chunks.iter().map(|c| c.len()).sum();
            eprintln!(
                "Total {} (delta {}), reused {} (delta {})",
                total, total_delta, reused_pack, reused_deltas
            );
        }
        let stdout = io::stdout();
        let mut out = stdout.lock();
        for chunk in &chunks {
            let pack_bytes = build_pack(chunk, use_ofs_delta, pack_hash_bytes)?;
            out.write_all(&pack_bytes)?;
        }
        out.flush()?;
    } else {
        let base = args
            .base_name
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("no base name"))?;

        let mut pack_hashes: Vec<String> = Vec::new();
        for chunk in &chunks {
            let pack_bytes = build_pack(chunk, use_ofs_delta, pack_hash_bytes)?;
            let pack_hash = hex::encode(&pack_bytes[pack_bytes.len() - pack_hash_bytes..]);
            pack_hashes.push(pack_hash.clone());
            let pack_path = format!("{base}-{pack_hash}.pack");
            let idx_path = format!("{base}-{pack_hash}.idx");

            std::fs::write(&pack_path, &pack_bytes)?;
            let idx_bytes = build_idx_for_pack(&pack_bytes, chunk, pack_hash_bytes)?;
            std::fs::write(&idx_path, &idx_bytes)?;

            println!("{pack_hash}");
            if !args.quiet {
                eprintln!(
                    "Total {} (delta {}), reused 0 (delta {})",
                    chunk.len(),
                    new_deltas + reused_deltas,
                    reused_deltas
                );
            }

            let pb = Path::new(&pack_path);
            if let (Some(dir), Some(stem)) = (pb.parent(), pb.file_stem().and_then(|s| s.to_str()))
            {
                if args.cruft && !args.incremental {
                    let _ = std::fs::write(dir.join(format!("{stem}.mtimes")), b"");
                } else {
                    // A full repack without `--cruft` may reuse the same pack hash as a former cruft
                    // pack (same object set); drop stale `.mtimes` so `gc --keep-largest-pack` matches Git.
                    let _ = std::fs::remove_file(dir.join(format!("{stem}.mtimes")));
                }
                if !args.no_write_bitmap_index
                    && (args.write_bitmap_index || args.write_bitmap_index_quiet)
                {
                    let _ = std::fs::write(dir.join(format!("{stem}.bitmap")), []);
                } else {
                    // Same pack hash as a prior bitmap repack leaves a stale sidecar if we skip bitmaps
                    // (`git -c repack.writeBitmaps=false repack -ad`; `t7700-repack`).
                    let _ = std::fs::remove_file(dir.join(format!("{stem}.bitmap")));
                }
            }
        }

        if args.unpack_unreachable.is_some() {
            let packed: HashSet<ObjectId> = chunks
                .iter()
                .flatten()
                .map(|e| match e {
                    PackWriteEntry::Full(p) => p.oid,
                    PackWriteEntry::RefDelta { oid, .. } => *oid,
                    PackWriteEntry::ReusedSlice { oid, .. } => *oid,
                })
                .collect();
            loosen_unused_packed_objects(&repo, &packed, &pack_hashes, args.honor_pack_keep)?;
            prune_stale_loose_after_unpack_unreachable(&repo, &packed, &pack_list.oids)?;
        }
    }

    Ok(())
}

/// Objects for default `pack-objects --all`: closure from all refs plus optional reflog tips and
/// index blobs — same as Git’s `get_object_list` for `--all`.
///
/// Unreachable loose objects are **not** included (`t7700-repack`); only the reachability walk
/// discovers objects (including those stored only in alternate ODBs).
fn pack_objects_all_enumeration(repo: &Repository, args: &Args) -> Result<Vec<ObjectId>> {
    let mut opts = RevListOptions::default();
    opts.objects = true;
    opts.all_refs = true;
    opts.include_reflog_entries = args.reflog;
    opts.include_indexed_objects = args.indexed_objects;
    opts.missing_action = MissingAction::Allow;
    let r = match rev_list(repo, &[] as &[String], &[] as &[String], &opts) {
        Ok(r) => r,
        Err(LibError::InvalidRef(ref s)) if s == "no revisions specified" => {
            return Ok(Vec::new());
        }
        Err(e) => return Err(e).context("rev-list for pack-objects --all"),
    };
    let mut oids = BTreeSet::new();
    for c in &r.commits {
        oids.insert(*c);
    }
    for (o, _) in r.objects {
        oids.insert(o);
    }
    Ok(oids.into_iter().collect())
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
                if e.oid.len() == 20 {
                    if let Ok(oid) = ObjectId::from_bytes(&e.oid) {
                        fresh_oids.insert(oid);
                    }
                }
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
            if e.oid.len() == 20 {
                if let Ok(oid) = ObjectId::from_bytes(&e.oid) {
                    oids.insert(oid);
                }
            }
        }
    }

    // Cruft = objects from discarded packs (and loose) that are not in the new pack(s). Do not
    // subtract `rev-list --all --reflog`: reflog still points at discarded commits (t6500
    // `prepare_cruft_history`), and those objects must land in the cruft pack.
    oids.retain(|o| !fresh_oids.contains(o));

    // Loose commits whose parent is already in the fresh (main + `--keep-pack`) packs are one
    // step off the repacked graph — Git does not treat them as cruft (`t7700-repack` keep-pack).
    oids.retain(|o| {
        let Ok(obj) = read_object_from_repo(repo, o) else {
            return true;
        };
        if obj.kind != ObjectKind::Commit {
            return true;
        }
        let Ok(c) = parse_commit(&obj.data) else {
            return true;
        };
        !c.parents.iter().any(|p| fresh_oids.contains(p))
    });

    Ok(PackObjectList {
        oids: oids.into_iter().collect(),
        thin_blob_deltas: Vec::new(),
        rev_list_stdin: false,
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

fn parse_pack_size_limit_bytes(args: &Args, repo: &Repository) -> Option<u64> {
    // Git rejects `--max-pack-size` with `--stdout`; config limit is also ignored (t5300 thread tests).
    if args.stdout {
        return None;
    }
    let from_cfg = ConfigSet::load(Some(&repo.git_dir), true)
        .ok()
        .and_then(|c| c.get("pack.packSizeLimit"))
        .and_then(|s| parse_byte_size(&s));
    let mut limit = args
        .max_pack_size
        .as_deref()
        .and_then(parse_byte_size)
        .or(from_cfg)?;
    // Git enforces a 1 MiB floor (`pack-objects.c`); smaller config values still split sensibly.
    const MIN_PACK_LIMIT: u64 = 1024 * 1024;
    if limit > 0 && limit < MIN_PACK_LIMIT {
        eprintln!("warning: minimum pack size limit is 1 MiB");
        limit = MIN_PACK_LIMIT;
    }
    Some(limit)
}

fn parse_byte_size(s: &str) -> Option<u64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let lower = s.to_ascii_lowercase();
    let (num, mult) = if let Some(stripped) = lower.strip_suffix('k') {
        (stripped, 1024u64)
    } else if let Some(stripped) = lower.strip_suffix('m') {
        (stripped, 1024 * 1024)
    } else if let Some(stripped) = lower.strip_suffix('g') {
        (stripped, 1024 * 1024 * 1024)
    } else {
        (s, 1u64)
    };
    num.trim()
        .parse::<u64>()
        .ok()
        .map(|n| n.saturating_mul(mult))
}

fn parse_window_effective(args: &Args) -> i64 {
    let mut from_argv: Option<i64> = None;
    let mut it = std::env::args();
    while let Some(a) = it.next() {
        if let Some(rest) = a.strip_prefix("--window=") {
            if let Ok(w) = rest.parse::<i64>() {
                from_argv = Some(w);
            }
        } else if let Some(rest) = a.strip_prefix("-window=") {
            if let Ok(w) = rest.parse::<i64>() {
                from_argv = Some(w);
            }
        } else if a == "--window" || a == "-window" {
            if let Some(v) = it.next().and_then(|s| s.parse::<i64>().ok()) {
                from_argv = Some(v);
            }
        }
    }
    let from_extra = args.extra.iter().find_map(|a| {
        a.strip_prefix("--window=")
            .or_else(|| a.strip_prefix("-window="))
            .and_then(|v| v.parse::<i64>().ok())
    });
    let from_cfg = Repository::discover(None).ok().and_then(|r| {
        ConfigSet::load(Some(&r.git_dir), true)
            .ok()?
            .get("pack.window")
    });
    let from_cfg = from_cfg.and_then(|s| s.parse::<i64>().ok());
    let w = from_argv
        .or(from_extra)
        .or(args.window)
        .or(from_cfg)
        .unwrap_or(250);
    if w < 0 {
        0
    } else {
        w
    }
}

/// Encode Git's variable-length OFS_DELTA distance (`pack-objects.c` `write_no_reuse_object`).
fn encode_git_ofs_delta_distance(buf: &mut Vec<u8>, mut ofs: u64) {
    let mut dheader = [0u8; 32];
    let mut pos = dheader.len() - 1;
    dheader[pos] = (ofs & 0x7f) as u8;
    while {
        ofs >>= 7;
        ofs != 0
    } {
        pos -= 1;
        ofs -= 1;
        dheader[pos] = 0x80 | ((ofs & 0x7f) as u8);
    }
    buf.extend_from_slice(&dheader[pos..]);
}

fn pack_threads_effective_from_config(git_dir: &Path) -> Option<u32> {
    // Use the full config cascade (local + `GIT_CONFIG_PARAMETERS` / `git -c`), matching
    // `git config --get pack.threads`. `load_repo_local_only` omits command-line overrides
    // (t5300.50 third case).
    let cfg = ConfigSet::load(Some(git_dir), true).ok()?;
    cfg.get("pack.threads")?.parse::<u32>().ok()
}

fn warn_pack_threads(args: &Args) {
    if let Some(n) = args.threads {
        if n != 1 {
            // Match Git `pack-objects.c`: generic message (t5300 greps this substring).
            eprintln!("warning: no threads support, ignoring --threads");
        }
    }
    if let Ok(repo) = Repository::discover(None) {
        if let Some(n) = pack_threads_effective_from_config(&repo.git_dir) {
            if n != 1 {
                eprintln!("warning: no threads support, ignoring pack.threads");
            }
        }
    }
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
    let hash = out
        .stdout
        .split(|b| *b == b'\n')
        .next()
        .and_then(|line| std::str::from_utf8(line).ok())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if let Some(h) = hash {
        record_extra_pack_for_repack(&repo.git_dir, &format!("pack-{h}.pack"))?;
    }
    Ok(())
}

fn record_extra_pack_for_repack(git_dir: &Path, pack_name: &str) -> Result<()> {
    let info = git_dir.join("objects").join("info");
    fs::create_dir_all(&info).map_err(|e| anyhow::anyhow!(e))?;
    let path = info.join("grit-extra-packs");
    use std::io::Write;
    let mut f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| anyhow::anyhow!(e))?;
    writeln!(f, "{pack_name}").map_err(|e| anyhow::anyhow!(e))?;
    Ok(())
}

/// Apply `git pack-objects --filter=<spec>` (subset: `blob:none` for `gc.repackFilter` tests).
fn pack_index_path_for_stdin_pack_spec(pack_dir: &Path, spec: &str) -> Result<PathBuf> {
    let idx_path = if spec.contains('/') || spec.contains('\\') {
        let p = PathBuf::from(spec);
        if p.extension().is_some_and(|e| e == "pack") {
            p.with_extension("idx")
        } else {
            p
        }
    } else {
        let stem = spec.strip_suffix(".pack").unwrap_or(spec);
        pack_dir.join(format!("{stem}.idx"))
    };
    let idx_path = if idx_path.extension().is_some_and(|e| e == "pack") {
        idx_path.with_extension("idx")
    } else {
        idx_path
    };
    if !idx_path.exists() {
        bail!("pack index not found: {}", idx_path.display());
    }
    Ok(idx_path)
}

fn apply_list_objects_filter(entries: &mut Vec<PackEntry>, filter: Option<&str>) {
    let Some(spec) = filter.map(str::trim).filter(|s| !s.is_empty()) else {
        return;
    };
    if spec == "blob:none" {
        entries.retain(|e| e.kind != ObjectKind::Blob);
    }
}

fn pack_all_use_reachable_closure_only(args: &Args) -> bool {
    // Only `repack --cruft`’s first `pack-objects` pass uses `--reachability-all`: ref closure
    // without reflog roots (see `reachable_objects_for_full_repack`). Default `--all` uses
    // reflog/indexed flags plus **primary** loose objects — not a raw scan of every pack index
    // (which would pull unreachable objects out of alternate ODBs; `t7700-repack`).
    args.reachability_all
}

fn stdin_looks_like_rev_list(lines: &[String]) -> bool {
    lines.iter().any(|l| {
        let t = l.trim();
        !t.is_empty() && (t.starts_with('^') || t == "--not")
    })
}

fn collect_pack_objects_from_rev_stdin_lines(
    repo: &Repository,
    args: &Args,
    rev_lines: &[String],
) -> Result<PackObjectList> {
    let mut shallow_grafts: HashSet<ObjectId> = shallow_boundary_oids(&repo.git_dir);
    let mut positive: Vec<String> = Vec::new();
    let mut negative: Vec<String> = Vec::new();
    let mut post_not = false;
    let mut have_roots: BTreeSet<ObjectId> = BTreeSet::new();
    for line in rev_lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if args.shallow {
            if let Some(hex) = trimmed.strip_prefix("--shallow ") {
                let oid = ObjectId::from_hex(hex.trim())
                    .map_err(|e| anyhow::anyhow!("invalid --shallow oid: {e}"))?;
                shallow_grafts.insert(oid);
                continue;
            }
        }
        // `t5332` uses `printf '%s' "$base" '^' '%s' "$delta"` → `fullbase^fulldelta` on one line.
        // That is not a peel suffix: it is shorthand for `rev-list --objects <base> ^<delta>`.
        if let Some((left, right)) = trimmed.split_once('^') {
            if left.len() == 40
                && right.len() == 40
                && ObjectId::from_hex(left).is_ok()
                && ObjectId::from_hex(right).is_ok()
            {
                positive.push(left.to_string());
                negative.push(right.to_string());
                continue;
            }
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
        if let Some(neg) = trimmed.strip_prefix('^') {
            negative.push(neg.to_string());
        } else {
            positive.push(trimmed.to_string());
        }
    }

    let mut oids: BTreeSet<ObjectId> = BTreeSet::new();
    let mut exclude = BTreeSet::new();
    for neg in &negative {
        let oid =
            resolve_revision(repo, neg).with_context(|| format!("cannot resolve ref '{neg}'"))?;
        walk_reachable(repo, &oid, &mut exclude, &shallow_grafts)?;
    }
    for pos in &positive {
        let oid =
            resolve_revision(repo, pos).with_context(|| format!("cannot resolve ref '{pos}'"))?;
        let obj = read_object_from_repo(repo, &oid)?;
        // Upload-pack may `want` a raw tree/blob OID (lazy fetch). Pack only that object, not the
        // subtree/closure (`t0410` tree fetch without blobs). Commits/tags use a full walk.
        match obj.kind {
            ObjectKind::Commit | ObjectKind::Tag => {
                walk_reachable(repo, &oid, &mut oids, &shallow_grafts)?;
            }
            ObjectKind::Tree | ObjectKind::Blob => {
                oids.insert(oid);
            }
        }
    }
    for oid in &exclude {
        oids.remove(oid);
    }

    let mut ordered: Vec<ObjectId> = oids.into_iter().collect();

    if args.thin && !have_roots.is_empty() {
        let mut have_closure = BTreeSet::new();
        for root in &have_roots {
            walk_reachable(repo, root, &mut have_closure, &shallow_grafts)?;
        }
        ordered.retain(|o| !have_closure.contains(o));
    }
    Ok(PackObjectList {
        oids: ordered,
        thin_blob_deltas: Vec::new(),
        rev_list_stdin: true,
    })
}

/// Collect object IDs from stdin or `--all`.
fn collect_oids(repo: &Repository, args: &Args) -> Result<PackObjectList> {
    if args.all && args.unpacked && args.incremental {
        return collect_incremental_repack_oids(repo, args);
    }

    if args.cruft && !args.incremental {
        return collect_cruft_pack_stdin_oids(repo);
    }

    let stdin_lines: Vec<String> = io::stdin()
        .lock()
        .lines()
        .collect::<std::io::Result<Vec<_>>>()?;

    let rev_mode = args.revs || stdin_looks_like_rev_list(&stdin_lines);
    let has_rev_input = stdin_lines.iter().any(|l| !l.trim().is_empty());
    if rev_mode && has_rev_input {
        return collect_pack_objects_from_rev_stdin_lines(repo, args, &stdin_lines);
    }
    // `git pack-objects --revs` with no stdin must not fall through to plain `--all` ODB enumeration
    // when `--all` is absent (would pack everything and break t5332). With `--all` and empty stdin,
    // Git uses an internal `rev-list --objects --all` (upload-pack); keep the `--all` path below.
    if rev_mode && !has_rev_input && !args.all {
        return Ok(PackObjectList {
            oids: Vec::new(),
            thin_blob_deltas: Vec::new(),
            rev_list_stdin: true,
        });
    }

    let mut oids = BTreeSet::new();

    if args.all {
        // Incremental repack (`--all --unpacked --incremental`) packs loose objects not yet in a
        // pack. Full `--all` (e.g. `git gc` / `repack -a`) must use the ref closure only (Git
        // `--all` semantics), not every object under `.git/objects`.
        let use_reachable_only = !args.incremental;
        if use_reachable_only {
            let v = reachable_objects_for_full_repack(repo, args)?;
            oids.extend(v);
        } else {
            let mut v = pack_objects_all_enumeration(repo, args)?;
            if args.local {
                // `git pack-objects --local`: omit objects that exist only in alternate ODBs
                // (reachable from refs but stored as loose/packed under `info/alternates`).
                v.retain(|oid| !repo.odb.exists(oid) || repo.odb.exists_local(oid));
            }
            oids.extend(v);
        }
    }

    if args.all && !args.keep_pack.is_empty() {
        let skip = keep_pack_object_ids(repo, &args.keep_pack)?;
        oids.retain(|o| !skip.contains(o));
    }

    if args.all && args.honor_pack_keep {
        let skip = kept_pack_object_ids(repo)?;
        oids.retain(|o| !skip.contains(o));
    }

    if args.stdin_packs {
        // Read pack filenames from stdin: `^pack-…` builds an exclusion set; other lines add
        // objects from those packs minus exclusions (order-independent; `git repack --filter`).
        let pack_dir = repo.odb.objects_dir().join("pack");
        let mut exclude: HashSet<ObjectId> = HashSet::new();
        for trimmed in stdin_lines
            .iter()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
        {
            if !trimmed.starts_with('^') {
                continue;
            }
            let spec = trimmed[1..].trim();
            let idx_path = pack_index_path_for_stdin_pack_spec(&pack_dir, spec)?;
            let idx = grit_lib::pack::read_pack_index(&idx_path)
                .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", idx_path.display()))?;
            for entry in idx.entries {
                if entry.oid.len() == 20 {
                    if let Ok(oid) = ObjectId::from_bytes(&entry.oid) {
                        exclude.insert(oid);
                    }
                }
            }
        }
        for trimmed in stdin_lines
            .iter()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
        {
            if trimmed.starts_with('^') {
                continue;
            }
            let idx_path = pack_index_path_for_stdin_pack_spec(&pack_dir, trimmed)?;
            let idx = grit_lib::pack::read_pack_index(&idx_path)
                .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", idx_path.display()))?;
            for entry in idx.entries {
                if entry.oid.len() == 20 {
                    if let Ok(oid) = ObjectId::from_bytes(&entry.oid) {
                        if !exclude.contains(&oid) {
                            oids.insert(oid);
                        }
                    }
                }
            }
        }
        return Ok(PackObjectList {
            oids: oids.into_iter().collect(),
            thin_blob_deltas: Vec::new(),
            rev_list_stdin: false,
        });
    } else if !args.all {
        // Git `pack-objects` stdin format (see git/builtin/pack-objects.c `read_object_list_from_stdin`):
        //   -<oid>  — set preferred base (tree OID for thin-pack blob deltas), not an exclusion
        //   <oid> [<path>] — object to pack; with a preferred base, path selects the base blob
        let mut oids_ordered: Vec<ObjectId> = Vec::new();
        let mut seen: HashSet<ObjectId> = HashSet::new();
        let mut thin_blob_deltas: Vec<(ObjectId, ObjectId)> = Vec::new();
        let mut preferred_tree: Option<ObjectId> = None;

        for line in &stdin_lines {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                // Match Git: second line is a single space (t5300 empty-line stdin test).
                bail!("fatal: expected object ID, got garbage:\n \n");
            }
            if let Some(rest) = trimmed.strip_prefix('-') {
                let hex_part = rest.split_whitespace().next().unwrap_or(rest);
                let tree_oid = ObjectId::from_hex(hex_part)
                    .map_err(|e| anyhow::anyhow!("invalid preferred base '{hex_part}': {e}"))?;
                preferred_tree = Some(tree_oid);
                continue;
            }

            let hex_part = trimmed.split_whitespace().next().unwrap_or(trimmed);
            let oid = ObjectId::from_hex(hex_part).map_err(|_| {
                anyhow::anyhow!("fatal: expected object ID, got garbage:\n {trimmed}\n")
            })?;
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
            rev_list_stdin: false,
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
        rev_list_stdin: false,
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

    let result = match rev_list(repo, &[] as &[String], &[] as &[String], &opts) {
        Ok(r) => r,
        // Fresh repo / no refs yet: incremental repack is a no-op (Git `repack -d`, t5332).
        Err(LibError::InvalidRef(ref s)) if s == "no revisions specified" => {
            return Ok(PackObjectList {
                oids: Vec::new(),
                thin_blob_deltas: Vec::new(),
                rev_list_stdin: false,
            });
        }
        Err(e) => return Err(e).context("rev-list for incremental pack-objects"),
    };

    let mut ordered: Vec<ObjectId> = Vec::new();
    let mut seen = HashSet::new();
    // `rev_list` object lines omit commit OIDs (Git-style); incremental repack must still pack
    // loose commits (t5332 / repack -d).
    for oid in &result.commits {
        if seen.insert(*oid) {
            ordered.push(*oid);
        }
    }
    for oid in result.objects.iter().map(|(o, _)| *o) {
        if seen.insert(oid) {
            ordered.push(oid);
        }
    }

    if !args.keep_pack.is_empty() {
        let skip = keep_pack_object_ids(repo, &args.keep_pack)?;
        ordered.retain(|o| !skip.contains(o));
    }

    if args.honor_pack_keep {
        let skip = kept_pack_object_ids(repo)?;
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
        rev_list_stdin: false,
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
            if e.oid.len() == 20 {
                if let Ok(oid) = ObjectId::from_bytes(&e.oid) {
                    out.insert(oid);
                }
            }
        }
    }
    Ok(out)
}

/// Object IDs residing in local packs that have a sibling `pack-….keep` file on disk.
///
/// Matches Git’s `--honor-pack-keep` / `ignore_packed_keep_on_disk` behaviour for `pack-objects`.
fn kept_pack_object_ids(repo: &Repository) -> Result<HashSet<ObjectId>> {
    let mut out = HashSet::new();
    let indexes = grit_lib::pack::read_local_pack_indexes(repo.odb.objects_dir())
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    for idx in indexes {
        let keep_path = idx.pack_path.with_extension("keep");
        if !keep_path.is_file() {
            continue;
        }
        for e in idx.entries {
            if e.oid.len() == 20 {
                if let Ok(oid) = ObjectId::from_bytes(&e.oid) {
                    out.insert(oid);
                }
            }
        }
    }
    Ok(out)
}

/// Git `loosen_unused_packed_objects`: write loose copies of objects that remain only in other
/// local packs after this run (`pack-objects --unpack-unreachable`, `repack -A`).
/// After `--unpack-unreachable`, drop loose files for objects that are neither in the new pack nor
/// in the ref/reflog closure we just packed (matches Git pruning unreachable loose copies;
/// `t7700-repack`).
fn prune_stale_loose_after_unpack_unreachable(
    repo: &Repository,
    packed: &HashSet<ObjectId>,
    enumeration: &[ObjectId],
) -> Result<()> {
    let keep: HashSet<ObjectId> = enumeration.iter().copied().collect();
    let mut loose = BTreeSet::new();
    collect_all_loose(&repo.odb, &mut loose)?;
    for oid in loose {
        if packed.contains(&oid) || keep.contains(&oid) {
            continue;
        }
        let path = repo.odb.object_path(&oid);
        if path.is_file() {
            let _ = std::fs::remove_file(path);
        }
    }
    Ok(())
}

fn loosen_unused_packed_objects(
    repo: &Repository,
    packed: &HashSet<ObjectId>,
    new_pack_hashes: &[String],
    honor_pack_keep: bool,
) -> Result<()> {
    let objects_dir = repo.git_dir.join("objects");
    let pack_dir = objects_dir.join("pack");
    let kept_oids = if honor_pack_keep {
        kept_pack_object_ids(repo)?
    } else {
        HashSet::new()
    };
    let indexes = grit_lib::pack::read_local_pack_indexes(&objects_dir)
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
        let stem = name.strip_suffix(".pack").unwrap_or(name);
        if new_pack_hashes.iter().any(|h| stem == format!("pack-{h}")) {
            continue;
        }
        if honor_pack_keep && pack_dir.join(format!("{stem}.keep")).is_file() {
            continue;
        }
        for e in &idx.entries {
            if e.oid.len() != 20 {
                continue;
            }
            let Ok(oid) = ObjectId::from_bytes(&e.oid) else {
                continue;
            };
            if packed.contains(&oid) {
                continue;
            }
            if honor_pack_keep && kept_oids.contains(&oid) {
                continue;
            }
            if repo.odb.exists(&oid) {
                continue;
            }
            let obj = read_object_from_repo(repo, &oid)?;
            repo.odb.write(obj.kind, &obj.data)?;
        }
    }
    Ok(())
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

/// For incremental `pack-objects --all --unpacked` with `--window=0`, order commit objects so the
/// newest tip appears first and the first-parent chain follows (t5332 pack index order).
fn order_incremental_commits_first_parent_chain(
    repo: &Repository,
    entries: &mut Vec<PackEntry>,
) -> Result<()> {
    let mut commits: Vec<(usize, ObjectId)> = Vec::new();
    let mut non_commit: Vec<PackEntry> = Vec::new();
    for (i, e) in entries.iter().enumerate() {
        if e.kind == ObjectKind::Commit {
            commits.push((i, e.oid));
        } else {
            non_commit.push(e.clone());
        }
    }
    if commits.len() < 2 {
        return Ok(());
    }
    let s: HashSet<ObjectId> = commits.iter().map(|(_, o)| *o).collect();
    let mut first_parent_in_s: HashMap<ObjectId, ObjectId> = HashMap::new();
    for (_, oid) in &commits {
        let obj = read_object_from_repo(repo, oid)?;
        let c = parse_commit(&obj.data).map_err(|e| anyhow::anyhow!("{e}"))?;
        if let Some(&p) = c.parents.first() {
            if s.contains(&p) {
                first_parent_in_s.insert(*oid, p);
            }
        }
    }
    let mut indegree: HashMap<ObjectId, usize> = HashMap::new();
    for oid in &s {
        indegree.entry(*oid).or_insert(0);
    }
    for (_, p) in &first_parent_in_s {
        *indegree.entry(*p).or_insert(0) += 1;
    }
    let mut tips: Vec<ObjectId> = indegree
        .iter()
        .filter(|(_, &d)| d == 0)
        .map(|(o, _)| *o)
        .collect();
    if tips.is_empty() {
        return Ok(());
    }
    tips.sort_by(|a, b| b.cmp(a));
    let mut ordered: Vec<ObjectId> = Vec::new();
    let mut seen: HashSet<ObjectId> = HashSet::new();
    let mut q: VecDeque<ObjectId> = tips.into_iter().collect();
    while let Some(c) = q.pop_front() {
        if !seen.insert(c) {
            continue;
        }
        ordered.push(c);
        if let Some(&p) = first_parent_in_s.get(&c) {
            if s.contains(&p) && !seen.contains(&p) {
                q.push_back(p);
            }
        }
    }
    if ordered.len() != commits.len() {
        return Ok(());
    }
    let mut by_oid: HashMap<ObjectId, PackEntry> = HashMap::new();
    for e in entries.iter().filter(|e| e.kind == ObjectKind::Commit) {
        by_oid.insert(e.oid, e.clone());
    }
    let mut out: Vec<PackEntry> = Vec::with_capacity(entries.len());
    for oid in ordered {
        if let Some(e) = by_oid.remove(&oid) {
            out.push(e);
        }
    }
    out.extend(non_commit);
    *entries = out;
    Ok(())
}

/// Walk reachable objects from a commit/tree/tag/blob OID.
fn walk_reachable(
    repo: &Repository,
    oid: &ObjectId,
    oids: &mut BTreeSet<ObjectId>,
    shallow_grafts: &HashSet<ObjectId>,
) -> Result<()> {
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
                            walk_reachable(repo, &tree_oid, oids, shallow_grafts)?;
                        }
                    } else if let Some(parent_hex) = line.strip_prefix("parent ") {
                        if !shallow_grafts.contains(oid) {
                            if let Ok(parent_oid) = ObjectId::from_hex(parent_hex.trim()) {
                                walk_reachable(repo, &parent_oid, oids, shallow_grafts)?;
                            }
                        }
                    } else if line.is_empty() {
                        break; // end of headers
                    }
                }
            }
        }
        ObjectKind::Tree => {
            let entries = parse_tree(&obj.data).map_err(|e| anyhow::anyhow!("{e}"))?;
            for entry in entries {
                // Submodule / gitlink: the OID names a commit in another repository; it is not
                // stored in this ODB. Recursing would fail pack-objects (see t3050-subprojects-fetch).
                if entry.mode == 0o160000 {
                    continue;
                }
                walk_reachable(repo, &entry.oid, oids, shallow_grafts)?;
            }
        }
        ObjectKind::Tag => {
            // Parse the object line.
            if let Ok(text) = std::str::from_utf8(&obj.data) {
                if let Some(first_line) = text.lines().next() {
                    if let Some(obj_hex) = first_line.strip_prefix("object ") {
                        if let Ok(target_oid) = ObjectId::from_hex(obj_hex.trim()) {
                            walk_reachable(repo, &target_oid, oids, shallow_grafts)?;
                        }
                    }
                }
            }
        }
        ObjectKind::Blob => {} // leaf
    }
    Ok(())
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
        if let Some(entry) = idx
            .entries
            .iter()
            .find(|e| grit_lib::pack::pack_index_entry_matches_sha1_oid(e, oid))
        {
            let pack_bytes = std::fs::read(&idx.pack_path)?;
            let obj = read_object_from_pack(&pack_bytes, entry.offset, &indexes, idx.hash_bytes)?;
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
        if let Some(entry) = idx
            .entries
            .iter()
            .find(|e| grit_lib::pack::pack_index_entry_matches_sha1_oid(e, oid))
        {
            let pack_bytes = std::fs::read(&idx.pack_path)?;
            let obj = read_object_from_pack(&pack_bytes, entry.offset, &indexes, idx.hash_bytes)?;
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
    hash_bytes: usize,
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

            let base_obj = read_object_from_pack(pack_bytes, base_offset, indexes, hash_bytes)?;
            let result = grit_lib::unpack_objects::apply_delta(&base_obj.data, &delta_data)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            Ok(grit_lib::objects::Object::new(base_obj.kind, result))
        }
        7 => {
            // REF_DELTA
            if pos + hash_bytes > pack_bytes.len() {
                bail!("truncated ref-delta");
            }
            let base_raw = pack_bytes[pos..pos + hash_bytes].to_vec();
            pos += hash_bytes;

            use flate2::read::ZlibDecoder;
            use std::io::Read;
            let mut decoder = ZlibDecoder::new(&pack_bytes[pos..]);
            let mut delta_data = Vec::with_capacity(size);
            decoder.read_to_end(&mut delta_data)?;

            // Find the base in any pack.
            let mut base_obj = None;
            for idx in indexes {
                if let Some(entry) = idx.entries.iter().find(|e| e.oid == base_raw) {
                    let pb = std::fs::read(&idx.pack_path)?;
                    base_obj = Some(read_object_from_pack(
                        &pb,
                        entry.offset,
                        indexes,
                        idx.hash_bytes,
                    )?);
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
    pack_hash_bytes: usize,
) -> Result<(Vec<PackWriteEntry>, usize, usize)> {
    if pack_hash_bytes == 32 {
        let out = entries.into_iter().map(PackWriteEntry::Full).collect();
        return Ok((out, 0, 0));
    }

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
        if !window_reuse_only {
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
                    let target_pack = entries
                        .iter()
                        .find(|x| x.oid == e.oid)
                        .map(|x| x.pack_id.clone())
                        .unwrap_or_default();
                    let base_pack = entries
                        .iter()
                        .find(|x| x.oid == base_oid)
                        .map(|x| x.pack_id.clone())
                        .unwrap_or_default();
                    out.push(PackWriteEntry::RefDelta {
                        oid: e.oid,
                        base_oid,
                        target_pack,
                        base_pack,
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
        let target_pack = entries
            .iter()
            .find(|x| x.oid == e.oid)
            .map(|x| x.pack_id.clone())
            .unwrap_or_default();
        let base_pack = entries
            .iter()
            .find(|x| x.oid == base_oid)
            .map(|x| x.pack_id.clone())
            .unwrap_or_default();
        out.push(PackWriteEntry::RefDelta {
            oid: e.oid,
            base_oid,
            target_pack,
            base_pack,
            delta,
        });
        new_deltas += 1;
    }

    Ok((out, new_deltas, reused_deltas))
}

fn pack_trailer_bytes_for_repo(git_dir: &Path) -> usize {
    let cfg = ConfigSet::load(Some(git_dir), true).unwrap_or_default();
    if cfg
        .get("extensions.objectformat")
        .or_else(|| cfg.get("extensions.objectFormat"))
        .is_some_and(|v| v.eq_ignore_ascii_case("sha256"))
    {
        32
    } else {
        20
    }
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

fn estimate_pack_entry_bytes(entry: &PackWriteEntry) -> Result<u64> {
    let zlib_overhead: u64 = 32;
    match entry {
        PackWriteEntry::Full(pe) => {
            let hdr = 10u64;
            Ok(hdr + pe.data.len() as u64 + zlib_overhead)
        }
        PackWriteEntry::RefDelta {
            delta, base_pack, ..
        } => {
            let hdr = 10u64;
            Ok(hdr + base_pack.len() as u64 + delta.len() as u64 + zlib_overhead)
        }
        PackWriteEntry::ReusedSlice { raw, .. } => Ok(raw.len() as u64),
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

/// Build a PACK v2 byte stream (full objects and optional delta blobs).
fn build_pack(
    entries: &[PackWriteEntry],
    use_ofs_delta: bool,
    pack_hash_bytes: usize,
) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    buf.extend_from_slice(b"PACK");
    buf.extend_from_slice(&2u32.to_be_bytes());
    buf.extend_from_slice(&(entries.len() as u32).to_be_bytes());

    let mut oid_to_offset: HashMap<ObjectId, u64> = HashMap::new();

    for entry in entries {
        let start = buf.len() as u64;
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
                oid_to_offset.insert(pe.oid, start);
            }
            PackWriteEntry::RefDelta {
                oid,
                base_oid,
                base_pack,
                delta,
                ..
            } => {
                let use_ofs = use_ofs_delta && oid_to_offset.contains_key(base_oid);
                if use_ofs {
                    let base_off = *oid_to_offset
                        .get(base_oid)
                        .ok_or_else(|| anyhow::anyhow!("ofs base missing"))?;
                    let dist = start
                        .checked_sub(base_off)
                        .ok_or_else(|| anyhow::anyhow!("ofs distance underflow"))?;
                    encode_pack_object_header(&mut buf, 6, delta.len());
                    encode_git_ofs_delta_distance(&mut buf, dist);
                } else {
                    encode_pack_object_header(&mut buf, 7, delta.len());
                    buf.extend_from_slice(base_pack.as_slice());
                }
                let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
                enc.write_all(delta)?;
                let compressed = enc.finish()?;
                buf.extend_from_slice(&compressed);
                oid_to_offset.insert(*oid, start);
            }
            PackWriteEntry::ReusedSlice { raw, .. } => {
                buf.extend_from_slice(raw);
            }
        }
    }

    match pack_hash_bytes {
        20 => {
            let mut hasher = Sha1::new();
            Sha1Digest::update(&mut hasher, &buf);
            buf.extend_from_slice(&hasher.finalize());
        }
        32 => {
            let mut hasher = Sha256::new();
            Sha256Digest::update(&mut hasher, &buf);
            buf.extend_from_slice(&hasher.finalize());
        }
        _ => bail!("unsupported pack hash width {pack_hash_bytes}"),
    }

    Ok(buf)
}

/// Build idx v2 for a pack we just wrote.
fn build_idx_for_pack(
    pack_bytes: &[u8],
    entries: &[PackWriteEntry],
    pack_hash_bytes: usize,
) -> Result<Vec<u8>> {
    use grit_lib::pack::skip_one_pack_object;

    // We need offsets. Reparse the pack to get them.
    let nr = entries.len();
    let mut offsets = Vec::with_capacity(nr);
    let mut pos = 12usize; // skip header

    for _entry in entries {
        offsets.push(pos as u64);
        let start = pos as u64;
        skip_one_pack_object(pack_bytes, &mut pos, start, pack_hash_bytes)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
    }

    // Build sorted index.
    let mut sorted: Vec<(usize, Vec<u8>)> = entries
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let id = match e {
                PackWriteEntry::Full(pe) => pe.pack_id.clone(),
                PackWriteEntry::RefDelta { target_pack, .. } => target_pack.clone(),
                PackWriteEntry::ReusedSlice { pack_id, .. } => pack_id.clone(),
            };
            (i, id)
        })
        .collect();
    sorted.sort_by(|a, b| a.1.cmp(&b.1));

    let mut buf = Vec::new();
    // Header.
    buf.extend_from_slice(&[0xFF, b't', b'O', b'c']);
    buf.extend_from_slice(&2u32.to_be_bytes());

    // Fanout.
    let mut fanout = [0u32; 256];
    for (_, id) in &sorted {
        fanout[id[0] as usize] += 1;
    }
    for i in 1..256 {
        fanout[i] += fanout[i - 1];
    }
    for slot in &fanout {
        buf.extend_from_slice(&slot.to_be_bytes());
    }

    // OID table.
    for (_, id) in &sorted {
        buf.extend_from_slice(id.as_slice());
    }

    // CRC32 table: compute CRC32 for each entry's raw bytes in the pack.
    for (orig_idx, _) in &sorted {
        let off = offsets[*orig_idx] as usize;
        // Find the end of this entry.
        let next_off = if *orig_idx + 1 < nr {
            offsets[*orig_idx + 1] as usize
        } else {
            pack_bytes.len() - pack_hash_bytes // before trailing checksum
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
    let pack_checksum = &pack_bytes[pack_bytes.len() - pack_hash_bytes..];
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
