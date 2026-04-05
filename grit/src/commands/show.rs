//! `grit show` — show various types of objects.
//!
//! For commits, displays the commit header (like `git log -1`) followed by the
//! diff introduced by that commit.  For tags, shows the tag object then the
//! tagged commit.  For trees, lists the tree contents (like `ls-tree`).  For
//! blobs, prints the raw blob content.

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use grit_lib::diff::{anchored_unified_diff, detect_copies, detect_renames, diff_trees, unified_diff, DiffEntry};
use grit_lib::objects::{parse_commit, parse_tag, parse_tree, ObjectId, ObjectKind};
use grit_lib::odb::Odb;
use grit_lib::repo::Repository;
use grit_lib::rev_parse::resolve_revision;
use std::io::{self, Write};

/// Arguments for `grit show`.
#[derive(Debug, ClapArgs)]
#[command(about = "Show various types of objects (commits, trees, blobs, tags)")]
pub struct Args {
    /// Object(s) to show (commit, tree, blob, or tag). Defaults to HEAD.
    #[arg()]
    pub objects: Vec<String>,

    /// Show only one line per commit (short hash + subject).
    #[arg(long = "oneline")]
    pub oneline: bool,

    /// Pretty-print format.
    #[arg(long = "format", alias = "pretty")]
    pub format: Option<String>,

    /// Suppress diff output (show only the commit header).
    #[arg(short = 'q', long = "quiet")]
    pub quiet: bool,

    /// Suppress diff output (alias for --quiet / -q).
    #[arg(short = 's', long = "no-patch")]
    pub no_patch: bool,

    /// Number of unified context lines for diff output.
    #[arg(short = 'U', long = "unified", value_name = "N")]
    pub unified: Option<usize>,

    /// Anchored diff: keep the specified text as context.
    #[arg(long = "anchored")]
    pub anchored: Vec<String>,

    /// Use the patience diff algorithm.
    #[arg(long = "patience")]
    pub patience: bool,

    /// Show a diffstat summary after the commit header.
    #[arg(long = "stat")]
    pub stat: bool,

    /// Show raw diff-tree output format.
    #[arg(long = "raw")]
    pub raw: bool,

    /// Show only names of changed files.
    #[arg(long = "name-only")]
    pub name_only: bool,

    /// Show names and status of changed files.
    #[arg(long = "name-status")]
    pub name_status: bool,

    /// Show a summary of extended header information (renames, mode changes).
    #[arg(long = "summary")]
    pub summary: bool,

    /// Show the patch (diff) output together with the diffstat.
    #[arg(long = "patch-with-stat")]
    pub patch_with_stat: bool,

    /// Show the patch (diff) output together with the raw output.
    #[arg(long = "patch-with-raw")]
    pub patch_with_raw: bool,

    /// Generate a patch.
    #[arg(short = 'p', long = "patch")]
    pub patch: bool,

    /// Show abbreviated OIDs.
    #[arg(long = "abbrev", value_name = "N", default_missing_value = "7", num_args = 0..=1, require_equals = true)]
    pub abbrev: Option<String>,

    /// Show full OIDs.
    #[arg(long = "no-abbrev")]
    pub no_abbrev: bool,

    /// Detect renames.
    #[arg(short = 'M', long = "find-renames", value_name = "N", default_missing_value = "50", num_args = 0..=1)]
    pub find_renames: Option<String>,

    /// Detect copies (use twice for harder).
    #[arg(short = 'C', long = "find-copies", value_name = "N", default_missing_value = "50", num_args = 0..=1, action = clap::ArgAction::Append)]
    pub find_copies: Vec<String>,

    /// Show the full diff (for merge commits).
    #[arg(short = 'm')]
    pub diff_merges: bool,

    /// Date format for display.
    #[arg(long = "date")]
    pub date: Option<String>,

    /// Don't show external diff helper.
    #[arg(long = "no-ext-diff")]
    pub no_ext_diff: bool,

    /// Show notes.
    #[arg(long = "notes", num_args = 0..=1, default_missing_value = "", require_equals = true)]
    pub notes: Option<String>,

    /// Full diff index hashes.
    #[arg(long = "full-index")]
    pub full_index: bool,

    /// Colorize the output.
    #[arg(long = "color", value_name = "WHEN", default_missing_value = "always", num_args = 0..=1)]
    pub color: Option<String>,

    /// Disable color.
    #[arg(long = "no-color")]
    pub no_color: bool,

    /// Show short stat summary.
    #[arg(long = "shortstat")]
    pub shortstat: bool,

    /// Disable textconv.
    #[arg(long = "no-textconv")]
    pub no_textconv: bool,

    /// Show binary diff in git binary format.
    #[arg(long = "binary")]
    pub binary: bool,

    /// Show numstat summary.
    #[arg(long = "numstat")]
    pub numstat: bool,
}

/// Run the `show` command.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;

    let specs: Vec<&str> = if args.objects.is_empty() {
        vec!["HEAD"]
    } else {
        // Split on -- to separate objects from pathspecs
        args.objects.iter().map(|s| s.as_str()).collect()
    };

    let stdout = io::stdout();
    let mut out = stdout.lock();

    for spec in &specs {
        if *spec == "--" {
            break;
        }
        let oid = resolve_revision(&repo, spec)
            .with_context(|| format!("unknown revision or path: '{spec}'"))?;

        let obj = repo.odb.read(&oid).context("reading object")?;

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

            let template = if let Some(t) = fmt.strip_prefix("format:") {
                t
            } else {
                &fmt[8..]
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
        Some("medium") | None => {
            // Medium format (default)
            writeln!(out, "commit {hex}")?;
            writeln!(out, "Author: {}", format_ident_display(&commit.author))?;
            writeln!(out, "Date:   {}", format_date(&commit.author))?;
            writeln!(out)?;
            for line in commit.message.lines() {
                writeln!(out, "    {line}")?;
            }
            writeln!(out)?;
        }
        Some("email") => {
            writeln!(out, "From {} Mon Sep 17 00:00:00 2001", hex)?;
            writeln!(out, "From: {}", format_ident_display(&commit.author))?;
            writeln!(out, "Date: {}", format_date(&commit.author))?;
            let subject = commit.message.lines().next().unwrap_or("");
            writeln!(out, "Subject: [PATCH] {}", subject)?;
            writeln!(out)?;
            for line in commit.message.lines() {
                writeln!(out, "{line}")?;
            }
            writeln!(out)?;
        }
        Some("raw") => {
            writeln!(out, "commit {hex}")?;
            writeln!(out, "tree {}", commit.tree.to_hex())?;
            for parent in &commit.parents {
                writeln!(out, "parent {}", parent.to_hex())?;
            }
            writeln!(out, "author {}", commit.author)?;
            writeln!(out, "committer {}", commit.committer)?;
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
    }

    if args.quiet || args.no_patch {
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

    // Apply rename/copy detection if -M or -C flags are set.
    let diff_entries = apply_rename_copy_detection(odb, diff_entries, args, old_tree_oid.as_ref());

    // --name-only: just print file names
    if args.name_only {
        for entry in &diff_entries {
            let path = entry
                .new_path
                .as_deref()
                .or(entry.old_path.as_deref())
                .unwrap_or("");
            writeln!(out, "{path}")?;
        }
        return Ok(());
    }

    // --name-status: print status letter and file name
    if args.name_status {
        for entry in &diff_entries {
            let path = entry
                .new_path
                .as_deref()
                .or(entry.old_path.as_deref())
                .unwrap_or("");
            let status = match entry.status {
                grit_lib::diff::DiffStatus::Added => 'A',
                grit_lib::diff::DiffStatus::Deleted => 'D',
                grit_lib::diff::DiffStatus::Modified => 'M',
                grit_lib::diff::DiffStatus::Renamed => 'R',
                grit_lib::diff::DiffStatus::Copied => 'C',
                grit_lib::diff::DiffStatus::TypeChanged => 'T',
                grit_lib::diff::DiffStatus::Unmerged => 'U',
            };
            writeln!(out, "{status}\t{path}")?;
        }
        return Ok(());
    }

    // Determine what sections to show. Multiple can be active simultaneously.
    let show_raw = args.raw;
    let show_numstat = args.numstat;
    let show_stat = args.stat;
    let show_patch = !args.quiet && !args.no_patch && !args.name_only && !args.name_status;

    // --raw: raw diff-tree output format
    if show_raw {
        for entry in &diff_entries {
            let old_path = entry.old_path.as_deref().or(entry.new_path.as_deref()).unwrap_or("");
            let new_path = entry.new_path.as_deref().or(entry.old_path.as_deref()).unwrap_or("");
            let status_char = match entry.status {
                grit_lib::diff::DiffStatus::Added => 'A',
                grit_lib::diff::DiffStatus::Deleted => 'D',
                grit_lib::diff::DiffStatus::Modified => 'M',
                grit_lib::diff::DiffStatus::Renamed => 'R',
                grit_lib::diff::DiffStatus::Copied => 'C',
                grit_lib::diff::DiffStatus::TypeChanged => 'T',
                grit_lib::diff::DiffStatus::Unmerged => 'U',
            };
            let status_str = match entry.status {
                grit_lib::diff::DiffStatus::Renamed | grit_lib::diff::DiffStatus::Copied => {
                    let score = entry.score.unwrap_or(0);
                    format!("{status_char}{score:03}")
                }
                _ => format!("{status_char}"),
            };
            let paths = match entry.status {
                grit_lib::diff::DiffStatus::Renamed | grit_lib::diff::DiffStatus::Copied => {
                    format!("{old_path}\t{new_path}")
                }
                _ => new_path.to_string(),
            };
            writeln!(out, ":{} {} {} {} {status_str}\t{paths}",
                entry.old_mode, entry.new_mode,
                &entry.old_oid.to_hex()[..7], &entry.new_oid.to_hex()[..7],
            )?;
        }
    }

    // --numstat
    if show_numstat {
        for entry in &diff_entries {
            write_numstat_line(out, odb, entry)?;
        }
    }

    // Blank line separator before patch when raw or numstat was shown
    if (show_raw || show_numstat) && show_patch {
        writeln!(out)?;
    }

    // --stat: show diffstat summary
    if show_stat && !show_raw && !show_numstat {
        write_diffstat(out, odb, &diff_entries)?;
        if !show_patch { return Ok(()); }
    }

    if !show_patch {
        return Ok(());
    }

    // Default: full unified diff
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

        let patch = if !args.anchored.is_empty() {
            anchored_unified_diff(
                &old_content,
                &new_content,
                old_path,
                new_path,
                context,
                &args.anchored,
            )
        } else {
            unified_diff(&old_content, &new_content, old_path, new_path, context)
        };
        write!(out, "{patch}")?;
    }

    Ok(())
}

/// Write a diffstat summary for the given diff entries.
/// Write a single numstat line for an entry.
fn write_numstat_line(
    out: &mut impl Write,
    odb: &Odb,
    entry: &grit_lib::diff::DiffEntry,
) -> Result<()> {
    let old_content = if entry.old_oid == grit_lib::diff::zero_oid() {
        String::new()
    } else {
        odb.read(&entry.old_oid).map(|o| String::from_utf8_lossy(&o.data).into_owned()).unwrap_or_default()
    };
    let new_content = if entry.new_oid == grit_lib::diff::zero_oid() {
        String::new()
    } else {
        odb.read(&entry.new_oid).map(|o| String::from_utf8_lossy(&o.data).into_owned()).unwrap_or_default()
    };

    let is_binary = old_content.bytes().any(|b| b == 0) || new_content.bytes().any(|b| b == 0);
    let path_str = format_rename_path(entry);

    if is_binary {
        writeln!(out, "-\t-\t{path_str}")?;
    } else {
        let (ins, del) = grit_lib::diff::count_changes(&old_content, &new_content);
        writeln!(out, "{ins}\t{del}\t{path_str}")?;
    }
    Ok(())
}

/// Format path for numstat/stat display (with rename arrow notation).
fn format_rename_path(entry: &grit_lib::diff::DiffEntry) -> String {
    let old_path = entry.old_path.as_deref().unwrap_or("");
    let new_path = entry.new_path.as_deref().unwrap_or("");
    match entry.status {
        grit_lib::diff::DiffStatus::Renamed | grit_lib::diff::DiffStatus::Copied => {
            // Use compact rename format: common_prefix/{old => new}/common_suffix
            grit_lib::diff::format_rename_path(old_path, new_path)
        }
        _ => new_path.to_string(),
    }
}

fn write_diffstat(
    out: &mut impl Write,
    odb: &Odb,
    entries: &[grit_lib::diff::DiffEntry],
) -> Result<()> {
    let mut stats: Vec<(String, usize, usize)> = Vec::new();
    let mut total_ins = 0usize;
    let mut total_del = 0usize;

    for entry in entries {
        let path = entry
            .new_path
            .as_deref()
            .or(entry.old_path.as_deref())
            .unwrap_or("")
            .to_string();

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

        let old_lines: Vec<&str> = if old_content.is_empty() {
            vec![]
        } else {
            old_content.lines().collect()
        };
        let new_lines: Vec<&str> = if new_content.is_empty() {
            vec![]
        } else {
            new_content.lines().collect()
        };

        // Simple line-count based insertions/deletions.
        let ins = new_lines
            .len()
            .saturating_sub(old_lines.len().min(new_lines.len()));
        let del = old_lines
            .len()
            .saturating_sub(new_lines.len().min(old_lines.len()));

        // More accurate: count changed lines using the diff
        let patch = unified_diff(&old_content, &new_content, &path, &path, 0);
        let mut insertions = 0usize;
        let mut deletions = 0usize;
        for line in patch.lines() {
            if line.starts_with('+') && !line.starts_with("+++") {
                insertions += 1;
            } else if line.starts_with('-') && !line.starts_with("---") {
                deletions += 1;
            }
        }
        // Use diff-based counts if available, else line-based.
        let _ = (ins, del);

        total_ins += insertions;
        total_del += deletions;
        stats.push((path, insertions, deletions));
    }

    let max_name_len = stats.iter().map(|(p, _, _)| p.len()).max().unwrap_or(0);
    let max_total = stats.iter().map(|(_, i, d)| i + d).max().unwrap_or(0);
    let num_width = format!("{}", max_total).len();

    for (path, ins, del) in &stats {
        let total = ins + del;
        let bar: String = "+".repeat(*ins).to_string() + &"-".repeat(*del);
        writeln!(
            out,
            " {path:<width$} | {total:>nw$} {bar}",
            width = max_name_len,
            nw = num_width,
        )?;
    }

    let files = stats.len();
    let file_word = if files == 1 {
        "file changed"
    } else {
        "files changed"
    };
    let ins_part = if total_ins > 0 {
        let word = if total_ins == 1 {
            "insertion(+)"
        } else {
            "insertions(+)"
        };
        format!(", {total_ins} {word}")
    } else {
        String::new()
    };
    let del_part = if total_del > 0 {
        let word = if total_del == 1 {
            "deletion(-)"
        } else {
            "deletions(-)"
        };
        format!(", {total_del} {word}")
    } else {
        String::new()
    };
    writeln!(out, " {files} {file_word}{ins_part}{del_part}")?;

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
                            result.push_str(&format_date_iso(info.author));
                        }
                        Some('r') => {
                            chars.next();
                            result.push_str(&format_date_relative(info.author));
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
                            result.push_str(&format_date_iso(info.committer));
                        }
                        Some('r') => {
                            chars.next();
                            result.push_str(&format_date_relative(info.committer));
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
                Some('D') => {
                    chars.next();
                    // %D: decorations without parentheses — we leave it empty
                    // since we don't have a ref database context here.
                }
                Some('d') => {
                    chars.next();
                    // %d: decorations with parentheses — we leave it empty.
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

/// Format the date portion of a Git ident string in ISO 8601 format (%ci / %ai).
fn format_date_iso(ident: &str) -> String {
    let parts: Vec<&str> = ident.rsplitn(3, ' ').collect();
    if parts.len() >= 2 {
        let ts_str = parts[1];
        let offset_str = parts[0];
        if let Ok(ts) = ts_str.parse::<i64>() {
            // Parse the offset to apply to the timestamp.
            let offset_secs = parse_offset_seconds(offset_str);
            let dt = time::OffsetDateTime::from_unix_timestamp(ts + offset_secs as i64)
                .unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
            let format =
                time::format_description::parse("[year]-[month]-[day] [hour]:[minute]:[second]");
            if let Ok(fmt) = format {
                if let Ok(formatted) = dt.format(&fmt) {
                    // Git outputs: 2001-09-09 01:46:40 +0000
                    return format!("{formatted} {offset_str}");
                }
            }
        }
        format!("{ts_str} {offset_str}")
    } else {
        ident.to_owned()
    }
}

/// Parse a Git timezone offset string like "+0200" or "-0530" into seconds.
fn parse_offset_seconds(offset: &str) -> i32 {
    if offset.len() < 5 {
        return 0;
    }
    let sign = if offset.starts_with('-') { -1 } else { 1 };
    let hours: i32 = offset[1..3].parse().unwrap_or(0);
    let minutes: i32 = offset[3..5].parse().unwrap_or(0);
    sign * (hours * 3600 + minutes * 60)
}

/// Format the date portion of a Git ident string as a relative date (%cr / %ar).
fn format_date_relative(ident: &str) -> String {
    let parts: Vec<&str> = ident.rsplitn(3, ' ').collect();
    if parts.len() >= 2 {
        let ts_str = parts[1];
        if let Ok(ts) = ts_str.parse::<i64>() {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            let diff = now - ts;
            if diff < 0 {
                return "in the future".to_string();
            }
            let diff = diff as u64;
            if diff < 60 {
                return format!("{diff} seconds ago");
            }
            let minutes = diff / 60;
            if minutes < 60 {
                return format!("{minutes} minutes ago");
            }
            let hours = minutes / 60;
            if hours < 24 {
                return format!("{hours} hours ago");
            }
            let days = hours / 24;
            if days < 14 {
                return format!("{days} days ago");
            }
            let weeks = days / 7;
            if weeks < 8 {
                return format!("{weeks} weeks ago");
            }
            let months = days / 30;
            if months < 12 {
                return format!("{months} months ago");
            }
            let years = days / 365;
            return format!("{years} years ago");
        }
    }
    ident.to_owned()
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

/// Format the date portion of a Git ident string for human display.
/// Default Git date format: "Thu Apr  7 15:13:13 2005 -0700"
fn format_date(ident: &str) -> String {
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
    let adjusted = ts + tz_offset_secs;
    let dt = time::OffsetDateTime::from_unix_timestamp(adjusted)
        .unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
    let weekday = match dt.weekday() {
        time::Weekday::Monday => "Mon",
        time::Weekday::Tuesday => "Tue",
        time::Weekday::Wednesday => "Wed",
        time::Weekday::Thursday => "Thu",
        time::Weekday::Friday => "Fri",
        time::Weekday::Saturday => "Sat",
        time::Weekday::Sunday => "Sun",
    };
    let month = match dt.month() {
        time::Month::January => "Jan",
        time::Month::February => "Feb",
        time::Month::March => "Mar",
        time::Month::April => "Apr",
        time::Month::May => "May",
        time::Month::June => "Jun",
        time::Month::July => "Jul",
        time::Month::August => "Aug",
        time::Month::September => "Sep",
        time::Month::October => "Oct",
        time::Month::November => "Nov",
        time::Month::December => "Dec",
    };
    format!(
        "{} {} {:>2} {:02}:{:02}:{:02} {} {}",
        weekday,
        month,
        dt.day(),
        dt.hour(),
        dt.minute(),
        dt.second(),
        dt.year(),
        offset_str
    )
}

/// Apply rename and/or copy detection to diff entries based on CLI flags.
fn apply_rename_copy_detection(
    odb: &Odb,
    entries: Vec<DiffEntry>,
    args: &Args,
    old_tree_oid: Option<&ObjectId>,
) -> Vec<DiffEntry> {
    let has_copies = !args.find_copies.is_empty();
    let has_renames = args.find_renames.is_some();

    if has_copies {
        let threshold = args.find_copies.last()
            .and_then(|v| v.parse::<u32>().ok())
            .or_else(|| args.find_renames.as_ref().and_then(|v| v.parse::<u32>().ok()))
            .unwrap_or(50);
        let find_copies_harder = args.find_copies.len() > 1;

        // Build source tree entries for copy detection.
        let source_tree_entries = if let Some(tree_oid) = old_tree_oid {
            collect_tree_entries_for_copies(odb, tree_oid)
        } else {
            vec![]
        };

        detect_copies(odb, entries, threshold, find_copies_harder, &source_tree_entries)
    } else if has_renames {
        let threshold = args.find_renames.as_ref()
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(50);
        detect_renames(odb, entries, threshold)
    } else {
        entries
    }
}

/// Collect all tree entries as (path, mode_str, oid) for copy detection.
fn collect_tree_entries_for_copies(
    odb: &Odb,
    tree_oid: &ObjectId,
) -> Vec<(String, String, ObjectId)> {
    let mut result = Vec::new();
    collect_tree_entries_recursive(odb, tree_oid, "", &mut result);
    result
}

fn collect_tree_entries_recursive(
    odb: &Odb,
    tree_oid: &ObjectId,
    prefix: &str,
    result: &mut Vec<(String, String, ObjectId)>,
) {
    let obj = match odb.read(tree_oid) {
        Ok(obj) => obj,
        Err(_) => return,
    };
    let tree = match parse_tree(&obj.data) {
        Ok(tree) => tree,
        Err(_) => return,
    };
    for entry in &tree {
        let name_str = String::from_utf8_lossy(&entry.name);
        let path = if prefix.is_empty() {
            name_str.into_owned()
        } else {
            format!("{prefix}/{name_str}")
        };
        if entry.mode == 0o040000 {
            collect_tree_entries_recursive(odb, &entry.oid, &path, result);
        } else {
            result.push((path, format!("{:06o}", entry.mode), entry.oid));
        }
    }
}
