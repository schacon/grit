//! Shared helpers for Git-compatible cherry-pick / revert sequencer state under
//! `.git/sequencer/`.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use grit_lib::index::Index;

use grit_lib::objects::ObjectId;
use grit_lib::state::resolve_head;

/// Kind of replay operation recorded in `sequencer/todo`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SequencerAction {
    Pick,
    Revert,
}

/// Read the first instruction line in `sequencer/todo` to determine pick vs revert.
///
/// Returns `None` if the todo file is missing, empty, or does not start with a
/// recognized command (`pick` / `revert`).
pub fn sequencer_last_action(git_dir: &Path) -> Option<SequencerAction> {
    let path = git_dir.join("sequencer").join("todo");
    let content = fs::read_to_string(path).ok()?;
    let line = content.lines().find(|l| {
        let t = l.trim();
        !t.is_empty() && !t.starts_with('#')
    })?;
    let mut parts = line.split_whitespace();
    let cmd = parts.next()?;
    match cmd {
        "pick" => Some(SequencerAction::Pick),
        "revert" => Some(SequencerAction::Revert),
        _ => None,
    }
}

/// True when `sequencer/todo` exists and its first real line is a `pick` command.
pub fn sequencer_is_pick_sequence(git_dir: &Path) -> bool {
    sequencer_last_action(git_dir) == Some(SequencerAction::Pick)
}

/// True when `sequencer/todo` exists and its first real line is a `revert` command.
pub fn sequencer_is_revert_sequence(git_dir: &Path) -> bool {
    sequencer_last_action(git_dir) == Some(SequencerAction::Revert)
}

/// Write `sequencer/abort-safety` with the current `HEAD` OID (or empty for null/unborn).
pub fn write_abort_safety_file(git_dir: &Path) -> std::io::Result<()> {
    let seq_dir = git_dir.join("sequencer");
    fs::create_dir_all(&seq_dir)?;
    let head =
        resolve_head(git_dir).map_err(|e| std::io::Error::other(format!("resolve HEAD: {e}")))?;
    let line = match head.oid() {
        Some(oid) => format!("{}\n", oid.to_hex()),
        None => "\n".to_string(),
    };
    fs::write(seq_dir.join("abort-safety"), line)
}

fn null_oid() -> ObjectId {
    ObjectId::from_hex("0000000000000000000000000000000000000000").unwrap()
}

/// Read the OID stored in `abort-safety`, or all-zero if missing/empty (matches Git).
pub fn read_abort_safety_oid(git_dir: &Path) -> ObjectId {
    let path = git_dir.join("sequencer").join("abort-safety");
    let Ok(content) = fs::read_to_string(path) else {
        return null_oid();
    };
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return null_oid();
    }
    ObjectId::from_hex(trimmed).unwrap_or_else(|_| null_oid())
}

/// True if current `HEAD` matches the OID in `abort-safety` (Git `rollback_is_safe`).
pub fn rollback_is_safe(git_dir: &Path) -> bool {
    let expected = read_abort_safety_oid(git_dir);
    let head = match resolve_head(git_dir) {
        Ok(h) => h,
        Err(_) => return false,
    };
    let actual = match head.oid() {
        Some(o) => *o,
        None => null_oid(),
    };
    actual == expected
}

/// Append Git's scissors + `# Conflicts:` trailer to `MERGE_MSG` during sequencer conflicts.
pub(crate) fn append_merge_msg_conflict_footer(msg: &mut String, conflicted_paths: &[Vec<u8>]) {
    msg.push('\n');
    msg.push_str("# ------------------------ >8 ------------------------\n");
    msg.push_str("# Do not modify or remove the line above.\n");
    msg.push_str("# Everything below it will be ignored.\n");
    msg.push_str("#\n");
    msg.push_str("# Conflicts:\n");
    for p in conflicted_paths {
        msg.push_str("#\t");
        msg.push_str(&String::from_utf8_lossy(p));
        msg.push('\n');
    }
}

/// Collect paths with unmerged index entries (sorted, unique).
pub(crate) fn unmerged_paths(index: &Index) -> Vec<Vec<u8>> {
    let mut paths: BTreeSet<Vec<u8>> = BTreeSet::new();
    for e in &index.entries {
        if e.stage() != 0 {
            paths.insert(e.path.clone());
        }
    }
    paths.into_iter().collect()
}

/// Remove the first non-empty, non-comment line from `sequencer/todo`.
pub fn strip_first_sequencer_todo_line(git_dir: &Path) -> std::io::Result<()> {
    let path = git_dir.join("sequencer").join("todo");
    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    let mut removed = false;
    let mut out = Vec::new();
    for line in content.lines() {
        let t = line.trim();
        if !removed && !t.is_empty() && !t.starts_with('#') {
            removed = true;
            continue;
        }
        out.push(line);
    }
    let new_content = if out.is_empty() {
        String::new()
    } else {
        out.join("\n") + "\n"
    };
    fs::write(path, new_content)
}
