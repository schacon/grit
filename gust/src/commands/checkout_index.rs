//! `gust checkout-index` — check out files from the index into the working tree.

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use std::io::{self, BufRead};
use std::path::PathBuf;

use gust_lib::index::{Index, MODE_EXECUTABLE, MODE_SYMLINK};
use gust_lib::repo::Repository;

/// Arguments for `gust checkout-index`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Checkout all files.
    #[arg(short = 'a', long = "all")]
    pub all: bool,

    /// Force overwrite existing files.
    #[arg(short = 'f', long)]
    pub force: bool,

    /// Update stat info in the index.
    #[arg(short = 'u')]
    pub update_stat: bool,

    /// Be quiet.
    #[arg(short = 'q', long)]
    pub quiet: bool,

    /// Don't actually check out files.
    #[arg(short = 'n', long = "no-create")]
    pub dry_run: bool,

    /// Create leading directories.
    #[arg(long = "mkdir")]
    pub mkdir: bool,

    /// Read paths from stdin (NUL terminated if -z).
    #[arg(long)]
    pub stdin: bool,

    /// \0 line termination for --stdin.
    #[arg(short = 'z')]
    pub null_terminated: bool,

    /// Prefix to prepend to all checked-out paths.
    #[arg(long)]
    pub prefix: Option<String>,

    /// Write to temp files instead of actual paths.
    #[arg(long)]
    pub temp: bool,

    /// Stage to check out (1, 2, or 3).
    #[arg(long = "stage")]
    pub stage: Option<u8>,

    /// Files to check out (if not --all or --stdin).
    pub files: Vec<PathBuf>,
}

/// Run `gust checkout-index`.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let work_tree = repo
        .work_tree
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("cannot checkout-index in bare repository"))?
        .to_path_buf();

    let index = Index::load(&repo.index_path()).context("loading index")?;

    let target_stage = args.stage.unwrap_or(0);

    let prefix = args.prefix.as_deref().unwrap_or("");

    if args.all {
        for entry in &index.entries {
            if entry.stage() != target_stage {
                continue;
            }
            checkout_entry(&repo, &index, entry, &work_tree, prefix, &args)?;
        }
    } else if args.stdin {
        let paths = read_stdin_paths(args.null_terminated)?;
        for path in paths {
            let path_bytes = {
                use std::os::unix::ffi::OsStrExt;
                path.as_os_str().as_bytes().to_vec()
            };
            let entry = index
                .get(&path_bytes, target_stage)
                .ok_or_else(|| anyhow::anyhow!("'{}' not in index", path.display()))?;
            checkout_entry(&repo, &index, entry, &work_tree, prefix, &args)?;
        }
    } else {
        for path in &args.files {
            let path_bytes = {
                use std::os::unix::ffi::OsStrExt;
                path.as_os_str().as_bytes().to_vec()
            };
            let entry = index
                .get(&path_bytes, target_stage)
                .ok_or_else(|| anyhow::anyhow!("'{}' not in index", path.display()))?;
            checkout_entry(&repo, &index, entry, &work_tree, prefix, &args)?;
        }
    }

    Ok(())
}

fn checkout_entry(
    repo: &Repository,
    _index: &Index,
    entry: &gust_lib::index::IndexEntry,
    work_tree: &std::path::Path,
    prefix: &str,
    args: &Args,
) -> Result<()> {
    let path_str = String::from_utf8_lossy(&entry.path).into_owned();
    let rel_path = format!("{prefix}{path_str}");
    let abs_path = work_tree.join(&rel_path);

    if abs_path.exists() && !args.force {
        if !args.quiet {
            eprintln!("warning: '{rel_path}' already exists, skipping (use --force to override)");
        }
        return Ok(());
    }

    if args.dry_run {
        return Ok(());
    }

    if let Some(parent) = abs_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let obj = repo
        .odb
        .read(&entry.oid)
        .context("reading object for checkout")?;

    if entry.mode == MODE_SYMLINK {
        let target = String::from_utf8(obj.data.clone())
            .map_err(|_| anyhow::anyhow!("symlink target is not UTF-8"))?;
        if abs_path.exists() {
            std::fs::remove_file(&abs_path)?;
        }
        std::os::unix::fs::symlink(&target, &abs_path)?;
    } else {
        std::fs::write(&abs_path, &obj.data)?;

        // Set executable bit if needed
        if entry.mode == MODE_EXECUTABLE {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&abs_path)?.permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&abs_path, perms)?;
        }
    }

    Ok(())
}

fn read_stdin_paths(null_terminated: bool) -> Result<Vec<PathBuf>> {
    let stdin = io::stdin();
    let mut paths = Vec::new();

    if null_terminated {
        use io::Read;
        let mut buf = Vec::new();
        io::stdin().read_to_end(&mut buf)?;
        for part in buf.split(|&b| b == 0) {
            if !part.is_empty() {
                let s = std::str::from_utf8(part).context("non-UTF-8 path")?;
                paths.push(PathBuf::from(s));
            }
        }
    } else {
        for line in stdin.lock().lines() {
            let line = line?;
            if !line.is_empty() {
                paths.push(PathBuf::from(line));
            }
        }
    }
    Ok(paths)
}
