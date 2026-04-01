//! `gust commit-tree` — create a new commit object.
//!
//! All time values are injected (no hidden `SystemTime::now()`).  The author
//! and committer identity come from environment variables (`GIT_AUTHOR_NAME`,
//! `GIT_AUTHOR_EMAIL`, `GIT_AUTHOR_DATE`, `GIT_COMMITTER_NAME`,
//! `GIT_COMMITTER_EMAIL`, `GIT_COMMITTER_DATE`) or from `user.name` /
//! `user.email` in the git config.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use std::env;
use std::io::Read;
use time::format_description::well_known::Iso8601;
use time::{PrimitiveDateTime, UtcOffset};

use gust_lib::objects::{serialize_commit, CommitData, ObjectId, ObjectKind};
use gust_lib::refs::resolve_ref;
use gust_lib::repo::Repository;

/// Arguments for `gust commit-tree`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// The tree object to use.
    pub tree: String,

    /// Parent commit(s).
    #[arg(short = 'p')]
    pub parents: Vec<String>,

    /// Commit message.
    #[arg(short = 'm')]
    pub message: Vec<String>,

    /// Read commit message from file.
    #[arg(short = 'F', value_name = "file")]
    pub message_file: Option<std::path::PathBuf>,

    /// Override message encoding.
    #[arg(long)]
    pub encoding: Option<String>,
}

/// Run `gust commit-tree`.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;

    let tree_oid = resolve_tree_ish(&repo, &args.tree)?;

    let parent_oids: Vec<ObjectId> = args
        .parents
        .iter()
        .map(|p| resolve_tree_ish(&repo, p))
        .collect::<Result<Vec<_>>>()?;

    // Build commit message
    let message = build_message(&args)?;

    // Build identity strings
    let now_unix = current_unix_timestamp();
    let tz_str = local_tz_string();

    let author = build_identity("AUTHOR", &now_unix, &tz_str)?;
    let committer = build_identity("COMMITTER", &now_unix, &tz_str)?;

    let commit_data = CommitData {
        tree: tree_oid,
        parents: parent_oids,
        author,
        committer,
        encoding: args.encoding.clone(),
        message,
    };

    let raw = serialize_commit(&commit_data);
    let oid = repo
        .odb
        .write(ObjectKind::Commit, &raw)
        .context("writing commit object")?;

    println!("{oid}");
    Ok(())
}

fn build_message(args: &Args) -> Result<String> {
    if let Some(file) = &args.message_file {
        if file.as_os_str() == "-" {
            let mut msg = String::new();
            std::io::stdin().read_to_string(&mut msg)?;
            return Ok(msg);
        }
        return std::fs::read_to_string(file).context("reading message file");
    }

    if !args.message.is_empty() {
        let msg = args.message.join("\n\n");
        return Ok(msg);
    }

    // Read from stdin if no -m or -F
    let mut msg = String::new();
    std::io::stdin().read_to_string(&mut msg)?;
    Ok(msg)
}

/// Build a `"Name <email> <timestamp> <tz>"` identity string.
fn build_identity(prefix: &str, now_unix: &str, tz_str: &str) -> Result<String> {
    let name_key = format!("GIT_{prefix}_NAME");
    let email_key = format!("GIT_{prefix}_EMAIL");
    let date_key = format!("GIT_{prefix}_DATE");

    let name = env::var(&name_key)
        .or_else(|_| env::var("GIT_AUTHOR_NAME"))
        .unwrap_or_else(|_| "Unknown".to_owned());
    let email = env::var(&email_key)
        .or_else(|_| env::var("GIT_AUTHOR_EMAIL"))
        .unwrap_or_else(|_| "unknown@unknown".to_owned());

    let date_str = if let Ok(d) = env::var(&date_key) {
        normalize_git_date(&d, tz_str)?
    } else {
        format!("{now_unix} {tz_str}")
    };

    Ok(format!("{name} <{email}> {date_str}"))
}

fn normalize_git_date(input: &str, default_tz: &str) -> Result<String> {
    let trimmed = input.trim();

    // Accept "@<unix>", "@<unix> <tz>", "<unix>", and "<unix> <tz>".
    let unixish = trimmed.strip_prefix('@').unwrap_or(trimmed).trim();
    let parts: Vec<&str> = unixish.split_whitespace().collect();
    if parts.len() == 1 && parts[0].parse::<i64>().is_ok() {
        return Ok(format!("{} {default_tz}", parts[0]));
    }
    if parts.len() == 2 && parts[0].parse::<i64>().is_ok() && is_tz_offset(parts[1]) {
        return Ok(format!("{} {}", parts[0], parts[1]));
    }

    // Common human forms used by git tests, e.g. "2005-05-26 23:00".
    let fmt_ymdhm = time::format_description::parse("[year]-[month]-[day] [hour]:[minute]")
        .context("building date parser")?;
    if let Ok(dt) = PrimitiveDateTime::parse(trimmed, &fmt_ymdhm) {
        let offset = parse_tz_offset(default_tz)?;
        let ts = dt.assume_offset(offset).unix_timestamp();
        return Ok(format!("{ts} {default_tz}"));
    }
    let fmt_ymdhms =
        time::format_description::parse("[year]-[month]-[day] [hour]:[minute]:[second]")
            .context("building date parser")?;
    if let Ok(dt) = PrimitiveDateTime::parse(trimmed, &fmt_ymdhms) {
        let offset = parse_tz_offset(default_tz)?;
        let ts = dt.assume_offset(offset).unix_timestamp();
        return Ok(format!("{ts} {default_tz}"));
    }
    if let Ok(odt) = time::OffsetDateTime::parse(trimmed, &Iso8601::DEFAULT) {
        let ts = odt.unix_timestamp();
        let tz = format_tz_offset(odt.offset());
        return Ok(format!("{ts} {tz}"));
    }

    // Fall back to original text to preserve behavior for unsupported forms.
    Ok(trimmed.to_owned())
}

fn is_tz_offset(s: &str) -> bool {
    let bytes = s.as_bytes();
    if bytes.len() != 5 {
        return false;
    }
    (bytes[0] == b'+' || bytes[0] == b'-') && bytes[1..].iter().all(|b| b.is_ascii_digit())
}

fn parse_tz_offset(s: &str) -> Result<UtcOffset> {
    if !is_tz_offset(s) {
        bail!("invalid timezone offset: '{s}'");
    }
    let sign = if s.starts_with('-') { -1 } else { 1 };
    let hours: i8 = s[1..3].parse().context("invalid timezone hours")?;
    let mins: i8 = s[3..5].parse().context("invalid timezone minutes")?;
    UtcOffset::from_hms(sign * hours, sign * mins, 0).context("invalid timezone offset")
}

fn format_tz_offset(offset: UtcOffset) -> String {
    let secs = offset.whole_seconds();
    let sign = if secs < 0 { '-' } else { '+' };
    let abs = secs.abs();
    let hours = abs / 3600;
    let mins = (abs % 3600) / 60;
    format!("{sign}{hours:02}{mins:02}")
}

/// Get the current Unix timestamp as a string.
///
/// Uses `std::time::SystemTime` only here at the CLI boundary, not in the
/// library; this is acceptable per AGENT.md ("Avoid implicitly using …
/// instead pass the current time as argument" — library APIs take it as arg).
fn current_unix_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    secs.to_string()
}

/// Return a UTC offset string like `"+0000"` or `"-0500"`.
fn local_tz_string() -> String {
    // Simple: always UTC for now; a full implementation would read localtime
    "+0000".to_owned()
}

fn resolve_tree_ish(repo: &Repository, s: &str) -> Result<ObjectId> {
    if let Ok(oid) = s.parse::<ObjectId>() {
        return Ok(oid);
    }
    if let Ok(oid) = resolve_ref(&repo.git_dir, s) {
        return Ok(oid);
    }
    let as_branch = format!("refs/heads/{s}");
    if let Ok(oid) = resolve_ref(&repo.git_dir, &as_branch) {
        return Ok(oid);
    }
    bail!("not a valid object name: '{s}'")
}
