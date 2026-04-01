//! `grit show` — show various types of objects.
//!
//! For commits, displays the commit header (like `git log -1`) followed by the
//! diff introduced by that commit.  For tags, shows the tag object then the
//! tagged commit.  For trees, lists the tree contents (like `ls-tree`).  For
//! blobs, prints the raw blob content.

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use grit_lib::diff::{diff_trees, unified_diff};
use grit_lib::objects::{parse_commit, parse_tag, parse_tree, ObjectId, ObjectKind};
use grit_lib::odb::Odb;
use grit_lib::repo::Repository;
use grit_lib::rev_parse::resolve_revision;
use std::io::{self, Write};

/// Arguments for `grit show`.
#[derive(Debug, ClapArgs)]
#[command(about = "Show various types of objects (commits, trees, blobs, tags)")]
pub struct Args {
    /// Object to show (commit, tree, blob, or tag). Defaults to HEAD.
    #[arg()]
    pub object: Option<String>,

    /// Show only one line per commit (short hash + subject).
    #[arg(long = "oneline")]
    pub oneline: bool,

    /// Pretty-print format.
    #[arg(long = "format", alias = "pretty")]
    pub format: Option<String>,

    /// Suppress diff output (show only the commit header).
    #[arg(short = 'q', long = "quiet")]
    pub quiet: bool,

    /// Number of unified context lines for diff output.
    #[arg(short = 'U', long = "unified", value_name = "N")]
    pub unified: Option<usize>,
}

/// Run the `show` command.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;

    let spec = args.object.as_deref().unwrap_or("HEAD");
    let oid = resolve_revision(&repo, spec)
        .with_context(|| format!("unknown revision or path: '{spec}'"))?;

    let obj = repo.odb.read(&oid).context("reading object")?;

    let stdout = io::stdout();
    let mut out = stdout.lock();

    match obj.kind {
        ObjectKind::Commit => {
            show_commit(&mut out, &repo.odb, &oid, &obj.data, &args)?;
        }
        ObjectKind::Tag => {
            show_tag(&mut out, &repo.odb, &obj.data, &args)?;
        }
        ObjectKind::Tree => {
            show_tree(&mut out, &obj.data)?;
        }
        ObjectKind::Blob => {
            out.write_all(&obj.data)?;
        }
    }

    Ok(())
}

/// Show a commit object: header + diff.
fn show_commit(
    out: &mut impl Write,
    odb: &Odb,
    oid: &ObjectId,
    data: &[u8],
    args: &Args,
) -> Result<()> {
    let commit = parse_commit(data).context("parsing commit")?;
    let hex = oid.to_hex();

    if args.oneline || args.format.as_deref() == Some("oneline") {
        let first_line = commit.message.lines().next().unwrap_or("");
        writeln!(out, "{} {}", &hex[..7], first_line)?;
        return Ok(());
    }

    let format = args.format.as_deref();
    match format {
        Some(fmt) if fmt.starts_with("format:") || fmt.starts_with("tformat:") => {
            let _template = fmt
                .strip_prefix("format:")
                .or_else(|| fmt.strip_prefix("tformat:"))
                .unwrap_or(fmt);
            let template = if let Some(s) = fmt.strip_prefix("format:") {
                s
            } else {
                fmt.strip_prefix("tformat:").unwrap_or(fmt)
            };
            let formatted = apply_format_string(template, oid, &commit);
            writeln!(out, "{formatted}")?;
        }
        Some("short") => {
            writeln!(out, "commit {hex}")?;
            let author_name = extract_name(&commit.author);
            writeln!(out, "Author: {author_name}")?;
            writeln!(out)?;
            for line in commit.message.lines().take(1) {
                writeln!(out, "    {line}")?;
            }
            writeln!(out)?;
        }
        Some("full") => {
            writeln!(out, "commit {hex}")?;
            writeln!(out, "Author: {}", format_ident_display(&commit.author))?;
            writeln!(out, "Commit: {}", format_ident_display(&commit.committer))?;
            writeln!(out)?;
            for line in commit.message.lines() {
                writeln!(out, "    {line}")?;
            }
            writeln!(out)?;
        }
        Some("fuller") => {
            writeln!(out, "commit {hex}")?;
            writeln!(out, "Author:     {}", format_ident_display(&commit.author))?;
            writeln!(out, "AuthorDate: {}", format_date(&commit.author))?;
            writeln!(
                out,
                "Commit:     {}",
                format_ident_display(&commit.committer)
            )?;
            writeln!(out, "CommitDate: {}", format_date(&commit.committer))?;
            writeln!(out)?;
            for line in commit.message.lines() {
                writeln!(out, "    {line}")?;
            }
            writeln!(out)?;
        }
        Some(other) if other.starts_with("format:") || other.starts_with("tformat:") => {
            // Already handled above — unreachable
        }
        Some(other) => {
            let formatted = apply_format_string(other, oid, &commit);
            writeln!(out, "{formatted}")?;
        }
        None => {
            // Default: medium format
            writeln!(out, "commit {hex}")?;
            writeln!(out, "Author: {}", format_ident_display(&commit.author))?;
            writeln!(out, "Date:   {}", format_date(&commit.author))?;
            writeln!(out)?;
            for line in commit.message.lines() {
                writeln!(out, "    {line}")?;
            }
            writeln!(out)?;
        }
    }

    if args.quiet {
        return Ok(());
    }

    // Show diff: compare this commit's tree against its first parent (or empty tree for root).
    let new_tree = Some(&commit.tree);
    let old_tree = commit.parents.first().map(|parent_oid| {
        odb.read(parent_oid)
            .ok()
            .and_then(|obj| parse_commit(&obj.data).ok())
            .map(|c| c.tree)
    });

    // old_tree is Option<Option<ObjectId>>; flatten and get a reference
    let old_tree_oid: Option<ObjectId> = old_tree.flatten();
    let context = args.unified.unwrap_or(3);

    let diff_entries =
        diff_trees(odb, old_tree_oid.as_ref(), new_tree, "").context("computing diff")?;

    for entry in &diff_entries {
        let old_path = entry.old_path.as_deref().unwrap_or("/dev/null");
        let new_path = entry.new_path.as_deref().unwrap_or("/dev/null");

        // Print the diff header
        write_diff_header(out, entry)?;

        let old_content = if entry.old_oid == grit_lib::diff::zero_oid() {
            String::new()
        } else {
            match odb.read(&entry.old_oid) {
                Ok(obj) => String::from_utf8_lossy(&obj.data).into_owned(),
                Err(_) => String::new(),
            }
        };

        let new_content = if entry.new_oid == grit_lib::diff::zero_oid() {
            String::new()
        } else {
            match odb.read(&entry.new_oid) {
                Ok(obj) => String::from_utf8_lossy(&obj.data).into_owned(),
                Err(_) => String::new(),
            }
        };

        let patch = unified_diff(&old_content, &new_content, old_path, new_path, context);
        write!(out, "{patch}")?;
    }

    Ok(())
}

/// Write a `diff --git a/path b/path` header plus index/mode lines.
fn write_diff_header(out: &mut impl Write, entry: &grit_lib::diff::DiffEntry) -> Result<()> {
    use grit_lib::diff::DiffStatus;

    let old_path = entry
        .old_path
        .as_deref()
        .unwrap_or(entry.new_path.as_deref().unwrap_or(""));
    let new_path = entry
        .new_path
        .as_deref()
        .unwrap_or(entry.old_path.as_deref().unwrap_or(""));

    writeln!(out, "diff --git a/{old_path} b/{new_path}")?;

    match entry.status {
        DiffStatus::Added => {
            writeln!(out, "new file mode {}", entry.new_mode)?;
            let old_abbrev = &entry.old_oid.to_hex()[..7];
            let new_abbrev = &entry.new_oid.to_hex()[..7];
            writeln!(out, "index {old_abbrev}..{new_abbrev}")?;
        }
        DiffStatus::Deleted => {
            writeln!(out, "deleted file mode {}", entry.old_mode)?;
            let old_abbrev = &entry.old_oid.to_hex()[..7];
            let new_abbrev = &entry.new_oid.to_hex()[..7];
            writeln!(out, "index {old_abbrev}..{new_abbrev}")?;
        }
        DiffStatus::Modified => {
            if entry.old_mode != entry.new_mode {
                writeln!(out, "old mode {}", entry.old_mode)?;
                writeln!(out, "new mode {}", entry.new_mode)?;
            }
            let old_abbrev = &entry.old_oid.to_hex()[..7];
            let new_abbrev = &entry.new_oid.to_hex()[..7];
            if entry.old_mode == entry.new_mode {
                writeln!(out, "index {old_abbrev}..{new_abbrev} {}", entry.old_mode)?;
            } else {
                writeln!(out, "index {old_abbrev}..{new_abbrev}")?;
            }
        }
        DiffStatus::Renamed => {
            writeln!(out, "similarity index 100%")?;
            writeln!(out, "rename from {old_path}")?;
            writeln!(out, "rename to {new_path}")?;
        }
        DiffStatus::Copied => {
            writeln!(out, "similarity index 100%")?;
            writeln!(out, "copy from {old_path}")?;
            writeln!(out, "copy to {new_path}")?;
        }
        DiffStatus::TypeChanged => {
            writeln!(out, "old mode {}", entry.old_mode)?;
            writeln!(out, "new mode {}", entry.new_mode)?;
        }
        DiffStatus::Unmerged => {}
    }

    Ok(())
}

/// Show a tag object: tag header, then the tagged object.
fn show_tag(out: &mut impl Write, odb: &Odb, data: &[u8], args: &Args) -> Result<()> {
    let tag = parse_tag(data).context("parsing tag")?;

    writeln!(out, "tag {}", tag.tag)?;
    if let Some(ref tagger) = tag.tagger {
        writeln!(out, "Tagger: {}", format_ident_display(tagger))?;
        writeln!(out, "Date:   {}", format_date(tagger))?;
    }
    writeln!(out)?;
    for line in tag.message.lines() {
        writeln!(out, "{line}")?;
    }
    if !tag.message.is_empty() {
        writeln!(out)?;
    }

    // Recursively show the tagged object
    let tagged_obj = odb.read(&tag.object).context("reading tagged object")?;
    match tagged_obj.kind {
        ObjectKind::Commit => {
            show_commit(out, odb, &tag.object, &tagged_obj.data, args)?;
        }
        ObjectKind::Tag => {
            show_tag(out, odb, &tagged_obj.data, args)?;
        }
        ObjectKind::Tree => {
            show_tree(out, &tagged_obj.data)?;
        }
        ObjectKind::Blob => {
            out.write_all(&tagged_obj.data)?;
        }
    }

    Ok(())
}

/// Show a tree object: list entries (like ls-tree).
fn show_tree(out: &mut impl Write, data: &[u8]) -> Result<()> {
    let entries = parse_tree(data).context("parsing tree")?;
    for entry in &entries {
        let kind = if entry.mode == 0o040000 {
            "tree"
        } else {
            "blob"
        };
        let name = String::from_utf8_lossy(&entry.name);
        writeln!(
            out,
            "{:06o} {} {}\t{}",
            entry.mode,
            kind,
            entry.oid.to_hex(),
            name
        )?;
    }
    Ok(())
}

/// Inline commit info for format string expansion (mirrors log.rs CommitInfo usage).
struct CommitInfo<'a> {
    tree: ObjectId,
    parents: &'a [ObjectId],
    author: &'a str,
    committer: &'a str,
    message: &'a str,
}

/// Apply a format string with placeholders like %H, %h, %s, %an, %ae, etc.
fn apply_format_string(
    template: &str,
    oid: &ObjectId,
    commit: &grit_lib::objects::CommitData,
) -> String {
    let info = CommitInfo {
        tree: commit.tree,
        parents: &commit.parents,
        author: &commit.author,
        committer: &commit.committer,
        message: &commit.message,
    };
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
                            result.push_str(&extract_name(info.author));
                        }
                        Some('e') => {
                            chars.next();
                            result.push_str(&extract_email(info.author));
                        }
                        Some('d') => {
                            chars.next();
                            result.push_str(&format_date(info.author));
                        }
                        Some('i') => {
                            chars.next();
                            result.push_str(info.author);
                        }
                        _ => result.push_str("%a"),
                    }
                }
                Some('c') => {
                    chars.next();
                    match chars.peek() {
                        Some('n') => {
                            chars.next();
                            result.push_str(&extract_name(info.committer));
                        }
                        Some('e') => {
                            chars.next();
                            result.push_str(&extract_email(info.committer));
                        }
                        Some('d') => {
                            chars.next();
                            result.push_str(&format_date(info.committer));
                        }
                        Some('i') => {
                            chars.next();
                            result.push_str(info.committer);
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

/// Extract the name portion from a Git ident string (e.g. "Name <email> ts offset").
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

/// Format the date portion of a Git ident string for human display.
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
