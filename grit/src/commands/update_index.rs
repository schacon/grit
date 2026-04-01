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

        if args.force_remove || args.remove {
            if !index.remove(&rel_bytes) && !args.force_remove && !args.ignore_missing {
                bail!("'{}' is not in the index", input_path.display());
            }
            continue;
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

        let oid = if args.add || index.get(&rel_bytes, 0).is_none() {
            repo.odb
                .write(grit_lib::objects::ObjectKind::Blob, &data)
                .context("writing blob")?
        } else {
            // Refreshing: keep existing OID but update stat
            index.get(&rel_bytes, 0).map(|e| e.oid).unwrap_or_else(|| {
                Odb::hash_object_data(grit_lib::objects::ObjectKind::Blob, &data)
            })
        };

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

        let (mode_str, oid_str) = match parts.len() {
            2 => (parts[0], parts[1]),
            3 => (parts[0], parts[2]), // skip type
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
