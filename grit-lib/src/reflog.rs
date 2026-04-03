//! Reflog reading and management.
//!
//! The reflog records updates to refs.  Each ref's log is stored at
//! `<git-dir>/logs/<refname>` (e.g. `logs/HEAD`, `logs/refs/heads/main`).
//! Each line has the format:
//!
//! ```text
//! <old-sha> <new-sha> <name> <<email>> <timestamp> <timezone>\t<message>
//! ```

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::objects::ObjectId;

/// A single reflog entry.
#[derive(Debug, Clone)]
pub struct ReflogEntry {
    /// Previous object ID.
    pub old_oid: ObjectId,
    /// New object ID.
    pub new_oid: ObjectId,
    /// Identity string: `"Name <email> timestamp tz"`.
    pub identity: String,
    /// The log message.
    pub message: String,
}

/// Return the filesystem path for a ref's reflog.
pub fn reflog_path(git_dir: &Path, refname: &str) -> PathBuf {
    git_dir.join("logs").join(refname)
}

/// Check whether a reflog exists for the given ref.
pub fn reflog_exists(git_dir: &Path, refname: &str) -> bool {
    if crate::reftable::is_reftable_repo(git_dir) {
        return crate::reftable::reftable_reflog_exists(git_dir, refname);
    }
    let path = reflog_path(git_dir, refname);
    path.is_file()
}

/// Read all reflog entries for the given ref, in file order (oldest first).
///
/// Returns an empty vec if the reflog file does not exist.
pub fn read_reflog(git_dir: &Path, refname: &str) -> Result<Vec<ReflogEntry>> {
    if crate::reftable::is_reftable_repo(git_dir) {
        return crate::reftable::reftable_read_reflog(git_dir, refname);
    }
    let path = reflog_path(git_dir, refname);
    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(Error::Io(e)),
    };

    let mut entries = Vec::new();
    for line in content.lines() {
        if line.is_empty() {
            continue;
        }
        if let Some(entry) = parse_reflog_line(line) {
            entries.push(entry);
        }
    }
    Ok(entries)
}

/// Parse a single reflog line.
///
/// Format: `<old-hex> <new-hex> <identity>\t<message>`
fn parse_reflog_line(line: &str) -> Option<ReflogEntry> {
    // Split on tab first to separate identity from message
    let (before_tab, message) = if let Some(pos) = line.find('\t') {
        (&line[..pos], line[pos + 1..].to_string())
    } else {
        (line, String::new())
    };

    // The first 40 chars are old OID, then space, then 40 chars new OID, then space, then identity
    if before_tab.len() < 83 {
        // 40 + 1 + 40 + 1 + at least 1 char identity
        return None;
    }

    let old_hex = &before_tab[..40];
    let new_hex = &before_tab[41..81];
    let identity = before_tab[82..].to_string();

    let old_oid = old_hex.parse::<ObjectId>().ok()?;
    let new_oid = new_hex.parse::<ObjectId>().ok()?;

    Some(ReflogEntry {
        old_oid,
        new_oid,
        identity,
        message,
    })
}

/// Delete specific reflog entries by index (0-based, newest-first order).
///
/// Rewrites the reflog file, omitting entries at the given indices.
pub fn delete_reflog_entries(
    git_dir: &Path,
    refname: &str,
    indices: &[usize],
) -> Result<()> {
    let mut entries = read_reflog(git_dir, refname)?;
    if entries.is_empty() {
        return Ok(());
    }

    // Indices are in newest-first order (like show), so reverse the entries
    // to map indices correctly.
    entries.reverse();

    let indices_set: std::collections::HashSet<usize> =
        indices.iter().copied().collect();

    let path = reflog_path(git_dir, refname);
    let remaining: Vec<&ReflogEntry> = entries
        .iter()
        .enumerate()
        .filter(|(i, _)| !indices_set.contains(i))
        .map(|(_, e)| e)
        .collect();

    // Write back in file order (oldest first), so reverse again
    let mut lines = Vec::new();
    for entry in remaining.iter().rev() {
        lines.push(format_reflog_entry(entry));
    }

    fs::write(&path, lines.join(""))?;
    Ok(())
}

/// Expire (prune) reflog entries older than a given timestamp (Unix seconds).
///
/// If `expire_time` is `None`, removes all entries.
pub fn expire_reflog(
    git_dir: &Path,
    refname: &str,
    expire_time: Option<i64>,
) -> Result<usize> {
    let entries = read_reflog(git_dir, refname)?;
    if entries.is_empty() {
        return Ok(0);
    }

    let path = reflog_path(git_dir, refname);
    let mut kept = Vec::new();
    let mut pruned = 0usize;

    for entry in &entries {
        let ts = parse_timestamp_from_identity(&entry.identity);
        let dominated = match (expire_time, ts) {
            (Some(cutoff), Some(t)) => t < cutoff,
            (None, _) => true, // expire all
            (Some(_), None) => false, // can't parse => keep
        };
        if dominated {
            pruned += 1;
        } else {
            kept.push(format_reflog_entry(entry));
        }
    }

    fs::write(&path, kept.join(""))?;
    Ok(pruned)
}

/// Format a reflog entry back into the on-disk line format.
fn format_reflog_entry(entry: &ReflogEntry) -> String {
    format!(
        "{} {} {}\t{}\n",
        entry.old_oid, entry.new_oid, entry.identity, entry.message
    )
}

/// Extract the Unix timestamp from an identity string.
///
/// Identity format: `Name <email> <timestamp> <tz>`
fn parse_timestamp_from_identity(identity: &str) -> Option<i64> {
    // Walk backwards: last token is tz (+0000), second-to-last is timestamp
    let parts: Vec<&str> = identity.rsplitn(3, ' ').collect();
    if parts.len() >= 2 {
        parts[1].parse::<i64>().ok()
    } else {
        None
    }
}

/// List all refs that have reflogs.
pub fn list_reflog_refs(git_dir: &Path) -> Result<Vec<String>> {
    let logs_dir = git_dir.join("logs");
    let mut refs = Vec::new();

    // Check HEAD
    if logs_dir.join("HEAD").is_file() {
        refs.push("HEAD".to_string());
    }

    // Walk logs/refs/
    let refs_logs = logs_dir.join("refs");
    if refs_logs.is_dir() {
        collect_reflog_refs(&refs_logs, "refs", &mut refs)?;
    }

    Ok(refs)
}

fn collect_reflog_refs(dir: &Path, prefix: &str, out: &mut Vec<String>) -> Result<()> {
    let read_dir = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(Error::Io(e)),
    };

    for entry in read_dir {
        let entry = entry.map_err(Error::Io)?;
        let name = entry.file_name().to_string_lossy().to_string();
        let full_name = format!("{prefix}/{name}");
        let ft = entry.file_type().map_err(Error::Io)?;
        if ft.is_dir() {
            collect_reflog_refs(&entry.path(), &full_name, out)?;
        } else if ft.is_file() {
            out.push(full_name);
        }
    }
    Ok(())
}
