//! `grit diff-files` command.
//!
//! Compares the working tree against the index.  This is the plumbing
//! equivalent of `grit diff` (without `--cached`).

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::config::ConfigSet;
use grit_lib::crlf::{self, ConversionConfig, GitAttributes};
use grit_lib::diff::{count_changes, format_stat_line, stat_matches, unified_diff, zero_oid};
use grit_lib::index::{
    Index, IndexEntry, MODE_EXECUTABLE, MODE_GITLINK, MODE_REGULAR, MODE_SYMLINK,
};
use grit_lib::objects::{ObjectId, ObjectKind};
use grit_lib::odb::Odb;
use grit_lib::repo::Repository;
use grit_lib::rev_parse::abbreviate_object_id;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

// ── Public clap interface ────────────────────────────────────────────

/// Arguments for `grit diff-files`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Raw command arguments forwarded by the CLI parser.
    #[arg(value_name = "ARG", num_args = 0.., allow_hyphen_values = true, trailing_var_arg = true)]
    pub args: Vec<String>,
}

/// Run `grit diff-files`.
pub fn run(args: Args) -> Result<()> {
    let mut options = parse_options(&args.args)?;
    let repo = Repository::discover(None).context("not a git repository")?;

    let Some(work_tree) = repo.work_tree.clone() else {
        bail!("this operation must be run in a work tree");
    };
    options.pathspecs = normalize_pathspecs(&options.pathspecs, &work_tree)?;

    let index_path = effective_index_path(&repo)?;
    let index = Index::load(&index_path).context("loading index")?;

    let changes = collect_changes(&repo, &index, &work_tree, &options)?;

    if !options.quiet && !options.suppress_diff {
        match options.format {
            OutputFormat::Raw => {
                for change in &changes {
                    println!("{}", render_raw(change, &repo, options.abbrev)?);
                }
            }
            OutputFormat::NameOnly => {
                for change in &changes {
                    println!("{}", change.path);
                }
            }
            OutputFormat::NameStatus => {
                for change in &changes {
                    println!("{}\t{}", change.status, change.path);
                }
            }
            OutputFormat::Patch => {
                for change in &changes {
                    print_patch(change, &repo, &work_tree)?;
                }
            }
            OutputFormat::Stat => {
                print_stat(&changes, &repo, &work_tree)?;
            }
            OutputFormat::NumStat => {
                print_numstat(&changes, &repo, &work_tree)?;
            }
        }
    }

    if (options.exit_code || options.quiet) && !changes.is_empty() {
        std::process::exit(1);
    }
    Ok(())
}

// ── Internal types ───────────────────────────────────────────────────

/// Output format for `diff-files`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputFormat {
    /// `:old-mode new-mode old-oid new-oid status\tpath` (default).
    Raw,
    /// Unified patch output.
    Patch,
    /// Diff stat summary.
    Stat,
    /// Numeric stat (NUL-line-terminated counts).
    NumStat,
    /// File names only.
    NameOnly,
    /// Status letter + tab + file name.
    NameStatus,
}

/// Parsed command-line options.
#[derive(Debug, Clone)]
struct Options {
    /// Paths to limit output to; empty means all paths.
    pathspecs: Vec<String>,
    /// Merge stage to diff against (0 = normal, 1–3 = unmerged stage).
    stage: u8,
    /// Suppress all output; exit 1 if any difference.
    quiet: bool,
    /// Exit 1 if differences, regardless of output format.
    exit_code: bool,
    /// Abbreviate OIDs to this many hex digits.
    abbrev: Option<usize>,
    /// Chosen output format.
    format: OutputFormat,
    /// Suppress diff output (-s / --no-patch).
    suppress_diff: bool,
}

/// A single changed file: index side vs working tree.
#[derive(Debug, Clone)]
struct Change {
    /// Relative path.
    path: String,
    /// Single-letter status code (`M`, `D`, `A`, `U`).
    status: char,
    /// Index-side mode (octal).
    old_mode: u32,
    /// Working-tree-side mode (octal), or 0 for deleted.
    new_mode: u32,
    /// Index-side OID.
    old_oid: ObjectId,
}

// ── Option parsing ───────────────────────────────────────────────────

fn parse_options(argv: &[String]) -> Result<Options> {
    let mut pathspecs = Vec::new();
    let mut stage: u8 = 0;
    let mut quiet = false;
    let mut exit_code = false;
    let mut abbrev: Option<usize> = None;
    let mut format = OutputFormat::Raw;
    let mut suppress_diff = false;
    let mut end_of_options = false;

    let mut idx = 0usize;
    while idx < argv.len() {
        let arg = &argv[idx];
        if !end_of_options && arg == "--" {
            end_of_options = true;
            idx += 1;
            continue;
        }
        if !end_of_options && arg.starts_with('-') {
            match arg.as_str() {
                "--raw" => {
                    format = OutputFormat::Raw;
                    suppress_diff = false;
                }
                "-p" | "--patch" | "-u" => {
                    format = OutputFormat::Patch;
                    suppress_diff = false;
                }
                "--stat" => {
                    format = OutputFormat::Stat;
                    suppress_diff = false;
                }
                "--numstat" => {
                    format = OutputFormat::NumStat;
                    suppress_diff = false;
                }
                "--name-only" => {
                    format = OutputFormat::NameOnly;
                    suppress_diff = false;
                }
                "--name-status" => {
                    format = OutputFormat::NameStatus;
                    suppress_diff = false;
                }
                "--exit-code" => exit_code = true,
                "-q" | "--quiet" => quiet = true,
                "-s" | "--no-patch" => suppress_diff = true,
                "--patch-with-raw" => {
                    format = OutputFormat::Patch;
                    suppress_diff = false;
                } // TODO: also show raw
                "--patch-with-stat" => {
                    format = OutputFormat::Patch;
                    suppress_diff = false;
                } // TODO: also show stat
                "-0" => stage = 0,
                "-1" => stage = 1,
                "-2" => stage = 2,
                "-3" => stage = 3,
                "--abbrev" => abbrev = Some(7),
                _ if arg.starts_with("--abbrev=") => {
                    let value = arg.trim_start_matches("--abbrev=");
                    let parsed = value
                        .parse::<usize>()
                        .with_context(|| format!("invalid --abbrev value: `{value}`"))?;
                    abbrev = Some(parsed);
                }
                // Silently accept diff options we don't fully implement yet
                "-w"
                | "--ignore-all-space"
                | "-b"
                | "--ignore-space-change"
                | "--ignore-space-at-eol"
                | "--ignore-blank-lines"
                | "--diff-filter"
                | "--full-index"
                | "--no-ext-diff"
                | "--no-prefix"
                | "--no-renames"
                | "--no-abbrev" => {}
                _ if arg.starts_with("--diff-filter=")
                    || arg.starts_with("-G")
                    || arg.starts_with("-S")
                    || arg.starts_with("-O")
                    || arg.starts_with("--src-prefix=")
                    || arg.starts_with("--dst-prefix=") => {}
                _ => bail!("unsupported option: {arg}"),
            }
            idx += 1;
            continue;
        }
        pathspecs.push(arg.clone());
        idx += 1;
    }

    Ok(Options {
        pathspecs,
        stage,
        quiet,
        exit_code,
        abbrev,
        format,
        suppress_diff,
    })
}

// ── Core diff logic ──────────────────────────────────────────────────

/// Build the list of changes between the index and the working tree.
fn collect_changes(
    repo: &Repository,
    index: &Index,
    work_tree: &Path,
    options: &Options,
) -> Result<Vec<Change>> {
    let config = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_else(|_| ConfigSet::new());
    let conv = ConversionConfig::from_config(&config);
    let attrs = crlf::load_gitattributes(work_tree);

    // Collect index entries, grouped by path.  For stage==0 we use merged
    // entries (stage 0).  For stage 1–3 we use that specific unmerged stage.
    // Paths that only have higher-stage entries and no stage-0 entry are
    // "unmerged"; we report them as 'U' when stage==0.
    let mut stage0: BTreeMap<String, (u32, ObjectId, &IndexEntry)> = BTreeMap::new();
    let mut unmerged_paths: BTreeSet<String> = BTreeSet::new();
    let mut staged: BTreeMap<String, (u32, ObjectId)> = BTreeMap::new();

    for entry in &index.entries {
        let Ok(path) = String::from_utf8(entry.path.clone()) else {
            continue;
        };
        if !matches_pathspec(&path, &options.pathspecs) {
            continue;
        }
        let s = entry.stage();
        if s == 0 {
            stage0.insert(path, (entry.mode, entry.oid, entry));
        } else {
            unmerged_paths.insert(path.clone());
            if s == options.stage {
                staged.insert(path, (entry.mode, entry.oid));
            }
        }
    }

    let mut changes: BTreeMap<String, Change> = BTreeMap::new();

    if options.stage == 0 {
        // Normal mode: compare stage-0 entries against worktree.
        // Use stat info to skip unchanged files (avoid hashing).
        for (path, (idx_mode, idx_oid, idx_entry)) in &stage0 {
            let abs = work_tree.join(path);
            match read_worktree_info_fast(repo, &abs, idx_entry, path, &conv, &attrs, &config)? {
                WorktreeStatus::Unchanged => { /* skip — stat says identical */ }
                WorktreeStatus::Modified(wt_mode, wt_oid) => {
                    let idx_canonical = canonicalize_mode(*idx_mode);
                    if wt_oid != *idx_oid || wt_mode != idx_canonical {
                        changes.insert(
                            path.clone(),
                            Change {
                                path: path.clone(),
                                status: 'M',
                                old_mode: idx_canonical,
                                new_mode: wt_mode,
                                old_oid: *idx_oid,
                            },
                        );
                    }
                }
                WorktreeStatus::Missing => {
                    // File missing from working tree.
                    changes.insert(
                        path.clone(),
                        Change {
                            path: path.clone(),
                            status: 'D',
                            old_mode: canonicalize_mode(*idx_mode),
                            new_mode: 0,
                            old_oid: *idx_oid,
                        },
                    );
                }
            }
        }

        // Unmerged paths (no stage-0 entry).
        for path in &unmerged_paths {
            if stage0.contains_key(path) {
                continue;
            }
            if !matches_pathspec(path, &options.pathspecs) {
                continue;
            }
            changes.insert(
                path.clone(),
                Change {
                    path: path.clone(),
                    status: 'U',
                    old_mode: 0,
                    new_mode: 0,
                    old_oid: zero_oid(),
                },
            );
        }
    } else {
        // Stage-specific mode: compare requested stage entries against worktree.
        for (path, (idx_mode, idx_oid)) in &staged {
            let abs = work_tree.join(path);
            match read_worktree_info(repo, &abs, path, Some(*idx_oid), &conv, &attrs, &config)? {
                Some((wt_mode, _wt_oid)) => {
                    changes.insert(
                        path.clone(),
                        Change {
                            path: path.clone(),
                            status: 'M',
                            old_mode: canonicalize_mode(*idx_mode),
                            new_mode: wt_mode,
                            old_oid: *idx_oid,
                        },
                    );
                }
                None => {
                    changes.insert(
                        path.clone(),
                        Change {
                            path: path.clone(),
                            status: 'D',
                            old_mode: canonicalize_mode(*idx_mode),
                            new_mode: 0,
                            old_oid: *idx_oid,
                        },
                    );
                }
            }
        }
    }

    Ok(changes.into_values().collect())
}

// ── Worktree probing ─────────────────────────────────────────────────

/// Result of probing a working-tree file against its index entry.
enum WorktreeStatus {
    /// File is unchanged according to stat info — no need to hash.
    Unchanged,
    /// File exists and may be modified (mode, oid from full hash).
    Modified(u32, ObjectId),
    /// File is missing from the working tree.
    Missing,
}

/// Fast worktree probe: uses stat() data from the index to skip hashing
/// when the file hasn't changed.  Falls back to full read+hash if stat
/// info doesn't match.
fn read_worktree_info_fast(
    repo: &Repository,
    abs_path: &Path,
    index_entry: &IndexEntry,
    rel_path: &str,
    conv: &ConversionConfig,
    attrs: &GitAttributes,
    config: &ConfigSet,
) -> Result<WorktreeStatus> {
    let meta = match fs::symlink_metadata(abs_path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(WorktreeStatus::Missing),
        Err(e) => return Err(e.into()),
    };

    let _ = repo;

    // Fast path: if stat info matches the index, file is unchanged.
    // But also check if the index mode differs from the worktree mode
    // (e.g., after git update-index --chmod=+x).
    if meta.file_type().is_file() && stat_matches(index_entry, &meta) {
        let wt_mode = if meta.permissions().mode() & 0o111 != 0 {
            MODE_EXECUTABLE
        } else {
            MODE_REGULAR
        };
        let idx_mode = canonicalize_mode(index_entry.mode);
        if wt_mode == idx_mode {
            return Ok(WorktreeStatus::Unchanged);
        }
        // Mode differs — report as modified with same OID.
        return Ok(WorktreeStatus::Modified(wt_mode, index_entry.oid));
    }

    if meta.file_type().is_symlink() {
        let target = fs::read_link(abs_path)?;
        let oid = Odb::hash_object_data(ObjectKind::Blob, target.as_os_str().as_bytes());
        return Ok(WorktreeStatus::Modified(MODE_SYMLINK, oid));
    }

    if meta.file_type().is_file() {
        let mode = if meta.permissions().mode() & 0o111 != 0 {
            MODE_EXECUTABLE
        } else {
            MODE_REGULAR
        };
        let raw = fs::read(abs_path)?;
        let raw_oid = Odb::hash_object_data(ObjectKind::Blob, &raw);
        let file_attrs = crlf::get_file_attrs(attrs, rel_path, config);
        let effective_attrs =
            if rel_path.ends_with("/.gitattributes") || rel_path == ".gitattributes" {
                let mut fa = file_attrs.clone();
                if fa.text == crlf::TextAttr::Set
                    && fa.eol == crlf::EolAttr::Crlf
                    && conv.autocrlf == crlf::AutoCrlf::True
                {
                    fa.text = crlf::TextAttr::Unspecified;
                    fa.eol = crlf::EolAttr::Unspecified;
                }
                fa
            } else {
                file_attrs
            };
        let normalized =
            crlf::convert_to_git(&raw, rel_path, conv, &effective_attrs).unwrap_or(raw);
        let normalized_oid = Odb::hash_object_data(ObjectKind::Blob, &normalized);
        let oid = if raw_oid == index_entry.oid {
            raw_oid
        } else {
            normalized_oid
        };
        return Ok(WorktreeStatus::Modified(mode, oid));
    }

    Ok(WorktreeStatus::Missing)
}

/// Read mode and OID for a working-tree file; returns `None` if missing.
///
/// The OID is computed by hashing the file content so we can detect
/// modifications.  The mode is canonicalized to one of the four Git modes.
fn read_worktree_info(
    repo: &Repository,
    abs_path: &Path,
    rel_path: &str,
    expected_oid: Option<ObjectId>,
    conv: &ConversionConfig,
    attrs: &GitAttributes,
    config: &ConfigSet,
) -> Result<Option<(u32, ObjectId)>> {
    let meta = match fs::symlink_metadata(abs_path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };

    let _ = repo;

    if meta.file_type().is_symlink() {
        let target = fs::read_link(abs_path)?;
        let oid = Odb::hash_object_data(ObjectKind::Blob, target.as_os_str().as_bytes());
        return Ok(Some((MODE_SYMLINK, oid)));
    }

    if meta.file_type().is_file() {
        let mode = if meta.permissions().mode() & 0o111 != 0 {
            MODE_EXECUTABLE
        } else {
            MODE_REGULAR
        };
        let raw = fs::read(abs_path)?;
        let raw_oid = Odb::hash_object_data(ObjectKind::Blob, &raw);
        let file_attrs = crlf::get_file_attrs(attrs, rel_path, config);
        let effective_attrs =
            if rel_path.ends_with("/.gitattributes") || rel_path == ".gitattributes" {
                let mut fa = file_attrs.clone();
                if fa.text == crlf::TextAttr::Set
                    && fa.eol == crlf::EolAttr::Crlf
                    && conv.autocrlf == crlf::AutoCrlf::True
                {
                    fa.text = crlf::TextAttr::Unspecified;
                    fa.eol = crlf::EolAttr::Unspecified;
                }
                fa
            } else {
                file_attrs
            };
        let normalized =
            crlf::convert_to_git(&raw, rel_path, conv, &effective_attrs).unwrap_or(raw);
        let normalized_oid = Odb::hash_object_data(ObjectKind::Blob, &normalized);
        let oid = if expected_oid.is_some_and(|oid| oid == raw_oid) {
            raw_oid
        } else {
            normalized_oid
        };
        return Ok(Some((mode, oid)));
    }

    Ok(None)
}

// ── Output renderers ─────────────────────────────────────────────────

/// Format a change in Git's raw diff format.
///
/// For `diff-files` the working-tree OID is always shown as zeros —
/// the worktree side has not been committed into the object store.
fn render_raw(change: &Change, repo: &Repository, abbrev: Option<usize>) -> Result<String> {
    let width = abbrev.unwrap_or(40).clamp(4, 40);
    let old_oid = format_oid(change.old_oid, repo, abbrev, width)?;
    // Working-tree OID is always zeros in diff-files output.
    let new_oid = "0".repeat(width);
    Ok(format!(
        ":{:06o} {:06o} {} {} {}\t{}",
        change.old_mode, change.new_mode, old_oid, new_oid, change.status, change.path
    ))
}

/// Print unified patch output for a single change.
fn print_patch(change: &Change, repo: &Repository, work_tree: &Path) -> Result<()> {
    let (old_content, new_content) = load_patch_contents(change, repo, work_tree)?;
    let path = &change.path;
    let old_label = if change.status == 'A' {
        "/dev/null".to_owned()
    } else {
        format!("a/{path}")
    };
    let new_label = if change.status == 'D' {
        "/dev/null".to_owned()
    } else {
        format!("b/{path}")
    };

    // Build mode header lines
    let mut header = format!("diff --git a/{path} b/{path}");
    if change.status == 'D' {
        header.push_str(&format!("\ndeleted file mode {:06o}", change.old_mode));
    } else if change.status == 'A' {
        header.push_str(&format!("\nnew file mode {:06o}", change.new_mode));
    } else if change.old_mode != change.new_mode {
        header.push_str(&format!(
            "\nold mode {:06o}\nnew mode {:06o}",
            change.old_mode, change.new_mode
        ));
    }

    if old_content == new_content && change.old_mode != change.new_mode {
        // Mode-only change, no content diff needed
        println!("{header}");
    } else if old_content != new_content {
        let patch = unified_diff(&old_content, &new_content, path, path, 3);
        // unified_diff already includes the --- / +++ lines; strip them.
        let body: String = patch.lines().skip(2).map(|l| format!("\n{l}")).collect();
        println!("{header}\n--- {old_label}\n+++ {new_label}{body}");
    } else {
        println!("{header}\n--- {old_label}\n+++ {new_label}");
    }
    Ok(())
}

/// Print `--stat` output for all changes.
fn print_stat(changes: &[Change], repo: &Repository, work_tree: &Path) -> Result<()> {
    if changes.is_empty() {
        return Ok(());
    }
    let max_len = changes.iter().map(|c| c.path.len()).max().unwrap_or(0);
    let mut total_ins = 0usize;
    let mut total_del = 0usize;
    for change in changes {
        let (old, new) = load_patch_contents(change, repo, work_tree)?;
        let (ins, del) = count_changes(&old, &new);
        total_ins += ins;
        total_del += del;
        println!("{}", format_stat_line(&change.path, ins, del, max_len));
    }
    let files = changes.len();
    let mut summary = format!(
        " {} file{} changed",
        files,
        if files == 1 { "" } else { "s" },
    );
    if total_ins > 0 || (total_ins == 0 && total_del == 0) {
        summary.push_str(&format!(
            ", {} insertion{}(+)",
            total_ins,
            if total_ins == 1 { "" } else { "s" }
        ));
    }
    if total_del > 0 || (total_ins == 0 && total_del == 0) {
        summary.push_str(&format!(
            ", {} deletion{}(-)",
            total_del,
            if total_del == 1 { "" } else { "s" }
        ));
    }
    println!("{summary}");
    Ok(())
}

/// Print `--numstat` output for all changes.
fn print_numstat(changes: &[Change], repo: &Repository, work_tree: &Path) -> Result<()> {
    for change in changes {
        let (old, new) = load_patch_contents(change, repo, work_tree)?;
        let (ins, del) = count_changes(&old, &new);
        println!("{}\t{}\t{}", ins, del, change.path);
    }
    Ok(())
}

/// Load old (index) and new (worktree) content for a change as UTF-8 strings.
///
/// Binary or unreadable content is returned as an empty string so that the
/// caller still produces correct insertion/deletion counts (zero vs zero).
fn load_patch_contents(
    change: &Change,
    repo: &Repository,
    work_tree: &Path,
) -> Result<(String, String)> {
    // Old side: read blob from object database.
    let old_content = if change.status == 'A' || change.old_oid == zero_oid() {
        String::new()
    } else {
        let obj = repo.odb.read(&change.old_oid)?;
        String::from_utf8(obj.data).unwrap_or_default()
    };

    // New side: read file from working tree.
    let new_content = if change.status == 'D' || change.new_mode == 0 {
        String::new()
    } else {
        let abs = work_tree.join(&change.path);
        match fs::read(&abs) {
            Ok(bytes) => String::from_utf8(bytes).unwrap_or_default(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(e.into()),
        }
    };

    Ok((old_content, new_content))
}

// ── Helpers ──────────────────────────────────────────────────────────

/// Format an OID, optionally abbreviated.
fn format_oid(
    oid: ObjectId,
    repo: &Repository,
    abbrev: Option<usize>,
    width: usize,
) -> Result<String> {
    if oid == zero_oid() {
        return Ok("0".repeat(width));
    }
    match abbrev {
        Some(min_len) => abbreviate_object_id(repo, oid, min_len).map_err(Into::into),
        None => Ok(oid.to_hex()),
    }
}

/// Canonicalize a raw file mode to one of the four Git modes.
fn canonicalize_mode(raw_mode: u32) -> u32 {
    match raw_mode & 0o170000 {
        0o120000 => MODE_SYMLINK,
        0o160000 => MODE_GITLINK,
        0o100000 => {
            if raw_mode & 0o111 != 0 {
                MODE_EXECUTABLE
            } else {
                MODE_REGULAR
            }
        }
        _ => MODE_REGULAR,
    }
}

/// Return true if `path` matches any of the given pathspecs.
///
/// An empty pathspec list matches everything.
fn matches_pathspec(path: &str, pathspecs: &[String]) -> bool {
    if pathspecs.is_empty() {
        return true;
    }
    pathspecs.iter().any(|spec| {
        if spec.is_empty() {
            return true;
        }
        crate::pathspec::pathspec_matches(spec, path)
    })
}

/// Resolve the index file path, honouring `GIT_INDEX_FILE`.
fn effective_index_path(repo: &Repository) -> Result<PathBuf> {
    if let Ok(raw) = std::env::var("GIT_INDEX_FILE") {
        let path = PathBuf::from(raw);
        if path.is_absolute() {
            return Ok(path);
        }
        let cwd = std::env::current_dir().context("resolving GIT_INDEX_FILE")?;
        return Ok(cwd.join(path));
    }
    Ok(repo.index_path())
}

/// Convert command-line pathspecs to worktree-relative patterns.
///
/// `git diff-files` interprets pathspecs relative to the current directory.
/// When invoked from a subdirectory, `.` therefore means "this subdirectory"
/// (not the repository root).
fn normalize_pathspecs(pathspecs: &[String], work_tree: &Path) -> Result<Vec<String>> {
    if pathspecs.is_empty() {
        return Ok(Vec::new());
    }

    let cwd = std::env::current_dir().context("resolving current directory")?;
    let cwd_prefix = cwd_prefix(work_tree, &cwd);
    let cwd_prefix_with_slash = cwd_prefix
        .as_ref()
        .map(|prefix| format!("{prefix}/"))
        .unwrap_or_default();

    let mut out = Vec::with_capacity(pathspecs.len());
    for spec in pathspecs {
        if spec == "." {
            out.push(cwd_prefix.clone().unwrap_or_default());
            continue;
        }

        let spec_path = Path::new(spec);
        if spec_path.is_absolute() {
            if let Ok(rel) = spec_path.strip_prefix(work_tree) {
                out.push(rel.to_string_lossy().into_owned());
            } else {
                out.push(spec.clone());
            }
            continue;
        }

        if cwd_prefix_with_slash.is_empty() {
            out.push(spec.clone());
        } else {
            out.push(format!("{cwd_prefix_with_slash}{spec}"));
        }
    }
    Ok(out)
}

/// Return the current directory prefix relative to the worktree, if any.
fn cwd_prefix(work_tree: &Path, cwd: &Path) -> Option<String> {
    let work_tree = work_tree
        .canonicalize()
        .unwrap_or_else(|_| work_tree.to_path_buf());
    let cwd = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    let rel = cwd.strip_prefix(&work_tree).ok()?;
    if rel.as_os_str().is_empty() {
        return None;
    }
    Some(rel.to_string_lossy().into_owned())
}
