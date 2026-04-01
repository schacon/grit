//! `grit log` — show commit logs.
//!
//! Displays the commit history starting from HEAD (or specified revisions),
//! with configurable formatting and filtering.

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use grit_lib::objects::{parse_commit, ObjectId};
use grit_lib::odb::Odb;
use grit_lib::repo::Repository;
use grit_lib::state::{resolve_head, HeadState};
use std::collections::HashSet;
use std::io::{self, Write};

/// Arguments for `grit log`.
#[derive(Debug, ClapArgs)]
#[command(about = "Show commit logs")]
pub struct Args {
    /// Revisions to start from (defaults to HEAD).
    #[arg()]
    pub revisions: Vec<String>,

    /// Limit the number of commits to show.
    #[arg(short = 'n', long = "max-count")]
    pub max_count: Option<usize>,

    /// Show only one line per commit.
    #[arg(long = "oneline")]
    pub oneline: bool,

    /// Pretty-print format.
    #[arg(long = "format", alias = "pretty")]
    pub format: Option<String>,

    /// Show in reverse order.
    #[arg(long = "reverse")]
    pub reverse: bool,

    /// Follow only the first parent of merge commits.
    #[arg(long = "first-parent")]
    pub first_parent: bool,

    /// Show a graph of the commit history.
    #[arg(long = "graph")]
    pub graph: bool,

    /// Decorate refs.
    #[arg(long = "decorate")]
    pub decorate: Option<Option<String>>,

    /// Do not decorate refs.
    #[arg(long = "no-decorate")]
    pub no_decorate: bool,

    /// Skip this many commits.
    #[arg(long = "skip")]
    pub skip: Option<usize>,
}

/// Run the `log` command.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;

    // Determine starting points
    let start_oids = if args.revisions.is_empty() {
        let head = resolve_head(&repo.git_dir)?;
        match head.oid() {
            Some(oid) => vec![*oid],
            None => return Ok(()), // Unborn branch — nothing to show
        }
    } else {
        let mut oids = Vec::new();
        for rev in &args.revisions {
            let oid = resolve_revision(&repo, rev)?;
            oids.push(oid);
        }
        oids
    };

    // Collect ref decorations
    let decorations = if args.no_decorate {
        None
    } else {
        Some(collect_decorations(&repo)?)
    };

    // Walk commits
    let commits = walk_commits(
        &repo.odb,
        &start_oids,
        args.max_count,
        args.skip,
        args.first_parent,
    )?;

    let commits = if args.reverse {
        commits.into_iter().rev().collect::<Vec<_>>()
    } else {
        commits
    };

    let stdout = io::stdout();
    let mut out = stdout.lock();

    for (oid, commit_data) in &commits {
        format_commit(&mut out, oid, commit_data, &args, decorations.as_ref())?;
    }

    Ok(())
}

/// Parsed commit with its OID.
struct CommitInfo {
    tree: ObjectId,
    parents: Vec<ObjectId>,
    author: String,
    committer: String,
    message: String,
}

/// Walk the commit graph in reverse chronological order.
fn walk_commits(
    odb: &Odb,
    start: &[ObjectId],
    max_count: Option<usize>,
    skip: Option<usize>,
    first_parent: bool,
) -> Result<Vec<(ObjectId, CommitInfo)>> {
    let mut visited = HashSet::new();
    let mut queue: Vec<ObjectId> = start.to_vec();
    let mut result = Vec::new();
    let mut skipped = 0;
    let skip_n = skip.unwrap_or(0);

    while let Some(oid) = queue.pop() {
        if !visited.insert(oid) {
            continue;
        }

        let obj = odb.read(&oid)?;
        let commit = parse_commit(&obj.data)?;

        let info = CommitInfo {
            tree: commit.tree,
            parents: commit.parents.clone(),
            author: commit.author.clone(),
            committer: commit.committer.clone(),
            message: commit.message.clone(),
        };

        if skipped < skip_n {
            skipped += 1;
        } else {
            result.push((oid, info));
            if let Some(max) = max_count {
                if result.len() >= max {
                    break;
                }
            }
        }

        // Add parents to queue
        if first_parent {
            if let Some(parent) = commit.parents.first() {
                queue.push(*parent);
            }
        } else {
            // Add parents in reverse order so first parent is processed first
            for parent in commit.parents.iter().rev() {
                if !visited.contains(parent) {
                    queue.push(*parent);
                }
            }
        }
    }

    // Sort by commit timestamp (author date) — descending
    result.sort_by(|a, b| {
        let ts_a = extract_timestamp(&a.1.committer);
        let ts_b = extract_timestamp(&b.1.committer);
        ts_b.cmp(&ts_a)
    });

    Ok(result)
}

/// Extract unix timestamp from an author/committer line.
fn extract_timestamp(ident: &str) -> i64 {
    // Format: "Name <email> timestamp offset"
    let parts: Vec<&str> = ident.rsplitn(3, ' ').collect();
    if parts.len() >= 2 {
        parts[1].parse().unwrap_or(0)
    } else {
        0
    }
}

/// Format and print a single commit.
fn format_commit(
    out: &mut impl Write,
    oid: &ObjectId,
    info: &CommitInfo,
    args: &Args,
    decorations: Option<&std::collections::HashMap<String, Vec<String>>>,
) -> Result<()> {
    let hex = oid.to_hex();

    if args.oneline || args.format.as_deref() == Some("oneline") {
        let first_line = info.message.lines().next().unwrap_or("");
        let dec = format_decoration(&hex, decorations);
        writeln!(out, "{}{} {}", &hex[..7], dec, first_line)?;
        return Ok(());
    }

    let format = args.format.as_deref();

    match format {
        Some(fmt) if fmt.starts_with("format:") || fmt.starts_with("tformat:") => {
            let template = fmt
                .strip_prefix("format:")
                .or_else(|| fmt.strip_prefix("tformat:"))
                .unwrap_or(fmt);
            let formatted = apply_format_string(template, oid, info);
            writeln!(out, "{formatted}")?;
        }
        Some("short") => {
            let dec = format_decoration(&hex, decorations);
            writeln!(out, "commit {hex}{dec}")?;
            let author_name = extract_name(&info.author);
            writeln!(out, "Author: {author_name}")?;
            writeln!(out)?;
            for line in info.message.lines().take(1) {
                writeln!(out, "    {line}")?;
            }
            writeln!(out)?;
        }
        Some("medium") | None => {
            let dec = format_decoration(&hex, decorations);
            writeln!(out, "commit {hex}{dec}")?;
            writeln!(out, "Author: {}", format_ident_display(&info.author))?;
            writeln!(out, "Date:   {}", format_date(&info.author))?;
            writeln!(out)?;
            for line in info.message.lines() {
                writeln!(out, "    {line}")?;
            }
            writeln!(out)?;
        }
        Some("full") => {
            let dec = format_decoration(&hex, decorations);
            writeln!(out, "commit {hex}{dec}")?;
            writeln!(out, "Author: {}", format_ident_display(&info.author))?;
            writeln!(out, "Commit: {}", format_ident_display(&info.committer))?;
            writeln!(out)?;
            for line in info.message.lines() {
                writeln!(out, "    {line}")?;
            }
            writeln!(out)?;
        }
        Some("fuller") => {
            let dec = format_decoration(&hex, decorations);
            writeln!(out, "commit {hex}{dec}")?;
            writeln!(out, "Author:     {}", format_ident_display(&info.author))?;
            writeln!(out, "AuthorDate: {}", format_date(&info.author))?;
            writeln!(out, "Commit:     {}", format_ident_display(&info.committer))?;
            writeln!(out, "CommitDate: {}", format_date(&info.committer))?;
            writeln!(out)?;
            for line in info.message.lines() {
                writeln!(out, "    {line}")?;
            }
            writeln!(out)?;
        }
        Some(other) => {
            // Try as a format string directly
            let formatted = apply_format_string(other, oid, info);
            writeln!(out, "{formatted}")?;
        }
    }

    Ok(())
}

/// Apply a format string with placeholders like %H, %h, %s, %an, %ae, etc.
fn apply_format_string(template: &str, oid: &ObjectId, info: &CommitInfo) -> String {
    let hex = oid.to_hex();
    let mut result = String::with_capacity(template.len());
    let mut chars = template.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '%' {
            match chars.peek() {
                Some('H') => {
                    chars.next();
                    result.push_str(&hex);
                }
                Some('h') => {
                    chars.next();
                    result.push_str(&hex[..7.min(hex.len())]);
                }
                Some('T') => {
                    chars.next();
                    result.push_str(&info.tree.to_hex());
                }
                Some('t') => {
                    chars.next();
                    result.push_str(&info.tree.to_hex()[..7]);
                }
                Some('P') => {
                    chars.next();
                    let parents: Vec<String> = info.parents.iter().map(|p| p.to_hex()).collect();
                    result.push_str(&parents.join(" "));
                }
                Some('p') => {
                    chars.next();
                    let parents: Vec<String> = info
                        .parents
                        .iter()
                        .map(|p| p.to_hex()[..7].to_owned())
                        .collect();
                    result.push_str(&parents.join(" "));
                }
                Some('a') => {
                    chars.next();
                    match chars.peek() {
                        Some('n') => {
                            chars.next();
                            result.push_str(&extract_name(&info.author));
                        }
                        Some('e') => {
                            chars.next();
                            result.push_str(&extract_email(&info.author));
                        }
                        Some('d') => {
                            chars.next();
                            result.push_str(&format_date(&info.author));
                        }
                        Some('i') => {
                            chars.next();
                            result.push_str(&info.author);
                        }
                        _ => result.push_str("%a"),
                    }
                }
                Some('c') => {
                    chars.next();
                    match chars.peek() {
                        Some('n') => {
                            chars.next();
                            result.push_str(&extract_name(&info.committer));
                        }
                        Some('e') => {
                            chars.next();
                            result.push_str(&extract_email(&info.committer));
                        }
                        Some('d') => {
                            chars.next();
                            result.push_str(&format_date(&info.committer));
                        }
                        Some('i') => {
                            chars.next();
                            result.push_str(&info.committer);
                        }
                        _ => result.push_str("%c"),
                    }
                }
                Some('s') => {
                    chars.next();
                    result.push_str(info.message.lines().next().unwrap_or(""));
                }
                Some('b') => {
                    chars.next();
                    let body: String = info.message.lines().skip(2).collect::<Vec<_>>().join("\n");
                    result.push_str(&body);
                }
                Some('n') => {
                    chars.next();
                    result.push('\n');
                }
                Some('%') => {
                    chars.next();
                    result.push('%');
                }
                _ => result.push('%'),
            }
        } else {
            result.push(ch);
        }
    }

    result
}

/// Extract the name portion from a Git ident string.
fn extract_name(ident: &str) -> String {
    if let Some(bracket) = ident.find('<') {
        ident[..bracket].trim().to_owned()
    } else {
        ident.to_owned()
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

/// Format ident for display: "Name <email>".
fn format_ident_display(ident: &str) -> String {
    let name = extract_name(ident);
    let email = extract_email(ident);
    format!("{name} <{email}>")
}

/// Format the date from an ident string for display.
fn format_date(ident: &str) -> String {
    // Git ident: "Name <email> timestamp offset"
    let parts: Vec<&str> = ident.rsplitn(3, ' ').collect();
    if parts.len() >= 2 {
        let ts_str = parts[1];
        let offset = parts[0];
        if let Ok(ts) = ts_str.parse::<i64>() {
            let dt = time::OffsetDateTime::from_unix_timestamp(ts)
                .unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
            // Format similar to Git's default
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

/// Resolve a revision string to an ObjectId.
fn resolve_revision(repo: &Repository, rev: &str) -> Result<ObjectId> {
    // Try as a hex OID first
    if let Ok(oid) = ObjectId::from_hex(rev) {
        return Ok(oid);
    }

    // Try as a ref
    let head = resolve_head(&repo.git_dir)?;
    if rev == "HEAD" {
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

/// Collect ref name → OID decorations from the repository.
fn collect_decorations(
    repo: &Repository,
) -> Result<std::collections::HashMap<String, Vec<String>>> {
    let mut map: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();

    // HEAD
    let head = resolve_head(&repo.git_dir)?;
    if let Some(oid) = head.oid() {
        let hex = oid.to_hex();
        let label = match &head {
            HeadState::Branch { short_name, .. } => format!("HEAD -> {short_name}"),
            _ => "HEAD".to_owned(),
        };
        map.entry(hex).or_default().push(label);
    }

    // refs/heads/
    collect_refs_from_dir(
        &repo.git_dir.join("refs/heads"),
        "refs/heads/",
        "",
        &mut map,
    )?;

    // refs/tags/
    collect_refs_from_dir(
        &repo.git_dir.join("refs/tags"),
        "refs/tags/",
        "tag: ",
        &mut map,
    )?;

    Ok(map)
}

/// Recursively collect refs from a directory.
fn collect_refs_from_dir(
    dir: &std::path::Path,
    strip_prefix: &str,
    display_prefix: &str,
    map: &mut std::collections::HashMap<String, Vec<String>>,
) -> Result<()> {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };

    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_refs_from_dir(&path, strip_prefix, display_prefix, map)?;
        } else if let Ok(content) = std::fs::read_to_string(&path) {
            let hex = content.trim();
            let full_ref = path.to_string_lossy();
            // Extract the ref name after the git_dir prefix
            if let Some(idx) = full_ref.find(strip_prefix) {
                let name = &full_ref[idx + strip_prefix.len()..];
                let label = format!("{display_prefix}{name}");
                map.entry(hex.to_owned()).or_default().push(label);
            }
        }
    }

    Ok(())
}

/// Format decoration string for a commit.
fn format_decoration(
    hex: &str,
    decorations: Option<&std::collections::HashMap<String, Vec<String>>>,
) -> String {
    match decorations {
        Some(map) => {
            if let Some(refs) = map.get(hex) {
                format!(" ({})", refs.join(", "))
            } else {
                String::new()
            }
        }
        None => String::new(),
    }
}
