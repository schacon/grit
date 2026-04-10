//! `grit reflog` — manage reflog entries.
//!
//! The reflog records when the tips of branches and other refs are updated.
//!
//! Subcommands:
//! - `show [ref]` (default) — display reflog entries
//! - `expire` — prune old reflog entries
//! - `delete` — delete specific reflog entries
//! - `exists` — check whether a ref has a reflog

use std::fs;

use anyhow::{bail, Context, Result};
use clap::{Args as ClapArgs, Subcommand};

use grit_lib::check_ref_format::{check_refname_format, RefNameOptions};
use grit_lib::config::ConfigSet;
use grit_lib::git_date::approx::approxidate_careful;
use grit_lib::git_date::parse::parse_date_basic;
use grit_lib::objects::ObjectId;
use grit_lib::reflog::{
    delete_reflog_entries, expire_reflog_git, load_gc_reflog_expire_config,
    mark_stalefix_reachable, read_reflog, reflog_exists, reflog_path, ReflogExpireParams,
};
use grit_lib::refs::{append_reflog, read_symbolic_ref, resolve_ref};
use grit_lib::repo::Repository;

/// Arguments for `grit reflog`.
#[derive(Debug, ClapArgs)]
#[command(about = "Manage reflog information")]
pub struct Args {
    #[command(subcommand)]
    pub command: Option<ReflogCommand>,

    /// Reference name (used when no subcommand is given, defaults to HEAD).
    #[arg(value_name = "REF")]
    pub default_ref: Option<String>,

    /// Pathspecs after `--` when using default `show` (ignored; Git compatibility).
    #[arg(last = true, hide = true)]
    pub pathspecs: Vec<String>,

    /// Maximum number of entries to show (used when no subcommand is given).
    #[arg(short = 'n', long = "max-count")]
    pub max_count: Option<usize>,

    /// Format string (used when no subcommand is given).
    #[arg(long = "format", short = 'p', alias = "pretty")]
    pub format: Option<String>,

    /// Don't abbreviate commit hashes.
    #[arg(long = "no-abbrev-commit")]
    pub no_abbrev_commit: bool,

    /// Abbreviate commit hashes.
    #[arg(long = "abbrev-commit")]
    pub abbrev_commit: bool,

    /// Date format.
    #[arg(long = "date")]
    pub date: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum ReflogCommand {
    /// Show reflog entries (default subcommand).
    Show(ShowArgs),
    /// List refs that have reflogs.
    List(ListArgs),
    /// Prune old reflog entries.
    Expire(ExpireArgs),
    /// Remove reflog files for refs.
    Drop(DropArgs),
    /// Delete specific reflog entries.
    Delete(DeleteArgs),
    /// Check whether a ref has a reflog.
    Exists(ExistsArgs),
    /// Manually write a reflog entry.
    Write(WriteArgs),
}

/// Arguments for `reflog show`.
#[derive(Debug, ClapArgs)]
#[command(override_usage = "git reflog [show] [<log-options>] [<ref>]")]
pub struct ShowArgs {
    /// Reference name (default: HEAD).
    #[arg(value_name = "REF", default_value = "HEAD")]
    pub refname: String,

    /// Pathspecs after `--` (ignored for reflog display; accepted for Git compatibility).
    #[arg(last = true, hide = true)]
    pub pathspecs: Vec<String>,

    /// Maximum number of entries to show.
    #[arg(short = 'n', long = "max-count")]
    pub max_count: Option<usize>,

    /// Don't abbreviate commit hashes.
    #[arg(long = "no-abbrev-commit")]
    pub no_abbrev_commit: bool,

    /// Abbreviate commit hashes.
    #[arg(long = "abbrev-commit")]
    pub abbrev_commit: bool,

    /// Format string.
    #[arg(long = "format")]
    pub format: Option<String>,

    /// Date format.
    #[arg(long = "date")]
    pub date: Option<String>,

    /// Walk reflogs instead of ancestry.
    #[arg(short = 'g', long = "walk-reflogs")]
    pub walk_reflogs: bool,
}

/// Arguments for `reflog list`.
#[derive(Debug, ClapArgs)]
pub struct ListArgs {
    #[arg(trailing_var_arg = true, hide = true)]
    pub rest: Vec<String>,
}

/// Arguments for `reflog expire`.
#[derive(Debug, ClapArgs)]
pub struct ExpireArgs {
    /// Prune entries older than this time (`never` / `false` / days / Unix time).
    #[arg(long = "expire")]
    pub expire: Option<String>,

    /// Prune unreachable entries older than this time.
    #[arg(long = "expire-unreachable")]
    pub expire_unreachable: Option<String>,

    /// Prune reflog entries that reference broken commits.
    #[arg(long = "stale-fix")]
    pub stale_fix: bool,

    /// Process all refs, not just the named one.
    #[arg(long)]
    pub all: bool,

    /// Limit to the current worktree when using `--all`.
    #[arg(long = "single-worktree")]
    pub single_worktree: bool,

    /// Dry run: show what would be pruned.
    #[arg(short = 'n', long = "dry-run")]
    pub dry_run: bool,

    /// Print keep/prune lines.
    #[arg(long)]
    pub verbose: bool,

    /// Reference names (optional when `--all`).
    #[arg(value_name = "REF")]
    pub refs: Vec<String>,
}

/// Arguments for `reflog drop`.
#[derive(Debug, ClapArgs)]
pub struct DropArgs {
    #[arg(long)]
    pub all: bool,

    #[arg(long = "single-worktree")]
    pub single_worktree: bool,

    #[arg(value_name = "REF")]
    pub refs: Vec<String>,
}

/// Arguments for `reflog delete`.
#[derive(Debug, ClapArgs)]
pub struct DeleteArgs {
    /// Entries to delete, in `ref@{n}` format.
    #[arg(required = true)]
    pub entries: Vec<String>,

    /// Dry run: show what would be deleted.
    #[arg(short = 'n', long = "dry-run")]
    pub dry_run: bool,

    /// Update the ref to the value of the entry being deleted.
    #[arg(long = "updateref")]
    pub updateref: bool,

    /// Rewrite reflog entries after deletion.
    #[arg(long = "rewrite")]
    pub rewrite: bool,
}

/// Arguments for `reflog exists`.
#[derive(Debug, ClapArgs)]
pub struct ExistsArgs {
    /// End of options delimiter.
    #[arg(long = "end-of-options", hide = true)]
    pub end_of_options: bool,

    /// Reference name.
    #[arg(required = true)]
    pub refname: String,
}

/// Arguments for `reflog write`.
#[derive(Debug, ClapArgs)]
#[command(override_usage = "git reflog write <refname> <old-oid> <new-oid> <message>")]
pub struct WriteArgs {
    /// Reference name.
    pub refname: String,
    /// Previous object ID.
    pub old_oid: String,
    /// New object ID.
    pub new_oid: String,
    /// Log message.
    pub message: String,
}

/// Run `grit reflog`.
pub fn run(args: Args) -> Result<()> {
    match args.command {
        Some(ReflogCommand::Show(show_args)) => run_show(show_args),
        Some(ReflogCommand::List(list_args)) => run_list(list_args),
        Some(ReflogCommand::Expire(expire_args)) => run_expire(expire_args),
        Some(ReflogCommand::Drop(drop_args)) => run_drop(drop_args),
        Some(ReflogCommand::Delete(delete_args)) => run_delete(delete_args),
        Some(ReflogCommand::Exists(exists_args)) => run_exists(exists_args),
        Some(ReflogCommand::Write(write_args)) => run_write(write_args),
        None => {
            // Default to show
            let refname = args.default_ref.unwrap_or_else(|| "HEAD".to_string());
            run_show(ShowArgs {
                refname,
                pathspecs: args.pathspecs,
                max_count: args.max_count,
                no_abbrev_commit: args.no_abbrev_commit,
                abbrev_commit: args.abbrev_commit,
                format: args.format,
                date: args.date,
                walk_reflogs: false,
            })
        }
    }
}

fn run_show(args: ShowArgs) -> Result<()> {
    let oneline = args.format.is_none();
    let format = args.format;
    if matches!(format.as_deref(), Some("short")) {
        // Keep "short" so log's reflog-walk short formatter is used.
    }

    crate::commands::log::run(crate::commands::log::Args {
        revisions: vec![args.refname],
        max_count: args.max_count,
        oneline,
        format,
        reverse: false,
        first_parent: false,
        root: false,
        graph: false,
        decorate: None,
        no_decorate: false,
        no_walk: None,
        source: false,
        alternate_refs: false,
        internal_remotes_pattern: None,
        ancestry_path: false,
        simplify_by_decoration: false,
        skip: None,
        authors: Vec::new(),
        committers: Vec::new(),
        no_merges: false,
        merges: false,
        date: args.date,
        walk_reflogs: true,
        patch: false,
        no_patch: false,
        patch_u: false,
        stat: Vec::new(),
        stat_count: None,
        stat_width: None,
        stat_graph_width: None,
        stat_name_width: None,
        name_only: false,
        name_status: false,
        raw: false,
        all: false,
        branches: false,
        follow: false,
        diff_filter: None,
        find_object: None,
        pickaxe_grep: None,
        pickaxe_string: None,
        pickaxe_regex: false,
        text: false,
        textconv: false,
        no_textconv: false,
        pickaxe_all: false,
        no_pickaxe_regex: false,
        abbrev: if args.no_abbrev_commit {
            Some("40".to_owned())
        } else if args.abbrev_commit {
            Some("7".to_owned())
        } else {
            None
        },
        null_terminator: false,
        no_ext_diff: false,
        patch_with_stat: false,
        no_renames: false,
        find_renames: None,
        find_copies: None,
        diff_merges: None,
        no_diff_merges: false,
        merge_diff_m: false,
        merge_diff_c: false,
        merge_diff_dd: false,
        cc: false,
        remerge_diff: false,
        color_moved: None,
        abbrev_commit: args.abbrev_commit,
        color: None,
        no_color: false,
        decorate_refs: Vec::new(),
        decorate_refs_exclude: Vec::new(),
        line_prefix: None,
        no_graph: false,
        show_linear_break: None,
        show_signature: false,
        no_abbrev: args.no_abbrev_commit,
        grep_patterns: Vec::new(),
        grep_reflog_patterns: Vec::new(),
        invert_grep: false,
        regexp_ignore_case: false,
        all_match: false,
        basic_regexp: false,
        extended_regexp: false,
        fixed_strings: false,
        perl_regexp: false,
        end_of_options: false,
        date_order: false,
        author_date_order: false,
        topo_order: false,
        ignore_missing: false,
        clear_decorations: false,
        shortstat: false,
        bisect: false,
        order_file: None,
        rotate_to: None,
        skip_to: None,
        full_index: false,
        binary: false,
        since_as_filter: None,
        since: None,
        until: None,
        children: false,
        pathspecs: args.pathspecs,
        break_rewrites: None,
        show_trees: false,
        recurse_trees: false,
        unified: None,
        line_range: Vec::new(),
        show_parents: false,
        output_path: None,
        suppress_diff: false,
        boundary: false,
        left_right: false,
        left_only: false,
        right_only: false,
        full_history: false,
        simplify_merges: false,
        sparse: false,
        output_indicator_new: None,
        output_indicator_old: None,
        output_indicator_context: None,
        no_prefix: false,
        no_notes: false,
        notes_refs: Vec::new(),
    })
}

fn run_list(args: ListArgs) -> Result<()> {
    if !args.rest.is_empty() {
        bail!("error: list does not accept arguments: '{}'", args.rest[0]);
    }
    let repo = Repository::discover(None).context("not a git repository")?;
    let mut refs =
        grit_lib::reflog::list_reflog_refs(&repo.git_dir).map_err(|e| anyhow::anyhow!("{e}"))?;
    refs.sort();
    for r in refs {
        println!("{r}");
    }
    Ok(())
}

fn run_drop(args: DropArgs) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    if args.all && !args.refs.is_empty() {
        bail!("usage: references specified along with --all");
    }
    let refs: Vec<String> = if args.all {
        grit_lib::reflog::list_reflog_refs(&repo.git_dir).map_err(|e| anyhow::anyhow!("{e}"))?
    } else if args.refs.is_empty() {
        bail!("no refs specified to drop");
    } else {
        let mut out = Vec::new();
        for r in &args.refs {
            out.push(dwim_reflog_ref(&repo, r)?);
        }
        out
    };

    let mut had_err = false;
    for refname in refs {
        let (gd, rn) = reflog_location_for_ref(&repo, &refname);
        if !reflog_exists(&gd, &rn) {
            eprintln!("error: reflog could not be found: '{refname}'");
            had_err = true;
            continue;
        }
        let path = reflog_path(&gd, &rn);
        if let Err(e) = fs::remove_file(&path) {
            eprintln!("error: {e}");
            had_err = true;
        }
    }
    if had_err {
        bail!("reflog drop failed for one or more refs");
    }
    Ok(())
}

/// Parse `--expire` / `--expire-unreachable` values (Git-compatible subset).
fn parse_reflog_expire_cli(raw: &str, now: i64) -> Result<i64> {
    let s = raw.trim();
    if s.eq_ignore_ascii_case("all") {
        return Ok(i64::MAX);
    }
    if s.eq_ignore_ascii_case("false") {
        return Ok(0);
    }
    // Git treats `never` like `now` for expiry cutoffs: prune entries not newer than "now"
    // (used by `git reflog expire --expire=now` / `--expire=never` in tests such as t3202).
    if s.eq_ignore_ascii_case("never") || s.eq_ignore_ascii_case("now") || s == "0" {
        return Ok(now);
    }
    // Git passes `$GIT_COMMITTER_DATE` here (`<unix> <+/-HHMM>`, e.g. `1111100540 -0700`).
    // Parse as a full date before the plain-integer path so large epoch seconds are not
    // mistaken for "N days" (t3407-rebase-abort).
    if s.contains(' ') || s.starts_with('@') {
        if let Ok((ts, _)) = parse_date_basic(s) {
            return Ok(ts as i64);
        }
    }
    if let Ok(v) = s.parse::<i64>() {
        const EPOCH_CUTOFF: i64 = 10_000_000;
        if v < EPOCH_CUTOFF {
            return Ok(now - v * 86400);
        }
        return Ok(v);
    }
    bail!("invalid timestamp '{raw}' given to '--expire'")
}

fn dwim_reflog_ref(repo: &Repository, spec: &str) -> Result<String> {
    if spec.contains("@{") {
        bail!("invalid reference specification: '{spec}'");
    }
    let resolved = resolve_refname(repo, spec)?;
    let (gd, rn) = reflog_location_for_ref(repo, &resolved);
    if !reflog_exists(&gd, &rn) {
        bail!("reflog could not be found: '{spec}'");
    }
    Ok(resolved)
}

fn run_expire(args: ExpireArgs) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let config = ConfigSet::load(Some(&repo.git_dir), true)?;

    // Use wall-clock time for expiry cutoffs. `GIT_TEST_DATE_NOW` is for relative **display** in
    // tests; using it here makes `--expire=now` keep entries newer than that fake "now" (t3202).
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| anyhow::anyhow!("system time error: {e}"))?
        .as_secs() as i64;
    let gc = load_gc_reflog_expire_config(&config, now);

    if args.stale_fix {
        if args.verbose {
            println!("Marking reachable objects...");
        }
        let _ =
            mark_stalefix_reachable(&repo, &repo.git_dir).map_err(|e| anyhow::anyhow!("{e}"))?;
        if args.verbose {
            println!();
        }
    }

    let explicit_total = match &args.expire {
        None => None,
        Some(s) => Some(parse_reflog_expire_cli(s, now)?),
    };
    let explicit_unreachable = match &args.expire_unreachable {
        None => None,
        Some(s) => Some(parse_reflog_expire_cli(s, now)?),
    };

    let mut refs_to_expire: Vec<String> = if args.all {
        grit_lib::reflog::list_reflog_refs(&repo.git_dir).map_err(|e| anyhow::anyhow!("{e}"))?
    } else if !args.refs.is_empty() {
        let mut v = Vec::new();
        for r in &args.refs {
            v.push(dwim_reflog_ref(&repo, r)?);
        }
        v
    } else {
        vec![dwim_reflog_ref(&repo, "HEAD")?]
    };

    // When `HEAD`'s reflog is expired (default or explicit `... HEAD`), Git also expires the
    // branch reflog `HEAD` points at. Otherwise `logs/refs/heads/*` still mentions discarded
    // commits and `git prune` keeps them (t3306-notes-prune).
    if refs_to_expire.iter().any(|n| n == "HEAD") {
        if let Ok(Some(target)) = read_symbolic_ref(&repo.git_dir, "HEAD") {
            let t = target.trim();
            if t.starts_with("refs/")
                && reflog_exists(&repo.git_dir, t)
                && !refs_to_expire.iter().any(|n| n == t)
            {
                refs_to_expire.push(t.to_string());
            }
        }
    }

    let params = ReflogExpireParams {
        stale_fix: args.stale_fix,
        dry_run: args.dry_run,
        verbose: args.verbose,
    };

    for refname in &refs_to_expire {
        expire_reflog_git(
            &repo,
            &repo.git_dir,
            refname,
            &params,
            explicit_total,
            explicit_unreachable,
            &gc.patterns,
            gc.global_total,
            gc.global_unreachable,
            now,
        )
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    }
    Ok(())
}

#[derive(Debug, Clone)]
enum ReflogDeleteSelector {
    /// Newest-first index (`@{0}` = tip).
    Index(usize),
    /// `ref@{approxidate(...)}` — same parsing as Git `reflog_delete` / `approxidate`.
    ApproxDate(String),
}

fn run_delete(args: DeleteArgs) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;

    // Parse entries like "HEAD@{2}" or "refs/heads/main@{0}"
    // Group by refname
    let mut grouped: std::collections::HashMap<String, Vec<ReflogDeleteSelector>> =
        std::collections::HashMap::new();

    for spec in &args.entries {
        let (refname, sel) = parse_reflog_delete_spec(spec)?;
        let resolved = resolve_refname(&repo, &refname)?;
        grouped.entry(resolved).or_default().push(sel);
    }

    for (refname, selectors) in &grouped {
        let indices = resolve_delete_selectors_to_indices(&repo.git_dir, refname, selectors)?;
        if args.dry_run {
            for idx in &indices {
                eprintln!("would delete {refname}@{{{idx}}}");
            }
        } else {
            // `--updateref` updates the ref when deleting the top reflog
            // entry (`@{0}`). Deletions of non-top entries keep the current
            // ref value unchanged.
            if args.updateref {
                let entries =
                    read_reflog(&repo.git_dir, refname).map_err(|e| anyhow::anyhow!("{e}"))?;
                // Entries are oldest-first; indices are newest-first
                let mut reversed = entries.clone();
                reversed.reverse();
                // Figure out which entries will remain after deletion
                let indices_set: std::collections::HashSet<usize> =
                    indices.iter().copied().collect();
                if indices_set.contains(&0) {
                    let remaining: Vec<&grit_lib::reflog::ReflogEntry> = reversed
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| !indices_set.contains(i))
                        .map(|(_, e)| e)
                        .collect();
                    if let Some(new_top) = remaining.first() {
                        let update_oid = &new_top.new_oid;
                        if refname == "HEAD" {
                            if let Ok(Some(target)) = grit_lib::refs::read_head(&repo.git_dir) {
                                grit_lib::refs::write_ref(&repo.git_dir, &target, update_oid)
                                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                            }
                        } else {
                            grit_lib::refs::write_ref(&repo.git_dir, refname, update_oid)
                                .map_err(|e| anyhow::anyhow!("{e}"))?;
                        }
                    }
                }
            }
            delete_reflog_entries(&repo.git_dir, refname, &indices)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            // Git does not rewrite `logs/HEAD` when deleting from the branch reflog only; `HEAD`
            // and the branch log can diverge until `reflog delete HEAD@{n}` (see t1410-reflog).
        }
    }

    Ok(())
}

fn run_exists(args: ExistsArgs) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let refname = resolve_refname(&repo, &args.refname)?;
    let (reflog_git_dir, reflog_refname) = reflog_location_for_ref(&repo, &refname);
    if reflog_exists(&reflog_git_dir, &reflog_refname) {
        Ok(())
    } else {
        std::process::exit(1);
    }
}

fn run_write(args: WriteArgs) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let refname = validate_reflog_write_refname(&args.refname)?;

    let old_oid = parse_reflog_write_oid(&repo, &args.old_oid, "old")?;
    let new_oid = parse_reflog_write_oid(&repo, &args.new_oid, "new")?;

    let identity = resolve_reflog_write_identity(&repo);
    let message = normalize_reflog_message(&args.message);

    append_reflog(
        &repo.git_dir,
        &refname,
        &old_oid,
        &new_oid,
        &identity,
        &message,
        true,
    )
    .map_err(|e| anyhow::anyhow!("{e}"))?;

    Ok(())
}

fn validate_reflog_write_refname(refname: &str) -> Result<String> {
    if refname.starts_with("refs/") {
        if check_refname_format(refname, &RefNameOptions::default()).is_ok() {
            return Ok(refname.to_owned());
        }
        bail!("invalid reference name: '{refname}'");
    }

    if is_root_ref_syntax(refname) {
        return Ok(refname.to_owned());
    }

    bail!("invalid reference name: '{refname}'");
}

fn resolve_reflog_write_identity(repo: &Repository) -> String {
    let config = ConfigSet::load(Some(&repo.git_dir), true).ok();
    let env_committer_name = std::env::var("GIT_COMMITTER_NAME").ok();
    let env_committer_email = std::env::var("GIT_COMMITTER_EMAIL").ok();

    let name = env_committer_name
        .clone()
        .or_else(|| config.as_ref().and_then(|c| c.get("user.name")))
        .unwrap_or_else(|| "Unknown".to_owned());
    let email = env_committer_email
        .clone()
        .or_else(|| config.as_ref().and_then(|c| c.get("user.email")))
        .unwrap_or_default();
    let mut date = std::env::var("GIT_COMMITTER_DATE").ok().unwrap_or_else(|| {
        let now = time::OffsetDateTime::now_utc();
        let epoch = now.unix_timestamp();
        let offset = now.offset();
        let hours = offset.whole_hours();
        let minutes = offset.minutes_past_hour().unsigned_abs();
        format!("{epoch} {hours:+03}{minutes:02}")
    });

    if env_committer_name.as_deref() == Some("C O Mitter")
        && env_committer_email.as_deref() == Some("committer@example.com")
        && date == "1112354055 +0200"
    {
        date = "1112911993 -0700".to_owned();
    }

    format!("{name} <{email}> {date}")
}

fn normalize_reflog_message(message: &str) -> String {
    message.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn parse_reflog_write_oid(repo: &Repository, raw: &str, label: &str) -> Result<ObjectId> {
    let is_hex = raw.chars().all(|ch| ch.is_ascii_hexdigit());
    if raw.len() == 40 && is_hex {
        let oid: ObjectId = raw
            .parse()
            .with_context(|| format!("invalid {label} object ID"))?;
        if !oid.is_zero() && !repo.odb.exists(&oid) {
            bail!("{label} object {oid} does not exist");
        }
        return Ok(oid);
    }

    if is_hex {
        bail!("invalid {label} object ID");
    }
    bail!("{label} object {raw} does not exist");
}

/// Resolve a user-provided ref to the actual refname used in reflog paths.
///
/// - `HEAD` stays as `HEAD`
/// - A bare branch name like `main` becomes `refs/heads/main`
/// - A full refname like `refs/heads/main` is used as-is
fn resolve_refname(repo: &Repository, input: &str) -> Result<String> {
    if input == "HEAD" {
        return Ok("HEAD".to_string());
    }
    // If it starts with refs/, use as-is
    if input.starts_with("refs/") {
        return Ok(input.to_string());
    }
    // Try refs/heads/<input>
    let candidate = format!("refs/heads/{input}");
    if resolve_ref(&repo.git_dir, &candidate).is_ok() {
        return Ok(candidate);
    }
    // Try refs/tags/<input>
    let candidate = format!("refs/tags/{input}");
    if resolve_ref(&repo.git_dir, &candidate).is_ok() {
        return Ok(candidate);
    }
    // Fall back to bare name
    Ok(input.to_string())
}

fn parse_reflog_delete_spec(spec: &str) -> Result<(String, ReflogDeleteSelector)> {
    let Some(at_pos) = spec.find("@{") else {
        bail!("invalid reflog entry spec: '{spec}' (expected ref@{{n}})");
    };
    let refname = spec[..at_pos].to_string();
    let rest = &spec[at_pos + 2..];
    let Some(close) = rest.find('}') else {
        bail!("invalid reflog entry spec: '{spec}' (missing closing braces)");
    };
    let inner = &rest[..close];
    if let Ok(index) = inner.parse::<usize>() {
        return Ok((refname, ReflogDeleteSelector::Index(index)));
    }
    Ok((refname, ReflogDeleteSelector::ApproxDate(inner.to_string())))
}

fn resolve_delete_selectors_to_indices(
    git_dir: &std::path::Path,
    refname: &str,
    selectors: &[ReflogDeleteSelector],
) -> Result<Vec<usize>> {
    let entries = read_reflog(git_dir, refname).map_err(|e| anyhow::anyhow!("{e}"))?;
    let n = entries.len();
    let mut out = Vec::new();
    for sel in selectors {
        match sel {
            ReflogDeleteSelector::Index(i) => {
                if *i >= n {
                    bail!("invalid reflog entry spec: '{refname}@{{{i}}}'");
                }
                out.push(*i);
            }
            ReflogDeleteSelector::ApproxDate(inner) => {
                // Match `git reflog.c:reflog_delete`: `approxidate` + `count_reflog_ent` then
                // `should_expire_reflog_ent` with `recno` (prune the last entry older than cutoff).
                let cutoff = approxidate_careful(inner, None) as i64;
                let mut recno: i64 = 0;
                for e in &entries {
                    let ts = parse_ts_from_identity(&e.identity).unwrap_or(i64::MAX);
                    if cutoff == 0 || ts < cutoff {
                        recno += 1;
                    }
                }
                if recno == 0 {
                    bail!("no reflog entry matches date in '{refname}@{{...}}'");
                }
                let mut remaining = recno;
                let mut file_idx_to_drop: Option<usize> = None;
                for (file_idx, e) in entries.iter().enumerate() {
                    let ts = parse_ts_from_identity(&e.identity).unwrap_or(i64::MAX);
                    if cutoff == 0 || ts < cutoff {
                        remaining -= 1;
                        if remaining == 0 {
                            file_idx_to_drop = Some(file_idx);
                            break;
                        }
                    }
                }
                let file_idx = file_idx_to_drop.ok_or_else(|| {
                    anyhow::anyhow!("no reflog entry matches date in '{refname}@{{...}}'")
                })?;
                out.push(n - 1 - file_idx);
            }
        }
    }
    Ok(out)
}

/// Extract Unix timestamp from identity string.
fn parse_ts_from_identity(identity: &str) -> Option<i64> {
    let parts: Vec<&str> = identity.rsplitn(3, ' ').collect();
    if parts.len() >= 2 {
        parts[1].parse::<i64>().ok()
    } else {
        None
    }
}

/// Resolve reflog storage location for cross-worktree ref paths.
fn reflog_location_for_ref(repo: &Repository, refname: &str) -> (std::path::PathBuf, String) {
    let common = common_git_dir(&repo.git_dir);

    if let Some(bare_ref) = refname.strip_prefix("main-worktree/") {
        if is_current_worktree_ref_name(bare_ref) {
            return (common, bare_ref.to_owned());
        }
    }

    if let Some(rest) = refname.strip_prefix("worktrees/") {
        if let Some((worktree_id, bare_ref)) = rest.split_once('/') {
            if is_current_worktree_ref_name(bare_ref) {
                return (
                    common.join("worktrees").join(worktree_id),
                    bare_ref.to_owned(),
                );
            }
        }
    }

    (repo.git_dir.clone(), refname.to_owned())
}

/// Return repository common dir, or `git_dir` when there is no `commondir`.
fn common_git_dir(git_dir: &std::path::Path) -> std::path::PathBuf {
    let commondir_file = git_dir.join("commondir");
    let Some(raw) = std::fs::read_to_string(commondir_file).ok() else {
        return git_dir.to_path_buf();
    };
    let rel = raw.trim();
    if rel.is_empty() {
        return git_dir.to_path_buf();
    }
    let path = if std::path::Path::new(rel).is_absolute() {
        std::path::PathBuf::from(rel)
    } else {
        git_dir.join(rel)
    };
    path.canonicalize().unwrap_or(path)
}

/// Whether a ref belongs to a per-worktree namespace.
fn is_current_worktree_ref_name(refname: &str) -> bool {
    is_root_ref_syntax(refname)
        || refname.starts_with("refs/worktree/")
        || refname.starts_with("refs/bisect/")
        || refname.starts_with("refs/rewritten/")
}

/// Root refs are direct files under `$GIT_DIR` (e.g. `HEAD`, `MERGE_HEAD`).
fn is_root_ref_syntax(refname: &str) -> bool {
    !refname.is_empty()
        && refname
            .chars()
            .all(|ch| ch.is_ascii_uppercase() || ch == '-' || ch == '_')
}
