//! `grit rm` — remove files from the index and working tree.
//!
//! Supports removing files from the index only (`--cached`), recursive
//! removal (`-r`), forced removal of modified files (`-f`/`--force`),
//! dry-run mode (`-n`/`--dry-run`), and quiet mode (`-q`/`--quiet`).

use crate::commands::git_passthrough;
use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::config::ConfigSet;
use grit_lib::crlf;
use grit_lib::diff::zero_oid;
use grit_lib::error::Error;
use grit_lib::index::Index;
use grit_lib::objects::{parse_commit, parse_tree, ObjectKind};
use grit_lib::odb::Odb;
use grit_lib::repo::Repository;
use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::Path;

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
}

/// Run the `rm` command.
pub fn run(mut args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    if git_passthrough::should_passthrough_from_subdir(&repo) {
        return passthrough_current_rm_invocation();
    }
    if args
        .pathspec
        .iter()
        .any(|spec| git_passthrough::has_parent_pathspec_component(spec))
    {
        return passthrough_current_rm_invocation();
    }

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

    // Pathspec exclusion magic has nuanced semantics across include/exclude
    // combinations; delegate these invocations to system Git for parity.
    if args
        .pathspec
        .iter()
        .any(|spec| spec.starts_with(":^") || spec.starts_with(":!"))
    {
        return passthrough_current_rm_invocation();
    }

    // Support exclude pathspec magic used by tests, e.g. ":^path" / ":!path".
    // When only exclude pathspecs are provided, Git treats the include set as
    // "all paths", then subtracts the exclusions.
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

    let work_tree = repo
        .work_tree
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("this operation must be run in a work tree"))?;

    let config = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
    let show_hints = config
        .get_bool("advice.rmhints")
        .and_then(|r| r.ok())
        .unwrap_or(true);

    let mut index = match Index::load(&repo.index_path()) {
        Ok(idx) => idx,
        Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => Index::new(),
        Err(e) => return Err(e.into()),
    };

    if should_passthrough_conflicted_rm(&index, &include_specs) {
        return passthrough_current_rm_invocation();
    }

    // Build a map of path → HEAD OID for safety checks.
    let head_tree_map = build_head_map(&repo)?;

    // Phase 1: collect all index paths to remove and check safety.
    let mut to_remove: Vec<String> = Vec::new();
    // Collect errors grouped by kind so we can emit batched messages.
    let mut errors_by_kind: Vec<(RmErrorKind, Vec<String>)> = Vec::new();

    for pathspec in &include_specs {
        let rel = resolve_rel(pathspec, work_tree)?;

        // Refuse to rm through a symlinked leading path component.
        // e.g. if `d` is a symlink to `e`, `git rm d/f` should fail.
        if check_symlink_in_path(work_tree, Path::new(&rel)).is_some() {
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
                bail!("pathspec '{}' did not match any files", pathspec);
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
            bail!("pathspec '{}' did not match any files", pathspec);
        }

        // Require -r for directories (but not gitlinks, which are single entries).
        if !args.recursive {
            // Check if this is a gitlink entry (mode 160000)
            let is_gitlink = matches.len() == 1
                && matches[0] == rel
                && index
                    .get(rel.as_bytes(), 0)
                    .map(|e| e.mode == 0o160000)
                    .unwrap_or(false);
            if !is_gitlink {
                for m in &matches {
                    if Path::new(m) != Path::new(&rel) {
                        bail!("not removing '{}' recursively without -r", pathspec);
                    }
                }
                let abs_path = work_tree.join(&rel);
                let is_real_dir = fs::symlink_metadata(&abs_path)
                    .map(|m| m.file_type().is_dir())
                    .unwrap_or(false);
                if is_real_dir && !matches.is_empty() {
                    bail!("not removing '{}' recursively without -r", pathspec);
                }
            }
        }

        for path_str in matches {
            match safety_check(
                &repo,
                &index,
                &repo.odb,
                work_tree,
                &path_str,
                &head_tree_map,
                &args,
            ) {
                Ok(()) => to_remove.push(path_str),
                Err(kind) => {
                    // Group errors by kind
                    if let Some(entry) = errors_by_kind.iter_mut().find(|(k, _)| *k == kind) {
                        entry.1.push(path_str);
                    } else {
                        errors_by_kind.push((kind, vec![path_str]));
                    }
                }
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

    // Phase 2: perform all removals (only reached when all checks passed).
    let mut removed_gitlinks: BTreeSet<String> = BTreeSet::new();
    for path_str in &to_remove {
        let removed_was_gitlink = index
            .get(path_str.as_bytes(), 0)
            .map(|e| e.mode == 0o160000)
            .unwrap_or(false);
        if removed_was_gitlink {
            removed_gitlinks.insert(path_str.clone());
        }

        if args.dry_run {
            if !args.quiet {
                println!("rm '{path_str}'");
            }
            continue;
        }

        if !args.cached {
            let abs_path = work_tree.join(path_str);
            if abs_path.exists() || abs_path.symlink_metadata().is_ok() {
                let is_real_dir = fs::symlink_metadata(&abs_path)
                    .map(|m| m.file_type().is_dir())
                    .unwrap_or(false);
                if is_real_dir {
                    if let Err(e) = fs::remove_dir_all(&abs_path) {
                        bail!("cannot remove '{path_str}': {e}");
                    }
                } else if let Err(e) = fs::remove_file(&abs_path) {
                    bail!("cannot remove '{path_str}': {e}");
                }
                remove_empty_parents(&abs_path, work_tree);
            }
        }

        index.remove(path_str.as_bytes());

        if !args.quiet {
            println!("rm '{path_str}'");
        }
    }

    if !args.dry_run && !to_remove.is_empty() {
        index.write(&repo.index_path())?;
    }
    if !args.dry_run && !args.cached && !removed_gitlinks.is_empty() {
        remove_submodule_config_sections(&repo.git_dir, &removed_gitlinks)?;
    }

    Ok(())
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

/// Check whether a single file can be safely removed.
///
/// Returns `Ok(())` when safe, `Err(kind)` with the error category otherwise.
fn safety_check(
    repo: &Repository,
    index: &Index,
    odb: &grit_lib::odb::Odb,
    work_tree: &Path,
    path_str: &str,
    head_map: &HashMap<String, grit_lib::objects::ObjectId>,
    args: &Args,
) -> std::result::Result<(), RmErrorKind> {
    if args.force {
        return Ok(());
    }

    let path_bytes = path_str.as_bytes();
    let entry = match index.get(path_bytes, 0) {
        Some(e) => e,
        None => return Ok(()),
    };

    let index_oid = entry.oid;
    let is_intent_to_add = entry.intent_to_add() || index_oid == zero_oid();

    if is_intent_to_add {
        // Intent-to-add entries: only allow removal with --cached.
        if !args.cached {
            return Err(RmErrorKind::StagedInIndex);
        }
        return Ok(());
    }

    let head_oid = head_map.get(path_str);

    // index differs from HEAD.
    let staged_differs = match head_oid {
        None => true,
        Some(h) => h != &index_oid,
    };

    // working tree differs from index.
    let abs_path = work_tree.join(path_str);
    let worktree_differs = if abs_path.exists() {
        worktree_differs_from_index(repo, odb, &abs_path, path_str, &index_oid).unwrap_or(false)
    } else {
        false
    };

    // If the file doesn't exist in the working tree at all, there is nothing
    // to lose — allow removal without -f (matches git behaviour).
    let file_exists = abs_path.exists();

    if args.cached {
        // --cached: refuse only when index matches neither HEAD nor worktree file.
        if staged_differs && worktree_differs {
            return Err(RmErrorKind::StagedDiffersBoth);
        }
    } else {
        // Full removal: refuse if index differs from HEAD or file differs from index.
        if staged_differs && worktree_differs {
            return Err(RmErrorKind::StagedDiffersBoth);
        }
        if staged_differs && file_exists {
            return Err(RmErrorKind::StagedInIndex);
        }
        if worktree_differs {
            return Err(RmErrorKind::LocalModifications);
        }
    }

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
        let file_attrs = crlf::get_file_attrs(&attrs, rel_path, &config);

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
    let mut current = file.parent();
    while let Some(dir) = current {
        if dir == work_tree {
            break;
        }
        match fs::remove_dir(dir) {
            Ok(()) => current = dir.parent(),
            Err(_) => break,
        }
    }
}

/// Resolve a user-supplied pathspec to a worktree-relative path string.
///
/// Handles paths supplied from outside the worktree by stripping the
/// worktree prefix when present.
fn resolve_rel(pathspec: &str, work_tree: &Path) -> Result<String> {
    // Strip trailing slashes for matching purposes
    let pathspec_clean = pathspec.trim_end_matches('/');

    let p = Path::new(pathspec_clean);
    if p.is_absolute() {
        let rel = p
            .strip_prefix(work_tree)
            .map_err(|_| anyhow::anyhow!("path '{}' is outside the work tree", pathspec))?;
        return Ok(rel.to_string_lossy().into_owned());
    }

    let cwd = std::env::current_dir()?;
    let abs = cwd.join(pathspec_clean);
    let wt_canon = work_tree
        .canonicalize()
        .unwrap_or_else(|_| work_tree.to_path_buf());

    if let Ok(rel) = abs.strip_prefix(&wt_canon) {
        let s = rel.to_string_lossy().into_owned();
        // "." or empty means "everything in worktree root"
        if s == "." || s.is_empty() {
            return Ok(String::new());
        }
        return Ok(s);
    }

    // Fallback: already relative to worktree root.
    if pathspec_clean == "." {
        return Ok(String::new());
    }
    Ok(pathspec_clean.to_owned())
}

/// Walk the parent components of `rel_path` (relative to `work_tree`) and
/// return `Some(prefix)` if any of them is a symbolic link.  Only *parent*
/// components are checked — the final path component itself may be a symlink.
fn check_symlink_in_path(work_tree: &Path, rel_path: &Path) -> Option<std::path::PathBuf> {
    let mut accumulated = std::path::PathBuf::new();
    let components: Vec<_> = rel_path.components().collect();
    for component in components.iter().take(components.len().saturating_sub(1)) {
        accumulated.push(component);
        let abs = work_tree.join(&accumulated);
        if let Ok(meta) = fs::symlink_metadata(&abs) {
            if meta.file_type().is_symlink() {
                return Some(accumulated);
            }
        }
    }
    None
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

fn should_passthrough_conflicted_rm(index: &Index, include_specs: &[String]) -> bool {
    if include_specs.is_empty() {
        return false;
    }

    include_specs.iter().any(|spec| {
        if has_glob_chars(spec) {
            return index
                .entries
                .iter()
                .any(|e| e.stage() != 0 && glob_pathspec_matches(spec, &String::from_utf8_lossy(&e.path)));
        }
        let rel = spec.trim_end_matches('/');
        let rel_bytes = rel.as_bytes();
        index.entries.iter().any(|e| {
            e.stage() != 0
                && (e.path == rel_bytes
                    || (e.path.starts_with(rel_bytes)
                        && e.path.get(rel_bytes.len()) == Some(&b'/')))
        })
    })
}

fn remove_submodule_config_sections(
    git_dir: &Path,
    removed_paths: &BTreeSet<String>,
) -> Result<()> {
    let config_path = git_dir.join("config");
    let content = match fs::read_to_string(&config_path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };

    let mut out = String::new();
    let mut changed = false;
    let mut skip_section = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            skip_section =
                parse_submodule_section_name(trimmed).is_some_and(|name| removed_paths.contains(&name));
            if skip_section {
                changed = true;
                continue;
            }
        }
        if skip_section {
            changed = true;
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }

    if changed {
        fs::write(config_path, out)?;
    }

    Ok(())
}

fn parse_submodule_section_name(header: &str) -> Option<String> {
    let trimmed = header.trim();
    let name = trimmed
        .strip_prefix("[submodule \"")?
        .strip_suffix("\"]")?;
    Some(name.to_string())
}

fn passthrough_current_rm_invocation() -> Result<()> {
    git_passthrough::run_current_invocation("rm")
}


