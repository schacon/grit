//! `grit diff` — show changes between commits, commit and working tree, etc.
//!
//! Modes:
//! - No revisions: working tree vs index (unstaged changes)
//! - `--cached [<commit>]`: index vs HEAD (or specified commit) — staged changes
//! - `<commit>`: commit's tree vs working tree (combined view)
//! - `<commit> <commit>`: commit-to-commit diff
//!
//! Output formats: unified patch (default), `--stat`, `--numstat`,
//! `--name-only`, `--name-status`.
//!
//! Exit codes: `--exit-code` / `--quiet` return exit code 1 if there are
//! differences.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::diff::{
    count_changes, diff_index_to_tree, diff_index_to_worktree, diff_tree_to_worktree, diff_trees,
    format_stat_line, unified_diff, zero_oid, DiffEntry, DiffStatus,
};
use grit_lib::error::Error;
use grit_lib::index::Index;
use grit_lib::objects::{parse_commit, ObjectId, ObjectKind};
use grit_lib::odb::Odb;
use grit_lib::repo::Repository;
use grit_lib::rev_parse::resolve_revision;
use std::io::{self, Write};

/// Arguments for `grit diff`.
#[derive(Debug, ClapArgs)]
#[command(about = "Show changes between commits, commit and working tree, etc.")]
pub struct Args {
    /// Show staged changes (index vs HEAD). Alias: --staged.
    #[arg(long = "cached", alias = "staged")]
    pub cached: bool,

    /// Show a diffstat summary instead of the patch.
    #[arg(long = "stat")]
    pub stat: bool,

    /// Show machine-readable stat (additions/deletions per file).
    #[arg(long = "numstat")]
    pub numstat: bool,

    /// Show only the names of changed files.
    #[arg(long = "name-only")]
    pub name_only: bool,

    /// Show the names and status of changed files.
    #[arg(long = "name-status")]
    pub name_status: bool,

    /// Exit with status 1 if there are differences, 0 otherwise.
    #[arg(long = "exit-code")]
    pub exit_code: bool,

    /// Suppress diff output; implies --exit-code.
    #[arg(short = 'q', long = "quiet")]
    pub quiet: bool,

    /// Number of context lines in unified diff output (default: 3).
    #[arg(short = 'U', long = "unified", value_name = "N")]
    pub unified: Option<usize>,

    /// Commits or paths. Use `--` to separate revisions from paths.
    #[arg(trailing_var_arg = true, allow_hyphen_values = false)]
    pub args: Vec<String>,
}

/// Run the `diff` command.
pub fn run(args: Args) -> Result<()> {
    let (revs, paths) = parse_rev_and_paths(&args.args);

    let repo = Repository::discover(None).context("not a git repository")?;
    let work_tree = repo.work_tree.as_deref();

    // Load index (empty if not found)
    let index = match Index::load(&repo.index_path()) {
        Ok(idx) => idx,
        Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => Index::new(),
        Err(e) => return Err(e.into()),
    };

    // Get HEAD tree OID (None if unborn)
    let head_tree = get_head_tree(&repo)?;

    let entries: Vec<DiffEntry> = match (args.cached, revs.len()) {
        (true, 0) => {
            // --cached with no revision: index vs HEAD
            diff_index_to_tree(&repo.odb, &index, head_tree.as_ref())?
        }
        (true, 1) => {
            // --cached with one revision: index vs that commit's tree
            let tree_oid = commit_or_tree_oid(&repo, &revs[0])?;
            diff_index_to_tree(&repo.odb, &index, Some(&tree_oid))?
        }
        (false, 0) => {
            // No flags: unstaged changes (index vs worktree)
            let wt = work_tree
                .ok_or_else(|| anyhow::anyhow!("this operation must be run in a work tree"))?;
            diff_index_to_worktree(&repo.odb, &index, wt)?
        }
        (false, 1) => {
            // One revision: tree vs worktree
            let tree_oid = commit_or_tree_oid(&repo, &revs[0])?;
            let wt = work_tree
                .ok_or_else(|| anyhow::anyhow!("this operation must be run in a work tree"))?;
            diff_tree_to_worktree(&repo.odb, Some(&tree_oid), wt, &index)?
        }
        (_, 2) => {
            // Two revisions: tree-to-tree diff
            let tree1 = commit_or_tree_oid(&repo, &revs[0])?;
            let tree2 = commit_or_tree_oid(&repo, &revs[1])?;
            diff_trees(&repo.odb, Some(&tree1), Some(&tree2), "")?
        }
        _ => {
            bail!("too many revisions");
        }
    };

    // Filter by pathspecs
    let entries = filter_by_paths(entries, &paths);

    let has_diff = !entries.is_empty();

    let stdout = io::stdout();
    let mut out = stdout.lock();

    if !args.quiet {
        let context_lines = args.unified.unwrap_or(3);
        if args.stat {
            write_stat(&mut out, &entries, &repo.odb)?;
        } else if args.numstat {
            write_numstat(&mut out, &entries, &repo.odb)?;
        } else if args.name_only {
            write_name_only(&mut out, &entries)?;
        } else if args.name_status {
            write_name_status(&mut out, &entries)?;
        } else {
            write_patch(&mut out, &entries, &repo.odb, context_lines)?;
        }
    }

    if (args.exit_code || args.quiet) && has_diff {
        std::process::exit(1);
    }

    Ok(())
}

/// Split args on `--` to separate revisions from paths.
///
/// If `--` is present, everything before is revisions, everything after is paths.
/// Otherwise, we try each arg: if it exists as a file, treat it (and all
/// subsequent args) as paths rather than revisions.
fn parse_rev_and_paths(args: &[String]) -> (Vec<String>, Vec<String>) {
    if let Some(sep) = args.iter().position(|a| a == "--") {
        let revs = args[..sep].to_vec();
        let paths = args[sep + 1..].to_vec();
        (revs, paths)
    } else {
        // Without `--`, try to guess: if an arg exists as a file/directory,
        // treat it and everything after as paths.
        let mut revs = Vec::new();
        let mut paths = Vec::new();
        let mut in_paths = false;

        for arg in args {
            if in_paths {
                paths.push(arg.clone());
            } else if std::path::Path::new(arg).exists() {
                in_paths = true;
                paths.push(arg.clone());
            } else {
                revs.push(arg.clone());
            }
        }

        (revs, paths)
    }
}

/// Get HEAD's tree OID, or `None` if the repository is unborn.
fn get_head_tree(repo: &Repository) -> Result<Option<ObjectId>> {
    let head_ref = repo.git_dir.join("HEAD");
    let content = match std::fs::read_to_string(&head_ref) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let content = content.trim();

    // Resolve symbolic ref or direct OID
    let oid_str = if let Some(symref) = content.strip_prefix("ref: ") {
        let ref_path = repo.git_dir.join(symref);
        match std::fs::read_to_string(&ref_path) {
            Ok(s) => s.trim().to_owned(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        }
    } else {
        content.to_owned()
    };

    if oid_str.is_empty() {
        return Ok(None);
    }

    let oid = ObjectId::from_hex(&oid_str).context("parsing HEAD OID")?;
    let obj = repo.odb.read(&oid).context("reading HEAD commit")?;
    let commit = parse_commit(&obj.data).context("parsing HEAD commit")?;
    Ok(Some(commit.tree))
}

/// Resolve a revision spec to a tree OID, handling both commit and tree objects.
fn commit_or_tree_oid(repo: &Repository, spec: &str) -> Result<ObjectId> {
    let mut oid =
        resolve_revision(repo, spec).with_context(|| format!("unknown revision: '{spec}'"))?;
    loop {
        let obj = repo
            .odb
            .read(&oid)
            .with_context(|| format!("reading object {oid}"))?;
        match obj.kind {
            ObjectKind::Tree => return Ok(oid),
            ObjectKind::Commit => {
                let commit = parse_commit(&obj.data).context("parsing commit")?;
                oid = commit.tree;
            }
            _ => bail!("object '{}' does not name a tree or commit", oid),
        }
    }
}

/// Filter diff entries to only those matching the given pathspecs.
/// Empty pathspecs means include everything.
fn filter_by_paths(entries: Vec<DiffEntry>, paths: &[String]) -> Vec<DiffEntry> {
    if paths.is_empty() {
        return entries;
    }
    entries
        .into_iter()
        .filter(|e| {
            let path = e.path();
            paths.iter().any(|spec| {
                if let Some(prefix) = spec.strip_suffix('/') {
                    path == prefix || path.starts_with(&format!("{prefix}/"))
                } else {
                    path == spec || path.starts_with(&format!("{spec}/"))
                }
            })
        })
        .collect()
}

/// Read content for a diff entry side from the ODB.
fn read_content(odb: &Odb, oid: &ObjectId) -> String {
    if *oid == zero_oid() {
        return String::new();
    }
    match odb.read(oid) {
        Ok(obj) => String::from_utf8_lossy(&obj.data).into_owned(),
        Err(_) => String::new(),
    }
}

/// Write a `diff --git` header plus index/mode lines.
fn write_diff_header(out: &mut impl Write, entry: &DiffEntry) -> Result<()> {
    let old_path = entry
        .old_path
        .as_deref()
        .unwrap_or(entry.new_path.as_deref().unwrap_or(""));
    let new_path = entry
        .new_path
        .as_deref()
        .unwrap_or(entry.old_path.as_deref().unwrap_or(""));

    writeln!(out, "diff --git a/{old_path} b/{new_path}")?;

    match entry.status {
        DiffStatus::Added => {
            writeln!(out, "new file mode {}", entry.new_mode)?;
            let old_abbrev = &entry.old_oid.to_hex()[..7];
            let new_abbrev = &entry.new_oid.to_hex()[..7];
            writeln!(out, "index {old_abbrev}..{new_abbrev}")?;
        }
        DiffStatus::Deleted => {
            writeln!(out, "deleted file mode {}", entry.old_mode)?;
            let old_abbrev = &entry.old_oid.to_hex()[..7];
            let new_abbrev = &entry.new_oid.to_hex()[..7];
            writeln!(out, "index {old_abbrev}..{new_abbrev}")?;
        }
        DiffStatus::Modified => {
            if entry.old_mode != entry.new_mode {
                writeln!(out, "old mode {}", entry.old_mode)?;
                writeln!(out, "new mode {}", entry.new_mode)?;
            }
            let old_abbrev = &entry.old_oid.to_hex()[..7];
            let new_abbrev = &entry.new_oid.to_hex()[..7];
            if entry.old_mode == entry.new_mode {
                writeln!(out, "index {old_abbrev}..{new_abbrev} {}", entry.old_mode)?;
            } else {
                writeln!(out, "index {old_abbrev}..{new_abbrev}")?;
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

    Ok(())
}

/// Write full unified diff output with `diff --git` headers.
fn write_patch(
    out: &mut impl Write,
    entries: &[DiffEntry],
    odb: &Odb,
    context_lines: usize,
) -> Result<()> {
    for entry in entries {
        let old_path = entry.old_path.as_deref().unwrap_or("/dev/null");
        let new_path = entry.new_path.as_deref().unwrap_or("/dev/null");

        write_diff_header(out, entry)?;

        let old_content = read_content(odb, &entry.old_oid);
        let new_content = read_content(odb, &entry.new_oid);

        let patch = unified_diff(
            &old_content,
            &new_content,
            old_path,
            new_path,
            context_lines,
        );
        write!(out, "{patch}")?;
    }
    Ok(())
}

/// Write a stat summary for each entry, followed by a totals line.
fn write_stat(out: &mut impl Write, entries: &[DiffEntry], odb: &Odb) -> Result<()> {
    if entries.is_empty() {
        return Ok(());
    }

    let max_path_len = entries.iter().map(|e| e.path().len()).max().unwrap_or(0);

    let mut total_ins = 0usize;
    let mut total_del = 0usize;
    let mut files_changed = 0usize;

    for entry in entries {
        let old_content = read_content(odb, &entry.old_oid);
        let new_content = read_content(odb, &entry.new_oid);
        let (ins, del) = count_changes(&old_content, &new_content);
        let line = format_stat_line(entry.path(), ins, del, max_path_len);
        writeln!(out, "{line}")?;
        total_ins += ins;
        total_del += del;
        files_changed += 1;
    }

    // Summary line
    let mut summary = format!(
        " {} file{} changed",
        files_changed,
        if files_changed == 1 { "" } else { "s" }
    );
    if total_ins > 0 {
        summary.push_str(&format!(
            ", {} insertion{}(+)",
            total_ins,
            if total_ins == 1 { "" } else { "s" }
        ));
    }
    if total_del > 0 {
        summary.push_str(&format!(
            ", {} deletion{}(-)",
            total_del,
            if total_del == 1 { "" } else { "s" }
        ));
    }
    writeln!(out, "{summary}")?;

    Ok(())
}

/// Write machine-readable numstat output: `{insertions}\t{deletions}\t{path}`.
fn write_numstat(out: &mut impl Write, entries: &[DiffEntry], odb: &Odb) -> Result<()> {
    for entry in entries {
        let old_content = read_content(odb, &entry.old_oid);
        let new_content = read_content(odb, &entry.new_oid);
        let (ins, del) = count_changes(&old_content, &new_content);
        writeln!(out, "{ins}\t{del}\t{}", entry.path())?;
    }
    Ok(())
}

/// Write only the names of changed files.
fn write_name_only(out: &mut impl Write, entries: &[DiffEntry]) -> Result<()> {
    for entry in entries {
        writeln!(out, "{}", entry.path())?;
    }
    Ok(())
}

/// Write `{status_letter}\t{path}` for each entry.
fn write_name_status(out: &mut impl Write, entries: &[DiffEntry]) -> Result<()> {
    for entry in entries {
        writeln!(out, "{}\t{}", entry.status.letter(), entry.path())?;
    }
    Ok(())
}
