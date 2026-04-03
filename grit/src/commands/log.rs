//! `grit log` — show commit logs.
//!
//! Displays the commit history starting from HEAD (or specified revisions),
//! with configurable formatting and filtering.

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use grit_lib::diff::diff_trees;
use grit_lib::objects::{parse_commit, ObjectId};
use grit_lib::odb::Odb;
use grit_lib::repo::Repository;
use grit_lib::state::{resolve_head, HeadState};
use regex::Regex;
use std::collections::HashSet;
use std::io::{self, Write};

/// Arguments for `grit log`.
#[derive(Debug, ClapArgs)]
#[command(about = "Show commit logs")]
pub struct Args {
    /// Revisions and pathspecs (separated by --).
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

    /// Filter by author (regex pattern).
    #[arg(long = "author")]
    pub author: Option<String>,

    /// Filter by committer (regex pattern).
    #[arg(long = "committer")]
    pub committer_filter: Option<String>,

    /// Filter by commit message (regex pattern).
    #[arg(long = "grep")]
    pub grep: Option<String>,

    /// Skip merge commits.
    #[arg(long = "no-merges")]
    pub no_merges: bool,

    /// Show only merge commits.
    #[arg(long = "merges")]
    pub merges: bool,

    /// Date format.
    #[arg(long = "date")]
    pub date: Option<String>,

    /// Pathspecs (after --).
    #[arg(last = true)]
    pub pathspecs: Vec<String>,
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

    // Compile filter regexes
    let author_re = args
        .author
        .as_ref()
        .map(|p| Regex::new(p))
        .transpose()
        .context("invalid --author regex")?;
    let committer_re = args
        .committer_filter
        .as_ref()
        .map(|p| Regex::new(p))
        .transpose()
        .context("invalid --committer regex")?;
    let grep_re = args
        .grep
        .as_ref()
        .map(|p| Regex::new(p))
        .transpose()
        .context("invalid --grep regex")?;

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
        author_re.as_ref(),
        committer_re.as_ref(),
        grep_re.as_ref(),
        args.no_merges,
        args.merges,
        &args.pathspecs,
    )?;

    let commits = if args.reverse {
        commits.into_iter().rev().collect::<Vec<_>>()
    } else {
        commits
    };

    let stdout = io::stdout();
    let mut out = stdout.lock();

    // Detect format: (separator) vs tformat: (terminator) semantics
    let is_format_separator = args
        .format
        .as_deref()
        .map(|f| f.starts_with("format:"))
        .unwrap_or(false);

    for (i, (oid, commit_data)) in commits.iter().enumerate() {
        if is_format_separator && i > 0 {
            writeln!(out)?;
        }
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
    author_re: Option<&Regex>,
    committer_re: Option<&Regex>,
    grep_re: Option<&Regex>,
    no_merges: bool,
    merges_only: bool,
    pathspecs: &[String],
) -> Result<Vec<(ObjectId, CommitInfo)>> {
    // Short-circuit: if max_count is explicitly 0, return nothing.
    if max_count == Some(0) {
        return Ok(Vec::new());
    }

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

        // Add parents to queue before filtering (we always walk)
        if first_parent {
            if let Some(parent) = commit.parents.first() {
                queue.push(*parent);
            }
        } else {
            for parent in commit.parents.iter().rev() {
                if !visited.contains(parent) {
                    queue.push(*parent);
                }
            }
        }

        // Apply filters
        let is_merge = info.parents.len() > 1;
        if no_merges && is_merge {
            continue;
        }
        if merges_only && !is_merge {
            continue;
        }
        if let Some(re) = author_re {
            if !re.is_match(&info.author) {
                continue;
            }
        }
        if let Some(re) = committer_re {
            if !re.is_match(&info.committer) {
                continue;
            }
        }
        if let Some(re) = grep_re {
            if !re.is_match(&info.message) {
                continue;
            }
        }
        if !pathspecs.is_empty() {
            if !commit_touches_paths(odb, &info, pathspecs)? {
                continue;
            }
        }

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
    }

    // Sort by commit timestamp (committer date) — descending
    result.sort_by(|a, b| {
        let ts_a = extract_timestamp(&a.1.committer);
        let ts_b = extract_timestamp(&b.1.committer);
        ts_b.cmp(&ts_a)
    });

    Ok(result)
}

/// Check if a commit touches any of the given pathspecs by diffing against parents.
fn commit_touches_paths(odb: &Odb, info: &CommitInfo, pathspecs: &[String]) -> Result<bool> {
    if info.parents.is_empty() {
        // Root commit: diff against empty tree
        let entries = diff_trees(odb, None, Some(&info.tree), "")?;
        return Ok(entries.iter().any(|e| {
            let path = e.path();
            pathspecs.iter().any(|ps| path_matches(path, ps))
        }));
    }

    for parent_oid in &info.parents {
        let parent_obj = odb.read(parent_oid)?;
        let parent_commit = parse_commit(&parent_obj.data)?;
        let entries = diff_trees(odb, Some(&parent_commit.tree), Some(&info.tree), "")?;
        if entries.iter().any(|e| {
            let path = e.path();
            pathspecs.iter().any(|ps| path_matches(path, ps))
        }) {
            return Ok(true);
        }
    }

    Ok(false)
}

/// Check if a file path matches a pathspec (prefix match or exact match).
fn path_matches(path: &str, pathspec: &str) -> bool {
    if path == pathspec {
        return true;
    }
    // Prefix match: pathspec is a directory prefix
    if path.starts_with(pathspec) && path.as_bytes().get(pathspec.len()) == Some(&b'/') {
        return true;
    }
    // pathspec could be a directory
    let ps = pathspec.strip_suffix('/').unwrap_or(pathspec);
    if path.starts_with(ps) && path.as_bytes().get(ps.len()) == Some(&b'/') {
        return true;
    }
    false
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

/// Parse a timezone offset string like "+0200" or "-0500" into seconds.
fn parse_tz_offset(offset: &str) -> i64 {
    let bytes = offset.as_bytes();
    if bytes.len() < 5 {
        return 0;
    }
    let sign = if bytes[0] == b'-' { -1i64 } else { 1i64 };
    let hours: i64 = offset[1..3].parse().unwrap_or(0);
    let minutes: i64 = offset[3..5].parse().unwrap_or(0);
    sign * (hours * 3600 + minutes * 60)
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
    let date_format = args.date.as_deref();

    match format {
        Some(fmt) if fmt.starts_with("format:") || fmt.starts_with("tformat:") => {
            let is_tformat = fmt.starts_with("tformat:");
            let template = if let Some(t) = fmt.strip_prefix("format:") {
                t
            } else {
                &fmt[8..]
            };
            let formatted = apply_format_string(template, oid, info, decorations, date_format);
            if is_tformat {
                writeln!(out, "{formatted}")?;
            } else {
                write!(out, "{formatted}")?;
            }
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
            writeln!(out, "Date:   {}", format_date_with_mode(&info.author, date_format))?;
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
            writeln!(out, "AuthorDate: {}", format_date_with_mode(&info.author, date_format))?;
            writeln!(out, "Commit:     {}", format_ident_display(&info.committer))?;
            writeln!(out, "CommitDate: {}", format_date_with_mode(&info.committer, date_format))?;
            writeln!(out)?;
            for line in info.message.lines() {
                writeln!(out, "    {line}")?;
            }
            writeln!(out)?;
        }
        Some(other) => {
            // Try as a format string directly
            let formatted = apply_format_string(other, oid, info, decorations, date_format);
            writeln!(out, "{formatted}")?;
        }
    }

    Ok(())
}

/// Apply a format string with placeholders like %H, %h, %s, %an, %ae, etc.
fn apply_format_string(
    template: &str,
    oid: &ObjectId,
    info: &CommitInfo,
    decorations: Option<&std::collections::HashMap<String, Vec<String>>>,
    date_format: Option<&str>,
) -> String {
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
                            result.push_str(&format_date_with_mode(&info.author, date_format));
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
                            result.push_str(&format_date_with_mode(&info.committer, date_format));
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
                    // Body: everything after the first paragraph separator (blank line)
                    let body = extract_body(&info.message);
                    result.push_str(&body);
                }
                Some('B') => {
                    chars.next();
                    // Raw body: entire commit message
                    result.push_str(&info.message);
                }
                Some('d') => {
                    chars.next();
                    // Decorations
                    let dec = format_decoration(&hex, decorations);
                    result.push_str(&dec);
                }
                Some('D') => {
                    chars.next();
                    // Decorations without parens
                    let dec = format_decoration_no_parens(&hex, decorations);
                    result.push_str(&dec);
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

/// Extract the message body (everything after the subject + blank line).
fn extract_body(message: &str) -> String {
    let msg = message.trim_end_matches('\n');
    let mut lines = msg.lines();
    // Skip subject line
    lines.next();
    // Skip blank line separator if present
    if let Some(line) = lines.next() {
        if !line.is_empty() {
            // No blank separator — include this line as body
            let rest: Vec<&str> = lines.collect();
            if rest.is_empty() {
                return format!("{line}\n");
            } else {
                return format!("{}\n{}\n", line, rest.join("\n"));
            }
        }
    }
    // Collect remaining lines as body
    let body_lines: Vec<&str> = lines.collect();
    if body_lines.is_empty() {
        String::new()
    } else {
        format!("{}\n", body_lines.join("\n"))
    }
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

/// Format the date from an ident string for display, with optional date mode.
fn format_date_with_mode(ident: &str, date_mode: Option<&str>) -> String {
    // Git ident: "Name <email> timestamp offset"
    let parts: Vec<&str> = ident.rsplitn(3, ' ').collect();
    if parts.len() < 2 {
        return ident.to_owned();
    }
    let ts_str = parts[1];
    let offset_str = parts[0];
    let ts = match ts_str.parse::<i64>() {
        Ok(v) => v,
        Err(_) => return format!("{ts_str} {offset_str}"),
    };

    let tz_offset_secs = parse_tz_offset(offset_str);

    match date_mode {
        Some("short") => {
            // YYYY-MM-DD in the author's timezone
            let adjusted = ts + tz_offset_secs;
            let dt = time::OffsetDateTime::from_unix_timestamp(adjusted)
                .unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
            format!("{:04}-{:02}-{:02}", dt.year(), dt.month() as u8, dt.day())
        }
        Some("iso") | Some("iso8601") => {
            // ISO format: 2005-04-07 15:13:13 +0200
            let adjusted = ts + tz_offset_secs;
            let dt = time::OffsetDateTime::from_unix_timestamp(adjusted)
                .unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
            format!(
                "{:04}-{:02}-{:02} {:02}:{:02}:{:02} {}",
                dt.year(),
                dt.month() as u8,
                dt.day(),
                dt.hour(),
                dt.minute(),
                dt.second(),
                offset_str
            )
        }
        Some("iso-strict") | Some("iso8601-strict") => {
            let adjusted = ts + tz_offset_secs;
            let dt = time::OffsetDateTime::from_unix_timestamp(adjusted)
                .unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
            let sign = if tz_offset_secs >= 0 { '+' } else { '-' };
            let abs_offset = tz_offset_secs.unsigned_abs();
            let h = abs_offset / 3600;
            let m = (abs_offset % 3600) / 60;
            format!(
                "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}{}{:02}:{:02}",
                dt.year(),
                dt.month() as u8,
                dt.day(),
                dt.hour(),
                dt.minute(),
                dt.second(),
                sign,
                h,
                m
            )
        }
        Some("raw") => {
            format!("{ts} {offset_str}")
        }
        _ => {
            // Default Git date format
            let dt = time::OffsetDateTime::from_unix_timestamp(ts)
                .unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
            let format = time::format_description::parse(
                "[weekday repr:short] [month repr:short] [day] [hour]:[minute]:[second] [year]",
            );
            if let Ok(fmt) = format {
                if let Ok(formatted) = dt.format(&fmt) {
                    return format!("{formatted} {offset_str}");
                }
            }
            format!("{ts_str} {offset_str}")
        }
    }
}

/// Format the date from an ident string (legacy, default mode).
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

/// Format decoration string for a commit (with parentheses).
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

/// Format decoration string without parentheses (for %D).
fn format_decoration_no_parens(
    hex: &str,
    decorations: Option<&std::collections::HashMap<String, Vec<String>>>,
) -> String {
    match decorations {
        Some(map) => {
            if let Some(refs) = map.get(hex) {
                refs.join(", ")
            } else {
                String::new()
            }
        }
        None => String::new(),
    }
}
