//! `grit whatchanged` — like `git log` but shows raw diff output.
//!
//! Equivalent to `git log --raw --no-merges`.  Reuses the existing log
//! infrastructure and appends diff-tree raw output for each commit.

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use grit_lib::diff::{diff_trees, format_raw};
use grit_lib::objects::{parse_commit, ObjectId};
use grit_lib::odb::Odb;
use grit_lib::repo::Repository;
use grit_lib::state::resolve_head;
use std::collections::HashSet;
use std::io::{self, Write};

/// Arguments for `grit whatchanged`.
#[derive(Debug, ClapArgs)]
#[command(about = "Show logs with raw diff output (no merges)")]
pub struct Args {
    /// Revisions to start from (defaults to HEAD).
    #[arg()]
    pub revisions: Vec<String>,

    /// Limit the number of commits to show.
    #[arg(short = 'n', long = "max-count")]
    pub max_count: Option<usize>,
}

/// Run the `whatchanged` command.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;

    let start_oids = if args.revisions.is_empty() {
        let head = resolve_head(&repo.git_dir)?;
        match head.oid() {
            Some(oid) => vec![*oid],
            None => return Ok(()),
        }
    } else {
        let mut oids = Vec::new();
        for rev in &args.revisions {
            let oid = grit_lib::rev_parse::resolve_revision(&repo, rev)?;
            oids.push(oid);
        }
        oids
    };

    let commits = walk_commits_no_merges(&repo.odb, &start_oids, args.max_count)?;

    let stdout = io::stdout();
    let mut out = stdout.lock();

    for (oid, parents, author, _committer, message) in &commits {
        let hex = oid.to_hex();
        writeln!(out, "commit {hex}")?;
        writeln!(out, "Author: {}", format_ident(author))?;
        writeln!(out, "Date:   {}", format_date(author))?;
        writeln!(out)?;
        for line in message.lines() {
            writeln!(out, "    {line}")?;
        }
        writeln!(out)?;

        // Show raw diff against first parent (or empty tree for root commits)
        let parent_tree = if let Some(parent_oid) = parents.first() {
            let parent_obj = repo.odb.read(parent_oid)?;
            let parent_commit = parse_commit(&parent_obj.data)?;
            Some(parent_commit.tree)
        } else {
            None
        };

        let obj = repo.odb.read(oid)?;
        let commit = parse_commit(&obj.data)?;

        let entries = if let Some(pt) = parent_tree {
            diff_trees(&repo.odb, Some(&pt), Some(&commit.tree), "")?
        } else {
            diff_trees(&repo.odb, None, Some(&commit.tree), "")?
        };

        for entry in &entries {
            writeln!(out, "{}", format_raw(entry))?;
        }
        writeln!(out)?;
    }

    Ok(())
}

/// Walk commits, skipping merges (commits with >1 parent).
fn walk_commits_no_merges(
    odb: &Odb,
    start: &[ObjectId],
    max_count: Option<usize>,
) -> Result<Vec<(ObjectId, Vec<ObjectId>, String, String, String)>> {
    let mut visited = HashSet::new();
    let mut queue: Vec<ObjectId> = start.to_vec();
    let mut result = Vec::new();

    while let Some(oid) = queue.pop() {
        if !visited.insert(oid) {
            continue;
        }

        let obj = odb.read(&oid)?;
        let commit = parse_commit(&obj.data)?;

        // Skip merge commits (>1 parent)
        if commit.parents.len() <= 1 {
            result.push((
                oid,
                commit.parents.clone(),
                commit.author.clone(),
                commit.committer.clone(),
                commit.message.clone(),
            ));
            if let Some(max) = max_count {
                if result.len() >= max {
                    break;
                }
            }
        }

        for parent in commit.parents.iter().rev() {
            if !visited.contains(parent) {
                queue.push(*parent);
            }
        }
    }

    // Sort by committer timestamp descending
    result.sort_by(|a, b| {
        let ts_a = extract_timestamp(&a.3);
        let ts_b = extract_timestamp(&b.3);
        ts_b.cmp(&ts_a)
    });

    Ok(result)
}

fn extract_timestamp(ident: &str) -> i64 {
    let parts: Vec<&str> = ident.rsplitn(3, ' ').collect();
    if parts.len() >= 2 {
        parts[1].parse().unwrap_or(0)
    } else {
        0
    }
}

fn format_ident(ident: &str) -> String {
    if let Some(bracket) = ident.find('<') {
        let name = ident[..bracket].trim();
        if let Some(end) = ident.find('>') {
            let email = &ident[bracket + 1..end];
            return format!("{name} <{email}>");
        }
    }
    ident.to_owned()
}

fn format_date(ident: &str) -> String {
    let parts: Vec<&str> = ident.rsplitn(3, ' ').collect();
    if parts.len() >= 2 {
        let ts_str = parts[1];
        let offset = parts[0];
        if let Ok(ts) = ts_str.parse::<i64>() {
            let dt = time::OffsetDateTime::from_unix_timestamp(ts)
                .unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
            let format = time::format_description::parse(
                "[weekday repr:short] [month repr:short] [day] [hour]:[minute]:[second] [year]",
            );
            if let Ok(fmt) = format {
                if let Ok(formatted) = dt.format(&fmt) {
                    return format!("{formatted} {offset}");
                }
            }
        }
        format!("{ts_str} {offset}")
    } else {
        ident.to_owned()
    }
}
