//! `grit diff-index` command.
//!
//! Compare a tree (usually a commit's tree) against the index or the working tree.
//!
//! Output formats: raw (default), unified patch (`-p`), `--stat`, `--numstat`,
//! `--name-only`, `--name-status`.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::diff::{
    count_changes, diff_index_to_tree, diff_tree_to_worktree,
    format_raw, unified_diff, zero_oid, DiffEntry, DiffStatus,
};
use grit_lib::index::Index;
use grit_lib::objects::{parse_commit, ObjectId, ObjectKind};
use grit_lib::odb::Odb;
use grit_lib::repo::Repository;
use grit_lib::rev_parse::resolve_revision;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Arguments for `grit diff-index`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Raw command arguments forwarded by the CLI parser.
    #[arg(value_name = "ARG", num_args = 0.., allow_hyphen_values = true, trailing_var_arg = true)]
    pub args: Vec<String>,
}

/// Output format for diff-index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputFormat {
    Raw,
    Patch,
    Stat,
    NumStat,
    NameOnly,
    NameStatus,
}

#[derive(Debug, Clone)]
struct Options {
    tree_ish: String,
    pathspecs: Vec<String>,
    cached: bool,
    match_missing: bool,
    quiet: bool,
    exit_code: bool,
    abbrev: Option<usize>,
    format: OutputFormat,
    context_lines: usize,
}

fn parse_options(argv: &[String]) -> Result<Options> {
    let mut cached = false;
    let mut match_missing = false;
    let mut quiet = false;
    let mut exit_code = false;
    let mut abbrev: Option<usize> = None;
    let mut tree_ish: Option<String> = None;
    let mut pathspecs = Vec::new();
    let mut end_of_options = false;
    let mut format = OutputFormat::Raw;
    let mut context_lines: usize = 3;

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
                "--cached" => cached = true,
                "-m" => match_missing = true,
                "--quiet" => quiet = true,
                "--exit-code" => exit_code = true,
                "--raw" => format = OutputFormat::Raw,
                "-p" | "-u" | "--patch" => format = OutputFormat::Patch,
                "--stat" => format = OutputFormat::Stat,
                "--numstat" => format = OutputFormat::NumStat,
                "--name-only" => format = OutputFormat::NameOnly,
                "--name-status" => format = OutputFormat::NameStatus,
                "--abbrev" => abbrev = Some(7),
                _ if arg.starts_with("--abbrev=") => {
                    let value = arg.trim_start_matches("--abbrev=");
                    let parsed = value
                        .parse::<usize>()
                        .with_context(|| format!("invalid --abbrev value: `{value}`"))?;
                    abbrev = Some(parsed);
                }
                _ if arg.starts_with("-U") => {
                    let val = &arg[2..];
                    if val.is_empty() {
                        idx += 1;
                        let next = argv
                            .get(idx)
                            .ok_or_else(|| anyhow::anyhow!("-U requires an argument"))?;
                        context_lines = next
                            .parse()
                            .with_context(|| format!("invalid -U value: `{next}`"))?;
                    } else {
                        context_lines = val
                            .parse()
                            .with_context(|| format!("invalid -U value: `{val}`"))?;
                    }
                }
                _ => bail!("unsupported option: {arg}"),
            }
            idx += 1;
            continue;
        }

        if tree_ish.is_none() {
            tree_ish = Some(arg.clone());
        } else {
            pathspecs.push(arg.clone());
        }
        idx += 1;
    }

    let Some(tree_ish) = tree_ish else {
        bail!("usage: grit diff-index [-m] [--cached] [--raw] [-p] [--quiet] [--exit-code] [--abbrev[=<n>]] <tree-ish> [<path>...]");
    };

    Ok(Options {
        tree_ish,
        pathspecs,
        cached,
        match_missing,
        quiet,
        exit_code,
        abbrev,
        format,
        context_lines,
    })
}

/// Run `grit diff-index`.
pub fn run(args: Args) -> Result<()> {
    let options = parse_options(&args.args)?;
    let repo = Repository::discover(None).context("not a git repository")?;

    // Resolve the tree-ish to a tree OID
    let tree_oid = resolve_to_tree(&repo, &options.tree_ish)?;

    // Load the index
    let index_path = effective_index_path(&repo)?;
    let index = Index::load(&index_path).context("loading index")?;

    // Compute diff entries using the library functions
    let entries = if options.cached {
        // --cached: compare tree vs index
        diff_index_to_tree(&repo.odb, &index, Some(&tree_oid))?
    } else {
        // Default: compare tree vs working tree (via index)
        let work_tree = repo.work_tree.as_deref()
            .ok_or_else(|| anyhow::anyhow!("this operation must be run in a work tree"))?;
        diff_tree_to_worktree(&repo.odb, Some(&tree_oid), work_tree, &index)?
    };

    // For non-cached mode, git diff-index shows 0{40} for the worktree side
    let entries = if !options.cached {
        entries.into_iter().map(|mut e| {
            if e.status != DiffStatus::Deleted {
                e.new_oid = zero_oid();
            }
            e
        }).collect()
    } else {
        entries
    };

    // Filter by pathspecs
    let entries = filter_pathspecs(entries, &options.pathspecs);

    // Filter out missing worktree files if -m is set and not --cached
    let entries = if options.match_missing && !options.cached {
        entries.into_iter().filter(|e| {
            // -m: suppress entries where the worktree file is missing
            // (i.e. status is Deleted and the file was tracked)
            e.status != DiffStatus::Deleted
        }).collect()
    } else {
        entries
    };

    if !options.quiet {
        let stdout = io::stdout();
        let mut out = stdout.lock();

        match options.format {
            OutputFormat::Raw => {
                for entry in &entries {
                    if let Some(abbrev) = options.abbrev {
                        writeln!(out, "{}", format_raw_with_abbrev(entry, abbrev))?;
                    } else {
                        writeln!(out, "{}", format_raw(entry))?;
                    }
                }
            }
            OutputFormat::Patch => {
                write_patch(&mut out, &entries, &repo.odb, options.context_lines,
                    if options.cached { None } else { repo.work_tree.as_deref() })?;
            }
            OutputFormat::Stat => {
                write_stat(&mut out, &entries, &repo.odb,
                    if options.cached { None } else { repo.work_tree.as_deref() })?;
            }
            OutputFormat::NumStat => {
                write_numstat(&mut out, &entries, &repo.odb,
                    if options.cached { None } else { repo.work_tree.as_deref() })?;
            }
            OutputFormat::NameOnly => {
                for entry in &entries {
                    writeln!(out, "{}", entry.path())?;
                }
            }
            OutputFormat::NameStatus => {
                for entry in &entries {
                    writeln!(out, "{}\t{}", entry.status.letter(), entry.path())?;
                }
            }
        }
    }

    if (options.exit_code || options.quiet) && !entries.is_empty() {
        std::process::exit(1);
    }
    Ok(())
}

/// Resolve a revision to a tree OID, peeling commits.
fn resolve_to_tree(repo: &Repository, spec: &str) -> Result<ObjectId> {
    let oid = resolve_revision(repo, spec)
        .with_context(|| format!("unknown revision: '{spec}'"))?;
    loop {
        let obj = repo.odb.read(&oid)?;
        match obj.kind {
            ObjectKind::Tree => return Ok(oid),
            ObjectKind::Commit => {
                let commit = parse_commit(&obj.data)?;
                return Ok(commit.tree);
            }
            _ => bail!("object '{}' does not name a tree", oid),
        }
    }
}

/// Format a raw diff entry with abbreviated OIDs (no trailing `...`), matching git behavior.
fn format_raw_with_abbrev(entry: &DiffEntry, abbrev_len: usize) -> String {
    let old_hex = entry.old_oid.to_hex();
    let new_hex = entry.new_oid.to_hex();
    let old_abbrev = &old_hex[..abbrev_len.min(old_hex.len())];
    let new_abbrev = &new_hex[..abbrev_len.min(new_hex.len())];
    let path = entry.path();
    format!(
        ":{} {} {} {} {}\t{}",
        entry.old_mode, entry.new_mode, old_abbrev, new_abbrev, entry.status.letter(), path
    )
}

/// Filter diff entries by pathspecs.
fn filter_pathspecs(entries: Vec<DiffEntry>, pathspecs: &[String]) -> Vec<DiffEntry> {
    if pathspecs.is_empty() {
        return entries;
    }
    entries
        .into_iter()
        .filter(|e| {
            let path = e.path();
            pathspecs.iter().any(|spec| {
                if let Some(prefix) = spec.strip_suffix('/') {
                    path == prefix || path.starts_with(&format!("{prefix}/"))
                } else {
                    path == spec || path.starts_with(&format!("{spec}/"))
                }
            })
        })
        .collect()
}

/// Read content for a blob, falling back to the working tree.
fn read_content(odb: &Odb, oid: &ObjectId, work_tree: Option<&Path>, path: &str) -> String {
    let raw = read_content_raw(odb, oid, work_tree, path);
    String::from_utf8_lossy(&raw).into_owned()
}

fn read_content_raw(odb: &Odb, oid: &ObjectId, work_tree: Option<&Path>, path: &str) -> Vec<u8> {
    if *oid == zero_oid() {
        return Vec::new();
    }
    if let Ok(obj) = odb.read(oid) {
        return obj.data;
    }
    if let Some(wt) = work_tree {
        if path != "/dev/null" {
            if let Ok(data) = std::fs::read(wt.join(path)) {
                return data;
            }
        }
    }
    Vec::new()
}

/// Check if content appears to be binary.
fn is_binary(data: &[u8]) -> bool {
    let check_len = data.len().min(8192);
    data[..check_len].contains(&0)
}

/// Write unified patch output for diff entries.
fn write_patch(
    out: &mut impl Write,
    entries: &[DiffEntry],
    odb: &Odb,
    context_lines: usize,
    work_tree: Option<&Path>,
) -> Result<()> {
    for entry in entries {
        let old_path = entry.old_path.as_deref().unwrap_or(entry.new_path.as_deref().unwrap_or(""));
        let new_path = entry.new_path.as_deref().unwrap_or(entry.old_path.as_deref().unwrap_or(""));

        writeln!(out, "diff --git a/{old_path} b/{new_path}")?;

        match entry.status {
            DiffStatus::Added => {
                writeln!(out, "new file mode {}", entry.new_mode)?;
                writeln!(out, "index {}..{}", &entry.old_oid.to_hex()[..7], &entry.new_oid.to_hex()[..7])?;
            }
            DiffStatus::Deleted => {
                writeln!(out, "deleted file mode {}", entry.old_mode)?;
                writeln!(out, "index {}..{}", &entry.old_oid.to_hex()[..7], &entry.new_oid.to_hex()[..7])?;
            }
            DiffStatus::Modified => {
                if entry.old_mode != entry.new_mode {
                    writeln!(out, "old mode {}", entry.old_mode)?;
                    writeln!(out, "new mode {}", entry.new_mode)?;
                }
                if entry.old_mode == entry.new_mode {
                    writeln!(out, "index {}..{} {}", &entry.old_oid.to_hex()[..7], &entry.new_oid.to_hex()[..7], entry.old_mode)?;
                } else {
                    writeln!(out, "index {}..{}", &entry.old_oid.to_hex()[..7], &entry.new_oid.to_hex()[..7])?;
                }
            }
            DiffStatus::Renamed => {
                writeln!(out, "similarity index 100%")?;
                writeln!(out, "rename from {old_path}")?;
                writeln!(out, "rename to {new_path}")?;
            }
            DiffStatus::Copied => {
                writeln!(out, "similarity index 100%")?;
                writeln!(out, "copy from {old_path}")?;
                writeln!(out, "copy to {new_path}")?;
            }
            DiffStatus::TypeChanged => {
                writeln!(out, "old mode {}", entry.old_mode)?;
                writeln!(out, "new mode {}", entry.new_mode)?;
            }
            DiffStatus::Unmerged => {}
        }

        let old_raw = read_content_raw(odb, &entry.old_oid, None, old_path);
        let new_raw = read_content_raw(odb, &entry.new_oid, work_tree, new_path);

        if is_binary(&old_raw) || is_binary(&new_raw) {
            writeln!(out, "Binary files a/{old_path} and b/{new_path} differ")?;
            continue;
        }

        let old_content = String::from_utf8_lossy(&old_raw).into_owned();
        let new_content = String::from_utf8_lossy(&new_raw).into_owned();

        let display_old = if entry.status == DiffStatus::Added { "/dev/null" } else { old_path };
        let display_new = if entry.status == DiffStatus::Deleted { "/dev/null" } else { new_path };

        let patch = unified_diff(&old_content, &new_content, display_old, display_new, context_lines);
        write!(out, "{patch}")?;
    }
    Ok(())
}

/// Write --stat output.
fn write_stat(
    out: &mut impl Write,
    entries: &[DiffEntry],
    odb: &Odb,
    work_tree: Option<&Path>,
) -> Result<()> {
    if entries.is_empty() {
        return Ok(());
    }

    let max_path_len = entries.iter().map(|e| e.path().len()).max().unwrap_or(0);
    let mut total_ins = 0usize;
    let mut total_del = 0usize;

    for entry in entries {
        let old_content = read_content(odb, &entry.old_oid, None, entry.path());
        let new_content = read_content(odb, &entry.new_oid, work_tree, entry.path());
        let (ins, del) = count_changes(&old_content, &new_content);
        total_ins += ins;
        total_del += del;
        let total = ins + del;
        let plus = "+".repeat(ins.min(50));
        let minus = "-".repeat(del.min(50));
        writeln!(out, " {:<width$} | {:>4} {plus}{minus}", entry.path(), total, width = max_path_len)?;
    }

    let n = entries.len();
    let mut summary = format!(" {} file{} changed", n, if n == 1 { "" } else { "s" });
    if total_ins > 0 {
        summary.push_str(&format!(", {} insertion{}(+)", total_ins, if total_ins == 1 { "" } else { "s" }));
    }
    if total_del > 0 {
        summary.push_str(&format!(", {} deletion{}(-)", total_del, if total_del == 1 { "" } else { "s" }));
    }
    writeln!(out, "{summary}")?;
    Ok(())
}

/// Write --numstat output.
fn write_numstat(
    out: &mut impl Write,
    entries: &[DiffEntry],
    odb: &Odb,
    work_tree: Option<&Path>,
) -> Result<()> {
    for entry in entries {
        let old_content = read_content(odb, &entry.old_oid, None, entry.path());
        let new_content = read_content(odb, &entry.new_oid, work_tree, entry.path());
        let (ins, del) = count_changes(&old_content, &new_content);
        writeln!(out, "{ins}\t{del}\t{}", entry.path())?;
    }
    Ok(())
}

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
