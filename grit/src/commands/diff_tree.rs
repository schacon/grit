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
    count_changes, detect_renames, diff_trees, diff_trees_show_tree_entries, format_raw,
    format_raw_abbrev, unified_diff, DiffEntry, DiffStatus,
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
    /// Show tree entries in recursive mode (`-t`).
    show_trees: bool,
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
    /// Rename detection threshold (None = disabled).
    find_renames: Option<u32>,
    /// Copy detection threshold (None = disabled).
    find_copies: Option<u32>,
    /// Use all source files for copy detection, not just modified ones.
    find_copies_harder: bool,
    /// Rename limit (max number of rename source candidates).
    rename_limit: Option<usize>,
    /// Show full object IDs in patch headers (--full-index).
    full_index: bool,
    /// Also show raw format with patch (--patch-with-raw).
    patch_with_raw: bool,
    /// Also show stat with patch (--patch-with-stat).
    patch_with_stat: bool,
    /// Show summary (new/deleted/mode changes) after diff.
    summary: bool,
    /// Pretty-print commit header (--pretty). None = off, Some("oneline"), Some("medium"), etc.
    pretty: Option<String>,
    /// Show combined stat+summary after diff.
    stat_too: bool,
    /// Limit recursion depth for --name-only etc.
    max_depth: Option<i32>,
    /// Exit with 1 if there are differences.
    exit_code: bool,
    /// Suppress all output, implies exit_code.
    quiet: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            objects: Vec::new(),
            pathspecs: Vec::new(),
            recursive: false,
            show_trees: false,
            root: false,
            stdin_mode: false,
            no_commit_id: false,
            verbose: false,
            suppress_diff: false,
            show_merges: false,
            format: OutputFormat::Raw,
            context_lines: 3,
            abbrev: None,
            find_renames: None,
            find_copies: None,
            find_copies_harder: false,
            rename_limit: None,
            full_index: false,
            patch_with_raw: false,
            patch_with_stat: false,
            summary: false,
            pretty: None,
            stat_too: false,
            max_depth: None,
            exit_code: false,
            quiet: false,
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
                "-t" => {
                    opts.recursive = true;
                    opts.show_trees = true;
                }
                "--root" => opts.root = true,
                "--stdin" => opts.stdin_mode = true,
                "--no-commit-id" => opts.no_commit_id = true,
                "-v" => opts.verbose = true,
                "-s" => opts.suppress_diff = true,
                "-m" => opts.show_merges = true,
                "--raw" => opts.format = OutputFormat::Raw,
                "-p" | "-u" | "--patch" => opts.format = OutputFormat::Patch,
                "--stat" => {
                    opts.format = OutputFormat::Stat;
                    opts.stat_too = true;
                }
                "--name-only" => opts.format = OutputFormat::NameOnly,
                "--name-status" => opts.format = OutputFormat::NameStatus,
                "--summary" => opts.summary = true,
                "--exit-code" => opts.exit_code = true,
                "-q" | "--quiet" => {
                    opts.quiet = true;
                    opts.exit_code = true;
                }
                "--full-index" => opts.full_index = true,
                _ if arg.starts_with("--max-depth=") => {
                    let val = &arg["--max-depth=".len()..];
                    opts.max_depth = Some(
                        val.parse::<i32>()
                            .with_context(|| format!("invalid --max-depth value: `{val}`"))?,
                    );
                }
                "--patch-with-stat" => {
                    opts.format = OutputFormat::Patch;
                    opts.patch_with_stat = true;
                }
                "--patch-with-raw" => {
                    opts.format = OutputFormat::Patch;
                    opts.patch_with_raw = true;
                }
                "--pretty" | "--pretty=medium" => opts.pretty = Some("medium".to_string()),
                _ if arg.starts_with("--pretty=") => {
                    let val = &arg["--pretty=".len()..];
                    opts.pretty = Some(val.to_string());
                }
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
                "-M" | "--find-renames" => opts.find_renames = Some(50),
                "-C" | "--find-copies" => {
                    opts.find_copies = Some(50);
                    // -C implies rename detection too.
                    if opts.find_renames.is_none() {
                        opts.find_renames = Some(50);
                    }
                }
                "--find-copies-harder" => opts.find_copies_harder = true,
                "--no-renames" => opts.find_renames = None,
                _ if arg.starts_with("-M") => {
                    let val = &arg[2..];
                    let pct = if val.ends_with('%') {
                        val[..val.len() - 1].parse::<u32>().unwrap_or(50)
                    } else {
                        // Could be e.g. -M80 or -M80%
                        val.parse::<u32>().unwrap_or(50)
                    };
                    opts.find_renames = Some(pct);
                }
                _ if arg.starts_with("--find-renames=") => {
                    let val = &arg["--find-renames=".len()..];
                    let pct = if val.ends_with('%') {
                        val[..val.len() - 1].parse::<u32>().unwrap_or(50)
                    } else {
                        val.parse::<u32>().unwrap_or(50)
                    };
                    opts.find_renames = Some(pct);
                }
                _ if arg.starts_with("-l") => {
                    let val = &arg[2..];
                    if let Ok(n) = val.parse::<usize>() {
                        opts.rename_limit = Some(if n == 0 { 32767 } else { n });
                    }
                }
                // Silently accept common diff options that we do not implement.
                "--no-rename-empty" | "--always" | "--diff-merges=off" | "-c" | "--cc"
                | "--check" => {}
                _ if arg.starts_with("--diff-filter=")
                    || arg.starts_with("--diff-merges=")
                    || arg.starts_with("--format=")
                    || arg.starts_with("-S")
                    || arg.starts_with("-G")
                    || arg.starts_with("--pickaxe-all")
                    || arg.starts_with("--pickaxe-regex")
                    || arg.starts_with("-O")
                    || arg.starts_with("-R")
                    || arg.starts_with("--relative") =>
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

    // Patch, stat, summary, name-only, name-status all imply recursion.
    match opts.format {
        OutputFormat::Patch
        | OutputFormat::Stat
        | OutputFormat::NameOnly
        | OutputFormat::NameStatus => {
            opts.recursive = true;
        }
        _ => {}
    }
    if opts.summary {
        opts.recursive = true;
    }

    Ok(opts)
}

// ── Public entry point ───────────────────────────────────────────────

/// Run `grit diff-tree`.
pub fn run(args: Args) -> Result<()> {
    let opts = parse_options(&args.args)?;
    if opts.max_depth.is_some()
        && opts
            .pathspecs
            .iter()
            .any(|spec| spec.contains('*') || spec.contains('?') || spec.contains('['))
    {
        bail!("--max-depth does not support wildcard pathspecs");
    }
    let repo = Repository::discover(None).context("not a git repository")?;

    let stdout = io::stdout();
    let mut out = stdout.lock();

    let has_diff = if opts.stdin_mode {
        run_stdin_mode(&repo, &opts, &mut out)?
    } else if opts.objects.len() == 2 {
        run_two_trees(&repo, &opts, &mut out)?
    } else if opts.objects.len() == 1 {
        run_one_commit(&repo, &opts, &mut out)?
    } else {
        bail!(
            "usage: grit diff-tree [--stdin] [-r] [--root] [-p|--stat|--name-only|--name-status] \
             <tree-ish> [<tree-ish>] [<path>...]"
        )
    };

    if opts.exit_code && has_diff {
        std::process::exit(1);
    }
    Ok(())
}

// ── Two-tree mode ────────────────────────────────────────────────────

fn run_two_trees(repo: &Repository, opts: &Options, out: &mut impl Write) -> Result<bool> {
    let oid1 = resolve_to_tree(repo, &opts.objects[0])?;
    let oid2 = resolve_to_tree(repo, &opts.objects[1])?;
    let entries = diff_with_opts(&repo.odb, Some(&oid1), Some(&oid2), opts)?;
    let filtered = filter_entries(entries, opts);
    let has_diff = !filtered.is_empty();
    if !opts.quiet {
        print_diff(out, &repo.odb, &filtered, opts, Some(&oid1))?;
    }
    Ok(has_diff)
}

// ── Single-commit mode ───────────────────────────────────────────────

fn run_one_commit(repo: &Repository, opts: &Options, out: &mut impl Write) -> Result<bool> {
    let spec = &opts.objects[0];
    let oid =
        resolve_revision(repo, spec).with_context(|| format!("unknown revision: '{spec}'"))?;
    let obj = repo.odb.read(&oid).context("reading object")?;

    let mut has_diff = false;
    match obj.kind {
        ObjectKind::Commit => {
            let commit = parse_commit(&obj.data).context("parsing commit")?;
            if commit.parents.is_empty() {
                if opts.root {
                    let entries = diff_with_opts(&repo.odb, None, Some(&commit.tree), opts)?;
                    let filtered = filter_entries(entries, opts);
                    has_diff = !filtered.is_empty();
                    if !opts.quiet && (has_diff || opts.pretty.is_some()) {
                        write_commit_header(out, &oid, &obj.data, opts)?;
                        print_diff(out, &repo.odb, &filtered, opts, None)?;
                    }
                }
            } else {
                let parent_tree = commit_tree(&repo.odb, &commit.parents[0])?;
                let entries =
                    diff_with_opts(&repo.odb, Some(&parent_tree), Some(&commit.tree), opts)?;
                let filtered = filter_entries(entries, opts);
                has_diff = !filtered.is_empty();
                if !opts.quiet && (has_diff || opts.pretty.is_some()) {
                    write_commit_header(out, &oid, &obj.data, opts)?;
                    print_diff(out, &repo.odb, &filtered, opts, Some(&parent_tree))?;
                }
            }
        }
        _ => bail!("'{spec}' does not name a commit"),
    }

    Ok(has_diff)
}

// ── --stdin mode ─────────────────────────────────────────────────────

fn run_stdin_mode(repo: &Repository, opts: &Options, out: &mut impl Write) -> Result<bool> {
    let stdin = io::stdin();
    let mut has_diff = false;
    for line in stdin.lock().lines() {
        let line = line.context("reading stdin")?;
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            continue;
        }
        if process_stdin_line(repo, opts, out, trimmed)? {
            has_diff = true;
        }
    }
    Ok(has_diff)
}

/// Process one line from stdin.
fn process_stdin_line(
    repo: &Repository,
    opts: &Options,
    out: &mut impl Write,
    line: &str,
) -> Result<bool> {
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
            return Ok(false);
        }
    };

    let obj = match repo.odb.read(&oid) {
        Ok(o) => o,
        Err(_) => {
            writeln!(out, "{line}")?;
            return Ok(false);
        }
    };

    match obj.kind {
        ObjectKind::Commit => process_stdin_commit(repo, opts, out, &oid, &obj.data, rest),
        ObjectKind::Tree => process_stdin_two_trees(repo, opts, out, &oid, rest),
        _ => {
            writeln!(out, "{line}")?;
            Ok(false)
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
) -> Result<bool> {
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
        return Ok(false);
    }

    // Skip merge commits unless -m.
    if commit.parents.len() > 1 && !opts.show_merges {
        return Ok(false);
    }

    // Override parents if the line contains extra OIDs.
    let extra_parents = parse_oid_list(rest)?;
    let parent_oids: Vec<ObjectId> = if extra_parents.is_empty() {
        commit.parents.clone()
    } else {
        extra_parents
    };

    let has_diff = if parent_oids.is_empty() {
        if opts.root {
            let entries = diff_with_opts(&repo.odb, None, Some(&commit.tree), opts)?;
            let filtered = filter_entries(entries, opts);
            let hd = !filtered.is_empty();
            print_diff(out, &repo.odb, &filtered, opts, None)?;
            hd
        } else {
            false
        }
    } else {
        let parent_tree = commit_tree(&repo.odb, &parent_oids[0])?;
        let entries = diff_with_opts(&repo.odb, Some(&parent_tree), Some(&commit.tree), opts)?;
        let filtered = filter_entries(entries, opts);
        let hd = !filtered.is_empty();
        print_diff(out, &repo.odb, &filtered, opts, None)?;
        hd
    };

    Ok(has_diff)
}

/// Handle a two-tree line from stdin: `<tree1> <tree2>`.
fn process_stdin_two_trees(
    repo: &Repository,
    opts: &Options,
    out: &mut impl Write,
    oid1: &ObjectId,
    rest: &str,
) -> Result<bool> {
    let oid2_str = rest.trim();
    if oid2_str.is_empty() {
        bail!("stdin two-tree format requires a second OID after the first");
    }
    let oid2 = oid2_str
        .parse::<ObjectId>()
        .with_context(|| format!("invalid OID: `{oid2_str}`"))?;

    // Print both tree OIDs.
    writeln!(out, "{} {}", oid1.to_hex(), oid2.to_hex())?;

    let entries = diff_with_opts(&repo.odb, Some(oid1), Some(&oid2), opts)?;
    let filtered = filter_entries(entries, opts);
    print_diff(out, &repo.odb, &filtered, opts, None)
}

// ── Diff helpers ─────────────────────────────────────────────────────

/// Compute the diff, recursing into sub-trees only when `recursive` is set.
fn diff_with_opts(
    odb: &Odb,
    old_tree: Option<&ObjectId>,
    new_tree: Option<&ObjectId>,
    opts: &Options,
) -> Result<Vec<DiffEntry>> {
    if opts.max_depth.is_some() {
        // Always do full recursion; max_depth is applied as a post-filter
        // after pathspec filtering (depth is relative to pathspec root).
        return diff_trees(odb, old_tree, new_tree, "").map_err(Into::into);
    }
    if opts.recursive {
        if opts.show_trees {
            diff_trees_show_tree_entries(odb, old_tree, new_tree, "").map_err(Into::into)
        } else {
            diff_trees(odb, old_tree, new_tree, "").map_err(Into::into)
        }
    } else {
        diff_trees_toplevel(odb, old_tree, new_tree)
    }
}

/// Apply max-depth filtering: collapse entries deeper than `max_depth` levels
/// relative to the deepest matching pathspec prefix.
fn filter_max_depth(
    entries: Vec<DiffEntry>,
    max_depth: i32,
    pathspecs: &[String],
) -> Vec<DiffEntry> {
    if max_depth < 0 {
        return entries; // unlimited
    }
    let max_depth = max_depth as usize;

    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::new();
    for entry in entries {
        let path = entry.path();
        let components: Vec<&str> = path.split('/').collect();
        let match_prefix_depth = if pathspecs.is_empty() {
            0usize
        } else {
            pathspecs
                .iter()
                .filter_map(|spec| {
                    let prefix = spec.strip_suffix('/').unwrap_or(spec);
                    if path == prefix || path.starts_with(&format!("{prefix}/")) {
                        Some(if prefix.is_empty() {
                            0
                        } else {
                            prefix.split('/').count()
                        })
                    } else {
                        None
                    }
                })
                .max()
                .unwrap_or(0)
        };

        let allowed_components = if match_prefix_depth > 0 {
            match_prefix_depth + max_depth
        } else {
            max_depth + 1
        };

        if components.len() <= allowed_components {
            result.push(entry);
        } else {
            // Truncate to allowed_components
            let truncated: String = components[..allowed_components].join("/");
            if seen.insert(truncated.clone()) {
                let mut collapsed = entry;
                collapsed.old_path = Some(truncated.clone());
                collapsed.new_path = Some(truncated);
                result.push(collapsed);
            }
        }
    }
    result
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
                            score: None,
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
                            score: None,
                        });
                        ni += 1;
                    }
                    std::cmp::Ordering::Equal => {
                        if o.oid != n.oid || o.mode != n.mode {
                            let path = o_name.into_owned();
                            // A mode-only change (e.g. chmod) is Modified, not TypeChanged.
                            // TypeChanged is only for actual type changes (blob ↔ symlink etc.)
                            let old_type = o.mode & 0o170000;
                            let new_type = n.mode & 0o170000;
                            let status = if old_type != new_type {
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
                                score: None,
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
                    score: None,
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
                    score: None,
                });
                ni += 1;
            }
            (None, None) => break,
        }
    }

    Ok(result)
}

// ── Output ───────────────────────────────────────────────────────────

/// Recursively collect all blob entries from a tree, returning (oid, path, mode).
fn collect_tree_blobs_recursive(
    odb: &Odb,
    tree_oid: &ObjectId,
    prefix: &str,
) -> Result<Vec<(ObjectId, String, String)>> {
    let obj = odb.read(tree_oid)?;
    let tree = parse_tree(&obj.data)?;
    let mut result = Vec::new();
    for entry in tree {
        let name = String::from_utf8_lossy(&entry.name).into_owned();
        let path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{}/{}", prefix, name)
        };
        if entry.mode == 0o040000 {
            // Subtree — recurse.
            if let Ok(sub) = collect_tree_blobs_recursive(odb, &entry.oid, &path) {
                result.extend(sub);
            }
        } else {
            result.push((entry.oid, path, format!("{:06o}", entry.mode)));
        }
    }
    Ok(result)
}

/// Detect copies among added entries by checking if their blob matches
/// any existing (unchanged or modified) entry in the old tree.
///
/// `old_tree_entries` provides all blobs from the old tree for
/// `--find-copies-harder`.  When `harder` is false, only entries in
/// the diff itself are considered as sources.
fn detect_copies(
    _odb: &grit_lib::odb::Odb,
    entries: Vec<DiffEntry>,
    threshold: u32,
    harder: bool,
    old_tree_entries: &[(ObjectId, String, String)], // (oid, path, mode)
) -> Vec<DiffEntry> {
    use std::collections::HashMap;

    // Build source map: blob OID → (path, mode).
    let mut sources: HashMap<ObjectId, (String, String)> = HashMap::new();

    if harder {
        // Use all old-tree blobs as potential copy sources.
        for (oid, path, mode) in old_tree_entries {
            sources
                .entry(*oid)
                .or_insert_with(|| (path.clone(), mode.clone()));
        }
    }

    // Also add modified entries from the diff as sources.
    for entry in &entries {
        match entry.status {
            DiffStatus::Added | DiffStatus::Deleted => {}
            _ => {
                if entry.old_oid.as_bytes() != &[0u8; 20] {
                    if let Some(ref p) = entry.old_path {
                        sources
                            .entry(entry.old_oid)
                            .or_insert_with(|| (p.clone(), entry.old_mode.clone()));
                    }
                }
            }
        }
    }

    let mut result = Vec::with_capacity(entries.len());
    for entry in entries {
        if entry.status == DiffStatus::Added {
            if let Some((src_path, src_mode)) = sources.get(&entry.new_oid) {
                // Exact OID match → 100% copy.
                if 100 >= threshold {
                    result.push(DiffEntry {
                        old_path: Some(src_path.clone()),
                        new_path: entry.new_path,
                        old_oid: entry.new_oid,
                        new_oid: entry.new_oid,
                        old_mode: src_mode.clone(),
                        new_mode: entry.new_mode,
                        status: DiffStatus::Copied,
                        score: Some(100),
                    });
                    continue;
                }
            }
        }
        result.push(entry);
    }
    result
}

/// Print the diff entries according to `opts.format`.
fn print_diff(
    out: &mut impl Write,
    odb: &Odb,
    entries: &[DiffEntry],
    opts: &Options,
    old_tree_oid: Option<&ObjectId>,
) -> Result<bool> {
    // Apply rename detection if requested.
    let owned_entries;
    let old_blobs = if opts.find_copies.is_some() && opts.find_copies_harder {
        if let Some(tree_oid) = old_tree_oid {
            collect_tree_blobs_recursive(odb, tree_oid, "").unwrap_or_default()
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };
    let entries = if let Some(threshold) = opts.find_renames {
        let mut result = detect_renames(odb, entries.to_vec(), threshold);
        if let Some(copy_threshold) = opts.find_copies {
            result = detect_copies(
                odb,
                result,
                copy_threshold,
                opts.find_copies_harder,
                &old_blobs,
            );
        }
        owned_entries = result;
        &owned_entries[..]
    } else if let Some(copy_threshold) = opts.find_copies {
        owned_entries = detect_copies(
            odb,
            entries.to_vec(),
            copy_threshold,
            opts.find_copies_harder,
            &old_blobs,
        );
        &owned_entries[..]
    } else {
        entries
    };

    match opts.format {
        OutputFormat::Raw => {
            // When --pretty is set AND --summary or --stat is also set, suppress raw output.
            // Otherwise show raw output normally.
            let suppress_raw = opts.pretty.is_some() && opts.summary;
            if !suppress_raw {
                for entry in entries {
                    if let Some(abbrev_len) = opts.abbrev {
                        writeln!(out, "{}", format_raw_abbrev(entry, abbrev_len))?;
                    } else {
                        writeln!(out, "{}", format_raw(entry))?;
                    }
                }
            }
            if opts.summary {
                write_summary(out, entries)?;
            }
        }
        OutputFormat::Patch => {
            // --patch-with-stat: show stat before patch
            if opts.patch_with_stat {
                print_stat_summary(out, odb, entries)?;
                writeln!(out)?;
            }
            // --patch-with-raw: show raw before patch
            if opts.patch_with_raw {
                for entry in entries {
                    if let Some(abbrev_len) = opts.abbrev {
                        writeln!(out, "{}", format_raw_abbrev(entry, abbrev_len))?;
                    } else {
                        writeln!(out, "{}", format_raw(entry))?;
                    }
                }
                writeln!(out)?;
            }
            for entry in entries {
                write_patch_entry(
                    out,
                    odb,
                    entry,
                    opts.context_lines,
                    opts.abbrev,
                    opts.full_index,
                )?;
            }
        }
        OutputFormat::Stat => {
            print_stat_summary(out, odb, entries)?;
            if opts.summary {
                write_summary(out, entries)?;
            }
        }
        OutputFormat::NameOnly => {
            for entry in entries {
                writeln!(out, "{}", entry.path())?;
            }
        }
        OutputFormat::NameStatus => {
            for entry in entries {
                match (entry.status, entry.score) {
                    (DiffStatus::Renamed, Some(s)) => {
                        writeln!(
                            out,
                            "R{:03}\t{}\t{}",
                            s,
                            entry.old_path.as_deref().unwrap_or(""),
                            entry.new_path.as_deref().unwrap_or("")
                        )?;
                    }
                    (DiffStatus::Copied, Some(s)) => {
                        writeln!(
                            out,
                            "C{:03}\t{}\t{}",
                            s,
                            entry.old_path.as_deref().unwrap_or(""),
                            entry.new_path.as_deref().unwrap_or("")
                        )?;
                    }
                    _ => {
                        writeln!(out, "{}\t{}", entry.status.letter(), entry.path())?;
                    }
                }
            }
        }
    }
    Ok(false)
}

/// Abbreviate an OID hex string to the given length.
fn abbrev_oid(hex: &str, abbrev: Option<usize>, full_index: bool) -> &str {
    if full_index {
        hex
    } else {
        let len = abbrev.unwrap_or(7).min(hex.len());
        &hex[..len]
    }
}

/// Write human-readable `--summary` lines (create mode, delete mode, mode change, etc.)
fn write_summary(out: &mut impl Write, entries: &[DiffEntry]) -> Result<()> {
    for entry in entries {
        match entry.status {
            DiffStatus::Added => {
                writeln!(out, " create mode {} {}", entry.new_mode, entry.path())?;
            }
            DiffStatus::Deleted => {
                writeln!(out, " delete mode {} {}", entry.old_mode, entry.path())?;
            }
            DiffStatus::Modified if entry.old_mode != entry.new_mode => {
                writeln!(
                    out,
                    " mode change {} => {} {}",
                    entry.old_mode,
                    entry.new_mode,
                    entry.path()
                )?;
            }
            DiffStatus::TypeChanged => {
                writeln!(
                    out,
                    " mode change {} => {} {}",
                    entry.old_mode,
                    entry.new_mode,
                    entry.path()
                )?;
            }
            DiffStatus::Renamed => {
                let sim = entry.score.unwrap_or(100);
                writeln!(
                    out,
                    " rename {} => {} ({sim}%)",
                    entry.old_path.as_deref().unwrap_or(""),
                    entry.new_path.as_deref().unwrap_or("")
                )?;
            }
            DiffStatus::Copied => {
                let sim = entry.score.unwrap_or(100);
                writeln!(
                    out,
                    " copy {} => {} ({sim}%)",
                    entry.old_path.as_deref().unwrap_or(""),
                    entry.new_path.as_deref().unwrap_or("")
                )?;
            }
            _ => {}
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
    abbrev: Option<usize>,
    full_index: bool,
) -> Result<bool> {
    let old_path = entry
        .old_path
        .as_deref()
        .unwrap_or(entry.new_path.as_deref().unwrap_or(""));
    let new_path = entry
        .new_path
        .as_deref()
        .unwrap_or(entry.old_path.as_deref().unwrap_or(""));

    let old_hex = entry.old_oid.to_hex();
    let new_hex = entry.new_oid.to_hex();
    let old_abbrev = abbrev_oid(&old_hex, abbrev, full_index);
    let new_abbrev = abbrev_oid(&new_hex, abbrev, full_index);

    writeln!(out, "diff --git a/{old_path} b/{new_path}")?;

    match entry.status {
        DiffStatus::Added => {
            writeln!(out, "new file mode {}", entry.new_mode)?;
            writeln!(out, "index {old_abbrev}..{new_abbrev}")?;
        }
        DiffStatus::Deleted => {
            writeln!(out, "deleted file mode {}", entry.old_mode)?;
            writeln!(out, "index {old_abbrev}..{new_abbrev}")?;
        }
        DiffStatus::Modified => {
            if entry.old_mode != entry.new_mode {
                writeln!(out, "old mode {}", entry.old_mode)?;
                writeln!(out, "new mode {}", entry.new_mode)?;
            }
            if entry.old_mode == entry.new_mode {
                writeln!(out, "index {old_abbrev}..{new_abbrev} {}", entry.old_mode)?;
            } else {
                writeln!(out, "index {old_abbrev}..{new_abbrev}")?;
            }
        }
        DiffStatus::Renamed => {
            let sim = entry.score.unwrap_or(100);
            writeln!(out, "similarity index {sim}%")?;
            writeln!(out, "rename from {old_path}")?;
            writeln!(out, "rename to {new_path}")?;
            if entry.old_oid != entry.new_oid {
                writeln!(out, "index {old_abbrev}..{new_abbrev}")?;
            }
        }
        DiffStatus::Copied => {
            let sim = entry.score.unwrap_or(100);
            writeln!(out, "similarity index {sim}%")?;
            writeln!(out, "copy from {old_path}")?;
            writeln!(out, "copy to {new_path}")?;
            if entry.old_oid != entry.new_oid {
                writeln!(out, "index {old_abbrev}..{new_abbrev}")?;
            }
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

    Ok(false)
}

/// Write a `--stat` summary.
fn print_stat_summary(out: &mut impl Write, odb: &Odb, entries: &[DiffEntry]) -> Result<bool> {
    use grit_lib::diff::format_stat_line_width;

    let max_path_len = entries.iter().map(|e| e.path().len()).max().unwrap_or(0);
    let mut total_ins = 0usize;
    let mut total_del = 0usize;

    // First pass: compute all stats
    let mut file_stats: Vec<(&str, usize, usize)> = Vec::new();
    for entry in entries {
        let (old_content, new_content) = read_blob_pair(odb, entry)?;
        let (ins, del) = count_changes(&old_content, &new_content);
        total_ins += ins;
        total_del += del;
        file_stats.push((entry.path(), ins, del));
    }

    // Compute count width based on max total change
    let max_count = file_stats.iter().map(|(_, i, d)| i + d).max().unwrap_or(0);
    let count_width = format!("{}", max_count).len();

    for (path, ins, del) in &file_stats {
        writeln!(
            out,
            "{}",
            format_stat_line_width(path, *ins, *del, max_path_len, count_width)
        )?;
    }

    let n = entries.len();
    let mut summary = format!(" {} file{} changed", n, if n == 1 { "" } else { "s" },);
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
    if total_ins == 0 && total_del == 0 {
        summary.push_str(", 0 insertions(+), 0 deletions(-)");
    }
    writeln!(out, "{summary}")?;

    Ok(false)
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
/// Write a commit header line. If `pretty` is set, write a full "medium" format
/// header; otherwise just write the OID.
fn write_commit_header(
    out: &mut impl Write,
    oid: &ObjectId,
    commit_data: &[u8],
    opts: &Options,
) -> Result<bool> {
    if let Some(ref pretty_fmt) = opts.pretty {
        let commit = parse_commit(commit_data).context("parsing commit for pretty")?;
        if pretty_fmt == "oneline" {
            let first_line = commit.message.lines().next().unwrap_or("");
            writeln!(out, "{oid} {first_line}")?;
            return Ok(false);
        }
        writeln!(out, "commit {oid}")?;
        // Parse author line: "Name <email> timestamp tz"
        let author = &commit.author;
        if let Some(date_start) = author.rfind('>') {
            let name_email = &author[..=date_start];
            let timestamp_tz = author[date_start + 1..].trim();
            writeln!(out, "Author: {name_email}")?;
            // Try to parse the date
            if let Some(formatted) = format_author_date(timestamp_tz) {
                writeln!(out, "Date:   {formatted}")?;
            }
        } else {
            writeln!(out, "Author: {author}")?;
        }
        writeln!(out)?;
        // Indent commit message
        for line in commit.message.lines() {
            writeln!(out, "    {line}")?;
        }
        // Use "---" separator when --patch-with-stat is active, blank line otherwise
        if opts.patch_with_stat {
            writeln!(out, "---")?;
        } else {
            writeln!(out)?;
        }
    } else if !opts.no_commit_id {
        writeln!(out, "{oid}")?;
    }
    Ok(false)
}

/// Format a Unix timestamp + tz offset into git's default date format.
fn format_commit_date(timestamp: i64, tz: &str) -> String {
    use time::OffsetDateTime;
    let tz_offset_secs = parse_tz_offset_secs(tz);
    if let Ok(offset) = time::UtcOffset::from_whole_seconds(tz_offset_secs) {
        if let Ok(dt) = OffsetDateTime::from_unix_timestamp(timestamp) {
            let dt = dt.to_offset(offset);
            let weekday = match dt.weekday() {
                time::Weekday::Monday => "Mon",
                time::Weekday::Tuesday => "Tue",
                time::Weekday::Wednesday => "Wed",
                time::Weekday::Thursday => "Thu",
                time::Weekday::Friday => "Fri",
                time::Weekday::Saturday => "Sat",
                time::Weekday::Sunday => "Sun",
            };
            let month = match dt.month() {
                time::Month::January => "Jan",
                time::Month::February => "Feb",
                time::Month::March => "Mar",
                time::Month::April => "Apr",
                time::Month::May => "May",
                time::Month::June => "Jun",
                time::Month::July => "Jul",
                time::Month::August => "Aug",
                time::Month::September => "Sep",
                time::Month::October => "Oct",
                time::Month::November => "Nov",
                time::Month::December => "Dec",
            };
            let sign = if tz_offset_secs < 0 { '-' } else { '+' };
            let abs = tz_offset_secs.unsigned_abs();
            let h = abs / 3600;
            let m = (abs % 3600) / 60;
            return format!(
                "{} {} {:2} {:02}:{:02}:{:02} {:4} {}{:02}{:02}",
                weekday,
                month,
                dt.day(),
                dt.hour(),
                dt.minute(),
                dt.second(),
                dt.year(),
                sign,
                h,
                m
            );
        }
    }
    format!("{timestamp} {tz}")
}

/// Parse an author date field and format it for pretty printing.
/// Handles both "<unix_ts> <tz>" and "YYYY-MM-DD HH:MM:SS <tz>" formats.
fn format_author_date(date_str: &str) -> Option<String> {
    if date_str.is_empty() {
        return None;
    }
    // Try "<unix_ts> <tz>" first
    let parts: Vec<&str> = date_str.splitn(2, ' ').collect();
    if parts.len() == 2 {
        if let Ok(ts) = parts[0].parse::<i64>() {
            return Some(format_commit_date(ts, parts[1]));
        }
    }
    // Try "YYYY-MM-DD HH:MM:SS <tz>" format
    // Split from the end to find the timezone
    let parts: Vec<&str> = date_str.rsplitn(2, ' ').collect();
    if parts.len() == 2 {
        let tz = parts[0];
        let datetime = parts[1];
        // Try to parse as ISO-ish datetime
        let tz_secs = parse_tz_offset_secs(tz);
        if let Ok(offset) = time::UtcOffset::from_whole_seconds(tz_secs) {
            // Try YYYY-MM-DD HH:MM:SS
            let ymd_hms =
                time::format_description::parse("[year]-[month]-[day] [hour]:[minute]:[second]")
                    .ok()?;
            if let Ok(naive) = time::PrimitiveDateTime::parse(datetime, &ymd_hms) {
                let dt = naive.assume_offset(offset);
                let ts = dt.unix_timestamp();
                return Some(format_commit_date(ts, tz));
            }
        }
    }
    // Fallback: just return the raw string
    Some(date_str.to_owned())
}

fn parse_tz_offset_secs(tz: &str) -> i32 {
    if tz.len() < 4 {
        return 0;
    }
    let (sign, rest) = if tz.starts_with('+') {
        (1i32, &tz[1..])
    } else if tz.starts_with('-') {
        (-1i32, &tz[1..])
    } else {
        (1i32, tz)
    };
    let hours: i32 = rest.get(..2).and_then(|s| s.parse().ok()).unwrap_or(0);
    let mins: i32 = rest.get(2..4).and_then(|s| s.parse().ok()).unwrap_or(0);
    sign * (hours * 3600 + mins * 60)
}

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

/// Apply all post-diff filters: pathspecs and max-depth.
fn filter_entries(entries: Vec<DiffEntry>, opts: &Options) -> Vec<DiffEntry> {
    let filtered = filter_pathspecs(entries, &opts.pathspecs);
    if let Some(depth) = opts.max_depth {
        filter_max_depth(filtered, depth, &opts.pathspecs)
    } else {
        filtered
    }
}

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
