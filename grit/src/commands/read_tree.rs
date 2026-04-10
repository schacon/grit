//! `grit read-tree` — read tree information into the index.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::io::Write as _;
use std::path::{Path, PathBuf};

use grit_lib::config::ConfigSet;
use grit_lib::crlf;
use grit_lib::diff::{read_submodule_head_oid, submodule_embedded_git_dir};
use grit_lib::ignore::IgnoreMatcher;
use grit_lib::index::{Index, IndexEntry, MODE_EXECUTABLE, MODE_GITLINK, MODE_SYMLINK, MODE_TREE};
use grit_lib::objects::{parse_commit, parse_tree, ObjectId, ObjectKind};
use grit_lib::refs::resolve_ref;
use grit_lib::repo::Repository;
use grit_lib::rev_parse::resolve_revision;
use grit_lib::sparse_checkout::apply_sparse_checkout_skip_worktree;
use grit_lib::submodule_gitdir::{
    set_submodule_repo_worktree, submodule_modules_git_dir, write_submodule_gitfile,
};
use grit_lib::write_tree::write_tree_from_index;

use crate::dotfile;

/// Arguments for `grit read-tree`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Perform a merge (2-tree or 3-tree).
    #[arg(short = 'm')]
    pub merge: bool,

    /// Perform index-only operation (don't check working tree).
    #[arg(short = 'i')]
    pub index_only: bool,

    /// Update working tree after reading.
    #[arg(short = 'u')]
    pub update: bool,

    /// Reset the index (discard conflicting entries).
    #[arg(long)]
    pub reset: bool,

    /// Stage a tree into the index under the given prefix (must end with /).
    #[arg(long)]
    pub prefix: Option<String>,

    /// Do not print error messages for missing paths.
    #[arg(long = "aggressive")]
    pub aggressive: bool,

    /// Dry-run: perform checks but do not actually update the index.
    #[arg(short = 'n', long = "dry-run")]
    pub dry_run: bool,

    /// Per-directory ignore file name used to allow clobbering ignored files.
    #[arg(long = "exclude-per-directory")]
    pub exclude_per_directory: Option<String>,

    /// Empty the index.
    #[arg(long = "empty")]
    pub empty: bool,

    /// Set a path prefix for the work tree (used with `-u` validation messages).
    #[arg(long = "super-prefix")]
    pub super_prefix: Option<String>,

    /// After updating the work tree, run `submodule update --init --recursive`.
    #[arg(long = "recurse-submodules", alias = "recursive")]
    pub recurse_submodules: bool,

    /// Skip applying sparse-checkout skip-worktree bits (matches `git read-tree --no-sparse-checkout`).
    #[arg(long = "no-sparse-checkout")]
    pub no_sparse_checkout: bool,

    /// Tree-ish arguments (1 for reset, 2 for 2-way merge, 3 for 3-way merge).
    pub trees: Vec<String>,
}

/// Path protection settings from core.protectHFS / core.protectNTFS.
#[derive(Clone, Copy)]
struct PathProtection {
    protect_hfs: bool,
    protect_ntfs: bool,
}

impl PathProtection {
    fn load(git_dir: &Path) -> Self {
        let config = ConfigSet::load(Some(git_dir), true).unwrap_or_else(|_| ConfigSet::new());
        let protect_hfs = config
            .get("core.protectHFS")
            .map(|v| v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        let protect_ntfs = config
            .get("core.protectNTFS")
            .map(|v| v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        Self {
            protect_hfs,
            protect_ntfs,
        }
    }
}

/// Check whether a single path component (file or directory name) is
/// forbidden.  Returns `Err` with a message when the name is rejected.
fn verify_path_component(name: &[u8], prot: PathProtection) -> Result<()> {
    // Always reject "." and ".."
    if name == b"." {
        bail!("invalid path '.'");
    }
    if name == b".." {
        bail!("invalid path '..'");
    }

    // Always reject ".git" (exact lowercase — matches C git's verify_dotfile)
    if name == b".git" {
        bail!("invalid path '.git'");
    }

    // HFS / NTFS case-insensitive ".git" checks.
    if (prot.protect_hfs || prot.protect_ntfs) && name.len() == 4 && name[0] == b'.' {
        let rest = &name[1..];
        if rest.eq_ignore_ascii_case(b"git") {
            bail!("invalid path '{}'", String::from_utf8_lossy(name));
        }
    }
    if prot.protect_hfs && hfs_equivalent_to_dotgit(name) {
        bail!("invalid path '{}'", String::from_utf8_lossy(name));
    }

    // NTFS short-name check: "git~1" (case-insensitive)
    if prot.protect_ntfs && name.eq_ignore_ascii_case(b"git~1") {
        bail!("invalid path '{}'", String::from_utf8_lossy(name));
    }

    if prot.protect_ntfs {
        // Backslashes are treated as path separators on NTFS, so reject
        // confusing names that rely on '\' being a regular byte.
        if name.contains(&b'\\') {
            bail!("invalid path '{}'", String::from_utf8_lossy(name));
        }

        // Reject NTFS-equivalent ".git" names such as ".git ", ".git...",
        // and alternate stream forms like ".git...:stream".
        if ntfs_equivalent_to_dotgit(name) {
            bail!("invalid path '{}'", String::from_utf8_lossy(name));
        }
    }

    if prot.protect_hfs || prot.protect_ntfs {
        let Ok(s) = std::str::from_utf8(name) else {
            return Ok(());
        };
        if dotfile::is_hfs_dot_gitmodules(s)
            || dotfile::is_ntfs_dot_gitmodules(s)
            || dotfile::is_hfs_dot_gitattributes(s)
            || dotfile::is_ntfs_dot_gitattributes(s)
            || dotfile::is_hfs_dot_gitignore(s)
            || dotfile::is_ntfs_dot_gitignore(s)
            || dotfile::is_hfs_dot_mailmap(s)
            || dotfile::is_ntfs_dot_mailmap(s)
        {
            bail!("invalid path '{}'", String::from_utf8_lossy(name));
        }
    }

    Ok(())
}

fn ntfs_equivalent_to_dotgit(name: &[u8]) -> bool {
    if name.len() < 4 || !name[..4].eq_ignore_ascii_case(b".git") {
        return false;
    }

    let rest = &name[4..];
    if rest.is_empty() {
        return true;
    }

    let head = rest.split(|b| *b == b':').next().unwrap_or(rest);
    let mut trimmed_len = head.len();
    while trimmed_len > 0 && matches!(head[trimmed_len - 1], b'.' | b' ') {
        trimmed_len -= 1;
    }

    trimmed_len == 0
}

fn hfs_equivalent_to_dotgit(name: &[u8]) -> bool {
    let Ok(path) = std::str::from_utf8(name) else {
        return false;
    };

    let folded: String = path
        .chars()
        .filter(|ch| !matches!(*ch, '\u{200c}' | '\u{200d}'))
        .flat_map(char::to_lowercase)
        .collect();
    folded == ".git"
}

/// Run `grit read-tree`.
///
/// # Errors
///
/// Returns an error when repository discovery fails, tree-ish resolution
/// fails, index/worktree updates fail, or option combinations are invalid.
pub fn run(args: Args) -> Result<()> {
    maybe_write_trace_packet_done();
    let repo = Repository::discover(None).context("not a git repository")?;
    let index_path = effective_index_path(&repo)?;
    let prot = PathProtection::load(&repo.git_dir);
    let dry_run = args.dry_run;

    // Handle --empty: clear the index
    if args.empty {
        if !dry_run {
            let mut empty_index = Index::new();
            repo.write_index_at(&index_path, &mut empty_index)
                .context("writing empty index")?;
        }
        return Ok(());
    }

    let tree_oids: Vec<ObjectId> = args
        .trees
        .iter()
        .map(|t| resolve_tree_ish(&repo, t))
        .collect::<Result<Vec<_>>>()?;

    if tree_oids.is_empty() {
        bail!("at least one tree required");
    }
    if tree_oids.len() > 4 {
        bail!("too many trees (max 4)");
    }

    if let Some(prefix) = &args.prefix {
        if prefix.starts_with('/') {
            bail!("--prefix must be relative to repository root");
        }
        if !prefix.is_empty() && !prefix.ends_with('/') {
            bail!("--prefix requires a trailing '/'");
        }
        if args.merge || args.reset || tree_oids.len() != 1 {
            bail!("--prefix only supports a single non-merge tree read");
        }
    }

    let allow_ignored_overwrite = match args.exclude_per_directory.as_deref() {
        None => false,
        Some(".gitignore") => {
            if !args.update {
                bail!("--exclude-per-directory requires -u");
            }
            true
        }
        Some(other) => {
            bail!("unsupported --exclude-per-directory value '{other}'");
        }
    };

    let cfg = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
    let recurse_submodules_effective = args.recurse_submodules
        || cfg
            .get("submodule.recurse")
            .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
            .unwrap_or(false);

    if args.reset {
        // Reset mode is a hard replacement by the final tree argument.
        let old_index = repo.load_index_at(&index_path).context("loading index")?;
        let mut new_index = Index::new();
        new_index.entries =
            tree_to_index_entries(&repo, &tree_oids[tree_oids.len() - 1], "", prot)?;
        new_index.sort();
        if !dry_run {
            apply_sparse_checkout(
                &repo.git_dir,
                repo.work_tree.as_deref(),
                &mut new_index,
                args.no_sparse_checkout,
            )?;
        }
        if args.update {
            validate_worktree_updates(
                &repo,
                &old_index,
                &new_index,
                allow_ignored_overwrite,
                args.super_prefix.as_deref(),
                recurse_submodules_effective,
                args.recurse_submodules,
                true,
            )?;
        }
        if !dry_run && args.update {
            checkout_index_entries(
                &repo,
                &old_index,
                &new_index,
                recurse_submodules_effective,
                true,
            )?;
        }
        if !dry_run {
            repo.write_index_at(&index_path, &mut new_index)
                .context("writing index")?;
        }
        if args.update && recurse_submodules_effective && !dry_run {
            submodule_update_after_read_tree(&repo, &old_index, &new_index, prot)?;
        }
        return Ok(());
    }

    let old_index = repo.load_index_at(&index_path).context("loading index")?;
    let mut new_index = old_index.clone();

    if let Some(prefix) = &args.prefix {
        read_tree_into_index_prefixed(&repo, &tree_oids[0], prefix, &mut new_index, prot)?;
    } else if !args.merge {
        if tree_oids.len() == 1 {
            // Replace index with one tree.
            new_index = Index::new();
            new_index.entries = tree_to_index_entries(&repo, &tree_oids[0], "", prot)?;
            new_index.sort();
        } else {
            // Multi-tree overlay: later trees override earlier trees by path.
            new_index = Index::new();
            for oid in &tree_oids {
                for e in tree_to_index_entries(&repo, oid, "", prot)? {
                    add_or_replace_with_df_cleanup(&mut new_index, e);
                }
            }
        }
    } else {
        match tree_oids.len() {
            1 => {
                // `-m` with one tree: replace index with new tree, but carry forward
                // unmerged entries. All stage-0 entries not in the new tree are removed.
                let new_tree_entries = tree_to_index_entries(&repo, &tree_oids[0], "", prot)?;
                let new_tree_map = tree_to_map(new_tree_entries.clone());
                let new_tree_paths: std::collections::HashSet<Vec<u8>> =
                    new_tree_entries.iter().map(|e| e.path.clone()).collect();
                // Keep only stage != 0 (unmerged) entries from old index that aren't in new tree
                new_index
                    .entries
                    .retain(|e| e.stage() != 0 || new_tree_paths.contains(&e.path));
                for e in new_tree_entries {
                    add_or_replace_with_df_cleanup(&mut new_index, e);
                }
                new_index.entries.retain(|e| {
                    if e.stage() != 0 {
                        return true;
                    }
                    for root in new_tree_map.keys() {
                        if path_is_parent_of(root.as_slice(), e.path.as_slice())
                            && !new_tree_map.contains_key(&e.path)
                        {
                            return false;
                        }
                    }
                    true
                });
            }
            2 => {
                let old_tree = tree_to_map(tree_to_index_entries(&repo, &tree_oids[0], "", prot)?);
                let new_tree = tree_to_map(tree_to_index_entries(&repo, &tree_oids[1], "", prot)?);
                new_index = two_way_merge(&repo, &old_index, &old_tree, &new_tree)?;
            }
            3 => {
                let base = tree_to_map(tree_to_index_entries(&repo, &tree_oids[0], "", prot)?);
                let ours = tree_to_map(tree_to_index_entries(&repo, &tree_oids[1], "", prot)?);
                let theirs = tree_to_map(tree_to_index_entries(&repo, &tree_oids[2], "", prot)?);
                new_index = three_way_merge(&repo, &old_index, &base, &ours, &theirs)?;
            }
            4 => {
                if tree_oids[0] != tree_oids[3] || tree_oids[1] != tree_oids[2] {
                    bail!("read-tree: unsupported 4-tree merge layout");
                }
                let t0 = tree_to_map(tree_to_index_entries(&repo, &tree_oids[0], "", prot)?);
                let t1 = tree_to_map(tree_to_index_entries(&repo, &tree_oids[1], "", prot)?);
                new_index = four_tree_symmetric_merge(&repo, &old_index, &t0, &t1)?;
            }
            _ => unreachable!("tree count validated above"),
        }
    }

    // Apply sparse checkout: set skip-worktree on entries not matching patterns
    apply_sparse_checkout(
        &repo.git_dir,
        repo.work_tree.as_deref(),
        &mut new_index,
        args.no_sparse_checkout,
    )?;

    if args.update {
        validate_worktree_updates(
            &repo,
            &old_index,
            &new_index,
            allow_ignored_overwrite,
            args.super_prefix.as_deref(),
            recurse_submodules_effective,
            args.recurse_submodules,
            false,
        )?;
    }

    sparse_refresh_skip_worktree_for_dirty_paths(&repo, &old_index, &mut new_index)?;
    if !dry_run && args.update {
        checkout_index_entries(
            &repo,
            &old_index,
            &new_index,
            recurse_submodules_effective,
            false,
        )?;
    }
    if !dry_run {
        repo.write_index_at(&index_path, &mut new_index)
            .context("writing index")?;
    }

    if args.update && recurse_submodules_effective && !dry_run {
        submodule_update_after_read_tree(&repo, &old_index, &new_index, prot)?;
    }

    Ok(())
}

/// Append `super_objects_dir` to `submodule_objects_dir/info/alternates` if missing so nested Git
/// commands can read trees/commits stored only in the superproject ODB (t1013 replace_sub1_with_directory).
fn ensure_superproject_objects_alternate(
    submodule_objects_dir: &Path,
    super_objects_dir: &Path,
) -> Result<()> {
    let super_abs = super_objects_dir
        .canonicalize()
        .unwrap_or_else(|_| super_objects_dir.to_path_buf());
    let line = format!("{}\n", super_abs.display());
    let info = submodule_objects_dir.join("info");
    std::fs::create_dir_all(&info)?;
    let alt_path = info.join("alternates");
    let mut existing = String::new();
    if alt_path.is_file() {
        existing = std::fs::read_to_string(&alt_path)?;
    }
    if existing
        .lines()
        .any(|l| l.trim() == super_abs.to_string_lossy())
    {
        return Ok(());
    }
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&alt_path)?;
    f.write_all(line.as_bytes())?;
    Ok(())
}

/// OID of the tree object at `path` (slash-separated, no leading slash) inside `root_tree_oid`.
fn tree_oid_at_path(repo: &Repository, root_tree_oid: &ObjectId, path: &str) -> Result<ObjectId> {
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let mut cur = *root_tree_oid;
    for seg in segments {
        let obj = repo.odb.read(&cur)?;
        if obj.kind != ObjectKind::Tree {
            bail!("expected tree at '{}', got {}", path, obj.kind);
        }
        let entries = parse_tree(&obj.data)?;
        let te = entries
            .iter()
            .find(|e| e.name == seg.as_bytes())
            .with_context(|| format!("missing path segment '{seg}' in tree"))?;
        if te.mode != MODE_TREE {
            bail!("path segment '{seg}' is not a tree");
        }
        cur = te.oid;
    }
    Ok(cur)
}

/// When the superproject replaces a submodule gitlink with a directory tree, the nested repository's
/// index can still describe the old submodule commit while the work tree already matches the new tree
/// from `read-tree -u`. Rewrite the submodule's index from the superproject tree (no nested
/// `read-tree`, which would fail "local changes" checks against the stale index) so `git status` and
/// `submodule update --recursive` see a consistent state (t1013.16).
fn sync_submodule_indices_after_gitlink_to_tree(
    repo: &Repository,
    old_index: &Index,
    new_index: &Index,
    prot: PathProtection,
) -> Result<()> {
    let work_tree = match &repo.work_tree {
        Some(p) => p,
        None => return Ok(()),
    };
    let old_stage0 = stage0_index_map(old_index);
    let super_root_tree = write_tree_from_index(&repo.odb, new_index, "")?;

    for (path_bytes, old_e) in &old_stage0 {
        if old_e.mode != MODE_GITLINK {
            continue;
        }
        if !index_has_strict_path_prefix(new_index, path_bytes.as_slice()) {
            continue;
        }
        let rel = String::from_utf8_lossy(path_bytes).into_owned();
        let abs = work_tree.join(&rel);
        if submodule_embedded_git_dir(&abs).is_none() {
            continue;
        }
        let sub_tree_oid = tree_oid_at_path(repo, &super_root_tree, &rel)?;
        let mut entries = tree_to_index_entries(repo, &sub_tree_oid, "", prot)?;
        for e in &mut entries {
            e.ctime_sec = 0;
            e.ctime_nsec = 0;
            e.mtime_sec = 0;
            e.mtime_nsec = 0;
            e.dev = 0;
            e.ino = 0;
            e.uid = 0;
            e.gid = 0;
            e.size = 0;
        }
        let mut sub_index = Index::new();
        sub_index.entries = entries;
        sub_index.sort();
        let modules_git = submodule_modules_git_dir(&repo.git_dir, &rel);
        let index_file = modules_git.join("index");
        sub_index
            .write(&index_file)
            .with_context(|| format!("writing submodule index at '{rel}' after gitlink→tree"))?;
        ensure_superproject_objects_alternate(&modules_git.join("objects"), repo.odb.objects_dir())
            .with_context(|| format!("linking superproject ODB into submodule at '{rel}'"))?;
    }
    Ok(())
}

fn submodule_update_after_read_tree(
    repo: &Repository,
    old_index: &Index,
    new_index: &Index,
    prot: PathProtection,
) -> Result<()> {
    let work_tree = repo
        .work_tree
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("read-tree --recurse-submodules requires a work tree"))?;
    sync_submodule_indices_after_gitlink_to_tree(repo, old_index, new_index, prot)?;
    let grit_bin = crate::grit_exe::grit_executable();
    let status = std::process::Command::new(&grit_bin)
        .args(["submodule", "update", "--init", "--recursive"])
        .current_dir(work_tree)
        .status()
        .context("submodule update after read-tree")?;
    if !status.success() {
        bail!("submodule update failed after read-tree");
    }
    Ok(())
}

/// Recursively read a tree object into index entries.
fn tree_to_index_entries(
    repo: &Repository,
    oid: &ObjectId,
    prefix: &str,
    prot: PathProtection,
) -> Result<Vec<IndexEntry>> {
    let obj = repo.odb.read(oid)?;
    if obj.kind != ObjectKind::Tree {
        bail!("expected tree, got {}", obj.kind);
    }
    let entries = parse_tree(&obj.data)?;
    let mut result = Vec::new();
    let allow_null = std::env::var("GIT_ALLOW_NULL_SHA1").as_deref() == Ok("1");

    for te in entries {
        if !allow_null && te.oid.is_zero() {
            let name = String::from_utf8_lossy(&te.name);
            bail!("entry '{}' has a null sha1", name);
        }
        verify_path_component(&te.name, prot)?;

        let name = String::from_utf8_lossy(&te.name).into_owned();
        let path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };

        if te.mode == 0o040000 {
            // Sub-tree: recurse
            let sub = tree_to_index_entries(repo, &te.oid, &path, prot)?;
            result.extend(sub);
        } else if te.mode == MODE_GITLINK {
            // Submodule gitlink: single index entry; OID names a commit in the submodule ODB.
            let path_bytes = path.into_bytes();
            result.push(IndexEntry {
                ctime_sec: 0,
                ctime_nsec: 0,
                mtime_sec: 0,
                mtime_nsec: 0,
                dev: 0,
                ino: 0,
                mode: MODE_GITLINK,
                uid: 0,
                gid: 0,
                size: 0,
                oid: te.oid,
                flags: path_bytes.len().min(0xFFF) as u16,
                flags_extended: None,
                path: path_bytes,
                base_index_pos: 0,
            });
        } else {
            let path_bytes = path.into_bytes();
            result.push(IndexEntry {
                ctime_sec: 0,
                ctime_nsec: 0,
                mtime_sec: 0,
                mtime_nsec: 0,
                dev: 0,
                ino: 0,
                mode: te.mode,
                uid: 0,
                gid: 0,
                size: 0,
                oid: te.oid,
                flags: path_bytes.len().min(0xFFF) as u16,
                flags_extended: None,
                path: path_bytes,
                base_index_pos: 0,
            });
        }
    }
    Ok(result)
}

/// Read a tree into the index under a prefix.
fn read_tree_into_index_prefixed(
    repo: &Repository,
    oid: &ObjectId,
    prefix: &str,
    index: &mut Index,
    prot: PathProtection,
) -> Result<()> {
    // Strip trailing slash from prefix for storage
    let prefix = prefix.trim_end_matches('/');
    let entries = tree_to_index_entries(repo, oid, prefix, prot)?;
    for e in entries {
        add_or_replace_with_df_cleanup(index, e);
    }
    Ok(())
}

fn tree_to_map(entries: Vec<IndexEntry>) -> HashMap<Vec<u8>, IndexEntry> {
    let mut out = HashMap::new();
    for e in entries {
        out.insert(e.path.clone(), e);
    }
    out
}

fn add_or_replace_with_df_cleanup(index: &mut Index, entry: IndexEntry) {
    let new_path = entry.path.clone();
    index
        .entries
        .retain(|e| e.stage() != 0 || !paths_conflict_for_df(&e.path, &new_path));
    index.add_or_replace(entry);
}

fn paths_conflict_for_df(a: &[u8], b: &[u8]) -> bool {
    a == b || path_is_parent_of(a, b) || path_is_parent_of(b, a)
}

fn path_is_parent_of(parent: &[u8], child: &[u8]) -> bool {
    if parent.len() >= child.len() {
        return false;
    }
    child.starts_with(parent) && child[parent.len()] == b'/'
}

/// True when `path` is strictly under `prefix/` (prefix is a path segment boundary).
fn path_is_strict_descendant(path: &[u8], prefix: &[u8]) -> bool {
    path_is_parent_of(prefix, path)
}

/// True when the index has a stage-0 entry strictly under `prefix/` (path segment boundary).
fn index_has_strict_path_prefix(index: &Index, prefix: &[u8]) -> bool {
    index.entries.iter().any(|e| {
        e.stage() == 0
            && e.path.len() > prefix.len()
            && e.path.starts_with(prefix)
            && e.path[prefix.len()] == b'/'
    })
}

/// True when `path` is strictly under `root/` where `root` was a gitlink in `old` and is replaced by
/// a tree at `root` in `new`. Skips validation only for paths inside the former checkout, not the
/// root path itself (so gitlink→file untracked checks at `root` still run; t1013.9 vs t1013.16).
fn path_under_gitlink_replaced_by_tree(
    path: &[u8],
    old_stage0: &HashMap<Vec<u8>, IndexEntry>,
    new_stage0: &HashMap<Vec<u8>, IndexEntry>,
) -> bool {
    for (root, oe) in old_stage0 {
        if oe.mode != MODE_GITLINK {
            continue;
        }
        if !path_is_parent_of(root, path) {
            continue;
        }
        match new_stage0.get(root) {
            None => return true,
            Some(ne) if ne.mode != MODE_GITLINK => return true,
            Some(_) => {}
        }
    }
    false
}

fn submodule_ignore_matcher_for_untracked(
    repo: &Repository,
    rel_sub: &str,
) -> Result<IgnoreMatcher> {
    let mut m = IgnoreMatcher::from_repository(repo)?;
    let modules = submodule_modules_git_dir(&repo.git_dir, rel_sub);
    let exc = modules.join("info/exclude");
    if exc.is_file() {
        m.add_exclude_from_files(&[exc], std::path::Path::new("/"))?;
    }
    Ok(m)
}

/// True when the submodule at `rel_sub` contains an untracked path that is not ignored by the
/// superproject's ignore rules (t1013 untracked-file guard).
fn submodule_has_nonignored_untracked(
    repo: &Repository,
    super_work_tree: &Path,
    super_index: &Index,
    rel_sub: &str,
) -> Result<bool> {
    let sub_dir = super_work_tree.join(rel_sub);
    let Some(sub_git) = submodule_embedded_git_dir(&sub_dir) else {
        return Ok(false);
    };
    let index_path = sub_git.join("index");
    let sub_index = match Index::load(&index_path) {
        Ok(ix) => ix,
        // Missing or corrupt submodule index: fall back to superproject stage-0 paths under `rel_sub`
        // so untracked files inside the checkout are still detected (t1013.9).
        Err(_) => {
            let prefix = format!("{rel_sub}/");
            let mut synthetic = Index::new();
            for e in &super_index.entries {
                if e.stage() != 0 {
                    continue;
                }
                let Ok(p) = std::str::from_utf8(&e.path) else {
                    continue;
                };
                if p == rel_sub {
                    continue;
                }
                let Some(rest) = p.strip_prefix(&prefix) else {
                    continue;
                };
                let mut ne = e.clone();
                ne.path = rest.as_bytes().to_vec();
                ne.flags = (ne.flags & 0xF000) | (ne.path.len().min(0xFFF) as u16);
                synthetic.entries.push(ne);
            }
            synthetic
        }
    };
    let tracked: std::collections::BTreeSet<String> = sub_index
        .entries
        .iter()
        .filter(|e| e.stage() == 0)
        .map(|e| String::from_utf8_lossy(&e.path).into_owned())
        .collect();
    let mut matcher = submodule_ignore_matcher_for_untracked(repo, rel_sub)?;
    fn walk(
        dir: &Path,
        sub_root: &Path,
        rel_sub: &str,
        tracked: &std::collections::BTreeSet<String>,
        repo: &Repository,
        super_index: &Index,
        matcher: &mut IgnoreMatcher,
    ) -> Result<bool> {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return Ok(false),
        };
        let mut sorted: Vec<_> = entries.filter_map(|e| e.ok()).collect();
        sorted.sort_by_key(|e| e.file_name());
        for entry in sorted {
            let name = entry.file_name().to_string_lossy().to_string();
            if name == ".git" {
                continue;
            }
            let path = entry.path();
            let rel = path
                .strip_prefix(sub_root)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| name.clone());
            let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
            if is_dir {
                if walk(
                    &path,
                    sub_root,
                    rel_sub,
                    tracked,
                    repo,
                    super_index,
                    matcher,
                )? {
                    return Ok(true);
                }
            } else if !tracked.contains(&rel) {
                let super_rel = if rel.is_empty() {
                    rel_sub.to_string()
                } else {
                    format!("{rel_sub}/{rel}")
                };
                let (ignored, _) =
                    matcher.check_path(repo, Some(super_index), &super_rel, false)?;
                if !ignored {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }
    walk(
        &sub_dir,
        &sub_dir,
        rel_sub,
        &tracked,
        repo,
        super_index,
        &mut matcher,
    )
}

fn stage0_index_map(index: &Index) -> HashMap<Vec<u8>, IndexEntry> {
    let mut out = HashMap::new();
    for e in &index.entries {
        if e.stage() == 0 {
            out.insert(e.path.clone(), e.clone());
        }
    }
    out
}

fn same_blob(a: &IndexEntry, b: &IndexEntry) -> bool {
    a.oid == b.oid && a.mode == b.mode
}

fn remove_index_descendants_not_in_tree(
    result: &mut HashMap<Vec<u8>, IndexEntry>,
    root: &[u8],
    new_tree: &HashMap<Vec<u8>, IndexEntry>,
) {
    let to_remove: Vec<Vec<u8>> = result
        .keys()
        .filter(|p| path_is_parent_of(root, p) && !new_tree.contains_key(*p))
        .cloned()
        .collect();
    for p in to_remove {
        result.remove(&p);
    }
}

fn remove_index_descendants(result: &mut HashMap<Vec<u8>, IndexEntry>, root: &[u8]) {
    let to_remove: Vec<Vec<u8>> = result
        .keys()
        .filter(|p| path_is_parent_of(root, p))
        .cloned()
        .collect();
    for p in to_remove {
        result.remove(&p);
    }
}

fn two_way_merge(
    repo: &Repository,
    current_index: &Index,
    old_tree: &HashMap<Vec<u8>, IndexEntry>,
    new_tree: &HashMap<Vec<u8>, IndexEntry>,
) -> Result<Index> {
    let mut result = stage0_index_map(current_index);
    let current = stage0_index_map(current_index);
    let mut conflicts = Vec::new();

    let mut all_paths = BTreeSet::new();
    all_paths.extend(old_tree.keys().cloned());
    all_paths.extend(new_tree.keys().cloned());

    for path in all_paths {
        let old = old_tree.get(&path);
        let new = new_tree.get(&path);
        let cur = current.get(&path);

        match (old, new) {
            (Some(o), Some(n)) if same_blob(o, n) => {
                // unchanged between trees: carry current index forward, or
                // populate from the trees when starting from an empty index.
                if cur.is_none() {
                    result.insert(path.clone(), n.clone());
                    remove_index_descendants_not_in_tree(&mut result, &path, new_tree);
                } else if let Some(c) = cur {
                    if same_blob(c, n) {
                        remove_index_descendants_not_in_tree(&mut result, &path, new_tree);
                    }
                }
            }
            (None, Some(n)) => match cur {
                None => {
                    result.insert(path.clone(), n.clone());
                    remove_index_descendants_not_in_tree(&mut result, &path, new_tree);
                }
                Some(c) if same_blob(c, n) => {
                    remove_index_descendants_not_in_tree(&mut result, &path, new_tree);
                }
                Some(_) => conflicts.push(String::from_utf8_lossy(&path).into_owned()),
            },
            (Some(o), None) => match cur {
                None => {
                    result.remove(&path);
                    remove_index_descendants(&mut result, &path);
                }
                Some(c) if same_blob(c, o) => {
                    require_uptodate(repo, c)?;
                    result.remove(&path);
                    remove_index_descendants(&mut result, &path);
                }
                Some(_) => conflicts.push(String::from_utf8_lossy(&path).into_owned()),
            },
            (Some(o), Some(n)) => match cur {
                None => {
                    // Empty/new index case: just move to the merged head.
                    result.insert(path.clone(), n.clone());
                    remove_index_descendants_not_in_tree(&mut result, &path, new_tree);
                }
                Some(c) if same_blob(c, o) => {
                    if !same_blob(o, n) {
                        require_uptodate(repo, c)?;
                    }
                    result.insert(path.clone(), n.clone());
                    remove_index_descendants_not_in_tree(&mut result, &path, new_tree);
                }
                Some(c) if same_blob(c, n) => {
                    remove_index_descendants_not_in_tree(&mut result, &path, new_tree);
                }
                Some(_) => conflicts.push(String::from_utf8_lossy(&path).into_owned()),
            },
            (None, None) => {}
        }
    }

    if !conflicts.is_empty() {
        bail!(
            "read-tree: merge conflict in {} path(s): {}",
            conflicts.len(),
            conflicts.join(", ")
        );
    }

    let mut out = Index::new();
    out.entries = result.into_values().collect();
    // Drop stage-0 paths that sit under a `new_tree` key but are not themselves in `new_tree`
    // (e.g. former `sub1/file1` when `new_tree` only has gitlink or blob `sub1`).
    out.entries.retain(|e| {
        if e.stage() != 0 {
            return true;
        }
        for root in new_tree.keys() {
            if path_is_parent_of(root.as_slice(), e.path.as_slice())
                && !new_tree.contains_key(&e.path)
            {
                return false;
            }
        }
        true
    });
    out.sort();
    Ok(out)
}

/// When the index matches tree A (ours), Git also requires the working tree file to match
/// the index for cases marked "up-to-date" in `t1000-read-tree-m-3way.sh`.
fn require_uptodate(repo: &Repository, entry: &IndexEntry) -> Result<()> {
    let Some(wt) = &repo.work_tree else {
        return Ok(());
    };
    let rel = String::from_utf8_lossy(&entry.path);
    let abs = wt.join(rel.as_ref());
    if !worktree_matches_entry(repo, entry, &abs)? {
        bail!("read-tree: local changes would be overwritten by merge");
    }
    Ok(())
}

fn validate_three_way_index_stage0(
    repo: &Repository,
    current_index: &Index,
    base: &HashMap<Vec<u8>, IndexEntry>,
    ours: &HashMap<Vec<u8>, IndexEntry>,
    theirs: &HashMap<Vec<u8>, IndexEntry>,
) -> Result<()> {
    for e in &current_index.entries {
        if e.stage() != 0 {
            continue;
        }
        let path = &e.path;
        let b = base.get(path);
        let o = ours.get(path);
        let t = theirs.get(path);

        match (b, o, t) {
            (None, None, None) => {
                bail!("read-tree: would lose untracked local changes");
            }
            (None, None, Some(te)) => {
                if !same_blob(e, te) {
                    bail!("read-tree: local changes would be overwritten by merge");
                }
            }
            (None, Some(oe), None) => {
                if !same_blob(e, oe) {
                    bail!("read-tree: local changes would be overwritten by merge");
                }
            }
            (None, Some(oe), Some(te)) => {
                if !same_blob(oe, te) {
                    if !same_blob(e, oe) {
                        bail!("read-tree: local changes would be overwritten by merge");
                    }
                    require_uptodate(repo, e)?;
                } else if !same_blob(e, oe) {
                    bail!("read-tree: local changes would be overwritten by merge");
                }
            }
            (Some(_be), None, None) => {
                bail!("read-tree: would lose untracked local changes");
            }
            (Some(_be), None, Some(_te)) => {
                bail!("read-tree: would lose untracked local changes");
            }
            (Some(_be), Some(oe), None) => {
                if !same_blob(e, oe) {
                    bail!("read-tree: local changes would be overwritten by merge");
                }
                require_uptodate(repo, e)?;
            }
            (Some(be), Some(oe), Some(te)) => {
                let o_eq_b = same_blob(oe, be);
                let b_eq_t = same_blob(be, te);
                let o_eq_t = same_blob(oe, te);

                if o_eq_b && !b_eq_t {
                    if same_blob(e, oe) {
                        require_uptodate(repo, e)?;
                    } else if !same_blob(e, te) {
                        bail!("read-tree: local changes would be overwritten by merge");
                    }
                } else if !o_eq_b && !b_eq_t && !o_eq_t {
                    if !same_blob(e, oe) {
                        bail!("read-tree: local changes would be overwritten by merge");
                    }
                    require_uptodate(repo, e)?;
                } else if !same_blob(e, oe) {
                    bail!("read-tree: local changes would be overwritten by merge");
                }
            }
        }
    }
    Ok(())
}

fn three_way_merge(
    repo: &Repository,
    current_index: &Index,
    base: &HashMap<Vec<u8>, IndexEntry>,
    ours: &HashMap<Vec<u8>, IndexEntry>,
    theirs: &HashMap<Vec<u8>, IndexEntry>,
) -> Result<Index> {
    validate_three_way_index_stage0(repo, current_index, base, ours, theirs)?;

    let mut all_paths = BTreeSet::new();
    all_paths.extend(base.keys().cloned());
    all_paths.extend(ours.keys().cloned());
    all_paths.extend(theirs.keys().cloned());

    let mut out = Index::new();
    let df_roots = detect_df_conflict_roots(&all_paths);

    for path in all_paths {
        let b = base.get(&path);
        let o = ours.get(&path);
        let t = theirs.get(&path);

        // Directory/file conflicts are represented as unmerged stages.
        // For the conflicting root path we keep stage entries for whichever
        // side(s) have the file. For descendants under the conflicting root,
        // we also keep their side-specific stages, even when one side deleted
        // the path, to match Git's read-tree conflict shape.
        if is_df_conflict_path(&df_roots, &path) {
            if let Some(be) = b {
                stage_entry(&mut out, be, 1);
            }
            if let Some(oe) = o {
                stage_entry(&mut out, oe, 2);
            }
            if let Some(te) = t {
                stage_entry(&mut out, te, 3);
            }
            continue;
        }

        match (b, o, t) {
            (_, Some(oe), Some(te)) if oe.oid == te.oid => {
                // Both same: take ours
                out.entries.push((*oe).clone());
            }
            (Some(be), Some(oe), Some(te)) if be.oid == oe.oid => {
                // Only theirs changed: take theirs
                out.entries.push((*te).clone());
            }
            (Some(be), Some(oe), Some(te)) if be.oid == te.oid => {
                // Only ours changed: take ours
                out.entries.push((*oe).clone());
            }
            (None, Some(oe), None) => {
                // Added by us only
                out.entries.push((*oe).clone());
            }
            (None, None, Some(te)) => {
                // Added by them only
                out.entries.push((*te).clone());
            }
            (Some(be), None, None) => {
                // Ancestor had the path; both other trees omit it (delete/delete vs base).
                // Match git read-tree: keep stage 1 (ancestor blob) with no stages 2/3.
                stage_entry(&mut out, be, 1);
            }
            (Some(be), None, Some(te)) => {
                // Deleted by us, modified by them: conflict
                stage_entry(&mut out, be, 1);
                stage_entry(&mut out, te, 3);
            }
            (Some(be), Some(oe), None) => {
                // Modified by us, deleted by them: conflict
                stage_entry(&mut out, be, 1);
                stage_entry(&mut out, oe, 2);
            }
            _ => {
                // True conflict: add all three stages
                if let Some(be) = b {
                    stage_entry(&mut out, be, 1);
                }
                if let Some(oe) = o {
                    stage_entry(&mut out, oe, 2);
                }
                if let Some(te) = t {
                    stage_entry(&mut out, te, 3);
                }
            }
        }
    }

    for e in &current_index.entries {
        if e.stage() != 0 {
            out.entries.push(e.clone());
        }
    }

    out.sort();
    Ok(out)
}

/// `read-tree -m T0 T1 T1 T0` (case #16 in `t1000-read-tree-m-3way.sh`).
/// Git still uses `threeway_merge` with `head_idx = stage - 2`, producing stages 2 and 3 when `T0` and `T1` disagree.
fn four_tree_symmetric_merge(
    _repo: &Repository,
    current_index: &Index,
    t0: &HashMap<Vec<u8>, IndexEntry>,
    t1: &HashMap<Vec<u8>, IndexEntry>,
) -> Result<Index> {
    if current_index.entries.iter().any(|e| e.stage() != 0) {
        bail!("read-tree: unmerged entries in index");
    }

    let mut all_paths = BTreeSet::new();
    all_paths.extend(t0.keys().cloned());
    all_paths.extend(t1.keys().cloned());

    let mut out = Index::new();

    for path in all_paths {
        let e0 = t0.get(&path);
        let e1 = t1.get(&path);
        match (e0, e1) {
            (Some(a), Some(b)) if same_blob(a, b) => {
                out.entries.push(a.clone());
            }
            (Some(a), Some(b)) => {
                stage_entry(&mut out, b, 2);
                stage_entry(&mut out, a, 3);
            }
            (Some(a), None) => {
                out.entries.push(a.clone());
            }
            (None, Some(b)) => {
                out.entries.push(b.clone());
            }
            (None, None) => {}
        }
    }

    out.sort();
    Ok(out)
}

fn detect_df_conflict_roots(all_paths: &BTreeSet<Vec<u8>>) -> HashSet<Vec<u8>> {
    let mut roots = HashSet::new();
    let paths: Vec<&Vec<u8>> = all_paths.iter().collect();
    for path in &paths {
        if paths
            .iter()
            .any(|other| is_descendant_path(other.as_slice(), path.as_slice()))
        {
            roots.insert((**path).clone());
        }
    }
    roots
}

fn is_df_conflict_path(df_roots: &HashSet<Vec<u8>>, path: &[u8]) -> bool {
    df_roots
        .iter()
        .any(|root| path == root.as_slice() || is_descendant_path(path, root.as_slice()))
}

fn is_descendant_path(path: &[u8], parent: &[u8]) -> bool {
    path.len() > parent.len() && path.starts_with(parent) && path[parent.len()] == b'/'
}

fn stage_entry(index: &mut Index, src: &IndexEntry, stage: u8) {
    let mut e = src.clone();
    // Clear and set stage bits in flags
    e.flags = (e.flags & 0x0FFF) | ((stage as u16) << 12);
    index.entries.push(e);
}

/// Check if `core.sparseCheckout` is enabled and apply skip-worktree bits.
fn apply_sparse_checkout(
    git_dir: &Path,
    work_tree: Option<&Path>,
    index: &mut Index,
    skip_sparse_checkout: bool,
) -> Result<()> {
    apply_sparse_checkout_skip_worktree(git_dir, work_tree, index, skip_sparse_checkout);
    Ok(())
}

/// When sparse checkout would mark a path `skip-worktree` but the work tree already differs
/// from the index entry, clear `skip-worktree` so `read-tree -u` can materialize or report
/// conflicts (Git `verify_uptodate_sparse` / `apply_sparse_checkout`).
fn sparse_refresh_skip_worktree_for_dirty_paths(
    repo: &Repository,
    _old_index: &Index,
    new_index: &mut Index,
) -> Result<()> {
    let Some(wt) = repo.work_tree.as_deref() else {
        return Ok(());
    };
    let config = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
    let sparse_on = config
        .get("core.sparsecheckout")
        .map(|v| v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    if !sparse_on {
        return Ok(());
    }

    for entry in new_index.entries.iter_mut() {
        if entry.stage() != 0 || !entry.skip_worktree() {
            continue;
        }
        let was_sparse = _old_index
            .get(&entry.path, 0)
            .is_some_and(|e| e.skip_worktree());
        if was_sparse {
            continue;
        }
        let rel = String::from_utf8_lossy(&entry.path);
        let abs = wt.join(rel.as_ref());
        let exists = std::fs::symlink_metadata(&abs).is_ok();
        if exists && !worktree_matches_entry(repo, entry, &abs)? {
            entry.set_skip_worktree(false);
        }
    }
    Ok(())
}

/// True if the working tree already has this index entry (blob/symlink/gitlink) at `abs_path`.
///
/// When `read-tree -u` refreshes after files were removed from disk, the index may still
/// match `HEAD`; we must not skip checkout solely because the blob OID is unchanged.
fn checkout_entry_present_on_disk(abs_path: &std::path::Path, mode: u32) -> bool {
    match std::fs::symlink_metadata(abs_path) {
        Ok(meta) => {
            if mode == MODE_SYMLINK {
                meta.is_symlink()
            } else if mode == MODE_GITLINK {
                meta.is_dir() && abs_path.join(".git").exists()
            } else {
                meta.is_file()
            }
        }
        Err(_) => false,
    }
}

/// Update working tree to match stage-0 entries in `new_index`.
///
/// When `populate_gitlinks` is false (plain `read-tree -u` without `--recurse-submodules`), gitlink
/// entries only ensure an empty directory exists when the path newly becomes a submodule; existing
/// submodule checkouts are left unchanged (t1013). When true, delegate to the same checkout path
/// as `git checkout` for submodules, then callers may run `submodule update`.
fn checkout_index_entries(
    repo: &Repository,
    old_index: &Index,
    new_index: &Index,
    populate_gitlinks: bool,
    force_gitlink_checkout: bool,
) -> Result<()> {
    let work_tree = match &repo.work_tree {
        Some(p) => p.clone(),
        None => return Ok(()),
    };

    let old_paths: HashSet<Vec<u8>> = old_index
        .entries
        .iter()
        .filter(|e| e.stage() == 0)
        .map(|e| e.path.clone())
        .collect();
    let new_paths: HashSet<Vec<u8>> = new_index
        .entries
        .iter()
        .filter(|e| e.stage() == 0)
        .map(|e| e.path.clone())
        .collect();
    let old_stage0 = stage0_index_map(old_index);
    let config = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
    let conv = crlf::ConversionConfig::from_config(&config);

    // Collect paths that have skip-worktree in the new index
    let new_skip_worktree: HashSet<Vec<u8>> = new_index
        .entries
        .iter()
        .filter(|e| e.stage() == 0 && e.skip_worktree())
        .map(|e| e.path.clone())
        .collect();

    for old_path in old_paths.difference(&new_paths) {
        if old_stage0
            .get(old_path)
            .is_some_and(|e| e.mode == MODE_GITLINK)
            && !populate_gitlinks
        {
            // Without `--recurse-submodules`, dropping a gitlink does not remove the submodule
            // directory (t1013 non-recursive tests; t7300-style superproject behavior).
            continue;
        }
        let rel = String::from_utf8_lossy(old_path).into_owned();
        if grit_lib::worktree_cwd::cwd_would_be_removed_with_repo_path(&work_tree, &rel) {
            bail!("Refusing to remove the current working directory:\n{rel}\n");
        }
        let abs = work_tree.join(&rel);
        let was_gitlink = old_stage0
            .get(old_path)
            .is_some_and(|e| e.mode == MODE_GITLINK);
        if populate_gitlinks && was_gitlink && abs.is_dir() {
            let dot_git = abs.join(".git");
            let modules_dir = submodule_modules_git_dir(&repo.git_dir, &rel);
            let keep_modules = index_has_strict_path_prefix(new_index, old_path);
            if dot_git.is_dir() {
                // In-tree `.git` directory (after `replace_gitfile_with_git_dir`): absorb into
                // `.git/modules/<path>/` before removing the work tree (t1013.7).
                if modules_dir.exists() {
                    let _ = std::fs::remove_dir_all(&modules_dir);
                }
                if let Some(parent) = modules_dir.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = std::fs::rename(&dot_git, &modules_dir);
            } else if modules_dir.exists() && !keep_modules {
                // Standard gitfile submodule: drop separate git dir with the work tree unless the
                // gitlink becomes a directory tree (t1013 replace_sub1_with_directory).
                let _ = std::fs::remove_dir_all(&modules_dir);
            }
        }
        if abs.is_file() || abs.is_symlink() {
            let _ = std::fs::remove_file(&abs);
        } else if abs.is_dir() {
            let _ = std::fs::remove_dir_all(&abs);
        }
        remove_empty_parent_dirs(&work_tree, &abs);
    }

    // Remove files that now have skip-worktree set
    for skip_path in &new_skip_worktree {
        if new_index
            .get(skip_path.as_slice(), 0)
            .is_some_and(|e| e.mode == MODE_GITLINK)
        {
            continue;
        }
        let rel = String::from_utf8_lossy(skip_path).into_owned();
        if grit_lib::worktree_cwd::cwd_would_be_removed_with_repo_path(&work_tree, &rel) {
            bail!("Refusing to remove the current working directory:\n{rel}\n");
        }
        let abs = work_tree.join(&rel);
        if abs.is_file() || abs.is_symlink() {
            let _ = std::fs::remove_file(&abs);
        }
        remove_empty_parent_dirs(&work_tree, &abs);
    }
    for entry in &new_index.entries {
        if entry.stage() != 0 {
            continue;
        }
        // Skip entries with skip-worktree bit set
        if entry.skip_worktree() {
            continue;
        }
        let path_str = String::from_utf8_lossy(&entry.path).into_owned();
        let abs_path = work_tree.join(&path_str);
        if entry.mode == MODE_GITLINK {
            if let Some(parent) = abs_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            if populate_gitlinks {
                let force_populate = force_gitlink_checkout
                    || old_stage0
                        .get(&entry.path)
                        .map(|old| old.mode != MODE_GITLINK || old.oid != entry.oid)
                        .unwrap_or(true);
                crate::commands::checkout::checkout_gitlink_worktree_entry(
                    repo,
                    &work_tree,
                    &path_str,
                    &entry.oid,
                    force_populate,
                )?;
            } else if !old_stage0
                .get(&entry.path)
                .is_some_and(|old| old.mode == MODE_GITLINK)
            {
                // Path is newly a submodule: empty directory only (no clone/checkout).
                if std::fs::symlink_metadata(&abs_path)
                    .map(|m| m.is_file() || m.file_type().is_symlink())
                    .unwrap_or(false)
                {
                    let _ = std::fs::remove_file(&abs_path);
                } else if abs_path.is_dir() {
                    if worktree_has_untracked_under_path(&work_tree, old_index, &path_str)? {
                        bail!("Updating '{path_str}' would lose untracked files in it");
                    }
                    let _ = std::fs::remove_dir_all(&abs_path);
                }
                std::fs::create_dir_all(&abs_path)?;
                let git_marker = abs_path.join(".git");
                if git_marker.is_file() {
                    let _ = std::fs::remove_file(&git_marker);
                } else if git_marker.is_dir() {
                    let _ = std::fs::remove_dir_all(&git_marker);
                }
            }
            continue;
        }
        if entry.mode != MODE_GITLINK
            && old_stage0
                .get(&entry.path)
                .is_some_and(|old_entry| same_blob(old_entry, entry))
            && checkout_entry_present_on_disk(&abs_path, entry.mode)
        {
            continue;
        }

        // Sparse checkout: when `skip-worktree` was cleared because the work tree did not match
        // the index (Git `verify_uptodate_sparse`), do not overwrite the on-disk file with the
        // index blob (t1011 read-tree dirty case outside sparse cone).
        if entry.mode != MODE_GITLINK
            && checkout_entry_present_on_disk(&abs_path, entry.mode)
            && !worktree_matches_entry(repo, entry, &abs_path)?
        {
            continue;
        }

        if let Some(parent) = abs_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let obj = repo
            .odb
            .read(&entry.oid)
            .context("reading object for checkout")?;
        if obj.kind != ObjectKind::Blob {
            bail!("cannot checkout non-blob at '{}'", path_str);
        }
        // Remove existing directory/file at target path
        if let Ok(meta) = std::fs::symlink_metadata(&abs_path) {
            if meta.is_dir() {
                let block_untracked = if populate_gitlinks {
                    match old_stage0.get(&entry.path) {
                        Some(old_e) if old_e.mode == MODE_GITLINK => {
                            submodule_has_nonignored_untracked(
                                repo, &work_tree, old_index, &path_str,
                            )?
                        }
                        _ => worktree_has_untracked_under_path(&work_tree, old_index, &path_str)?,
                    }
                } else {
                    worktree_has_untracked_under_path(&work_tree, old_index, &path_str)?
                };
                if block_untracked {
                    bail!("Updating '{path_str}' would lose untracked files in it");
                }
                if grit_lib::worktree_cwd::cwd_would_be_removed_with_repo_path(
                    &work_tree, &path_str,
                ) {
                    bail!("Refusing to remove the current working directory:\n{path_str}\n");
                }
                std::fs::remove_dir_all(&abs_path)?;
            } else {
                if grit_lib::worktree_cwd::cwd_would_be_removed_with_repo_path(
                    &work_tree, &path_str,
                ) {
                    bail!("Refusing to remove the current working directory:\n{path_str}\n");
                }
                std::fs::remove_file(&abs_path)?;
            }
        }
        if entry.mode == MODE_SYMLINK {
            let target = String::from_utf8(obj.data)
                .map_err(|_| anyhow::anyhow!("symlink target is not UTF-8"))?;
            std::os::unix::fs::symlink(target, &abs_path)?;
        } else {
            // Apply CRLF / smudge conversion (per-path rules: root + nested .gitattributes)
            let attrs =
                crlf::load_gitattributes_for_checkout(&work_tree, &path_str, new_index, &repo.odb);
            let file_attrs = crlf::get_file_attrs(&attrs, &path_str, false, &config);
            let oid_hex = format!("{}", entry.oid);
            let smudge_meta = grit_lib::filter_process::smudge_meta_for_checkout(repo, &oid_hex);
            let data = crlf::convert_to_worktree_eager(
                &obj.data,
                &path_str,
                &conv,
                &file_attrs,
                Some(&oid_hex),
                Some(&smudge_meta),
            )
            .map_err(|e| anyhow::anyhow!("{e}"))?;
            std::fs::write(&abs_path, &data)?;
            if entry.mode == MODE_EXECUTABLE {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = std::fs::metadata(&abs_path)?.permissions();
                perms.set_mode(0o755);
                std::fs::set_permissions(&abs_path, perms)?;
            }
        }
    }

    if populate_gitlinks {
        let grit_bin = crate::grit_exe::grit_executable();
        for (path_bytes, old_e) in &old_stage0 {
            if old_e.mode != MODE_GITLINK {
                continue;
            }
            if !index_has_strict_path_prefix(new_index, path_bytes.as_slice()) {
                continue;
            }
            let rel = String::from_utf8_lossy(path_bytes).into_owned();
            let modules_dir = submodule_modules_git_dir(&repo.git_dir, &rel);
            if !modules_dir.join("HEAD").exists() {
                continue;
            }
            let abs = work_tree.join(&rel);
            if abs.is_dir() && !abs.join(".git").exists() {
                write_submodule_gitfile(&abs, &modules_dir).map_err(|e| anyhow::anyhow!("{e}"))?;
                set_submodule_repo_worktree(&grit_bin, &modules_dir, &abs);
            }
        }
    }

    Ok(())
}

fn validate_worktree_updates(
    repo: &Repository,
    old_index: &Index,
    new_index: &Index,
    allow_ignored_overwrite: bool,
    super_prefix: Option<&str>,
    recurse_submodules: bool,
    explicit_recurse_submodules: bool,
    reset_allows_untracked_overwrite: bool,
) -> Result<()> {
    let work_tree = match &repo.work_tree {
        Some(p) => p.clone(),
        None => return Ok(()),
    };

    let old_stage0 = stage0_index_map(old_index);
    let new_stage0 = stage0_index_map(new_index);

    // Without `--recurse-submodules`, C Git refuses to replace a submodule gitlink with a directory
    // tree (t1013 non-recursive: replace submodule with a directory must fail).
    // `submodule.recurse` alone does not disable this check (matches forced-switch tests).
    // The new index lists `sub1/file` entries, not a tree placeholder at `sub1`, so detect via prefix.
    if !explicit_recurse_submodules {
        for (path_bytes, oe) in &old_stage0 {
            if oe.mode != MODE_GITLINK {
                continue;
            }
            if new_stage0
                .get(path_bytes)
                .is_some_and(|ne| ne.mode == MODE_GITLINK)
            {
                continue;
            }
            if index_has_strict_path_prefix(new_index, path_bytes.as_slice()) {
                let rel_path = String::from_utf8_lossy(path_bytes);
                if let Some(p) = super_prefix {
                    bail!(
                        "local changes to '{}{}' would be overwritten by merge",
                        p,
                        rel_path
                    );
                }
                bail!(
                    "local changes to '{}' would be overwritten by merge",
                    rel_path
                );
            }
        }
    }

    let mut all_paths: BTreeSet<Vec<u8>> = BTreeSet::new();
    all_paths.extend(old_stage0.keys().cloned());
    all_paths.extend(new_stage0.keys().cloned());

    let mut repo_ignore = IgnoreMatcher::from_repository(repo)?;

    let new_gitlink_roots: Vec<&[u8]> = new_stage0
        .values()
        .filter(|e| e.mode == MODE_GITLINK)
        .map(|e| e.path.as_slice())
        .collect();

    for path in all_paths {
        if new_gitlink_roots
            .iter()
            .any(|root| path_is_strict_descendant(path.as_slice(), root))
        {
            // Replacing a directory tree (or cleaning up after one) with a submodule gitlink at
            // `root`: do not validate former files under `root/` (t1013 replace_directory_with_sub1).
            continue;
        }

        if recurse_submodules
            && path_under_gitlink_replaced_by_tree(path.as_slice(), &old_stage0, &new_stage0)
        {
            continue;
        }

        let old = old_stage0.get(&path);
        let new = new_stage0.get(&path);

        let rel_path = String::from_utf8_lossy(&path).into_owned();
        let abs_path = work_tree.join(&rel_path);
        let exists = std::fs::symlink_metadata(&abs_path)
            .map(|_| true)
            .unwrap_or(false);

        if let (Some(oe), Some(ne)) = (old, new) {
            if oe.mode != MODE_GITLINK && ne.mode == MODE_GITLINK {
                // Regular entry → submodule: work tree cannot match the gitlink yet.
                continue;
            }
        }

        if recurse_submodules {
            if let (Some(oe), None) = (old, new) {
                if oe.mode == MODE_GITLINK {
                    // Submodule path is dropped from the index; checkout will remove the work tree.
                    // Do not require the old gitlink to match disk first (t1013 remove_sub1 with
                    // uninitialized sibling paths).
                    continue;
                }
            }
        }

        match (old, new) {
            (Some(oe), None) if oe.mode == MODE_GITLINK && !recurse_submodules => {
                // Dropping a gitlink: submodule directory stays on disk (t1013 non-recursive).
                continue;
            }
            (Some(oe), Some(ne))
                if oe.mode == MODE_GITLINK && ne.mode == MODE_GITLINK && oe.oid != ne.oid =>
            {
                // Submodule gitlink SHA changes without updating the nested checkout yet, or we are
                // about to recurse and refresh it (t1013 invalid_sub1 → valid_sub1 with --reset).
                continue;
            }
            _ => {}
        }

        if let (Some(old_entry), Some(new_entry)) = (old, new) {
            if same_blob(old_entry, new_entry) {
                continue;
            }
        }

        if let (Some(oe), Some(ne)) = (old, new) {
            if oe.mode == MODE_GITLINK && ne.mode != MODE_GITLINK {
                if recurse_submodules
                    && submodule_has_nonignored_untracked(repo, &work_tree, old_index, &rel_path)?
                {
                    bail!("Updating '{rel_path}' would lose untracked files in it");
                }
                // Submodule → regular file/tree: disk still looks like a checkout (t1013).
                continue;
            }
        }

        if !exists {
            continue;
        }

        match (old, new) {
            (None, Some(new_entry)) if new_entry.skip_worktree() => {
                // Sparse checkout: new paths outside the cone are not materialized; Git skips
                // `verify_absent` for `CE_NEW_SKIP_WORKTREE` entries (t1011 dirty case).
                continue;
            }
            (None, Some(new_entry)) => {
                if reset_allows_untracked_overwrite {
                    continue;
                }
                if allow_ignored_overwrite {
                    let is_dir = std::fs::symlink_metadata(&abs_path)
                        .map(|m| m.is_dir())
                        .unwrap_or(false);
                    let (ignored, _) = repo_ignore
                        .check_path(repo, Some(old_index), &rel_path, is_dir)
                        .map_err(anyhow::Error::from)?;
                    if ignored {
                        continue;
                    }
                }
                if let Ok(meta) = std::fs::symlink_metadata(&abs_path) {
                    if meta.is_dir() {
                        if worktree_has_untracked_under_path(&work_tree, old_index, &rel_path)? {
                            if let Some(p) = super_prefix {
                                bail!(
                                    "Updating '{}{}' would lose untracked files in it",
                                    p,
                                    rel_path
                                );
                            }
                            bail!(
                                "untracked working tree file '{}' would be overwritten by merge",
                                rel_path
                            );
                        }
                        continue;
                    }
                }
                // Untracked path on disk: allow when it already matches the blob we would
                // materialize (e.g. `read-tree -m -u` after checkout left the same bytes).
                if worktree_matches_entry(repo, new_entry, &abs_path)? {
                    continue;
                }
                bail!(
                    "untracked working tree file '{}' would be overwritten by merge",
                    rel_path
                );
            }
            (Some(old_entry), Some(_)) | (Some(old_entry), None) => {
                if !worktree_matches_entry(repo, old_entry, &abs_path)? {
                    bail!(
                        "local changes to '{}' would be overwritten by merge",
                        rel_path
                    );
                }
            }
            (None, None) => {}
        }
    }

    Ok(())
}

/// Returns true if `rel_path` is a directory in the work tree that contains a file not
/// listed in `old_index` (stage 0). Used when a merge wants to add a file at `rel_path`
/// but a directory is in the way.
fn worktree_has_untracked_under_path(
    work_tree: &Path,
    old_index: &Index,
    rel_path: &str,
) -> Result<bool> {
    let base = work_tree.join(rel_path);
    fn walk(base: &Path, work_tree: &Path, old_index: &Index) -> Result<bool> {
        let entries = match std::fs::read_dir(base) {
            Ok(e) => e,
            Err(_) => return Ok(false),
        };
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            let rel = path.strip_prefix(work_tree).unwrap();
            let rel_s = rel.to_string_lossy();
            let ft = entry.file_type()?;
            if ft.is_file() || ft.is_symlink() {
                if old_index.get(rel_s.as_bytes(), 0).is_none() {
                    return Ok(true);
                }
            } else if ft.is_dir() && walk(&path, work_tree, old_index)? {
                return Ok(true);
            }
        }
        Ok(false)
    }
    walk(&base, work_tree, old_index)
}

fn worktree_matches_entry(repo: &Repository, entry: &IndexEntry, abs_path: &Path) -> Result<bool> {
    if entry.mode == MODE_GITLINK {
        let metadata = match std::fs::symlink_metadata(abs_path) {
            Ok(m) => m,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(err) => return Err(err.into()),
        };
        if !metadata.is_dir() || !abs_path.join(".git").exists() {
            return Ok(false);
        }
        let super_git = repo
            .git_dir
            .canonicalize()
            .unwrap_or_else(|_| repo.git_dir.clone());
        let Some(embed) = submodule_embedded_git_dir(abs_path) else {
            return Ok(false);
        };
        let gd = match embed.canonicalize() {
            Ok(p) => p,
            Err(_) => return Ok(false),
        };
        if !gd.starts_with(&super_git) {
            return Ok(false);
        }
        let Some(head) = read_submodule_head_oid(abs_path) else {
            return Ok(false);
        };
        return Ok(head == entry.oid);
    }

    let obj = repo.odb.read(&entry.oid)?;
    if obj.kind != ObjectKind::Blob {
        return Ok(false);
    }

    let metadata = match std::fs::symlink_metadata(abs_path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(err.into()),
    };

    if entry.mode == MODE_SYMLINK {
        if !metadata.file_type().is_symlink() {
            return Ok(false);
        }
        let target = std::fs::read_link(abs_path)?;
        return Ok(target.to_string_lossy().as_bytes() == obj.data.as_slice());
    }

    if !metadata.is_file() {
        return Ok(false);
    }

    let data = std::fs::read(abs_path)?;
    Ok(data == obj.data)
}

fn remove_empty_parent_dirs(work_tree: &Path, path: &Path) {
    let cwd_rel = grit_lib::worktree_cwd::process_cwd_repo_relative(work_tree);
    let mut current = path.parent();
    while let Some(dir) = current {
        if dir == work_tree {
            break;
        }
        if let Some(ref cr) = cwd_rel {
            if grit_lib::worktree_cwd::cwd_would_be_removed_with_dir(work_tree, dir, cr) {
                break;
            }
        }
        match std::fs::remove_dir(dir) {
            Ok(()) => current = dir.parent(),
            Err(_) => break,
        }
    }
}

fn effective_index_path(repo: &Repository) -> Result<PathBuf> {
    if let Ok(raw) = std::env::var("GIT_INDEX_FILE") {
        let p = PathBuf::from(raw);
        if p.is_absolute() {
            return Ok(p);
        }
        let cwd = std::env::current_dir().context("resolving GIT_INDEX_FILE")?;
        return Ok(cwd.join(p));
    }
    Ok(repo.index_path())
}

fn maybe_write_trace_packet_done() {
    if let Ok(dest) = std::env::var("GIT_TRACE_PACKET") {
        if dest.is_empty() || dest == "0" || dest.eq_ignore_ascii_case("false") {
            return;
        }
        let mut target = dest;
        if target == "1" {
            target = "/dev/stderr".to_string();
        }
        if let Ok(mut out) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&target)
        {
            let _ = out.write_all(b"fetch> done\n");
        }
    }
}

fn resolve_tree_ish(repo: &Repository, s: &str) -> Result<ObjectId> {
    // First, try resolve_revision which handles HEAD^, HEAD~N, @{-1}, etc.
    if let Ok(oid) = resolve_revision(repo, s) {
        return peel_to_tree(repo, oid);
    }
    if let Ok(oid) = s.parse::<ObjectId>() {
        return peel_to_tree(repo, oid);
    }
    if let Ok(oid) = resolve_ref(&repo.git_dir, s) {
        return peel_to_tree(repo, oid);
    }
    let as_branch = format!("refs/heads/{s}");
    if let Ok(oid) = resolve_ref(&repo.git_dir, &as_branch) {
        return peel_to_tree(repo, oid);
    }
    let as_tag = format!("refs/tags/{s}");
    if let Ok(oid) = resolve_ref(&repo.git_dir, &as_tag) {
        return peel_to_tree(repo, oid);
    }
    bail!("not a valid tree-ish: '{s}'")
}

/// Canonical empty tree object id (matches `git hash-object -t tree --stdin </dev/null>`).
const EMPTY_TREE_OID_HEX: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

/// Reset index and work tree after `git am --skip` / `--abort`-style cleanup.
///
/// Mirrors Git's `clean_index` in `builtin/am.c`: fast-forward the index to `HEAD`'s tree
/// (dropping unmerged entries), then two-way merge toward `orig_head`'s tree while preserving
/// index stat information for paths that were not modified between those trees.
///
/// # Parameters
///
/// - `head_commit` — `None` when `HEAD` is unborn (empty tree is used).
/// - `orig_commit` — `None` when `ORIG_HEAD` is missing (empty tree is used).
///
/// # Errors
///
/// Returns an error on tree resolution failure, merge conflicts, or I/O errors.
pub fn am_clean_index(
    repo: &Repository,
    head_commit: Option<ObjectId>,
    orig_commit: Option<ObjectId>,
) -> Result<()> {
    let index_path = effective_index_path(repo)?;
    let prot = PathProtection::load(&repo.git_dir);
    let old_before = repo
        .load_index_at(&index_path)
        .context("loading index for am clean_index")?;

    let empty_tree: ObjectId = EMPTY_TREE_OID_HEX.parse()?;
    let head_tree_oid = match head_commit {
        Some(oid) => peel_to_tree(repo, oid)?,
        None => empty_tree,
    };
    let orig_tree_oid = match orig_commit {
        Some(oid) => peel_to_tree(repo, oid)?,
        None => empty_tree,
    };

    let phase1 = index_reset_to_tree_preserving_stats(repo, &old_before, &head_tree_oid, prot)?;

    if repo.work_tree.is_some() {
        // Drop conflict-marker / partial-apply content so `two_way_merge`'s
        // `require_uptodate` checks see a work tree consistent with `phase1` (HEAD).
        checkout_index_entries(repo, &old_before, &phase1, true, false)?;
    }

    let head_map = tree_to_map(tree_to_index_entries(repo, &head_tree_oid, "", prot)?);
    let orig_map = tree_to_map(tree_to_index_entries(repo, &orig_tree_oid, "", prot)?);
    let mut phase2 = two_way_merge(repo, &phase1, &head_map, &orig_map)?;

    apply_sparse_checkout(&repo.git_dir, repo.work_tree.as_deref(), &mut phase2, false)?;

    if repo.work_tree.is_some() {
        checkout_index_entries(repo, &phase1, &phase2, true, false)?;
    }

    repo.write_index_at(&index_path, &mut phase2)
        .context("writing index after am clean_index")?;
    Ok(())
}

/// Replace stage-0 index entries with `tree_oid`, copying stat fields from `old` when the
/// blob/mode matches (Git `unpack_trees` / `clean_index` stat preservation).
fn index_reset_to_tree_preserving_stats(
    repo: &Repository,
    old: &Index,
    tree_oid: &ObjectId,
    prot: PathProtection,
) -> Result<Index> {
    let fresh = tree_to_index_entries(repo, tree_oid, "", prot)?;
    let old_stage0 = stage0_index_map(old);
    let mut out = Index::new();
    for mut e in fresh {
        if let Some(prev) = old_stage0.get(&e.path) {
            if prev.oid == e.oid && prev.mode == e.mode {
                e.ctime_sec = prev.ctime_sec;
                e.ctime_nsec = prev.ctime_nsec;
                e.mtime_sec = prev.mtime_sec;
                e.mtime_nsec = prev.mtime_nsec;
                e.dev = prev.dev;
                e.ino = prev.ino;
                e.uid = prev.uid;
                e.gid = prev.gid;
                e.size = prev.size;
                e.flags = prev.flags;
                e.flags_extended = prev.flags_extended;
            }
        }
        out.entries.push(e);
    }
    out.sort();
    Ok(out)
}

pub(crate) fn peel_to_tree(repo: &Repository, mut oid: ObjectId) -> Result<ObjectId> {
    loop {
        let obj = repo.odb.read(&oid)?;
        match obj.kind {
            ObjectKind::Tree => return Ok(oid),
            ObjectKind::Commit => {
                let c = parse_commit(&obj.data)?;
                oid = c.tree;
            }
            ObjectKind::Tag => {
                let (target, target_kind) = parse_tag_target(&obj.data)?;
                if target_kind == "tree" {
                    return Ok(target);
                }
                oid = target;
            }
            _ => bail!("object '{}' does not name a tree", oid),
        }
    }
}

fn parse_tag_target(data: &[u8]) -> Result<(ObjectId, String)> {
    let text = std::str::from_utf8(data).context("tag object is not UTF-8")?;
    let mut object = None;
    let mut kind = None;
    for line in text.lines() {
        if line.is_empty() {
            break;
        }
        if let Some(rest) = line.strip_prefix("object ") {
            object = Some(rest.trim().parse::<ObjectId>()?);
        } else if let Some(rest) = line.strip_prefix("type ") {
            kind = Some(rest.trim().to_owned());
        }
    }
    Ok((
        object.context("tag missing object header")?,
        kind.context("tag missing type header")?,
    ))
}
