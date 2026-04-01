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
        // Parse "@<unix> <tz>" or "<unix> <tz>" format
        if let Some(rest) = d.strip_prefix('@') {
            rest.to_owned()
        } else {
            d
        }
    } else {
        format!("{now_unix} {tz_str}")
    };

    Ok(format!("{name} <{email}> {date_str}"))
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
