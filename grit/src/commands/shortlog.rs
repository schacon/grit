//! `grit shortlog` — summarize git log output by author.
//!
//! Groups commits by author (or committer) and shows a count with commit
//! subjects, similar to `git shortlog`.

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use grit_lib::objects::{parse_commit, ObjectId};
use grit_lib::odb::Odb;
use grit_lib::repo::Repository;
use grit_lib::state::resolve_head;
use std::collections::{HashMap, HashSet};
use std::io::{self, BufRead, IsTerminal, Write};

/// Arguments for `grit shortlog`.
#[derive(Debug, ClapArgs)]
#[command(about = "Summarize git log output")]
pub struct Args {
    /// Revisions to summarize (defaults to HEAD).
    #[arg()]
    pub revisions: Vec<String>,

    /// Sort by number of commits per author (descending).
    #[arg(short = 'n', long = "numbered")]
    pub numbered: bool,

    /// Only show commit count per author (suppress subjects).
    #[arg(short = 's', long = "summary")]
    pub summary: bool,

    /// Show email address of each author.
    #[arg(short = 'e', long = "email")]
    pub email: bool,

    /// Format each commit description (default: commit subject).
    #[arg(long = "format")]
    pub format: Option<String>,

    /// Group by: author (default) or committer.
    #[arg(long = "group", default_value = "author")]
    pub group: String,
}

/// A single entry: group key → list of formatted commit descriptions.
struct ShortlogEntry {
    key: String,
    commits: Vec<String>,
}

/// Run the `shortlog` command.
pub fn run(args: Args) -> Result<()> {
    let stdin = io::stdin();
    let use_stdin = !stdin.is_terminal() && args.revisions.is_empty();

    let (entries, from_stdin) = if use_stdin {
        (collect_from_stdin(&stdin, &args)?, true)
    } else {
        (collect_from_repo(&args)?, false)
    };

    let mut grouped = group_entries(entries, from_stdin);

    // Sort
    if args.numbered {
        // By count descending, then alphabetical for ties
        grouped.sort_by(|a, b| {
            b.commits
                .len()
                .cmp(&a.commits.len())
                .then_with(|| a.key.to_lowercase().cmp(&b.key.to_lowercase()))
        });
    } else {
        // Alphabetical by key (case-insensitive)
        grouped.sort_by(|a, b| a.key.to_lowercase().cmp(&b.key.to_lowercase()));
    }

    let stdout = io::stdout();
    let mut out = stdout.lock();

    for (i, entry) in grouped.iter().enumerate() {
        if i > 0 && !args.summary {
            writeln!(out)?;
        }
        if args.summary {
            writeln!(out, "{:>6}\t{}", entry.commits.len(), entry.key)?;
        } else {
            writeln!(out, "{} ({}):", entry.key, entry.commits.len())?;
            for desc in &entry.commits {
                writeln!(out, "      {desc}")?;
            }
        }
    }

    Ok(())
}

/// Collect commits from the repository by walking the commit graph.
fn collect_from_repo(args: &Args) -> Result<Vec<(String, String)>> {
    let repo = Repository::discover(None).context("not a git repository")?;

    // Pre-process revisions: expand --glob and --glob=<pattern>
    let mut expanded_revs = Vec::new();
    let mut i = 0;
    while i < args.revisions.len() {
        let rev = &args.revisions[i];
        if let Some(pattern) = rev.strip_prefix("--glob=") {
            let full = normalize_shortlog_glob(pattern);
            let matching = grit_lib::refs::list_refs_glob(&repo.git_dir, &full).unwrap_or_default();
            for (_, oid) in matching {
                expanded_revs.push(oid.to_hex());
            }
        } else if rev == "--glob" {
            i += 1;
            if let Some(pattern) = args.revisions.get(i) {
                let full = normalize_shortlog_glob(pattern);
                let matching =
                    grit_lib::refs::list_refs_glob(&repo.git_dir, &full).unwrap_or_default();
                for (_, oid) in matching {
                    expanded_revs.push(oid.to_hex());
                }
            }
        } else {
            expanded_revs.push(rev.clone());
        }
        i += 1;
    }

    let start_oids = if expanded_revs.is_empty() {
        let head = resolve_head(&repo.git_dir)?;
        match head.oid() {
            Some(oid) => vec![*oid],
            None => return Ok(vec![]),
        }
    } else {
        let mut oids = Vec::new();
        for rev in &expanded_revs {
            let oid = resolve_revision(&repo, rev)?;
            oids.push(oid);
        }
        oids
    };

    let commits = walk_commits(&repo.odb, &start_oids)?;

    let mut result = Vec::new();
    for (_oid, author, committer, message) in &commits {
        let ident = match args.group.as_str() {
            "committer" => committer,
            _ => author,
        };
        let key = format_key(ident, args.email);
        let desc = format_description(message, args.format.as_deref());
        result.push((key, desc));
    }

    Ok(result)
}

/// Collect commits from stdin (piped from `git log`).
///
/// Supports two input formats:
/// 1. `git log --format=raw` — lines starting with "commit ", "author ", etc.
/// 2. Default `git log` output — "commit <hash>", "Author: ...", blank line, indented message
fn collect_from_stdin(stdin: &io::Stdin, args: &Args) -> Result<Vec<(String, String)>> {
    let reader = stdin.lock();
    let mut result = Vec::new();

    let mut current_author = String::new();
    let mut current_committer = String::new();
    let mut current_message = String::new();
    let mut in_message = false;
    let mut has_commit = false;
    let mut raw_format = false;

    for line in reader.lines() {
        let line = line?;

        // Detect raw format (has "tree " line after "commit " line)
        if line.starts_with("commit ") {
            // Flush previous commit
            if has_commit {
                let ident = match args.group.as_str() {
                    "committer" => &current_committer,
                    _ => &current_author,
                };
                let key = format_key(ident, args.email);
                let desc = format_description(current_message.trim(), args.format.as_deref());
                result.push((key, desc));
            }
            has_commit = true;
            current_author.clear();
            current_committer.clear();
            current_message.clear();
            in_message = false;
            continue;
        }

        if line.starts_with("tree ") {
            raw_format = true;
            continue;
        }

        if line.starts_with("parent ") {
            continue;
        }

        if line.starts_with("author ") && raw_format {
            current_author = line[7..].to_owned();
            continue;
        }

        if line.starts_with("committer ") && raw_format {
            current_committer = line[10..].to_owned();
            continue;
        }

        // Standard log format: "Author: Name <email>"
        if line.starts_with("Author:") {
            let val = line[7..].trim();
            // Convert "Name <email>" to raw ident format "Name <email> 0 +0000"
            current_author = val.to_owned();
            continue;
        }

        // Standard log format: "Commit: Name <email>" (for fuller format)
        if line.starts_with("Commit:") {
            let val = line[7..].trim();
            current_committer = val.to_owned();
            continue;
        }

        // Date line — skip
        if line.starts_with("Date:")
            || line.starts_with("AuthorDate:")
            || line.starts_with("CommitDate:")
        {
            continue;
        }

        if has_commit {
            if line.is_empty() && !in_message {
                in_message = true;
                continue;
            }
            if in_message {
                let trimmed = if line.starts_with("    ") {
                    &line[4..]
                } else {
                    &line
                };
                if !current_message.is_empty() {
                    current_message.push('\n');
                }
                current_message.push_str(trimmed);
            }
        }
    }

    // Flush last commit
    if has_commit {
        let ident = match args.group.as_str() {
            "committer" => &current_committer,
            _ => &current_author,
        };
        let key = format_key(ident, args.email);
        let desc = format_description(current_message.trim(), args.format.as_deref());
        result.push((key, desc));
    }

    Ok(result)
}

/// Group entries by key, preserving commit order within each group.
/// If `reverse_within` is true, reverse entries within each group (for stdin input
/// where commits arrive newest-first).
fn group_entries(entries: Vec<(String, String)>, reverse_within: bool) -> Vec<ShortlogEntry> {
    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    let mut order: Vec<String> = Vec::new();

    for (key, desc) in entries {
        if !map.contains_key(&key) {
            order.push(key.clone());
        }
        map.entry(key).or_default().push(desc);
    }

    order
        .into_iter()
        .map(|key| {
            let mut commits = map.remove(&key).unwrap_or_default();
            if reverse_within {
                commits.reverse();
            }
            ShortlogEntry { key, commits }
        })
        .collect()
}

/// Format the grouping key from an ident string.
fn format_key(ident: &str, show_email: bool) -> String {
    let name = extract_name(ident);
    if show_email {
        let email = extract_email(ident);
        if email.is_empty() {
            name
        } else {
            format!("{name} <{email}>")
        }
    } else {
        name
    }
}

/// Format a commit description line. Default is the first line (subject).
fn format_description(message: &str, format: Option<&str>) -> String {
    match format {
        Some(fmt) => {
            // Simple format: just replace %s with subject
            let subject = message.lines().next().unwrap_or("");
            fmt.replace("%s", subject)
        }
        None => {
            // Default: first line of the message (subject)
            let subject = message.lines().next().unwrap_or("").to_owned();
            // Strip [PATCH] prefix as git does
            if let Some(rest) = subject.strip_prefix("[PATCH] ") {
                rest.to_owned()
            } else {
                subject
            }
        }
    }
}

/// Extract the name portion from a Git ident string.
fn extract_name(ident: &str) -> String {
    if let Some(bracket) = ident.find('<') {
        ident[..bracket].trim().to_owned()
    } else {
        // Might be just a name without email
        // Strip trailing timestamp parts if present (raw format: "Name <email> ts tz")
        ident.trim().to_owned()
    }
}

/// Extract the email portion from a Git ident string.
fn extract_email(ident: &str) -> String {
    if let Some(start) = ident.find('<') {
        if let Some(end) = ident.find('>') {
            return ident[start + 1..end].to_owned();
        }
    }
    String::new()
}

/// Walk the commit graph, returning (oid, author, committer, message).
fn walk_commits(odb: &Odb, start: &[ObjectId]) -> Result<Vec<(ObjectId, String, String, String)>> {
    let mut visited = HashSet::new();
    let mut queue: Vec<ObjectId> = start.to_vec();
    let mut result = Vec::new();

    while let Some(oid) = queue.pop() {
        if !visited.insert(oid) {
            continue;
        }

        let obj = odb.read(&oid)?;
        let commit = parse_commit(&obj.data)?;

        result.push((
            oid,
            commit.author.clone(),
            commit.committer.clone(),
            commit.message.clone(),
        ));

        for parent in commit.parents.iter().rev() {
            if !visited.contains(parent) {
                queue.push(*parent);
            }
        }
    }

    // Sort by committer timestamp descending first to get topo order,
    // then reverse so oldest commits come first (git shortlog order).
    result.sort_by(|a, b| {
        let ts_a = extract_timestamp(&a.2);
        let ts_b = extract_timestamp(&b.2);
        ts_b.cmp(&ts_a)
    });
    result.reverse();

    Ok(result)
}

/// Extract unix timestamp from an ident line.
fn extract_timestamp(ident: &str) -> i64 {
    let parts: Vec<&str> = ident.rsplitn(3, ' ').collect();
    if parts.len() >= 2 {
        parts[1].parse().unwrap_or(0)
    } else {
        0
    }
}

/// Resolve a revision string to an ObjectId.
fn resolve_revision(repo: &Repository, rev: &str) -> Result<ObjectId> {
    if let Ok(oid) = ObjectId::from_hex(rev) {
        return Ok(oid);
    }

    if rev == "HEAD" {
        let head = resolve_head(&repo.git_dir)?;
        if let Some(oid) = head.oid() {
            return Ok(*oid);
        }
    }

    // Try refs/heads/<rev>
    let ref_path = repo.git_dir.join("refs/heads").join(rev);
    if let Ok(content) = std::fs::read_to_string(&ref_path) {
        if let Ok(oid) = ObjectId::from_hex(content.trim()) {
            return Ok(oid);
        }
    }

    // Try refs/tags/<rev>
    let tag_path = repo.git_dir.join("refs/tags").join(rev);
    if let Ok(content) = std::fs::read_to_string(&tag_path) {
        if let Ok(oid) = ObjectId::from_hex(content.trim()) {
            return Ok(oid);
        }
    }

    anyhow::bail!("unknown revision '{rev}'");
}

fn normalize_shortlog_glob(pattern: &str) -> String {
    let full = if pattern.starts_with("refs/") {
        pattern.to_owned()
    } else {
        format!("refs/{pattern}")
    };
    if !full.contains('*') && !full.contains('?') && !full.contains('[') {
        format!("{full}/*")
    } else {
        full
    }
}
