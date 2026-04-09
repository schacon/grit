//! `grit diff-tree` — compare the content and mode of blobs found via two tree objects.
//!
//! # Modes
//!
//! - Two tree-ish arguments: compare the trees directly.
//! - One commit argument: compare the commit's tree against its parent(s).
//! - `--stdin`: read commit or tree-pair OIDs from standard input.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use encoding_rs::Encoding;
use grit_lib::combined_tree_diff::{
    combined_diff_paths_filtered, CombinedDiffPath, CombinedParentStatus, CombinedTreeDiffOptions,
};
use grit_lib::config::ConfigSet;
use grit_lib::diff::{
    count_changes, detect_renames, diff_trees, diff_trees_show_tree_entries, format_raw,
    format_raw_abbrev, unified_diff, DiffEntry, DiffStatus,
};
use grit_lib::merge_base::merge_bases_first_vs_rest;
use grit_lib::merge_diff::{
    combined_diff_paths, format_combined_textconv_patch, is_binary_for_diff,
};
use grit_lib::objects::{parse_commit, parse_tree, ObjectId, ObjectKind};
use grit_lib::odb::Odb;
use grit_lib::pathspec::{context_from_mode_octal, matches_pathspec_with_context};
use grit_lib::repo::{resolve_dot_git, Repository};
use grit_lib::rev_parse::resolve_revision;
use regex::Regex;
use std::io::{self, BufRead, Write};
use std::path::Path;

/// Default maximum tree recursion depth when `core.maxtreedepth` is unset.
const DEFAULT_MAX_TREE_DEPTH: usize = 2048;

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
    /// Output binary patches (--binary).
    binary: bool,
    /// Show diffs for merge commits in stdin mode (`-m`).
    show_merges: bool,
    /// Combined diff for merge commits (`-c` / `--cc`, plumbing: no textconv).
    combined_patch: bool,
    /// Use `diff --cc` instead of `diff --combined` in combined mode.
    combined_use_cc_word: bool,
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
    /// Re-merge parents and diff against merge result tree.
    remerge_diff: bool,
    /// Swap the two tree sides (`-R`), inverting raw/patch output like Git.
    reverse: bool,
    /// Pickaxe: only entries where `-S` string occurrence count changes between blobs.
    pickaxe_string: Option<String>,
    /// Pickaxe: only entries whose diff has added/removed lines matching `-G` regex.
    pickaxe_grep: Option<String>,
    /// Treat `-S` pattern as a regex (count regex matches per side).
    pickaxe_regex: bool,
    /// Show all matching files when using pickaxe, not only those with count changes (`--pickaxe-all`).
    pickaxe_all: bool,
    /// Submodule diff format (`log` shows one-line summaries for gitlinks, like Git's `diff --submodule=log`).
    submodule_mode: Option<String>,
    /// Object id spec for `--find-object` (resolved against the repo before the walk).
    find_object: Option<String>,
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
            binary: false,
            show_merges: false,
            combined_patch: false,
            combined_use_cc_word: false,
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
            remerge_diff: false,
            reverse: false,
            pickaxe_string: None,
            pickaxe_grep: None,
            pickaxe_regex: false,
            pickaxe_all: false,
            submodule_mode: None,
            find_object: None,
        }
    }
}

/// True when `spec` resolves to a commit, tree, or annotated tag (Git `setup_revisions` tree-ish).
fn spec_names_commit_or_tree(repo: &Repository, spec: &str) -> bool {
    match resolve_revision(repo, spec) {
        Ok(oid) => match repo.odb.read(&oid) {
            Ok(obj) => match obj.kind {
                ObjectKind::Commit | ObjectKind::Tree => true,
                ObjectKind::Tag => true,
                ObjectKind::Blob => false,
            },
            Err(_) => false,
        },
        Err(_) => false,
    }
}

/// Parse the raw argument vector.
fn parse_options(repo: &Repository, argv: &[String]) -> Result<Options> {
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
                "-c" => opts.combined_patch = true,
                "--cc" => {
                    opts.combined_patch = true;
                    opts.combined_use_cc_word = true;
                }
                "--raw" => opts.format = OutputFormat::Raw,
                "-p" | "-u" | "--patch" => opts.format = OutputFormat::Patch,
                "--binary" => {
                    opts.format = OutputFormat::Patch;
                    opts.binary = true;
                }
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
                "--remerge-diff" => opts.remerge_diff = true,
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
                "--no-abbrev" => opts.abbrev = Some(40),
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
                "--no-rename-empty" | "--always" | "--diff-merges=off" | "--check" => {}
                "-R" => opts.reverse = true,
                _ if arg.len() > 2 && arg.starts_with("-R") => {
                    opts.reverse = true;
                    let rest = arg[2..].chars();
                    for ch in rest {
                        match ch {
                            'p' | 'u' => opts.format = OutputFormat::Patch,
                            _ => bail!("unknown option: -{ch}"),
                        }
                    }
                }
                _ if arg.starts_with("--find-object=") => {
                    opts.find_object = Some(arg["--find-object=".len()..].to_string());
                }
                "--find-object" => {
                    i += 1;
                    let next = argv
                        .get(i)
                        .ok_or_else(|| anyhow::anyhow!("`--find-object` requires a value"))?;
                    opts.find_object = Some(next.clone());
                }
                _ if arg.starts_with("--format=") => {
                    let val = &arg["--format=".len()..];
                    opts.pretty = Some(format!("format:{val}"));
                }
                _ if arg.starts_with("--diff-filter=")
                    || arg.starts_with("--diff-merges=")
                    || arg.starts_with("-O")
                    || arg.starts_with("--relative") =>
                {
                    // ignored
                }
                "--pickaxe-regex" => opts.pickaxe_regex = true,
                "--pickaxe-all" => opts.pickaxe_all = true,
                s if s.starts_with("-S") => {
                    if s.len() > 2 {
                        opts.pickaxe_string = Some(s[2..].to_owned());
                    } else {
                        i += 1;
                        if i >= argv.len() {
                            bail!("option `-S` requires a value");
                        }
                        opts.pickaxe_string = Some(argv[i].clone());
                    }
                    i += 1;
                    continue;
                }
                s if s.starts_with("-G") => {
                    if s.len() > 2 {
                        opts.pickaxe_grep = Some(s[2..].to_owned());
                    } else {
                        i += 1;
                        if i >= argv.len() {
                            bail!("option `-G` requires a value");
                        }
                        opts.pickaxe_grep = Some(argv[i].clone());
                    }
                    i += 1;
                    continue;
                }
                _ if arg.starts_with("--submodule=") => {
                    opts.submodule_mode = Some(arg["--submodule=".len()..].to_string());
                }
                _ => bail!("unknown option: {arg}"),
            }
            i += 1;
            continue;
        }

        // Positional: like Git `setup_revisions` — up to two tree-ishes, then pathspecs.
        if end_of_options || opts.objects.len() >= 2 {
            opts.pathspecs.push(arg.clone());
        } else if opts.objects.len() == 1 && !spec_names_commit_or_tree(repo, arg) {
            opts.pathspecs.push(arg.clone());
        } else {
            opts.objects.push(arg.clone());
        }
        i += 1;
    }

    // Patch and stat imply recursion (Git shows nested file paths). `--name-only`
    // and `--name-status` follow plain `diff-tree` rules: top-level entries only
    // unless `-r` is given (see t4010-diff-pathspec).
    match opts.format {
        OutputFormat::Patch | OutputFormat::Stat => {
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
pub fn run(mut args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    if grit_lib::precompose_config::effective_core_precomposeunicode(Some(&repo.git_dir)) {
        crate::precompose::precompose_diff_tree_argv(&mut args.args);
    }
    let opts = parse_options(&repo, &args.args)?;

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
    let (spec_a, spec_b) = if opts.reverse {
        (&opts.objects[1], &opts.objects[0])
    } else {
        (&opts.objects[0], &opts.objects[1])
    };
    let oid1 = resolve_to_tree(repo, spec_a)?;
    let oid2 = resolve_to_tree(repo, spec_b)?;
    let max_tree_depth = resolve_max_tree_depth(repo)?;
    let old_tree = if is_magic_empty_tree_oid(&oid1) {
        None
    } else {
        Some(&oid1)
    };
    let new_tree = if is_magic_empty_tree_oid(&oid2) {
        None
    } else {
        Some(&oid2)
    };
    if let Some(tree_oid) = old_tree {
        validate_tree_depth_limit(&repo.odb, tree_oid, 0, max_tree_depth)?;
    }
    if let Some(tree_oid) = new_tree {
        validate_tree_depth_limit(&repo.odb, tree_oid, 0, max_tree_depth)?;
    }
    let entries = diff_with_opts(&repo.odb, old_tree, new_tree, opts)?;
    let filtered = filter_entries(&repo.odb, &repo, entries, opts)?;
    let has_diff = !filtered.is_empty();
    if !opts.quiet {
        print_diff(
            out,
            &repo.odb,
            &filtered,
            opts,
            old_tree,
            &repo.git_dir,
            repo.work_tree.as_deref(),
        )?;
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
            let max_tree_depth = resolve_max_tree_depth(repo)?;
            validate_tree_depth_limit(&repo.odb, &commit.tree, 0, max_tree_depth)?;
            if commit.parents.is_empty() {
                if opts.root {
                    let entries = diff_with_opts(&repo.odb, None, Some(&commit.tree), opts)?;
                    let filtered = filter_entries(&repo.odb, &repo, entries, opts)?;
                    has_diff = !filtered.is_empty();
                    if !opts.quiet && (has_diff || opts.pretty.is_some()) {
                        write_commit_header(out, &oid, &obj.data, opts)?;
                        print_diff(
                            out,
                            &repo.odb,
                            &filtered,
                            opts,
                            None,
                            &repo.git_dir,
                            repo.work_tree.as_deref(),
                        )?;
                    }
                }
            } else if commit.parents.len() == 2
                && opts.remerge_diff
                && opts.format == OutputFormat::Patch
            {
                use crate::commands::remerge_diff::{write_remerge_diff, RemergeDiffOptions};
                let ro = RemergeDiffOptions {
                    pathspecs: &opts.pathspecs,
                    diff_filter: None,
                    pickaxe: None,
                    find_object: None,
                    submodule_mode: None,
                    context_lines: opts.context_lines,
                };
                let mut buf = Vec::new();
                write_remerge_diff(&mut buf, repo, &commit.tree, &commit.parents, &ro)?;
                let hd = !buf.is_empty();
                has_diff = hd;
                if !opts.quiet && (hd || opts.pretty.is_some()) {
                    write_commit_header(out, &oid, &obj.data, opts)?;
                    out.write_all(&buf)?;
                }
            } else if commit.parents.len() > 1
                && opts.combined_patch
                && matches!(
                    opts.format,
                    OutputFormat::NameStatus | OutputFormat::NameOnly
                )
            {
                let find_oid = if let Some(ref spec) = opts.find_object {
                    Some(
                        resolve_revision(repo, spec)
                            .with_context(|| format!("unable to resolve '{spec}'"))?,
                    )
                } else {
                    None
                };
                let walk = CombinedTreeDiffOptions {
                    recursive: opts.recursive,
                    tree_in_recursive: find_oid.is_some(),
                };
                let paths = combined_diff_paths_filtered(
                    &repo.odb,
                    &commit.tree,
                    &commit.parents,
                    &walk,
                    find_oid.as_ref(),
                )?;
                has_diff = !paths.is_empty();
                if !opts.quiet && (has_diff || opts.pretty.is_some()) {
                    write_commit_header(out, &oid, &obj.data, opts)?;
                    if has_diff {
                        print_combined_paths(out, &paths, opts)?;
                    }
                }
            } else if commit.parents.len() > 1
                && opts.combined_patch
                && opts.format == OutputFormat::Patch
            {
                let config = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
                let abbrev_len = if opts.full_index {
                    40usize
                } else {
                    opts.abbrev.unwrap_or(7)
                };
                let paths = combined_diff_paths(&repo.odb, &commit.tree, &commit.parents);
                has_diff = !paths.is_empty();
                if !opts.quiet && (has_diff || opts.pretty.is_some()) {
                    write_commit_header(out, &oid, &obj.data, opts)?;
                    let mut parent_trees = Vec::with_capacity(commit.parents.len());
                    for p in &commit.parents {
                        parent_trees.push(commit_tree(&repo.odb, p)?);
                    }
                    if parent_trees.len() == 2 {
                        for path in paths {
                            if let Some(patch) = format_combined_textconv_patch(
                                &repo.git_dir,
                                &config,
                                &repo.odb,
                                &path,
                                &parent_trees,
                                &commit.tree,
                                abbrev_len,
                                opts.context_lines,
                                opts.combined_use_cc_word,
                                false,
                            ) {
                                write!(out, "{patch}")?;
                            }
                        }
                    }
                }
            } else {
                let parent_tree = commit_tree(&repo.odb, &commit.parents[0])?;
                let entries =
                    diff_with_opts(&repo.odb, Some(&parent_tree), Some(&commit.tree), opts)?;
                let filtered = filter_entries(&repo.odb, &repo, entries, opts)?;
                has_diff = !filtered.is_empty();
                if !opts.quiet && (has_diff || opts.pretty.is_some()) {
                    write_commit_header(out, &oid, &obj.data, opts)?;
                    print_diff(
                        out,
                        &repo.odb,
                        &filtered,
                        opts,
                        Some(&parent_tree),
                        &repo.git_dir,
                        repo.work_tree.as_deref(),
                    )?;
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

    // Print commit-id header (unless `--no-commit-id`). `--quiet` still prints this
    // line (only the raw/patch diff is suppressed), matching `git diff-tree`.
    if !opts.no_commit_id {
        writeln!(out, "{}", oid.to_hex())?;
    }

    // `-v` shows the commit message even with `--quiet` (raw diff and commit-id line stay off).
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

    // Skip merge commits unless -m or remerge-diff patch.
    let remerge_merge_stdin =
        commit.parents.len() == 2 && opts.remerge_diff && opts.format == OutputFormat::Patch;
    if commit.parents.len() > 1 && !opts.show_merges && !remerge_merge_stdin {
        return Ok(false);
    }

    // Override parents if the line contains extra OIDs.
    let extra_parents = parse_oid_list(rest)?;
    let parent_oids: Vec<ObjectId> = if extra_parents.is_empty() {
        commit.parents.clone()
    } else {
        extra_parents
    };

    let has_diff = if remerge_merge_stdin && parent_oids.len() == 2 {
        use crate::commands::remerge_diff::{write_remerge_diff, RemergeDiffOptions};
        let ro = RemergeDiffOptions {
            pathspecs: &opts.pathspecs,
            diff_filter: None,
            pickaxe: None,
            find_object: None,
            submodule_mode: None,
            context_lines: opts.context_lines,
        };
        let mut buf = Vec::new();
        write_remerge_diff(&mut buf, repo, &commit.tree, &parent_oids, &ro)?;
        let hd = !buf.is_empty();
        if !opts.quiet {
            out.write_all(&buf)?;
        }
        hd
    } else if parent_oids.is_empty() {
        if opts.root {
            let entries = diff_with_opts(&repo.odb, None, Some(&commit.tree), opts)?;
            let filtered = filter_entries(&repo.odb, &repo, entries, opts)?;
            let hd = !filtered.is_empty();
            if !opts.quiet {
                print_diff(
                    out,
                    &repo.odb,
                    &filtered,
                    opts,
                    None,
                    &repo.git_dir,
                    repo.work_tree.as_deref(),
                )?;
            }
            hd
        } else {
            false
        }
    } else {
        let parent_tree = commit_tree(&repo.odb, &parent_oids[0])?;
        let entries = diff_with_opts(&repo.odb, Some(&parent_tree), Some(&commit.tree), opts)?;
        let filtered = filter_entries(&repo.odb, &repo, entries, opts)?;
        let hd = !filtered.is_empty();
        if !opts.quiet {
            print_diff(
                out,
                &repo.odb,
                &filtered,
                opts,
                None,
                &repo.git_dir,
                repo.work_tree.as_deref(),
            )?;
        }
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

    if !opts.quiet {
        writeln!(out, "{} {}", oid1.to_hex(), oid2.to_hex())?;
    }

    let (old_side, new_side) = if opts.reverse {
        (Some(&oid2), Some(oid1))
    } else {
        (Some(oid1), Some(&oid2))
    };
    let entries = diff_with_opts(&repo.odb, old_side, new_side, opts)?;
    let filtered = filter_entries(&repo.odb, &repo, entries, opts)?;
    let has_diff = !filtered.is_empty();
    if !opts.quiet {
        print_diff(
            out,
            &repo.odb,
            &filtered,
            opts,
            None,
            &repo.git_dir,
            repo.work_tree.as_deref(),
        )?;
    }
    Ok(has_diff)
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

    // For each entry, find the matching pathspec and compute relative depth.
    // Depth 0 means the entry is directly in the pathspec root.
    let prefix_depth = if pathspecs.is_empty() {
        0usize
    } else {
        // Use the longest (most specific) matching prefix for each entry.
        // For simplicity, use the minimum prefix depth across all pathspecs.
        pathspecs
            .iter()
            .map(|p| {
                let p = p.strip_suffix('/').unwrap_or(p);
                if p.is_empty() {
                    0
                } else {
                    p.split('/').count()
                }
            })
            .min()
            .unwrap_or(0)
    };

    // Maximum number of path components allowed in output.
    let allowed_components = if prefix_depth > 0 {
        prefix_depth + max_depth
    } else {
        max_depth + 1
    };

    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::new();
    for entry in entries {
        let path = entry.path();
        let components: Vec<&str> = path.split('/').collect();

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

fn is_gitlink_mode(mode: &str) -> bool {
    mode == "160000"
}

/// For `submodule=log`, Git collapses a pure submodule path change (same gitlink OID) into a
/// single `(new submodule)` line at the new path — omit the paired delete at the old path.
fn preprocess_gitlink_renames_for_submodule_log(entries: Vec<DiffEntry>) -> Vec<DiffEntry> {
    let mut out = Vec::with_capacity(entries.len());
    let mut i = 0usize;
    while i < entries.len() {
        let e = &entries[i];
        if i + 1 < entries.len()
            && e.status == DiffStatus::Deleted
            && is_gitlink_mode(&e.old_mode)
            && entries[i + 1].status == DiffStatus::Added
            && is_gitlink_mode(&entries[i + 1].new_mode)
            && e.old_oid == entries[i + 1].new_oid
            && e.old_oid != grit_lib::diff::zero_oid()
        {
            out.push(entries[i + 1].clone());
            i += 2;
        } else {
            out.push(entries[i].clone());
            i += 1;
        }
    }
    out
}

/// Open the submodule object database for `path`, matching Git's `open_submodule`: prefer the
/// checked-out work tree (gitfile), else `.git/modules/<path>` when the work tree was removed.
fn open_submodule_repo(
    super_git_dir: &Path,
    work_tree: Option<&Path>,
    path: &str,
) -> Option<Repository> {
    if let Some(wt) = work_tree {
        let sub_wt = wt.join(path);
        let dot_git = sub_wt.join(".git");
        if dot_git.exists() {
            if let Ok(gd) = resolve_dot_git(&dot_git) {
                if let Ok(repo) = Repository::open(&gd, Some(&sub_wt)) {
                    return Some(repo);
                }
            }
        }
    }
    let modules_dir = super_git_dir.join("modules").join(path);
    if modules_dir.is_dir() {
        Repository::open(&modules_dir, None).ok()
    } else {
        None
    }
}

fn commit_exists_in_repo(repo: &Repository, oid: &ObjectId) -> bool {
    match repo.odb.read(oid) {
        Ok(obj) => obj.kind == ObjectKind::Commit,
        Err(_) => false,
    }
}

fn format_submodule_log_subject(commit: &grit_lib::objects::CommitData) -> String {
    let first_line = commit.message.lines().next().unwrap_or("").trim_end();
    let raw_body: &[u8] = commit
        .raw_message
        .as_deref()
        .unwrap_or(commit.message.as_bytes());
    if let Some(enc_name) = commit.encoding.as_deref() {
        if let Some(enc) = Encoding::for_label(enc_name.as_bytes()) {
            let (cow, _, _) = enc.decode(raw_body);
            let s = cow.lines().next().unwrap_or("").trim_end();
            if !s.is_empty() {
                return s.to_string();
            }
        }
    }
    first_line.to_string()
}

fn print_submodule_log_for_entry(
    out: &mut impl Write,
    super_git_dir: &Path,
    work_tree: Option<&Path>,
    entry: &DiffEntry,
    abbrev_len: usize,
) -> Result<()> {
    let zero = grit_lib::diff::zero_oid();
    let path = entry.path();
    let (one, two) = match entry.status {
        DiffStatus::Added => (zero, entry.new_oid),
        DiffStatus::Deleted => (entry.old_oid, zero),
        DiffStatus::Modified | DiffStatus::TypeChanged => (entry.old_oid, entry.new_oid),
        DiffStatus::Renamed | DiffStatus::Copied => (entry.old_oid, entry.new_oid),
        DiffStatus::Unmerged => return Ok(()),
    };

    let mut message: Option<&'static str> = None;
    if one == zero {
        message = Some("(new submodule)");
    } else if two == zero {
        message = Some("(submodule deleted)");
    }

    let sub_repo = open_submodule_repo(super_git_dir, work_tree, path);
    if sub_repo.is_none() && message.is_none() {
        message = Some("(commits not present)");
    }

    let left = if one != zero {
        sub_repo
            .as_ref()
            .filter(|r| commit_exists_in_repo(r, &one))
            .map(|_| one)
    } else {
        Some(one)
    };
    let right = if two != zero {
        sub_repo
            .as_ref()
            .filter(|r| commit_exists_in_repo(r, &two))
            .map(|_| two)
    } else {
        Some(two)
    };

    if sub_repo.is_some()
        && message.is_none()
        && ((one != zero && left.is_none()) || (two != zero && right.is_none()))
    {
        message = Some("(commits not present)");
    }

    let mut fast_forward = false;
    let mut fast_backward = false;
    if let (Some(ref sr), Some(l), Some(r)) = (&sub_repo, left, right) {
        if l != r && l != zero && r != zero {
            if let Ok(bases) = merge_bases_first_vs_rest(sr, l, &[r]) {
                if let Some(b) = bases.first() {
                    if *b == l {
                        fast_forward = true;
                    } else if *b == r {
                        fast_backward = true;
                    }
                }
            }
        }
    }

    if one == two {
        return Ok(());
    }

    let sep = if fast_forward || fast_backward {
        ".."
    } else {
        "..."
    };
    let one_hex = one.to_hex();
    let two_hex = two.to_hex();
    let a1 = abbrev_oid(&one_hex, Some(abbrev_len), false);
    let a2 = abbrev_oid(&two_hex, Some(abbrev_len), false);
    write!(out, "Submodule {path} {a1}{sep}{a2}")?;
    if let Some(m) = message {
        writeln!(out, " {m}")?;
    } else if fast_backward {
        writeln!(out, " (rewind):")?;
    } else {
        writeln!(out, ":")?;
    }

    if let (Some(sr), Some(l), Some(r)) = (sub_repo, left, right) {
        if l != zero && r != zero && l != r {
            let l_ancestor_of_r = merge_bases_first_vs_rest(&sr, l, &[r])
                .ok()
                .and_then(|b| b.first().copied())
                .is_some_and(|b| b == l);
            if l_ancestor_of_r {
                let mut cur = r;
                let mut logged: Vec<grit_lib::objects::CommitData> = Vec::new();
                while cur != l {
                    let obj = match sr.odb.read(&cur) {
                        Ok(o) => o,
                        Err(_) => break,
                    };
                    if obj.kind != ObjectKind::Commit {
                        break;
                    }
                    let data = match parse_commit(&obj.data) {
                        Ok(d) => d,
                        Err(_) => break,
                    };
                    let Some(parent) = data.parents.first().copied() else {
                        break;
                    };
                    logged.push(data);
                    cur = parent;
                }
                for data in logged {
                    let subj = format_submodule_log_subject(&data);
                    writeln!(out, "  > {subj}")?;
                }
            }
        }
    }

    Ok(())
}

/// Print combined merge paths (`-c` / `--cc` with `--name-status` / `--name-only`).
fn print_combined_paths(
    out: &mut impl Write,
    paths: &[CombinedDiffPath],
    opts: &Options,
) -> Result<()> {
    for p in paths {
        match opts.format {
            OutputFormat::NameOnly => {
                writeln!(out, "{}", p.path)?;
            }
            OutputFormat::NameStatus => {
                let letters: String = p
                    .parents
                    .iter()
                    .map(|side| match side.status {
                        CombinedParentStatus::Added => 'A',
                        CombinedParentStatus::Modified => 'M',
                        CombinedParentStatus::Deleted => 'D',
                    })
                    .collect();
                writeln!(out, "{letters}\t{}", p.path)?;
            }
            _ => {}
        }
    }
    Ok(())
}

/// Print the diff entries according to `opts.format`.
fn print_diff(
    out: &mut impl Write,
    odb: &Odb,
    entries: &[DiffEntry],
    opts: &Options,
    old_tree_oid: Option<&ObjectId>,
    git_dir: &Path,
    work_tree: Option<&Path>,
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

    let submodule_log = opts.format == OutputFormat::Patch
        && opts.submodule_mode.as_deref().is_some_and(|m| m == "log");

    let preprocessed_owned;
    let entries = if submodule_log {
        preprocessed_owned = preprocess_gitlink_renames_for_submodule_log(entries.to_vec());
        &preprocessed_owned[..]
    } else {
        entries
    };

    if submodule_log {
        let abbrev_len = if opts.full_index {
            40usize
        } else {
            opts.abbrev.unwrap_or(7)
        };
        for entry in entries {
            if is_gitlink_mode(&entry.old_mode) || is_gitlink_mode(&entry.new_mode) {
                print_submodule_log_for_entry(out, git_dir, work_tree, entry, abbrev_len)?;
            }
        }
        return Ok(false);
    }

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
                    git_dir,
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
    git_dir: &Path,
) -> Result<bool> {
    let (old_content, new_content) = read_blob_pair(odb, entry)?;

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
    let path_for_attrs = entry.path();
    let old_raw = old_content.as_bytes();
    let new_raw = new_content.as_bytes();
    if is_binary_for_diff(git_dir, path_for_attrs, old_raw)
        || is_binary_for_diff(git_dir, path_for_attrs, new_raw)
    {
        writeln!(out, "Binary files a/{new_path} and b/{new_path} differ")?;
        return Ok(false);
    }
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
    if spec == "4b825dc642cb6eb9a060e54bf899d69f7c6948d4"
        || spec == "4b825dc642cb6eb9a060e54bf8d69288fbee4904"
    {
        return ObjectId::from_hex("4b825dc642cb6eb9a060e54bf8d69288fbee4904").map_err(Into::into);
    }
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

fn is_magic_empty_tree_oid(oid: &ObjectId) -> bool {
    let hex = oid.to_hex();
    hex == "4b825dc642cb6eb9a060e54bf899d69f7c6948d4"
        || hex == "4b825dc642cb6eb9a060e54bf8d69288fbee4904"
}

fn resolve_max_tree_depth(repo: &Repository) -> Result<usize> {
    let config = grit_lib::config::ConfigSet::load(Some(&repo.git_dir), true)?;
    let depth = if let Some(raw) = config.get("core.maxtreedepth") {
        raw.parse::<usize>()
            .map_err(|_| anyhow::anyhow!("invalid core.maxtreedepth: '{raw}'"))?
    } else {
        DEFAULT_MAX_TREE_DEPTH
    };
    Ok(depth)
}

fn validate_tree_depth_limit(
    odb: &Odb,
    tree_oid: &ObjectId,
    depth: usize,
    max_tree_depth: usize,
) -> Result<()> {
    if depth > max_tree_depth {
        bail!(
            "tree depth {} exceeds core.maxtreedepth {}",
            depth,
            max_tree_depth
        );
    }

    let obj = odb
        .read(tree_oid)
        .context("reading tree for depth validation")?;
    let entries = parse_tree(&obj.data).context("parsing tree for depth validation")?;
    for entry in entries {
        if entry.mode == 0o040000 {
            validate_tree_depth_limit(odb, &entry.oid, depth + 1, max_tree_depth)?;
        }
    }
    Ok(())
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
        if let Some(template) = pretty_fmt
            .strip_prefix("tformat:")
            .or_else(|| pretty_fmt.strip_prefix("format:"))
        {
            if template == "%s" {
                let first_line = commit.message.lines().next().unwrap_or("");
                writeln!(out, "{first_line}")?;
                writeln!(out)?;
                return Ok(false);
            }
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
///
/// Fails with `unable to read <oid>` when a side stores the null OID but a real
/// blob mode (bogus tree entry), or when a non-null OID is missing from the ODB,
/// matching `git diff-tree` / `git diff-index` patch behavior.
fn read_blob_pair(odb: &Odb, entry: &DiffEntry) -> Result<(String, String)> {
    let zero = grit_lib::diff::zero_oid();

    let old_content = if entry.old_oid == zero {
        if entry.old_mode != "000000" {
            bail!("unable to read {}", zero.to_hex());
        }
        String::new()
    } else {
        let obj = odb
            .read(&entry.old_oid)
            .map_err(|_| anyhow::anyhow!("unable to read {}", entry.old_oid.to_hex()))?;
        String::from_utf8_lossy(&obj.data).into_owned()
    };

    let new_content = if entry.new_oid == zero {
        if entry.new_mode != "000000" {
            bail!("unable to read {}", zero.to_hex());
        }
        String::new()
    } else {
        let obj = odb
            .read(&entry.new_oid)
            .map_err(|_| anyhow::anyhow!("unable to read {}", entry.new_oid.to_hex()))?;
        String::from_utf8_lossy(&obj.data).into_owned()
    };

    Ok((old_content, new_content))
}

/// Apply post-diff filters: pathspecs, max-depth, and pickaxe (`-S` / `-G`).
fn filter_entries(
    odb: &Odb,
    repo: &Repository,
    entries: Vec<DiffEntry>,
    opts: &Options,
) -> Result<Vec<DiffEntry>> {
    let mut filtered = filter_pathspecs(entries, &opts.pathspecs);
    if let Some(depth) = opts.max_depth {
        filtered = filter_max_depth(filtered, depth, &opts.pathspecs);
    }
    let filtered = apply_pickaxe_filter(odb, filtered, opts)?;
    apply_find_object_filter(repo, filtered, opts)
}

/// Keep entries whose old or new blob OID matches `--find-object` (non-combined diffs).
fn apply_find_object_filter(
    repo: &Repository,
    entries: Vec<DiffEntry>,
    opts: &Options,
) -> Result<Vec<DiffEntry>> {
    let Some(ref spec) = opts.find_object else {
        return Ok(entries);
    };
    let oid =
        resolve_revision(repo, spec).with_context(|| format!("unable to resolve '{spec}'"))?;
    let filtered: Vec<DiffEntry> = entries
        .into_iter()
        .filter(|e| e.old_oid == oid || e.new_oid == oid)
        .collect();
    Ok(filtered)
}

/// Keep only diff entries that match `-G` / `-S` pickaxe rules (same semantics as `git diff`).
fn apply_pickaxe_filter(
    odb: &Odb,
    entries: Vec<DiffEntry>,
    opts: &Options,
) -> Result<Vec<DiffEntry>> {
    if let Some(ref pattern) = opts.pickaxe_grep {
        let re =
            Regex::new(pattern).with_context(|| format!("invalid pickaxe regex: {pattern}"))?;
        let mut out = Vec::new();
        for e in entries {
            let (old, new) = read_blob_pair(odb, &e)?;
            let mut keep = false;
            for line in new.lines() {
                if re.is_match(line) {
                    keep = true;
                    break;
                }
            }
            if !keep {
                for line in old.lines() {
                    if re.is_match(line) {
                        keep = true;
                        break;
                    }
                }
            }
            if keep {
                out.push(e);
            }
        }
        return Ok(out);
    }

    if let Some(ref needle) = opts.pickaxe_string {
        if opts.pickaxe_regex {
            let re =
                Regex::new(needle).with_context(|| format!("invalid pickaxe regex: {needle}"))?;
            let mut out = Vec::new();
            for e in entries {
                let (old, new) = read_blob_pair(odb, &e)?;
                let old_count = re.find_iter(&old).count();
                let new_count = re.find_iter(&new).count();
                let keep = if opts.pickaxe_all {
                    old_count > 0 || new_count > 0
                } else {
                    old_count != new_count
                };
                if keep {
                    out.push(e);
                }
            }
            return Ok(out);
        }

        let mut out = Vec::new();
        for e in entries {
            let (old, new) = read_blob_pair(odb, &e)?;
            let old_count = old.matches(needle.as_str()).count();
            let new_count = new.matches(needle.as_str()).count();
            let keep = if opts.pickaxe_all {
                old_count > 0 || new_count > 0
            } else {
                old_count != new_count
            };
            if keep {
                out.push(e);
            }
        }
        return Ok(out);
    }

    Ok(entries)
}

fn filter_pathspecs(entries: Vec<DiffEntry>, pathspecs: &[String]) -> Vec<DiffEntry> {
    if pathspecs.is_empty() {
        return entries;
    }
    entries
        .into_iter()
        .filter(|e| diff_entry_matches_pathspecs(e, pathspecs))
        .collect()
}

fn diff_entry_pathspec_context(entry: &DiffEntry) -> grit_lib::pathspec::PathspecMatchContext {
    use grit_lib::pathspec::PathspecMatchContext;

    match entry.status {
        DiffStatus::Deleted => context_from_mode_octal(&entry.old_mode),
        DiffStatus::Added => context_from_mode_octal(&entry.new_mode),
        _ => {
            let old = context_from_mode_octal(&entry.old_mode);
            let new = context_from_mode_octal(&entry.new_mode);
            PathspecMatchContext {
                is_directory: old.is_directory || new.is_directory,
                is_git_submodule: old.is_git_submodule || new.is_git_submodule,
            }
        }
    }
}

fn diff_entry_matches_pathspecs(entry: &DiffEntry, pathspecs: &[String]) -> bool {
    let ctx = diff_entry_pathspec_context(entry);
    pathspecs.iter().any(|spec| {
        if let Some(ref p) = entry.new_path {
            if matches_pathspec_with_context(spec, p, ctx) {
                return true;
            }
        }
        if let Some(ref p) = entry.old_path {
            if entry.new_path.as_ref() != Some(p) && matches_pathspec_with_context(spec, p, ctx) {
                return true;
            }
        }
        false
    })
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
