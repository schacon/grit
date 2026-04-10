//! Shallow repository metadata (`.git/shallow`).

use crate::objects::ObjectId;
use std::collections::HashSet;
use std::fs;
use std::path::Path;

/// Returns the set of commit OIDs recorded as shallow boundaries in `.git/shallow`.
///
/// For each listed commit, history must not be traversed past its parents (the parents may be
/// absent from the object database). This matches Git's behavior for `git fsck` and reachability.
#[must_use]
pub fn load_shallow_boundaries(git_dir: &Path) -> HashSet<ObjectId> {
    let shallow_path = git_dir.join("shallow");
    let mut set = HashSet::new();
    let Ok(contents) = fs::read_to_string(&shallow_path) else {
        return set;
    };
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(oid) = line.parse::<ObjectId>() {
            set.insert(oid);
        }
    }
    set
}
