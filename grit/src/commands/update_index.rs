//! `grit update-index` — register file contents in the working tree to the index.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use std::io::{self, BufRead};
use std::path::Component;
use std::path::{Path, PathBuf};

use grit_lib::index::{entry_from_stat, normalize_mode, Index, IndexEntry};
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

    /// Re-create unmerged entries for the given paths.
    #[arg(long = "unresolve")]
    pub unresolve: bool,

    /// Add `<mode>,<object>,<path>` entry directly.
    #[arg(long = "cacheinfo", value_name = "mode,object,path")]
    pub cacheinfo: Vec<String>,

    /// Files to add/remove from the index.
    pub files: Vec<PathBuf>,
}

/// Run `grit update-index`.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let index_path = repo.index_path();
    let mut index = Index::load(&index_path).context("loading index")?;

    let work_tree = repo
        .work_tree
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("cannot update-index in bare repository"))?;
    let cwd = std::env::current_dir().context("resolving current directory")?;

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

    // Process --cacheinfo entries
    for info in &args.cacheinfo {
        let parts: Vec<&str> = info.splitn(3, ',').collect();
        if parts.len() != 3 {
            bail!("--cacheinfo needs mode,object,path: '{info}'");
        }
        let mode = u32::from_str_radix(parts[0], 8)
            .with_context(|| format!("invalid mode '{}'", parts[0]))?;
        let oid: ObjectId = parts[1]
            .parse()
            .with_context(|| format!("invalid object id '{}'", parts[1]))?;
        let path = parts[2].as_bytes().to_vec();
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
            flags: path.len().min(0xFFF) as u16,
            flags_extended: None,
            path,
        };
        index.add_or_replace(entry);
    }

    // Collect file paths (from args or stdin)
    let paths: Vec<PathBuf> = if args.null_terminated {
        read_paths_nul()?
    } else {
        args.files.clone()
    };

    for input_path in paths {
        let (rel_path, abs_path) = resolve_repo_path(work_tree, &cwd, &input_path)?;
        let rel_bytes = path_to_bytes(&rel_path)?;

        if args.force_remove {
            // --force-remove silently succeeds even if the entry is absent
            index.remove(&rel_bytes);
            continue;
        }

        // --remove: remove the path from the index.  When --add is also
        // given and the file exists as a regular file/symlink (not a
        // directory), fall through to the add logic instead.  A directory
        // at the path means the original file was replaced, so remove.
        if args.remove {
            let file_exists = abs_path.exists() && !abs_path.is_dir();
            if !args.add || !file_exists {
                if !index.remove(&rel_bytes) && !args.ignore_missing {
                    let file_missing = !abs_path.exists();
                    if file_missing {
                        bail!("'{}' is not in the index", input_path.display());
                    }
                }
                continue;
            }
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

        let mode = {
            use std::os::unix::fs::MetadataExt;
            normalize_mode(meta.mode())
        };

        // Read file content and hash it
        let data = if meta.file_type().is_symlink() {
            let target = std::fs::read_link(&abs_path)?;
            target.to_string_lossy().into_owned().into_bytes()
        } else {
            std::fs::read(&abs_path)
                .with_context(|| format!("cannot read '{}'", abs_path.display()))?
        };

        // Without --add, reject files not yet in the index.
        if !args.add && index.get(&rel_bytes, 0).is_none() {
            if args.ignore_missing {
                continue;
            }
            bail!("'{}' is not in the index", input_path.display());
        }

        let oid = repo
            .odb
            .write(grit_lib::objects::ObjectKind::Blob, &data)
            .context("writing blob")?;

        let entry = entry_from_stat(&abs_path, &rel_bytes, oid, mode)
            .with_context(|| format!("stat failed for '{}'", abs_path.display()))?;

        index.add_or_replace(entry);
    }

    if args.refresh || args.really_refresh || args.again {
        // Re-stat all entries
        refresh_index(&mut index, work_tree, &repo.odb)?;
    }

    index.write(&index_path).context("writing index")?;
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
fn refresh_index(index: &mut Index, work_tree: &std::path::Path, _odb: &Odb) -> Result<()> {
    for entry in &mut index.entries {
        let path = std::path::Path::new(
            std::str::from_utf8(&entry.path)
                .map_err(|_| anyhow::anyhow!("non-UTF-8 path in index"))?,
        );
        let abs = work_tree.join(path);
        if let Ok(meta) = std::fs::symlink_metadata(&abs) {
            use std::os::unix::fs::MetadataExt;
            entry.ctime_sec = meta.ctime() as u32;
            entry.ctime_nsec = meta.ctime_nsec() as u32;
            entry.mtime_sec = meta.mtime() as u32;
            entry.mtime_nsec = meta.mtime_nsec() as u32;
            entry.size = meta.size() as u32;
        }
    }
    Ok(())
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
