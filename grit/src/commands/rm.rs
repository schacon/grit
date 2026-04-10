//! `grit rm` — remove files from the index and working tree.
//!
//! Supports removing files from the index only (`--cached`), recursive
//! removal (`-r`), forced removal of modified files (`-f`/`--force`),
//! dry-run mode (`-n`/`--dry-run`), quiet mode (`-q`/`--quiet`), and
//! sparse-checkout awareness (`--sparse`).

use crate::commands::cwd_pathspec;
use crate::commands::sparse_advice::emit_sparse_path_advice;
use crate::commands::submodule::parse_gitmodules;
use crate::grit_exe;
use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::config::{ConfigFile, ConfigScope, ConfigSet};
use grit_lib::crlf;
use grit_lib::diff::{read_submodule_head_oid, submodule_embedded_git_dir, zero_oid};
use grit_lib::error::Error;
use grit_lib::ignore::path_in_sparse_checkout as path_in_sparse_checkout_lines;
use grit_lib::index::Index;
use grit_lib::objects::{parse_commit, parse_tree, ObjectKind};
use grit_lib::odb::Odb;
use grit_lib::repo::Repository;
use grit_lib::sparse_checkout::{parse_sparse_checkout_file, path_in_sparse_checkout_patterns};
use grit_lib::submodule_gitdir::submodule_modules_git_dir;
use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug)]
enum RmValidationErr {
    Die(String),
    Grouped(RmErrorKind),
}

/// The category of a safety-check failure.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum RmErrorKind {
    /// Index content differs from both the file and HEAD.
    StagedDiffersBoth,
    /// Index content differs from HEAD (staged changes).
    StagedInIndex,
    /// Working tree differs from index (local modifications).
    LocalModifications,
}

/// Arguments for `grit rm`.
#[derive(Debug, ClapArgs)]
#[command(about = "Remove files from the working tree and from the index")]
pub struct Args {
    /// Files to remove.
    pub pathspec: Vec<String>,

    /// Read pathspec from file (use "-" for stdin).
    #[arg(long = "pathspec-from-file", value_name = "FILE")]
    pub pathspec_from_file: Option<String>,

    /// NUL-terminated pathspec input (requires --pathspec-from-file).
    #[arg(long = "pathspec-file-nul")]
    pub pathspec_file_nul: bool,

    /// Only remove from the index; keep the working tree file.
    #[arg(long = "cached")]
    pub cached: bool,

    /// Override the up-to-date check; allow removing files with local changes.
    #[arg(short = 'f', long = "force")]
    pub force: bool,

    /// Allow recursive removal when a leading directory name is given.
    #[arg(short = 'r')]
    pub recursive: bool,

    /// Dry run — show what would be removed without doing it.
    #[arg(short = 'n', long = "dry-run")]
    pub dry_run: bool,

    /// Suppress the `rm 'file'` output message.
    #[arg(short = 'q', long = "quiet")]
    pub quiet: bool,

    /// Exit with zero status even if no files matched.
    #[arg(long = "ignore-unmatch")]
    pub ignore_unmatch: bool,

    /// Allow removing index entries outside the sparse-checkout cone (and skip-worktree entries).
    #[arg(long = "sparse")]
    pub sparse: bool,
}

/// Run the `rm` command.
pub fn run(mut args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;

    // Handle --pathspec-from-file / --pathspec-file-nul
    if args.pathspec_file_nul && args.pathspec_from_file.is_none() {
        eprintln!("fatal: the option '--pathspec-file-nul' requires '--pathspec-from-file'");
        std::process::exit(128);
    }
    if let Some(ref psf) = args.pathspec_from_file {
        if !args.pathspec.is_empty() {
            eprintln!(
                "fatal: '--pathspec-from-file' and pathspec arguments cannot be used together"
            );
            std::process::exit(128);
        }
        let content = if psf == "-" {
            use std::io::Read;
            let mut buf = String::new();
            std::io::stdin().read_to_string(&mut buf)?;
            buf
        } else {
            std::fs::read_to_string(psf)
                .with_context(|| format!("could not read pathspec from '{psf}'"))?
        };
        let paths: Vec<String> = if args.pathspec_file_nul {
            content
                .split('\0')
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect()
        } else {
            content
                .lines()
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect()
        };
        if paths.is_empty() {
            eprintln!("fatal: No pathspec was given. Which files should I remove?");
            std::process::exit(128);
        }
        args.pathspec = paths;
    }
    if args.pathspec.is_empty() {
        eprintln!("fatal: No pathspec was given. Which files should I remove?");
        std::process::exit(128);
    }

    // Exclude pathspec magic (`:^` / `:!`): include set defaults to "." when only
    // exclusions are given; matches are then filtered (see loop over `matches` below).
    let mut include_specs: Vec<String> = Vec::new();
    let mut exclude_specs: Vec<String> = Vec::new();
    for spec in &args.pathspec {
        if let Some(ex) = spec.strip_prefix(":^").or_else(|| spec.strip_prefix(":!")) {
            exclude_specs.push(ex.to_string());
        } else {
            include_specs.push(spec.clone());
        }
    }
    if include_specs.is_empty() && !exclude_specs.is_empty() {
        include_specs.push(".".to_string());
    }
    if include_specs.iter().any(|s| s.is_empty()) {
        eprintln!("fatal: empty string is not a valid pathspec. please use . instead if you meant to match all paths");
        std::process::exit(128);
    }

    let work_tree = repo
        .work_tree
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("this operation must be run in a work tree"))?;

    let config = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
    let show_hints = config
        .get_bool("advice.rmhints")
        .and_then(|r| r.ok())
        .unwrap_or(true);
    let sparse_enabled = config
        .get("core.sparseCheckout")
        .map(|v| v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    let cone_cfg = config
        .get("core.sparseCheckoutCone")
        .and_then(|v| v.parse::<bool>().ok())
        .unwrap_or(true);
    let sparse_patterns: Vec<String> = if sparse_enabled {
        let sc_path = repo.git_dir.join("info").join("sparse-checkout");
        match fs::read_to_string(&sc_path) {
            Ok(s) => parse_sparse_checkout_file(&s),
            Err(_) => Vec::new(),
        }
    } else {
        Vec::new()
    };

    let mut index = match repo.load_index() {
        Ok(idx) => idx,
        Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => Index::new(),
        Err(e) => return Err(e.into()),
    };

    // Build a map of path → HEAD OID for safety checks.
    let head_tree_map = build_head_map(&repo)?;

    // Phase 1: collect all index paths to remove and check safety.
    let mut to_remove: Vec<String> = Vec::new();
    // Collect errors grouped by kind so we can emit batched messages.
    let mut errors_by_kind: Vec<(RmErrorKind, Vec<String>)> = Vec::new();
    let mut sparse_only_pathspecs: Vec<String> = Vec::new();
    let mut matched_any_eligible = false;

    for pathspec in &include_specs {
        let rel = resolve_rel(pathspec, work_tree)?;

        // Refuse `rm` through a symlinked leading path (t3600 `d`→`e`). Exception: when the index
        // still records the path and the symlink parent is *dangling* (`d`→missing), removal
        // proceeds like Git (t3600 broken `d` + tracked `d/f`).
        let index_has_tracked_at_rel = index.entries.iter().any(|e| {
            if e.stage() != 0 {
                return false;
            }
            let p = String::from_utf8_lossy(&e.path);
            p == rel || p.starts_with(&format!("{rel}/"))
        });
        let parent_symlink_dangling = rm_parent_symlink_is_dangling(work_tree, &rel);
        if path_beyond_symlink_ancestor(work_tree, &rel)
            && !(index_has_tracked_at_rel && parent_symlink_dangling)
        {
            bail!("'{}' is beyond a symbolic link", rel);
        }

        // If pathspec has trailing slash, it must be a directory
        if pathspec.ends_with('/') {
            let abs_path = work_tree.join(&rel);
            // Check if it's a regular file (not a dir) — that should fail
            if abs_path.is_file() {
                bail!("not removing '{}' recursively without -r", pathspec);
            }
            // If it doesn't exist and nothing in index matches as dir prefix, fail
            let has_entries = index.entries.iter().any(|e| {
                let p = String::from_utf8_lossy(&e.path);
                p.starts_with(&format!("{rel}/"))
            });
            if !abs_path.is_dir() && !has_entries {
                if args.ignore_unmatch {
                    continue;
                }
                bail!("fatal: pathspec '{}' did not match any files", pathspec);
            }
        }

        // Collect matching index entries (by prefix for directories).
        let is_glob = has_glob_chars(&rel);
        let mut matches: Vec<String> = index
            .entries
            .iter()
            .filter(|e| {
                let p = String::from_utf8_lossy(&e.path);
                if rel.is_empty() {
                    // Empty rel means match everything (pathspec ".")
                    true
                } else if is_glob {
                    glob_pathspec_matches(&rel, &p)
                } else {
                    p == rel || p.starts_with(&format!("{rel}/"))
                }
            })
            .map(|e| String::from_utf8_lossy(&e.path).into_owned())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();

        if !exclude_specs.is_empty() {
            let mut resolved_excludes: Vec<String> = Vec::new();
            for ex in &exclude_specs {
                resolved_excludes.push(resolve_rel(ex, work_tree)?);
            }
            matches.retain(|p| !resolved_excludes.iter().any(|ex| pathspec_matches(ex, p)));
        }

        if matches.is_empty() {
            if args.ignore_unmatch {
                continue;
            }
            bail!("fatal: pathspec '{}' did not match any files", pathspec);
        }

        let eligible: Vec<String> = if args.sparse || !sparse_enabled {
            matches
        } else {
            matches
                .into_iter()
                .filter(|p| {
                    index.get(p.as_bytes(), 0).is_some_and(|e| {
                        rm_entry_matches_sparse_worktree(
                            e,
                            p,
                            &sparse_patterns,
                            cone_cfg,
                            Some(work_tree),
                        )
                    })
                })
                .collect()
        };

        if eligible.is_empty() {
            if args.ignore_unmatch {
                continue;
            }
            sparse_only_pathspecs.push(pathspec.clone());
            continue;
        }

        matched_any_eligible = true;

        // Require -r for directories (but not gitlinks, which are single entries).
        // Wildcard pathspecs may match several files at once without `-r` (Git: `ce_path_match`).
        if !args.recursive {
            // Check if this is a gitlink entry (mode 160000)
            let is_gitlink = eligible.len() == 1
                && eligible[0] == rel
                && index
                    .get(rel.as_bytes(), 0)
                    .map(|e| e.mode == 0o160000)
                    .unwrap_or(false);
            if !is_gitlink && !is_glob {
                for m in &eligible {
                    if Path::new(m) != Path::new(&rel) {
                        bail!("not removing '{}' recursively without -r", pathspec);
                    }
                }
                let abs_path = work_tree.join(&rel);
                let is_real_dir = fs::symlink_metadata(&abs_path)
                    .map(|m| m.file_type().is_dir())
                    .unwrap_or(false);
                if is_real_dir && !eligible.is_empty() {
                    bail!("not removing '{}' recursively without -r", pathspec);
                }
            }
        }

        for path_str in eligible {
            match validate_rm_entry(
                &repo,
                &index,
                &repo.odb,
                work_tree,
                &path_str,
                &head_tree_map,
                &args,
            ) {
                Ok(()) => to_remove.push(path_str),
                Err(RmValidationErr::Die(msg)) => bail!(msg),
                Err(RmValidationErr::Grouped(kind)) => {
                    if let Some(entry) = errors_by_kind.iter_mut().find(|(k, _)| *k == kind) {
                        entry.1.push(path_str);
                    } else {
                        errors_by_kind.push((kind, vec![path_str]));
                    }
                }
            }
        }
    }

    let mut exit_for_sparse_advice = false;
    if !sparse_only_pathspecs.is_empty() {
        sparse_only_pathspecs.sort();
        sparse_only_pathspecs.dedup();
        emit_sparse_path_advice(&mut std::io::stderr(), &config, &sparse_only_pathspecs)?;
        exit_for_sparse_advice = true;
    }

    if !matched_any_eligible && exit_for_sparse_advice {
        std::process::exit(1);
    }

    if !args.dry_run && !args.cached {
        absorb_submodule_gitdirs_for_rm(&repo, work_tree, &index, &to_remove, args.quiet)?;
    }

    if !args.cached {
        if let Err(msg) = assert_staging_gitmodules_ok(&repo, work_tree, &index, &to_remove) {
            bail!(msg);
        }
    }

    if !args.force && !args.cached {
        for path in &to_remove {
            let is_gitlink = index
                .entries
                .iter()
                .any(|e| e.path == path.as_bytes() && e.stage() == 0 && e.mode == 0o160000);
            if !is_gitlink {
                continue;
            }
            match bad_to_remove_submodule(work_tree, path, SubmoduleRmFlags::default()) {
                Ok(false) => {}
                Ok(true) => {
                    if let Some(entry) = errors_by_kind
                        .iter_mut()
                        .find(|(k, _)| *k == RmErrorKind::LocalModifications)
                    {
                        entry.1.push(path.clone());
                    } else {
                        errors_by_kind.push((RmErrorKind::LocalModifications, vec![path.clone()]));
                    }
                }
                Err(e) => return Err(e),
            }
        }
    }

    if !errors_by_kind.is_empty() {
        // Sort errors by kind priority to match git's output order:
        // StagedDiffersBoth first, then StagedInIndex, then LocalModifications.
        errors_by_kind.sort_by_key(|(kind, _)| match kind {
            RmErrorKind::StagedDiffersBoth => 0,
            RmErrorKind::StagedInIndex => 1,
            RmErrorKind::LocalModifications => 2,
        });
        for (kind, paths) in &mut errors_by_kind {
            paths.sort();
            let (header, hint) = error_message(kind, paths.len(), &args);
            eprintln!("error: {header}");
            for p in paths {
                eprintln!("    {p}");
            }
            if show_hints {
                if let Some(h) = hint {
                    eprintln!("{h}");
                }
            }
        }
        // Exit with non-zero status without printing an additional error
        // message — git rm does not print a summary line.
        std::process::exit(1);
    }

    // Phase 2: print lines, update index, then remove work tree (Git order).
    let mut gitmodules_modified = false;
    for path_str in &to_remove {
        if !args.quiet {
            rm_rm_line(path_str)?;
        }

        if args.dry_run {
            continue;
        }

        let was_gitlink = index
            .entries
            .iter()
            .any(|e| e.path == path_str.as_bytes() && e.mode == 0o160000);
        index.remove_path_all_stages(path_str.as_bytes());

        if args.cached {
            continue;
        }

        if !args.force
            && grit_lib::worktree_cwd::cwd_would_be_removed_with_repo_path(work_tree, path_str)
        {
            bail!("Refusing to remove the current working directory:\n{path_str}\n");
        }

        if was_gitlink {
            remove_submodule_worktree(&repo, work_tree, path_str, args.force)?;
            if remove_path_from_gitmodules_flow(work_tree, path_str)? {
                gitmodules_modified = true;
            }
        } else {
            remove_worktree_path_for_rm(work_tree, path_str)?;
        }
    }

    if gitmodules_modified {
        refresh_index_gitmodules_blob(&repo, work_tree, &mut index)?;
    }

    if !args.dry_run && !to_remove.is_empty() {
        repo.write_index(&mut index)?;
    }
    // Git keeps `submodule.<name>.*` entries in `.git/config` after `git rm` on a gitlink;
    // `git submodule deinit` / `git config --remove-section` clear them (t7400 cleanup).

    if exit_for_sparse_advice {
        std::process::exit(1);
    }

    Ok(())
}

/// Whether `git rm` may update this index entry without `--sparse` while sparse-checkout is on.
///
/// Matches Git's `builtin/rm.c`: entries with `skip-worktree` or outside the sparse definition are
/// skipped unless `--sparse` is given.
fn rm_entry_matches_sparse_worktree(
    entry: &grit_lib::index::IndexEntry,
    path: &str,
    patterns: &[String],
    cone_cfg: bool,
    work_tree: Option<&std::path::Path>,
) -> bool {
    if entry.skip_worktree() {
        return false;
    }
    let in_sparse = if patterns.is_empty() {
        true
    } else if cone_cfg {
        path_in_sparse_checkout_patterns(path, patterns, true)
    } else {
        path_in_sparse_checkout_lines(path, patterns, work_tree)
    };
    in_sparse
}

/// Generate error header and optional hint for a batch of failures.
fn error_message(kind: &RmErrorKind, count: usize, args: &Args) -> (String, Option<String>) {
    let plural = if count > 1 { "s have" } else { " has" };
    match kind {
        RmErrorKind::StagedDiffersBoth => {
            let header = format!(
                "the following file{plural} staged content different from both the\nfile and the HEAD:"
            );
            let hint = Some("(use -f to force removal)".to_owned());
            (header, hint)
        }
        RmErrorKind::StagedInIndex => {
            let header = format!("the following file{plural} changes staged in the index:");
            let hint = Some("(use --cached to keep the file, or -f to force removal)".to_owned());
            (header, hint)
        }
        RmErrorKind::LocalModifications => {
            let header = format!("the following file{plural} local modifications:");
            let hint = if args.cached {
                None
            } else {
                Some("(use --cached to keep the file, or -f to force removal)".to_owned())
            };
            (header, hint)
        }
    }
}

fn pick_rm_index_entry<'a>(
    index: &'a Index,
    path_str: &str,
) -> Option<&'a grit_lib::index::IndexEntry> {
    let p = path_str.as_bytes();
    if let Some(e) = index.get(p, 0) {
        return Some(e);
    }
    index
        .entries
        .iter()
        .find(|e| e.path == p && e.stage() == 2)
        .or_else(|| index.entries.iter().find(|e| e.path == p && e.stage() == 1))
        .or_else(|| index.entries.iter().find(|e| e.path == p))
}

/// `git rm` safety checks aligned with Git's `check_local_mod` in `builtin/rm.c`.
fn validate_rm_entry(
    repo: &Repository,
    index: &Index,
    odb: &grit_lib::odb::Odb,
    work_tree: &Path,
    path_str: &str,
    head_map: &HashMap<String, grit_lib::objects::ObjectId>,
    args: &Args,
) -> std::result::Result<(), RmValidationErr> {
    let Some(entry) = pick_rm_index_entry(index, path_str) else {
        return Ok(());
    };

    let abs_path = work_tree.join(path_str);

    // Unmerged submodule with an embedded `.git` directory (not a gitfile): Git refuses removal
    // even with `-f` (t3600-rm).
    if entry.mode == 0o160000
        && index
            .entries
            .iter()
            .any(|e| e.path == path_str.as_bytes() && e.stage() != 0)
        && !submodule_top_level_uses_gitfile(&abs_path)
    {
        return Err(RmValidationErr::Die(format!(
            "fatal: could not remove '{path_str}' (submodule git directory is not using a gitfile)"
        )));
    }

    if args.force {
        return Ok(());
    }

    // Unmerged, non-gitlink: Git skips `check_local_mod` for this path.
    if entry.stage() != 0 && entry.mode != 0o160000 {
        return Ok(());
    }

    let meta = match fs::symlink_metadata(&abs_path) {
        Ok(m) => Some(m),
        Err(e)
            if e.kind() == io::ErrorKind::NotFound || e.kind() == io::ErrorKind::NotADirectory =>
        {
            None
        }
        Err(e) => {
            return Err(RmValidationErr::Die(format!(
                "failed to stat '{path_str}': {e}"
            )));
        }
    };

    if let Some(m) = &meta {
        if m.file_type().is_dir() && entry.mode != 0o160000 {
            return Ok(());
        }
    }

    // Unmerged gitlink + empty work tree: safe (t3600 conflicted unpopulated submodule).
    if entry.stage() != 0 && entry.mode == 0o160000 && is_empty_dir_path(&abs_path) {
        return Ok(());
    }

    let index_oid = entry.oid;
    let is_intent_to_add = entry.intent_to_add() || index_oid == zero_oid();

    if is_intent_to_add {
        if !args.cached {
            return Err(RmValidationErr::Grouped(RmErrorKind::StagedInIndex));
        }
        return Ok(());
    }

    let head_oid = head_map.get(path_str);
    let staged_differs = match head_oid {
        None => true,
        Some(h) => h != &index_oid,
    };

    let worktree_differs = if entry.mode == 0o160000 {
        let populated = submodule_embedded_git_dir(&abs_path).is_some();
        let head_mismatch =
            populated && read_submodule_head_oid(&abs_path).as_ref() != Some(&index_oid);
        let bad = bad_to_remove_submodule(work_tree, path_str, SubmoduleRmFlags::default())
            .map_err(|e| RmValidationErr::Die(e.to_string()))?;
        head_mismatch || bad
    } else if meta.is_some() {
        worktree_differs_from_index(repo, odb, &abs_path, path_str, &index_oid).unwrap_or(false)
    } else {
        false
    };

    let file_exists = meta.is_some();

    if args.cached {
        if staged_differs && worktree_differs {
            return Err(RmValidationErr::Grouped(RmErrorKind::StagedDiffersBoth));
        }
    } else {
        if staged_differs && worktree_differs {
            return Err(RmValidationErr::Grouped(RmErrorKind::StagedDiffersBoth));
        }
        if staged_differs && file_exists {
            return Err(RmValidationErr::Grouped(RmErrorKind::StagedInIndex));
        }
        if worktree_differs {
            return Err(RmValidationErr::Grouped(RmErrorKind::LocalModifications));
        }
    }

    Ok(())
}

#[derive(Clone, Copy)]
struct SubmoduleRmFlags {
    ignore_untracked: bool,
    include_ignored: bool,
}

impl Default for SubmoduleRmFlags {
    fn default() -> Self {
        Self {
            ignore_untracked: false,
            include_ignored: false,
        }
    }
}

fn bad_to_remove_submodule(
    super_worktree: &Path,
    rel_path: &str,
    flags: SubmoduleRmFlags,
) -> Result<bool> {
    let abs = super_worktree.join(rel_path);
    if !abs.exists() || is_empty_dir_path(&abs) {
        return Ok(false);
    }
    if !submodule_top_level_uses_gitfile(&abs) {
        return Ok(true);
    }
    let grit = grit_exe::grit_executable();
    let mut cmd = Command::new(&grit);
    grit_exe::strip_trace2_env(&mut cmd);
    cmd.current_dir(&abs);
    cmd.arg("status");
    cmd.arg("--porcelain");
    cmd.arg("--ignore-submodules=none");
    if flags.ignore_untracked {
        cmd.arg("-uno");
    } else {
        cmd.arg("-uall");
    }
    if flags.include_ignored {
        cmd.arg("--ignored");
    }
    cmd.stdin(std::process::Stdio::null());
    let out = cmd.output().context("spawning grit status in submodule")?;
    if !out.status.success() {
        bail!("could not run 'grit status' in submodule '{rel_path}'");
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    Ok(submodule_porcelain_implies_dirty(&stdout))
}

/// Git's `bad_to_remove_submodule` treats any porcelain output beyond ~2 bytes as "dirty".
/// Grit may print a `##` branch header where Git does not; ignore those lines (t3600-rm).
fn submodule_porcelain_implies_dirty(stdout: &str) -> bool {
    stdout.lines().any(|l| {
        let l = l.trim_end();
        !l.is_empty() && !l.starts_with("## ")
    })
}

fn submodule_top_level_uses_gitfile(submodule_worktree: &Path) -> bool {
    let git_path = submodule_worktree.join(".git");
    if git_path.is_file() {
        return fs::read_to_string(&git_path)
            .ok()
            .is_some_and(|s| s.lines().any(|l| l.starts_with("gitdir:")));
    }
    false
}

fn is_empty_dir_path(path: &Path) -> bool {
    match fs::read_dir(path) {
        Ok(mut it) => it.next().is_none(),
        Err(e) if e.kind() == io::ErrorKind::NotFound => true,
        Err(_) => false,
    }
}

fn rm_rm_line(path_str: &str) -> Result<()> {
    let line = format!("rm '{path_str}'\n");
    let mut out = io::stdout().lock();
    out.write_all(line.as_bytes())?;
    Ok(())
}

fn path_beyond_symlink_ancestor(work_tree: &Path, rel: &str) -> bool {
    let rel_path = Path::new(rel);
    let mut accumulated = PathBuf::new();
    let components: Vec<_> = rel_path.components().collect();
    for component in components.iter().take(components.len().saturating_sub(1)) {
        accumulated.push(component);
        let abs = work_tree.join(&accumulated);
        if let Ok(meta) = fs::symlink_metadata(&abs) {
            if meta.file_type().is_symlink() {
                return true;
            }
        }
    }
    false
}

/// True when some parent of `rel` is a symlink whose target does not resolve (broken link).
fn rm_parent_symlink_is_dangling(work_tree: &Path, rel: &str) -> bool {
    let rel_path = Path::new(rel);
    let mut accumulated = PathBuf::new();
    let components: Vec<_> = rel_path.components().collect();
    for component in components.iter().take(components.len().saturating_sub(1)) {
        accumulated.push(component);
        let abs = work_tree.join(&accumulated);
        if let Ok(meta) = fs::symlink_metadata(&abs) {
            if meta.file_type().is_symlink() {
                let target = fs::read_link(&abs).unwrap_or_default();
                let joined = abs.parent().map(|p| p.join(&target)).unwrap_or(target);
                return fs::symlink_metadata(&joined).is_err();
            }
        }
    }
    false
}

fn absorb_submodule_gitdirs_for_rm(
    _repo: &Repository,
    work_tree: &Path,
    index: &Index,
    paths: &[String],
    _quiet: bool,
) -> Result<()> {
    let grit = grit_exe::grit_executable();
    for path_str in paths {
        let abs = work_tree.join(path_str);
        if !abs.is_dir() || is_empty_dir_path(&abs) {
            continue;
        }
        let is_gitlink = index
            .get(path_str.as_bytes(), 0)
            .is_some_and(|e| e.mode == 0o160000);
        if !is_gitlink {
            continue;
        }
        let dot_git = abs.join(".git");
        if !dot_git.is_dir() {
            continue;
        }
        let mut sub = Command::new(&grit);
        grit_exe::strip_trace2_env(&mut sub);
        sub.arg("submodule")
            .arg("absorbgitdirs")
            .arg("--")
            .arg(path_str)
            .current_dir(work_tree)
            .stdin(std::process::Stdio::null());
        let _ = sub.status();
    }
    Ok(())
}

fn assert_staging_gitmodules_ok(
    _repo: &Repository,
    work_tree: &Path,
    index: &Index,
    to_remove: &[String],
) -> std::result::Result<(), String> {
    let touches_gitlink = to_remove.iter().any(|p| {
        index
            .entries
            .iter()
            .any(|e| e.path == p.as_bytes() && e.mode == 0o160000)
    });
    if !touches_gitlink {
        return Ok(());
    }
    let gm = work_tree.join(".gitmodules");
    let Some(entry) = index.get(b".gitmodules", 0) else {
        return Ok(());
    };
    if !gm.exists() {
        return Ok(());
    }
    let Ok(data) = fs::read(&gm) else {
        return Ok(());
    };
    let disk_oid = Odb::hash_object_data(ObjectKind::Blob, &data);
    if disk_oid != entry.oid {
        return Err(
            "please stage your changes to .gitmodules or stash them to proceed".to_string(),
        );
    }
    Ok(())
}

fn remove_path_from_gitmodules_flow(work_tree: &Path, path_str: &str) -> Result<bool> {
    let gm_path = work_tree.join(".gitmodules");
    if !gm_path.exists() {
        return Ok(false);
    }
    let modules = parse_gitmodules(work_tree)?;
    let Some(m) = modules.iter().find(|m| m.path == path_str) else {
        eprintln!("warning: Could not find section in .gitmodules where path={path_str}");
        return Ok(false);
    };
    let content =
        fs::read_to_string(&gm_path).with_context(|| format!("reading {}", gm_path.display()))?;
    let mut cfg = ConfigFile::parse(&gm_path, &content, ConfigScope::Local)?;
    let section = format!("submodule.{}", m.name);
    let removed = cfg.remove_section(&section).unwrap_or(false);
    if !removed {
        eprintln!("warning: Could not remove .gitmodules entry for {path_str}");
        return Ok(false);
    }
    cfg.write()
        .with_context(|| format!("writing {}", gm_path.display()))?;
    Ok(true)
}

fn refresh_index_gitmodules_blob(
    repo: &Repository,
    work_tree: &Path,
    index: &mut Index,
) -> Result<()> {
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

fn remove_submodule_worktree(
    repo: &Repository,
    work_tree: &Path,
    rel: &str,
    force: bool,
) -> Result<()> {
    let abs = work_tree.join(rel);
    if abs.exists() || fs::symlink_metadata(&abs).is_ok() {
        if let Err(e) = fs::remove_dir_all(&abs) {
            if force {
                let _ = fs::remove_dir_all(&abs);
            } else {
                bail!("could not remove '{rel}': {e}");
            }
        }
    }
    let modules_gitdir = submodule_modules_git_dir(&repo.git_dir, rel);
    if modules_gitdir.exists() {
        let _ = fs::remove_dir_all(&modules_gitdir);
    }
    Ok(())
}

fn remove_worktree_path_for_rm(work_tree: &Path, path_str: &str) -> Result<()> {
    let abs_path = work_tree.join(path_str);
    if !(abs_path.exists() || fs::symlink_metadata(&abs_path).is_ok()) {
        return Ok(());
    }
    let is_real_dir = fs::symlink_metadata(&abs_path)
        .map(|m| m.file_type().is_dir())
        .unwrap_or(false);
    if is_real_dir {
        fs::remove_dir_all(&abs_path).with_context(|| format!("cannot remove '{path_str}'"))?;
    } else {
        match fs::remove_file(&abs_path) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e).with_context(|| format!("cannot remove '{path_str}'")),
        }
    }
    remove_empty_parents(&abs_path, work_tree);
    Ok(())
}

/// Returns `true` if the working tree file content differs from the index OID.
fn worktree_differs_from_index(
    repo: &Repository,
    odb: &grit_lib::odb::Odb,
    abs_path: &Path,
    rel_path: &str,
    index_oid: &grit_lib::objects::ObjectId,
) -> Result<bool> {
    let meta = fs::symlink_metadata(abs_path)?;
    let data = if meta.file_type().is_symlink() {
        let target = fs::read_link(abs_path)?;
        target.to_string_lossy().into_owned().into_bytes()
    } else {
        let raw = fs::read(abs_path)?;
        let config = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
        let conv = {
            let mut c = crlf::ConversionConfig::from_config(&config);
            c.safecrlf = crlf::SafeCrlf::False;
            c
        };
        let attrs = repo
            .work_tree
            .as_deref()
            .map(crlf::load_gitattributes)
            .unwrap_or_default();
        let file_attrs = crlf::get_file_attrs(&attrs, rel_path, false, &config);

        // Keep raw bytes for legacy CRLF blobs committed before autocrlf.
        let expected_has_crlf = odb
            .read(index_oid)
            .ok()
            .map(|obj| obj.kind == ObjectKind::Blob && crlf::has_crlf(&obj.data))
            .unwrap_or(false);
        if expected_has_crlf {
            raw
        } else {
            crlf::convert_to_git(&raw, rel_path, &conv, &file_attrs).unwrap_or(raw)
        }
    };

    let wt_oid = Odb::hash_object_data(ObjectKind::Blob, &data);
    Ok(wt_oid != *index_oid)
}

/// Build a map from repo-relative path string to HEAD tree OID.
fn build_head_map(repo: &Repository) -> Result<HashMap<String, grit_lib::objects::ObjectId>> {
    let head = grit_lib::state::resolve_head(&repo.git_dir)?;
    let commit_oid = match head.oid() {
        Some(o) => o,
        None => return Ok(HashMap::new()),
    };
    let commit_obj = repo.odb.read(commit_oid)?;
    let commit = parse_commit(&commit_obj.data)?;
    flatten_tree_to_map(&repo.odb, &commit.tree, "")
}

/// Recursively flatten a tree into a path→OID map.
fn flatten_tree_to_map(
    odb: &grit_lib::odb::Odb,
    tree_oid: &grit_lib::objects::ObjectId,
    prefix: &str,
) -> Result<HashMap<String, grit_lib::objects::ObjectId>> {
    let obj = odb.read(tree_oid)?;
    let entries = parse_tree(&obj.data)?;
    let mut map = HashMap::new();

    for entry in entries {
        let name = String::from_utf8_lossy(&entry.name);
        let path = if prefix.is_empty() {
            name.into_owned()
        } else {
            format!("{prefix}/{name}")
        };

        if entry.mode == 0o040000 {
            let nested = flatten_tree_to_map(odb, &entry.oid, &path)?;
            map.extend(nested);
        } else {
            map.insert(path, entry.oid);
        }
    }

    Ok(map)
}

/// Remove empty parent directories up to (but not including) the worktree root.
fn remove_empty_parents(file: &Path, work_tree: &Path) {
    let cwd_rel = grit_lib::worktree_cwd::process_cwd_repo_relative(work_tree);
    let mut current = file.parent();
    while let Some(dir) = current {
        if dir == work_tree {
            break;
        }
        if let Some(ref cr) = cwd_rel {
            if grit_lib::worktree_cwd::cwd_would_be_removed_with_dir(work_tree, dir, cr) {
                break;
            }
        }
        match fs::remove_dir(dir) {
            Ok(()) => current = dir.parent(),
            Err(_) => break,
        }
    }
}

/// Lexically normalize `.` / `..` components (no filesystem access).
fn lexical_normalize_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            std::path::Component::Normal(_)
            | std::path::Component::RootDir
            | std::path::Component::Prefix(_) => {
                out.push(Path::new(c.as_os_str()));
            }
        }
    }
    out
}

/// Resolve `pathspec` relative to `cwd` (handles `..` per Git pathspec rules).
fn lexical_resolve_under_cwd(pathspec: &str, cwd: &Path) -> PathBuf {
    let mut out = cwd.to_path_buf();
    for c in Path::new(pathspec).components() {
        match c {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            std::path::Component::Normal(_)
            | std::path::Component::RootDir
            | std::path::Component::Prefix(_) => {
                out.push(Path::new(c.as_os_str()));
            }
        }
    }
    out
}

/// Resolve a user-supplied pathspec to a worktree-relative path string.
///
/// Handles paths supplied from outside the worktree by stripping the
/// worktree prefix when present, and `..` relative to the current directory.
fn resolve_rel(pathspec: &str, work_tree: &Path) -> Result<String> {
    // Strip trailing slashes for matching purposes
    let pathspec_clean = pathspec.trim_end_matches('/');

    let wt_canon = work_tree
        .canonicalize()
        .unwrap_or_else(|_| work_tree.to_path_buf());

    let p = Path::new(pathspec_clean);
    if p.is_absolute() {
        // Resolve lexically first so a symlink as the final component is not followed:
        // `git rm foo` must remove path `foo`, not the symlink target (matches Git).
        let abs_lex = lexical_normalize_path(p);
        if let Ok(rel) = abs_lex.strip_prefix(&wt_canon) {
            let s = rel.to_string_lossy().into_owned();
            if s == "." || s.is_empty() {
                return Ok(String::new());
            }
            return Ok(s);
        }
        let abs = abs_lex.canonicalize().unwrap_or(abs_lex);
        let rel = abs
            .strip_prefix(&wt_canon)
            .map_err(|_| anyhow::anyhow!("path '{}' is outside the work tree", pathspec))?;
        return Ok(rel.to_string_lossy().into_owned());
    }

    let cwd = std::env::current_dir()?;
    let cwd_canon = cwd.canonicalize().unwrap_or(cwd);
    let abs = lexical_resolve_under_cwd(pathspec_clean, &cwd_canon);
    // Strip using lexical paths only — `canonicalize` follows symlinks and can
    // collapse a tracked symlink like `foo -> .` to the work tree root, which
    // would make `git rm foo` match every index entry (t6430 cherry-pick).
    let wt_norm = lexical_normalize_path(&wt_canon);
    let abs_norm = lexical_normalize_path(&abs);
    if let Ok(rel) = abs_norm.strip_prefix(&wt_norm) {
        let s = rel.to_string_lossy().into_owned();
        if s == "." || s.is_empty() {
            return Ok(String::new());
        }
        return Ok(s);
    }

    // Pathspec relative to worktree root (e.g. when cwd is not under the repo).
    let from_root = lexical_normalize_path(&wt_canon.join(pathspec_clean));
    if let Ok(rel) = from_root.strip_prefix(&wt_norm) {
        let s = rel.to_string_lossy().into_owned();
        if s == "." || s.is_empty() {
            return Ok(String::new());
        }
        return Ok(s);
    }

    if pathspec_clean == "." {
        return Ok(String::new());
    }
    if cwd_pathspec::has_parent_pathspec_component(pathspec_clean) {
        bail!("pathspec '{}' resolved outside the work tree", pathspec);
    }
    Ok(pathspec_clean.to_owned())
}

fn has_glob_chars(s: &str) -> bool {
    s.contains('*') || s.contains('?') || s.contains('[')
}

fn glob_matches(pattern: &str, path: &str) -> bool {
    glob_matches_inner(pattern.as_bytes(), path.as_bytes())
}

fn glob_matches_inner(pattern: &[u8], path: &[u8]) -> bool {
    let mut pi = 0;
    let mut si = 0;
    let mut star_pi = usize::MAX;
    let mut star_si = 0;

    while si < path.len() {
        if pi < pattern.len() && pattern[pi] == b'?' && path[si] != b'/' {
            pi += 1;
            si += 1;
        } else if pi < pattern.len() && pattern[pi] == b'*' {
            if pi + 1 < pattern.len() && pattern[pi + 1] == b'*' {
                let rest = &pattern[pi + 2..];
                let rest = if !rest.is_empty() && rest[0] == b'/' {
                    &rest[1..]
                } else {
                    rest
                };
                for i in si..=path.len() {
                    if glob_matches_inner(rest, &path[i..]) {
                        return true;
                    }
                }
                return false;
            }
            star_pi = pi;
            star_si = si;
            pi += 1;
        } else if pi < pattern.len() && pattern[pi] == b'[' {
            pi += 1;
            let negate = pi < pattern.len() && (pattern[pi] == b'!' || pattern[pi] == b'^');
            if negate {
                pi += 1;
            }
            let mut found = false;
            let ch = path[si];
            while pi < pattern.len() && pattern[pi] != b']' {
                if pi + 2 < pattern.len() && pattern[pi + 1] == b'-' {
                    if ch >= pattern[pi] && ch <= pattern[pi + 2] {
                        found = true;
                    }
                    pi += 3;
                } else {
                    if ch == pattern[pi] {
                        found = true;
                    }
                    pi += 1;
                }
            }
            if pi < pattern.len() {
                pi += 1;
            }
            if found == negate {
                if star_pi != usize::MAX && path[si] != b'/' {
                    pi = star_pi + 1;
                    star_si += 1;
                    si = star_si;
                } else {
                    return false;
                }
            } else {
                si += 1;
            }
        } else if pi < pattern.len() && pattern[pi] == path[si] {
            pi += 1;
            si += 1;
        } else if star_pi != usize::MAX && path[si] != b'/' {
            pi = star_pi + 1;
            star_si += 1;
            si = star_si;
        } else {
            return false;
        }
    }

    while pi < pattern.len() && pattern[pi] == b'*' {
        pi += 1;
    }
    pi == pattern.len()
}

fn glob_pathspec_matches(pattern: &str, path: &str) -> bool {
    if glob_matches(pattern, path) {
        return true;
    }
    // For directory-like pathspecs (e.g. "*" or "dir*"), Git also matches
    // top-level path components and then applies recursion with -r.
    if let Some((first, _)) = path.split_once('/') {
        glob_matches(pattern, first)
    } else {
        false
    }
}

fn pathspec_matches(spec: &str, path: &str) -> bool {
    if spec.is_empty() {
        return true;
    }
    if has_glob_chars(spec) {
        return glob_pathspec_matches(spec, path);
    }
    path == spec || path.starts_with(&format!("{spec}/"))
}
