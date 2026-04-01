//! `gust checkout-index` — check out files from the index into the working tree.

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use std::fs;
use std::io::{self, BufRead};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use gust_lib::index::{entry_from_stat, Index, MODE_EXECUTABLE, MODE_REGULAR, MODE_SYMLINK};
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

    /// Directory to use for --temp output files.
    #[arg(long = "tmpdir", requires = "temp")]
    pub tmpdir: Option<PathBuf>,

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

    let mut index = Index::load(&repo.index_path()).context("loading index")?;

    let target_stage = args.stage.unwrap_or(0);

    let prefix = args.prefix.as_deref().unwrap_or("");
    let core_symlinks = repo_core_symlinks(&repo)?;
    let mut should_write_index = false;

    let mut selected_entries = Vec::new();
    if args.all {
        for entry in &index.entries {
            if entry.stage() != target_stage {
                continue;
            }
            selected_entries.push(entry.path.clone());
        }
    } else if args.stdin {
        let paths = read_stdin_paths(args.null_terminated)?;
        for path in paths {
            use std::os::unix::ffi::OsStrExt;
            let path_bytes = path.as_os_str().as_bytes().to_vec();
            if index.get(&path_bytes, target_stage).is_none() {
                return Err(anyhow::anyhow!("'{}' not in index", path.display()));
            }
            selected_entries.push(path_bytes);
        }
    } else {
        for path in &args.files {
            use std::os::unix::ffi::OsStrExt;
            let path_bytes = path.as_os_str().as_bytes().to_vec();
            if index.get(&path_bytes, target_stage).is_none() {
                return Err(anyhow::anyhow!("'{}' not in index", path.display()));
            }
            selected_entries.push(path_bytes);
        }
    }

    for path_bytes in selected_entries {
        let entry = index
            .get(&path_bytes, target_stage)
            .ok_or_else(|| {
                anyhow::anyhow!("'{}' not in index", String::from_utf8_lossy(&path_bytes))
            })?
            .clone();
        let abs_path = checkout_entry(&repo, &entry, &work_tree, prefix, &args, core_symlinks)?;

        if args.update_stat && !args.dry_run && !args.temp {
            let rel_path = format!("{prefix}{}", String::from_utf8_lossy(&entry.path));
            let rel_bytes = rel_path.into_bytes();
            let mode = if entry.mode == MODE_EXECUTABLE {
                MODE_EXECUTABLE
            } else {
                MODE_REGULAR
            };
            let mut refreshed = entry_from_stat(&abs_path, &rel_bytes, entry.oid, mode)
                .context("refreshing index stat data")?;
            refreshed.flags = entry.flags;
            refreshed.flags_extended = entry.flags_extended;
            if let Some(existing) = index.get_mut(&path_bytes, target_stage) {
                *existing = refreshed;
                should_write_index = true;
            }
        }
    }

    if should_write_index {
        index
            .write(&repo.index_path())
            .context("writing refreshed index")?;
    }

    Ok(())
}

fn checkout_entry(
    repo: &Repository,
    entry: &gust_lib::index::IndexEntry,
    work_tree: &Path,
    prefix: &str,
    args: &Args,
    core_symlinks: bool,
) -> Result<PathBuf> {
    let path_str = String::from_utf8_lossy(&entry.path).into_owned();
    let rel_path = format!("{prefix}{path_str}");
    let abs_path = work_tree.join(&rel_path);

    if args.temp {
        if args.dry_run {
            return Ok(abs_path);
        }
        let tmp_path = write_temp_checkout_file(&args.tmpdir, &entry.oid, &repo.odb)?;
        println!("{}\t{}", tmp_path.display(), path_str);
        return Ok(tmp_path);
    }

    if abs_path.exists() && !args.force {
        if !args.quiet {
            eprintln!("warning: '{rel_path}' already exists, skipping (use --force to override)");
        }
        return Ok(abs_path);
    }

    if args.dry_run {
        return Ok(abs_path);
    }

    if let Some(parent) = abs_path.parent() {
        if !parent.exists() {
            if args.mkdir {
                fs::create_dir_all(parent)?;
            } else {
                return Err(anyhow::anyhow!(
                    "leading directories do not exist for '{}'; use --mkdir",
                    rel_path
                ));
            }
        }
    }

    let obj = repo
        .odb
        .read(&entry.oid)
        .context("reading object for checkout")?;

    if entry.mode == MODE_SYMLINK && core_symlinks {
        let target = String::from_utf8(obj.data.clone())
            .map_err(|_| anyhow::anyhow!("symlink target is not UTF-8"))?;
        if abs_path.exists() {
            fs::remove_file(&abs_path)?;
        }
        std::os::unix::fs::symlink(&target, &abs_path)?;
    } else {
        fs::write(&abs_path, &obj.data)?;

        // Set executable bit if needed
        if entry.mode == MODE_EXECUTABLE {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&abs_path)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&abs_path, perms)?;
        }
    }

    Ok(abs_path)
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

fn repo_core_symlinks(repo: &Repository) -> Result<bool> {
    let config_path = repo.git_dir.join("config");
    let config = match fs::read_to_string(&config_path) {
        Ok(c) => c,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(true),
        Err(e) => return Err(e.into()),
    };

    let mut in_core = false;
    for line in config.lines() {
        let t = line.trim();
        if t.starts_with('[') && t.ends_with(']') {
            in_core = t.eq_ignore_ascii_case("[core]");
            continue;
        }
        if !in_core {
            continue;
        }
        if let Some((k, v)) = t.split_once('=') {
            if k.trim().eq_ignore_ascii_case("symlinks") {
                return Ok(!v.trim().eq_ignore_ascii_case("false"));
            }
        }
    }
    Ok(true)
}

fn write_temp_checkout_file(
    tmpdir: &Option<PathBuf>,
    oid: &gust_lib::objects::ObjectId,
    odb: &gust_lib::odb::Odb,
) -> Result<PathBuf> {
    let base = tmpdir.clone().unwrap_or_else(std::env::temp_dir);
    fs::create_dir_all(&base)?;
    let obj = odb.read(oid).context("reading object for checkout")?;

    let pid = std::process::id();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();

    for seq in 0..1024u32 {
        let candidate = base.join(format!("gust-checkout-{pid}-{nanos}-{seq}"));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(mut file) => {
                use std::io::Write;
                file.write_all(&obj.data)?;
                return Ok(candidate);
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.into()),
        }
    }

    Err(anyhow::anyhow!(
        "unable to create temporary checkout file in {}",
        base.display()
    ))
}
