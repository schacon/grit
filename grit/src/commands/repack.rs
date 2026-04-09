//! `grit repack` — pack unpacked objects or run geometric repacks.
//!
//! Non-`--geometric` mode runs [`pack_objects`](crate::commands::pack_objects) with `--all`.
//! `--geometric` mirrors `git repack --geometric`: `pack-objects --stdin-packs --unpacked`
//! over a computed pack split, optional promisor merge, MIDX, and redundant pack removal.

use crate::commands::update_server_info;
use crate::grit_exe;
use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use grit_lib::config::ConfigSet;
use grit_lib::midx::{write_multi_pack_index_with_options, WriteMultiPackIndexOptions};
use grit_lib::pack_geometry::{
    collect_geometry_packs, collect_promisor_geometry_packs, compute_geometry_split,
    preferred_pack_stem_after_split, GeometricPack,
};
use grit_lib::promisor::{promisor_pack_object_ids, repo_treats_promisor_packs};
use grit_lib::repo::Repository;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

/// Arguments for `grit repack`.
#[derive(Debug, ClapArgs)]
#[command(about = "Pack unpacked objects in a repository")]
pub struct Args {
    /// Remove redundant packs after repacking (keeps packs created by this run).
    #[arg(short = 'd')]
    pub delete_old: bool,

    /// Pass `--local` to pack-objects (local packs only for geometry collection).
    #[arg(short = 'l')]
    pub local: bool,

    /// Pack everything in all packs (incompatible with `--geometric`).
    #[arg(short = 'a')]
    pub all: bool,

    /// Write a bitmap index (same as `git repack -b`).
    #[arg(short = 'b', long = "write-bitmap")]
    pub write_bitmap: bool,

    #[arg(short = 'q', long = "quiet")]
    pub quiet: bool,

    /// Pass `--no-reuse-delta` (forwarded to pack-objects).
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

    #[arg(long = "keep-largest-pack")]
    pub keep_largest_pack: bool,

    /// Geometric repack factor (same as `git repack --geometric=<n>`).
    #[arg(short = 'g', long = "geometric")]
    pub geometric: Option<i32>,

    /// Write multi-pack-index after repack.
    #[arg(short = 'm', long = "write-midx")]
    pub write_midx: bool,

    /// Do not write bitmap index (forwarded to pack-objects / MIDX).
    #[arg(long = "no-write-bitmap-index")]
    pub no_write_bitmap_index: bool,

    /// Repack objects inside `.keep` packs (matches `git repack --pack-kept-objects`).
    #[arg(long = "pack-kept-objects")]
    pub pack_kept_objects: bool,

    /// Maximum pack size in bytes (forwarded to pack-objects).
    #[arg(long = "max-pack-size")]
    pub max_pack_size: Option<String>,

    /// Pack names to preserve unchanged (`--keep-pack=<name>`).
    #[arg(long = "keep-pack")]
    pub keep_pack: Vec<String>,

    /// Extra arguments (ignored).
    #[arg(value_name = "ARG", num_args = 0.., allow_hyphen_values = true, trailing_var_arg = true)]
    pub rest: Vec<String>,
}

/// Run `grit repack`.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let cfg = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();

    let pack_kept_objects = if args.pack_kept_objects {
        true
    } else {
        cfg.get("repack.packkeptobjects")
            .or_else(|| cfg.get("repack.packKeptObjects"))
            .map(|v| v == "true" || v == "1" || v.eq_ignore_ascii_case("yes"))
            .unwrap_or(false)
    };

    let geometric = args.geometric.unwrap_or(0).max(0);
    if geometric > 0 && args.all {
        anyhow::bail!("options '--geometric' and '-a' cannot be used together");
    }
    if geometric > 0 {
        return run_geometric(&repo, &args, pack_kept_objects, geometric);
    }

    if args.write_bitmap {
        let objects_dir = repo.git_dir.join("objects");
        if repo_treats_promisor_packs(&repo.git_dir, &cfg)
            && !promisor_pack_object_ids(&objects_dir).is_empty()
        {
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

    let mut cmd = Command::new(&grit_bin);
    cmd.current_dir(work_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .arg("pack-objects")
        .arg("--all")
        .arg(pack_base);
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
    if args.cruft {
        cmd.arg("--cruft");
    }
    if args.no_cruft {
        cmd.arg("--no-cruft");
    }
    if args.keep_largest_pack {
        cmd.arg("--keep-largest-pack");
    }
    if repo_treats_promisor_packs(&repo.git_dir, &cfg) {
        cmd.arg("--exclude-promisor-objects");
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
        .context("pack-objects did not print a pack hash on stdout")?;

    let new_pack_name = format!("pack-{hash}.pack");

    if args.delete_old {
        remove_superseded_packs(&pack_dir_abs, &new_pack_name)?;
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

    if args.local
        && grit_lib::pack::read_alternates_recursive(&objects_dir).map_or(false, |v| !v.is_empty())
    {
        let mut want_bitmap = args.write_bitmap;
        if !want_bitmap {
            if let Some(v) = cfg
                .get("repack.writebitmaps")
                .or_else(|| cfg.get("pack.writeBitmaps"))
            {
                want_bitmap = v == "true" || v == "1" || v.eq_ignore_ascii_case("yes");
            }
        }
        if want_bitmap && !args.no_write_bitmap_index {
            eprintln!("warning: disabling bitmap writing, as some objects are not being packed");
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
                let bitmap_placeholders = args.write_bitmap && !args.no_write_bitmap_index;
                write_multi_pack_index_with_options(
                    &pack_dir,
                    &WriteMultiPackIndexOptions {
                        preferred_pack_idx: pref_idx,
                        write_bitmap_placeholders: bitmap_placeholders,
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
        let bitmap = args.write_bitmap
            && !args.no_write_bitmap_index
            && !(args.local
                && grit_lib::pack::read_alternates_recursive(&objects_dir)
                    .map_or(false, |v| !v.is_empty()));
        write_multi_pack_index_with_options(
            &pack_dir,
            &WriteMultiPackIndexOptions {
                preferred_pack_idx: pref_idx,
                write_bitmap_placeholders: bitmap,
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
    if !pack_kept_from_config(args, cfg) {
        cmd.arg("--honor-pack-keep");
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

fn pack_kept_from_config(args: &Args, cfg: &ConfigSet) -> bool {
    if args.pack_kept_objects {
        return true;
    }
    cfg.get("repack.packkeptobjects")
        .or_else(|| cfg.get("repack.packKeptObjects"))
        .map(|v| v == "true" || v == "1" || v.eq_ignore_ascii_case("yes"))
        .unwrap_or(false)
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

fn basename_matches(keep: &str, stem: &str) -> bool {
    let p = Path::new(keep);
    let fname = p.file_name().and_then(|s| s.to_str()).unwrap_or(keep);
    let no_suf = fname.strip_suffix(".pack").unwrap_or(fname);
    no_suf == stem || fname == format!("{stem}.pack")
}

/// Deletes every `pack-*.pack` in `pack_dir` except the given basenames, unless a matching
/// `pack-*.keep` file exists for that pack. Used by `gc` when writing both a merged promisor pack
/// and a non-promisor pack in one pass.
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
        if keep_pack_names.iter().any(|k| k == &name) {
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
    }

    Ok(())
}

/// Deletes every `pack-*.pack` in `pack_dir` except `keep_pack_name`, unless a matching
/// `pack-*.keep` file exists for that pack.
fn remove_superseded_packs(pack_dir: &Path, keep_pack_name: &str) -> Result<()> {
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
        if name == keep_pack_name {
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
        let pack_path = pack_dir.join(&name);
        let idx_path = pack_dir.join(format!("{stem}.idx"));
        let _ = fs::remove_file(&pack_path);
        let _ = fs::remove_file(&idx_path);
    }

    Ok(())
}
