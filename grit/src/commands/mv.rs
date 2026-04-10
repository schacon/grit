//! `grit mv` — move or rename files in the index and working tree.
//!
//! Renames files (or directories) both on disk and in the index so the change
//! is automatically staged for the next commit.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::config::{ConfigFile, ConfigScope, ConfigSet};
use grit_lib::diff::worktree_differs_from_index_entry;
use grit_lib::error::Error;
use grit_lib::index::{Index, MODE_GITLINK};
use grit_lib::objects::ObjectKind;
use grit_lib::repo::Repository;
use grit_lib::sparse_checkout::{
    parse_sparse_checkout_file, path_in_cone_mode_sparse_checkout, path_in_sparse_checkout_patterns,
};
use grit_lib::unicode_normalization::precompose_utf8_path;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::grit_exe;

use crate::commands::sparse_advice::{emit_dirty_sparse_advice, emit_sparse_path_advice};

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
    let (mut raw_sources, mut raw_dest) = {
        let mut all = args.paths;
        let dest = all
            .pop()
            .ok_or_else(|| anyhow::anyhow!("usage: grit mv <source> ... <destination>"))?;
        (all, dest)
    };

    let repo = Repository::discover(None).context("not a git repository")?;
    if grit_lib::precompose_config::effective_core_precomposeunicode(Some(&repo.git_dir)) {
        for s in &mut raw_sources {
            *s = precompose_utf8_path(s).into_owned();
        }
        raw_dest = precompose_utf8_path(&raw_dest).into_owned();
    }
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
    let ignore_case = config
        .get_bool("core.ignorecase")
        .and_then(|r| r.ok())
        .unwrap_or(false);
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

    let precompose_unicode =
        grit_lib::precompose_config::effective_core_precomposeunicode(Some(&repo.git_dir));
    let mut sources: Vec<String> = raw_sources
        .iter()
        .map(|s| resolve_path(s, prefix.as_deref(), work_tree))
        .collect();
    if precompose_unicode {
        for s in &mut sources {
            *s = canonicalize_source_path_for_index(&index, s);
        }
    }

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
            && !path_in_sparse_checkout_patterns(&dest_with_slash, &sparse_patterns, cone_cfg)
            && empty_dir_has_sparse_contents(&dest_rel, &index)
            && sparse_enabled);

    // Git: `builtin/mv.c` sets `SKIP_WORKTREE_DIR` when the destination directory is
    // outside the sparse cone but the index still has skip-worktree entries under it
    // (typical after sparse-checkout removed those files from the worktree).
    if sparse_enabled
        && !dest_rel.is_empty()
        && !path_in_sparse_checkout_patterns(&dest_with_slash, &sparse_patterns, cone_cfg)
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

    if sources.len() == 1
        && !dest_is_dir
        && sparse_enabled
        && cone_cfg
        && !path_in_cone_mode_sparse_checkout(&dest_rel, &sparse_patterns, cone_cfg)
    {
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
        let key = precompose_utf8_path(&src_rel).into_owned();
        let mut src_abs = work_tree.join(&src_rel);
        if precompose_unicode && !src_abs.exists() {
            let nfc_path = work_tree.join(&key);
            if nfc_path.exists() {
                src_abs = nfc_path;
            } else if !src_rel.contains('/') {
                if let Ok(rd) = fs::read_dir(work_tree) {
                    for ent in rd.flatten() {
                        let name = ent.file_name().to_string_lossy().into_owned();
                        if precompose_utf8_path(&name).as_ref() == key.as_str() {
                            src_abs = ent.path();
                            break;
                        }
                    }
                }
            }
        }
        let index_src_rel = key;

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
            if index.get(index_src_rel.as_bytes(), 0).is_some() {
                vec![(index_src_rel.clone(), dst_rel.clone())]
            } else {
                expand_dir_sources(&index_src_rel, &dst_rel, &index)
            }
        } else if !src_abs.exists() && empty_dir_has_sparse_contents(&index_src_rel, &index) {
            expand_dir_sources(&index_src_rel, &dst_rel, &index)
        } else {
            vec![(index_src_rel.clone(), dst_rel.clone())]
        };

        if !args.sparse && sparse_enabled {
            let mut blocked = false;
            for (fsrc, fdst) in &sparse_path_pairs {
                if !path_in_sparse_checkout_patterns(fsrc, &sparse_patterns, cone_cfg) {
                    sparse_blocklist.push(fsrc.clone());
                    blocked = true;
                }
                if !path_in_sparse_checkout_patterns(fdst, &sparse_patterns, cone_cfg) {
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
                if index.get(index_src_rel.as_bytes(), 0).is_some() {
                    rows.push(MoveRow {
                        src: index_src_rel.clone(),
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
                moved_dir_roots.insert(index_src_rel.clone());
                rows.push(MoveRow {
                    src: index_src_rel.clone(),
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
            let pos = index.entries.iter().position(|e| {
                e.stage() == 0
                    && precompose_utf8_path(String::from_utf8_lossy(&e.path).as_ref()).as_ref()
                        == precompose_utf8_path(&index_src_rel).as_ref()
            });
            if pos.is_none()
                && !src_abs.exists()
                && empty_dir_has_sparse_contents(&index_src_rel, &index)
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
                moved_dir_roots.insert(index_src_rel.clone());
                rows.push(MoveRow {
                    src: index_src_rel.clone(),
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

        let has_conflict = index.entries.iter().any(|e| {
            e.stage() > 0
                && precompose_utf8_path(String::from_utf8_lossy(&e.path).as_ref()).as_ref()
                    == precompose_utf8_path(&index_src_rel).as_ref()
        });
        if has_conflict {
            let msg = format!("conflicted, source='{src_rel}', destination='{dst_rel}'");
            if args.skip_errors {
                continue;
            }
            bail!("{msg}");
        }

        let stage0 = index.get(index_src_rel.as_bytes(), 0);
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

        if index_src_rel == dst_rel {
            let msg = format!(
                "source and destination are the same, source='{src_rel}', destination='{dst_rel}'"
            );
            if args.skip_errors {
                continue;
            }
            bail!("{msg}");
        }

        // Git `builtin/mv.c`: on case-insensitive filesystems (`core.ignorecase`), a path that
        // only differs by case from the source is not a separate destination — `exists()` would
        // still be true for the same inode.
        let dst_fs_collides =
            dst_abs.exists() && !(ignore_case && index_src_rel.eq_ignore_ascii_case(&dst_rel));

        if dst_fs_collides
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
            src: index_src_rel,
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

        if !row.index_only {
            if let Some(e) = index.get(row.src.as_bytes(), 0) {
                if e.mode == MODE_GITLINK {
                    update_gitmodules_submodule_path(
                        &repo, work_tree, &mut index, &row.src, &row.dst,
                    )?;
                    rename_submodule_modules_dir(&repo.git_dir, &row.src, &row.dst)?;
                }
            }
        }

        let src_abs = work_tree.join(&row.src);
        let dst_abs = work_tree.join(&row.dst);
        let row_is_gitlink = index
            .get(row.src.as_bytes(), 0)
            .is_some_and(|e| e.mode == MODE_GITLINK);

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
            if row_is_gitlink {
                rewrite_submodule_worktree_gitfile(&repo.git_dir, work_tree, &row.dst)?;
            }
        }

        let Some(old_entry) = index.get(row.src.as_bytes(), 0).cloned() else {
            continue;
        };

        let mut sparse_and_dirty = false;
        if args.sparse && sparse_enabled && cone_cfg && !row.sparse_source && src_abs.exists() {
            sparse_and_dirty =
                worktree_differs_from_index_entry(&repo.odb, work_tree, &old_entry, false)?;
        }

        let new_path = row.dst.as_bytes().to_vec();
        let path_len = new_path.len().min(0x0FFF);
        let mut new_entry = old_entry;
        new_entry.flags = (new_entry.flags & !0x0FFF) | path_len as u16;
        new_entry.path = new_path;

        index.remove(row.src.as_bytes());
        index.add_or_replace(new_entry);

        if args.sparse && sparse_enabled && cone_cfg {
            let dst_in = path_in_sparse_checkout_patterns(&row.dst, &sparse_patterns, cone_cfg);
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

fn empty_dir_has_sparse_contents(name: &str, index: &Index) -> bool {
    let key = precompose_utf8_path(name.trim_end_matches('/'));
    let prefix_nfc = format!("{}/", key.as_ref());
    index.entries.iter().any(|e| {
        if e.stage() != 0 || !e.skip_worktree() {
            return false;
        }
        let p = String::from_utf8_lossy(&e.path);
        precompose_utf8_path(p.as_ref())
            .as_ref()
            .starts_with(prefix_nfc.as_str())
    })
}

/// When renaming a submodule (gitlink), update `submodule.*.path` in `.gitmodules`
/// and refresh the `.gitmodules` blob in the index.
/// When renaming a submodule directory, move `.git/modules/<old>` → `.git/modules/<new>` (Git
/// stores the object database under the path name).
fn rename_submodule_modules_dir(
    super_git_dir: &Path,
    old_path: &str,
    new_path: &str,
) -> Result<()> {
    let old_modules = super_git_dir.join("modules").join(old_path);
    if !old_modules.is_dir() {
        return Ok(());
    }
    let new_modules = super_git_dir.join("modules").join(new_path);
    if new_modules.exists() {
        bail!(
            "cannot move submodule gitdir: destination '{}' already exists",
            new_modules.display()
        );
    }
    fs::rename(&old_modules, &new_modules).with_context(|| {
        format!(
            "renaming submodule gitdir {} -> {}",
            old_modules.display(),
            new_modules.display()
        )
    })?;
    Ok(())
}

/// Point `<work_tree>/<sub_path>/.git` at `.git/modules/<sub_path>` using a relative path.
fn rewrite_submodule_worktree_gitfile(
    super_git_dir: &Path,
    work_tree: &Path,
    sub_path: &str,
) -> Result<()> {
    let sub_wt = work_tree.join(sub_path);
    let gitfile = sub_wt.join(".git");
    if !gitfile.is_file() {
        return Ok(());
    }
    let modules_dir = super_git_dir.join("modules").join(sub_path);
    if !modules_dir.is_dir() {
        return Ok(());
    }
    let rel = pathdiff_relative(&sub_wt, &modules_dir);
    fs::write(&gitfile, format!("gitdir: {rel}\n"))
        .with_context(|| format!("updating submodule gitfile at {}", gitfile.display()))?;
    refresh_submodule_core_worktree(super_git_dir, work_tree, sub_path)?;
    Ok(())
}

/// Update `core.worktree` in `.git/modules/<path>` after the submodule directory was renamed on disk.
fn refresh_submodule_core_worktree(
    super_git_dir: &Path,
    work_tree: &Path,
    sub_path: &str,
) -> Result<()> {
    let modules_dir = super_git_dir.join("modules").join(sub_path);
    let sub_wt = work_tree.join(sub_path);
    if !modules_dir.is_dir() || !sub_wt.join(".git").exists() {
        return Ok(());
    }
    let wt = pathdiff_relative(&modules_dir, &sub_wt);
    let grit_bin = grit_exe::grit_executable();
    let status = Command::new(&grit_bin)
        .arg("--git-dir")
        .arg(&modules_dir)
        .args(["config", "core.worktree"])
        .arg(&wt)
        .status()
        .with_context(|| format!("setting core.worktree for {}", modules_dir.display()))?;
    if !status.success() {
        bail!(
            "failed to set core.worktree in submodule gitdir {}",
            modules_dir.display()
        );
    }
    Ok(())
}

/// Relative path from directory `from` to path `to` (for gitfile `gitdir:` lines).
fn pathdiff_relative(from: &Path, to: &Path) -> String {
    let from_abs = from.canonicalize().unwrap_or_else(|_| from.to_path_buf());
    let to_abs = to.canonicalize().unwrap_or_else(|_| to.to_path_buf());

    let from_parts: Vec<_> = from_abs.components().collect();
    let to_parts: Vec<_> = to_abs.components().collect();

    let common = from_parts
        .iter()
        .zip(to_parts.iter())
        .take_while(|(a, b)| a == b)
        .count();

    let mut result = PathBuf::new();
    for _ in common..from_parts.len() {
        result.push("..");
    }
    for part in &to_parts[common..] {
        result.push(part);
    }

    result.to_string_lossy().into_owned()
}

fn update_gitmodules_submodule_path(
    repo: &Repository,
    work_tree: &Path,
    index: &mut Index,
    old_path: &str,
    new_path: &str,
) -> Result<()> {
    let path = work_tree.join(".gitmodules");
    if !path.is_file() {
        return Ok(());
    }
    let content =
        fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let mut config = ConfigFile::parse(&path, &content, ConfigScope::Local)
        .with_context(|| format!("parsing {}", path.display()))?;

    let mut matched = false;
    for entry in &config.entries.clone() {
        let key = &entry.key;
        let Some(rest) = key.strip_prefix("submodule.") else {
            continue;
        };
        let Some(name) = rest.strip_suffix(".path") else {
            continue;
        };
        let Some(val) = entry.value.as_deref() else {
            continue;
        };
        if val.trim() == old_path {
            config.set(&format!("submodule.{name}.path"), new_path)?;
            matched = true;
        }
    }

    if matched {
        config
            .write()
            .with_context(|| format!("writing {}", path.display()))?;
        refresh_index_gitmodules(repo, work_tree, index)?;
    }
    Ok(())
}

fn refresh_index_gitmodules(repo: &Repository, work_tree: &Path, index: &mut Index) -> Result<()> {
    let path = work_tree.join(".gitmodules");
    if !path.is_file() {
        return Ok(());
    }
    let data = fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
    let oid = repo
        .odb
        .write(ObjectKind::Blob, &data)
        .context("writing .gitmodules object")?;
    if let Some(mut entry) = index.get(b".gitmodules", 0).cloned() {
        entry.oid = oid;
        entry.size = data.len().try_into().unwrap_or(u32::MAX);
        index.remove(b".gitmodules");
        index.add_or_replace(entry);
    }
    Ok(())
}

/// Expand all index entries under `src_dir/` to their new paths under `dst_dir/`.
///
/// Returns a list of `(old_index_path, new_index_path)` pairs for every file
/// inside the directory.
fn expand_dir_sources(src_dir: &str, dst_dir: &str, index: &Index) -> Vec<(String, String)> {
    let src_key = precompose_utf8_path(src_dir.trim_end_matches('/'));
    let prefix_nfc = format!("{}/", src_key.as_ref());
    let dst_base = dst_dir.trim_end_matches('/');
    index
        .entries
        .iter()
        .filter(|e| e.stage() == 0)
        .filter_map(|e| {
            let p = String::from_utf8_lossy(&e.path).into_owned();
            let pn = precompose_utf8_path(&p);
            if pn.starts_with(prefix_nfc.as_str()) {
                let suffix = pn[prefix_nfc.len()..].to_string();
                let new_path = format!("{dst_base}/{suffix}");
                Some((p, new_path))
            } else {
                None
            }
        })
        .collect()
}

fn is_index_dir(path: &str, index: &Index) -> bool {
    let key = precompose_utf8_path(path.trim_end_matches('/'));
    let prefix_nfc = format!("{}/", key.as_ref());
    index.entries.iter().any(|e| {
        if e.stage() != 0 {
            return false;
        }
        let p = String::from_utf8_lossy(&e.path);
        precompose_utf8_path(p.as_ref())
            .as_ref()
            .starts_with(prefix_nfc.as_str())
    })
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

/// When `core.precomposeunicode` is on, the index stores NFC while argv or `resolve_path` may
/// yield a different UTF-8 spelling for the same logical path (t3910 `git mv`).
fn canonicalize_source_path_for_index(index: &Index, rel: &str) -> String {
    if index.get(rel.as_bytes(), 0).is_some() {
        return rel.to_owned();
    }
    let want = precompose_utf8_path(rel);
    for e in &index.entries {
        if e.stage() != 0 {
            continue;
        }
        let p = String::from_utf8_lossy(&e.path);
        if precompose_utf8_path(p.as_ref()).as_ref() == want.as_ref() {
            return p.into_owned();
        }
    }
    rel.to_owned()
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
