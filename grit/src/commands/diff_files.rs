//! `grit diff-files` command.
//!
//! Compares the working tree against the index.  This is the plumbing
//! equivalent of `grit diff` (without `--cached`).

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::diff::{
    count_changes, detect_copies, format_stat_line, stat_matches, unified_diff, zero_oid,
    DiffEntry, DiffStatus,
};
use grit_lib::index::{
    Index, IndexEntry, MODE_EXECUTABLE, MODE_GITLINK, MODE_REGULAR, MODE_SYMLINK,
};
use grit_lib::objects::{ObjectId, ObjectKind};
use grit_lib::odb::Odb;
use grit_lib::repo::Repository;
use grit_lib::rev_parse::abbreviate_object_id;
#[cfg(unix)]
use libc;
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
    let options = parse_options(&args.args)?;
    let repo = Repository::discover(None).context("not a git repository")?;

    let Some(work_tree) = repo.work_tree.clone() else {
        bail!("this operation must be run in a work tree");
    };

    let index_path = effective_index_path(&repo)?;
    let index = repo.load_index_at(&index_path).context("loading index")?;

    let changes = collect_changes(&repo, &index, &work_tree, &options)?;

    let mut diff_entries: Vec<DiffEntry> = changes.iter().map(change_to_diff_entry).collect();

    if options.reverse {
        diff_entries = diff_entries
            .into_iter()
            .map(reverse_diff_entry_for_diff_files)
            .collect();
    }

    if options.find_copies {
        let threshold = options.find_renames.unwrap_or(50);
        let source_index_entries: Vec<(String, String, ObjectId)> = index
            .entries
            .iter()
            .filter(|e| e.stage() == 0)
            .filter_map(|e| {
                let path = String::from_utf8(e.path.clone()).ok()?;
                if options.ignore_submodules && e.mode == MODE_GITLINK {
                    return None;
                }
                if !matches_pathspec(&path, &options.pathspecs) {
                    return None;
                }
                let mode = format!("{:06o}", canonicalize_mode(e.mode));
                Some((path, mode, e.oid))
            })
            .collect();
        diff_entries = detect_copies(
            &repo.odb,
            diff_entries,
            threshold,
            options.find_copies_harder,
            &source_index_entries,
        );
    } else if let Some(threshold) = options.find_renames {
        diff_entries = grit_lib::diff::detect_renames(&repo.odb, diff_entries, threshold);
    }

    if !options.quiet && !options.suppress_diff {
        match options.format {
            OutputFormat::Raw => {
                for entry in &diff_entries {
                    println!(
                        "{}",
                        render_raw_diff_entry(entry, &repo, options.abbrev, options.reverse)?
                    );
                }
            }
            OutputFormat::NameOnly => {
                for entry in &diff_entries {
                    println!("{}", entry.path());
                }
            }
            OutputFormat::NameStatus => {
                for entry in &diff_entries {
                    match (entry.status, entry.score) {
                        (DiffStatus::Renamed, Some(s)) => {
                            println!(
                                "R{s:03}\t{}\t{}",
                                entry.old_path.as_deref().unwrap_or(""),
                                entry.new_path.as_deref().unwrap_or("")
                            );
                        }
                        (DiffStatus::Copied, Some(s)) => {
                            println!(
                                "C{s:03}\t{}\t{}",
                                entry.old_path.as_deref().unwrap_or(""),
                                entry.new_path.as_deref().unwrap_or("")
                            );
                        }
                        _ => {
                            println!("{}\t{}", entry.status.letter(), entry.path());
                        }
                    }
                }
            }
            OutputFormat::Patch => {
                for entry in &diff_entries {
                    print_patch_from_diff_entry(entry, &repo, &work_tree)?;
                }
            }
            OutputFormat::Stat => {
                print_stat_from_diff_entries(&diff_entries, &repo, &work_tree)?;
            }
            OutputFormat::NumStat => {
                print_numstat_from_diff_entries(&diff_entries, &repo, &work_tree)?;
            }
        }
    }

    if (options.exit_code || options.quiet) && !diff_entries.is_empty() {
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
    /// Optional diff-filter specification.
    diff_filter: Option<String>,
    /// Omit submodule entries (gitlinks) from the diff.
    ignore_submodules: bool,
    /// Rename similarity threshold (percent); `None` disables rename detection.
    find_renames: Option<u32>,
    /// Enable copy detection (`-C` / `--find-copies`).
    find_copies: bool,
    /// Consider unmodified index entries as copy sources (`--find-copies-harder`).
    find_copies_harder: bool,
    /// Swap old/new sides (reverse diff).
    reverse: bool,
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
    /// Working-tree blob OID (hashed content); zero when unknown or deleted from worktree.
    new_oid: ObjectId,
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
    let mut diff_filter: Option<String> = None;
    let mut ignore_submodules = false;
    let mut find_renames: Option<u32> = None;
    let mut find_copies = false;
    let mut find_copies_harder = false;
    let mut c_count = 0u32;
    let mut reverse = false;
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
                "-R" => reverse = true,
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
                | "--full-index"
                | "--no-ext-diff"
                | "--no-prefix"
                | "--no-abbrev" => {}
                "-M" | "--find-renames" => {
                    find_renames = Some(50);
                }
                "--no-renames" => {
                    find_renames = None;
                }
                _ if arg.starts_with("-M") && arg.len() > 2 => {
                    let val = &arg[2..];
                    let pct = if val.ends_with('%') {
                        val[..val.len() - 1].parse::<u32>().unwrap_or(50)
                    } else {
                        val.parse::<u32>().unwrap_or(50)
                    };
                    find_renames = Some(pct);
                }
                _ if arg.starts_with("--find-renames=") => {
                    let val = &arg["--find-renames=".len()..];
                    let pct = if val.ends_with('%') {
                        val[..val.len() - 1].parse::<u32>().unwrap_or(50)
                    } else {
                        val.parse::<u32>().unwrap_or(50)
                    };
                    find_renames = Some(pct);
                }
                "-C" | "--find-copies" => {
                    c_count += 1;
                    find_copies = true;
                    if c_count >= 2 {
                        find_copies_harder = true;
                    }
                    if find_renames.is_none() {
                        find_renames = Some(50);
                    }
                }
                "--find-copies-harder" => {
                    find_copies = true;
                    find_copies_harder = true;
                    if find_renames.is_none() {
                        find_renames = Some(50);
                    }
                }
                "--diff-filter" => {
                    if idx + 1 < argv.len() {
                        diff_filter = Some(argv[idx + 1].clone());
                        idx += 1;
                    }
                }
                _ if arg.starts_with("--diff-filter=") => {
                    diff_filter = Some(arg.trim_start_matches("--diff-filter=").to_string());
                }
                "--ignore-submodules" => {
                    ignore_submodules = true;
                    if idx + 1 < argv.len() {
                        let n = argv[idx + 1].as_str();
                        if matches!(n, "all" | "dirty" | "untracked" | "none") {
                            idx += 1;
                        }
                    }
                }
                // Global flags passed through that we accept but ignore
                "--literal-pathspecs"
                | "--glob-pathspecs"
                | "--noglob-pathspecs"
                | "--icase-pathspecs" => {}
                _ if arg.starts_with("-G")
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
        diff_filter,
        ignore_submodules,
        find_renames,
        find_copies,
        find_copies_harder,
        reverse,
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
            if options.ignore_submodules && entry.mode == MODE_GITLINK {
                continue;
            }
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
            match read_worktree_info_fast(repo, &abs, idx_entry)? {
                WorktreeStatus::Unchanged => { /* skip — stat says identical */ }
                WorktreeStatus::Modified(wt_mode, wt_oid) => {
                    let idx_canonical = canonicalize_mode(*idx_mode);
                    if wt_oid != *idx_oid || wt_mode != idx_canonical || is_stat_smudged(idx_entry)
                    {
                        // Detect type changes (e.g., symlink ↔ regular, regular ↔ submodule)
                        let status = if mode_type(idx_canonical) != mode_type(wt_mode) {
                            'T'
                        } else {
                            'M'
                        };
                        changes.insert(
                            path.clone(),
                            Change {
                                path: path.clone(),
                                status,
                                old_mode: idx_canonical,
                                new_mode: wt_mode,
                                old_oid: *idx_oid,
                                new_oid: wt_oid,
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
                            new_oid: zero_oid(),
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
                    new_oid: zero_oid(),
                },
            );
        }
    } else {
        // Stage-specific mode: compare requested stage entries against worktree.
        for (path, (idx_mode, idx_oid)) in &staged {
            let abs = work_tree.join(path);
            match read_worktree_info(repo, &abs)? {
                Some((wt_mode, wt_oid)) => {
                    changes.insert(
                        path.clone(),
                        Change {
                            path: path.clone(),
                            status: 'M',
                            old_mode: canonicalize_mode(*idx_mode),
                            new_mode: wt_mode,
                            old_oid: *idx_oid,
                            new_oid: wt_oid,
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
                            new_oid: zero_oid(),
                        },
                    );
                }
            }
        }
    }

    let mut out: Vec<Change> = changes.into_values().collect();
    if let Some(spec) = options.diff_filter.as_deref() {
        out.retain(|change| matches_diff_filter(change.status, spec));
    }
    Ok(out)
}

fn change_to_diff_entry(c: &Change) -> DiffEntry {
    let old_mode_str = format!("{:06o}", c.old_mode);
    let new_mode_str = format!("{:06o}", c.new_mode);
    match c.status {
        'D' => DiffEntry {
            status: DiffStatus::Deleted,
            old_path: Some(c.path.clone()),
            new_path: None,
            old_mode: old_mode_str,
            new_mode: new_mode_str,
            old_oid: c.old_oid,
            new_oid: zero_oid(),
            score: None,
        },
        'U' => DiffEntry {
            status: DiffStatus::Unmerged,
            old_path: Some(c.path.clone()),
            new_path: Some(c.path.clone()),
            old_mode: old_mode_str,
            new_mode: new_mode_str,
            old_oid: c.old_oid,
            new_oid: c.new_oid,
            score: None,
        },
        'T' => DiffEntry {
            status: DiffStatus::TypeChanged,
            old_path: Some(c.path.clone()),
            new_path: Some(c.path.clone()),
            old_mode: old_mode_str,
            new_mode: new_mode_str,
            old_oid: c.old_oid,
            new_oid: c.new_oid,
            score: None,
        },
        _ => DiffEntry {
            status: DiffStatus::Modified,
            old_path: Some(c.path.clone()),
            new_path: Some(c.path.clone()),
            old_mode: old_mode_str,
            new_mode: new_mode_str,
            old_oid: c.old_oid,
            new_oid: c.new_oid,
            score: None,
        },
    }
}

/// Swap old/new sides for `diff-files -R` before rename/copy detection.
fn reverse_diff_entry_for_diff_files(mut e: DiffEntry) -> DiffEntry {
    match e.status {
        DiffStatus::Added => {
            e.status = DiffStatus::Deleted;
            e.old_path = e.new_path.take();
            e.new_path = None;
            std::mem::swap(&mut e.old_mode, &mut e.new_mode);
            std::mem::swap(&mut e.old_oid, &mut e.new_oid);
        }
        DiffStatus::Deleted => {
            e.status = DiffStatus::Added;
            e.new_path = e.old_path.take();
            e.old_path = None;
            std::mem::swap(&mut e.old_mode, &mut e.new_mode);
            std::mem::swap(&mut e.old_oid, &mut e.new_oid);
        }
        DiffStatus::Renamed | DiffStatus::Copied => {
            std::mem::swap(&mut e.old_path, &mut e.new_path);
            std::mem::swap(&mut e.old_mode, &mut e.new_mode);
            std::mem::swap(&mut e.old_oid, &mut e.new_oid);
        }
        DiffStatus::Modified | DiffStatus::TypeChanged | DiffStatus::Unmerged => {
            std::mem::swap(&mut e.old_mode, &mut e.new_mode);
            std::mem::swap(&mut e.old_oid, &mut e.new_oid);
        }
    }
    e
}

fn render_raw_diff_entry(
    entry: &DiffEntry,
    repo: &Repository,
    abbrev: Option<usize>,
    reverse: bool,
) -> Result<String> {
    let width = abbrev.unwrap_or(40).clamp(4, 40);
    let old_oid = format_oid_for_raw(entry.old_oid, repo, abbrev, width)?;
    let new_oid = if reverse {
        format_oid_for_raw(entry.new_oid, repo, abbrev, width)?
    } else {
        "0".repeat(width)
    };

    let status_str = match (entry.status, entry.score) {
        (DiffStatus::Renamed, Some(s)) => format!("R{s:03}"),
        (DiffStatus::Copied, Some(s)) => format!("C{s:03}"),
        _ => entry.status.letter().to_string(),
    };

    let path = match entry.status {
        DiffStatus::Renamed | DiffStatus::Copied => format!(
            "{}\t{}",
            entry.old_path.as_deref().unwrap_or(""),
            entry.new_path.as_deref().unwrap_or("")
        ),
        _ => entry.path().to_owned(),
    };

    Ok(format!(
        ":{} {} {} {} {}\t{}",
        entry.old_mode, entry.new_mode, old_oid, new_oid, status_str, path
    ))
}

fn format_oid_for_raw(
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

fn print_patch_from_diff_entry(
    entry: &DiffEntry,
    repo: &Repository,
    work_tree: &Path,
) -> Result<()> {
    let (old_content, new_content) = load_patch_contents_for_diff_entry(entry, repo, work_tree)?;
    let old_path = entry
        .old_path
        .as_deref()
        .unwrap_or(entry.new_path.as_deref().unwrap_or(""));
    let new_path = entry
        .new_path
        .as_deref()
        .unwrap_or(entry.old_path.as_deref().unwrap_or(""));

    let old_label = match entry.status {
        DiffStatus::Added => "/dev/null".to_owned(),
        _ => format!("a/{old_path}"),
    };
    let new_label = match entry.status {
        DiffStatus::Deleted => "/dev/null".to_owned(),
        _ => format!("b/{new_path}"),
    };

    let display_path = entry.path();
    let mut header = format!("diff --git a/{old_path} b/{new_path}");
    match entry.status {
        DiffStatus::Deleted => {
            header.push_str(&format!("\ndeleted file mode {}", entry.old_mode));
        }
        DiffStatus::Added => {
            header.push_str(&format!("\nnew file mode {}", entry.new_mode));
        }
        DiffStatus::Renamed => {
            let sim = entry.score.unwrap_or(100);
            header.push_str(&format!(
                "\nsimilarity index {sim}%\nrename from {old_path}\nrename to {new_path}"
            ));
        }
        DiffStatus::Copied => {
            let sim = entry.score.unwrap_or(100);
            header.push_str(&format!(
                "\nsimilarity index {sim}%\ncopy from {old_path}\ncopy to {new_path}"
            ));
        }
        _ => {
            if entry.old_mode != entry.new_mode {
                header.push_str(&format!(
                    "\nold mode {}\nnew mode {}",
                    entry.old_mode, entry.new_mode
                ));
            }
        }
    }

    if (entry.status == DiffStatus::Renamed || entry.status == DiffStatus::Copied)
        && entry.old_oid == entry.new_oid
    {
        println!("{header}");
        return Ok(());
    }

    if old_content == new_content
        && entry.old_mode != entry.new_mode
        && entry.status != DiffStatus::Renamed
        && entry.status != DiffStatus::Copied
    {
        println!("{header}");
    } else if old_content != new_content {
        let patch = unified_diff(&old_content, &new_content, display_path, display_path, 3);
        let body: String = patch.lines().skip(2).map(|l| format!("\n{l}")).collect();
        println!("{header}\n--- {old_label}\n+++ {new_label}{body}");
    } else {
        println!("{header}\n--- {old_label}\n+++ {new_label}");
    }
    Ok(())
}

fn print_stat_from_diff_entries(
    entries: &[DiffEntry],
    repo: &Repository,
    work_tree: &Path,
) -> Result<()> {
    if entries.is_empty() {
        return Ok(());
    }
    let max_len = entries.iter().map(|e| e.path().len()).max().unwrap_or(0);
    let mut total_ins = 0usize;
    let mut total_del = 0usize;
    for entry in entries {
        let (old, new) = load_patch_contents_for_diff_entry(entry, repo, work_tree)?;
        let (ins, del) = count_changes(&old, &new);
        total_ins += ins;
        total_del += del;
        println!("{}", format_stat_line(entry.path(), ins, del, max_len));
    }
    let files = entries.len();
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

fn print_numstat_from_diff_entries(
    entries: &[DiffEntry],
    repo: &Repository,
    work_tree: &Path,
) -> Result<()> {
    for entry in entries {
        let (old, new) = load_patch_contents_for_diff_entry(entry, repo, work_tree)?;
        let (ins, del) = count_changes(&old, &new);
        println!("{}\t{}\t{}", ins, del, entry.path());
    }
    Ok(())
}

fn load_patch_contents_for_diff_entry(
    entry: &DiffEntry,
    repo: &Repository,
    work_tree: &Path,
) -> Result<(String, String)> {
    let old_content = if entry.status == DiffStatus::Added || entry.old_oid == zero_oid() {
        String::new()
    } else {
        let obj = repo.odb.read(&entry.old_oid)?;
        String::from_utf8(obj.data).unwrap_or_default()
    };

    let new_content = if entry.status == DiffStatus::Deleted {
        String::new()
    } else {
        let path = entry.new_path.as_deref().unwrap_or(entry.path());
        let abs = work_tree.join(path);
        match fs::read(&abs) {
            Ok(bytes) => String::from_utf8(bytes).unwrap_or_default(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(e.into()),
        }
    };

    Ok((old_content, new_content))
}

fn matches_diff_filter(status: char, spec: &str) -> bool {
    if spec.is_empty() {
        return true;
    }
    let status = status.to_ascii_uppercase();
    let mut includes: Vec<char> = Vec::new();
    let mut excludes: Vec<char> = Vec::new();
    for c in spec.chars() {
        if c == '*' {
            continue;
        }
        if c.is_ascii_uppercase() {
            includes.push(c);
        } else if c.is_ascii_lowercase() {
            excludes.push(c.to_ascii_uppercase());
        }
    }
    if !includes.is_empty() && !includes.contains(&status) {
        return false;
    }
    if excludes.contains(&status) {
        return false;
    }
    true
}

/// `read-tree`-style entries carry zeroed stat data and are considered dirty
/// until an explicit refresh (e.g. `checkout-index -u` / `update-index --refresh`).
fn is_stat_smudged(entry: &IndexEntry) -> bool {
    entry.ctime_sec == 0
        && entry.ctime_nsec == 0
        && entry.mtime_sec == 0
        && entry.mtime_nsec == 0
        && entry.dev == 0
        && entry.ino == 0
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
fn path_component_is_not_directory(err: &std::io::Error) -> bool {
    if err.kind() == std::io::ErrorKind::NotADirectory {
        return true;
    }
    #[cfg(unix)]
    {
        if err.raw_os_error() == Some(libc::ENOTDIR) {
            return true;
        }
    }
    false
}

fn read_worktree_info_fast(
    repo: &Repository,
    abs_path: &Path,
    index_entry: &IndexEntry,
) -> Result<WorktreeStatus> {
    let meta = match fs::symlink_metadata(abs_path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if canonicalize_mode(index_entry.mode) == MODE_GITLINK {
                return Ok(WorktreeStatus::Unchanged);
            }
            return Ok(WorktreeStatus::Missing);
        }
        Err(e) if path_component_is_not_directory(&e) => {
            if canonicalize_mode(index_entry.mode) == MODE_GITLINK {
                return Ok(WorktreeStatus::Unchanged);
            }
            return Ok(WorktreeStatus::Missing);
        }
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
        let data = fs::read(abs_path)?;
        let oid = Odb::hash_object_data(ObjectKind::Blob, &data);
        return Ok(WorktreeStatus::Modified(mode, oid));
    }

    // If it's a directory, check if it's a submodule (has .git subdirectory)
    if meta.file_type().is_dir() {
        let dot_git = abs_path.join(".git");
        if dot_git.exists() {
            // Treat as a submodule (mode 160000)
            let sub_oid = read_submodule_head(abs_path).unwrap_or(index_entry.oid);
            return Ok(WorktreeStatus::Modified(0o160000, sub_oid));
        }
        // Superproject gitlink with an empty placeholder directory (no embedded
        // repo yet) matches the index — same as Git after merge/checkout.
        if canonicalize_mode(index_entry.mode) == MODE_GITLINK {
            return Ok(WorktreeStatus::Unchanged);
        }
    }

    Ok(WorktreeStatus::Missing)
}

/// Read the current HEAD commit OID of a submodule at the given path.
fn read_submodule_head(path: &Path) -> Result<ObjectId> {
    let head_path = path.join(".git").join("HEAD");
    let content = std::fs::read_to_string(&head_path)?;
    let content = content.trim();
    if let Some(refname) = content.strip_prefix("ref: ") {
        let ref_file = path.join(".git").join(refname);
        let ref_content = std::fs::read_to_string(&ref_file)?;
        Ok(ref_content.trim().parse()?)
    } else {
        Ok(content.parse()?)
    }
}

/// Read mode and OID for a working-tree file; returns `None` if missing.
///
/// The OID is computed by hashing the file content so we can detect
/// modifications.  The mode is canonicalized to one of the four Git modes.
fn read_worktree_info(repo: &Repository, abs_path: &Path) -> Result<Option<(u32, ObjectId)>> {
    let meta = match fs::symlink_metadata(abs_path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) if path_component_is_not_directory(&e) => return Ok(None),
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
        let data = fs::read(abs_path)?;
        let oid = Odb::hash_object_data(ObjectKind::Blob, &data);
        return Ok(Some((mode, oid)));
    }

    Ok(None)
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
        if let Some(prefix) = spec.strip_suffix('/') {
            path == prefix || path.starts_with(&format!("{prefix}/"))
        } else {
            path == spec || path.starts_with(&format!("{spec}/"))
        }
    })
}

/// Return the file type category for a mode: 0=regular, 1=executable, 2=symlink, 3=submodule, 4=other
fn mode_type(mode: u32) -> u32 {
    match mode {
        0o100644 => 0,
        0o100755 => 1,
        0o120000 => 2,
        0o160000 => 3,
        _ => 4,
    }
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
