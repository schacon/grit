//! Partial-clone promisor bookkeeping used by Grit.
//!
//! Git records missing objects via the promisor protocol; Grit uses a marker file
//! (`grit-promisor-missing`) so commands like `rev-list --missing=print` and
//! `backfill` can track which blob OIDs are not present locally.

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use crate::error::Result;
use crate::objects::ObjectId;

/// Basename of the marker file under the git directory.
pub const PROMISOR_MISSING_FILE: &str = "grit-promisor-missing";

/// Read OIDs listed in the promisor-missing marker (40-char hex lines).
///
/// Order matches the file; duplicate lines are skipped after the first.
#[must_use]
pub fn read_promisor_missing_oids(git_dir: &Path) -> Vec<ObjectId> {
    let path = git_dir.join(PROMISOR_MISSING_FILE);
    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for line in content.lines() {
        let t = line.trim();
        if t.len() != 40 || !t.chars().all(|c| c.is_ascii_hexdigit()) {
            continue;
        }
        if let Ok(oid) = ObjectId::from_hex(t) {
            if seen.insert(oid) {
                out.push(oid);
            }
        }
    }
    out
}

/// Rewrite the promisor-missing marker from a set of OIDs (sorted, one per line).
pub fn write_promisor_marker(git_dir: &Path, oids: &HashSet<ObjectId>) -> Result<()> {
    let path = git_dir.join(PROMISOR_MISSING_FILE);
    let mut v: Vec<String> = oids.iter().map(|o| o.to_hex()).collect();
    v.sort();
    if v.is_empty() {
        fs::write(&path, b"")?;
    } else {
        fs::write(&path, format!("{}\n", v.join("\n")))?;
    }
    Ok(())
}
