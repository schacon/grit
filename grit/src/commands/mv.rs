//! `grit mv` — move or rename files in the index and working tree.
//!
//! Renames files (or directories) both on disk and in the index so the change
//! is automatically staged for the next commit.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::config::ConfigSet;
use grit_lib::diff::worktree_differs_from_index_entry;
use grit_lib::error::Error;
use grit_lib::index::Index;
use grit_lib::repo::Repository;
use grit_lib::sparse_checkout::{
    parse_sparse_checkout_file, path_in_cone_mode_sparse_checkout, path_in_sparse_checkout,
};
use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Arguments for `grit mv`.
#[derive(Debug, ClapArgs)]
#[command(
    about = "Move or rename a file, a directory, or a symlink",
    override_usage = "grit mv [-v] [-f] [-n] [-k] [--sparse] <source> <destination>\n       \
                      grit mv [-v] [-f] [-n] [-k] [--sparse] <source>... <destination-directory>"
)]
pub struct Args {
    /// Source(s) and destination — last element is always the destination.
    /// At least two values are required.
    #[arg(required = true, num_args = 2..)]
    pub paths: Vec<String>,

    /// Force move/rename even if target exists.
    #[arg(short = 'f', long = "force")]
    pub force: bool,

    /// Dry run — show what would be moved without doing it.
    #[arg(short = 'n', long = "dry-run")]
    pub dry_run: bool,

    /// Skip move/rename errors instead of aborting.
    #[arg(short = 'k')]
    pub skip_errors: bool,

    /// Allow updating index entries outside the sparse-checkout cone.
    #[arg(long = "sparse")]
    pub sparse: bool,

    /// Be verbose.
    #[arg(short = 'v', long = "verbose")]
    pub verbose: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DstSparseMode {
    Normal,
    /// Destination is a directory path outside sparse cone with only skip-worktree entries.
    SkipWorktreeDir,
    /// Single-file destination outside cone (cone mode only).
    SparseFile,
}

#[derive(Clone, Debug)]
struct MoveRow {
    src: String,
    dst: String,
    /// On-disk rename for this row (false when a parent directory move handles it).
    do_fs_rename: bool,
    /// Only update index (used for files under a renamed directory).
    index_only: bool,
    /// Source was skip-worktree (sparse) before the move.
    sparse_source: bool,
}

/// Run the `mv` command.
pub fn run(args: Args) -> Result<()> {
    let (raw_sources, raw_dest) = {
        let mut all = args.paths;
        let dest = all
            .pop()
            .ok_or_else(|| anyhow::anyhow!("usage: grit mv <source> ... <destination>"))?;
        (all, dest)
    };

    let repo = Repository::discover(None).context("not a git repository")?;
    let work_tree = repo
        .work_tree
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("this operation must be run in a work tree"))?;

    let mut index = match repo.load_index() {
        Ok(idx) => idx,
        Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => Index::new(),
        Err(e) => return Err(e.into()),
    };

    let config = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
    let sparse_enabled = config
        .get("core.sparseCheckout")
        .map(|v| v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    let cone_cfg = config
        .get("core.sparseCheckoutCone")
        .and_then(|v| v.parse::<bool>().ok())
        .unwrap_or(true);
    let sparse_patterns = if sparse_enabled {
        let sc_path = repo.git_dir.join("info").join("sparse-checkout");
        match fs::read_to_string(&sc_path) {
            Ok(s) => parse_sparse_checkout_file(&s),
            Err(_) => Vec::new(),
        }
    } else {
        Vec::new()
    };
    let cwd = std::env::current_dir()?;
    let prefix = compute_prefix(&cwd, work_tree);

    let sources: Vec<String> = raw_sources
        .iter()
        .map(|s| resolve_path(s, prefix.as_deref(), work_tree))
        .collect();

    for (raw, resolved) in raw_sources.iter().zip(sources.iter()) {
        if Path::new(resolved).is_absolute() {
            bail!("source '{}' is outside the work tree", raw);
        }
    }

    let dest_has_trailing_slash = raw_dest.ends_with('/') || raw_dest.ends_with('\\');
    let dest_trimmed = raw_dest.trim_end_matches('/').trim_end_matches('\\');
    let dest_rel = resolve_path(dest_trimmed, prefix.as_deref(), work_tree);

    if Path::new(&dest_rel).is_absolute() {
        bail!("destination '{}' is outside the work tree", raw_dest);
    }

    let dest_abs = work_tree.join(&dest_rel);

    let dest_with_slash = if dest_rel.is_empty() {
        String::new()
    } else {
        format!("{}/", dest_rel.trim_end_matches('/'))
    };

    let mut dst_mode = DstSparseMode::Normal;
    let dest_is_dir = dest_abs.is_dir()
        || dest_rel.is_empty()
        || is_index_dir(&dest_rel, &index)
        || (!dest_abs.exists()
            && !path_in_sparse_checkout(&dest_with_slash, &sparse_patterns, cone_cfg)
            && empty_dir_has_sparse_contents(&dest_rel, &index)
            && sparse_enabled);

    // Git: `builtin/mv.c` sets `SKIP_WORKTREE_DIR` when the destination directory is
    // outside the sparse cone but the index still has skip-worktree entries under it
    // (typical after sparse-checkout removed those files from the worktree).
    if sparse_enabled
        && !dest_rel.is_empty()
        && !path_in_sparse_checkout(&dest_with_slash, &sparse_patterns, cone_cfg)
        && empty_dir_has_sparse_contents(&dest_rel, &index)
    {
        dst_mode = DstSparseMode::SkipWorktreeDir;
    }

    if !dest_is_dir && sources.len() > 1 {
        bail!("destination '{}' is not a directory", dest_trimmed);
    }

    if dest_has_trailing_slash && !dest_abs.is_dir() && !dest_abs.exists() {
        let single_src_is_dir = sources.len() == 1 && {
            let sabs = work_tree.join(&sources[0]);
            sabs.is_dir() || is_index_dir(&sources[0], &index)
        };
        if !single_src_is_dir {
            bail!("destination directory '{}' does not exist", dest_trimmed);
        }
    }

    if sources.len() > 1 {
        for (i, src_a) in sources.iter().enumerate() {
            let src_a_clean = src_a.trim_end_matches('/').trim_end_matches('\\');
            let prefix_a = format!("{}/", src_a_clean);
            for (j, src_b) in sources.iter().enumerate() {
                if i == j {
                    continue;
                }
                let src_b_clean = src_b.trim_end_matches('/').trim_end_matches('\\');
                if src_b_clean.starts_with(&prefix_a) {
                    bail!(
                        "cannot move both '{}' and its parent directory '{}'",
                        src_b_clean,
                        src_a_clean
                    );
                }
            }
        }
    }

    if sources.len() == 1 && !dest_is_dir && sparse_enabled && cone_cfg
        && !path_in_cone_mode_sparse_checkout(&dest_rel, &sparse_patterns, cone_cfg) {
            dst_mode = DstSparseMode::SparseFile;
        }

    let mut rows: Vec<MoveRow> = Vec::new();
    let mut sparse_blocklist: Vec<String> = Vec::new();
    let mut moved_dir_roots: HashSet<String> = HashSet::new();

    for src_rel in &sources {
        let src_rel = src_rel
            .trim_end_matches('/')
            .trim_end_matches('\\')
            .to_owned();
        let src_abs = work_tree.join(&src_rel);

        let dst_rel: String = if dest_is_dir {
            let basename = Path::new(&src_rel)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| src_rel.clone());
            if dest_rel.is_empty() {
                basename
            } else {
                format!("{}/{}", dest_rel.trim_end_matches('/'), basename)
            }
        } else {
            dest_rel.clone()
        };
        let dst_abs = work_tree.join(&dst_rel);

        let sparse_path_pairs: Vec<(String, String)> = if src_abs.is_dir() {
            if index.get(src_rel.as_bytes(), 0).is_some() {
                vec![(src_rel.clone(), dst_rel.clone())]
            } else {
                expand_dir_sources(&src_rel, &dst_rel, &index)
            }
        } else if !src_abs.exists() && empty_dir_has_sparse_contents(&src_rel, &index) {
            expand_dir_sources(&src_rel, &dst_rel, &index)
        } else {
            vec![(src_rel.clone(), dst_rel.clone())]
        };

        if !args.sparse && sparse_enabled {
            let mut blocked = false;
            for (fsrc, fdst) in &sparse_path_pairs {
                if !path_in_sparse_checkout(fsrc, &sparse_patterns, cone_cfg) {
                    sparse_blocklist.push(fsrc.clone());
                    blocked = true;
                }
                if !path_in_sparse_checkout(fdst, &sparse_patterns, cone_cfg) {
                    sparse_blocklist.push(fdst.clone());
                    blocked = true;
                }
            }
            if blocked {
                continue;
            }
        }

        let mut sparse_source = false;

        if src_abs.exists() {
            if src_abs.is_dir() {
                if index.get(src_rel.as_bytes(), 0).is_some() {
                    rows.push(MoveRow {
                        src: src_rel.clone(),
                        dst: dst_rel.clone(),
                        do_fs_rename: true,
                        index_only: false,
                        sparse_source: false,
                    });
                    continue;
                }

                let expanded = sparse_path_pairs;
                if expanded.is_empty() {
                    let msg = format!("source directory is empty or not tracked: '{src_rel}'");
                    if args.skip_errors {
                        continue;
                    }
                    bail!("{msg}");
                }
                if dst_abs.is_dir() {
                    let msg = format!("destination already exists: '{dst_rel}'");
                    if args.skip_errors {
                        continue;
                    }
                    bail!("{msg}");
                }
                moved_dir_roots.insert(src_rel.clone());
                rows.push(MoveRow {
                    src: src_rel.clone(),
                    dst: dst_rel.clone(),
                    do_fs_rename: true,
                    index_only: false,
                    sparse_source: false,
                });
                for (fsrc, fdst) in expanded {
                    let ce = index.get(fsrc.as_bytes(), 0);
                    let sw = ce.is_some_and(|e| e.skip_worktree());
                    rows.push(MoveRow {
                        src: fsrc,
                        dst: fdst,
                        do_fs_rename: false,
                        index_only: true,
                        sparse_source: sw,
                    });
                }
                continue;
            }
        } else {
            let pos = index
                .entries
                .iter()
                .position(|e| e.path == src_rel.as_bytes());
            if pos.is_none() && !src_abs.exists() && empty_dir_has_sparse_contents(&src_rel, &index)
            {
                let expanded = sparse_path_pairs;
                if expanded.is_empty() {
                    let msg = format!("source directory is empty or not tracked: '{src_rel}'");
                    if args.skip_errors {
                        continue;
                    }
                    bail!("{msg}");
                }
                if dst_abs.is_dir() {
                    let msg = format!("destination already exists: '{dst_rel}'");
                    if args.skip_errors {
                        continue;
                    }
                    bail!("{msg}");
                }
                moved_dir_roots.insert(src_rel.clone());
                rows.push(MoveRow {
                    src: src_rel.clone(),
                    dst: dst_rel.clone(),
                    do_fs_rename: false,
                    index_only: false,
                    sparse_source: false,
                });
                for (fsrc, fdst) in expanded {
                    let ce = index.get(fsrc.as_bytes(), 0);
                    let sw = ce.is_some_and(|e| e.skip_worktree());
                    rows.push(MoveRow {
                        src: fsrc,
                        dst: fdst,
                        do_fs_rename: false,
                        index_only: true,
                        sparse_source: sw,
                    });
                }
                continue;
            }

            if let Some(p) = pos {
                let ce = &index.entries[p];
                if !ce.skip_worktree() {
                    let msg = format!(
                        "not under version control, source='{src_rel}', destination='{dst_rel}'"
                    );
                    if args.skip_errors {
                        continue;
                    }
                    bail!("{msg}");
                }
                if !args.sparse {
                    sparse_blocklist.push(src_rel.clone());
                    continue;
                }
                if index.get(dst_rel.as_bytes(), 0).is_none() {
                    sparse_source = true;
                } else if !args.force {
                    let msg =
                        format!("destination exists, source='{src_rel}', destination='{dst_rel}'");
                    if args.skip_errors {
                        continue;
                    }
                    bail!("{msg}");
                } else {
                    sparse_source = true;
                }
            } else {
                let msg = format!(
                    "not under version control, source='{src_rel}', destination='{dst_rel}'"
                );
                if args.skip_errors {
                    continue;
                }
                bail!("{msg}");
            }
        }

        let has_conflict = index
            .entries
            .iter()
            .any(|e| e.path == src_rel.as_bytes() && e.stage() > 0);
        if has_conflict {
            let msg = format!("conflicted, source='{src_rel}', destination='{dst_rel}'");
            if args.skip_errors {
                continue;
            }
            bail!("{msg}");
        }

        let stage0 = index.get(src_rel.as_bytes(), 0);
        if stage0.is_none() && !src_abs.is_dir() {
            let msg =
                format!("not under version control, source='{src_rel}', destination='{dst_rel}'");
            if args.skip_errors {
                continue;
            }
            bail!("{msg}");
        }

        if args.sparse
            && matches!(
                dst_mode,
                DstSparseMode::SkipWorktreeDir | DstSparseMode::SparseFile
            )
            && index.get(dst_rel.as_bytes(), 0).is_some()
            && !args.force
        {
            let msg = format!(
                "destination exists in the index, source='{src_rel}', destination='{dst_rel}'"
            );
            if args.skip_errors {
                continue;
            }
            bail!("{msg}");
        }

        if dst_abs.exists()
            && !(args.force && (dst_abs.is_file() || dst_abs.is_symlink()) && !dst_abs.is_dir())
        {
            if !args.force {
                let msg =
                    format!("destination exists, source='{src_rel}', destination='{dst_rel}'");
                if args.skip_errors {
                    continue;
                }
                bail!("{msg}");
            }
            if dst_abs.is_dir() {
                let msg = format!("Cannot overwrite, source='{src_rel}', destination='{dst_rel}'");
                if args.skip_errors {
                    continue;
                }
                bail!("{msg}");
            }
        }

        if dest_has_trailing_slash && !dest_abs.exists() && sources.len() == 1 {
            let msg = format!("destination directory does not exist: '{dest_trimmed}/'");
            if args.skip_errors {
                continue;
            }
            bail!("{msg}");
        }

        rows.push(MoveRow {
            src: src_rel,
            dst: dst_rel,
            do_fs_rename: true,
            index_only: false,
            sparse_source,
        });
    }

    sparse_blocklist.sort();
    sparse_blocklist.dedup();
    if !sparse_blocklist.is_empty() {
        emit_sparse_path_advice(&mut std::io::stderr(), &config, &sparse_blocklist)?;
        if !args.skip_errors {
            // Match Git: exit non-zero after advice with no extra `error:` line (tests compare stderr).
            std::process::exit(1);
        }
    }

    for row in &rows {
        let needle = row.src.trim_end_matches('/');
        if needle.is_empty() {
            continue;
        }
        for other in &rows {
            if other.src == row.src {
                continue;
            }
            let o = other.src.trim_end_matches('/');
            if o.starts_with(needle) && o.as_bytes().get(needle.len()) == Some(&b'/') {
                if moved_dir_roots.contains(needle) {
                    continue;
                }
                bail!(
                    "cannot move both '{}' and its parent directory '{}'",
                    other.src,
                    needle
                );
            }
        }
    }

    let mut dirty_advice: Vec<String> = Vec::new();

    for row in &rows {
        if args.verbose || args.dry_run {
            println!("Renaming {} to {}", row.src, row.dst);
        }
        if args.dry_run {
            continue;
        }

        let src_abs = work_tree.join(&row.src);
        let dst_abs = work_tree.join(&row.dst);

        if row.do_fs_rename
            && !row.index_only
            && !matches!(
                dst_mode,
                DstSparseMode::SkipWorktreeDir | DstSparseMode::SparseFile
            )
        {
            if let Some(parent) = dst_abs.parent() {
                if !parent.exists() {
                    fs::create_dir_all(parent)?;
                }
            }
            if src_abs.exists() {
                fs::rename(&src_abs, &dst_abs)
                    .with_context(|| format!("renaming '{}' failed", row.src))?;
            }
        }

        let Some(old_entry) = index.get(row.src.as_bytes(), 0).cloned() else {
            continue;
        };

        let mut sparse_and_dirty = false;
        if args.sparse && sparse_enabled && cone_cfg && !row.sparse_source && src_abs.exists() {
            sparse_and_dirty = worktree_differs_from_index_entry(&repo.odb, work_tree, &old_entry)?;
        }

        let new_path = row.dst.as_bytes().to_vec();
        let path_len = new_path.len().min(0x0FFF);
        let mut new_entry = old_entry;
        new_entry.flags = (new_entry.flags & !0x0FFF) | path_len as u16;
        new_entry.path = new_path;

        index.remove(row.src.as_bytes());
        index.add_or_replace(new_entry);

        if args.sparse && sparse_enabled && cone_cfg {
            let dst_in = path_in_sparse_checkout(&row.dst, &sparse_patterns, cone_cfg);
            if row.sparse_source && dst_in {
                let dst_pos = index
                    .entries
                    .iter()
                    .position(|e| e.path == row.dst.as_bytes() && e.stage() == 0);
                if let Some(p) = dst_pos {
                    index.entries[p].set_skip_worktree(false);
                }
                if dst_abs.parent().is_some_and(|p| !p.exists()) {
                    fs::create_dir_all(dst_abs.parent().unwrap())?;
                }
                if let Some(ent) = index.get(row.dst.as_bytes(), 0).cloned() {
                    let data = repo.odb.read(&ent.oid)?.data;
                    fs::write(&dst_abs, data)?;
                }
            } else if matches!(
                dst_mode,
                DstSparseMode::SkipWorktreeDir | DstSparseMode::SparseFile
            ) && !row.sparse_source
                && !dst_in
            {
                let dst_pos = index
                    .entries
                    .iter()
                    .position(|e| e.path == row.dst.as_bytes() && e.stage() == 0);
                if let Some(p) = dst_pos {
                    if !sparse_and_dirty {
                        index.entries[p].set_skip_worktree(true);
                        let _ = fs::remove_file(&src_abs);
                    } else {
                        if let Some(parent) = dst_abs.parent() {
                            fs::create_dir_all(parent)?;
                        }
                        if src_abs.exists() {
                            fs::rename(&src_abs, &dst_abs)
                                .with_context(|| format!("renaming '{}' failed", row.src))?;
                        }
                        dirty_advice.push(row.dst.clone());
                    }
                }
            }
        }
    }

    dirty_advice.sort();
    dirty_advice.dedup();
    if !dirty_advice.is_empty() {
        emit_dirty_sparse_advice(&mut std::io::stderr(), &config, &dirty_advice)?;
    }

    if !args.dry_run {
        repo.write_index(&mut index)?;
    }

    Ok(())
}

fn emit_sparse_path_advice(w: &mut impl Write, config: &ConfigSet, paths: &[String]) -> Result<()> {
    if paths.is_empty() {
        return Ok(());
    }
    if !advice_update_sparse_path_enabled(config) {
        return Ok(());
    }
    writeln!(
        w,
        "The following paths and/or pathspecs matched paths that exist\n\
outside of your sparse-checkout definition, so will not be\n\
updated in the index:"
    )?;
    for p in paths {
        writeln!(w, "{p}")?;
    }
    writeln!(
        w,
        "hint: If you intend to update such entries, try one of the following:\n\
hint: * Use the --sparse option.\n\
hint: * Disable or modify the sparsity rules.\n\
hint: Disable this message with \"git config set advice.updateSparsePath false\""
    )?;
    Ok(())
}

fn emit_dirty_sparse_advice(
    w: &mut impl Write,
    config: &ConfigSet,
    paths: &[String],
) -> Result<()> {
    if paths.is_empty() {
        return Ok(());
    }
    if !advice_update_sparse_path_enabled(config) {
        return Ok(());
    }
    writeln!(
        w,
        "The following paths have been moved outside the\n\
sparse-checkout definition but are not sparse due to local\n\
modifications."
    )?;
    for p in paths {
        writeln!(w, "{p}")?;
    }
    writeln!(
        w,
        "hint: To correct the sparsity of these paths, do the following:\n\
hint: * Use \"git add --sparse <paths>\" to update the index\n\
hint: * Use \"git sparse-checkout reapply\" to apply the sparsity rules\n\
hint: Disable this message with \"git config set advice.updateSparsePath false\""
    )?;
    Ok(())
}

fn advice_update_sparse_path_enabled(config: &ConfigSet) -> bool {
    if let Ok(v) = std::env::var("GIT_ADVICE") {
        if v == "0" || v.eq_ignore_ascii_case("false") {
            return false;
        }
        if v == "1" || v.eq_ignore_ascii_case("true") {
            return true;
        }
    }
    config
        .get_bool("advice.updateSparsePath")
        .and_then(|r| r.ok())
        .unwrap_or(true)
}

fn empty_dir_has_sparse_contents(name: &str, index: &Index) -> bool {
    let with_slash = format!("{}/", name.trim_end_matches('/'));
    let prefix = with_slash.as_bytes();
    index
        .entries
        .iter()
        .any(|e| e.path.starts_with(prefix) && e.stage() == 0 && e.skip_worktree())
}

fn expand_dir_sources(src_dir: &str, dst_dir: &str, index: &Index) -> Vec<(String, String)> {
    let prefix = format!("{}/", src_dir);
    index
        .entries
        .iter()
        .filter(|e| {
            let p = String::from_utf8_lossy(&e.path);
            p.starts_with(&prefix)
        })
        .map(|e| {
            let p = String::from_utf8_lossy(&e.path).to_string();
            let suffix = &p[prefix.len()..];
            let new_path = format!("{}/{}", dst_dir, suffix);
            (p, new_path)
        })
        .collect()
}

fn is_index_dir(path: &str, index: &Index) -> bool {
    let prefix = format!("{}/", path);
    index
        .entries
        .iter()
        .any(|e| String::from_utf8_lossy(&e.path).starts_with(&prefix))
}

fn compute_prefix(cwd: &Path, work_tree: &Path) -> Option<String> {
    let cwd_c = cwd.canonicalize().ok()?;
    let wt_c = work_tree.canonicalize().ok()?;
    if cwd_c == wt_c {
        return None;
    }
    cwd_c
        .strip_prefix(&wt_c)
        .ok()
        .map(|p| p.to_string_lossy().to_string())
}

fn resolve_path(path: &str, prefix: Option<&str>, work_tree: &Path) -> String {
    let p = Path::new(path);

    if p.is_absolute() {
        let wt_canon = work_tree
            .canonicalize()
            .unwrap_or_else(|_| work_tree.to_path_buf());
        if let Ok(rel) = p.strip_prefix(&wt_canon) {
            return normalise_path(&rel.to_string_lossy());
        }
        if let Ok(rel) = p.strip_prefix(work_tree) {
            return normalise_path(&rel.to_string_lossy());
        }
        return path.to_owned();
    }

    match prefix {
        Some(pfx) if !pfx.is_empty() => {
            let combined = PathBuf::from(pfx).join(path);
            normalise_path(&combined.to_string_lossy())
        }
        _ => normalise_path(path),
    }
}

fn normalise_path(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for component in path.split('/') {
        match component {
            "." | "" => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}
