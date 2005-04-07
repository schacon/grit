//! `grit repack` — pack objects and optionally remove redundant packs.
//!
//! Matches Git’s plumbing: default `repack -d -l` runs `pack-objects` with **`--all --reflog
//! --indexed-objects --unpacked --incremental`** (incremental repack). **`repack -a` / `-A`**
//! runs a full repack into one pack (with optional **`--unpack-unreachable`**), same as `git gc`.
//! **`--geometric`** mirrors `git repack --geometric`: `pack-objects --stdin-packs --unpacked`
//! over a computed pack split, optional promisor merge, MIDX, and redundant pack removal.

use crate::commands::update_server_info;
use crate::grit_exe;
use crate::trace2_emit_git_subcommand_argv;
use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use grit_lib::config::ConfigSet;
use grit_lib::midx::{
    clear_pack_midx_state, write_multi_pack_index_with_options, WriteMultiPackIndexOptions,
};
use grit_lib::objects::ObjectId;
use grit_lib::pack::read_pack_index;
use grit_lib::pack_geometry::{
    collect_geometry_packs, collect_promisor_geometry_packs, compute_geometry_split,
    preferred_pack_stem_after_split, GeometricPack,
};
use grit_lib::promisor::{promisor_pack_object_ids, repo_treats_promisor_packs};
use grit_lib::repo::Repository;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Arguments for `grit repack`.
#[derive(Debug, ClapArgs)]
#[command(about = "Pack unpacked objects in a repository")]
pub struct Args {
    /// Remove redundant packs after repacking (keeps the pack created by this run).
    #[arg(short = 'd')]
    pub delete_old: bool,

    /// Pass `--local` to pack-objects (accepted for compat).
    #[arg(short = 'l')]
    pub local: bool,

    /// Pack everything into a single pack (Git `-a`).
    #[arg(short = 'a', conflicts_with = "repack_all_unpack")]
    pub all: bool,

    /// Like `-a`, and loosen unreachable objects per `--unpack-unreachable` (Git `-A`).
    #[arg(short = 'A', conflicts_with = "all")]
    pub repack_all_unpack: bool,

    /// Write a bitmap index (same as `git repack -b`). Fails when promisor packs are present or
    /// the object set is not closed (matches Git’s bitmap constraints).
    #[arg(
        short = 'b',
        long = "write-bitmap-index",
        visible_alias = "write-bitmap"
    )]
    pub write_bitmap: bool,

    /// Suppress bitmap index write (Git `repack` incremental auto-gc path).
    #[arg(long = "no-write-bitmap-index")]
    pub no_write_bitmap_index: bool,

    #[arg(short = 'q', long = "quiet")]
    pub quiet: bool,

    /// Pass `--no-reuse-delta` (accepted; forwarded to pack-objects).
    #[arg(short = 'f')]
    pub force: bool,

    /// Use deeper delta compression (same as `git gc --aggressive`).
    #[arg(long)]
    pub aggressive: bool,

    #[arg(long)]
    pub window: Option<i64>,

    #[arg(long)]
    pub depth: Option<i64>,

    /// Write cruft pack (accepted; forwarded to pack-objects).
    #[arg(long)]
    pub cruft: bool,

    #[arg(long = "no-cruft")]
    pub no_cruft: bool,

    /// Expire cruft objects older than this (`git repack --cruft-expiration`, forwarded to the
    /// cruft `pack-objects` pass).
    #[arg(long = "cruft-expiration", value_name = "TIME")]
    pub cruft_expiration: Option<String>,

    /// With `-A` / `-a`, do not loosen objects older than this (Git `--unpack-unreachable=<date>`).
    #[arg(long = "unpack-unreachable", value_name = "DATE")]
    pub unpack_unreachable: Option<String>,

    /// List-objects filter (forwarded to `pack-objects`, e.g. `blob:none`).
    #[arg(long = "filter", value_name = "SPEC")]
    pub filter: Option<String>,

    /// Destination pack prefix for filtered-out objects (`git repack --filter-to`).
    #[arg(long = "filter-to", value_name = "DIR")]
    pub filter_to: Option<String>,

    /// Alternate location for pruned objects (`git repack --expire-to`).
    #[arg(long = "expire-to", value_name = "DIR")]
    pub expire_to: Option<String>,

    /// Limit cruft pack size (`git repack --max-cruft-size`).
    #[arg(long = "max-cruft-size", value_name = "SIZE")]
    pub max_cruft_size: Option<String>,

    /// Do not repack this pack (basename `pack-….pack`; repeatable).
    #[arg(long = "keep-pack", value_name = "NAME", action = clap::ArgAction::Append)]
    pub keep_pack: Vec<String>,

    /// Geometric repack factor (same as `git repack --geometric=<n>`).
    #[arg(short = 'g', long = "geometric")]
    pub geometric: Option<i32>,

    /// Write multi-pack-index after repack.
    #[arg(short = 'm', long = "write-midx")]
    pub write_midx: bool,

    /// Repack objects inside `.keep` packs (matches `git repack --pack-kept-objects`).
    #[arg(long = "pack-kept-objects", action = clap::ArgAction::SetTrue)]
    pub pack_kept_objects: bool,

    /// Maximum pack size in bytes (forwarded to pack-objects).
    #[arg(long = "max-pack-size")]
    pub max_pack_size: Option<String>,

    /// Extra arguments (ignored).
    #[arg(value_name = "ARG", num_args = 0.., allow_hyphen_values = true, trailing_var_arg = true)]
    pub rest: Vec<String>,
}

fn parse_config_byte_size(raw: &str) -> Option<u64> {
    let s = raw.trim();
    if s.is_empty() {
        return None;
    }
    let upper = s.to_ascii_uppercase();
    let (digits, mult) = if upper.ends_with('K') {
        (&s[..s.len() - 1], 1024u64)
    } else if upper.ends_with('M') {
        (&s[..s.len() - 1], 1024u64 * 1024)
    } else if upper.ends_with('G') {
        (&s[..s.len() - 1], 1024u64 * 1024 * 1024)
    } else {
        (s, 1u64)
    };
    let n: u64 = digits.trim().parse().ok()?;
    Some(n.saturating_mul(mult))
}

/// Git `write_bitmaps` after config / defaults: negative means “quiet bitmap” path, `>0` enables.
fn effective_write_bitmaps_int(
    args: &Args,
    cfg: &ConfigSet,
    full_repack: bool,
    bare_repo: bool,
) -> i32 {
    let mut wb: i32 = if args.write_bitmap {
        1
    } else if args.no_write_bitmap_index {
        0
    } else {
        -1
    };
    if wb < 0 {
        if let Some(v) = cfg
            .get("repack.writebitmaps")
            .or_else(|| cfg.get("pack.writeBitmaps"))
        {
            wb = if v == "true" || v == "1" || v.eq_ignore_ascii_case("yes") {
                1
            } else {
                0
            };
        }
    }
    if wb < 0 {
        if !args.write_midx && (!full_repack || !bare_repo) {
            wb = 0;
        }
    }
    wb
}

/// Whether to include objects from `.keep` packs in the new pack(s), matching Git `pack_kept_objects`.
fn resolve_pack_kept_objects(
    args: &Args,
    cfg: &ConfigSet,
    full_repack: bool,
    bare_repo: bool,
) -> bool {
    if args.pack_kept_objects {
        return true;
    }
    if let Some(v) = cfg
        .get("repack.packkeptobjects")
        .or_else(|| cfg.get("repack.packKeptObjects"))
    {
        return v == "true" || v == "1" || v.eq_ignore_ascii_case("yes");
    }
    effective_write_bitmaps_int(args, cfg, full_repack, bare_repo) > 0 && !args.write_midx
}

/// Run `grit repack`.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let cfg = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();

    let geometric = args.geometric.unwrap_or(0).max(0);
    let full_repack_early = args.all || args.repack_all_unpack || args.cruft;
    let bare_repo = repo.work_tree.is_none();
    let pack_kept_objects = resolve_pack_kept_objects(&args, &cfg, full_repack_early, bare_repo);

    if geometric > 0 && (args.all || args.repack_all_unpack) {
        anyhow::bail!("options '--geometric' and '-a' cannot be used together");
    }
    if geometric > 0 {
        return run_geometric(&repo, &args, pack_kept_objects, geometric);
    }

    if args.cruft && args.repack_all_unpack {
        anyhow::bail!("options '-A' and '--cruft' cannot be used together");
    }
    // Git `pack-objects` cannot combine `--write-bitmap-index` with `--filter`; repack fails once
    // the filtered pack is incomplete for bitmap closure (`t7700-repack`).
    if (args.all || args.repack_all_unpack)
        && !args.cruft
        && !args.write_midx
        && args
            .filter
            .as_deref()
            .map(str::trim)
            .is_some_and(|s| !s.is_empty())
    {
        let wb = effective_write_bitmaps_int(&args, &cfg, true, bare_repo);
        if wb > 0 {
            anyhow::bail!("fatal: failed to write bitmap index");
        }
    }
    let work_dir = repo.work_tree.as_deref().unwrap_or(&repo.git_dir);
    let grit_bin = grit_exe::grit_executable();

    let pack_base = if repo.work_tree.is_some() {
        ".git/objects/pack/pack"
    } else {
        "objects/pack/pack"
    };

    let pack_dir_abs = repo.git_dir.join("objects").join("pack");
    ensure_no_orphan_pack_indexes(&pack_dir_abs)?;

    let full_repack = args.all || args.repack_all_unpack || args.cruft;
    if full_repack {
        clear_pack_midx_state(&pack_dir_abs).map_err(|e| anyhow::anyhow!("{e}"))?;
    }
    let loosen_unreachable = args.repack_all_unpack && !args.cruft;

    let mut write_bitmaps = effective_write_bitmaps_int(&args, &cfg, full_repack, bare_repo);
    let objects_dir_for_warn = repo.git_dir.join("objects");
    let mut quiet_pack_objects_local_alt = false;
    if args.local
        && grit_lib::pack::read_alternates_recursive(&objects_dir_for_warn)
            .map_or(false, |v| !v.is_empty())
        && !args.no_write_bitmap_index
        && write_bitmaps != 0
    {
        eprintln!("warning: disabling bitmap writing, as some objects are not being packed");
        write_bitmaps = 0;
        quiet_pack_objects_local_alt = true;
    }
    if write_bitmaps != 0 {
        if repo_treats_promisor_packs(&repo.git_dir, &cfg)
            && !promisor_pack_object_ids(&objects_dir_for_warn).is_empty()
        {
            anyhow::bail!("fatal: failed to write bitmap index");
        }
    }
    if pack_dir_has_any_keep_file(&pack_dir_abs) {
        write_bitmaps = 0;
    }
    if let Some(raw) = cfg
        .get("pack.packsizelimit")
        .or_else(|| cfg.get("pack.packSizeLimit"))
    {
        if parse_config_byte_size(&raw).map(|n| n > 0).unwrap_or(false)
            && args.max_pack_size.is_none()
            && write_bitmaps < 0
        {
            // Git disables quiet bitmaps when packs may split (`t7700-repack` auto-bitmaps test).
            write_bitmaps = 0;
        }
    }

    let mut new_pack_names: Vec<String> = Vec::new();

    let max_cruft_bytes = args
        .max_cruft_size
        .as_deref()
        .and_then(parse_config_byte_size);

    let run_one_pack_objects =
        |main_phase: bool, stdin_lines: Option<&[String]>, base: &str| -> Result<String> {
            let mut cmd = Command::new(&grit_bin);
            cmd.current_dir(work_dir)
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .arg("pack-objects")
                .arg("--keep-true-parents")
                .arg("--non-empty");

            for k in &args.keep_pack {
                cmd.arg("--keep-pack").arg(k);
            }

            if !main_phase || !pack_kept_objects {
                cmd.arg("--honor-pack-keep");
            }

            cmd.arg("--all");
            if full_repack {
                cmd.arg("--reflog").arg("--indexed-objects");
            }

            if full_repack {
                if main_phase {
                    if args.cruft {
                        cmd.arg("--reachability-all");
                    }
                    if args.no_cruft {
                        cmd.arg("--no-cruft");
                    }
                } else {
                    cmd.arg("--cruft");
                    if let Some(ref exp) = args.cruft_expiration {
                        if !exp.is_empty() {
                            cmd.arg(format!("--cruft-expiration={exp}"));
                        }
                    }
                    if let Some(n) = max_cruft_bytes {
                        // Git maps `--max-cruft-size` on repack to `pack-objects --max-pack-size`.
                        cmd.arg(format!("--max-pack-size={n}"));
                    }
                }
                if main_phase {
                    if let Some(exp) = args.unpack_unreachable.as_deref() {
                        cmd.arg(format!("--unpack-unreachable={exp}"));
                    } else if loosen_unreachable {
                        cmd.arg("--unpack-unreachable");
                    }
                }
            } else {
                cmd.arg("--reflog")
                    .arg("--indexed-objects")
                    .arg("--unpacked")
                    .arg("--incremental");
            }

            if let Some(ref f) = args.filter {
                if !f.is_empty() {
                    cmd.arg(format!("--filter={f}"));
                }
            }
            if let Some(ref to) = args.filter_to {
                if !to.is_empty() {
                    cmd.arg("--filter-to").arg(to);
                }
            }

            cmd.arg(base);

            // Incremental repack (`repack -d` without `-a`) must stay stderr-silent (`t7700-repack`).
            // Full repack without bitmaps must not print `pack-objects` progress (`t7700-repack`).
            if args.quiet
                || !full_repack
                || (main_phase && quiet_pack_objects_local_alt)
                || (full_repack && main_phase && write_bitmaps == 0)
            {
                cmd.arg("-q");
            }
            if args.aggressive {
                cmd.arg("-f");
                cmd.arg("--window").arg("250");
                cmd.arg("--depth").arg("250");
            } else {
                if args.force {
                    cmd.arg("-f");
                }
                if let Some(w) = args.window {
                    cmd.arg("--window").arg(w.to_string());
                }
                if let Some(d) = args.depth {
                    cmd.arg("--depth").arg(d.to_string());
                }
            }

            if repo_treats_promisor_packs(&repo.git_dir, &cfg) {
                cmd.arg("--exclude-promisor-objects");
            }

            if args.local {
                cmd.arg("--local");
            }

            if main_phase {
                if write_bitmaps > 0 {
                    cmd.arg("--write-bitmap-index");
                } else if write_bitmaps < 0 && !args.no_write_bitmap_index {
                    cmd.arg("--write-bitmap-index-quiet");
                }
            }
            if args.no_write_bitmap_index {
                cmd.arg("--no-write-bitmap-index");
            }

            if let Some(lines) = stdin_lines {
                use std::io::Write;
                cmd.stdin(Stdio::piped());
                let mut child = cmd.spawn().context("failed to spawn grit pack-objects")?;
                {
                    let mut stdin = child.stdin.take().context("pack-objects stdin")?;
                    for line in lines {
                        writeln!(stdin, "{line}")?;
                    }
                }
                let output = child
                    .wait_with_output()
                    .context("failed to run grit pack-objects")?;
                if !output.status.success() {
                    anyhow::bail!("pack-objects failed with status {}", output.status);
                }
                let hash = output
                    .stdout
                    .split(|b| *b == b'\n')
                    .next()
                    .and_then(|line| std::str::from_utf8(line).ok())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .unwrap_or("");
                return Ok(hash.to_string());
            }

            let output = cmd.output().context("failed to run grit pack-objects")?;
            if !output.status.success() {
                anyhow::bail!("pack-objects failed with status {}", output.status);
            }

            let hash = output
                .stdout
                .split(|b| *b == b'\n')
                .next()
                .and_then(|line| std::str::from_utf8(line).ok())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or("");
            Ok(hash.to_string())
        };

    if args.cruft && full_repack {
        let main_hash = run_one_pack_objects(true, None, pack_base)?;
        if !main_hash.is_empty() {
            new_pack_names.push(format!("pack-{main_hash}.pack"));

            let objects_dir = repo.git_dir.join("objects");
            let indexes_before_cruft = grit_lib::pack::read_local_pack_indexes(&objects_dir)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            let mut stdin_lines: Vec<String> = Vec::new();
            stdin_lines.push(format!("pack-{main_hash}.pack"));
            for idx in &indexes_before_cruft {
                let name = idx
                    .pack_path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("");
                if !name.ends_with(".pack") {
                    continue;
                }
                if name == format!("pack-{main_hash}.pack") {
                    continue;
                }
                let stem = name.strip_suffix(".pack").unwrap_or(name);
                let retained = args.keep_pack.iter().any(|k| basename_matches(k, stem));
                if retained {
                    stdin_lines.push(name.to_string());
                } else {
                    stdin_lines.push(format!("-{name}"));
                }
            }

            let cruft_base = if let Some(ref et) = args.expire_to {
                let t = et.trim();
                if !t.is_empty() {
                    t
                } else {
                    pack_base
                }
            } else {
                pack_base
            };

            let cruft_hash = run_one_pack_objects(false, Some(&stdin_lines), cruft_base)?;
            if !cruft_hash.is_empty() {
                new_pack_names.push(format!("pack-{cruft_hash}.pack"));
            }
        }
    } else {
        let hash = run_one_pack_objects(true, None, pack_base)?;
        if !hash.is_empty() {
            new_pack_names.push(format!("pack-{hash}.pack"));
        }
    }

    // Second `pack-objects --stdin-packs` pass for `repack --filter` (Git `write_filtered_pack`).
    if full_repack
        && !args.cruft
        && args
            .filter
            .as_deref()
            .map(str::trim)
            .is_some_and(|s| !s.is_empty())
    {
        if let Some(last) = new_pack_names.last().cloned() {
            let filter_dest = args
                .filter_to
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or(pack_base);
            if let Some(h) = run_filtered_followup_pack_objects(
                &grit_bin,
                work_dir,
                &repo.git_dir,
                &pack_dir_abs,
                &last,
                filter_dest,
                &args,
                pack_kept_objects,
                write_bitmaps,
            )? {
                new_pack_names.push(format!("pack-{h}.pack"));
            }
        }
    }

    let mut trace_argv = vec![
        "git".to_string(),
        "repack".to_string(),
        "-d".to_string(),
        "-l".to_string(),
    ];
    if !full_repack {
        if args.no_write_bitmap_index {
            trace_argv.push("--no-write-bitmap-index".to_string());
        }
    } else if args.cruft {
        trace_argv.push("--cruft".to_string());
        let exp = args
            .cruft_expiration
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("2.weeks.ago");
        trace_argv.push(format!("--cruft-expiration={exp}"));
        if let Some(n) = max_cruft_bytes {
            trace_argv.push(format!("--max-cruft-size={n}"));
        }
    } else if args.repack_all_unpack {
        trace_argv.push("-A".to_string());
        match args.unpack_unreachable.as_deref() {
            Some(u) if !u.is_empty() => trace_argv.push(format!("--unpack-unreachable={u}")),
            _ => trace_argv.push("--unpack-unreachable".to_string()),
        }
    } else if args.all {
        trace_argv.push("-a".to_string());
    }
    if args.no_cruft {
        trace_argv.push("--no-cruft".to_string());
    }
    for k in &args.keep_pack {
        trace_argv.push("--keep-pack".to_string());
        trace_argv.push(k.clone());
    }
    if let Some(ref f) = args.filter {
        if !f.is_empty() {
            trace_argv.push(format!("--filter={f}"));
        }
    }
    if let Some(ref to) = args.filter_to {
        if !to.is_empty() {
            trace_argv.push("--filter-to".to_string());
            trace_argv.push(to.clone());
        }
    }
    if let Some(ref et) = args.expire_to {
        let t = et.trim();
        if !t.is_empty() {
            trace_argv.push(format!("--expire-to={t}"));
        }
    }
    if args.quiet {
        trace_argv.push("-q".to_string());
    }
    if args.aggressive {
        trace_argv.push("--aggressive".to_string());
    }
    if write_bitmaps > 0 {
        trace_argv.push("-b".to_string());
    }
    trace2_emit_git_subcommand_argv(&trace_argv);

    if args.delete_old {
        if full_repack {
            let mut keep: Vec<String> = new_pack_names.clone();
            // `pack-objects` may append names here when `blob:none` writes a sibling pack via stdin
            // (`write_pack_via_stdin_objects`). That pack must stay in the keep set while old packs
            // are still retained for duplicate objects; otherwise `remove_superseded_packs_*` treats
            // the side pack as redundant (`t7700-repack` filter tests).
            keep.extend(take_extra_packs_recorded_for_repack(&repo.git_dir)?);
            keep.extend(args.keep_pack.iter().cloned());
            let mut extra_objects_dirs: Vec<PathBuf> = Vec::new();
            for ft in [
                args.filter_to
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty()),
                args.expire_to
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty()),
            ]
            .into_iter()
            .flatten()
            {
                let base = work_dir.join(ft);
                if let Some(pack_dir_parent) = base.parent() {
                    if let Some(objects_dir) = pack_dir_parent.parent() {
                        extra_objects_dirs.push(objects_dir.to_path_buf());
                    }
                }
            }
            remove_superseded_packs_after_full_repack(&pack_dir_abs, &keep, &extra_objects_dirs)?;
        } else {
            let new_pack_name = new_pack_names.first().cloned().context("no pack written")?;
            remove_superseded_packs_incremental(&pack_dir_abs, &new_pack_name, &args.keep_pack)?;
        }
        update_server_info::refresh_objects_info_packs(&repo)?;
    }

    Ok(())
}

fn run_geometric(
    repo: &Repository,
    args: &Args,
    pack_kept_objects: bool,
    split_factor: i32,
) -> Result<()> {
    let cfg = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
    let work_dir = repo.work_tree.as_deref().unwrap_or(&repo.git_dir);
    let grit_bin = grit_exe::grit_executable();
    let pack_dir = repo.git_dir.join("objects").join("pack");
    let objects_dir = repo.git_dir.join("objects");
    ensure_no_orphan_pack_indexes(&pack_dir)?;

    let bare_repo = repo.work_tree.is_none();
    let mut write_bitmaps = effective_write_bitmaps_int(args, &cfg, false, bare_repo);
    if args.local
        && grit_lib::pack::read_alternates_recursive(&objects_dir).map_or(false, |v| !v.is_empty())
        && !args.no_write_bitmap_index
        && write_bitmaps != 0
    {
        eprintln!("warning: disabling bitmap writing, as some objects are not being packed");
        write_bitmaps = 0;
    }
    if pack_dir_has_any_keep_file(&pack_dir) {
        write_bitmaps = 0;
    }
    if let Some(raw) = cfg
        .get("pack.packsizelimit")
        .or_else(|| cfg.get("pack.packSizeLimit"))
    {
        if parse_config_byte_size(&raw).map(|n| n > 0).unwrap_or(false)
            && args.max_pack_size.is_none()
            && write_bitmaps < 0
        {
            write_bitmaps = 0;
        }
    }

    let keep_packs: Vec<String> = args.keep_pack.clone();

    let normal = collect_geometry_packs(&objects_dir, pack_kept_objects, &keep_packs)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let weights: Vec<usize> = normal.iter().map(|p| p.object_count).collect();
    let split = compute_geometry_split(&weights, split_factor);
    let pref_stem = preferred_pack_stem_after_split(&normal, split);

    let promisor_list =
        collect_promisor_geometry_packs(&objects_dir, pack_kept_objects, &keep_packs)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
    let prom_weights: Vec<usize> = promisor_list.iter().map(|p| p.object_count).collect();
    let prom_split = compute_geometry_split(&prom_weights, split_factor);

    let has_loose = objects_dir_has_loose_objects(&objects_dir);

    let pack_base = if repo.work_tree.is_some() {
        ".git/objects/pack/pack"
    } else {
        "objects/pack/pack"
    };

    let mut promisor_written: Vec<String> = Vec::new();
    let mut normal_written: Vec<String> = Vec::new();

    let should_run_pack_objects = prom_split > 0 || split > 0 || !normal.is_empty() || has_loose;

    if !should_run_pack_objects {
        if !args.quiet {
            println!("Nothing new to pack.");
        }
    } else {
        if prom_split > 0 {
            let stdin = build_stdin_packs_lines(&promisor_list, prom_split);
            promisor_written = run_pack_objects_stdin(
                &grit_bin,
                work_dir,
                &repo.git_dir,
                pack_base,
                &stdin,
                args,
                &cfg,
                pack_kept_objects,
                write_bitmaps,
                true,
            )?;
        }

        if split > 0 {
            let stdin = build_stdin_packs_lines(&normal, split);
            normal_written = run_pack_objects_stdin(
                &grit_bin,
                work_dir,
                &repo.git_dir,
                pack_base,
                &stdin,
                args,
                &cfg,
                pack_kept_objects,
                write_bitmaps,
                false,
            )?;
        } else if !normal.is_empty() || has_loose {
            // Progression intact (or no packs yet) but loose objects need packing (`--unpacked`).
            let stdin = build_stdin_packs_lines(&normal, 0);
            normal_written = run_pack_objects_stdin(
                &grit_bin,
                work_dir,
                &repo.git_dir,
                pack_base,
                &stdin,
                args,
                &cfg,
                pack_kept_objects,
                write_bitmaps,
                false,
            )?;
        }

        if normal_written.is_empty() && promisor_written.is_empty() && !args.quiet {
            println!("Nothing new to pack.");
        }
    }

    if !should_run_pack_objects {
        if args.write_midx {
            let has_local_idx = fs::read_dir(&pack_dir)
                .map(|rd| {
                    rd.filter_map(|e| e.ok()).any(|e| {
                        let n = e.file_name().to_string_lossy().to_string();
                        n.starts_with("pack-") && n.ends_with(".idx")
                    })
                })
                .unwrap_or(false);
            if has_local_idx {
                let pref_idx = preferred_pack_index(&pack_dir, pref_stem.as_deref())?;
                let bitmap_placeholders = write_bitmaps > 0 && !args.no_write_bitmap_index;
                write_multi_pack_index_with_options(
                    &pack_dir,
                    &WriteMultiPackIndexOptions {
                        preferred_pack_idx: pref_idx,
                        write_bitmap_placeholders: bitmap_placeholders,
                        ..Default::default()
                    },
                )
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            }
        }
        if args.delete_old {
            update_server_info::refresh_objects_info_packs(repo)?;
        }
        return Ok(());
    }

    if args.write_midx {
        let pref_idx = preferred_pack_index(&pack_dir, pref_stem.as_deref())?;
        let bitmap = write_bitmaps > 0
            && !args.no_write_bitmap_index
            && !(args.local
                && grit_lib::pack::read_alternates_recursive(&objects_dir)
                    .map_or(false, |v| !v.is_empty()));
        write_multi_pack_index_with_options(
            &pack_dir,
            &WriteMultiPackIndexOptions {
                preferred_pack_idx: pref_idx,
                write_bitmap_placeholders: bitmap,
                ..Default::default()
            },
        )
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    }

    if args.delete_old {
        remove_geometry_redundant(
            &pack_dir,
            &normal,
            split,
            &promisor_list,
            prom_split,
            pack_kept_objects,
            &keep_packs,
            &promisor_written,
            &normal_written,
        )?;
        let opts = grit_lib::prune_packed::PrunePackedOptions::default();
        grit_lib::prune_packed::prune_packed_objects(&objects_dir, opts)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        remove_duplicate_packs_matching_alternates(&objects_dir)?;
        update_server_info::refresh_objects_info_packs(repo)?;
    }

    Ok(())
}

fn objects_dir_has_loose_objects(objects_dir: &Path) -> bool {
    let Ok(rd) = fs::read_dir(objects_dir) else {
        return false;
    };
    for entry in rd.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.len() != 2 || !name.chars().all(|c| c.is_ascii_hexdigit()) {
            continue;
        }
        let Ok(sub) = fs::read_dir(entry.path()) else {
            continue;
        };
        for f in sub.flatten() {
            let n = f.file_name().to_string_lossy().to_string();
            if n.len() == 38 && n.chars().all(|c| c.is_ascii_hexdigit()) {
                return true;
            }
        }
    }
    false
}

fn remove_duplicate_packs_matching_alternates(objects_dir: &Path) -> Result<()> {
    let local_pack = objects_dir.join("pack");
    let alts = match grit_lib::pack::read_alternates_recursive(objects_dir) {
        Ok(a) => a,
        Err(_) => return Ok(()),
    };
    for alt in alts {
        let alt_pack = alt.join("pack");
        let rd = match fs::read_dir(&local_pack) {
            Ok(r) => r,
            Err(_) => return Ok(()),
        };
        for entry in rd.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.starts_with("pack-") || !name.ends_with(".pack") {
                continue;
            }
            let local_path = entry.path();
            let alt_path = alt_pack.join(&name);
            if !alt_path.is_file() {
                continue;
            }
            let lm = fs::metadata(&local_path).map_err(|e| anyhow::anyhow!(e))?;
            let am = fs::metadata(&alt_path).map_err(|e| anyhow::anyhow!(e))?;
            if lm.len() != am.len() || lm.len() < 20 {
                continue;
            }
            let mut lb = vec![0u8; 20];
            let mut ab = vec![0u8; 20];
            let ldata = fs::read(&local_path).map_err(|e| anyhow::anyhow!(e))?;
            let adata = fs::read(&alt_path).map_err(|e| anyhow::anyhow!(e))?;
            if ldata.len() != adata.len() {
                continue;
            }
            if ldata.len() < 20 {
                continue;
            }
            lb.copy_from_slice(&ldata[ldata.len() - 20..]);
            ab.copy_from_slice(&adata[adata.len() - 20..]);
            if lb != ab {
                continue;
            }
            let stem = name.strip_suffix(".pack").unwrap_or(&name).to_string();
            let _ = fs::remove_file(&local_path);
            let _ = fs::remove_file(local_pack.join(format!("{stem}.idx")));
            let _ = fs::remove_file(local_pack.join(format!("{stem}.promisor")));
        }
    }
    Ok(())
}

fn preferred_pack_index(pack_dir: &Path, stem: Option<&str>) -> Result<Option<u32>> {
    let Some(stem) = stem else {
        return Ok(None);
    };
    let want = format!("{stem}.idx");
    let mut names: Vec<String> = fs::read_dir(pack_dir)
        .map_err(|e| anyhow::anyhow!(e))?
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let n = e.file_name().to_string_lossy().to_string();
            (n.starts_with("pack-") && n.ends_with(".idx")).then_some(n)
        })
        .collect();
    names.sort();
    let idx = names.iter().position(|n| n == &want);
    Ok(idx.map(|i| i as u32))
}

fn build_stdin_packs_lines(packs: &[GeometricPack], split: usize) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut inc: Vec<&GeometricPack> = packs.iter().take(split).collect();
    inc.sort_by_key(|p| p.mtime_secs);
    for p in inc {
        lines.push(p.stem.clone());
    }
    for p in packs.iter().skip(split) {
        lines.push(format!("^{}", p.stem));
    }
    format!("{}\n", lines.join("\n"))
}

fn run_pack_objects_stdin(
    grit_bin: &Path,
    work_dir: &Path,
    git_dir: &Path,
    pack_base: &str,
    stdin_text: &str,
    args: &Args,
    cfg: &ConfigSet,
    pack_kept_objects: bool,
    write_bitmaps: i32,
    is_promisor: bool,
) -> Result<Vec<String>> {
    let mut cmd = Command::new(grit_bin);
    cmd.current_dir(work_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .arg("pack-objects")
        .arg("--stdin-packs")
        .arg("--unpacked")
        .arg("--non-empty")
        .arg(pack_base);
    if args.quiet {
        cmd.arg("-q");
    }
    if args.local {
        cmd.arg("--local");
    }
    if !pack_kept_objects {
        cmd.arg("--honor-pack-keep");
    }
    if write_bitmaps > 0 {
        cmd.arg("--write-bitmap-index");
    } else if write_bitmaps < 0 && !args.no_write_bitmap_index {
        cmd.arg("--write-bitmap-index-quiet");
    }
    if args.no_write_bitmap_index {
        cmd.arg("--no-write-bitmap-index");
    }
    if args.aggressive {
        cmd.arg("-f");
        cmd.arg("--window").arg("250");
        cmd.arg("--depth").arg("250");
    } else {
        if args.force {
            cmd.arg("-f");
        }
        if let Some(w) = args.window {
            cmd.arg("--window").arg(w.to_string());
        }
        if let Some(d) = args.depth {
            cmd.arg("--depth").arg(d.to_string());
        }
    }
    if let Some(ref s) = args.max_pack_size {
        cmd.arg("--max-pack-size").arg(s);
    }
    if repo_treats_promisor_packs(git_dir, cfg) && !is_promisor {
        cmd.arg("--exclude-promisor-objects");
    }

    let mut child = cmd.spawn().context("spawn pack-objects")?;
    let mut stdin = child.stdin.take().context("pack-objects stdin")?;
    stdin.write_all(stdin_text.as_bytes())?;
    drop(stdin);
    let output = child.wait_with_output().context("wait pack-objects")?;
    if !output.status.success() {
        anyhow::bail!("pack-objects failed with status {}", output.status);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let hashes: Vec<String> = stdout
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect();
    Ok(hashes)
}

fn remove_geometry_redundant(
    pack_dir: &Path,
    normal: &[GeometricPack],
    split: usize,
    promisor: &[GeometricPack],
    prom_split: usize,
    pack_kept_objects: bool,
    keep_pack_names: &[String],
    promisor_new_hashes: &[String],
    normal_new_hashes: &[String],
) -> Result<()> {
    fn remove_pack_stem(pack_dir: &Path, stem: &str) {
        let _ = fs::remove_file(pack_dir.join(format!("{stem}.pack")));
        let _ = fs::remove_file(pack_dir.join(format!("{stem}.idx")));
        let _ = fs::remove_file(pack_dir.join(format!("{stem}.promisor")));
    }

    for p in normal.iter().take(split) {
        if pack_dir.join(format!("{}.keep", p.stem)).is_file() && !pack_kept_objects {
            continue;
        }
        if keep_pack_names.iter().any(|k| basename_matches(k, &p.stem)) {
            continue;
        }
        remove_pack_stem(pack_dir, &p.stem);
    }

    for p in promisor.iter().take(prom_split) {
        if pack_dir.join(format!("{}.keep", p.stem)).is_file() && !pack_kept_objects {
            continue;
        }
        if keep_pack_names.iter().any(|k| basename_matches(k, &p.stem)) {
            continue;
        }
        remove_pack_stem(pack_dir, &p.stem);
    }

    for h in promisor_new_hashes {
        let stem = format!("pack-{h}");
        let marker = pack_dir.join(format!("{stem}.promisor"));
        if !marker.exists() {
            let _ = fs::write(&marker, []);
        }
    }
    let _ = normal_new_hashes;

    Ok(())
}

/// Second `pack-objects` invocation for `repack -a -d --filter=…` (Git `write_filtered_pack`).
///
/// Upstream `write_filtered_pack` runs `pack-objects --stdin-packs` **without** `--filter` (Git
/// forbids combining those options). The first pass already applied the filter; this pass packs
/// objects present in older packs but omitted from the new main pack.
fn run_filtered_followup_pack_objects(
    grit_bin: &Path,
    work_dir: &Path,
    git_dir: &Path,
    pack_dir: &Path,
    new_pack_name: &str,
    out_prefix: &str,
    args: &Args,
    pack_kept_objects: bool,
    write_bitmaps: i32,
) -> Result<Option<String>> {
    let new_base = pack_basename(new_pack_name);
    if !new_base.ends_with(".pack") {
        return Ok(None);
    }
    let mut stdin_lines: Vec<String> = vec![format!("^{new_base}")];
    let rd = fs::read_dir(pack_dir).map_err(|e| anyhow::anyhow!(e))?;
    for ent in rd.flatten() {
        let name = ent.file_name().to_string_lossy().to_string();
        if !name.starts_with("pack-") || !name.ends_with(".pack") {
            continue;
        }
        if name == new_base {
            continue;
        }
        let stem = name.strip_suffix(".pack").unwrap_or(&name);
        let kept_by_flag = args.keep_pack.iter().any(|k| basename_matches(k, stem));
        let kept_by_file = pack_dir.join(format!("{stem}.keep")).is_file();
        if kept_by_flag || kept_by_file {
            if pack_kept_objects {
                stdin_lines.push(name);
            } else {
                stdin_lines.push(format!("^{name}"));
            }
        } else {
            stdin_lines.push(name);
        }
    }
    if stdin_lines.len() <= 1 {
        return Ok(None);
    }

    let cfg = ConfigSet::load(Some(git_dir), true).unwrap_or_default();

    let mut cmd = Command::new(grit_bin);
    cmd.current_dir(work_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .arg("pack-objects")
        .arg("--stdin-packs")
        .arg("--keep-true-parents")
        .arg("--non-empty")
        .arg(out_prefix);
    for k in &args.keep_pack {
        cmd.arg("--keep-pack").arg(k);
    }
    if args.quiet || write_bitmaps == 0 {
        cmd.arg("-q");
    }
    if args.aggressive {
        cmd.arg("-f");
        cmd.arg("--window").arg("250");
        cmd.arg("--depth").arg("250");
    } else {
        if args.force {
            cmd.arg("-f");
        }
        if let Some(w) = args.window {
            cmd.arg("--window").arg(w.to_string());
        }
        if let Some(d) = args.depth {
            cmd.arg("--depth").arg(d.to_string());
        }
    }
    if let Some(ref s) = args.max_pack_size {
        cmd.arg("--max-pack-size").arg(s);
    }
    if repo_treats_promisor_packs(git_dir, &cfg) {
        cmd.arg("--exclude-promisor-objects");
    }
    if args.local {
        cmd.arg("--local");
    }
    if write_bitmaps > 0 {
        cmd.arg("--write-bitmap-index");
    } else if write_bitmaps < 0 && !args.no_write_bitmap_index {
        cmd.arg("--write-bitmap-index-quiet");
    }
    if args.no_write_bitmap_index {
        cmd.arg("--no-write-bitmap-index");
    }

    let mut child = cmd.spawn().context("spawn pack-objects filter follow-up")?;
    {
        let mut stdin = child.stdin.take().context("pack-objects stdin")?;
        for line in &stdin_lines {
            writeln!(stdin, "{line}")?;
        }
    }
    let output = child
        .wait_with_output()
        .context("wait pack-objects filter follow-up")?;
    if !output.status.success() {
        anyhow::bail!(
            "pack-objects (filter follow-up) failed with status {}",
            output.status
        );
    }
    let hash = output
        .stdout
        .split(|b| *b == b'\n')
        .next()
        .and_then(|line| std::str::from_utf8(line).ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    Ok(hash)
}

fn pack_dir_has_any_keep_file(pack_dir: &Path) -> bool {
    let Ok(rd) = fs::read_dir(pack_dir) else {
        return false;
    };
    for ent in rd.flatten() {
        let n = ent.file_name().to_string_lossy().to_string();
        if n.starts_with("pack-") && n.ends_with(".keep") {
            return true;
        }
    }
    false
}

/// Fail repack when a parseable `.idx` has no sibling `.pack` (repository corruption; `t7700-repack`).
/// Reads and removes `objects/info/grit-extra-packs` (one pack basename per line).
///
/// Populated by `pack-objects` when it writes an auxiliary pack during `--filter=blob:none`
/// handling; `repack -d` consumes the list so it is not reused across runs.
fn take_extra_packs_recorded_for_repack(git_dir: &Path) -> Result<Vec<String>> {
    let path = git_dir
        .join("objects")
        .join("info")
        .join("grit-extra-packs");
    let contents = match fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    let lines: Vec<String> = contents
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    let _ = fs::remove_file(&path);
    Ok(lines)
}

fn ensure_no_orphan_pack_indexes(pack_dir: &Path) -> Result<()> {
    let rd = match fs::read_dir(pack_dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    for ent in rd {
        let ent = ent.context("read objects/pack")?;
        let path = ent.path();
        if path.extension().and_then(|s| s.to_str()) != Some("idx") {
            continue;
        }
        if read_pack_index(&path).is_err() {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if !pack_dir.join(format!("{stem}.pack")).is_file() {
            anyhow::bail!("bad object pack {stem}.pack (missing)");
        }
    }
    Ok(())
}

fn basename_matches(keep: &str, stem: &str) -> bool {
    let p = Path::new(keep);
    let fname = p.file_name().and_then(|s| s.to_str()).unwrap_or(keep);
    let no_suf = fname.strip_suffix(".pack").unwrap_or(fname);
    no_suf == stem || fname == format!("{stem}.pack")
}

/// Deletes every `pack-*.pack` in `pack_dir` except the given basenames, unless a matching
/// `pack-*.keep` file exists for that pack. Used by `gc` when writing both a merged promisor pack
/// and a non-promisor pack in one pass.
fn pack_basename(name: &str) -> &str {
    Path::new(name)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(name)
}

fn remove_pack_sidecars(pack_dir: &Path, stem: &str) {
    let _ = fs::remove_file(pack_dir.join(format!("{stem}.mtimes")));
    let _ = fs::remove_file(pack_dir.join(format!("{stem}.rev")));
    let _ = fs::remove_file(pack_dir.join(format!("{stem}.bitmap")));
}

/// After a full repack, delete packs whose objects are entirely present in the union of the new
/// pack and any packs we must retain (e.g. `--keep-pack`, or an older pack that still holds
/// objects omitted by `--filter=blob:none`).
fn remove_superseded_packs_after_full_repack(
    pack_dir: &Path,
    initial_keep: &[String],
    extra_objects_dirs: &[PathBuf],
) -> Result<()> {
    let objects_dir = pack_dir
        .parent()
        .ok_or_else(|| anyhow::anyhow!("invalid pack directory"))?;
    let indexes =
        grit_lib::pack::read_local_pack_indexes(objects_dir).map_err(|e| anyhow::anyhow!("{e}"))?;

    let mut by_name: HashMap<String, HashSet<ObjectId>> = HashMap::new();
    for idx in &indexes {
        let name = idx
            .pack_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        if !name.ends_with(".pack") {
            continue;
        }
        let oids: HashSet<ObjectId> = idx.entries.iter().map(|e| e.oid).collect();
        by_name.insert(name, oids);
    }

    let mut retained: HashSet<String> = initial_keep
        .iter()
        .map(|k| pack_basename(k).to_string())
        .collect();

    let mut union_oids: HashSet<ObjectId> = HashSet::new();
    for name in &retained {
        if let Some(s) = by_name.get(name) {
            union_oids.extend(s.iter().copied());
        }
    }
    // Objects only in `.keep` packs must count toward supersession: otherwise an old pack that
    // duplicates a kept object is never removed (`t7700-repack` alternate + `.keep` chain).
    for (name, oids) in &by_name {
        let stem = name
            .strip_suffix(".pack")
            .unwrap_or(name.as_str())
            .to_string();
        if pack_dir.join(format!("{stem}.keep")).exists() {
            union_oids.extend(oids.iter().copied());
        }
    }
    for dir in extra_objects_dirs {
        let idxs =
            grit_lib::pack::read_local_pack_indexes(dir).map_err(|e| anyhow::anyhow!("{e}"))?;
        for idx in idxs {
            union_oids.extend(idx.entries.iter().map(|e| e.oid));
        }
    }

    let mut changed = true;
    while changed {
        changed = false;
        for (name, oids) in &by_name {
            if retained.contains(name) {
                continue;
            }
            let stem = name
                .strip_suffix(".pack")
                .unwrap_or(name.as_str())
                .to_string();
            if pack_dir.join(format!("{stem}.keep")).exists() {
                continue;
            }
            if pack_dir.join(format!("{stem}.promisor")).exists() {
                continue;
            }
            if oids.iter().all(|o| union_oids.contains(o)) {
                continue;
            }
            retained.insert(name.clone());
            union_oids.extend(oids.iter().copied());
            changed = true;
        }
    }

    for (name, _) in &by_name {
        if retained.contains(name) {
            continue;
        }
        let stem = name
            .strip_suffix(".pack")
            .unwrap_or(name.as_str())
            .to_string();
        if pack_dir.join(format!("{stem}.keep")).exists() {
            continue;
        }
        if pack_dir.join(format!("{stem}.promisor")).exists() {
            continue;
        }
        let _ = fs::remove_file(pack_dir.join(name));
        let _ = fs::remove_file(pack_dir.join(format!("{stem}.idx")));
        let _ = fs::remove_file(pack_dir.join(format!("{stem}.promisor")));
        remove_pack_sidecars(pack_dir, &stem);
    }

    Ok(())
}

pub(crate) fn remove_superseded_packs_multi(
    pack_dir: &Path,
    keep_pack_names: &[String],
) -> Result<()> {
    let rd = match fs::read_dir(pack_dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };

    for entry in rd {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.ends_with(".pack") {
            continue;
        }
        if keep_pack_names
            .iter()
            .any(|k| pack_basename(k) == name.as_str())
        {
            continue;
        }
        let stem = name
            .strip_suffix(".pack")
            .unwrap_or(name.as_str())
            .to_string();
        if pack_dir.join(format!("{stem}.keep")).exists() {
            continue;
        }
        let pack_path = pack_dir.join(&name);
        let idx_path = pack_dir.join(format!("{stem}.idx"));
        let _ = fs::remove_file(&pack_path);
        let _ = fs::remove_file(&idx_path);
        let _ = fs::remove_file(pack_dir.join(format!("{stem}.promisor")));
        remove_pack_sidecars(pack_dir, &stem);
    }

    Ok(())
}

/// Incremental repack: remove packs that became redundant (every object also in another pack).
fn remove_superseded_packs_incremental(
    pack_dir: &Path,
    new_pack_name: &str,
    always_keep: &[String],
) -> Result<()> {
    let objects_dir = pack_dir
        .parent()
        .ok_or_else(|| anyhow::anyhow!("invalid pack directory"))?;
    let indexes =
        grit_lib::pack::read_local_pack_indexes(objects_dir).map_err(|e| anyhow::anyhow!("{e}"))?;
    if indexes.len() < 2 {
        return Ok(());
    }

    let mut pack_to_oids: Vec<(
        String,
        std::collections::HashSet<grit_lib::objects::ObjectId>,
    )> = Vec::new();
    for idx in &indexes {
        let name = idx
            .pack_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        if !name.ends_with(".pack") {
            continue;
        }
        let oids: std::collections::HashSet<_> = idx.entries.iter().map(|e| e.oid).collect();
        pack_to_oids.push((name, oids));
    }

    let new_set = pack_to_oids
        .iter()
        .find(|(n, _)| n == new_pack_name)
        .map(|(_, s)| s.clone())
        .unwrap_or_default();

    for (name, oids) in &pack_to_oids {
        if name == new_pack_name {
            continue;
        }
        if always_keep
            .iter()
            .any(|k| pack_basename(k) == name.as_str())
        {
            continue;
        }
        let stem = name
            .strip_suffix(".pack")
            .unwrap_or(name.as_str())
            .to_string();
        if pack_dir.join(format!("{stem}.keep")).exists() {
            continue;
        }
        if pack_dir.join(format!("{stem}.promisor")).exists() {
            continue;
        }
        let mut covered = true;
        for oid in oids {
            if new_set.contains(oid) {
                continue;
            }
            let mut in_other = false;
            for (other_name, other_oids) in &pack_to_oids {
                if other_name == name {
                    continue;
                }
                if other_oids.contains(oid) {
                    in_other = true;
                    break;
                }
            }
            if !in_other {
                covered = false;
                break;
            }
        }
        if covered {
            let _ = fs::remove_file(pack_dir.join(name));
            let _ = fs::remove_file(pack_dir.join(format!("{stem}.idx")));
            let _ = fs::remove_file(pack_dir.join(format!("{stem}.promisor")));
            remove_pack_sidecars(pack_dir, &stem);
        }
    }

    Ok(())
}
