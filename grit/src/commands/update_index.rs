//! `grit update-index` — register file contents in the working tree to the index.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use std::io::{self, BufRead};
use std::path::Component;
use std::path::{Path, PathBuf};

use grit_lib::config::{ConfigFile, ConfigScope, ConfigSet};
use grit_lib::index::{
    entry_from_stat, normalize_mode, Index, IndexEntry, MODE_EXECUTABLE, MODE_GITLINK,
    MODE_REGULAR, MODE_SYMLINK,
};
use grit_lib::objects::ObjectId;
use grit_lib::odb::Odb;
use grit_lib::repo::Repository;

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

    /// Ignore submodule changes for --refresh.
    #[arg(long = "ignore-submodules")]
    pub ignore_submodules: bool,

    /// When removing entries, don't update (skip-worktree) entries.
    #[arg(long = "ignore-skip-worktree-entries")]
    pub ignore_skip_worktree_entries: bool,

    /// Re-create unmerged entries for the given paths.
    #[arg(long = "unresolve")]
    pub unresolve: bool,

    /// Show the index format version.
    #[arg(long = "show-index-version")]
    pub show_index_version: bool,

    /// Set the index format version.
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

    /// Files to add/remove from the index.
    pub files: Vec<PathBuf>,
}

/// Run `grit update-index`.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let index_path = repo.index_path();
    let mut index = Index::load(&index_path).context("loading index")?;
    let core_symlinks = core_symlinks_enabled(&repo.git_dir);
    let config = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_else(|_| ConfigSet::new());
    let config_index_version = config
        .get("index.version")
        .and_then(|v| v.parse::<u32>().ok())
        .filter(|v| (2..=4).contains(v));
    let effective_index_version =
        config_index_version.unwrap_or_else(|| if index.version == 0 { 2 } else { index.version });
    let mut verbose_lines: Vec<String> = Vec::new();
    let mut chmod_apply_index = 0usize;

    let work_tree = repo
        .work_tree
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("cannot update-index in bare repository"))?;
    let cwd = std::env::current_dir().context("resolving current directory")?;

    if args.show_index_version {
        println!("{effective_index_version}");
        return Ok(());
    }

    if let Some(ver) = args.index_version {
        if !(2..=4).contains(&ver) {
            bail!("index-version {} not in range [2, 4]", ver);
        }
        let old = effective_index_version;
        index.version = ver;
        set_local_index_version(&repo.git_dir, ver)?;
        if args.verbose {
            verbose_lines.push(format!("index-version: was {}, set to {}", old, ver));
        }
    }

    if args.index_info {
        return run_index_info(&mut index, &index_path, &repo.odb);
    }

    if args.unresolve {
        // --unresolve: not yet implemented (requires MERGE_HEAD / merge-base logic).
        // Accept the flag silently so scripts that pass it don't hard-fail.
        // If paths are given, just succeed; real git re-creates stage 1/2/3 entries.
        eprintln!("warning: --unresolve is not yet fully implemented");
        index.write(&index_path).context("writing index")?;
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
            if oid == grit_lib::diff::zero_oid() {
                if args.verbose {
                    println!("add '{}'", String::from_utf8_lossy(&path_bytes));
                }
                bail!(
                    "error: cache entry has null sha1: {}\nfatal: Unable to write new index file",
                    String::from_utf8_lossy(&path_bytes)
                );
            }
            let display_path = String::from_utf8_lossy(&path_bytes).to_string();
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
            index.add_or_replace(entry);
            if args.verbose {
                println!("add '{}'", display_path);
            }
        }
    }

    // Collect file paths (from args or stdin)
    let paths: Vec<PathBuf> = if args.null_terminated {
        read_paths_nul()?
    } else {
        args.files.clone()
    };

    for input_path in &paths {
        let (rel_path, abs_path) = resolve_repo_path(work_tree, &cwd, &input_path)?;
        let rel_bytes = path_to_bytes(&rel_path)?;

        if args.add && has_df_conflict(&index, &rel_bytes) {
            bail!("'{}' appears as both a file and as a directory", input_path.display());
        }

        // Refuse to add a path that traverses through a symbolic link.
        // Check every *parent* component of the repo-relative path.
        if check_symlink_in_path(work_tree, &rel_path).is_some() {
            bail!("'{}' is beyond a symbolic link", input_path.display());
        }

        if args.force_remove {
            // --force-remove silently succeeds even if the entry is absent
            index.remove(&rel_bytes);
            continue;
        }

        // --remove: if the file doesn't exist on disk (or is a directory
        // that replaced it), remove the entry from the index.  If the file
        // *does* exist on disk, fall through to the normal update/add logic
        // so the index entry gets refreshed.
        if args.remove {
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

        let chmod_for_path = if !args.chmod.is_empty() {
            let raw = args
                .chmod
                .get(chmod_apply_index)
                .or_else(|| args.chmod.last())
                .ok_or_else(|| anyhow::anyhow!("missing --chmod value"))?
                .clone();
            chmod_apply_index += 1;
            let mode = match raw.as_str() {
                "+x" => 0o100755u32,
                "-x" => 0o100644u32,
                other => bail!("--chmod param '{}' must be either +x or -x", other),
            };
            Some((raw, mode))
        } else {
            None
        };

        // --chmod=+x or --chmod=-x without --add: change the mode of an existing entry.
        if let Some((ref chmod_val, new_mode)) = chmod_for_path {
            if !args.add {
                if let Some(e) = index.get_mut(&rel_bytes, 0) {
                    e.mode = new_mode;
                } else {
                    bail!("'{}' is not in the index", input_path.display());
                }
                if args.verbose {
                    verbose_lines.push(format!("add '{}'", input_path.display()));
                    verbose_lines.push(format!("chmod {} '{}'", chmod_val, input_path.display()));
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

        // Without --add, reject files not yet in the index.
        if !args.add && index.get(&rel_bytes, 0).is_none() {
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
                    .with_context(|| format!("reading HEAD of submodule"))?;
                let head_content = head_content.trim();
                let oid: ObjectId = if let Some(refname) = head_content.strip_prefix("ref: ") {
                    let ref_path = sub_git_dir.join(refname);
                    let ref_content = std::fs::read_to_string(&ref_path)
                        .with_context(|| format!("reading ref in submodule"))?;
                    ref_content.trim().parse().with_context(|| "invalid oid")?
                } else {
                    head_content.parse().with_context(|| "invalid HEAD oid")?
                };
                let entry = IndexEntry {
                    ctime_sec: 0, ctime_nsec: 0, mtime_sec: 0, mtime_nsec: 0,
                    dev: 0, ino: 0, mode: grit_lib::index::MODE_GITLINK,
                    uid: 0, gid: 0, size: 0, oid,
                    flags: rel_bytes.len().min(0xFFF) as u16,
                    flags_extended: None, path: rel_bytes.to_vec(),
                };
                index.add_or_replace(entry);
            }
            continue;
        }

        let mode = {
            use std::os::unix::fs::MetadataExt;
            normalize_mode(meta.mode())
        };
        // On filesystems/configs without symlink support, symlinks are
        // represented in the worktree as regular files containing the link
        // target. If this path is already tracked as a symlink, keep the
        // index mode as MODE_SYMLINK even though `lstat` reports a file.
        let mode = if !core_symlinks && mode != MODE_SYMLINK {
            if index
                .get(&rel_bytes, 0)
                .map(|e| e.mode == MODE_SYMLINK)
                .unwrap_or(false)
            {
                MODE_SYMLINK
            } else {
                mode
            }
        } else {
            mode
        };

        let data = if meta.file_type().is_symlink() {
            let target = std::fs::read_link(&abs_path)?;
            target.to_string_lossy().into_owned().into_bytes()
        } else {
            std::fs::read(&abs_path)
                .with_context(|| format!("cannot read '{}'", abs_path.display()))?
        };

        let oid = match repo.odb.write(grit_lib::objects::ObjectKind::Blob, &data) {
            Ok(oid) => oid,
            Err(err) => {
                if is_permission_denied_error(&err) {
                    eprintln!(
                        "error: insufficient permission for adding an object to repository database .git/objects"
                    );
                    eprintln!("error: {}: failed to insert into database", input_path.display());
                    eprintln!("fatal: Unable to process path {}", input_path.display());
                    std::process::exit(128);
                }
                return Err(anyhow::anyhow!("writing blob: {err}"));
            }
        };

        let entry = entry_from_stat(&abs_path, &rel_bytes, oid, mode)
            .with_context(|| format!("stat failed for '{}'", abs_path.display()))?;

        index.add_or_replace(entry);

        // Apply --chmod after adding the entry.
        if let Some((ref chmod_val, new_mode)) = chmod_for_path {
            if let Some(e) = index.get_mut(&rel_bytes, 0) {
                e.mode = new_mode;
            }
            if args.verbose {
                verbose_lines.push(format!("add '{}'", input_path.display()));
                verbose_lines.push(format!("chmod {} '{}'", chmod_val, input_path.display()));
            }
        }
    }

    if args.refresh || args.really_refresh || args.again {
        let refresh_only_mode = !args.add
            && !args.remove
            && !args.force_remove
            && !args.assume_unchanged
            && !args.no_assume_unchanged
            && !args.skip_worktree
            && !args.no_skip_worktree
            && args.cacheinfo.is_empty()
            && args.chmod.is_empty()
            && paths.is_empty();
        let index_mtime_sec = {
            use std::os::unix::fs::MetadataExt;
            std::fs::metadata(&index_path)
                .ok()
                .map(|m| m.mtime() as u32)
                .unwrap_or(0)
        };
        // Re-stat all entries
        let only_paths = if paths.is_empty() {
            None
        } else {
            Some(
                paths
                    .iter()
                    .map(|p| path_to_bytes(p.as_path()))
                    .collect::<Result<std::collections::HashSet<_>>>()?,
            )
        };
        let (stale, refresh_needs_write) = refresh_index(
            &mut index,
            work_tree,
            &repo.odb,
            args.ignore_missing,
            args.unmerged,
            args.ignore_submodules,
            args.really_refresh,
            index_mtime_sec,
            only_paths.as_ref(),
        )?;
        let quiet_refresh = args.quiet || matches!(std::env::var("GIT_QUIET"), Ok(v) if !v.is_empty());
        if !stale.is_empty() {
            if refresh_only_mode && refresh_needs_write {
                index.write(&index_path).context("writing index")?;
            }
            if !quiet_refresh {
                for path in stale {
                    println!("{path}");
                }
                std::process::exit(1);
            }
        }
        if refresh_only_mode && !refresh_needs_write {
            return Ok(());
        }
    }

    index.write(&index_path).context("writing index")?;
    if args.verbose {
        for line in verbose_lines {
            println!("{line}");
        }
    }
    Ok(())
}

fn set_local_index_version(git_dir: &Path, version: u32) -> Result<()> {
    let config_path = git_dir.join("config");
    let mut file = match ConfigFile::from_path(&config_path, ConfigScope::Local)? {
        Some(f) => f,
        None => ConfigFile::parse(&config_path, "", ConfigScope::Local)?,
    };
    file.set("index.version", &version.to_string())?;
    file.write()?;
    Ok(())
}

/// Process `--index-info` stdin: lines of `"<mode> <oid>\t<path>"`.
fn run_index_info(index: &mut Index, index_path: &std::path::Path, _odb: &Odb) -> Result<()> {
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

    index.write(index_path).context("writing index")?;
    Ok(())
}

/// Re-stat all tracked files, updating mtime/ctime/size.
fn refresh_index(
    index: &mut Index,
    work_tree: &std::path::Path,
    _odb: &Odb,
    ignore_missing: bool,
    allow_unmerged: bool,
    ignore_submodules: bool,
    really_refresh: bool,
    index_mtime_sec: u32,
    only_paths: Option<&std::collections::HashSet<Vec<u8>>>,
) -> Result<(Vec<String>, bool)> {
    use std::os::unix::fs::MetadataExt;

    let mut stale_paths: Vec<String> = Vec::new();
    let mut needs_write = false;
    let mut seen_unmerged = std::collections::HashSet::<Vec<u8>>::new();

    for entry in &mut index.entries {
        if let Some(filter) = only_paths {
            if !filter.contains(&entry.path) {
                continue;
            }
        }

        let path = std::path::Path::new(
            std::str::from_utf8(&entry.path)
                .map_err(|_| anyhow::anyhow!("non-UTF-8 path in index"))?,
        );
        let path_str = path.to_string_lossy().to_string();

        if entry.stage() != 0 {
            if !allow_unmerged && seen_unmerged.insert(entry.path.clone()) {
                stale_paths.push(path_str);
            }
            continue;
        }

        if entry.mode == MODE_GITLINK {
            if ignore_submodules {
                continue;
            }
            let abs = work_tree.join(path);
            let dot_git = abs.join(".git");
            let current = if dot_git.exists() {
                let sub_git_dir = resolve_gitdir(&dot_git)?;
                let head_path = sub_git_dir.join("HEAD");
                let head_content = std::fs::read_to_string(&head_path)?;
                let head_content = head_content.trim();
                Some(if let Some(refname) = head_content.strip_prefix("ref: ") {
                    let ref_path = sub_git_dir.join(refname);
                    let ref_content = std::fs::read_to_string(&ref_path)?;
                    ref_content.trim().parse().context("invalid submodule ref oid")?
                } else {
                    head_content.parse().context("invalid submodule HEAD oid")?
                })
            } else {
                None
            };
            if current != Some(entry.oid) {
                stale_paths.push(path_str);
            }
            continue;
        }

        let abs = work_tree.join(path);
        let meta = match std::fs::symlink_metadata(&abs) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                if !ignore_missing {
                    stale_paths.push(path_str);
                }
                continue;
            }
            Err(e) => return Err(e.into()),
        };

        if meta.file_type().is_symlink() {
            let target = std::fs::read_link(&abs)?;
            let oid = Odb::hash_object_data(
                grit_lib::objects::ObjectKind::Blob,
                target.as_os_str().as_encoded_bytes(),
            );
            if oid != entry.oid || entry.mode != MODE_SYMLINK {
                stale_paths.push(path_str);
                continue;
            }
            if meta.mtime() as u32 == index_mtime_sec {
                needs_write = true;
            }
        } else if meta.file_type().is_file() {
            let mode = if meta.mode() & 0o111 != 0 {
                MODE_EXECUTABLE
            } else {
                MODE_REGULAR
            };
            let data = std::fs::read(&abs)?;
            let oid = Odb::hash_object_data(grit_lib::objects::ObjectKind::Blob, &data);
            if oid != entry.oid || entry.mode != mode {
                stale_paths.push(path_str);
                continue;
            }
            if meta.mtime() as u32 == index_mtime_sec {
                needs_write = true;
            }
        } else {
            stale_paths.push(path_str);
            continue;
        }

        if really_refresh {
            let ctime_sec = meta.ctime() as u32;
            let ctime_nsec = meta.ctime_nsec() as u32;
            let mtime_sec = meta.mtime() as u32;
            let mtime_nsec = meta.mtime_nsec() as u32;
            let dev = meta.dev() as u32;
            let ino = meta.ino() as u32;
            let uid = meta.uid();
            let gid = meta.gid();
            let size = meta.size() as u32;
            if entry.ctime_sec != ctime_sec
                || entry.ctime_nsec != ctime_nsec
                || entry.mtime_sec != mtime_sec
                || entry.mtime_nsec != mtime_nsec
                || entry.dev != dev
                || entry.ino != ino
                || entry.uid != uid
                || entry.gid != gid
                || entry.size != size
            {
                needs_write = true;
                entry.ctime_sec = ctime_sec;
                entry.ctime_nsec = ctime_nsec;
                entry.mtime_sec = mtime_sec;
                entry.mtime_nsec = mtime_nsec;
                entry.dev = dev;
                entry.ino = ino;
                entry.uid = uid;
                entry.gid = gid;
                entry.size = size;
            }
        }
    }
    Ok((stale_paths, needs_write))
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
    if meta.is_dir() { return Ok(dot_git.to_path_buf()); }
    let content = std::fs::read_to_string(dot_git)?;
    let content = content.trim();
    let target = content.strip_prefix("gitdir: ")
        .ok_or_else(|| anyhow::anyhow!("invalid .git file"))?;
    let target_path = Path::new(target);
    if target_path.is_absolute() {
        Ok(target_path.to_path_buf())
    } else {
        Ok(dot_git.parent().unwrap_or(Path::new(".")).join(target_path))
    }
}

fn core_symlinks_enabled(git_dir: &Path) -> bool {
    ConfigSet::load(Some(git_dir), true)
        .ok()
        .and_then(|cfg| cfg.get_bool("core.symlinks"))
        .and_then(|v| v.ok())
        .unwrap_or(true)
}

fn is_permission_denied_error(err: &grit_lib::error::Error) -> bool {
    err.to_string().contains("Permission denied")
        || err.to_string().contains("permission denied")
}

fn has_df_conflict(index: &Index, path: &[u8]) -> bool {
    index
        .entries
        .iter()
        .filter(|e| e.stage() == 0)
        .map(|e| e.path.as_slice())
        .any(|existing| {
            existing != path
                && (is_tree_prefix(existing, path) || is_tree_prefix(path, existing))
        })
}

fn is_tree_prefix(prefix: &[u8], full: &[u8]) -> bool {
    full.len() > prefix.len()
        && full.starts_with(prefix)
        && full.get(prefix.len()) == Some(&b'/')
}
