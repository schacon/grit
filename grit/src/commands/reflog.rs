//! `grit reflog` — manage reflog entries.
//!
//! The reflog records when the tips of branches and other refs are updated.
//!
//! Subcommands:
//! - `show [ref]` (default) — display reflog entries
//! - `expire` — prune old reflog entries
//! - `delete` — delete specific reflog entries
//! - `exists` — check whether a ref has a reflog

use anyhow::{bail, Context, Result};
use clap::{Args as ClapArgs, Subcommand};

use grit_lib::config::ConfigSet;
use grit_lib::objects::ObjectId;
use grit_lib::reflog::{delete_reflog_entries, expire_reflog, read_reflog, reflog_exists};
use grit_lib::refs::{append_reflog, resolve_ref};
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

    /// Maximum number of entries to show (used when no subcommand is given).
    #[arg(short = 'n', long = "max-count")]
    pub max_count: Option<usize>,
}

#[derive(Debug, Subcommand)]
pub enum ReflogCommand {
    /// Show reflog entries (default subcommand).
    Show(ShowArgs),
    /// Prune old reflog entries.
    Expire(ExpireArgs),
    /// Delete specific reflog entries.
    Delete(DeleteArgs),
    /// Check whether a ref has a reflog.
    Exists(ExistsArgs),
    /// Manually write a reflog entry.
    Write(WriteArgs),
}

/// Arguments for `reflog show`.
#[derive(Debug, ClapArgs)]
pub struct ShowArgs {
    /// Reference name (default: HEAD).
    #[arg(default_value = "HEAD")]
    pub refname: String,

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

/// Arguments for `reflog expire`.
#[derive(Debug, ClapArgs)]
pub struct ExpireArgs {
    /// Expire entries older than this value. Use "all" to expire all, or a number of days.
    #[arg(long = "expire", default_value = "90")]
    pub expire: String,

    /// Process all refs, not just the named one.
    #[arg(long)]
    pub all: bool,

    /// Dry run: show what would be pruned.
    #[arg(short = 'n', long = "dry-run")]
    pub dry_run: bool,

    /// Reference name (default: HEAD).
    #[arg(value_name = "REF")]
    pub refname: Option<String>,
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

    /// Rewrite remaining entries to maintain predecessor links.
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
        Some(ReflogCommand::Expire(expire_args)) => run_expire(expire_args),
        Some(ReflogCommand::Delete(delete_args)) => run_delete(delete_args),
        Some(ReflogCommand::Exists(exists_args)) => run_exists(exists_args),
        Some(ReflogCommand::Write(write_args)) => run_write(write_args),
        None => {
            // Default to show
            let refname = args.default_ref.unwrap_or_else(|| "HEAD".to_string());
            run_show(ShowArgs {
                refname,
                max_count: args.max_count,
                no_abbrev_commit: false,
                abbrev_commit: false,
                format: None,
                date: None,
                walk_reflogs: false,
            })
        }
    }
}

fn run_show(args: ShowArgs) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let refname = resolve_refname(&repo, &args.refname)?;
    let display_name = display_refname(&refname);

    let entries = read_reflog(&repo.git_dir, &refname).map_err(|e| anyhow::anyhow!("{e}"))?;

    if entries.is_empty() {
        return Ok(());
    }

    // Entries are in file order (oldest first); display newest first.
    let iter = entries.iter().rev().enumerate();
    let max = args.max_count.unwrap_or(usize::MAX);

    for (i, entry) in iter {
        if i >= max {
            break;
        }
        let oid_str = if args.no_abbrev_commit {
            entry.new_oid.to_hex()
        } else {
            abbreviate_oid(&entry.new_oid, 7)
        };
        println!("{oid_str} {display_name}@{{{i}}}: {}", entry.message);
    }

    Ok(())
}

fn run_expire(args: ExpireArgs) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| anyhow::anyhow!("system time error: {e}"))?
        .as_secs() as i64;

    let expire_secs = if args.expire == "all" || args.expire == "0" || args.expire == "now" {
        Some(now) // expire all entries (now = everything is old enough)
    } else if let Ok(expire_days) = args.expire.parse::<u64>() {
        Some(now - (expire_days as i64 * 86400))
    } else if let Ok(ts) = args.expire.parse::<i64>() {
        Some(ts) // raw epoch
    } else {
        bail!("invalid expire value: '{}'", args.expire)
    };

    let refs_to_expire: Vec<String> = if args.all {
        grit_lib::reflog::list_reflog_refs(&repo.git_dir).map_err(|e| anyhow::anyhow!("{e}"))?
    } else {
        let refname = args.refname.as_deref().unwrap_or("HEAD");
        let resolved = resolve_refname(&repo, refname)?;
        vec![resolved]
    };

    for refname in &refs_to_expire {
        if args.dry_run {
            let entries =
                read_reflog(&repo.git_dir, refname).map_err(|e| anyhow::anyhow!("{e}"))?;
            let would_prune = entries
                .iter()
                .filter(|e| {
                    let ts = parse_ts_from_identity(&e.identity);
                    match (expire_secs, ts) {
                        (Some(cutoff), Some(t)) => t < cutoff,
                        (None, _) => true,
                        _ => false,
                    }
                })
                .count();
            if would_prune > 0 {
                eprintln!("would prune {would_prune} entries from {refname}");
            }
        } else {
            let pruned = expire_reflog(&repo.git_dir, refname, expire_secs)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            if pruned > 0 {
                eprintln!("pruned {pruned} entries from {refname}");
            }
        }
    }

    Ok(())
}

fn run_delete(args: DeleteArgs) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;

    // Parse entries like "HEAD@{2}" or "refs/heads/main@{0}"
    // Group by refname
    let mut grouped: std::collections::HashMap<String, Vec<usize>> =
        std::collections::HashMap::new();

    for spec in &args.entries {
        let (refname, index) = parse_reflog_spec(spec)?;
        let resolved = resolve_refname(&repo, &refname)?;
        grouped.entry(resolved).or_default().push(index);
    }

    for (refname, indices) in &grouped {
        if args.dry_run {
            for idx in indices {
                eprintln!("would delete {refname}@{{{idx}}}");
            }
        } else {
            // If --updateref, after deleting, update the ref to the new_oid
            // of whatever entry becomes the new @{0}
            if args.updateref {
                let entries =
                    read_reflog(&repo.git_dir, refname).map_err(|e| anyhow::anyhow!("{e}"))?;
                // Entries are oldest-first; indices are newest-first
                let mut reversed = entries.clone();
                reversed.reverse();
                // Figure out which entries will remain after deletion
                let indices_set: std::collections::HashSet<usize> =
                    indices.iter().copied().collect();
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
            delete_reflog_entries(&repo.git_dir, refname, indices)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
        }
    }

    Ok(())
}

fn run_exists(args: ExistsArgs) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let refname = resolve_refname(&repo, &args.refname)?;

    if reflog_exists(&repo.git_dir, &refname) {
        Ok(())
    } else {
        std::process::exit(1);
    }
}

fn run_write(args: WriteArgs) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;

    // Validate refname: reject spaces, control characters, and other invalid chars.
    if args.refname.contains(' ')
        || args.refname.contains('\t')
        || args.refname.contains("..")
        || args.refname.contains("\\")
        || args.refname.ends_with('.')
        || args.refname.ends_with('/')
        || args.refname.contains("@{")
        || args.refname.is_empty()
    {
        bail!("invalid refname: '{}'", args.refname);
    }

    let old_oid: ObjectId = args.old_oid.parse().context("invalid old object ID")?;
    let new_oid: ObjectId = args.new_oid.parse().context("invalid new object ID")?;

    let config = ConfigSet::load(Some(&repo.git_dir), true).ok();
    let name = std::env::var("GIT_COMMITTER_NAME")
        .ok()
        .or_else(|| config.as_ref().and_then(|c| c.get("user.name")))
        .unwrap_or_else(|| "Unknown".to_owned());
    let email = std::env::var("GIT_COMMITTER_EMAIL")
        .ok()
        .or_else(|| config.as_ref().and_then(|c| c.get("user.email")))
        .unwrap_or_default();
    let now = time::OffsetDateTime::now_utc();
    let epoch = now.unix_timestamp();
    let offset = now.offset();
    let hours = offset.whole_hours();
    let minutes = offset.minutes_past_hour().unsigned_abs();
    let identity = format!("{name} <{email}> {epoch} {hours:+03}{minutes:02}");

    append_reflog(
        &repo.git_dir,
        &args.refname,
        &old_oid,
        &new_oid,
        &identity,
        &args.message,
    )
    .map_err(|e| anyhow::anyhow!("{e}"))?;

    Ok(())
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

/// Format refname for display: `HEAD` stays, `refs/heads/main` stays.
fn display_refname(refname: &str) -> &str {
    refname
}

/// Abbreviate an OID to the given hex length.
fn abbreviate_oid(oid: &ObjectId, len: usize) -> String {
    let hex = oid.to_hex();
    hex[..len.min(hex.len())].to_string()
}

/// Parse a `ref@{n}` spec into (refname, index).
fn parse_reflog_spec(spec: &str) -> Result<(String, usize)> {
    let Some(at_pos) = spec.find("@{") else {
        bail!("invalid reflog entry spec: '{spec}' (expected ref@{{n}})");
    };
    let refname = &spec[..at_pos];
    let rest = &spec[at_pos + 2..];
    let Some(close) = rest.find('}') else {
        bail!("invalid reflog entry spec: '{spec}' (missing closing braces)");
    };
    let index: usize = rest[..close]
        .parse()
        .context(format!("invalid index in '{spec}'"))?;
    Ok((refname.to_string(), index))
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
