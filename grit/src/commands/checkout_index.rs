//! `grit checkout-index` — check out files from the index into the working tree.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::config::ConfigSet;
use grit_lib::crlf;
use grit_lib::objects::ObjectKind;
use std::io::{self, BufRead};
use std::path::Component;
use std::path::PathBuf;

use grit_lib::index::{Index, MODE_EXECUTABLE, MODE_SYMLINK};
use grit_lib::repo::Repository;

/// Arguments for `grit checkout-index`.
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

    /// Directory for temporary files (used with --temp).
    #[arg(long = "tmpdir", value_name = "dir")]
    pub tmpdir: Option<PathBuf>,

    /// Stage to check out (1, 2, or 3).
    #[arg(long = "stage")]
    pub stage: Option<u8>,

    /// Ignore skip-worktree bits and checkout all entries.
    #[arg(long = "ignore-skip-worktree-bits")]
    pub ignore_skip_worktree_bits: bool,

    /// Files to check out (if not --all or --stdin).
    pub files: Vec<PathBuf>,
}

/// Run `grit checkout-index`.
pub fn run(args: Args) -> Result<()> {
    if args.tmpdir.is_some() && !args.temp {
        bail!("--tmpdir requires --temp");
    }

    let repo = Repository::discover(None).context("not a git repository")?;
    let work_tree = repo
        .work_tree
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("cannot checkout-index in bare repository"))?
        .to_path_buf();

    let index_path = repo.index_path();
    let mut index = Index::load(&index_path).context("loading index")?;

    let target_stage = args.stage.unwrap_or(0);
    let cwd = std::env::current_dir().context("resolving current directory")?;

    let prefix = args.prefix.as_deref().unwrap_or("");
    let symlinks_enabled = core_symlinks_enabled(&repo);
    let mut selected_paths: Vec<Vec<u8>> = Vec::new();
    let mut index_needs_write = false;

    if args.all {
        for entry in &index.entries {
            if entry.stage() != target_stage {
                continue;
            }
            if entry.skip_worktree() && !args.ignore_skip_worktree_bits {
                continue;
            }
            selected_paths.push(entry.path.clone());
        }
    } else if args.stdin {
        let paths = read_stdin_paths(args.null_terminated)?;
        for input_path in paths {
            let repo_path = resolve_repo_path(&work_tree, &cwd, &input_path)?;
            let path_bytes = path_to_bytes(&repo_path);
            if index.get(&path_bytes, target_stage).is_none() {
                if args.quiet {
                    continue;
                }
                bail!("'{}' is not in the cache", input_path.display());
            }
            selected_paths.push(path_bytes);
        }
    } else {
        for input_path in &args.files {
            let repo_path = resolve_repo_path(&work_tree, &cwd, input_path)?;
            let path_bytes = path_to_bytes(&repo_path);
            let maybe_entry = index.get(&path_bytes, target_stage);
            if maybe_entry.is_none()
                || (maybe_entry
                    .is_some_and(|e| e.skip_worktree() && !args.ignore_skip_worktree_bits))
            {
                if args.quiet {
                    continue;
                }
                bail!("'{}' is not in the cache", input_path.display());
            }
            selected_paths.push(path_bytes);
        }
    }

    let mut has_errors = false;
    for path in selected_paths {
        let entry = index
            .get(&path, target_stage)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("'{}' is not in the cache", String::from_utf8_lossy(&path)))?;
        match checkout_entry(&repo, &entry, &work_tree, prefix, symlinks_enabled, &args) {
            Ok(outcome) => {
                if let Some(updated) = outcome.updated_entry {
                    index.add_or_replace(updated);
                    index_needs_write = true;
                }
                if let Some(line) = outcome.temp_output {
                    println!("{line}");
                }
            }
            Err(_) => {
                has_errors = true;
            }
        }
    }

    if index_needs_write {
        index.write(&index_path).context("writing index")?;
    }

    if has_errors {
        std::process::exit(1);
    }

    Ok(())
}

#[derive(Default)]
struct CheckoutOutcome {
    updated_entry: Option<grit_lib::index::IndexEntry>,
    temp_output: Option<String>,
}

fn checkout_entry(
    repo: &Repository,
    entry: &grit_lib::index::IndexEntry,
    work_tree: &std::path::Path,
    prefix: &str,
    symlinks_enabled: bool,
    args: &Args,
) -> Result<CheckoutOutcome> {
    let path_str = String::from_utf8_lossy(&entry.path).into_owned();
    let rel_path = format!("{prefix}{path_str}");
    let abs_path = work_tree.join(&rel_path);
    let mut outcome = CheckoutOutcome::default();

    if args.dry_run {
        return Ok(outcome);
    }

    // Submodule entries cannot be checked out
    if entry.mode == 0o160000 {
        if args.temp {
            eprintln!("cannot create temporary submodule {path_str}");
            return Err(anyhow::anyhow!("cannot create temporary submodule {path_str}"));
        }
        return Ok(outcome);
    }

    let obj = match repo.odb.read(&entry.oid) {
        Ok(obj) => obj,
        Err(_) => {
            eprintln!("unable to read sha1 file of {path_str} ({})", entry.oid);
            return Err(anyhow::anyhow!("unable to read sha1 file of {path_str} ({})", entry.oid));
        }
    };
    if obj.kind != ObjectKind::Blob {
        bail!("cannot checkout non-blob at '{path_str}'");
    }

    if args.temp {
        let tmp_path = write_temp_blob(entry, &obj.data, args)?;
        outcome.temp_output = Some(format!("{}\t{path_str}", tmp_path.display()));
        return Ok(outcome);
    }

    let existing_meta = std::fs::symlink_metadata(&abs_path).ok();
    let should_skip_existing =
        existing_meta.is_some() && !args.force && !args.ignore_skip_worktree_bits;
    if should_skip_existing {
        if !args.quiet {
            eprintln!("warning: '{rel_path}' already exists, skipping (use --force to override)");
        }
        return Ok(outcome);
    }

    if let Some(parent) = abs_path.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent)?;
        }
    }

    if abs_path.is_dir() {
        std::fs::remove_dir_all(&abs_path)?;
    } else if existing_meta.is_some() {
        std::fs::remove_file(&abs_path)?;
    }

    if entry.mode == MODE_SYMLINK && symlinks_enabled {
        let target = String::from_utf8(obj.data)
            .map_err(|_| anyhow::anyhow!("symlink target is not UTF-8"))?;
        std::os::unix::fs::symlink(&target, &abs_path)?;
    } else {
        // Apply CRLF / smudge conversion
        let data = {
            let config = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
            let conv = crlf::ConversionConfig::from_config(&config);
            let mut attrs = crlf::load_gitattributes(work_tree);
            if attrs.is_empty() {
                if let Ok(idx) = Index::load(&repo.index_path()) {
                    attrs = crlf::load_gitattributes_from_index(&idx, &repo.odb);
                }
            }
            let file_attrs = crlf::get_file_attrs(&attrs, &path_str, &config);
            let oid_hex = format!("{}", entry.oid);
            crlf::convert_to_worktree(&obj.data, &path_str, &conv, &file_attrs, Some(&oid_hex))
        };
        std::fs::write(&abs_path, &data)?;

        // Set executable bit if needed
        if entry.mode == MODE_EXECUTABLE {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&abs_path)?.permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&abs_path, perms)?;
        }
    }

    if args.update_stat && prefix.is_empty() && entry.stage() == 0 {
        outcome.updated_entry = Some(refresh_stat_for_entry(entry, &abs_path)?);
    }

    Ok(outcome)
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

fn refresh_stat_for_entry(
    entry: &grit_lib::index::IndexEntry,
    abs_path: &std::path::Path,
) -> Result<grit_lib::index::IndexEntry> {
    use std::os::unix::fs::MetadataExt;
    let meta = std::fs::symlink_metadata(abs_path)
        .with_context(|| format!("cannot stat '{}'", abs_path.display()))?;
    let mut refreshed = entry.clone();
    refreshed.ctime_sec = meta.ctime() as u32;
    refreshed.ctime_nsec = meta.ctime_nsec() as u32;
    refreshed.mtime_sec = meta.mtime() as u32;
    refreshed.mtime_nsec = meta.mtime_nsec() as u32;
    refreshed.dev = meta.dev() as u32;
    refreshed.ino = meta.ino() as u32;
    refreshed.uid = meta.uid();
    refreshed.gid = meta.gid();
    refreshed.size = meta.size() as u32;
    Ok(refreshed)
}

fn write_temp_blob(
    entry: &grit_lib::index::IndexEntry,
    data: &[u8],
    args: &Args,
) -> Result<PathBuf> {
    use std::fs::OpenOptions;
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    use std::time::{SystemTime, UNIX_EPOCH};

    let base_dir = args
        .tmpdir
        .as_ref()
        .cloned()
        .unwrap_or_else(|| PathBuf::from("."));
    if !base_dir.exists() {
        std::fs::create_dir_all(&base_dir).with_context(|| {
            format!(
                "cannot create tmpdir '{}'",
                base_dir.as_path().to_string_lossy()
            )
        })?;
    }

    let pid = std::process::id();
    for attempt in 0..1000u32 {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let name = format!(".merge_file_{pid}_{nanos}_{attempt}");
        let candidate = base_dir.join(name);
        let mut file = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&candidate)
        {
            Ok(file) => file,
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(err) => {
                return Err(anyhow::anyhow!(
                    "cannot create temp file '{}': {err}",
                    candidate.display()
                ));
            }
        };

        file.write_all(data)
            .with_context(|| format!("cannot write temp file '{}'", candidate.display()))?;

        if entry.mode == MODE_EXECUTABLE {
            let mut perms = std::fs::metadata(&candidate)?.permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&candidate, perms)?;
        }

        return Ok(candidate);
    }

    bail!(
        "unable to create unique temporary file in '{}'",
        base_dir.display()
    )
}

fn core_symlinks_enabled(repo: &Repository) -> bool {
    let config_path = repo.git_dir.join("config");
    let content = match std::fs::read_to_string(config_path) {
        Ok(c) => c,
        Err(_) => return true,
    };

    let mut in_core = false;
    for raw in content.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            let section = line[1..line.len() - 1].trim().to_ascii_lowercase();
            in_core = section == "core";
            continue;
        }
        if !in_core {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim().eq_ignore_ascii_case("symlinks") {
            let v = value.trim().to_ascii_lowercase();
            if matches!(v.as_str(), "false" | "no" | "off" | "0") {
                return false;
            }
            if matches!(v.as_str(), "true" | "yes" | "on" | "1") {
                return true;
            }
        }
    }
    true
}

fn resolve_repo_path(
    work_tree: &std::path::Path,
    cwd: &std::path::Path,
    input: &std::path::Path,
) -> Result<PathBuf> {
    let combined = if input.is_absolute() {
        input.to_path_buf()
    } else {
        cwd.join(input)
    };
    let normalized = normalize_path(&combined);
    let rel = normalized
        .strip_prefix(work_tree)
        .with_context(|| format!("path '{}' is outside repository work tree", input.display()))?;
    Ok(rel.to_path_buf())
}

fn normalize_path(path: &std::path::Path) -> PathBuf {
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

fn path_to_bytes(path: &std::path::Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str().as_bytes().to_vec()
}
