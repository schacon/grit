//! `grit add` — add file contents to the index.
//!
//! Stages files from the working tree into the index so they will be
//! included in the next commit.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::config::ConfigSet;
use grit_lib::crlf::{self, ConversionConfig, GitAttributes};
use grit_lib::diff::stat_matches;
use grit_lib::ignore::IgnoreMatcher;
use grit_lib::index::{entry_from_metadata, normalize_mode, Index, IndexEntry};
#[allow(unused_imports)]
use grit_lib::objects::ObjectId;
use grit_lib::objects::ObjectKind;
use grit_lib::odb::Odb;
use grit_lib::repo::Repository;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

/// Arguments for `grit add`.
#[derive(Debug, ClapArgs)]
#[command(about = "Add file contents to the index")]
pub struct Args {
    /// Files to add. Use '.' to add everything.
    pub pathspec: Vec<String>,

    /// Update tracked files (don't add new files).
    #[arg(short = 'u', long = "update")]
    pub update: bool,

    /// Add, modify, and remove index entries to match the working tree.
    #[arg(short = 'A', long = "all", alias = "no-ignore-removal")]
    pub all: bool,

    /// Record only the intent to add a path (placeholder entry).
    #[arg(short = 'N', long = "intent-to-add")]
    pub intent_to_add: bool,

    /// Dry run — show what would be added.
    #[arg(short = 'n', long = "dry-run")]
    pub dry_run: bool,

    /// Be verbose.
    #[arg(short = 'v', long = "verbose")]
    pub verbose: bool,

    /// Allow adding otherwise ignored files.
    #[arg(short = 'f', long = "force")]
    pub force: bool,

    /// Interactive patch mode.
    #[arg(short = 'p', long = "patch")]
    pub patch: bool,

    /// Interactive add mode.
    #[arg(short = 'i', long = "interactive")]
    pub interactive: bool,

    /// Edit the diff vs. the index before staging.
    #[arg(short = 'e', long = "edit")]
    pub edit: bool,

    /// Override the file mode for the added files (+x or -x).
    #[arg(long = "chmod")]
    pub chmod: Option<String>,

    /// Renormalize tracked files (apply clean/smudge filters).
    #[arg(long = "renormalize")]
    pub renormalize: bool,

    /// Refresh stat info in the index without changing content.
    #[arg(long = "refresh")]
    pub refresh: bool,

    /// Continue adding files when some cannot be added.
    #[arg(long = "ignore-errors")]
    pub ignore_errors: bool,

    /// Suppress warning for non-existent pathspecs (with --refresh).
    #[arg(long = "ignore-missing")]
    pub ignore_missing: bool,

    /// Suppress warning for adding an embedded repository.
    #[arg(long = "no-warn-embedded-repo")]
    pub no_warn_embedded_repo: bool,

    /// Read pathspecs from a file (one per line).
    #[arg(long = "pathspec-from-file", value_name = "FILE")]
    pub pathspec_from_file: Option<PathBuf>,
}

/// Run the `add` command.
pub fn run(mut args: Args) -> Result<()> {
    // --pathspec-from-file: read pathspecs from a file and append to pathspec list
    if let Some(ref file) = args.pathspec_from_file {
        let content = fs::read_to_string(file)
            .with_context(|| format!("cannot read pathspec file '{}'", file.display()))?;
        for line in content.lines() {
            let line = line.trim();
            if !line.is_empty() {
                args.pathspec.push(line.to_owned());
            }
        }
    }

    // --dry-run is incompatible with interactive modes
    if args.dry_run && (args.interactive || args.patch) {
        bail!("options '--dry-run' and '--interactive'/'--patch' cannot be used together");
    }

    // Stubs for unsupported interactive modes — accept the flags
    // gracefully so scripts that pass them don't hard-fail.
    if args.patch || args.interactive || args.edit {
        // Real git would enter interactive mode; we just warn and succeed.
        let mode_name = if args.patch {
            "-p/--patch"
        } else if args.interactive {
            "-i/--interactive"
        } else {
            "-e/--edit"
        };
        eprintln!(
            "warning: {} mode is not yet implemented; doing nothing",
            mode_name
        );
        return Ok(());
    }

    let repo = Repository::discover(None).context("not a git repository")?;
    let work_tree = repo
        .work_tree
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("this operation must be run in a work tree"))?;

    let config = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
    let core_filemode = config
        .get_bool("core.filemode")
        .and_then(|r| r.ok())
        .unwrap_or(true);

    let index_path = repo.index_path();
    let idx_exists = index_path.exists();
    let _cfg_ver = config.get("index.version");
    let _cfg_many = config.get("feature.manyFiles");
    let mut index = if idx_exists {
        Index::load(&index_path)?
    } else {
        Index::new_with_config(
            config.get("index.version").as_deref(),
            config.get("feature.manyFiles").as_deref(),
        )
    };

    let odb = &repo.odb;

    // Resolve the current working directory relative to the worktree
    let cwd = std::env::current_dir()?;
    let prefix = pathdiff(&cwd, work_tree);

    // Validate empty string pathspecs
    for ps in &args.pathspec {
        if ps.is_empty() {
            bail!("invalid path ''");
        }
    }

    // "git add" with no pathspecs and no flags: give advice
    if args.pathspec.is_empty()
        && !args.all
        && !args.update
        && !args.refresh
        && args.chmod.is_none()
    {
        eprintln!("Nothing specified, nothing added.");
        eprintln!("hint: Maybe you wanted to say 'git add .'?");
        eprintln!(
            "hint: Disable this message with \"git config set advice.addEmptyPathspec false\""
        );
        return Ok(());
    }

    // --refresh mode
    if args.refresh {
        return run_refresh(&repo, &mut index, work_tree, prefix.as_deref(), &args);
    }

    // --chmod with no pathspecs: do nothing (don't error, just return)
    if args.chmod.is_some() && args.pathspec.is_empty() {
        if !args.dry_run {
            index.write(&repo.index_path())?;
        }
        return Ok(());
    }

    // Build ignore matcher if needed (not needed with --force)
    let mut ignore_matcher = if !args.force {
        Some(IgnoreMatcher::from_repository(&repo)?)
    } else {
        None
    };

    let conv = ConversionConfig::from_config(&config);
    let attrs = crlf::load_gitattributes(work_tree);

    let add_cfg = AddConfig {
        core_filemode,
        ignore_errors: args.ignore_errors
            || config
                .get_bool("add.ignore-errors")
                .and_then(|r| r.ok())
                .unwrap_or(false),
        conv,
        attrs,
        config: config.clone(),
    };

    // --renormalize: re-apply clean conversion to tracked files
    if args.renormalize {
        return run_renormalize(
            odb,
            &mut index,
            work_tree,
            prefix.as_deref(),
            &args,
            &add_cfg,
        );
    }

    if args.all || args.pathspec.iter().any(|p| p == ".") {
        add_all(
            odb,
            &mut index,
            work_tree,
            prefix.as_deref(),
            &args,
            &repo,
            &mut ignore_matcher,
            &add_cfg,
        )?;
    } else if args.update {
        update_tracked(
            odb,
            &mut index,
            work_tree,
            prefix.as_deref(),
            &args,
            &add_cfg,
        )?;
    } else {
        let mut had_errors = false;
        let mut had_ignored = false;
        for pathspec in &args.pathspec {
            let resolved = resolve_pathspec(pathspec, work_tree, prefix.as_deref());
            // Expand glob patterns (e.g. "file?.t", "*.c") against the working tree.
            let expanded = expand_glob_pathspec(&resolved, work_tree);
            for resolved in &expanded {
                match add_path(
                    odb,
                    &mut index,
                    work_tree,
                    resolved,
                    &args,
                    &repo,
                    &mut ignore_matcher,
                    &add_cfg,
                ) {
                    Ok(()) => {}
                    Err(AddPathError::Ignored(msg)) => {
                        eprintln!("{msg}");
                        had_ignored = true;
                        had_errors = true;
                    }
                    Err(AddPathError::IoError(e)) => {
                        if add_cfg.ignore_errors {
                            eprintln!("warning: {e}");
                            had_errors = true;
                        } else {
                            // Write index even on error if we've done partial work
                            return Err(e);
                        }
                    }
                    Err(AddPathError::Other(e)) => {
                        if add_cfg.ignore_errors {
                            eprintln!("warning: {e}");
                            had_errors = true;
                        } else {
                            return Err(e);
                        }
                    }
                }
            } // end expanded loop
        }

        if had_ignored {
            if !args.dry_run {
                index.write(&repo.index_path())?;
            }
            bail!("some ignored files could not be added");
        }
        if had_errors && !add_cfg.ignore_errors {
            if !args.dry_run {
                index.write(&repo.index_path())?;
            }
            bail!("adding files failed");
        }
    }

    if !args.dry_run {
        index.write(&repo.index_path())?;
    }

    Ok(())
}

struct AddConfig {
    core_filemode: bool,
    ignore_errors: bool,
    conv: ConversionConfig,
    attrs: GitAttributes,
    config: ConfigSet,
}

#[allow(dead_code)]
enum AddPathError {
    Ignored(String),
    IoError(anyhow::Error),
    Other(anyhow::Error),
}

impl From<anyhow::Error> for AddPathError {
    fn from(e: anyhow::Error) -> Self {
        AddPathError::Other(e)
    }
}

/// Run --refresh: update stat info in the index.
fn run_refresh(
    repo: &Repository,
    index: &mut Index,
    work_tree: &Path,
    prefix: Option<&str>,
    args: &Args,
) -> Result<()> {
    if args.pathspec.is_empty() {
        // Refresh all entries
        for ie in &mut index.entries {
            let path_str = String::from_utf8_lossy(&ie.path).to_string();
            if let Some(p) = prefix {
                if !path_str.starts_with(p) {
                    continue;
                }
            }
            let abs_path = work_tree.join(&path_str);
            if let Ok(meta) = fs::symlink_metadata(&abs_path) {
                // Update stat fields but keep oid/mode
                ie.ctime_sec = meta.ctime() as u32;
                ie.ctime_nsec = meta.ctime_nsec() as u32;
                ie.mtime_sec = meta.mtime() as u32;
                ie.mtime_nsec = meta.mtime_nsec() as u32;
                ie.dev = meta.dev() as u32;
                ie.ino = meta.ino() as u32;
                ie.uid = meta.uid();
                ie.gid = meta.gid();
                ie.size = meta.len() as u32;
            }
        }
    } else {
        for pathspec in &args.pathspec {
            let resolved = resolve_pathspec(pathspec, work_tree, prefix);
            let found = index.entries.iter_mut().any(|ie| {
                let path_str = String::from_utf8_lossy(&ie.path);
                if path_str == resolved || path_str.starts_with(&format!("{resolved}/")) {
                    let abs_path = work_tree.join(path_str.as_ref());
                    if let Ok(meta) = fs::symlink_metadata(&abs_path) {
                        ie.ctime_sec = meta.ctime() as u32;
                        ie.ctime_nsec = meta.ctime_nsec() as u32;
                        ie.mtime_sec = meta.mtime() as u32;
                        ie.mtime_nsec = meta.mtime_nsec() as u32;
                        ie.dev = meta.dev() as u32;
                        ie.ino = meta.ino() as u32;
                        ie.uid = meta.uid();
                        ie.gid = meta.gid();
                        ie.size = meta.len() as u32;
                    }
                    true
                } else {
                    false
                }
            });
            if !found && !args.ignore_missing {
                bail!("pathspec '{}' did not match any files", pathspec);
            }
        }
    }

    if !args.dry_run {
        index.write(&repo.index_path())?;
    }

    Ok(())
}

/// Re-apply clean conversion (CRLF normalization) to tracked files.
fn run_renormalize(
    odb: &Odb,
    index: &mut Index,
    work_tree: &Path,
    _prefix: Option<&str>,
    args: &Args,
    add_cfg: &AddConfig,
) -> Result<()> {
    // Reload gitattributes (may have been updated)
    let attrs = crlf::load_gitattributes(work_tree);

    // Collect paths to renormalize based on pathspecs
    let entries: Vec<(Vec<u8>, ObjectId, u32)> = index
        .entries
        .iter()
        .filter(|ie| {
            if ie.stage() != 0 {
                return false;
            }
            if args.pathspec.is_empty() {
                return true;
            }
            let path_str = String::from_utf8_lossy(&ie.path);
            args.pathspec.iter().any(|ps| {
                // Simple glob/prefix match
                let ps_clean = ps.trim_end_matches('*').trim_end_matches('/');
                path_str.starts_with(ps_clean) || glob_matches_simple(ps, &path_str)
            })
        })
        .map(|ie| (ie.path.clone(), ie.oid, ie.mode))
        .collect();

    for (path, oid, _mode) in entries {
        let rel_path = String::from_utf8_lossy(&path).to_string();
        let file_attrs = crlf::get_file_attrs(&attrs, &rel_path, &add_cfg.config);

        // Read current blob content
        let obj = odb.read(&oid).context("reading blob for renormalize")?;
        if obj.kind != ObjectKind::Blob {
            continue;
        }

        // Apply clean conversion
        let converted = match crlf::convert_to_git(&obj.data, &rel_path, &add_cfg.conv, &file_attrs)
        {
            Ok(c) => c,
            Err(_) => continue,
        };

        // If content changed, write new blob and update index
        if converted != obj.data {
            let new_oid = odb.write(ObjectKind::Blob, &converted)?;
            if let Some(entry) = index.get_mut(path.as_slice(), 0) {
                entry.oid = new_oid;
            }
        }
    }

    if !args.dry_run {
        index.write(&work_tree.join(".git/index"))?;
    }

    Ok(())
}

fn glob_matches_simple(pattern: &str, text: &str) -> bool {
    if !pattern.contains('*') && !pattern.contains('?') {
        return text == pattern || text.starts_with(&format!("{pattern}/"));
    }
    // Simple glob: *.txt
    if let Some(suffix) = pattern.strip_prefix('*') {
        return text.ends_with(suffix);
    }
    text == pattern
}

/// Add all files under the working tree (or a prefix) to the index.
fn add_all(
    odb: &Odb,
    index: &mut Index,
    work_tree: &Path,
    prefix: Option<&str>,
    args: &Args,
    repo: &Repository,
    ignore_matcher: &mut Option<IgnoreMatcher>,
    add_cfg: &AddConfig,
) -> Result<()> {
    let scan_root = match prefix {
        Some(p) if !p.is_empty() => work_tree.join(p),
        _ => work_tree.to_path_buf(),
    };

    let mut paths = Vec::new();
    walk_directory(
        &scan_root,
        work_tree,
        &mut paths,
        repo,
        ignore_matcher,
        args.force,
    )?;

    // Build a set of worktree paths for fast deletion detection
    let worktree_paths: std::collections::HashSet<&str> =
        paths.iter().map(|s| s.as_str()).collect();

    for rel_path in &paths {
        let abs_path = work_tree.join(rel_path);
        if let Err(e) = stage_file(odb, index, work_tree, rel_path, &abs_path, args, add_cfg) {
            if add_cfg.ignore_errors {
                eprintln!("warning: {e}");
            } else {
                return Err(e);
            }
        }
    }

    // Handle deletions: index entries whose files are not in the worktree scan
    let prefix_bytes = prefix.map(|p| p.as_bytes());
    let removed: Vec<Vec<u8>> = index
        .entries
        .iter()
        .filter(|ie| {
            if let Some(pb) = prefix_bytes {
                if !ie.path.starts_with(pb) {
                    return false;
                }
            }
            let path_str = std::str::from_utf8(&ie.path).unwrap_or("");
            !worktree_paths.contains(path_str)
        })
        .map(|ie| ie.path.clone())
        .collect();

    for path in removed {
        if args.verbose {
            let path_str = String::from_utf8_lossy(&path);
            eprintln!("remove '{path_str}'");
        }
        if !args.dry_run {
            index.remove(&path);
        }
    }

    Ok(())
}

/// Update only already-tracked files.
fn update_tracked(
    odb: &Odb,
    index: &mut Index,
    work_tree: &Path,
    prefix: Option<&str>,
    args: &Args,
    add_cfg: &AddConfig,
) -> Result<()> {
    let tracked: Vec<(Vec<u8>, String)> = index
        .entries
        .iter()
        .filter(|ie| {
            let path_str = String::from_utf8_lossy(&ie.path);
            prefix.map(|p| path_str.starts_with(p)).unwrap_or(true)
        })
        .map(|ie| {
            let path_str = String::from_utf8_lossy(&ie.path).to_string();
            (ie.path.clone(), path_str)
        })
        .collect();

    for (raw_path, path_str) in &tracked {
        let abs_path = work_tree.join(path_str);
        if abs_path.exists() {
            stage_file(odb, index, work_tree, path_str, &abs_path, args, add_cfg)?;
        } else {
            if args.verbose {
                eprintln!("remove '{path_str}'");
            }
            if !args.dry_run {
                index.remove(raw_path);
            }
        }
    }

    Ok(())
}

/// Add a single pathspec (which may be a file or directory).
fn add_path(
    odb: &Odb,
    index: &mut Index,
    work_tree: &Path,
    path: &str,
    args: &Args,
    repo: &Repository,
    ignore_matcher: &mut Option<IgnoreMatcher>,
    add_cfg: &AddConfig,
) -> std::result::Result<(), AddPathError> {
    let abs_path = work_tree.join(path);

    // Refuse to add a path inside a registered submodule (gitlink).
    // Only reject when a *proper* parent directory is a gitlink;
    // adding the submodule entry itself (e.g. `git add embed`) is fine.
    {
        let components: Vec<&str> = path.split('/').collect();
        let mut prefix = String::new();
        for &component in &components[..components.len().saturating_sub(1)] {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(component);
            if let Some(ie) = index.get(prefix.as_bytes(), 0) {
                if ie.mode == 0o160000 {
                    eprintln!("fatal: Pathspec '{}' is in submodule '{}'", path, prefix);
                    std::process::exit(128);
                }
            }
        }
    }

    // Refuse to add a path that traverses through a symbolic link.
    if check_symlink_in_path(work_tree, Path::new(path)).is_some() {
        return Err(AddPathError::Other(anyhow::anyhow!(
            "'{}' is beyond a symbolic link",
            path
        )));
    }

    // Use symlink_metadata to detect dangling symlinks (exists() follows symlinks)
    if fs::symlink_metadata(&abs_path).is_err() {
        let path_bytes = path.as_bytes();
        // Check if it's an index entry that needs to be removed
        if index.get(path_bytes, 0).is_some() {
            if !args.dry_run {
                index.remove(path_bytes);
            }
            if args.verbose {
                eprintln!("remove '{path}'");
            }
            return Ok(());
        }
        // Check unmerged entries (stages 1, 2, 3)
        let has_unmerged = (1..=3).any(|stage| index.get(path_bytes, stage).is_some());
        if has_unmerged {
            // Can't resolve a conflict if file doesn't exist
            return Err(AddPathError::Other(anyhow::anyhow!(
                "pathspec '{}' did not match any files",
                path
            )));
        }
        return Err(AddPathError::Other(anyhow::anyhow!(
            "pathspec '{}' did not match any files",
            path
        )));
    }

    // Use symlink_metadata so symlinks to directories are staged as
    // symlinks, not traversed.
    let is_real_dir = fs::symlink_metadata(&abs_path)
        .map(|m| m.file_type().is_dir())
        .unwrap_or(false);
    if is_real_dir {
        // Check for embedded repository (directory with its own .git)
        let embedded_git = abs_path.join(".git");
        if embedded_git.exists() {
            // This is an embedded repository — stage as a gitlink (160000)
            return stage_gitlink(odb, index, work_tree, path, &abs_path, args)
                .map_err(AddPathError::IoError);
        }

        let mut paths = Vec::new();
        walk_directory(
            &abs_path,
            work_tree,
            &mut paths,
            repo,
            ignore_matcher,
            args.force,
        )?;
        for rel_path in &paths {
            let file_abs = work_tree.join(rel_path);
            if let Err(e) = stage_file(odb, index, work_tree, rel_path, &file_abs, args, add_cfg) {
                if add_cfg.ignore_errors {
                    eprintln!("warning: {e}");
                } else {
                    return Err(AddPathError::IoError(e));
                }
            }
        }
    } else {
        // Allow adding ignored files when resolving merge conflicts (unmerged entries).
        let path_bytes = path.as_bytes();
        let has_unmerged = (1..=3).any(|stage| index.get(path_bytes, stage).is_some());

        // Check ignore patterns for explicitly named files (like real git),
        // but skip the check if the file has unmerged entries (conflict resolution).
        if !has_unmerged {
            if let Some(ref mut matcher) = ignore_matcher {
                let (is_ignored, _match_info) = matcher
                    .check_path(repo, Some(&*index), path, false)
                    .map_err(|e| AddPathError::Other(e.into()))?;
                if is_ignored {
                    return Err(AddPathError::Ignored(format!(
                        "The following paths are ignored by one of your .gitignore files:\n\
                         {path}\n\
                         Use -f if you really want to add them."
                    )));
                }
            }
        }
        stage_file(odb, index, work_tree, path, &abs_path, args, add_cfg)
            .map_err(AddPathError::IoError)?;
    }

    Ok(())
}

/// Stage an embedded repository as a gitlink (mode 160000) in the index.
///
/// Reads the HEAD of the embedded repo to get the commit OID, and warns
/// (unless `--no-warn-embedded-repo` is set) that a bare `git add` of an
/// embedded repo is probably a mistake.
fn stage_gitlink(
    _odb: &Odb,
    index: &mut Index,
    _work_tree: &Path,
    rel_path: &str,
    abs_path: &Path,
    args: &Args,
) -> Result<()> {
    // Read the embedded repo's HEAD to get the commit OID
    let embedded_head_path = abs_path.join(".git/HEAD");
    let head_content = fs::read_to_string(&embedded_head_path)
        .with_context(|| format!("cannot read HEAD of embedded repo '{}'", rel_path))?;
    let head_trimmed = head_content.trim();

    // Resolve the HEAD
    let oid_hex = if let Some(refname) = head_trimmed.strip_prefix("ref: ") {
        let ref_path = abs_path.join(".git").join(refname);
        fs::read_to_string(&ref_path)
            .with_context(|| {
                format!(
                    "cannot resolve ref '{}' in embedded repo '{}'",
                    refname, rel_path
                )
            })?
            .trim()
            .to_string()
    } else {
        head_trimmed.to_string()
    };

    let oid = ObjectId::from_hex(&oid_hex)
        .with_context(|| format!("invalid HEAD OID in embedded repo '{}'", rel_path))?;

    // Check whether this entry is already tracked as a gitlink in the index
    let already_tracked = index
        .get(rel_path.as_bytes(), 0)
        .map(|e| e.mode == 0o160000)
        .unwrap_or(false);

    // Warn about embedded repository unless suppressed
    if !args.no_warn_embedded_repo && !already_tracked {
        eprintln!("warning: adding embedded git repository: {}", rel_path);
        eprintln!("hint: You've added another git repository inside your current repository.");
        eprintln!("hint: Clones of the outer repository will not contain the contents of");
        eprintln!("hint: the embedded repository and will not know how to obtain it.");
        eprintln!("hint: If you meant to add a submodule, use:");
        eprintln!("hint: ");
        eprintln!("hint: \tgit submodule add <url> {}", rel_path);
        eprintln!("hint: ");
        eprintln!("hint: If you added this path by mistake, you can remove it from the");
        eprintln!("hint: index with:");
        eprintln!("hint: ");
        eprintln!("hint: \tgit rm --cached {}", rel_path);
        eprintln!("hint: ");
        eprintln!("hint: See \"git help submodule\" for more information.");
    }

    if args.dry_run {
        eprintln!("add '{}'", rel_path);
        return Ok(());
    }

    let meta = fs::metadata(abs_path)?;
    let entry = IndexEntry {
        ctime_sec: meta.ctime() as u32,
        ctime_nsec: meta.ctime_nsec() as u32,
        mtime_sec: meta.mtime() as u32,
        mtime_nsec: meta.mtime_nsec() as u32,
        dev: meta.dev() as u32,
        ino: meta.ino() as u32,
        mode: 0o160000, // gitlink mode
        uid: meta.uid(),
        gid: meta.gid(),
        size: 0,
        oid,
        flags: rel_path.len().min(0xFFF) as u16,
        flags_extended: None,
        path: rel_path.as_bytes().to_vec(),
    };
    index.add_or_replace(entry);

    if args.verbose {
        eprintln!("add '{}'", rel_path);
    }

    Ok(())
}

/// Stage a single file into the index.
fn stage_file(
    odb: &Odb,
    index: &mut Index,
    _work_tree: &Path,
    rel_path: &str,
    abs_path: &Path,
    args: &Args,
    add_cfg: &AddConfig,
) -> Result<()> {
    if args.dry_run {
        if args.chmod.is_some() {
            // Don't actually stage, just check if the file exists
            return Ok(());
        }
        eprintln!("add '{rel_path}'");
        return Ok(());
    }

    let meta = fs::symlink_metadata(abs_path)?;

    if args.intent_to_add {
        let mode = if meta.file_type().is_symlink() {
            0o120000
        } else if add_cfg.core_filemode {
            normalize_mode(meta.mode())
        } else {
            0o100644 // When core.filemode=false, default to regular
        };
        let entry = IndexEntry {
            ctime_sec: meta.ctime() as u32,
            ctime_nsec: meta.ctime_nsec() as u32,
            mtime_sec: meta.mtime() as u32,
            mtime_nsec: meta.mtime_nsec() as u32,
            dev: meta.dev() as u32,
            ino: meta.ino() as u32,
            mode,
            uid: meta.uid(),
            gid: meta.gid(),
            size: 0,
            oid: grit_lib::diff::zero_oid(),
            flags: rel_path.len().min(0xFFF) as u16,
            flags_extended: None,
            path: rel_path.as_bytes().to_vec(),
        };
        index.add_or_replace(entry);
        if args.verbose {
            eprintln!("add '{rel_path}'");
        }
        return Ok(());
    }

    // Determine mode
    let is_symlink = meta.file_type().is_symlink();
    let mode = if is_symlink {
        0o120000
    } else if add_cfg.core_filemode {
        normalize_mode(meta.mode())
    } else {
        // core.filemode=false: preserve existing mode from index if any,
        // otherwise default to 100644
        // Check for unmerged entries: prefer higher stages for mode
        let existing_mode = index
            .get(rel_path.as_bytes(), 0)
            .or_else(|| index.get(rel_path.as_bytes(), 2))
            .or_else(|| index.get(rel_path.as_bytes(), 1))
            .map(|e| e.mode);
        existing_mode.unwrap_or(0o100644)
    };

    // Handle --chmod flag
    let final_mode = if let Some(ref chmod_val) = args.chmod {
        if is_symlink {
            let display_path = rel_path;
            eprintln!("warning: cannot chmod {} '{}'", chmod_val, display_path);
            return Err(anyhow::anyhow!(
                "cannot chmod {} '{}'",
                chmod_val,
                display_path
            ));
        }
        match chmod_val.as_str() {
            "+x" => 0o100755,
            "-x" => 0o100644,
            other => bail!("unrecognized --chmod value: {}", other),
        }
    } else {
        mode
    };

    // Skip if index already has this file with matching stat data and no chmod override
    if args.chmod.is_none() {
        if let Some(existing) = index.get(rel_path.as_bytes(), 0) {
            if stat_matches(existing, &meta) && existing.mode == final_mode {
                return Ok(());
            }
        }
    }

    // Read file content and hash it
    let data = if is_symlink {
        let target = fs::read_link(abs_path)?;
        target.to_string_lossy().into_owned().into_bytes()
    } else {
        let raw = fs::read(abs_path)?;
        // Apply CRLF / clean-filter conversion
        let file_attrs = crlf::get_file_attrs(&add_cfg.attrs, rel_path, &add_cfg.config);
        // Apply working-tree-encoding conversion (e.g. UTF-16 → UTF-8)
        let raw = if let Some(ref encoding) = file_attrs.working_tree_encoding {
            convert_from_working_tree_encoding(&raw, encoding).with_context(|| {
                format!(
                    "failed to convert '{}' from encoding '{}'",
                    rel_path, encoding
                )
            })?
        } else {
            raw
        };
        match crlf::convert_to_git(&raw, rel_path, &add_cfg.conv, &file_attrs) {
            Ok(converted) => converted,
            Err(msg) => bail!("{msg}"),
        }
    };

    let oid = odb.write(ObjectKind::Blob, &data)?;
    let mut entry = entry_from_metadata(&meta, rel_path.as_bytes(), oid, final_mode);
    entry.mode = final_mode; // Ensure mode override sticks
                             // Use stage_file which also clears conflict stages (1, 2, 3) for the same
                             // path — this is how `git add` resolves merge/cherry-pick conflicts.
    index.stage_file(entry);

    if args.verbose {
        eprintln!("add '{rel_path}'");
    }

    Ok(())
}

/// Recursively walk a directory, collecting relative paths (skipping .git and ignored files).
fn walk_directory(
    dir: &Path,
    work_tree: &Path,
    out: &mut Vec<String>,
    repo: &Repository,
    ignore_matcher: &mut Option<IgnoreMatcher>,
    force: bool,
) -> Result<()> {
    let entries = fs::read_dir(dir)?;
    let mut sorted: Vec<_> = entries.filter_map(|e| e.ok()).collect();
    sorted.sort_by_key(|e| e.file_name());

    for entry in sorted {
        let path = entry.path();
        let file_name = entry.file_name();
        let name_str = file_name.to_string_lossy();

        if name_str == ".git" {
            continue;
        }

        let rel = path
            .strip_prefix(work_tree)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| path.to_string_lossy().to_string());

        // Use symlink_metadata to detect symlinks *before* following them.
        // A symlink to a directory should be stored as a symlink blob,
        // not traversed into.
        let ft = match fs::symlink_metadata(&path) {
            Ok(m) => m.file_type(),
            Err(_) => continue,
        };
        let is_symlink = ft.is_symlink();
        let is_dir = !is_symlink && ft.is_dir();

        // Check if ignored
        if !force {
            if let Some(matcher) = ignore_matcher.as_mut() {
                if let Ok((ignored, _)) = matcher.check_path(repo, None, &rel, is_dir) {
                    if ignored {
                        continue;
                    }
                }
            }
        }

        if is_dir {
            walk_directory(&path, work_tree, out, repo, ignore_matcher, force)?;
        } else {
            out.push(rel);
        }
    }

    Ok(())
}

/// Compute path relative to work tree from cwd.
fn pathdiff(cwd: &Path, work_tree: &Path) -> Option<String> {
    let cwd_canon = cwd.canonicalize().ok()?;
    let wt_canon = work_tree.canonicalize().ok()?;

    if cwd_canon == wt_canon {
        return None;
    }

    cwd_canon
        .strip_prefix(&wt_canon)
        .ok()
        .map(|p| p.to_string_lossy().to_string())
}

/// Walk the parent components of `rel_path` (relative to `work_tree`) and
/// return `Some(prefix)` if any of them is a symbolic link.  Only *parent*
/// components are checked — the final path component itself may be a symlink.
fn check_symlink_in_path(work_tree: &Path, rel_path: &Path) -> Option<PathBuf> {
    let mut accumulated = PathBuf::new();
    let components: Vec<_> = rel_path.components().collect();
    // Check all components except the last one (the file itself).
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

/// Resolve a pathspec relative to the prefix (cwd within worktree).
fn resolve_pathspec(pathspec: &str, _work_tree: &Path, prefix: Option<&str>) -> String {
    if pathspec == "." {
        return prefix.unwrap_or("").to_owned();
    }

    match prefix {
        Some(p) if !p.is_empty() => {
            let combined = PathBuf::from(p).join(pathspec);
            combined.to_string_lossy().to_string()
        }
        _ => pathspec.to_owned(),
    }
}

/// Check whether a string contains glob metacharacters.
fn has_glob_chars(s: &str) -> bool {
    s.contains('*') || s.contains('?') || s.contains('[')
}

/// Simple glob pattern matching against a single path component name.
fn glob_matches(pattern: &str, name: &str) -> bool {
    glob_match_inner(pattern.as_bytes(), name.as_bytes())
}

fn glob_match_inner(pat: &[u8], text: &[u8]) -> bool {
    let mut pi = 0;
    let mut ti = 0;
    let mut star_pi = usize::MAX;
    let mut star_ti = 0;
    while ti < text.len() {
        if pi < pat.len() && (pat[pi] == b'?' || pat[pi] == text[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < pat.len() && pat[pi] == b'[' {
            // bracket expression
            if let Some(end) = pat[pi..].iter().position(|&b| b == b']') {
                let inside = &pat[pi + 1..pi + end];
                let matches_bracket = inside.contains(&text[ti]);
                if matches_bracket {
                    pi = pi + end + 1;
                    ti += 1;
                } else if star_pi != usize::MAX {
                    pi = star_pi + 1;
                    star_ti += 1;
                    ti = star_ti;
                } else {
                    return false;
                }
            } else {
                return false;
            }
        } else if pi < pat.len() && pat[pi] == b'*' {
            star_pi = pi;
            star_ti = ti;
            pi += 1;
        } else if star_pi != usize::MAX {
            pi = star_pi + 1;
            star_ti += 1;
            ti = star_ti;
        } else {
            return false;
        }
    }
    while pi < pat.len() && pat[pi] == b'*' {
        pi += 1;
    }
    pi == pat.len()
}

/// Expand a pathspec containing glob characters against the working tree.
///
/// If the pathspec does not contain glob characters, returns it unchanged.
/// Otherwise, matches it against files/dirs in the working tree directory.
fn expand_glob_pathspec(pathspec: &str, work_tree: &Path) -> Vec<String> {
    if !has_glob_chars(pathspec) {
        return vec![pathspec.to_owned()];
    }

    // Split into directory prefix and glob pattern.
    // e.g. "dir/file?.t" -> dir_prefix="dir", pattern="file?.t"
    let (dir_prefix, pattern) = if let Some(slash_pos) = pathspec.rfind('/') {
        (&pathspec[..slash_pos], &pathspec[slash_pos + 1..])
    } else {
        ("", pathspec)
    };

    let search_dir = if dir_prefix.is_empty() {
        work_tree.to_owned()
    } else {
        work_tree.join(dir_prefix)
    };

    let mut matches = Vec::new();
    if let Ok(entries) = fs::read_dir(&search_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if glob_matches(pattern, &name_str) {
                let rel = if dir_prefix.is_empty() {
                    name_str.to_string()
                } else {
                    format!("{dir_prefix}/{name_str}")
                };
                matches.push(rel);
            }
        }
    }

    if matches.is_empty() {
        // No matches — return original pathspec so add_path gives a proper error.
        vec![pathspec.to_owned()]
    } else {
        matches.sort();
        matches
    }
}

/// Convert file content from a working-tree encoding to UTF-8.
fn convert_from_working_tree_encoding(data: &[u8], encoding: &str) -> Result<Vec<u8>> {
    let enc_upper = encoding.to_uppercase().replace('-', "");
    match enc_upper.as_str() {
        "UTF16" | "UTF16LE" | "UTF16BE" => {
            // Detect BOM and decode accordingly
            let text = if data.len() >= 2 && data[0] == 0xFF && data[1] == 0xFE {
                // UTF-16 LE with BOM
                let u16_data: Vec<u16> = data[2..]
                    .chunks(2)
                    .filter(|c| c.len() == 2)
                    .map(|c| u16::from_le_bytes([c[0], c[1]]))
                    .collect();
                String::from_utf16(&u16_data)
                    .map_err(|e| anyhow::anyhow!("invalid UTF-16 LE: {e}"))?
            } else if data.len() >= 2 && data[0] == 0xFE && data[1] == 0xFF {
                // UTF-16 BE with BOM
                let u16_data: Vec<u16> = data[2..]
                    .chunks(2)
                    .filter(|c| c.len() == 2)
                    .map(|c| u16::from_be_bytes([c[0], c[1]]))
                    .collect();
                String::from_utf16(&u16_data)
                    .map_err(|e| anyhow::anyhow!("invalid UTF-16 BE: {e}"))?
            } else if enc_upper == "UTF16BE" {
                let u16_data: Vec<u16> = data
                    .chunks(2)
                    .filter(|c| c.len() == 2)
                    .map(|c| u16::from_be_bytes([c[0], c[1]]))
                    .collect();
                String::from_utf16(&u16_data)
                    .map_err(|e| anyhow::anyhow!("invalid UTF-16 BE: {e}"))?
            } else {
                // Default: try LE
                let u16_data: Vec<u16> = data
                    .chunks(2)
                    .filter(|c| c.len() == 2)
                    .map(|c| u16::from_le_bytes([c[0], c[1]]))
                    .collect();
                String::from_utf16(&u16_data)
                    .map_err(|e| anyhow::anyhow!("invalid UTF-16 LE: {e}"))?
            };
            Ok(text.into_bytes())
        }
        "UTF8" | "UTF8BOM" => {
            // Already UTF-8, just strip BOM if present
            if data.starts_with(&[0xEF, 0xBB, 0xBF]) {
                Ok(data[3..].to_vec())
            } else {
                Ok(data.to_vec())
            }
        }
        _ => {
            // Unknown encoding — fail
            anyhow::bail!("unsupported working-tree-encoding: {encoding}");
        }
    }
}
