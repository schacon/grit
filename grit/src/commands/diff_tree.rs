//! `grit diff-tree` — compare the content and mode of blobs found via two tree objects.
//!
//! # Modes
//!
//! - Two tree-ish arguments: compare the trees directly.
//! - One commit argument: compare the commit's tree against its parent(s).
//! - `--stdin`: read commit or tree-pair OIDs from standard input.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::diff::{
    count_changes, diff_trees, format_raw, format_stat_line, unified_diff, DiffEntry, DiffStatus,
};
use grit_lib::objects::{parse_commit, parse_tree, ObjectId, ObjectKind};
use grit_lib::odb::Odb;
use grit_lib::repo::Repository;
use grit_lib::rev_parse::resolve_revision;
use std::io::{self, BufRead, Write};

/// Arguments for `grit diff-tree`.
#[derive(Debug, ClapArgs)]
#[command(about = "Compare the content and mode of blobs found via two tree objects")]
pub struct Args {
    /// All flags and positional arguments forwarded from the CLI.
    #[arg(
        value_name = "ARG",
        num_args = 0..,
        allow_hyphen_values = true,
        trailing_var_arg = true
    )]
    pub args: Vec<String>,
}

// ── Output format ────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputFormat {
    Raw,
    Patch,
    Stat,
    NameOnly,
    NameStatus,
}

// ── Parsed options ───────────────────────────────────────────────────

struct Options {
    /// Positional tree-ish or commit arguments (0–2).
    objects: Vec<String>,
    /// Optional path-limiting specs.
    pathspecs: Vec<String>,
    /// Recurse into sub-trees (`-r`).
    recursive: bool,
    /// Show root commit as diff against empty tree (`--root`).
    root: bool,
    /// Read OIDs from stdin (`--stdin`).
    stdin_mode: bool,
    /// Suppress the commit-id header line in stdin mode (`--no-commit-id`).
    no_commit_id: bool,
    /// Show commit message before diff in stdin mode (`-v`).
    verbose: bool,
    /// Suppress diff output in stdin mode (`-s`).
    suppress_diff: bool,
    /// Show diffs for merge commits in stdin mode (`-m`).
    show_merges: bool,
    /// Output format.
    format: OutputFormat,
    /// Number of unified context lines for patch output.
    context_lines: usize,
    /// Abbreviate OIDs to this length (None = full).
    abbrev: Option<usize>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            objects: Vec::new(),
            pathspecs: Vec::new(),
            recursive: false,
            root: false,
            stdin_mode: false,
            no_commit_id: false,
            verbose: false,
            suppress_diff: false,
            show_merges: false,
            format: OutputFormat::Raw,
            context_lines: 3,
            abbrev: None,
        }
    }
}

/// Parse the raw argument vector.
fn parse_options(argv: &[String]) -> Result<Options> {
    let mut opts = Options::default();
    let mut end_of_options = false;
    let mut i = 0usize;

    while i < argv.len() {
        let arg = &argv[i];

        if !end_of_options && arg == "--" {
            end_of_options = true;
            i += 1;
            continue;
        }

        if !end_of_options && arg.starts_with('-') {
            match arg.as_str() {
                "-r" => opts.recursive = true,
                "-t" => opts.recursive = true, // -t implies -r
                "--root" => opts.root = true,
                "--stdin" => opts.stdin_mode = true,
                "--no-commit-id" => opts.no_commit_id = true,
                "-v" => opts.verbose = true,
                "-s" => opts.suppress_diff = true,
                "-m" => opts.show_merges = true,
                "--raw" => opts.format = OutputFormat::Raw,
                "-p" | "-u" | "--patch" => opts.format = OutputFormat::Patch,
                "--stat" => opts.format = OutputFormat::Stat,
                "--name-only" => opts.format = OutputFormat::NameOnly,
                "--name-status" => opts.format = OutputFormat::NameStatus,
                "--abbrev" => opts.abbrev = Some(7),
                _ if arg.starts_with("--abbrev=") => {
                    let val = &arg["--abbrev=".len()..];
                    opts.abbrev = Some(
                        val.parse::<usize>()
                            .with_context(|| format!("invalid --abbrev value: `{val}`"))?,
                    );
                }
                _ if arg.starts_with("-U") => {
                    let val = &arg[2..];
                    if val.is_empty() {
                        i += 1;
                        let next = argv
                            .get(i)
                            .ok_or_else(|| anyhow::anyhow!("-U requires an argument"))?;
                        opts.context_lines = next
                            .parse()
                            .with_context(|| format!("invalid -U value: `{next}`"))?;
                    } else {
                        opts.context_lines = val
                            .parse()
                            .with_context(|| format!("invalid -U value: `{val}`"))?;
                    }
                }
                // Silently accept common diff options that we do not implement.
                "--no-renames" | "--no-rename-empty" | "--always" | "--diff-merges=off" | "-c"
                | "--cc" => {}
                _ if arg.starts_with("--diff-filter=")
                    || arg.starts_with("--diff-merges=")
                    || arg.starts_with("--pretty")
                    || arg.starts_with("--format=") =>
                {
                    // ignored
                }
                _ => bail!("unknown option: {arg}"),
            }
            i += 1;
            continue;
        }

        // Positional: first two are tree-ish, rest are pathspecs.
        if end_of_options || opts.objects.len() >= 2 {
            opts.pathspecs.push(arg.clone());
        } else {
            opts.objects.push(arg.clone());
        }
        i += 1;
    }

    Ok(opts)
}

// ── Public entry point ───────────────────────────────────────────────

/// Run `grit diff-tree`.
pub fn run(args: Args) -> Result<()> {
    let opts = parse_options(&args.args)?;
    let repo = Repository::discover(None).context("not a git repository")?;

    let stdout = io::stdout();
    let mut out = stdout.lock();

    if opts.stdin_mode {
        run_stdin_mode(&repo, &opts, &mut out)
    } else if opts.objects.len() == 2 {
        run_two_trees(&repo, &opts, &mut out)
    } else if opts.objects.len() == 1 {
        run_one_commit(&repo, &opts, &mut out)
    } else {
        bail!(
            "usage: grit diff-tree [--stdin] [-r] [--root] [-p|--stat|--name-only|--name-status] \
             <tree-ish> [<tree-ish>] [<path>...]"
        )
    }
}

// ── Two-tree mode ────────────────────────────────────────────────────

fn run_two_trees(repo: &Repository, opts: &Options, out: &mut impl Write) -> Result<()> {
    let oid1 = resolve_to_tree(repo, &opts.objects[0])?;
    let oid2 = resolve_to_tree(repo, &opts.objects[1])?;
    let entries = diff_maybe_recursive(&repo.odb, Some(&oid1), Some(&oid2), opts.recursive)?;
    let filtered = filter_pathspecs(entries, &opts.pathspecs);
    print_diff(out, &repo.odb, &filtered, opts)
}

// ── Single-commit mode ───────────────────────────────────────────────

fn run_one_commit(repo: &Repository, opts: &Options, out: &mut impl Write) -> Result<()> {
    let spec = &opts.objects[0];
    let oid =
        resolve_revision(repo, spec).with_context(|| format!("unknown revision: '{spec}'"))?;
    let obj = repo.odb.read(&oid).context("reading object")?;

    match obj.kind {
        ObjectKind::Commit => {
            let commit = parse_commit(&obj.data).context("parsing commit")?;
            if commit.parents.is_empty() {
                if opts.root {
                    let entries =
                        diff_maybe_recursive(&repo.odb, None, Some(&commit.tree), opts.recursive)?;
                    let filtered = filter_pathspecs(entries, &opts.pathspecs);
                    print_diff(out, &repo.odb, &filtered, opts)?;
                }
                // Without --root, root commits produce no output.
            } else {
                let parent_tree = commit_tree(&repo.odb, &commit.parents[0])?;
                let entries = diff_maybe_recursive(
                    &repo.odb,
                    Some(&parent_tree),
                    Some(&commit.tree),
                    opts.recursive,
                )?;
                let filtered = filter_pathspecs(entries, &opts.pathspecs);
                print_diff(out, &repo.odb, &filtered, opts)?;
            }
        }
        _ => bail!("'{spec}' does not name a commit"),
    }

    Ok(())
}

// ── --stdin mode ─────────────────────────────────────────────────────

fn run_stdin_mode(repo: &Repository, opts: &Options, out: &mut impl Write) -> Result<()> {
    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let line = line.context("reading stdin")?;
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            continue;
        }
        process_stdin_line(repo, opts, out, trimmed)?;
    }
    Ok(())
}

/// Process one line from stdin.
fn process_stdin_line(
    repo: &Repository,
    opts: &Options,
    out: &mut impl Write,
    line: &str,
) -> Result<()> {
    // Split on the first space to get the leading OID and optional remainder.
    let (oid_str, rest) = line
        .split_once(' ')
        .map(|(a, b)| (a, b))
        .unwrap_or((line, ""));

    let oid = match oid_str.parse::<ObjectId>() {
        Ok(o) => o,
        Err(_) => {
            // Not a valid OID: pass through.
            writeln!(out, "{line}")?;
            return Ok(());
        }
    };

    let obj = match repo.odb.read(&oid) {
        Ok(o) => o,
        Err(_) => {
            writeln!(out, "{line}")?;
            return Ok(());
        }
    };

    match obj.kind {
        ObjectKind::Commit => process_stdin_commit(repo, opts, out, &oid, &obj.data, rest),
        ObjectKind::Tree => process_stdin_two_trees(repo, opts, out, &oid, rest),
        _ => {
            writeln!(out, "{line}")?;
            Ok(())
        }
    }
}

/// Handle a commit line from stdin.
fn process_stdin_commit(
    repo: &Repository,
    opts: &Options,
    out: &mut impl Write,
    oid: &ObjectId,
    data: &[u8],
    rest: &str,
) -> Result<()> {
    let commit = parse_commit(data).context("parsing commit")?;

    // Print commit-id header (unless suppressed).
    if !opts.no_commit_id {
        writeln!(out, "{}", oid.to_hex())?;
    }

    if opts.verbose {
        writeln!(out, "commit {}", oid.to_hex())?;
        writeln!(out)?;
        for msg_line in commit.message.lines() {
            writeln!(out, "    {msg_line}")?;
        }
        writeln!(out)?;
    }

    if opts.suppress_diff {
        return Ok(());
    }

    // Skip merge commits unless -m.
    if commit.parents.len() > 1 && !opts.show_merges {
        return Ok(());
    }

    // Override parents if the line contains extra OIDs.
    let extra_parents = parse_oid_list(rest)?;
    let parent_oids: Vec<ObjectId> = if extra_parents.is_empty() {
        commit.parents.clone()
    } else {
        extra_parents
    };

    if parent_oids.is_empty() {
        if opts.root {
            let entries =
                diff_maybe_recursive(&repo.odb, None, Some(&commit.tree), opts.recursive)?;
            let filtered = filter_pathspecs(entries, &opts.pathspecs);
            print_diff(out, &repo.odb, &filtered, opts)?;
        }
    } else {
        let parent_tree = commit_tree(&repo.odb, &parent_oids[0])?;
        let entries = diff_maybe_recursive(
            &repo.odb,
            Some(&parent_tree),
            Some(&commit.tree),
            opts.recursive,
        )?;
        let filtered = filter_pathspecs(entries, &opts.pathspecs);
        print_diff(out, &repo.odb, &filtered, opts)?;
    }

    Ok(())
}

/// Handle a two-tree line from stdin: `<tree1> <tree2>`.
fn process_stdin_two_trees(
    repo: &Repository,
    opts: &Options,
    out: &mut impl Write,
    oid1: &ObjectId,
    rest: &str,
) -> Result<()> {
    let oid2_str = rest.trim();
    if oid2_str.is_empty() {
        bail!("stdin two-tree format requires a second OID after the first");
    }
    let oid2 = oid2_str
        .parse::<ObjectId>()
        .with_context(|| format!("invalid OID: `{oid2_str}`"))?;

    // Print both tree OIDs.
    writeln!(out, "{} {}", oid1.to_hex(), oid2.to_hex())?;

    let entries = diff_maybe_recursive(&repo.odb, Some(oid1), Some(&oid2), opts.recursive)?;
    let filtered = filter_pathspecs(entries, &opts.pathspecs);
    print_diff(out, &repo.odb, &filtered, opts)
}

// ── Diff helpers ─────────────────────────────────────────────────────

/// Compute the diff, recursing into sub-trees only when `recursive` is set.
fn diff_maybe_recursive(
    odb: &Odb,
    old_tree: Option<&ObjectId>,
    new_tree: Option<&ObjectId>,
    recursive: bool,
) -> Result<Vec<DiffEntry>> {
    if recursive {
        diff_trees(odb, old_tree, new_tree, "").map_err(Into::into)
    } else {
        diff_trees_toplevel(odb, old_tree, new_tree)
    }
}

/// Non-recursive tree diff: only top-level entries.
///
/// Tree sub-directories are shown as single entries with their tree OIDs,
/// not expanded.
fn diff_trees_toplevel(
    odb: &Odb,
    old_tree_oid: Option<&ObjectId>,
    new_tree_oid: Option<&ObjectId>,
) -> Result<Vec<DiffEntry>> {
    let zero = grit_lib::diff::zero_oid();

    let old_entries = match old_tree_oid {
        Some(oid) => {
            let obj = odb.read(oid).context("reading old tree")?;
            parse_tree(&obj.data).context("parsing old tree")?
        }
        None => Vec::new(),
    };
    let new_entries = match new_tree_oid {
        Some(oid) => {
            let obj = odb.read(oid).context("reading new tree")?;
            parse_tree(&obj.data).context("parsing new tree")?
        }
        None => Vec::new(),
    };

    let mut result = Vec::new();
    let mut oi = 0usize;
    let mut ni = 0usize;

    while oi < old_entries.len() || ni < new_entries.len() {
        match (old_entries.get(oi), new_entries.get(ni)) {
            (Some(o), Some(n)) => {
                let o_name = String::from_utf8_lossy(&o.name);
                let n_name = String::from_utf8_lossy(&n.name);
                match o_name.cmp(&n_name) {
                    std::cmp::Ordering::Less => {
                        result.push(DiffEntry {
                            status: DiffStatus::Deleted,
                            old_path: Some(o_name.into_owned()),
                            new_path: None,
                            old_mode: format!("{:06o}", o.mode),
                            new_mode: "000000".to_owned(),
                            old_oid: o.oid,
                            new_oid: zero,
                        });
                        oi += 1;
                    }
                    std::cmp::Ordering::Greater => {
                        result.push(DiffEntry {
                            status: DiffStatus::Added,
                            old_path: None,
                            new_path: Some(n_name.into_owned()),
                            old_mode: "000000".to_owned(),
                            new_mode: format!("{:06o}", n.mode),
                            old_oid: zero,
                            new_oid: n.oid,
                        });
                        ni += 1;
                    }
                    std::cmp::Ordering::Equal => {
                        if o.oid != n.oid || o.mode != n.mode {
                            let path = o_name.into_owned();
                            let status = if o.mode != n.mode && o.oid == n.oid {
                                DiffStatus::TypeChanged
                            } else {
                                DiffStatus::Modified
                            };
                            result.push(DiffEntry {
                                status,
                                old_path: Some(path.clone()),
                                new_path: Some(path),
                                old_mode: format!("{:06o}", o.mode),
                                new_mode: format!("{:06o}", n.mode),
                                old_oid: o.oid,
                                new_oid: n.oid,
                            });
                        }
                        oi += 1;
                        ni += 1;
                    }
                }
            }
            (Some(o), None) => {
                result.push(DiffEntry {
                    status: DiffStatus::Deleted,
                    old_path: Some(String::from_utf8_lossy(&o.name).into_owned()),
                    new_path: None,
                    old_mode: format!("{:06o}", o.mode),
                    new_mode: "000000".to_owned(),
                    old_oid: o.oid,
                    new_oid: zero,
                });
                oi += 1;
            }
            (None, Some(n)) => {
                result.push(DiffEntry {
                    status: DiffStatus::Added,
                    old_path: None,
                    new_path: Some(String::from_utf8_lossy(&n.name).into_owned()),
                    old_mode: "000000".to_owned(),
                    new_mode: format!("{:06o}", n.mode),
                    old_oid: zero,
                    new_oid: n.oid,
                });
                ni += 1;
            }
            (None, None) => break,
        }
    }

    Ok(result)
}

// ── Output ───────────────────────────────────────────────────────────

/// Print the diff entries according to `opts.format`.
fn print_diff(
    out: &mut impl Write,
    odb: &Odb,
    entries: &[DiffEntry],
    opts: &Options,
) -> Result<()> {
    match opts.format {
        OutputFormat::Raw => {
            for entry in entries {
                writeln!(out, "{}", format_raw(entry))?;
            }
        }
        OutputFormat::Patch => {
            for entry in entries {
                write_patch_entry(out, odb, entry, opts.context_lines)?;
            }
        }
        OutputFormat::Stat => {
            print_stat_summary(out, odb, entries)?;
        }
        OutputFormat::NameOnly => {
            for entry in entries {
                writeln!(out, "{}", entry.path())?;
            }
        }
        OutputFormat::NameStatus => {
            for entry in entries {
                writeln!(out, "{}\t{}", entry.status.letter(), entry.path())?;
            }
        }
    }
    Ok(())
}

/// Write a unified-diff block for one entry.
fn write_patch_entry(
    out: &mut impl Write,
    odb: &Odb,
    entry: &DiffEntry,
    context_lines: usize,
) -> Result<()> {
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
            writeln!(
                out,
                "index {}..{}",
                &entry.old_oid.to_hex()[..7],
                &entry.new_oid.to_hex()[..7]
            )?;
        }
        DiffStatus::Deleted => {
            writeln!(out, "deleted file mode {}", entry.old_mode)?;
            writeln!(
                out,
                "index {}..{}",
                &entry.old_oid.to_hex()[..7],
                &entry.new_oid.to_hex()[..7]
            )?;
        }
        DiffStatus::Modified => {
            if entry.old_mode != entry.new_mode {
                writeln!(out, "old mode {}", entry.old_mode)?;
                writeln!(out, "new mode {}", entry.new_mode)?;
            }
            if entry.old_mode == entry.new_mode {
                writeln!(
                    out,
                    "index {}..{} {}",
                    &entry.old_oid.to_hex()[..7],
                    &entry.new_oid.to_hex()[..7],
                    entry.old_mode
                )?;
            } else {
                writeln!(
                    out,
                    "index {}..{}",
                    &entry.old_oid.to_hex()[..7],
                    &entry.new_oid.to_hex()[..7]
                )?;
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

    let (old_content, new_content) = read_blob_pair(odb, entry)?;
    let display_old = if entry.status == DiffStatus::Added {
        "/dev/null"
    } else {
        old_path
    };
    let display_new = if entry.status == DiffStatus::Deleted {
        "/dev/null"
    } else {
        new_path
    };
    let patch = unified_diff(
        &old_content,
        &new_content,
        display_old,
        display_new,
        context_lines,
    );
    write!(out, "{patch}")?;

    Ok(())
}

/// Write a `--stat` summary.
fn print_stat_summary(out: &mut impl Write, odb: &Odb, entries: &[DiffEntry]) -> Result<()> {
    let max_path_len = entries.iter().map(|e| e.path().len()).max().unwrap_or(0);
    let mut total_ins = 0usize;
    let mut total_del = 0usize;

    for entry in entries {
        let (old_content, new_content) = read_blob_pair(odb, entry)?;
        let (ins, del) = count_changes(&old_content, &new_content);
        total_ins += ins;
        total_del += del;
        writeln!(
            out,
            "{}",
            format_stat_line(entry.path(), ins, del, max_path_len)
        )?;
    }

    let n = entries.len();
    writeln!(
        out,
        " {} file{} changed, {} insertion{}, {} deletion{}",
        n,
        if n == 1 { "" } else { "s" },
        total_ins,
        if total_ins == 1 { "+" } else { "s(+)" },
        total_del,
        if total_del == 1 { "-" } else { "s(-)" },
    )?;

    Ok(())
}

// ── Small helpers ─────────────────────────────────────────────────────

/// Resolve a tree-ish (commit or tree) to a tree OID.
fn resolve_to_tree(repo: &Repository, spec: &str) -> Result<ObjectId> {
    let mut oid =
        resolve_revision(repo, spec).with_context(|| format!("unknown revision: '{spec}'"))?;
    loop {
        let obj = repo.odb.read(&oid)?;
        match obj.kind {
            ObjectKind::Tree => return Ok(oid),
            ObjectKind::Commit => {
                let commit = parse_commit(&obj.data)?;
                oid = commit.tree;
            }
            _ => bail!("'{spec}' does not name a tree or commit"),
        }
    }
}

/// Retrieve the tree OID from a commit OID.
fn commit_tree(odb: &Odb, commit_oid: &ObjectId) -> Result<ObjectId> {
    let obj = odb.read(commit_oid).context("reading parent commit")?;
    let commit = parse_commit(&obj.data).context("parsing parent commit")?;
    Ok(commit.tree)
}

/// Read both blob sides of a diff entry as UTF-8 strings.
fn read_blob_pair(odb: &Odb, entry: &DiffEntry) -> Result<(String, String)> {
    let zero = grit_lib::diff::zero_oid();

    let old_content = if entry.old_oid == zero {
        String::new()
    } else {
        match odb.read(&entry.old_oid) {
            Ok(obj) => String::from_utf8_lossy(&obj.data).into_owned(),
            Err(_) => String::new(),
        }
    };

    let new_content = if entry.new_oid == zero {
        String::new()
    } else {
        match odb.read(&entry.new_oid) {
            Ok(obj) => String::from_utf8_lossy(&obj.data).into_owned(),
            Err(_) => String::new(),
        }
    };

    Ok((old_content, new_content))
}

/// Filter diff entries to only those matching the given path-specs.
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

/// Parse a whitespace-separated list of OID strings.
fn parse_oid_list(s: &str) -> Result<Vec<ObjectId>> {
    s.split_whitespace()
        .filter(|t| !t.is_empty())
        .map(|t| {
            t.parse::<ObjectId>()
                .with_context(|| format!("invalid OID: `{t}`"))
        })
        .collect()
}
