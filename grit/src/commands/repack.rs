//! `grit repack` — pack objects and optionally remove redundant packs.
//!
//! Matches Git’s plumbing: default `repack -d -l` runs `pack-objects` with **`--all --reflog
//! --indexed-objects --unpacked --incremental`** (incremental repack). **`repack -a` / `-A`**
//! runs a full repack into one pack (with optional **`--unpack-unreachable`**), same as `git gc`.

use crate::commands::update_server_info;
use crate::grit_exe;
use crate::trace2_emit_git_subcommand_argv;
use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use grit_lib::config::ConfigSet;
use grit_lib::objects::ObjectId;
use grit_lib::promisor::{promisor_pack_object_ids, repo_treats_promisor_packs};
use grit_lib::repo::Repository;
use std::collections::{HashMap, HashSet};
use std::fs;
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
    #[arg(short = 'b', long = "write-bitmap")]
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

    /// Extra arguments (ignored).
    #[arg(value_name = "ARG", num_args = 0.., allow_hyphen_values = true, trailing_var_arg = true)]
    pub rest: Vec<String>,
}

/// Run `grit repack`.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    if args.write_bitmap {
        let cfg = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
        let objects_dir = repo.git_dir.join("objects");
        if repo_treats_promisor_packs(&repo.git_dir, &cfg)
            && !promisor_pack_object_ids(&objects_dir).is_empty()
        {
            anyhow::bail!("fatal: failed to write bitmap index");
        }
    }
    if args.cruft && args.repack_all_unpack {
        anyhow::bail!("options '-A' and '--cruft' cannot be used together");
    }
    fn parse_byte_size_with_suffix(raw: &str) -> Option<u64> {
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
    let work_dir = repo.work_tree.as_deref().unwrap_or(&repo.git_dir);
    let grit_bin = grit_exe::grit_executable();

    let pack_base = if repo.work_tree.is_some() {
        ".git/objects/pack/pack"
    } else {
        "objects/pack/pack"
    };

    let pack_dir_abs = repo.git_dir.join("objects").join("pack");

    let full_repack = args.all || args.repack_all_unpack || args.cruft;
    let loosen_unreachable = args.repack_all_unpack && !args.cruft;

    let cfg = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();

    let mut new_pack_names: Vec<String> = Vec::new();

    let max_cruft_bytes = args
        .max_cruft_size
        .as_deref()
        .and_then(parse_byte_size_with_suffix);

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

            if args.quiet {
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

            if args.write_bitmap {
                cmd.arg("--write-bitmap-index");
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
                stdin_lines.push(format!("-{name}"));
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
    if args.write_bitmap {
        trace_argv.push("-b".to_string());
    }
    trace2_emit_git_subcommand_argv(&trace_argv);

    if args.delete_old {
        if full_repack {
            let mut keep: Vec<String> = new_pack_names.clone();
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
