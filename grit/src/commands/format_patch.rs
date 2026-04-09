//! `grit format-patch` — generate patch files from commits.
//!
//! Produces email-style patch files (with From/Subject/Date headers and a diff)
//! for each commit in a range.  Output goes to individual `.patch` files in the
//! current directory (or `-o <dir>`), or to stdout with `--stdout`.

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use grit_lib::config::{parse_bool, ConfigSet};
use grit_lib::diff::{count_changes, diff_trees, unified_diff_with_prefix, zero_oid};
use grit_lib::diffstat::{
    write_diffstat_block, DiffstatOptions, FileStatInput, FORMAT_PATCH_STAT_WIDTH,
};
use grit_lib::merge_base::merge_bases_first_vs_rest;
use grit_lib::objects::{parse_commit, CommitData, ObjectId};
use grit_lib::odb::Odb;
use grit_lib::patch_ids::compute_patch_id;
use grit_lib::repo::Repository;
use grit_lib::rev_list::{
    rev_list, split_revision_token, split_symmetric_diff, OrderingMode, RevListOptions,
};
use grit_lib::rev_parse::{resolve_revision, resolve_revision_for_range_end};
use std::collections::HashSet;
use std::io::{self, Write};
use std::path::PathBuf;

use crate::git_commit_encoding::commit_message_unicode_for_display;
use crate::ident::{read_git_identity_name_env, GitIdentityNameEnv};

/// Arguments for `grit format-patch`.
#[derive(Debug, ClapArgs)]
#[command(about = "Prepare patches for e-mail submission")]
pub struct Args {
    /// Revision(s), range, or count. Empty means last commit (`-1`).
    /// With `--cherry-pick --right-only`, supports symmetric `A...B` like `git rebase --apply`.
    #[arg(value_name = "REV", num_args = 0.., allow_hyphen_values = true)]
    pub revisions: Vec<String>,

    /// Commit message encoding for the patch (Git compatibility; `t3901` passes this).
    #[arg(long = "encoding", value_name = "ENCODING")]
    pub encoding: Option<String>,

    /// Write output to stdout instead of individual files.
    #[arg(long)]
    pub stdout: bool,

    /// Omit patch-id equivalents on the left side of a symmetric range (used by `rebase --apply`).
    #[arg(long = "cherry-pick")]
    pub cherry_pick: bool,

    /// With `--cherry-pick`, list only commits on the right side of `A...B`.
    #[arg(long = "right-only")]
    pub right_only: bool,

    /// Use default a/b path prefixes (accepted for `rebase --apply` compatibility).
    #[arg(long = "default-prefix", hide = true)]
    pub default_prefix: bool,

    /// Do not detect renames in diffs (accepted for `rebase --apply` compatibility).
    #[arg(long = "no-renames", hide = true)]
    pub no_renames: bool,

    /// Do not emit a cover letter (accepted for `rebase --apply` compatibility).
    #[arg(long = "no-cover-letter", hide = true)]
    pub no_cover_letter: bool,

    /// Pretty-print / mbox encoding (accepted; `mboxrd` is the default output shape).
    #[arg(long = "pretty", value_name = "FORMAT", hide = true)]
    pub pretty: Option<String>,

    /// Order commits topologically (used by `rebase --apply`).
    #[arg(long = "topo-order")]
    pub topo_order: bool,

    /// Omit base-commit trailer (accepted for `rebase --apply` compatibility).
    #[arg(long = "no-base", hide = true)]
    pub no_base: bool,

    /// Add `[PATCH n/m]` numbering to subjects.
    #[arg(short = 'n', long = "numbered")]
    pub numbered: bool,

    /// Suppress `[PATCH n/m]` numbering.
    #[arg(short = 'N', long = "no-numbered")]
    pub no_numbered: bool,

    /// Start numbering patches at <n> instead of 1.
    #[arg(long = "start-number", value_name = "N", default_value_t = 1)]
    pub start_number: usize,

    /// Generate a cover letter as patch 0.
    #[arg(long = "cover-letter")]
    pub cover_letter: bool,

    /// Format all commits from root (instead of since a revision).
    #[arg(long = "root")]
    pub root: bool,

    /// Custom subject prefix (default: "PATCH").
    #[arg(long = "subject-prefix", value_name = "PREFIX")]
    pub subject_prefix: Option<String>,

    /// Output directory for patch files.
    #[arg(short = 'o', long = "output-directory", value_name = "DIR")]
    pub output_directory: Option<PathBuf>,

    /// Add base-commit info (the commit the series is based on).
    #[arg(long = "base", value_name = "COMMIT")]
    pub base: Option<String>,

    /// Add Signed-off-by trailer using the committer identity.
    #[arg(short = 's', long = "signoff")]
    pub signoff: bool,

    /// Set the In-Reply-To header (for threading patches).
    #[arg(long = "in-reply-to", value_name = "MESSAGE-ID")]
    pub in_reply_to: Option<String>,

    /// Add Cc header(s) to each patch email.
    #[arg(long = "cc", value_name = "EMAIL")]
    pub cc: Vec<String>,

    /// Add To header(s) to each patch email.
    #[arg(long = "to", value_name = "EMAIL")]
    pub to: Vec<String>,

    /// Create MIME multipart attachment.
    #[arg(long = "attach")]
    pub attach: bool,

    /// Create MIME inline attachment.
    #[arg(long = "inline")]
    pub inline: bool,

    /// Keep subject intact (do not strip/add [PATCH] prefix).
    #[arg(short = 'k', long = "keep-subject")]
    pub keep_subject: bool,

    /// Include patches for commits that don't change any files.
    #[arg(long = "always")]
    pub always: bool,

    /// Use RFC 2047 encoding for non-ASCII characters.
    #[arg(long = "rfc")]
    pub rfc: bool,

    /// Add extra header.
    #[arg(long = "add-header", value_name = "HEADER")]
    pub add_header: Vec<String>,

    /// Number of context lines in patches.
    #[arg(short = 'U', value_name = "N")]
    pub context_lines: Option<usize>,

    /// Include binary diffs (accepted for compatibility).
    #[arg(long = "binary")]
    pub binary: bool,

    /// Do not use a/b/ prefix in diff output.
    #[arg(long = "no-prefix")]
    pub no_prefix: bool,

    /// Detect renames.
    #[arg(short = 'M')]
    pub detect_renames: bool,

    /// Use numbered filenames (0001, 0002, ...) instead of subject-based names.
    #[arg(long = "numbered-files")]
    pub numbered_files: bool,

    /// Include diffstat in patch body (`--stat[=width[,name-width[,count]]]`).
    #[arg(long = "stat", num_args = 0..=1, default_missing_value = "", require_equals = true)]
    pub stat: Option<String>,

    /// Limit files shown in embedded `--stat`.
    #[arg(long = "stat-count")]
    pub stat_count: Option<usize>,

    /// Total width for embedded `--stat`.
    #[arg(long = "stat-width")]
    pub stat_width_cli: Option<usize>,

    /// Graph width cap for embedded `--stat`.
    #[arg(long = "stat-graph-width")]
    pub stat_graph_width: Option<usize>,

    /// Name width cap for embedded `--stat`.
    #[arg(long = "stat-name-width")]
    pub stat_name_width: Option<usize>,

    /// Limit number of patches (e.g., -1 for only the last commit).
    #[arg(short = '1', hide = true)]
    pub last_one: bool,

    /// Populated by the CLI layer from `-N` count shorthands (`format-patch -3`).
    #[arg(long = "grit-format-patch-max-count", hide = true, value_name = "N")]
    pub grit_format_patch_max_count: Option<usize>,
    /// Use the From: header to attribute patches (accepted, partial impl).
    #[arg(long = "from", default_missing_value = "", num_args = 0..=1, require_equals = true)]
    pub from: Option<String>,

    /// Suppress signature.
    #[arg(long = "no-signature")]
    pub no_signature: bool,

    /// Append notes.
    #[arg(long = "notes", default_missing_value = "", num_args = 0..=1, require_equals = true)]
    pub notes: Option<String>,

    /// Suppress notes.
    #[arg(long = "no-notes")]
    pub no_notes: bool,

    /// Ignore if upstream already has the patch.
    #[arg(long = "ignore-if-in-upstream")]
    pub ignore_if_in_upstream: bool,

    /// Reroll count / version prefix (e.g. -v2).
    #[arg(short = 'v', long = "reroll-count", value_name = "N")]
    pub reroll_count: Option<String>,

    /// Include interdiff against a previous version.
    #[arg(long = "interdiff", value_name = "REV")]
    pub interdiff: Option<String>,

    /// Include range-diff against a previous version.
    #[arg(long = "range-diff", value_name = "REV")]
    pub range_diff: Option<String>,

    /// Show patch (accepted for compat, default behavior).
    #[arg(short = 'p', long = "patch")]
    pub patch: bool,

    /// Max filename length for patches.
    #[arg(long = "filename-max-length", value_name = "N")]
    pub filename_max_length: Option<usize>,

    /// Creation factor for range-diff.
    #[arg(long = "creation-factor", value_name = "N")]
    pub creation_factor: Option<usize>,

    /// Output file (instead of per-patch files).
    #[arg(long = "output", value_name = "FILE")]
    pub output: Option<PathBuf>,

    /// Suppress Cc: headers.
    #[arg(long = "no-cc")]
    pub no_cc: bool,

    /// Suppress To: headers.
    #[arg(long = "no-to")]
    pub no_to: bool,

    /// Suppress From: header (overrides `format.from`).
    #[arg(long = "no-from")]
    pub no_from: bool,

    /// Do not add `format.headers` from config (still allows `--add-header`).
    #[arg(long = "no-add-header")]
    pub no_add_header: bool,

    /// Progress display (accepted for compat, no-op).
    #[arg(long = "progress")]
    pub progress: bool,

    /// Quiet mode.
    #[arg(short = 'q', long = "quiet")]
    pub quiet: bool,

    /// Order files according to the given orderfile.
    #[arg(short = 'O', value_name = "orderfile")]
    pub order_file: Option<String>,

    /// Show full object hashes in diff output.
    #[arg(long = "full-index")]
    pub full_index: bool,

    /// Graph mode for mbox output (affects embedded diffstat layout).
    #[arg(long = "graph", hide = true)]
    pub graph: bool,
}

/// How to populate the `From:` mbox header (the `From ` line is always the commit OID).
#[derive(Debug, Clone)]
enum FromHeaderMode {
    /// Use the commit author (Git default when `format.from` is unset / false).
    Author,
    /// `format.from=true` or `--from` without `=` value.
    Committer,
    /// Explicit mailbox from `--from=` or `format.from=<ident>`.
    Custom(String),
    /// `--no-from` or suppressed.
    Omit,
}

/// Extra headers/options computed from args, passed into formatting functions.
struct PatchOptions {
    in_reply_to: Option<String>,
    cc: Vec<String>,
    to: Vec<String>,
    extra_headers: Vec<String>,
    from_header: FromHeaderMode,
    signoff: bool,
    attach: bool,
    inline: bool,
    keep_subject: bool,
    base_commit: Option<String>,
    order_file: Option<String>,
    stat_width: usize,
    stat_name_width: Option<usize>,
    stat_graph_width: Option<usize>,
    stat_count: Option<usize>,
    /// `format-patch --graph`: indent stat like Git's mbox graph mode.
    format_patch_graph: bool,
    /// Unified diff path prefixes (`a/` / `b/` unless `format.noprefix` / `--no-prefix`).
    diff_src_prefix: &'static str,
    diff_dst_prefix: &'static str,
}

pub fn run(mut args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;

    // Load git configuration for format.* keys
    let config = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();

    if let Some(raw) = config.get("format.noprefix") {
        parse_bool(&raw).map_err(|e| {
            let q = |s: &str| format!("'{s}'");
            anyhow::anyhow!(
                "fatal: {e} for {}\
                 \nhint: {} used to accept any value and treat that as {}.\
                 \nhint: Now it only accepts boolean values, like what {} does.",
                q("format.noprefix"),
                q("format.noprefix"),
                q("true"),
                q("diff.noprefix"),
            )
        })?;
    }

    let (diff_src_prefix, diff_dst_prefix) = if args.no_prefix {
        ("", "")
    } else if args.default_prefix {
        ("a/", "b/")
    } else {
        match config.get("format.noprefix") {
            Some(v) => {
                let b = parse_bool(&v).map_err(|e| {
                    let q = |s: &str| format!("'{s}'");
                    anyhow::anyhow!(
                        "fatal: {e} for {}\
                         \nhint: {} used to accept any value and treat that as {}.\
                         \nhint: Now it only accepts boolean values, like what {} does.",
                        q("format.noprefix"),
                        q("format.noprefix"),
                        q("true"),
                        q("diff.noprefix"),
                    )
                })?;
                if b {
                    ("", "")
                } else {
                    ("a/", "b/")
                }
            }
            None => ("a/", "b/"),
        }
    };

    if let Some(ref val) = args.stat {
        if !val.is_empty() {
            let parts: Vec<&str> = val.split(',').collect();
            if let Some(w) = parts.first().and_then(|s| s.parse::<usize>().ok()) {
                if args.stat_width_cli.is_none() {
                    args.stat_width_cli = Some(w);
                }
            }
            if let Some(nw) = parts.get(1).and_then(|s| s.parse::<usize>().ok()) {
                if args.stat_name_width.is_none() {
                    args.stat_name_width = Some(nw);
                }
            }
            if let Some(c) = parts.get(2).and_then(|s| s.parse::<usize>().ok()) {
                if args.stat_count.is_none() {
                    args.stat_count = Some(c);
                }
            }
        }
    }

    let stat_width = args.stat_width_cli.unwrap_or(FORMAT_PATCH_STAT_WIDTH);
    let stat_name_width = args.stat_name_width;
    let stat_graph_width = args.stat_graph_width;

    let filename_max_length = args.filename_max_length.or_else(|| {
        config
            .get("format.filenamemaxlength")
            .or_else(|| config.get("format.filenameMaxLength"))
            .and_then(|s| s.trim().parse().ok())
    });

    let (positive_specs, exclude_specs): (Vec<&String>, Vec<&String>) =
        args.revisions.iter().partition(|s| !s.starts_with('^'));
    let exclude_rest: Vec<String> = exclude_specs
        .iter()
        .map(|s| s.strip_prefix('^').unwrap_or(s.as_str()).to_string())
        .collect();

    let mut rev_tokens: Vec<String> = positive_specs.iter().map(|s| (*s).clone()).collect();
    let max_count_flag = if args.last_one { Some(1) } else { None };
    let max_count_from_argv = strip_leading_neg_count(&mut rev_tokens);
    let mut max_count = max_count_flag
        .or(args.grit_format_patch_max_count)
        .or(max_count_from_argv);
    if positive_specs.is_empty() && max_count.is_none() {
        max_count = Some(1);
    }

    // Determine the list of commits to format.
    let commits = if args.cherry_pick && args.right_only {
        let range_spec = rev_tokens
            .first()
            .map(|s| s.as_str())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "--cherry-pick --right-only requires a symmetric revision range A...B"
                )
            })?;
        collect_cherry_pick_right_only_commits(&repo, range_spec, &exclude_rest, args.topo_order)?
    } else {
        if !exclude_rest.is_empty() {
            anyhow::bail!(
                "revision exclusions (^rev) are only supported with --cherry-pick --right-only"
            );
        }
        if args.root {
            let revision = rev_tokens
                .first()
                .cloned()
                .unwrap_or_else(|| "-1".to_owned());
            collect_root_commits(&repo, &revision)?
        } else {
            let (mut out, pos_specs, neg_specs) =
                collect_commits_for_format_patch(&repo, &rev_tokens, max_count, args.topo_order)?;
            if args.ignore_if_in_upstream {
                filter_ignore_if_in_upstream(&repo, &pos_specs, &neg_specs, &mut out)?;
            }
            out
        }
    };

    if commits.is_empty() {
        return Ok(());
    }

    let total = commits.len();
    let prefix = args.subject_prefix.as_deref().unwrap_or("PATCH");

    // Determine whether to number patches.
    let config_numbered_val = config.get("format.numbered").map(|v| v.to_string());
    let config_numbered = match config_numbered_val.as_deref() {
        Some("true") | Some("yes") | Some("1") => true,
        _ => false,
    };
    let use_numbering = if args.no_numbered {
        false
    } else if args.numbered || args.cover_letter || config_numbered {
        true
    } else {
        // Default behavior (and format.numbered=auto): number if multiple patches
        total > 1
    };

    let start = args.start_number;
    let display_total = if start != 1 { start + total - 1 } else { total };

    // Resolve --base commit (`rebase --apply` passes `--no-base`).
    let base_commit = if args.no_base {
        None
    } else if let Some(ref base_rev) = args.base {
        let base_oid = resolve_revision(&repo, base_rev)
            .with_context(|| format!("unknown base revision '{base_rev}'"))?;
        Some(base_oid.to_hex())
    } else {
        None
    };

    // Build merged To/Cc lists from config + command line.
    // format.to / format.cc are single-valued; format.headers is multi-valued
    // and can contain arbitrary "Header: value" lines.
    let mut to_list: Vec<String> = Vec::new();
    let mut cc_list: Vec<String> = Vec::new();
    let mut extra_headers: Vec<String> = Vec::new();

    if !args.no_add_header {
        // Read format.headers from config (multi-value)
        for h in config.get_all("format.headers") {
            let h = h.trim_end_matches('\n').to_string();
            if h.is_empty() {
                continue;
            }
            if let Some(val) = h.strip_prefix("To:") {
                to_list.push(val.trim().to_string());
            } else if let Some(val) = h.strip_prefix("Cc:") {
                cc_list.push(val.trim().to_string());
            } else {
                extra_headers.push(h);
            }
        }
    }

    // Read format.to and format.cc from config
    if !args.no_to {
        if let Some(to) = config.get("format.to") {
            to_list.push(to);
        }
    }
    if !args.no_cc {
        if let Some(cc) = config.get("format.cc") {
            cc_list.push(cc);
        }
    }

    // Append command-line --to and --cc
    to_list.extend(args.to.iter().cloned());
    cc_list.extend(args.cc.iter().cloned());

    // Append --add-header
    extra_headers.extend(args.add_header.iter().cloned());

    let from_header_mode = if args.no_from {
        FromHeaderMode::Omit
    } else if let Some(ref from_arg) = args.from {
        if from_arg.is_empty() {
            FromHeaderMode::Committer
        } else {
            FromHeaderMode::Custom(from_arg.clone())
        }
    } else {
        match config
            .get("format.from")
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            None => FromHeaderMode::Author,
            Some("true" | "yes" | "1") => FromHeaderMode::Committer,
            Some("false" | "no" | "0") => FromHeaderMode::Author,
            Some(s) => FromHeaderMode::Custom(s.to_owned()),
        }
    };

    let opts = PatchOptions {
        in_reply_to: args.in_reply_to.clone(),
        cc: cc_list,
        to: to_list,
        extra_headers,
        from_header: from_header_mode,
        signoff: args.signoff,
        attach: args.attach,
        inline: args.inline,
        keep_subject: args.keep_subject,
        base_commit,
        order_file: args.order_file.clone(),
        stat_width,
        stat_name_width,
        stat_graph_width,
        stat_count: args.stat_count,
        format_patch_graph: args.graph,
        diff_src_prefix,
        diff_dst_prefix,
    };

    let mut log_output_encoding = config
        .get("i18n.logOutputEncoding")
        .or_else(|| config.get("i18n.logoutputencoding"))
        .unwrap_or_else(|| "UTF-8".to_owned());
    if let Some(enc) = args.encoding.as_deref() {
        let t = enc.trim();
        if !t.is_empty() {
            log_output_encoding = t.to_owned();
        }
    }

    // Ensure output directory exists
    let out_dir = if let Some(ref dir) = args.output_directory {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("cannot create output directory '{}'", dir.display()))?;
        dir.clone()
    } else {
        std::env::current_dir().context("cannot determine current directory")?
    };

    let stdout_handle = io::stdout();

    // If --cover-letter, emit a cover letter first (patch 0/N)
    if args.cover_letter {
        let cover_subject = if use_numbering {
            format!("[{prefix} 0/{display_total}] *** SUBJECT HERE ***")
        } else {
            format!("[{prefix}] *** SUBJECT HERE ***")
        };
        let cover =
            format_cover_letter(&repo, &commits, &cover_subject, &opts, &log_output_encoding)?;
        if args.stdout {
            let mut out = stdout_handle.lock();
            write!(out, "{cover}")?;
        } else {
            let filename = "0000-cover-letter.patch".to_string();
            let path = out_dir.join(&filename);
            std::fs::write(&path, &cover)
                .with_context(|| format!("cannot write cover letter '{}'", path.display()))?;
            println!("{}", path.display());
        }
    }

    let is_last_patch = |idx: usize| idx + 1 == total;

    for (idx, (oid, commit)) in commits.iter().enumerate() {
        let patch_num = start + idx;
        let display_msg = commit_message_unicode_for_display(
            commit.encoding.as_deref(),
            &commit.message,
            commit.raw_message.as_deref(),
        );
        let subject_line = display_msg.lines().next().unwrap_or("");

        // Build the subject with optional numbering
        let subject = if opts.keep_subject {
            subject_line.to_string()
        } else if use_numbering {
            format!("[{prefix} {patch_num}/{display_total}] {subject_line}")
        } else {
            format!("[{prefix}] {subject_line}")
        };

        // Format the patch — append base-commit info to last patch
        let include_base = is_last_patch(idx);
        let patch = format_single_patch(
            &repo.odb,
            oid,
            commit,
            &subject,
            &opts,
            include_base,
            &log_output_encoding,
        )?;

        if args.stdout {
            let mut out = stdout_handle.lock();
            write!(out, "{patch}")?;
            // Separator between patches on stdout
            if idx + 1 < total {
                writeln!(out, "-- ")?;
                writeln!(out)?;
            }
        } else {
            let filename = format!(
                "{:04}-{}.patch",
                patch_num,
                sanitize_subject_with_limit(subject_line, filename_max_length)
            );
            let path = out_dir.join(&filename);
            std::fs::write(&path, &patch)
                .with_context(|| format!("cannot write patch file '{}'", path.display()))?;
            println!("{}", path.display());
        }
    }

    Ok(())
}

/// Commits for `git format-patch --cherry-pick --right-only A...B` (`rebase --apply` path).
fn collect_cherry_pick_right_only_commits(
    repo: &Repository,
    range_spec: &str,
    extra_excludes: &[String],
    topo_order: bool,
) -> Result<Vec<(ObjectId, CommitData)>> {
    let Some((lhs, rhs)) = split_symmetric_diff(range_spec) else {
        anyhow::bail!("expected symmetric revision range A...B, got '{range_spec}'");
    };
    let left_tip = if lhs.is_empty() {
        resolve_revision_for_range_end(repo, "HEAD")?
    } else {
        resolve_revision_for_range_end(repo, &lhs)?
    };
    let right_tip = if rhs.is_empty() {
        resolve_revision_for_range_end(repo, "HEAD")?
    } else {
        resolve_revision_for_range_end(repo, &rhs)?
    };
    let bases = merge_bases_first_vs_rest(repo, left_tip, &[right_tip])?;
    let mut negative: Vec<String> = bases.iter().map(|b| b.to_hex()).collect();
    for e in extra_excludes {
        let oid = resolve_revision_for_range_end(repo, e)?;
        negative.push(oid.to_hex());
    }
    let ordering = if topo_order {
        OrderingMode::Topo
    } else {
        OrderingMode::Default
    };
    let result = rev_list(
        repo,
        &[left_tip.to_hex(), right_tip.to_hex()],
        &negative,
        &RevListOptions {
            cherry_pick: true,
            right_only: true,
            left_right: true,
            symmetric_left: Some(left_tip),
            symmetric_right: Some(right_tip),
            ordering,
            ..Default::default()
        },
    )
    .map_err(|e| anyhow::anyhow!("{e}"))?;
    let mut oids = result.commits;
    oids.reverse();
    let mut commits = Vec::new();
    for oid in oids {
        let obj = repo.odb.read(&oid)?;
        let c = parse_commit(&obj.data)?;
        if c.parents.len() > 1 {
            continue;
        }
        commits.push((oid, c));
    }
    Ok(commits)
}

/// If the first revision token is `-N`, strip it and return `Some(N)` (Git `format-patch -3`).
fn strip_leading_neg_count(tokens: &mut Vec<String>) -> Option<usize> {
    let Some(first) = tokens.first() else {
        return None;
    };
    let rest = first.strip_prefix('-')?;
    let n: usize = rest.parse().ok()?;
    tokens.remove(0);
    Some(n)
}

/// Resolve revision argv like Git `format-patch`: `rev_list` with `max_parents=1`, reversed for
/// patch order. Returns positive/negative spec strings for `--ignore-if-in-upstream`.
fn collect_commits_for_format_patch(
    repo: &Repository,
    rev_tokens: &[String],
    max_count: Option<usize>,
    topo_order: bool,
) -> Result<(Vec<(ObjectId, CommitData)>, Vec<String>, Vec<String>)> {
    let mut positive: Vec<String> = Vec::new();
    let mut negative: Vec<String> = Vec::new();

    if rev_tokens.is_empty() {
        positive.push("HEAD".to_owned());
    } else {
        for t in rev_tokens {
            let (pos, neg) = split_revision_token(t);
            positive.extend(pos);
            negative.extend(neg);
        }
    }

    if positive.is_empty() {
        positive.push("HEAD".to_owned());
    }

    // `git format-patch <since>` with a single committish: same as `<since>..HEAD` (see `setup_revisions`).
    if negative.is_empty() && positive.len() == 1 {
        let spec = positive[0].trim();
        if spec != "HEAD"
            && !spec.is_empty()
            && grit_lib::rev_parse::split_double_dot_range(spec).is_none()
        {
            let since = positive[0].clone();
            positive[0] = "HEAD".to_owned();
            negative.push(since);
        }
    }

    let ordering = if topo_order {
        OrderingMode::Topo
    } else {
        OrderingMode::Default
    };

    let opts = RevListOptions {
        max_parents: Some(1),
        reverse: true,
        ordering,
        max_count,
        ..Default::default()
    };

    let result = rev_list(repo, &positive, &negative, &opts).map_err(|e| anyhow::anyhow!("{e}"))?;

    let mut commits = Vec::with_capacity(result.commits.len());
    for oid in result.commits {
        let obj = repo.odb.read(&oid).context("reading commit")?;
        let c = parse_commit(&obj.data).context("parsing commit")?;
        commits.push((oid, c));
    }

    Ok((commits, positive, negative))
}

/// Drop commits whose patch-id already appears on the other side of a two-endpoint range.
fn filter_ignore_if_in_upstream(
    repo: &Repository,
    positive: &[String],
    negative: &[String],
    commits: &mut Vec<(ObjectId, CommitData)>,
) -> Result<()> {
    if positive.len() != 1 || negative.len() != 1 {
        anyhow::bail!("--ignore-if-in-upstream requires exactly one range (e.g. main..side)");
    }
    // Match `get_patch_ids` in Git: collect patch-ids from commits reachable from the range's
    // left endpoint but not from its right (`main..side` → `main` minus `side`).
    let check_pos = vec![negative[0].clone()];
    let check_neg = vec![positive[0].clone()];
    let ids_result = rev_list(
        repo,
        &check_pos,
        &check_neg,
        &RevListOptions {
            max_parents: Some(1),
            ..Default::default()
        },
    )
    .map_err(|e| anyhow::anyhow!("{e}"))?;

    let mut upstream_patch_ids = HashSet::new();
    for oid in ids_result.commits {
        if let Ok(Some(pid)) = compute_patch_id(&repo.odb, &oid) {
            upstream_patch_ids.insert(pid);
        }
    }

    commits.retain(|(oid, _)| {
        if let Ok(Some(pid)) = compute_patch_id(&repo.odb, oid) {
            !upstream_patch_ids.contains(&pid)
        } else {
            true
        }
    });
    Ok(())
}

/// Collect all commits from root up to the given revision (for --root).
fn collect_root_commits(repo: &Repository, revision: &str) -> Result<Vec<(ObjectId, CommitData)>> {
    // If revision is a negative count, just use that
    if let Some(count_str) = revision.strip_prefix('-') {
        if let Ok(count) = count_str.parse::<usize>() {
            return collect_last_n_commits(repo, count);
        }
    }

    // Resolve the target
    let target_oid = resolve_revision(repo, revision)
        .with_context(|| format!("unknown revision '{revision}'"))?;

    // Walk all the way back to root
    let mut commits = Vec::new();
    let mut current = target_oid;

    loop {
        let obj = repo.odb.read(&current).context("reading commit")?;
        let commit = parse_commit(&obj.data).context("parsing commit")?;
        let parent = commit.parents.first().copied();
        commits.push((current, commit));
        match parent {
            Some(p) => current = p,
            None => break,
        }
    }

    commits.reverse();
    Ok(commits)
}

/// Collect the last N commits from HEAD.
fn collect_last_n_commits(repo: &Repository, count: usize) -> Result<Vec<(ObjectId, CommitData)>> {
    let head_oid = resolve_head_oid(repo)?;
    let mut commits = Vec::new();
    let mut current = head_oid;

    for _ in 0..count {
        let obj = repo.odb.read(&current).context("reading commit")?;
        let commit = parse_commit(&obj.data).context("parsing commit")?;
        let parent = commit.parents.first().copied();
        commits.push((current, commit));
        match parent {
            Some(p) => current = p,
            None => break,
        }
    }

    commits.reverse();
    Ok(commits)
}

/// Collect exactly one commit identified by `revision`.
fn collect_single_commit(repo: &Repository, revision: &str) -> Result<Vec<(ObjectId, CommitData)>> {
    let oid = resolve_revision(repo, revision)
        .with_context(|| format!("unknown revision '{revision}'"))?;
    let obj = repo.odb.read(&oid).context("reading commit")?;
    let commit = parse_commit(&obj.data).context("parsing commit")?;
    Ok(vec![(oid, commit)])
}

fn diffstat_for_patch_entries(
    odb: &Odb,
    entries: &[grit_lib::diff::DiffEntry],
    opts: &PatchOptions,
) -> Result<String> {
    let mut files: Vec<FileStatInput> = Vec::with_capacity(entries.len());
    for entry in entries {
        let path = entry.path().to_string();
        let old_raw = read_blob_raw(odb, &entry.old_oid);
        let new_raw = read_blob_raw(odb, &entry.new_oid);
        let binary = old_raw.contains(&0) || new_raw.contains(&0);
        if binary {
            let deleted = if entry.old_oid == zero_oid() {
                0
            } else {
                old_raw.len()
            };
            let added = if entry.new_oid == zero_oid() {
                0
            } else {
                new_raw.len()
            };
            files.push(FileStatInput {
                path_display: path,
                insertions: added,
                deletions: deleted,
                is_binary: true,
            });
        } else {
            let old_content = String::from_utf8_lossy(&old_raw).into_owned();
            let new_content = String::from_utf8_lossy(&new_raw).into_owned();
            let (ins, del) = count_changes(&old_content, &new_content);
            files.push(FileStatInput {
                path_display: path,
                insertions: ins,
                deletions: del,
                is_binary: false,
            });
        }
    }
    let line_prefix = if opts.format_patch_graph { "|  " } else { "" };
    let dstat_opts = DiffstatOptions {
        total_width: opts.stat_width,
        line_prefix,
        subtract_prefix_from_terminal: false,
        stat_name_width: opts.stat_name_width,
        stat_graph_width: opts.stat_graph_width,
        stat_count: opts.stat_count,
        color_add: "",
        color_del: "",
        color_reset: "",
        graph_bar_slack: 0,
        graph_prefix_budget_slack: 0,
    };
    let mut buf = Vec::new();
    write_diffstat_block(&mut buf, &files, &dstat_opts)?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

fn read_blob_raw(odb: &Odb, oid: &ObjectId) -> Vec<u8> {
    if *oid == zero_oid() {
        return Vec::new();
    }
    odb.read(oid).map(|o| o.data).unwrap_or_default()
}

/// Resolve HEAD to an ObjectId.
fn resolve_head_oid(repo: &Repository) -> Result<ObjectId> {
    let head = grit_lib::state::resolve_head(&repo.git_dir).context("cannot resolve HEAD")?;
    head.oid()
        .copied()
        .ok_or_else(|| anyhow::anyhow!("HEAD is unborn"))
}

/// Generate a cover letter for a patch series.
fn format_cover_letter(
    repo: &Repository,
    commits: &[(ObjectId, CommitData)],
    subject: &str,
    patch_opts: &PatchOptions,
    log_output_encoding: &str,
) -> Result<String> {
    let mut out = String::new();

    // Use the last commit's info for From/Date
    let (last_oid, last_commit) = commits.last().expect("non-empty commits");

    out.push_str(&format!(
        "From {} Mon Sep 17 00:00:00 2001\n",
        last_oid.to_hex()
    ));

    let charset_label = rfc2047_charset_label(log_output_encoding);
    let use_utf8_log = charset_label.eq_ignore_ascii_case("UTF-8");
    if !matches!(patch_opts.from_header, FromHeaderMode::Omit) {
        let mailbox = mailbox_for_from_header(last_commit, &patch_opts.from_header);
        let from_header = if use_utf8_log {
            encode_email_address(&mailbox)
        } else {
            encode_email_address_for_charset(&mailbox, &charset_label)
        };
        out.push_str(&format!("From: {from_header}\n"));
    }

    let date = format_date_rfc2822(&last_commit.author);
    out.push_str(&format!("Date: {date}\n"));

    out.push_str(&format!("Subject: {subject}\n"));
    if !use_utf8_log {
        out.push_str("MIME-Version: 1.0\n");
        out.push_str(&format!(
            "Content-Type: text/plain; charset={charset_label}\n"
        ));
        out.push_str("Content-Transfer-Encoding: 8bit\n");
    }
    out.push('\n');
    out.push_str("*** BLURB HERE ***\n");
    out.push('\n');

    // Shortlog
    for (_oid, commit) in commits {
        let display_msg = commit_message_unicode_for_display(
            commit.encoding.as_deref(),
            &commit.message,
            commit.raw_message.as_deref(),
        );
        let first_line = display_msg.lines().next().unwrap_or("");
        let decoded_author =
            crate::git_commit_encoding::decode_rfc2047_mailbox_from_line(&commit.author);
        let author_name = if let Some(bracket) = decoded_author.find('<') {
            decoded_author[..bracket].trim()
        } else {
            decoded_author.as_str()
        };
        out.push_str(&format!("  {author_name} ({}):\n", 1));
        out.push_str(&format!("    {first_line}\n"));
        out.push('\n');
    }

    // Diffstat across all commits
    let first_parent_tree = commits.first().and_then(|(_oid, commit)| {
        commit.parents.first().and_then(|parent_oid| {
            repo.odb
                .read(parent_oid)
                .ok()
                .and_then(|obj| parse_commit(&obj.data).ok())
                .map(|c| c.tree)
        })
    });
    let last_tree = &last_commit.tree;

    let diff_entries = diff_trees(&repo.odb, first_parent_tree.as_ref(), Some(last_tree), "")
        .context("computing diff for cover letter")?;

    out.push_str(&diffstat_for_patch_entries(
        &repo.odb,
        &diff_entries,
        patch_opts,
    )?);
    out.push('\n');

    out.push_str("-- \n");
    out.push_str("grit\n");
    out.push('\n');

    Ok(out)
}

/// Get the signoff identity, preferring GIT_COMMITTER_NAME/EMAIL env vars.
fn get_signoff_identity(committer_ident: &str) -> (String, String) {
    let env_email = std::env::var("GIT_COMMITTER_EMAIL").ok();

    let name = match read_git_identity_name_env("GIT_COMMITTER_NAME") {
        GitIdentityNameEnv::Set(s) if !s.is_empty() => s,
        _ => {
            if let Some(bracket) = committer_ident.find('<') {
                committer_ident[..bracket].trim().to_string()
            } else {
                "Unknown".to_string()
            }
        }
    };
    let email = env_email.unwrap_or_else(|| {
        extract_email(committer_ident)
            .unwrap_or("unknown")
            .to_string()
    });

    (name, email)
}

/// Extract the email portion from an ident string like "Name <email> ts tz".
fn extract_email(ident: &str) -> Option<&str> {
    let start = ident.find('<')?;
    let end = ident.find('>')?;
    Some(&ident[start + 1..end])
}

/// Format a single commit as an email-style patch.
fn format_single_patch(
    odb: &Odb,
    oid: &ObjectId,
    commit: &CommitData,
    subject: &str,
    opts: &PatchOptions,
    include_base: bool,
    log_output_encoding: &str,
) -> Result<String> {
    let mut out = String::new();
    let charset_label = rfc2047_charset_label(log_output_encoding);
    let use_utf8_log = charset_label.eq_ignore_ascii_case("UTF-8");
    let commit_msg_unicode = commit_message_unicode_for_display(
        commit.encoding.as_deref(),
        &commit.message,
        commit.raw_message.as_deref(),
    );

    // Generate the diff first (needed for MIME attachment)
    let parent_tree = commit.parents.first().map(|parent_oid| {
        odb.read(parent_oid)
            .ok()
            .and_then(|obj| parse_commit(&obj.data).ok())
            .map(|c| c.tree)
    });
    let parent_tree_oid: Option<ObjectId> = parent_tree.flatten();

    let diff_entries_raw = diff_trees(odb, parent_tree_oid.as_ref(), Some(&commit.tree), "")
        .context("computing diff")?;
    let diff_entries = if let Some(ref order_path) = opts.order_file {
        let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        crate::commands::diff::apply_orderfile_entries(diff_entries_raw, order_path, &cwd)?
    } else {
        diff_entries_raw
    };

    // Build stat + full diff into separate string
    let mut diff_text = String::new();
    diff_text.push_str(&diffstat_for_patch_entries(odb, &diff_entries, opts)?);
    diff_text.push('\n');

    for entry in &diff_entries {
        let old_path = entry.old_path.as_deref().unwrap_or("/dev/null");
        let new_path = entry.new_path.as_deref().unwrap_or("/dev/null");
        write_diff_header_to_string(
            &mut diff_text,
            entry,
            opts.diff_src_prefix,
            opts.diff_dst_prefix,
        );
        let old_content = read_blob_content(odb, &entry.old_oid);
        let new_content = read_blob_content(odb, &entry.new_oid);
        let patch = unified_diff_with_prefix(
            &old_content,
            &new_content,
            old_path,
            new_path,
            3,
            0,
            opts.diff_src_prefix,
            opts.diff_dst_prefix,
        );
        diff_text.push_str(&patch);
    }

    let patch_start = diff_text.find("diff --git").unwrap_or(diff_text.len());
    let stat_block = diff_text[..patch_start].to_string();
    let patch_only = diff_text[patch_start..].to_string();

    let use_mime = opts.attach || opts.inline;
    let boundary = "------------grit-patch-boundary";

    // From line
    out.push_str(&format!("From {} Mon Sep 17 00:00:00 2001\n", oid.to_hex()));

    if !matches!(opts.from_header, FromHeaderMode::Omit) {
        let mailbox = mailbox_for_from_header(commit, &opts.from_header);
        let from_header = if use_utf8_log {
            encode_email_address(&mailbox)
        } else {
            encode_email_address_for_charset(&mailbox, &charset_label)
        };
        out.push_str(&format!("From: {from_header}\n"));
    }

    // Date: from author timestamp
    let date = format_date_rfc2822(&commit.author);
    out.push_str(&format!("Date: {date}\n"));

    // Subject
    out.push_str(&format!("Subject: {subject}\n"));

    // In-Reply-To / References headers
    if let Some(ref msg_id) = opts.in_reply_to {
        out.push_str(&format!("In-Reply-To: {msg_id}\n"));
        out.push_str(&format!("References: {msg_id}\n"));
    }

    // Extra headers from --add-header and format.headers (excluding To/Cc)
    for h in &opts.extra_headers {
        let h = h.trim_end_matches('\n');
        if !h.is_empty() {
            out.push_str(h);
            out.push('\n');
        }
    }

    // Cc headers — emit as a single folded header if multiple
    if !opts.cc.is_empty() {
        let encoded: Vec<String> = opts.cc.iter().map(|a| encode_email_address(a)).collect();
        write_folded_header(&mut out, "Cc", &encoded);
    }

    // To headers — emit as a single folded header if multiple
    if !opts.to.is_empty() {
        let encoded: Vec<String> = opts.to.iter().map(|a| encode_email_address(a)).collect();
        write_folded_header(&mut out, "To", &encoded);
    }

    // Check if body/signoff will contain non-ASCII (need MIME headers)
    let signoff_has_non_ascii = if opts.signoff {
        let (name, _email) = get_signoff_identity(&commit.committer);
        name.bytes().any(|b| b > 127)
    } else {
        false
    };
    let body_has_non_ascii = commit_msg_unicode.bytes().any(|b| b > 127) || signoff_has_non_ascii;
    let needs_mime = use_mime || body_has_non_ascii;

    // MIME headers for --attach / --inline or non-ASCII body
    if needs_mime {
        out.push_str("MIME-Version: 1.0\n");
        if use_mime {
            out.push_str(&format!(
                "Content-Type: multipart/mixed; boundary=\"{}\"\n",
                boundary
            ));
        } else {
            out.push_str(&format!(
                "Content-Type: text/plain; charset={charset_label}\n"
            ));
            out.push_str("Content-Transfer-Encoding: 8bit\n");
        }
    }

    out.push('\n');

    // Commit message body (skip first line which is in Subject)
    let body: String = commit_msg_unicode
        .lines()
        .skip(1)
        .collect::<Vec<_>>()
        .join("\n");
    let body = body.trim_start_matches('\n');

    if use_mime {
        // MIME multipart: description part, then patch as attachment
        out.push_str(&format!("--{boundary}\n"));
        out.push_str(&format!(
            "Content-Type: text/plain; charset={charset_label}\n"
        ));
        out.push_str("Content-Transfer-Encoding: 8bit\n");
        out.push('\n');
        if !body.is_empty() {
            out.push_str(body);
            out.push('\n');
        }

        // Signoff in body part
        if opts.signoff {
            let (name, email) = get_signoff_identity(&commit.committer);
            let sob = if use_utf8_log {
                format!("{name} <{email}>")
            } else {
                encode_email_address_for_charset(&format!("{name} <{email}>"), &charset_label)
            };
            out.push_str(&format!("\nSigned-off-by: {sob}\n"));
        }

        out.push_str("---\n");
        out.push_str(&stat_block);
        out.push('\n');

        // Patch attachment part
        out.push_str(&format!("--{boundary}\n"));
        let disposition = if opts.inline { "inline" } else { "attachment" };
        let subject_line = commit_msg_unicode.lines().next().unwrap_or("patch");
        let filename = format!("{}.patch", sanitize_subject(subject_line));
        out.push_str(&format!(
            "Content-Type: text/x-patch; charset={charset_label}\n"
        ));
        out.push_str("Content-Transfer-Encoding: 8bit\n");
        out.push_str(&format!(
            "Content-Disposition: {disposition}; filename=\"{filename}\"\n"
        ));
        out.push('\n');
        out.push_str(&patch_only);
        out.push_str(&format!("--{boundary}--\n"));
    } else {
        // Standard (non-MIME) patch format
        if !body.is_empty() {
            out.push_str(body);
            out.push('\n');
        }

        // Signoff trailer
        if opts.signoff {
            let (name, email) = get_signoff_identity(&commit.committer);
            let sob = if use_utf8_log {
                format!("{name} <{email}>")
            } else {
                encode_email_address_for_charset(&format!("{name} <{email}>"), &charset_label)
            };
            out.push_str(&format!("\nSigned-off-by: {sob}\n"));
        }

        out.push_str("---\n");
        out.push_str(&diff_text);
    }

    // base-commit info (appended to the last patch in the series)
    if include_base {
        if let Some(ref base_hex) = opts.base_commit {
            out.push_str(&format!("base-commit: {base_hex}\n"));
        }
    }

    out.push_str("-- \n");
    out.push_str("grit\n");
    out.push('\n');

    Ok(out)
}

/// Read blob content as UTF-8 string (empty for zero OID).
fn read_blob_content(odb: &Odb, oid: &ObjectId) -> String {
    if *oid == zero_oid() {
        return String::new();
    }
    match odb.read(oid) {
        Ok(obj) => String::from_utf8_lossy(&obj.data).into_owned(),
        Err(_) => String::new(),
    }
}

/// Write diff header to a string.
fn write_diff_header_to_string(
    out: &mut String,
    entry: &grit_lib::diff::DiffEntry,
    src_prefix: &str,
    dst_prefix: &str,
) {
    use grit_lib::diff::DiffStatus;
    use std::fmt::Write;

    let old_path = entry
        .old_path
        .as_deref()
        .unwrap_or(entry.new_path.as_deref().unwrap_or(""));
    let new_path = entry
        .new_path
        .as_deref()
        .unwrap_or(entry.old_path.as_deref().unwrap_or(""));

    if src_prefix.is_empty() && dst_prefix.is_empty() {
        let _ = writeln!(out, "diff --git {old_path} {new_path}");
    } else {
        let _ = writeln!(
            out,
            "diff --git {src_prefix}{old_path} {dst_prefix}{new_path}",
            src_prefix = src_prefix,
            dst_prefix = dst_prefix
        );
    }

    match entry.status {
        DiffStatus::Added => {
            let _ = writeln!(out, "new file mode {}", entry.new_mode);
            let old_abbrev = &entry.old_oid.to_hex()[..7];
            let new_abbrev = &entry.new_oid.to_hex()[..7];
            let _ = writeln!(out, "index {old_abbrev}..{new_abbrev}");
        }
        DiffStatus::Deleted => {
            let _ = writeln!(out, "deleted file mode {}", entry.old_mode);
            let old_abbrev = &entry.old_oid.to_hex()[..7];
            let new_abbrev = &entry.new_oid.to_hex()[..7];
            let _ = writeln!(out, "index {old_abbrev}..{new_abbrev}");
        }
        DiffStatus::Modified => {
            if entry.old_mode != entry.new_mode {
                let _ = writeln!(out, "old mode {}", entry.old_mode);
                let _ = writeln!(out, "new mode {}", entry.new_mode);
            }
            let old_abbrev = &entry.old_oid.to_hex()[..7];
            let new_abbrev = &entry.new_oid.to_hex()[..7];
            if entry.old_mode == entry.new_mode {
                let _ = writeln!(out, "index {old_abbrev}..{new_abbrev} {}", entry.old_mode);
            } else {
                let _ = writeln!(out, "index {old_abbrev}..{new_abbrev}");
            }
        }
        DiffStatus::Renamed => {
            let _ = writeln!(out, "similarity index 100%");
            let _ = writeln!(out, "rename from {old_path}");
            let _ = writeln!(out, "rename to {new_path}");
        }
        DiffStatus::Copied => {
            let _ = writeln!(out, "similarity index 100%");
            let _ = writeln!(out, "copy from {old_path}");
            let _ = writeln!(out, "copy to {new_path}");
        }
        DiffStatus::TypeChanged => {
            let _ = writeln!(out, "old mode {}", entry.old_mode);
            let _ = writeln!(out, "new mode {}", entry.new_mode);
        }
        DiffStatus::Unmerged => {}
    }
}

/// Build the `From:` mailbox for a patch from `format.from` / `--from`.
fn mailbox_for_from_header(commit: &CommitData, mode: &FromHeaderMode) -> String {
    match mode {
        FromHeaderMode::Omit => String::new(),
        FromHeaderMode::Author => format_ident(
            &crate::git_commit_encoding::decode_rfc2047_mailbox_from_line(&commit.author),
        ),
        FromHeaderMode::Committer => format_ident(
            &crate::git_commit_encoding::decode_rfc2047_mailbox_from_line(&commit.committer),
        ),
        FromHeaderMode::Custom(s) => {
            format_ident(&crate::git_commit_encoding::decode_rfc2047_mailbox_from_line(s))
        }
    }
}

/// Format an identity string as "Name <email>".
fn format_ident(ident: &str) -> String {
    if let Some(bracket) = ident.find('<') {
        if let Some(end) = ident.find('>') {
            let name = ident[..bracket].trim();
            let email = &ident[bracket..=end];
            return format!("{name} {email}");
        }
    }
    ident.to_owned()
}

/// Encode an email address for use in email headers.
///
/// Rules:
/// - If the display name contains non-ASCII chars → RFC 2047 encode it
/// - If the display name contains RFC 822 special chars (like `.`) → quote it
/// - Otherwise → use as-is
fn encode_email_address(addr: &str) -> String {
    // Parse "Display Name <email@example.com>" form
    if let (Some(lt), Some(gt)) = (addr.rfind('<'), addr.rfind('>')) {
        if lt < gt {
            let name = addr[..lt].trim();
            let email_part = &addr[lt..=gt]; // "<email>"
            if name.is_empty() {
                return addr.to_string();
            }
            let encoded_name = encode_display_name(name);
            return format!("{encoded_name} {email_part}");
        }
    }
    // No angle brackets — return as-is
    addr.to_string()
}

/// Charset token for RFC 2047 `=?charset?q?...?=` (matches Git test expectations).
fn rfc2047_charset_label(log_output_encoding: &str) -> String {
    let t = log_output_encoding.trim();
    let lower = t.to_ascii_lowercase();
    if lower == "utf-8" || lower == "utf8" {
        return "UTF-8".to_owned();
    }
    if matches!(
        lower.as_str(),
        "iso-8859-1" | "iso8859-1" | "latin1" | "latin-1"
    ) {
        return "ISO8859-1".to_owned();
    }
    t.to_owned()
}

/// Like [`encode_email_address`] but uses `charset_label` for RFC 2047 when non-ASCII.
fn encode_email_address_for_charset(addr: &str, charset_label: &str) -> String {
    if charset_label.eq_ignore_ascii_case("UTF-8") {
        return encode_email_address(addr);
    }
    if let (Some(lt), Some(gt)) = (addr.rfind('<'), addr.rfind('>')) {
        if lt < gt {
            let name = addr[..lt].trim();
            let email_part = &addr[lt..=gt];
            if name.is_empty() {
                return addr.to_string();
            }
            let encoded_name = encode_display_name_for_charset(name, charset_label);
            return format!("{encoded_name} {email_part}");
        }
    }
    addr.to_string()
}

fn encode_display_name_for_charset(name: &str, charset_label: &str) -> String {
    if charset_label.eq_ignore_ascii_case("UTF-8") {
        return encode_display_name(name);
    }
    if name.bytes().any(|b| b > 0x7f) {
        return rfc2047_encode_with_charset(name, charset_label);
    }
    let specials = |c: char| {
        matches!(
            c,
            '(' | ')' | '<' | '>' | '[' | ']' | ':' | ';' | '@' | '\\' | ',' | '.' | '"'
        )
    };
    if name.chars().any(specials) {
        let escaped = name.replace('\\', "\\\\").replace('"', "\\\"");
        return format!("\"{escaped}\"");
    }
    name.to_string()
}

fn rfc2047_encode_with_charset(name: &str, charset_label: &str) -> String {
    let bytes = if charset_label.eq_ignore_ascii_case("UTF-8") {
        name.as_bytes().to_vec()
    } else {
        match crate::git_commit_encoding::encode_unicode(charset_label, name) {
            Some(mut raw) => {
                while raw.last() == Some(&b'\n') {
                    raw.pop();
                }
                raw
            }
            None => return rfc2047_encode(name),
        }
    };
    let mut encoded = String::new();
    for &byte in &bytes {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' => {
                encoded.push(byte as char);
            }
            b' ' => encoded.push_str("=20"),
            _ => encoded.push_str(&format!("={byte:02X}")),
        }
    }
    format!("=?{charset_label}?q?{encoded}?=")
}

/// Encode a display name portion of an email address.
///
/// - Non-ASCII → RFC 2047 UTF-8 quoted-printable
/// - Contains RFC 822 specials → RFC 822 quoted string
/// - Otherwise → plain
fn encode_display_name(name: &str) -> String {
    // Check for non-ASCII
    if name.bytes().any(|b| b > 0x7f) {
        return rfc2047_encode(name);
    }
    // RFC 822 specials that require quoting
    // Specials are: ( ) < > [ ] : ; @ \ , . "
    let specials = |c: char| {
        matches!(
            c,
            '(' | ')' | '<' | '>' | '[' | ']' | ':' | ';' | '@' | '\\' | ',' | '.' | '"'
        )
    };
    if name.chars().any(specials) {
        // Quote the name
        let escaped = name.replace('\\', "\\\\").replace('"', "\\\"");
        return format!("\"{escaped}\"");
    }
    name.to_string()
}

/// RFC 2047 UTF-8 quoted-printable encoding for an email display name.
fn rfc2047_encode(name: &str) -> String {
    let mut encoded = String::new();
    for byte in name.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' => {
                encoded.push(*byte as char);
            }
            b' ' => {
                encoded.push_str("=20");
            }
            _ => {
                encoded.push_str(&format!("={:02X}", byte));
            }
        }
    }
    format!("=?UTF-8?q?{encoded}?=")
}

/// Write a folded email header with multiple values.
///
/// Emits:
/// ```
/// HeaderName: value1,
///  value2
/// ```
fn write_folded_header(out: &mut String, name: &str, values: &[String]) {
    if values.is_empty() {
        return;
    }
    out.push_str(name);
    out.push_str(": ");
    for (i, val) in values.iter().enumerate() {
        if i > 0 {
            out.push_str(",\n ");
        }
        out.push_str(val);
    }
    out.push('\n');
}

/// Extract date from identity string and format as RFC 2822-like.
fn format_date_rfc2822(ident: &str) -> String {
    // Git ident: "Name <email> timestamp offset"
    let parts: Vec<&str> = ident.rsplitn(3, ' ').collect();
    if parts.len() >= 2 {
        let ts_str = parts[1];
        let offset_str = parts[0];
        if let Ok(ts) = ts_str.parse::<i64>() {
            // Parse the offset string (e.g. "+0000", "-0700") into a UtcOffset
            let tz_offset = parse_tz_offset(offset_str).unwrap_or(time::UtcOffset::UTC);
            let dt = time::OffsetDateTime::from_unix_timestamp(ts)
                .unwrap_or(time::OffsetDateTime::UNIX_EPOCH)
                .to_offset(tz_offset);
            let format = time::format_description::parse(
                "[weekday repr:short], [day] [month repr:short] [year] [hour]:[minute]:[second] ",
            );
            if let Ok(fmt) = format {
                if let Ok(formatted) = dt.format(&fmt) {
                    return format!("{formatted}{offset_str}");
                }
            }
        }
        format!("{ts_str} {offset_str}")
    } else {
        ident.to_owned()
    }
}

fn parse_tz_offset(s: &str) -> Option<time::UtcOffset> {
    if s.len() != 5 {
        return None;
    }
    let sign: i8 = match s.as_bytes()[0] {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let hours: i8 = s[1..3].parse::<i8>().ok()?;
    let minutes: i8 = s[3..5].parse::<i8>().ok()?;
    time::UtcOffset::from_hms(sign * hours, sign * minutes, 0).ok()
}

/// Sanitize a subject line for use as a filename.
fn sanitize_subject_with_limit(subject: &str, max_len: Option<usize>) -> String {
    let limit = max_len.unwrap_or(64);
    let sanitized = sanitize_subject(subject);
    if sanitized.len() > limit {
        sanitized[..limit].trim_end_matches('-').to_owned()
    } else {
        sanitized
    }
}

fn sanitize_subject(subject: &str) -> String {
    subject
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_owned()
}
