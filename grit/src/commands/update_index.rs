//! `grit update-index` — register file contents in the working tree to the index.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use std::io::{self, BufRead};
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
use std::path::Component;
use std::path::{Path, PathBuf};

use grit_lib::config::ConfigSet;
use grit_lib::crlf;
use grit_lib::diff::read_submodule_head_oid;
use grit_lib::index::{entry_from_stat, normalize_mode, Index, IndexEntry};
use grit_lib::objects::ObjectId;
use grit_lib::odb::Odb;
use grit_lib::repo::Repository;

/// Match Git: when `--chmod` changes the index entry, update the working tree file mode so a
/// subsequent `git add` of the same path keeps the executable bit.
fn mirror_index_executable_to_worktree(abs_path: &Path, executable: bool) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let Ok(meta) = std::fs::symlink_metadata(abs_path) else {
            return;
        };
        if !meta.is_file() {
            return;
        }
        let Ok(mut perms) = std::fs::metadata(abs_path).map(|m| m.permissions()) else {
            return;
        };
        let mode = perms.mode();
        let new_mode = if executable {
            mode | 0o111
        } else {
            mode & !0o111
        };
        perms.set_mode(new_mode);
        let _ = std::fs::set_permissions(abs_path, perms);
    }
    #[cfg(not(unix))]
    let _ = (abs_path, executable);
}

/// Arguments for `grit update-index`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Add specified files to the index.
    #[arg(long)]
    pub add: bool,

    /// Remove specified files from the index.
    #[arg(long)]
    pub remove: bool,

    /// Force removal even if file exists.
    #[arg(long = "force-remove")]
    pub force_remove: bool,

    /// Only record object info, don't check or update file in work tree.
    #[arg(long = "info-only")]
    pub info_only: bool,

    /// Read index info from stdin.
    #[arg(long = "index-info")]
    pub index_info: bool,

    /// Refresh stat info without changing object names.
    #[arg(long)]
    pub refresh: bool,

    /// Like --refresh but ignores assume-unchanged bit.
    #[arg(long = "really-refresh")]
    pub really_refresh: bool,

    /// Like --refresh but only on entries that have changed.
    #[arg(long)]
    pub again: bool,

    /// Mark files as "assume unchanged".
    #[arg(long = "assume-unchanged")]
    pub assume_unchanged: bool,

    /// Mark files as "no assume unchanged".
    #[arg(long = "no-assume-unchanged")]
    pub no_assume_unchanged: bool,

    /// Mark files as skip-worktree.
    #[arg(long = "skip-worktree")]
    pub skip_worktree: bool,

    /// Unset skip-worktree.
    #[arg(long = "no-skip-worktree")]
    pub no_skip_worktree: bool,

    /// Read paths from stdin (NUL terminated).
    #[arg(short = 'z')]
    pub null_terminated: bool,

    /// Ignore missing files when adding.
    #[arg(long = "ignore-missing")]
    pub ignore_missing: bool,

    /// When removing entries, don't update (skip-worktree) entries.
    #[arg(long = "ignore-skip-worktree-entries")]
    pub ignore_skip_worktree_entries: bool,

    /// Re-create unmerged entries for the given paths.
    #[arg(long = "unresolve")]
    pub unresolve: bool,

    /// Show the index format version.
    #[arg(long = "show-index-version")]
    pub show_index_version: bool,

    /// Set the index file version.
    #[arg(long = "index-version", value_name = "N")]
    pub index_version: Option<u32>,

    /// Add `<mode>,<object>,<path>` entry directly.
    /// Also accepts legacy 3-argument form: --cacheinfo <mode> <object> <path>.
    #[arg(long = "cacheinfo", value_name = "mode,object,path", num_args = 1..=3, action = clap::ArgAction::Append, allow_hyphen_values = true)]
    pub cacheinfo: Vec<String>,

    /// Set the execute bit on tracked files (+x or -x). Can be repeated.
    #[arg(long = "chmod", value_name = "MODE", action = clap::ArgAction::Append)]
    pub chmod: Vec<String>,

    /// Replace the entire index (used with --index-info).
    #[arg(long = "replace")]
    pub replace: bool,

    /// Do not complain about unmerged entries.
    #[arg(long = "unmerged")]
    pub unmerged: bool,

    /// Verbose mode.
    #[arg(short = 'v', long = "verbose")]
    pub verbose: bool,

    /// Suppress output.
    #[arg(short = 'q', long = "quiet")]
    pub quiet: bool,

    /// Ignore changes to submodule during --refresh.
    #[arg(long = "ignore-submodules")]
    pub ignore_submodules: bool,

    /// Files to add/remove from the index.
    pub files: Vec<PathBuf>,
}

/// Per-path operation mode for `update-index`.
///
/// Git uses sticky flags: each `--add`, `--remove`, or `--force-remove` applies to
/// following path arguments until another mode flag appears.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PathMode {
    /// Update an existing index entry only (no `--add`).
    Update,
    /// `--add`
    Add,
    /// `--remove`
    Remove,
    /// `--force-remove`
    ForceRemove,
    /// Both `--add` and `--remove` are set: Git enables both and `process_path` decides
    /// (e.g. removing a file from the index when a directory replaced it on disk).
    AddRemoveCombo,
}

fn global_path_mode(args: &Args) -> PathMode {
    if args.force_remove {
        PathMode::ForceRemove
    } else if args.add && args.remove {
        PathMode::AddRemoveCombo
    } else if args.remove {
        PathMode::Remove
    } else if args.add {
        PathMode::Add
    } else {
        PathMode::Update
    }
}

fn skip_one_update_index_arg(rest: &[String], i: usize) -> usize {
    let tok = &rest[i];
    if tok == "--cacheinfo" {
        if i + 1 < rest.len() {
            let next = &rest[i + 1];
            if next.contains(',') {
                return i + 2;
            }
            if i + 3 < rest.len() {
                return i + 4;
            }
        }
        return (i + 1).min(rest.len());
    }
    if tok == "--chmod" && i + 1 < rest.len() && !rest[i + 1].starts_with('-') {
        return i + 2;
    }
    if tok.starts_with("--chmod=") {
        return i + 1;
    }
    if tok == "--index-version" && i + 1 < rest.len() {
        return i + 2;
    }
    (i + 1).min(rest.len())
}

fn sticky_path_modes_for_paths(rest: &[String], files: &[PathBuf]) -> Result<Vec<PathMode>> {
    let mut modes = Vec::with_capacity(files.len());
    let mut file_idx = 0usize;
    let mut mode = PathMode::Update;
    let mut i = 0usize;
    while i < rest.len() {
        let tok = &rest[i];
        match tok.as_str() {
            "--add" => {
                mode = PathMode::Add;
                i += 1;
            }
            "--remove" => {
                mode = PathMode::Remove;
                i += 1;
            }
            "--force-remove" => {
                mode = PathMode::ForceRemove;
                i += 1;
            }
            "--" => {
                i += 1;
                while i < rest.len() {
                    if file_idx >= files.len() {
                        bail!("unexpected extra path after '--'");
                    }
                    if !paths_equal(files.get(file_idx), &rest[i]) {
                        bail!(
                            "path order mismatch at '{}': expected '{}'",
                            rest[i],
                            files[file_idx].display()
                        );
                    }
                    modes.push(mode);
                    file_idx += 1;
                    i += 1;
                }
            }
            t if t.starts_with('-') => {
                i = skip_one_update_index_arg(rest, i);
            }
            _ => {
                if file_idx >= files.len() {
                    bail!("unexpected path argument '{tok}'");
                }
                if !paths_equal(files.get(file_idx), tok) {
                    bail!(
                        "path order mismatch at '{tok}': expected '{}'",
                        files[file_idx].display()
                    );
                }
                modes.push(mode);
                file_idx += 1;
                i += 1;
            }
        }
    }
    if file_idx != files.len() {
        bail!(
            "path modes: expected {} paths, got {}",
            files.len(),
            file_idx
        );
    }
    Ok(modes)
}

fn paths_equal(expected: Option<&PathBuf>, actual: &str) -> bool {
    let Some(exp) = expected else {
        return false;
    };
    exp.as_path() == Path::new(actual)
}

/// Run `grit update-index`.
pub fn run(args: Args, raw_rest: &[String]) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let index_path = repo.index_path();
    let mut index = repo.load_index_at(&index_path).context("loading index")?;
    let symlinks_enabled = core_symlinks_enabled(&repo);

    let work_tree = repo
        .work_tree
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("cannot update-index in bare repository"))?;
    let cwd = std::env::current_dir().context("resolving current directory")?;

    let config = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
    let conv = crlf::ConversionConfig::from_config(&config);
    let attrs = crlf::load_gitattributes(work_tree);

    if args.show_index_version {
        println!("{}", index.version);
        return Ok(());
    }

    if let Some(ver) = args.index_version {
        let old_ver = index.version;
        if args.verbose {
            println!("index-version: was {old_ver}, set to {ver}");
        }
        index.version = ver;
        repo.write_index_at(&index_path, &mut index)
            .context("writing index")?;
        return Ok(());
    }

    if args.index_info {
        return run_index_info(&repo, &mut index, &index_path);
    }

    if args.unresolve {
        // --unresolve: not yet implemented (requires MERGE_HEAD / merge-base logic).
        // Accept the flag silently so scripts that pass it don't hard-fail.
        // If paths are given, just succeed; real git re-creates stage 1/2/3 entries.
        eprintln!("warning: --unresolve is not yet fully implemented");
        repo.write_index_at(&index_path, &mut index)
            .context("writing index")?;
        return Ok(());
    }

    // Process --cacheinfo entries.
    // Supports both forms:
    //   new: --cacheinfo <mode>,<sha1>,<path>  (one comma-separated arg)
    //   legacy: --cacheinfo <mode> <sha1> <path>  (three separate args, num_args=1..=3)
    {
        let cacheinfo_vals = &args.cacheinfo;
        // With num_args=1..=3 and action=Append, each --cacheinfo invocation
        // adds 1-3 values to the flat vector. We need to process them in groups.
        // Strategy: if a value contains a comma, it's the new comma-separated form (1 arg).
        // Otherwise, consume groups of 3 as the legacy form.
        let mut i = 0;
        while i < cacheinfo_vals.len() {
            let val = &cacheinfo_vals[i];
            let (mode_str, oid_str, path_bytes) = if val.contains(',') {
                // New form: single comma-separated value
                let parts: Vec<&str> = val.splitn(3, ',').collect();
                if parts.len() != 3 {
                    bail!("--cacheinfo needs mode,object,path: '{val}'");
                }
                i += 1;
                (
                    parts[0].to_string(),
                    parts[1].to_string(),
                    parts[2].as_bytes().to_vec(),
                )
            } else {
                // Legacy form: 3 separate values
                if i + 2 >= cacheinfo_vals.len() {
                    bail!("--cacheinfo needs mode,object,path: '{val}'");
                }
                let mode_s = val.clone();
                let oid_s = cacheinfo_vals[i + 1].clone();
                let path_s = cacheinfo_vals[i + 2].clone();
                i += 3;
                (mode_s, oid_s, path_s.as_bytes().to_vec())
            };
            let mode = u32::from_str_radix(&mode_str, 8)
                .with_context(|| format!("invalid mode '{mode_str}'"))?;
            let oid: ObjectId = oid_str
                .parse()
                .with_context(|| format!("invalid object id '{oid_str}'"))?;
            // Reject null (all-zero) SHA1 — print verbose but skip
            if oid.is_zero() {
                let path_str = String::from_utf8_lossy(&path_bytes);
                if args.verbose {
                    println!("add '{path_str}'");
                }
                eprintln!("error: git update-index: --cacheinfo cannot add a null sha1");
                std::process::exit(1);
            }
            // Directory/file conflicts: reject adding a blob under an existing
            // file prefix, or a file when the index already has longer paths
            // under that directory (matches git's update-index checks).
            if mode != grit_lib::index::MODE_TREE && mode != grit_lib::index::MODE_GITLINK {
                let rel_str = String::from_utf8_lossy(&path_bytes);
                if args.replace {
                    remove_index_path_conflicts_for_replace(&mut index, &path_bytes);
                } else {
                    let mut prefix = rel_str.as_ref();
                    while let Some(pos) = prefix.rfind('/') {
                        prefix = &prefix[..pos];
                        if index.get(prefix.as_bytes(), 0).is_some() {
                            bail!("error: invalid path '{}'", rel_str);
                        }
                    }
                    let dir_prefix = format!("{rel_str}/");
                    let has_dir_entries = index.entries.iter().any(|e| {
                        let p = String::from_utf8_lossy(&e.path);
                        p.starts_with(dir_prefix.as_str())
                    });
                    if has_dir_entries {
                        bail!("error: invalid path '{}'", rel_str);
                    }
                }
            }

            let entry = IndexEntry {
                ctime_sec: 0,
                ctime_nsec: 0,
                mtime_sec: 0,
                mtime_nsec: 0,
                dev: 0,
                ino: 0,
                mode,
                uid: 0,
                gid: 0,
                size: 0,
                oid,
                flags: path_bytes.len().min(0xFFF) as u16,
                flags_extended: None,
                path: path_bytes,
            };
            if args.verbose {
                let path_str = String::from_utf8_lossy(&entry.path).into_owned();
                index.add_or_replace(entry);
                println!("add '{path_str}'");
            } else {
                index.add_or_replace(entry);
            }
        }
    }

    // Build per-file chmod override map by scanning raw args.
    // Handles: --chmod=+x A --chmod=-x B (chmod applies to following file)
    let mut per_file_chmod: std::collections::HashMap<PathBuf, String> =
        std::collections::HashMap::new();
    {
        let raw: Vec<String> = std::env::args().collect();
        let mut pending_chmod: Option<String> = None;
        let mut i = 0usize;
        while i < raw.len() {
            let tok = &raw[i];
            if let Some(val) = tok.strip_prefix("--chmod=") {
                pending_chmod = Some(val.to_owned());
                i += 1;
            } else if tok == "--chmod" {
                if let Some(next) = raw.get(i + 1) {
                    if !next.starts_with('-') || next == "-" {
                        pending_chmod = Some(next.clone());
                        i += 2;
                        continue;
                    }
                }
                i += 1;
            } else if !tok.starts_with('-') {
                if let Some(ref c) = pending_chmod {
                    per_file_chmod.insert(PathBuf::from(tok), c.clone());
                }
                i += 1;
            } else {
                i += 1;
            }
        }
    }

    // Collect file paths (from args or stdin)
    let paths: Vec<PathBuf> = if args.null_terminated {
        read_paths_nul()?
    } else {
        args.files.clone()
    };

    let path_modes: Vec<PathMode> = if args.null_terminated {
        vec![global_path_mode(&args); paths.len()]
    } else if args.force_remove && args.add {
        sticky_path_modes_for_paths(raw_rest, &paths)?
    } else {
        vec![global_path_mode(&args); paths.len()]
    };

    for (input_path, path_mode_orig) in paths.iter().zip(path_modes.iter()) {
        let mut path_mode = *path_mode_orig;
        let (rel_path, abs_path) = resolve_repo_path(work_tree, &cwd, input_path)?;
        let rel_bytes = path_to_bytes(&rel_path)?;

        // Refuse to add a path that traverses through a symbolic link.
        // Check every *parent* component of the repo-relative path.
        if check_symlink_in_path(work_tree, &rel_path).is_some() {
            bail!("'{}' is beyond a symbolic link", input_path.display());
        }

        if path_mode == PathMode::AddRemoveCombo {
            match std::fs::symlink_metadata(&abs_path) {
                Ok(meta) if meta.is_dir() => {
                    if let Some(e) = index.get(&rel_bytes, 0) {
                        if e.mode != grit_lib::index::MODE_GITLINK && e.mode != 0o040000 {
                            index.remove(&rel_bytes);
                            continue;
                        }
                    }
                }
                // Git: `--add --remove` removes index entries for paths that no
                // longer exist on disk (e.g. rename flow: `rm A && git update-index --add --remove A B`).
                Err(_) => {
                    let _ = index.remove(&rel_bytes);
                    continue;
                }
                Ok(_) => {}
            }
            path_mode = PathMode::Add;
        }

        if path_mode == PathMode::ForceRemove {
            // --force-remove silently succeeds even if the entry is absent
            index.remove(&rel_bytes);
            continue;
        }

        // --remove: if the file doesn't exist on disk (or is a directory
        // that replaced it), remove the entry from the index.  If the file
        // *does* exist on disk, fall through to the normal update/add logic
        // so the index entry gets refreshed.
        if path_mode == PathMode::Remove {
            // --ignore-skip-worktree-entries: skip entries with skip-worktree bit
            if args.ignore_skip_worktree_entries {
                if let Some(e) = index.get(&rel_bytes, 0) {
                    if e.skip_worktree() {
                        continue; // leave this entry alone
                    }
                }
            }
            let file_exists = match std::fs::symlink_metadata(&abs_path) {
                Ok(m) => !m.is_dir(),
                Err(_) => false,
            };
            if !file_exists {
                if !index.remove(&rel_bytes) && !args.ignore_missing {
                    // Entry wasn't in the index — only error if the file is
                    // truly gone (not just a directory replacement).
                    bail!("'{}' is not in the index", input_path.display());
                }
                continue;
            }
            // File exists on disk — fall through to update it in the index.
        }

        if args.assume_unchanged {
            if let Some(e) = index.get_mut(&rel_bytes, 0) {
                e.set_assume_unchanged(true);
            }
            continue;
        }
        if args.no_assume_unchanged {
            if let Some(e) = index.get_mut(&rel_bytes, 0) {
                e.set_assume_unchanged(false);
            }
            continue;
        }
        if args.skip_worktree {
            if let Some(e) = index.get_mut(&rel_bytes, 0) {
                e.set_skip_worktree(true);
                if e.flags_extended.is_some() {
                    index.version = 3;
                }
            }
            continue;
        }
        if args.no_skip_worktree {
            if let Some(e) = index.get_mut(&rel_bytes, 0) {
                e.set_skip_worktree(false);
            }
            continue;
        }

        // --chmod=+x or --chmod=-x without --add: change the mode of an existing entry.
        // Per-file chmod (from interleaved args like --chmod=+x A --chmod=-x B) takes priority.
        let effective_chmod = per_file_chmod
            .get(input_path)
            .map(|s| s.as_str())
            .or_else(|| args.chmod.last().map(|s| s.as_str()));
        if let Some(ref chmod_val) = effective_chmod.map(|s| s.to_owned()) {
            if path_mode != PathMode::Add {
                let new_mode = match chmod_val.as_str() {
                    "+x" => 0o100755u32,
                    "-x" => 0o100644u32,
                    other => bail!("--chmod param '{}' must be either +x or -x", other),
                };
                if let Some(e) = index.get_mut(&rel_bytes, 0) {
                    e.mode = new_mode;
                } else {
                    bail!("'{}' is not in the index", input_path.display());
                }
                if !args.info_only {
                    mirror_index_executable_to_worktree(&abs_path, chmod_val.as_str() == "+x");
                }
                if args.verbose {
                    println!("add '{}'", rel_path.display());
                    println!("chmod {} '{}'", chmod_val, rel_path.display());
                }
                continue;
            }
            // With --add --chmod, fall through to add/update the file first,
            // then apply the chmod below.
        }

        // Stat the file
        let meta = match std::fs::symlink_metadata(&abs_path) {
            Ok(m) => m,
            Err(_) if args.ignore_missing => continue,
            Err(e) => {
                return Err(anyhow::anyhow!(
                    "cannot stat '{}': {e}",
                    input_path.display()
                ))
            }
        };

        // Check for D/F conflicts in the index before adding.
        // Skip for gitlinks (submodule directories).
        let is_gitlink = meta.file_type().is_dir() && abs_path.join(".git").exists();
        if path_mode == PathMode::Add && !is_gitlink {
            let rel_str = String::from_utf8_lossy(&rel_bytes);
            if args.replace {
                remove_index_path_conflicts_for_replace(&mut index, &rel_bytes);
            } else {
                // Check if any ancestor path is already a file in the index
                let mut prefix = rel_str.as_ref();
                while let Some(pos) = prefix.rfind('/') {
                    prefix = &prefix[..pos];
                    if index.get(prefix.as_bytes(), 0).is_some() {
                        bail!("error: invalid path '{}'", rel_str);
                    }
                }
                // Check if any existing index entry has this path as a prefix
                let dir_prefix = format!("{rel_str}/");
                let has_dir_entries = index.entries.iter().any(|e| {
                    let p = String::from_utf8_lossy(&e.path);
                    p.starts_with(dir_prefix.as_str())
                });
                if has_dir_entries {
                    bail!("error: invalid path '{}'", rel_str);
                }
            }
        }

        // Without --add, reject files not yet in the index.
        if path_mode != PathMode::Add && index.get(&rel_bytes, 0).is_none() {
            if args.ignore_missing {
                continue;
            }
            bail!("'{}' is not in the index", input_path.display());
        }

        // Handle gitlink (submodule directory with .git)
        if meta.is_dir() {
            let dot_git = abs_path.join(".git");
            if dot_git.exists() {
                let sub_git_dir = resolve_gitdir(&dot_git)?;
                let head_path = sub_git_dir.join("HEAD");
                let head_content = std::fs::read_to_string(&head_path)
                    .with_context(|| "reading HEAD of submodule".to_string())?;
                let head_content = head_content.trim();
                let oid: ObjectId = if let Some(refname) = head_content.strip_prefix("ref: ") {
                    let ref_path = sub_git_dir.join(refname);
                    let ref_content = std::fs::read_to_string(&ref_path)
                        .with_context(|| "reading ref in submodule".to_string())?;
                    ref_content.trim().parse().with_context(|| "invalid oid")?
                } else {
                    head_content.parse().with_context(|| "invalid HEAD oid")?
                };
                let entry = IndexEntry {
                    ctime_sec: 0,
                    ctime_nsec: 0,
                    mtime_sec: 0,
                    mtime_nsec: 0,
                    dev: 0,
                    ino: 0,
                    mode: grit_lib::index::MODE_GITLINK,
                    uid: 0,
                    gid: 0,
                    size: 0,
                    oid,
                    flags: rel_bytes.len().min(0xFFF) as u16,
                    flags_extended: None,
                    path: rel_bytes.to_vec(),
                };
                index.add_or_replace(entry);
            }
            continue;
        }

        let mut mode = {
            use std::os::unix::fs::MetadataExt;
            normalize_mode(meta.mode())
        };
        let existing_mode = index.get(&rel_bytes, 0).map(|e| e.mode);
        // On filesystems without symlink support (core.symlinks=false), keep
        // an existing symlink entry's mode even if the worktree stores it
        // as a plain file containing the link target.
        if !symlinks_enabled
            && !meta.file_type().is_symlink()
            && existing_mode == Some(grit_lib::index::MODE_SYMLINK)
        {
            mode = grit_lib::index::MODE_SYMLINK;
        }

        let rel_str = String::from_utf8_lossy(&rel_bytes);
        let data = if meta.file_type().is_symlink() {
            let target = std::fs::read_link(&abs_path)?;
            target.to_string_lossy().into_owned().into_bytes()
        } else {
            let raw = std::fs::read(&abs_path)
                .with_context(|| format!("cannot read '{}'", abs_path.display()))?;
            let file_attrs = crlf::get_file_attrs(&attrs, rel_str.as_ref(), &config);
            crlf::convert_to_git(&raw, rel_str.as_ref(), &conv, &file_attrs)
                .map_err(|msg| anyhow::anyhow!("{msg}"))?
        };

        let oid = match repo.odb.write(grit_lib::objects::ObjectKind::Blob, &data) {
            Ok(oid) => oid,
            Err(err) => {
                if is_permission_denied_error(&err) {
                    eprintln!(
                        "error: insufficient permission for adding an object to repository database .git/objects"
                    );
                    eprintln!(
                        "error: {}: failed to insert into database",
                        input_path.display()
                    );
                    eprintln!("fatal: Unable to process path {}", input_path.display());
                    std::process::exit(128);
                }
                return Err(anyhow::anyhow!("writing blob: {err}"));
            }
        };

        let entry = entry_from_stat(&abs_path, &rel_bytes, oid, mode)
            .with_context(|| format!("stat failed for '{}'", abs_path.display()))?;

        index.add_or_replace(entry);

        // Apply --chmod after adding the entry (per-file takes priority over global).
        let apply_chmod = per_file_chmod
            .get(input_path)
            .map(|s| s.as_str())
            .or_else(|| args.chmod.last().map(|s| s.as_str()))
            .map(|s| s.to_owned());
        if let Some(ref chmod_val) = apply_chmod {
            let new_mode = match chmod_val.as_str() {
                "+x" => 0o100755u32,
                "-x" => 0o100644u32,
                other => bail!("--chmod param '{}' must be either +x or -x", other),
            };
            if let Some(e) = index.get_mut(&rel_bytes, 0) {
                e.mode = new_mode;
            }
            if !args.info_only {
                mirror_index_executable_to_worktree(&abs_path, chmod_val.as_str() == "+x");
            }
            if args.verbose {
                println!("chmod {} '{}'", chmod_val, rel_path.display());
            }
        }
    }

    // --again: re-add all tracked files that have changed (rehash, update index)
    if args.again {
        let mut needs_update = false;
        let entries_snapshot: Vec<Vec<u8>> = index
            .entries
            .iter()
            .filter(|e| e.stage() == 0)
            .map(|e| e.path.clone())
            .collect();
        for path_bytes in entries_snapshot {
            let rel_path = String::from_utf8_lossy(&path_bytes).into_owned();
            let abs_path = work_tree.join(&rel_path);
            match std::fs::symlink_metadata(&abs_path) {
                Ok(meta) => {
                    use std::os::unix::fs::MetadataExt as _;
                    // Check if the file has changed vs the index entry
                    if let Some(entry) = index.get(&path_bytes, 0) {
                        let idx_mtime = entry.mtime_sec as i64;
                        let file_mtime = meta.mtime();
                        let idx_size = entry.size as u64;
                        let file_size = meta.size();
                        // If mtime or size changed, rehash and update
                        if idx_mtime != file_mtime || idx_size != file_size {
                            match std::fs::read(&abs_path) {
                                Ok(raw) => {
                                    let mode = entry.mode;
                                    let data = if mode == grit_lib::index::MODE_SYMLINK {
                                        raw
                                    } else {
                                        let file_attrs =
                                            crlf::get_file_attrs(&attrs, &rel_path, &config);
                                        match crlf::convert_to_git(
                                            &raw,
                                            &rel_path,
                                            &conv,
                                            &file_attrs,
                                        ) {
                                            Ok(d) => d,
                                            Err(e) => {
                                                return Err(anyhow::anyhow!(e));
                                            }
                                        }
                                    };
                                    match repo.odb.write(grit_lib::objects::ObjectKind::Blob, &data)
                                    {
                                        Ok(new_oid) => {
                                            let new_entry = entry_from_stat(
                                                &abs_path,
                                                &path_bytes,
                                                new_oid,
                                                mode,
                                            )
                                            .with_context(|| format!("stat '{}'", rel_path))?;
                                            index.add_or_replace(new_entry);
                                        }
                                        Err(_) => {
                                            eprintln!("{rel_path}: needs update");
                                            needs_update = true;
                                        }
                                    }
                                }
                                Err(_) => {
                                    eprintln!("{rel_path}: needs update");
                                    needs_update = true;
                                }
                            }
                        }
                    }
                }
                Err(_) => {
                    if args.remove {
                        index.entries.retain(|e| e.path != path_bytes);
                    } else {
                        eprintln!("{rel_path}: needs update");
                        needs_update = true;
                    }
                }
            }
        }
        repo.write_index_at(&index_path, &mut index)
            .context("writing index")?;
        if needs_update {
            std::process::exit(1);
        }
        return Ok(());
    }

    if args.refresh || args.really_refresh {
        // Re-stat all entries; exit 1 if any files need updating.
        let (uptodate, _) = refresh_index(
            &mut index,
            work_tree,
            &repo.odb,
            args.unmerged,
            args.ignore_missing,
            args.ignore_submodules,
        )?;
        repo.write_index_at(&index_path, &mut index)
            .context("writing index")?;
        // -q (quiet) suppresses the error exit; otherwise exit 1 if files need updating
        if !uptodate && !args.quiet {
            std::process::exit(1);
        }
        return Ok(());
    }

    repo.write_index_at(&index_path, &mut index)
        .context("writing index")?;
    Ok(())
}

/// Process `--index-info` stdin: lines of `"<mode> <oid>\t<path>"`.
fn run_index_info(
    repo: &grit_lib::repo::Repository,
    index: &mut Index,
    index_path: &std::path::Path,
) -> Result<()> {
    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let line = line?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // Format: "<mode> SP <oid> TAB <path>"
        // or: "<mode> SP <type> SP <oid> TAB <path>" (extended)
        let tab = line
            .find('\t')
            .ok_or_else(|| anyhow::anyhow!("bad --index-info line: no tab: '{line}'"))?;
        let meta = &line[..tab];
        let path = line.as_bytes()[tab + 1..].to_vec();

        let parts: Vec<&str> = meta.split(' ').collect();

        // Supported formats:
        //   2-part: "<mode> <sha1>"              → stage 0
        //   3-part: "<mode> <sha1> <stage>"      → stage 0-3 (git standard)
        //   3-part: "<mode> <type> <sha1>"       → stage 0 (extended, legacy)
        //
        // Disambiguate the 3-part case: if parts[2] is a single decimal digit
        // (0-3) it is a stage number; otherwise treat parts[1] as a type token
        // and parts[2] as the sha1.
        let (mode_str, oid_str, stage) = match parts.len() {
            2 => (parts[0], parts[1], 0u8),
            3 => {
                let third = parts[2];
                if third.len() == 1 && matches!(third, "0" | "1" | "2" | "3") {
                    let s: u8 = third.parse().unwrap_or(0);
                    (parts[0], parts[1], s)
                } else {
                    // Legacy: "<mode> <type> <sha1>"
                    (parts[0], parts[2], 0u8)
                }
            }
            _ => bail!("bad --index-info line: '{line}'"),
        };

        if mode_str == "0" {
            // Delete entry
            index.remove(&path);
            continue;
        }

        let mode = u32::from_str_radix(mode_str, 8)
            .with_context(|| format!("invalid mode '{mode_str}'"))?;
        let oid: ObjectId = oid_str
            .parse()
            .with_context(|| format!("invalid oid '{oid_str}'"))?;

        // Encode stage in the upper 2 bits of flags (bits 13-12).
        let base_flags = path.len().min(0xFFF) as u16;
        let flags = base_flags | ((stage as u16) << 12);

        let entry = IndexEntry {
            ctime_sec: 0,
            ctime_nsec: 0,
            mtime_sec: 0,
            mtime_nsec: 0,
            dev: 0,
            ino: 0,
            mode,
            uid: 0,
            gid: 0,
            size: 0,
            oid,
            flags,
            flags_extended: None,
            path,
        };
        index.add_or_replace(entry);
    }

    repo.write_index_at(index_path, index)
        .context("writing index")?;
    Ok(())
}

/// Re-stat all tracked files, updating mtime/ctime/size.
fn refresh_index(
    index: &mut Index,
    work_tree: &std::path::Path,
    odb: &Odb,
    allow_unmerged: bool,
    ignore_missing: bool,
    ignore_submodules: bool,
) -> Result<(bool, bool)> {
    // Returns (all_uptodate, index_modified)
    // all_uptodate: true if no files need updating
    // index_modified: true if index stat data was changed
    if !allow_unmerged {
        if let Some(entry) = index.entries.iter().find(|entry| entry.stage() != 0) {
            let rel = std::str::from_utf8(&entry.path)
                .map_err(|_| anyhow::anyhow!("non-UTF-8 path in index"))?;
            bail!("{rel}: needs merge");
        }
    }

    let mut all_uptodate = true;
    let mut index_modified = false;
    for entry in &mut index.entries {
        if entry.stage() != 0 {
            continue;
        }
        // Handle gitlinks (submodules)
        if entry.mode == 0o160000 {
            if ignore_submodules {
                continue; // ignore submodule changes
            }
            let path_str2 = std::str::from_utf8(&entry.path).unwrap_or("");
            let sub_dir = work_tree.join(path_str2);
            let submodule_matches = match read_submodule_head_oid(&sub_dir) {
                Some(h) => h == entry.oid,
                None => true, // uninitialized / no checkout — do not block refresh (matches Git)
            };
            if !submodule_matches {
                eprintln!("{path_str2}: needs update");
                all_uptodate = false;
            }
            continue;
        }
        let path_str = std::str::from_utf8(&entry.path)
            .map_err(|_| anyhow::anyhow!("non-UTF-8 path in index"))?;
        let path = std::path::Path::new(path_str);
        let abs = work_tree.join(path);
        match std::fs::symlink_metadata(&abs) {
            Ok(meta) => {
                use std::os::unix::fs::MetadataExt;
                // Symlinks: compare link target to the blob Git stores (matches
                // readlink + hash, not `read()` which follows the link).
                if meta.file_type().is_symlink() {
                    let target = std::fs::read_link(&abs)?;
                    let data = target.as_os_str().as_bytes();
                    let actual_oid = grit_lib::odb::Odb::hash_object_data(
                        grit_lib::objects::ObjectKind::Blob,
                        data,
                    );
                    let stat_changed =
                        entry.mtime_sec != meta.mtime() as u32 || entry.size != meta.size() as u32;
                    if actual_oid != entry.oid {
                        eprintln!("{path_str}: needs update");
                        all_uptodate = false;
                    } else if stat_changed {
                        entry.ctime_sec = meta.ctime() as u32;
                        entry.ctime_nsec = meta.ctime_nsec() as u32;
                        entry.mtime_sec = meta.mtime() as u32;
                        entry.mtime_nsec = meta.mtime_nsec() as u32;
                        entry.size = meta.size() as u32;
                        index_modified = true;
                    } else {
                        let new_ctime = meta.ctime() as u32;
                        if entry.ctime_sec != new_ctime {
                            entry.ctime_sec = new_ctime;
                            entry.ctime_nsec = meta.ctime_nsec() as u32;
                            index_modified = true;
                        }
                    }
                    continue;
                }
                // Check if stat data differs from index
                let stat_changed =
                    entry.mtime_sec != meta.mtime() as u32 || entry.size != meta.size() as u32;
                if stat_changed {
                    // Check if content actually changed
                    let content_changed = if let Ok(data) = std::fs::read(&abs) {
                        let actual_oid = odb.write(grit_lib::objects::ObjectKind::Blob, &data).ok();
                        actual_oid.map(|o| o != entry.oid).unwrap_or(true)
                    } else {
                        true
                    };
                    if content_changed {
                        eprintln!("{path_str}: needs update");
                        all_uptodate = false;
                    } else {
                        // Update stat info
                        entry.ctime_sec = meta.ctime() as u32;
                        entry.ctime_nsec = meta.ctime_nsec() as u32;
                        entry.mtime_sec = meta.mtime() as u32;
                        entry.mtime_nsec = meta.mtime_nsec() as u32;
                        entry.size = meta.size() as u32;
                        index_modified = true;
                    }
                } else {
                    // Stat matches, update ctime if it changed
                    let new_ctime = meta.ctime() as u32;
                    if entry.ctime_sec != new_ctime {
                        entry.ctime_sec = new_ctime;
                        entry.ctime_nsec = meta.ctime_nsec() as u32;
                        index_modified = true;
                    }
                }
            }
            Err(_) => {
                // File missing
                if !ignore_missing {
                    eprintln!("{path_str}: does not exist and --remove not set");
                    all_uptodate = false;
                }
            }
        }
    }
    Ok((all_uptodate, index_modified))
}

fn read_paths_nul() -> Result<Vec<PathBuf>> {
    use std::io::Read;
    let mut buf = Vec::new();
    io::stdin().read_to_end(&mut buf)?;
    let paths = buf
        .split(|&b| b == 0)
        .filter(|s| !s.is_empty())
        .map(|s| {
            std::str::from_utf8(s)
                .map(PathBuf::from)
                .map_err(|_| anyhow::anyhow!("non-UTF-8 path"))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(paths)
}

fn path_to_bytes(p: &Path) -> Result<Vec<u8>> {
    use std::os::unix::ffi::OsStrExt;
    Ok(p.as_os_str().as_bytes().to_vec())
}

fn resolve_repo_path(
    work_tree: &Path,
    cwd: &Path,
    input_path: &Path,
) -> Result<(PathBuf, PathBuf)> {
    let combined = if input_path.is_absolute() {
        input_path.to_path_buf()
    } else {
        cwd.join(input_path)
    };
    let normalized = normalize_path(&combined);
    let rel = normalized.strip_prefix(work_tree).with_context(|| {
        format!(
            "path '{}' is outside repository work tree",
            input_path.display()
        )
    })?;
    Ok((rel.to_path_buf(), work_tree.join(rel)))
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
        if let Ok(meta) = std::fs::symlink_metadata(&abs) {
            if meta.file_type().is_symlink() {
                return Some(accumulated);
            }
        }
    }
    None
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn resolve_gitdir(dot_git: &Path) -> anyhow::Result<PathBuf> {
    let meta = std::fs::symlink_metadata(dot_git)?;
    if meta.is_dir() {
        return Ok(dot_git.to_path_buf());
    }
    let content = std::fs::read_to_string(dot_git)?;
    let content = content.trim();
    let target = content
        .strip_prefix("gitdir: ")
        .ok_or_else(|| anyhow::anyhow!("invalid .git file"))?;
    let target_path = Path::new(target);
    if target_path.is_absolute() {
        Ok(target_path.to_path_buf())
    } else {
        Ok(dot_git.parent().unwrap_or(Path::new(".")).join(target_path))
    }
}

fn is_permission_denied_error(err: &grit_lib::error::Error) -> bool {
    err.to_string().contains("Permission denied") || err.to_string().contains("permission denied")
}

/// Remove index entries that conflict with adding `new_path` when `--replace` is set:
/// descendants (`new_path/...`) and ancestor files (`prefix` where `new_path` is `prefix/...`).
fn remove_index_path_conflicts_for_replace(index: &mut Index, new_path: &[u8]) {
    let mut child_prefix = new_path.to_vec();
    child_prefix.push(b'/');

    index.entries.retain(|e| {
        if e.path.starts_with(&child_prefix) {
            return false;
        }
        let mut anc = e.path.clone();
        anc.push(b'/');
        !new_path.starts_with(&anc)
    });
}

fn core_symlinks_enabled(repo: &Repository) -> bool {
    ConfigSet::load(Some(repo.git_dir.as_path()), true)
        .ok()
        .and_then(|cfg| cfg.get_bool("core.symlinks"))
        .and_then(|v| v.ok())
        .unwrap_or(true)
}

/// Non-CLI: `update-index -q --refresh` — refresh stat cache without exiting when entries are stale.
pub fn run_refresh_quiet(repo: &Repository) -> Result<()> {
    let index_path = repo.index_path();
    let mut index = repo.load_index_at(&index_path).context("loading index")?;
    let work_tree = repo
        .work_tree
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("cannot update-index in bare repository"))?;
    let (_uptodate, _) = refresh_index(&mut index, work_tree, &repo.odb, false, false, false)?;
    repo.write_index_at(&index_path, &mut index)
        .context("writing index")?;
    Ok(())
}
