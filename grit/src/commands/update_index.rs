//! `grit update-index` — register file contents in the working tree to the index.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use std::io::{self, BufRead};
use std::collections::BTreeSet;
use std::path::Component;
use std::path::{Path, PathBuf};

use grit_lib::config::ConfigSet;
use grit_lib::crlf;
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

    /// Ignore submodule work tree state during --refresh.
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

    /// Set the execute bit on tracked files (+x or -x).
    #[arg(long = "chmod", value_name = "MODE", action = clap::ArgAction::Append)]
    pub chmod: Vec<String>,

    /// Replace the entire index (used with --index-info).
    #[arg(long = "replace")]
    pub replace: bool,

    /// Enable split index mode.
    #[arg(long = "split-index")]
    pub split_index: bool,

    /// Disable split index mode.
    #[arg(long = "no-split-index")]
    pub no_split_index: bool,

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

    let work_tree = repo
        .work_tree
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("cannot update-index in bare repository"))?;
    let config = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
    let conv = crlf::ConversionConfig::from_config(&config);
    let attrs = crlf::load_gitattributes(work_tree);
    let cwd = std::env::current_dir().context("resolving current directory")?;

    if args.show_index_version {
        println!("{}", index.version);
        return Ok(());
    }
    if let Some(v) = args.index_version {
        if !(2..=4).contains(&v) {
            bail!("bad index version {v}");
        }
        let old = index.version;
        index.version = v;
        if args.verbose {
            println!("index-version: was {old}, set to {v}");
        }
    }

    if args.index_info {
        return run_index_info(&mut index, &index_path, &repo.odb, args.null_terminated);
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
            if oid.is_zero() {
                let path_display = String::from_utf8_lossy(&path_bytes);
                if args.verbose {
                    println!("add '{path_display}'");
                }
                bail!("invalid object {mode_str} {oid_str} for '{path_display}'");
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
                path: path_bytes.clone(),
            };
            index.add_or_replace(entry);
            if args.verbose {
                if let Ok(path_str) = std::str::from_utf8(&path_bytes) {
                    println!("add '{path_str}'");
                } else {
                    println!("add '{}'", String::from_utf8_lossy(&path_bytes));
                }
            }
        }
    }

    // Collect file paths (from args or stdin)
    let paths: Vec<PathBuf> = if args.null_terminated {
        read_paths_nul()?
    } else {
        args.files.clone()
    };

    let chmod_mode_for = |path_idx: usize| -> Result<Option<&str>> {
        if args.chmod.is_empty() {
            return Ok(None);
        }
        if args.chmod.len() == 1 {
            return Ok(Some(args.chmod[0].as_str()));
        }
        if args.chmod.len() == paths.len() {
            return Ok(Some(args.chmod[path_idx].as_str()));
        }
        bail!("the argument '--chmod <MODE>' cannot be used multiple times");
    };

    for (path_idx, input_path) in paths.iter().enumerate() {
        let (rel_path, abs_path) = resolve_repo_path(work_tree, &cwd, &input_path)?;
        let rel_bytes = path_to_bytes(&rel_path)?;
        let chmod_mode = chmod_mode_for(path_idx)?;

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

        // --chmod=+x or --chmod=-x without --add: change the mode of an existing entry.
        if let Some(chmod_val) = chmod_mode {
            if !args.add {
                let new_mode = match chmod_val {
                    "+x" => 0o100755u32,
                    "-x" => 0o100644u32,
                    other => bail!("--chmod param '{}' must be either +x or -x", other),
                };
                if let Some(e) = index.get_mut(&rel_bytes, 0) {
                    e.mode = new_mode;
                    if args.verbose {
                        let rel = rel_path.to_string_lossy();
                        println!("add '{rel}'");
                        println!("chmod {} '{rel}'", chmod_val);
                    }
                } else {
                    bail!("'{}' is not in the index", input_path.display());
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

        let data = if meta.file_type().is_symlink() {
            let target = std::fs::read_link(&abs_path)?;
            target.to_string_lossy().into_owned().into_bytes()
        } else {
            let raw = std::fs::read(&abs_path)
                .with_context(|| format!("cannot read '{}'", abs_path.display()))?;
            let rel_path = rel_path.to_string_lossy();
            let file_attrs = crlf::get_file_attrs(&attrs, &rel_path, &config);
            let mut conv_for_hash = conv.clone();
            conv_for_hash.safecrlf = crlf::SafeCrlf::False;
            crlf::convert_to_git(&raw, &rel_path, &conv_for_hash, &file_attrs).unwrap_or(raw)
        };

        let oid = match repo.odb.write(grit_lib::objects::ObjectKind::Blob, &data) {
            Ok(oid) => oid,
            Err(err) => {
                if is_unwritable_odb_error(&err) {
                    eprintln!(
                        "error: insufficient permission for adding an object to repository database .git/objects"
                    );
                    eprintln!(
                        "error: {}: failed to insert into database",
                        rel_path.display()
                    );
                    eprintln!("fatal: Unable to process path {}", rel_path.display());
                    std::process::exit(1);
                }
                return Err(err.into());
            }
        };

        let entry = entry_from_stat(&abs_path, &rel_bytes, oid, mode)
            .with_context(|| format!("stat failed for '{}'", abs_path.display()))?;

        index.add_or_replace(entry);
        if args.verbose {
            let rel = rel_path.to_string_lossy();
            println!("add '{rel}'");
        }

        // Apply --chmod after adding the entry.
        if let Some(chmod_val) = chmod_mode {
            let new_mode = match chmod_val {
                "+x" => 0o100755u32,
                "-x" => 0o100644u32,
                other => bail!("--chmod param '{}' must be either +x or -x", other),
            };
            if let Some(e) = index.get_mut(&rel_bytes, 0) {
                e.mode = new_mode;
                if args.verbose {
                    let rel = rel_path.to_string_lossy();
                    println!("chmod {} '{rel}'", chmod_val);
                }
            }
        }
    }

    let mut refresh_had_issues = false;
    let mut refresh_had_metadata_changes = false;
    if args.refresh || args.really_refresh || args.again {
        // Re-stat all entries, reporting paths that are not up-to-date.
        let (had_issues, had_metadata_changes) = refresh_index(
            &mut index,
            work_tree,
            &repo.odb,
            &conv,
            &attrs,
            &config,
            args.ignore_missing,
            args.unmerged,
            args.ignore_submodules,
            args.quiet,
        )?;
        refresh_had_issues = had_issues;
        refresh_had_metadata_changes = had_metadata_changes;
    }

    let only_refresh_like = (args.refresh || args.really_refresh || args.again)
        && args.files.is_empty()
        && args.cacheinfo.is_empty()
        && !args.assume_unchanged
        && !args.no_assume_unchanged
        && !args.skip_worktree
        && !args.no_skip_worktree
        && !args.add
        && !args.remove
        && !args.force_remove
        && !args.info_only
        && !args.unresolve
        && !args.index_info
        && args.index_version.is_none()
        && !args.replace
        && !args.split_index
        && !args.no_split_index
        && args.chmod.is_empty();

    if refresh_had_metadata_changes || !only_refresh_like {
        index.write(&index_path).context("writing index")?;
    }
    if refresh_had_issues && !args.quiet {
        std::process::exit(1);
    }
    Ok(())
}

/// Process `--index-info` stdin.
///
/// Accepted record formats:
/// - LF-terminated records (default)
/// - NUL-terminated records when `--stdin -z` is used
///
/// Record syntax:
/// - `<mode> <oid>\t<path>`
/// - `<mode> <oid> <stage>\t<path>`
/// - `<mode> <type> <oid>\t<path>` (legacy extended form)
fn run_index_info(
    index: &mut Index,
    index_path: &std::path::Path,
    _odb: &Odb,
    null_terminated: bool,
) -> Result<()> {
    if null_terminated {
        use std::io::Read;
        let mut buf = Vec::new();
        io::stdin().read_to_end(&mut buf)?;
        for rec in buf.split(|&b| b == 0).filter(|r| !r.is_empty()) {
            parse_index_info_record(index, rec)?;
        }
    } else {
        let stdin = io::stdin();
        for line in stdin.lock().lines() {
            let line = line?;
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            parse_index_info_record(index, line.as_bytes())?;
        }
    }

    index.write(index_path).context("writing index")?;
    Ok(())
}

fn parse_index_info_record(index: &mut Index, rec: &[u8]) -> Result<()> {
    // Format: "<mode> SP <oid> TAB <path>"
    // or: "<mode> SP <type> SP <oid> TAB <path>" (extended)
    let tab = rec
        .iter()
        .position(|b| *b == b'\t')
        .ok_or_else(|| anyhow::anyhow!("bad --index-info record: no tab"))?;
    let meta = std::str::from_utf8(&rec[..tab])
        .map_err(|_| anyhow::anyhow!("bad --index-info record: non-UTF-8 metadata"))?;
    let path = rec[tab + 1..].to_vec();

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
        _ => bail!("bad --index-info record"),
    };

    if mode_str == "0" {
        // Delete entry
        index.remove(&path);
        return Ok(());
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
    Ok(())
}

/// Re-stat tracked files, updating mtime/ctime/size for entries that are
/// content-identical to the index and reporting out-of-date paths.
fn refresh_index(
    index: &mut Index,
    work_tree: &std::path::Path,
    _odb: &Odb,
    conv: &crlf::ConversionConfig,
    attrs: &crlf::GitAttributes,
    config: &ConfigSet,
    ignore_missing: bool,
    allow_unmerged: bool,
    ignore_submodules: bool,
    quiet: bool,
) -> Result<(bool, bool)> {
    use std::os::unix::fs::MetadataExt;

    let mut problems = BTreeSet::new();
    let index_mtime = std::fs::metadata(work_tree.join(".git/index"))
        .ok()
        .map(|m| (m.mtime() as i64, m.mtime_nsec() as i64));
    let mut touched_entries = false;

    for entry in &mut index.entries {
        let path = std::str::from_utf8(&entry.path)
            .map_err(|_| anyhow::anyhow!("non-UTF-8 path in index"))?
            .to_string();
        let rel = std::path::Path::new(&path);

        // Stage entries are unmerged. Unless explicitly allowed, report them
        // as needing refresh.
        if entry.stage() != 0 {
            if !allow_unmerged {
                problems.insert(path);
            }
            continue;
        }

        let abs = work_tree.join(rel);

        // Special handling for gitlinks (submodules).
        if entry.mode == grit_lib::index::MODE_GITLINK {
            if ignore_submodules {
                continue;
            }
            let current = read_gitlink_head_oid(&abs);
            match current {
                Ok(oid) if oid == entry.oid => {}
                _ => {
                    problems.insert(path);
                }
            }
            continue;
        }

        let meta = match std::fs::symlink_metadata(&abs) {
            Ok(m) => m,
            Err(_) => {
                if !ignore_missing {
                    problems.insert(path);
                }
                continue;
            }
        };

        let mode = normalize_mode(meta.mode());
        let current_oid = if meta.file_type().is_symlink() {
            let target = std::fs::read_link(&abs)?;
            Odb::hash_object_data(
                grit_lib::objects::ObjectKind::Blob,
                target.to_string_lossy().as_bytes(),
            )
        } else if meta.file_type().is_file() {
            let raw = std::fs::read(&abs)?;
            let file_attrs = crlf::get_file_attrs(attrs, &path, config);
            let mut conv_for_hash = conv.clone();
            conv_for_hash.safecrlf = crlf::SafeCrlf::False;
            let filtered = crlf::convert_to_git(&raw, &path, &conv_for_hash, &file_attrs)
                .unwrap_or(raw);
            Odb::hash_object_data(grit_lib::objects::ObjectKind::Blob, &filtered)
        } else {
            if !ignore_missing {
                problems.insert(path);
            }
            continue;
        };

        if mode != entry.mode || current_oid != entry.oid {
            problems.insert(path);
            continue;
        }

        // For racily-clean entries (file mtime >= index mtime), force a
        // refresh rewrite so future checks are reliable.
        let is_racy = index_mtime
            .map(|(idx_sec, idx_nsec)| {
                let file_sec = meta.mtime() as i64;
                let file_nsec = meta.mtime_nsec() as i64;
                file_sec > idx_sec || (file_sec == idx_sec && file_nsec >= idx_nsec)
            })
            .unwrap_or(false);
        if !is_racy {
            continue;
        }

        entry.ctime_sec = meta.ctime() as u32;
        entry.ctime_nsec = meta.ctime_nsec() as u32;
        entry.mtime_sec = meta.mtime() as u32;
        entry.mtime_nsec = meta.mtime_nsec() as u32;
        entry.size = meta.size() as u32;
        touched_entries = true;
    }

    if !quiet {
        for path in &problems {
            println!("{path}");
        }
    }
    Ok((!problems.is_empty(), touched_entries))
}

fn read_gitlink_head_oid(submodule_path: &Path) -> Result<ObjectId> {
    let dot_git = submodule_path.join(".git");
    let git_dir = resolve_gitdir(&dot_git)?;
    let head_path = git_dir.join("HEAD");
    let head_content = std::fs::read_to_string(&head_path)?;
    let head = head_content.trim();
    if let Some(reference) = head.strip_prefix("ref: ") {
        let ref_path = git_dir.join(reference);
        let resolved = std::fs::read_to_string(ref_path)?;
        return resolved
            .trim()
            .parse()
            .with_context(|| "invalid submodule HEAD oid");
    }
    head.parse().with_context(|| "invalid submodule HEAD oid")
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

fn is_unwritable_odb_error(err: &grit_lib::error::Error) -> bool {
    matches!(
        err,
        grit_lib::error::Error::Io(io_err) if io_err.kind() == std::io::ErrorKind::PermissionDenied
    )
}
