//! `grit format-patch` — generate patch files from commits.
//!
//! Produces email-style patch files (with From/Subject/Date headers and a diff)
//! for each commit in a range.  Output goes to individual `.patch` files in the
//! current directory (or `-o <dir>`), or to stdout with `--stdout`.

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use grit_lib::diff::{diff_trees, unified_diff, zero_oid};
use grit_lib::objects::{parse_commit, CommitData, ObjectId};
use grit_lib::odb::Odb;
use grit_lib::repo::Repository;
use grit_lib::rev_parse::resolve_revision;
use std::io::{self, Write};
use std::path::PathBuf;

/// Arguments for `grit format-patch`.
#[derive(Debug, ClapArgs)]
#[command(about = "Prepare patches for e-mail submission")]
pub struct Args {
    /// Revision or count. Use a commit ref (e.g. `HEAD~3`) to generate patches
    /// for all commits since that ref, or a negative number (`-3`) for last N commits.
    #[arg(allow_hyphen_values = true)]
    pub revision: Option<String>,

    /// Write output to stdout instead of individual files.
    #[arg(long)]
    pub stdout: bool,

    /// Add `[PATCH n/m]` numbering to subjects.
    #[arg(short = 'n', long = "numbered")]
    pub numbered: bool,

    /// Custom subject prefix (default: "PATCH").
    #[arg(long = "subject-prefix", value_name = "PREFIX")]
    pub subject_prefix: Option<String>,

    /// Output directory for patch files.
    #[arg(short = 'o', long = "output-directory", value_name = "DIR")]
    pub output_directory: Option<PathBuf>,
}

pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;

    // Determine the list of commits to format
    let revision = args.revision.as_deref().unwrap_or("-1");
    let commits = collect_commits(&repo, revision)?;

    if commits.is_empty() {
        return Ok(());
    }

    let total = commits.len();
    let prefix = args.subject_prefix.as_deref().unwrap_or("PATCH");

    // Ensure output directory exists
    let out_dir = if let Some(ref dir) = args.output_directory {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("cannot create output directory '{}'", dir.display()))?;
        dir.clone()
    } else {
        std::env::current_dir().context("cannot determine current directory")?
    };

    let stdout_handle = io::stdout();

    for (idx, (oid, commit)) in commits.iter().enumerate() {
        let patch_num = idx + 1;
        let subject_line = commit.message.lines().next().unwrap_or("");

        // Build the subject with optional numbering
        let subject = if args.numbered || total > 1 {
            format!("[{prefix} {patch_num}/{total}] {subject_line}")
        } else {
            format!("[{prefix}] {subject_line}")
        };

        // Format the patch
        let patch = format_single_patch(&repo.odb, oid, commit, &subject)?;

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
                sanitize_subject(subject_line)
            );
            let path = out_dir.join(&filename);
            std::fs::write(&path, &patch)
                .with_context(|| format!("cannot write patch file '{}'", path.display()))?;
            println!("{}", path.display());
        }
    }

    Ok(())
}

/// Collect commits to format, in patch order (oldest first).
fn collect_commits(
    repo: &Repository,
    revision: &str,
) -> Result<Vec<(ObjectId, CommitData)>> {
    // Check if it's a `-<n>` count form
    if let Some(count_str) = revision.strip_prefix('-') {
        if let Ok(count) = count_str.parse::<usize>() {
            return collect_last_n_commits(repo, count);
        }
    }

    // Otherwise treat as a "since" revision — all commits after it up to HEAD
    let since_oid = resolve_revision(repo, revision)
        .with_context(|| format!("unknown revision '{revision}'"))?;

    // Walk from HEAD back, stop when we hit since_oid
    let head_oid = resolve_head_oid(repo)?;
    let mut commits = Vec::new();
    let mut current = head_oid;

    loop {
        if current == since_oid {
            break;
        }
        let obj = repo.odb.read(&current).context("reading commit")?;
        let commit = parse_commit(&obj.data).context("parsing commit")?;
        let parent = commit.parents.first().copied();
        commits.push((current, commit));
        match parent {
            Some(p) => current = p,
            None => break, // Root commit
        }
    }

    // Reverse so oldest is first (patch order)
    commits.reverse();
    Ok(commits)
}

/// Collect the last N commits from HEAD.
fn collect_last_n_commits(
    repo: &Repository,
    count: usize,
) -> Result<Vec<(ObjectId, CommitData)>> {
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

/// Resolve HEAD to an ObjectId.
fn resolve_head_oid(repo: &Repository) -> Result<ObjectId> {
    let head = grit_lib::state::resolve_head(&repo.git_dir)
        .context("cannot resolve HEAD")?;
    head.oid()
        .copied()
        .ok_or_else(|| anyhow::anyhow!("HEAD is unborn"))
}

/// Format a single commit as an email-style patch.
fn format_single_patch(
    odb: &Odb,
    oid: &ObjectId,
    commit: &CommitData,
    subject: &str,
) -> Result<String> {
    let mut out = String::new();

    // From line
    out.push_str(&format!("From {} Mon Sep 17 00:00:00 2001\n", oid.to_hex()));

    // From: author
    let author_display = format_ident(&commit.author);
    out.push_str(&format!("From: {author_display}\n"));

    // Date: from author timestamp
    let date = format_date_rfc2822(&commit.author);
    out.push_str(&format!("Date: {date}\n"));

    // Subject
    out.push_str(&format!("Subject: {subject}\n"));
    out.push('\n');

    // Commit message body (skip first line which is in Subject)
    let body: String = commit
        .message
        .lines()
        .skip(1)
        .collect::<Vec<_>>()
        .join("\n");
    let body = body.trim_start_matches('\n');
    if !body.is_empty() {
        out.push_str(body);
        out.push('\n');
    }

    out.push_str("---\n");

    // Generate the diff
    let parent_tree = commit.parents.first().map(|parent_oid| {
        odb.read(parent_oid)
            .ok()
            .and_then(|obj| parse_commit(&obj.data).ok())
            .map(|c| c.tree)
    });
    let parent_tree_oid: Option<ObjectId> = parent_tree.flatten();

    let diff_entries = diff_trees(odb, parent_tree_oid.as_ref(), Some(&commit.tree), "")
        .context("computing diff")?;

    // Stat summary
    let mut stat_lines = Vec::new();
    let mut total_ins = 0;
    let mut total_del = 0;
    let mut max_path_len = 0;

    for entry in &diff_entries {
        let path = entry.path().to_owned();
        if path.len() > max_path_len {
            max_path_len = path.len();
        }

        let old_content = read_blob_content(odb, &entry.old_oid);
        let new_content = read_blob_content(odb, &entry.new_oid);
        let (ins, del) = grit_lib::diff::count_changes(&old_content, &new_content);
        total_ins += ins;
        total_del += del;
        stat_lines.push(grit_lib::diff::format_stat_line(&path, ins, del, max_path_len));
    }

    for line in &stat_lines {
        out.push_str(line);
        out.push('\n');
    }

    let files_changed = diff_entries.len();
    out.push_str(&format!(
        " {} file{} changed",
        files_changed,
        if files_changed == 1 { "" } else { "s" }
    ));
    if total_ins > 0 {
        out.push_str(&format!(
            ", {} insertion{}(+)",
            total_ins,
            if total_ins == 1 { "" } else { "s" }
        ));
    }
    if total_del > 0 {
        out.push_str(&format!(
            ", {} deletion{}(-)",
            total_del,
            if total_del == 1 { "" } else { "s" }
        ));
    }
    out.push('\n');
    out.push('\n');

    // Full diff
    for entry in &diff_entries {
        let old_path = entry.old_path.as_deref().unwrap_or("/dev/null");
        let new_path = entry.new_path.as_deref().unwrap_or("/dev/null");

        // diff --git header
        write_diff_header_to_string(&mut out, entry);

        let old_content = read_blob_content(odb, &entry.old_oid);
        let new_content = read_blob_content(odb, &entry.new_oid);
        let patch = unified_diff(&old_content, &new_content, old_path, new_path, 3);
        out.push_str(&patch);
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
fn write_diff_header_to_string(out: &mut String, entry: &grit_lib::diff::DiffEntry) {
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

    let _ = writeln!(out, "diff --git a/{old_path} b/{new_path}");

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

/// Extract date from identity string and format as RFC 2822-like.
fn format_date_rfc2822(ident: &str) -> String {
    // Git ident: "Name <email> timestamp offset"
    let parts: Vec<&str> = ident.rsplitn(3, ' ').collect();
    if parts.len() >= 2 {
        let ts_str = parts[1];
        let offset_str = parts[0];
        if let Ok(ts) = ts_str.parse::<i64>() {
            // Parse the offset string (e.g. "+0000", "-0700") into a UtcOffset
            let tz_offset = parse_tz_offset(offset_str)
                .unwrap_or(time::UtcOffset::UTC);
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
