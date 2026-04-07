//! `grit revert` — revert existing commits.
//!
//! Creates new commits that undo the changes introduced by the given commits.
//! Revert is essentially a reverse cherry-pick: it applies the inverse of a
//! commit's diff onto the current HEAD.
//!
//! For a commit C with parent P:
//!   - base  = C.tree   (the commit being reverted)
//!   - ours  = HEAD.tree (current state)
//!   - theirs = P.tree   (the state before the commit)
//!
//! This three-way merge produces the revert of C's changes.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::Path;

use grit_lib::config::ConfigSet;
use grit_lib::error::Error as GritError;
use grit_lib::index::{Index, IndexEntry, MODE_EXECUTABLE, MODE_SYMLINK};
use grit_lib::merge_file::{merge, MergeInput};
use grit_lib::objects::{
    parse_commit, parse_tree, serialize_commit, CommitData, ObjectId, ObjectKind,
};
use grit_lib::repo::Repository;
use grit_lib::rev_parse::resolve_revision;
use grit_lib::state::{resolve_head, HeadState};
use grit_lib::write_tree::write_tree_from_index;

/// Arguments for `grit revert`.
#[derive(Debug, ClapArgs)]
#[command(about = "Revert some existing commits")]
pub struct Args {
    /// Commits to revert.
    #[arg(value_name = "COMMIT")]
    pub commits: Vec<String>,

    /// Apply revert to index and working tree without committing.
    #[arg(short = 'n', long = "no-commit")]
    pub no_commit: bool,

    /// For reverting merge commits, specify which parent (1-based) is mainline.
    #[arg(short = 'm', long = "mainline")]
    pub mainline: Option<usize>,

    /// Continue a revert after resolving conflicts.
    #[arg(long = "continue")]
    pub r#continue: bool,

    /// Abort an in-progress revert.
    #[arg(long = "abort")]
    pub abort: bool,
}

/// Run the `revert` command.
pub fn run(args: Args) -> Result<()> {
    if args.abort {
        return do_abort();
    }
    if args.r#continue {
        return do_continue();
    }
    if args.commits.is_empty() {
        bail!("nothing to revert; specify at least one commit");
    }
    do_revert(args)
}

// ── Main revert flow ────────────────────────────────────────────────

fn do_revert(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let git_dir = &repo.git_dir;

    // Don't start a new revert if one is already in progress.
    if git_dir.join("REVERT_HEAD").exists() {
        bail!(
            "error: a revert is already in progress\n\
             hint: use \"grit revert --continue\" to continue\n\
             hint: or \"grit revert --abort\" to abort"
        );
    }

    // Save ORIG_HEAD before starting the revert sequence
    let head = resolve_head(git_dir)?;
    if let Some(head_oid) = head.oid() {
        let _ = fs::write(
            git_dir.join("ORIG_HEAD"),
            format!("{}\n", head_oid.to_hex()),
        );
    }

    // Expand commit specs (including A..B ranges) into a list of specs
    let expanded = expand_revert_specs(&repo, &args.commits)?;

    // If multiple commits, set up sequencer state
    if expanded.len() > 1 {
        let seq_dir = git_dir.join("sequencer");
        let _ = fs::create_dir_all(&seq_dir);
        // Write todo with remaining commits
        let mut todo_entries: Vec<String> = Vec::new();
        for spec in &expanded {
            let oid =
                resolve_revision(&repo, spec).with_context(|| format!("bad revision '{spec}'"))?;
            let obj = repo.odb.read(&oid)?;
            let commit = parse_commit(&obj.data)?;
            let subject = commit.message.lines().next().unwrap_or("");
            todo_entries.push(format!("revert {} {}", &oid.to_hex()[..7], subject));
        }
        fs::write(seq_dir.join("todo"), todo_entries.join("\n") + "\n")?;
    }

    for (i, spec) in expanded.iter().enumerate() {
        match revert_one_commit(&repo, spec, args.mainline, args.no_commit) {
            Ok(()) => {
                // Update sequencer todo: remove the completed entry
                if expanded.len() > 1 {
                    let seq_dir = git_dir.join("sequencer");
                    let todo_path = seq_dir.join("todo");
                    if let Ok(content) = fs::read_to_string(&todo_path) {
                        let remaining: Vec<&str> = content.lines().skip(1).collect();
                        if remaining.is_empty() {
                            // All done, clean up sequencer
                            let _ = fs::remove_dir_all(&seq_dir);
                        } else {
                            let _ = fs::write(&todo_path, remaining.join("\n") + "\n");
                        }
                    }
                }
            }
            Err(e) => {
                // If there are remaining commits, keep the sequencer todo
                if expanded.len() > 1 && i < expanded.len() - 1 {
                    let seq_dir = git_dir.join("sequencer");
                    let todo_path = seq_dir.join("todo");
                    if let Ok(content) = fs::read_to_string(&todo_path) {
                        // Keep only remaining (unprocessed) entries
                        let remaining: Vec<&str> = content.lines().skip(i + 1).collect();
                        if !remaining.is_empty() {
                            let _ = fs::write(&todo_path, remaining.join("\n") + "\n");
                        }
                    }
                }
                return Err(e);
            }
        }
    }

    // Clean up sequencer if all commits were processed
    if expanded.len() > 1 {
        let _ = fs::remove_dir_all(git_dir.join("sequencer"));
    }

    Ok(())
}

/// Expand revert commit specs, handling A..B ranges.
/// For revert, A..B means revert commits from B down to (but not including) A,
/// in reverse order (newest first).
fn expand_revert_specs(repo: &Repository, specs: &[String]) -> Result<Vec<String>> {
    let mut result = Vec::new();
    for spec in specs {
        if let Some((lhs, rhs)) = spec.split_once("..") {
            let exclude_oid =
                resolve_revision(repo, lhs).with_context(|| format!("bad revision '{lhs}'"))?;
            let include_oid =
                resolve_revision(repo, rhs).with_context(|| format!("bad revision '{rhs}'"))?;
            let range_oids = walk_commit_range(repo, exclude_oid, include_oid)?;
            // Revert in reverse order (newest first)
            for oid in range_oids.into_iter().rev() {
                result.push(oid.to_hex());
            }
        } else {
            result.push(spec.clone());
        }
    }
    Ok(result)
}

/// Walk commits reachable from `tip` but not from `base`, oldest first.
fn walk_commit_range(repo: &Repository, base: ObjectId, tip: ObjectId) -> Result<Vec<ObjectId>> {
    let mut result = Vec::new();
    let mut current = tip;
    loop {
        if current == base {
            break;
        }
        result.push(current);
        let obj = repo.odb.read(&current)?;
        let commit = parse_commit(&obj.data)?;
        if commit.parents.is_empty() {
            break;
        }
        current = commit.parents[0];
    }
    result.reverse(); // oldest first
    Ok(result)
}

fn revert_one_commit(
    repo: &Repository,
    spec: &str,
    mainline: Option<usize>,
    no_commit: bool,
) -> Result<()> {
    let git_dir = &repo.git_dir;

    // Resolve commit to revert.
    let commit_oid =
        resolve_revision(repo, spec).with_context(|| format!("bad revision '{spec}'"))?;
    let commit_obj = repo.odb.read(&commit_oid)?;
    if commit_obj.kind != ObjectKind::Commit {
        bail!("object {} is not a commit", commit_oid);
    }
    let commit = parse_commit(&commit_obj.data)?;

    // Determine parent (base for the original change).
    let parent_oid = if commit.parents.len() > 1 {
        // Merge commit — require --mainline.
        let m = mainline.ok_or_else(|| {
            anyhow::anyhow!(
                "commit {} is a merge but no -m option was given",
                commit_oid
            )
        })?;
        if m == 0 || m > commit.parents.len() {
            bail!("commit {} does not have parent {}", commit_oid, m);
        }
        commit.parents[m - 1]
    } else if commit.parents.is_empty() {
        bail!("cannot revert a root commit (no parent)");
    } else {
        commit.parents[0]
    };

    // Read the parent commit's tree.
    let parent_obj = repo.odb.read(&parent_oid)?;
    let parent_commit = parse_commit(&parent_obj.data)?;
    let parent_tree_oid = parent_commit.tree;

    // The commit's own tree.
    let commit_tree_oid = commit.tree;

    // Resolve HEAD tree.
    let head = resolve_head(git_dir)?;
    let head_oid = head
        .oid()
        .ok_or_else(|| anyhow::anyhow!("cannot revert: HEAD does not point to a commit"))?
        .to_owned();
    let head_obj = repo.odb.read(&head_oid)?;
    let head_commit = parse_commit(&head_obj.data)?;
    let head_tree_oid = head_commit.tree;

    // Three-way merge:  base=commit_tree, ours=HEAD_tree, theirs=parent_tree
    // This effectively reverses the commit's changes.
    let base_entries = tree_to_map(tree_to_index_entries(repo, &commit_tree_oid, "")?);
    let ours_entries = tree_to_map(tree_to_index_entries(repo, &head_tree_oid, "")?);
    let theirs_entries = tree_to_map(tree_to_index_entries(repo, &parent_tree_oid, "")?);

    let merged_index =
        three_way_merge_with_content(repo, &base_entries, &ours_entries, &theirs_entries)?;

    // Check for conflicts (any entry with stage != 0).
    let has_conflicts = merged_index.entries.iter().any(|e| e.stage() != 0);

    // Check if the revert produces an empty commit (no changes).
    if !has_conflicts {
        let merged_tree = write_tree_from_index(&repo.odb, &merged_index, "")?;
        if merged_tree == head_tree_oid {
            eprintln!(
                "error: The previous revert is now empty, possibly due to conflict resolution."
            );
            // Write REVERT_HEAD so the sequencer knows the state
            fs::write(
                git_dir.join("REVERT_HEAD"),
                format!("{}\n", commit_oid.to_hex()),
            )?;
            std::process::exit(1);
        }
    }

    // Load old index BEFORE writing new one (needed for worktree cleanup).
    let old_index = load_index(repo)?;

    // Write index.
    let index_path = repo.index_path();
    merged_index.write(&index_path).context("writing index")?;

    // Update working tree.
    let work_tree = repo
        .work_tree
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("cannot revert in a bare repository"))?;
    checkout_merged_index(repo, work_tree, &old_index, &merged_index)?;

    if has_conflicts {
        // Write REVERT_HEAD and MERGE_MSG so --continue can finish.
        let short_oid = &commit_oid.to_hex()[..7];
        let subject = commit.message.lines().next().unwrap_or("");
        let msg = format!(
            "Revert \"{subject}\"\n\nThis reverts commit {oid}.\n",
            oid = commit_oid.to_hex()
        );
        fs::write(
            git_dir.join("REVERT_HEAD"),
            format!("{}\n", commit_oid.to_hex()),
        )?;
        fs::write(git_dir.join("MERGE_MSG"), &msg)?;

        eprintln!(
            "error: could not revert {short_oid}... {subject}\n\
             hint: after resolving the conflicts, mark the corrected paths\n\
             hint: with 'git add <paths>' or 'git rm <paths>'\n\
             hint: and commit the result with 'git revert --continue'"
        );
        // Exit with error code like git does.
        std::process::exit(1);
    }

    if no_commit {
        // Write REVERT_HEAD but don't commit.
        fs::write(
            git_dir.join("REVERT_HEAD"),
            format!("{}\n", commit_oid.to_hex()),
        )?;
        return Ok(());
    }

    // Build commit message.
    let subject = commit.message.lines().next().unwrap_or("");
    let msg = format!(
        "Revert \"{subject}\"\n\nThis reverts commit {oid}.\n",
        oid = commit_oid.to_hex()
    );

    // Create the revert commit.
    create_revert_commit(repo, &head, &merged_index, &msg)?;

    // Print summary.
    let short_oid_new = {
        let new_head = resolve_head(git_dir)?;
        let new_oid = new_head
            .oid()
            .ok_or_else(|| anyhow::anyhow!("HEAD has no OID"))?;
        new_oid.to_hex()[..7].to_owned()
    };
    let branch = match &head {
        HeadState::Branch { short_name, .. } => short_name.as_str(),
        HeadState::Detached { .. } => "HEAD detached",
        HeadState::Invalid => "unknown",
    };
    let first_line = msg.lines().next().unwrap_or("");
    eprintln!("[{branch} {short_oid_new}] {first_line}");

    Ok(())
}

// ── --continue ──────────────────────────────────────────────────────

fn do_continue() -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let git_dir = &repo.git_dir;

    // Must be in a revert.
    if !git_dir.join("REVERT_HEAD").exists() {
        bail!("error: no revert in progress");
    }

    // Load current index and check for unresolved conflicts.
    let index = load_index(&repo)?;
    if index.entries.iter().any(|e| e.stage() != 0) {
        bail!(
            "error: commit is not possible because you have unmerged files\n\
             hint: fix conflicts and then commit the result with 'git revert --continue'"
        );
    }

    // Read the saved message.
    let msg = match fs::read_to_string(git_dir.join("MERGE_MSG")) {
        Ok(m) => m,
        Err(_) => {
            // Fall back: construct from REVERT_HEAD.
            let revert_oid = fs::read_to_string(git_dir.join("REVERT_HEAD"))?;
            let revert_oid = revert_oid.trim();
            let oid = ObjectId::from_hex(revert_oid)?;
            let obj = repo.odb.read(&oid)?;
            let commit = parse_commit(&obj.data)?;
            let subject = commit.message.lines().next().unwrap_or("");
            format!("Revert \"{subject}\"\n\nThis reverts commit {revert_oid}.\n")
        }
    };

    let head = resolve_head(git_dir)?;
    create_revert_commit(&repo, &head, &index, &msg)?;

    // Cleanup state files.
    cleanup_revert_state(git_dir);

    let new_head = resolve_head(git_dir)?;
    let new_oid = new_head
        .oid()
        .ok_or_else(|| anyhow::anyhow!("HEAD has no OID"))?;
    let short = &new_oid.to_hex()[..7];
    let branch = match &head {
        HeadState::Branch { short_name, .. } => short_name.as_str(),
        HeadState::Detached { .. } => "HEAD detached",
        HeadState::Invalid => "unknown",
    };
    let first_line = msg.lines().next().unwrap_or("");
    eprintln!("[{branch} {short}] {first_line}");

    Ok(())
}

// ── --abort ─────────────────────────────────────────────────────────

fn do_abort() -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let git_dir = &repo.git_dir;

    if !git_dir.join("REVERT_HEAD").exists() && !git_dir.join("sequencer").join("todo").exists() {
        bail!("error: no revert in progress");
    }

    // Restore HEAD to ORIG_HEAD if available, otherwise use current HEAD tree.
    let restore_oid = if let Ok(orig) = fs::read_to_string(git_dir.join("ORIG_HEAD")) {
        use grit_lib::objects::ObjectId;
        Some(ObjectId::from_hex(orig.trim())?)
    } else {
        None
    };

    let head = resolve_head(git_dir)?;
    let target_oid = restore_oid.as_ref().or_else(|| head.oid());

    if let Some(oid) = target_oid {
        let obj = repo.odb.read(oid)?;
        let commit = parse_commit(&obj.data)?;
        let entries = tree_to_index_entries(&repo, &commit.tree, "")?;
        let old_idx = load_index(&repo)?;
        let mut index = Index::new();
        index.entries = entries;
        index.sort();
        let index_path = repo.index_path();
        index.write(&index_path)?;

        if let Some(wt) = &repo.work_tree {
            checkout_merged_index(&repo, wt, &old_idx, &index)?;
        }

        // Move HEAD back to ORIG_HEAD
        if let Some(ref orig_oid) = restore_oid {
            match &head {
                HeadState::Branch { refname, .. } => {
                    let ref_path = git_dir.join(refname);
                    let _ = fs::write(&ref_path, format!("{}\n", orig_oid.to_hex()));
                }
                HeadState::Detached { .. } => {
                    let _ = fs::write(git_dir.join("HEAD"), format!("{}\n", orig_oid.to_hex()));
                }
                HeadState::Invalid => {}
            }
        }
    }

    cleanup_revert_state(git_dir);
    let _ = fs::remove_file(git_dir.join("ORIG_HEAD"));
    Ok(())
}

// ── Helpers ─────────────────────────────────────────────────────────

fn cleanup_revert_state(git_dir: &Path) {
    let _ = fs::remove_file(git_dir.join("REVERT_HEAD"));
    let _ = fs::remove_file(git_dir.join("MERGE_MSG"));
    let _ = fs::remove_dir_all(git_dir.join("sequencer"));
}

fn load_index(repo: &Repository) -> Result<Index> {
    let index_path = repo.index_path();
    match Index::load(&index_path) {
        Ok(idx) => Ok(idx),
        Err(GritError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => Ok(Index::new()),
        Err(e) => Err(e.into()),
    }
}

fn create_revert_commit(
    repo: &Repository,
    head: &HeadState,
    index: &Index,
    message: &str,
) -> Result<()> {
    let tree_oid = write_tree_from_index(&repo.odb, index, "")?;
    let git_dir = &repo.git_dir;

    let mut parents = Vec::new();
    if let Some(head_oid) = head.oid() {
        parents.push(*head_oid);
    }

    let config = ConfigSet::load(Some(git_dir), true)?;
    let now = time::OffsetDateTime::now_utc();
    let author = resolve_identity(&config, "AUTHOR")?;
    let committer = resolve_identity(&config, "COMMITTER")?;

    let commit_data = CommitData {
        tree: tree_oid,
        parents,
        author: format_ident(&author, now),
        committer: format_ident(&committer, now),
        encoding: None,
        message: message.to_owned(),
        raw_message: None,
    };

    let commit_bytes = serialize_commit(&commit_data);
    let commit_oid = repo.odb.write(ObjectKind::Commit, &commit_bytes)?;

    // Update HEAD.
    update_head(git_dir, head, &commit_oid)?;

    // Clean up revert state.
    cleanup_revert_state(git_dir);

    Ok(())
}

fn resolve_identity(config: &ConfigSet, kind: &str) -> Result<(String, String)> {
    let name_var = format!("GIT_{kind}_NAME");
    let email_var = format!("GIT_{kind}_EMAIL");

    let name = std::env::var(&name_var)
        .ok()
        .or_else(|| config.get("user.name"))
        .unwrap_or_else(|| "Unknown".to_owned());
    let email = std::env::var(&email_var)
        .ok()
        .or_else(|| config.get("user.email"))
        .unwrap_or_default();

    Ok((name, email))
}

fn format_ident(ident: &(String, String), now: time::OffsetDateTime) -> String {
    let (name, email) = ident;
    let epoch = now.unix_timestamp();
    let offset = now.offset();
    let hours = offset.whole_hours();
    let minutes = offset.minutes_past_hour().unsigned_abs();

    let date_str = std::env::var(if name == "Unknown" {
        "GIT_COMMITTER_DATE"
    } else {
        "GIT_AUTHOR_DATE"
    })
    .ok();

    let timestamp = date_str
        .map(|d| super::commit::parse_date_to_git_timestamp(&d).unwrap_or(d))
        .unwrap_or_else(|| format!("{epoch} {hours:+03}{minutes:02}"));
    format!("{name} <{email}> {timestamp}")
}

fn update_head(git_dir: &Path, head: &HeadState, commit_oid: &ObjectId) -> Result<()> {
    match head {
        HeadState::Branch { refname, .. } => {
            let ref_path = git_dir.join(refname);
            if let Some(parent) = ref_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&ref_path, format!("{}\n", commit_oid.to_hex()))?;
        }
        HeadState::Detached { .. } | HeadState::Invalid => {
            fs::write(git_dir.join("HEAD"), format!("{}\n", commit_oid.to_hex()))?;
        }
    }
    Ok(())
}

// ── Tree → index helpers ────────────────────────────────────────────

fn tree_to_index_entries(
    repo: &Repository,
    oid: &ObjectId,
    prefix: &str,
) -> Result<Vec<IndexEntry>> {
    let obj = repo.odb.read(oid)?;
    if obj.kind != ObjectKind::Tree {
        bail!("expected tree, got {}", obj.kind);
    }
    let entries = parse_tree(&obj.data)?;
    let mut result = Vec::new();

    for te in entries {
        let name = String::from_utf8_lossy(&te.name).into_owned();
        let path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };

        if te.mode == 0o040000 {
            let sub = tree_to_index_entries(repo, &te.oid, &path)?;
            result.extend(sub);
        } else {
            let path_bytes = path.into_bytes();
            result.push(IndexEntry {
                ctime_sec: 0,
                ctime_nsec: 0,
                mtime_sec: 0,
                mtime_nsec: 0,
                dev: 0,
                ino: 0,
                mode: te.mode,
                uid: 0,
                gid: 0,
                size: 0,
                oid: te.oid,
                flags: path_bytes.len().min(0xFFF) as u16,
                flags_extended: None,
                path: path_bytes,
            });
        }
    }
    Ok(result)
}

fn tree_to_map(entries: Vec<IndexEntry>) -> HashMap<Vec<u8>, IndexEntry> {
    let mut out = HashMap::new();
    for e in entries {
        out.insert(e.path.clone(), e);
    }
    out
}

fn same_blob(a: &IndexEntry, b: &IndexEntry) -> bool {
    a.oid == b.oid && a.mode == b.mode
}

fn stage_entry(index: &mut Index, src: &IndexEntry, stage: u8) {
    let mut e = src.clone();
    e.flags = (e.flags & 0x0FFF) | ((stage as u16) << 12);
    index.entries.push(e);
}

/// Three-way merge with content-level merging for conflicting files.
///
/// base = commit_tree (what we're reverting)
/// ours = HEAD_tree (current state)
/// theirs = parent_tree (state before the commit)
fn three_way_merge_with_content(
    repo: &Repository,
    base: &HashMap<Vec<u8>, IndexEntry>,
    ours: &HashMap<Vec<u8>, IndexEntry>,
    theirs: &HashMap<Vec<u8>, IndexEntry>,
) -> Result<Index> {
    let mut all_paths = BTreeSet::new();
    all_paths.extend(base.keys().cloned());
    all_paths.extend(ours.keys().cloned());
    all_paths.extend(theirs.keys().cloned());

    let mut out = Index::new();

    for path in all_paths {
        let b = base.get(&path);
        let o = ours.get(&path);
        let t = theirs.get(&path);

        match (b, o, t) {
            // Both sides same → take ours
            (_, Some(oe), Some(te)) if same_blob(oe, te) => {
                out.entries.push(oe.clone());
            }
            // Base == ours, only theirs changed → take theirs
            (Some(be), Some(oe), Some(te)) if same_blob(be, oe) => {
                out.entries.push(te.clone());
            }
            // Base == theirs, only ours changed → take ours
            (Some(be), Some(oe), Some(te)) if same_blob(be, te) => {
                out.entries.push(oe.clone());
            }
            // All three differ → try content merge
            (Some(be), Some(oe), Some(te)) => {
                content_merge_or_conflict(repo, &mut out, &path, be, oe, te)?;
            }
            // Added only in ours (not in base, not in theirs)
            (None, Some(oe), None) => {
                out.entries.push(oe.clone());
            }
            // Added only in theirs (not in base, not in ours)
            (None, None, Some(te)) => {
                out.entries.push(te.clone());
            }
            // Added in both with same content
            (None, Some(oe), Some(te)) if same_blob(oe, te) => {
                out.entries.push(oe.clone());
            }
            // Added in both with different content → conflict
            (None, Some(oe), Some(te)) => {
                stage_entry(&mut out, oe, 2);
                stage_entry(&mut out, te, 3);
            }
            // Deleted by both → skip
            (Some(_), None, None) => {}
            // Deleted by theirs, unchanged in ours (base == ours)
            (Some(be), Some(oe), None) if same_blob(be, oe) => {
                // theirs deleted it → delete
            }
            // Deleted by ours, unchanged in theirs (base == theirs)
            (Some(be), None, Some(te)) if same_blob(be, te) => {
                // ours deleted it → delete
            }
            // Deleted by theirs, modified in ours → conflict
            (Some(be), Some(oe), None) => {
                stage_entry(&mut out, be, 1);
                stage_entry(&mut out, oe, 2);
            }
            // Deleted by ours, modified in theirs → conflict
            (Some(be), None, Some(te)) => {
                stage_entry(&mut out, be, 1);
                stage_entry(&mut out, te, 3);
            }
            // Nothing anywhere
            (None, None, None) => {}
        }
    }

    out.sort();
    Ok(out)
}

/// Try a content-level three-way merge; fall back to conflict stages.
fn content_merge_or_conflict(
    repo: &Repository,
    index: &mut Index,
    path: &[u8],
    base: &IndexEntry,
    ours: &IndexEntry,
    theirs: &IndexEntry,
) -> Result<()> {
    let base_obj = repo.odb.read(&base.oid)?;
    let ours_obj = repo.odb.read(&ours.oid)?;
    let theirs_obj = repo.odb.read(&theirs.oid)?;

    // Only attempt content merge for text blobs.
    if grit_lib::merge_file::is_binary(&base_obj.data)
        || grit_lib::merge_file::is_binary(&ours_obj.data)
        || grit_lib::merge_file::is_binary(&theirs_obj.data)
    {
        // Binary conflict.
        stage_entry(index, base, 1);
        stage_entry(index, ours, 2);
        stage_entry(index, theirs, 3);
        return Ok(());
    }

    let path_str = String::from_utf8_lossy(path);
    let input = MergeInput {
        base: &base_obj.data,
        ours: &ours_obj.data,
        theirs: &theirs_obj.data,
        label_ours: "HEAD",
        label_base: "parent of reverted commit",
        label_theirs: &path_str,
        favor: Default::default(),
        style: Default::default(),
        marker_size: 7,
        diff_algorithm: None,
        ignore_all_space: false,
        ignore_space_change: false,
        ignore_space_at_eol: false,
        ignore_cr_at_eol: false,
    };

    let result = merge(&input)?;

    if result.conflicts > 0 {
        // Add conflict stages.
        stage_entry(index, base, 1);
        stage_entry(index, ours, 2);
        stage_entry(index, theirs, 3);
    } else {
        // Clean merge → write merged blob.
        let merged_oid = repo.odb.write(ObjectKind::Blob, &result.content)?;
        let mut entry = ours.clone();
        entry.oid = merged_oid;
        // Use ours mode (or theirs if ours didn't change from base).
        if base.mode == ours.mode && base.mode != theirs.mode {
            entry.mode = theirs.mode;
        }
        index.entries.push(entry);
    }

    Ok(())
}

/// Write merged index entries to the working tree.
fn checkout_merged_index(
    repo: &Repository,
    work_tree: &Path,
    old_index: &Index,
    index: &Index,
) -> Result<()> {
    let new_paths: HashSet<Vec<u8>> = index.entries.iter().map(|e| e.path.clone()).collect();

    // Remove files that were in the old index but not in the new one.
    for entry in &old_index.entries {
        if entry.stage() == 0 && !new_paths.contains(&entry.path) {
            let path_str = String::from_utf8_lossy(&entry.path).into_owned();
            let abs_path = work_tree.join(&path_str);
            if abs_path.exists() || abs_path.is_symlink() {
                let _ = fs::remove_file(&abs_path);
                remove_empty_parent_dirs(work_tree, &abs_path);
            }
        }
    }

    // Write new entries.
    let mut written = HashSet::new();

    for entry in &index.entries {
        let path_str = String::from_utf8_lossy(&entry.path).into_owned();
        let abs_path = work_tree.join(&path_str);

        if entry.stage() == 0 {
            write_entry_to_worktree(repo, &abs_path, entry)?;
            written.insert(entry.path.clone());
        } else if entry.stage() == 2 && !written.contains(&entry.path) {
            // For conflicts, write the ours (stage 2) version to worktree.
            write_entry_to_worktree(repo, &abs_path, entry)?;
            written.insert(entry.path.clone());
        }
    }

    Ok(())
}

fn remove_empty_parent_dirs(work_tree: &Path, path: &Path) {
    let mut current = path.parent();
    while let Some(dir) = current {
        if dir == work_tree {
            break;
        }
        match fs::remove_dir(dir) {
            Ok(()) => current = dir.parent(),
            Err(_) => break,
        }
    }
}

fn write_entry_to_worktree(repo: &Repository, abs_path: &Path, entry: &IndexEntry) -> Result<()> {
    if let Some(parent) = abs_path.parent() {
        fs::create_dir_all(parent)?;
    }

    if entry.mode == 0o160000 {
        let _ = fs::create_dir_all(abs_path);
        return Ok(());
    }

    let obj = repo
        .odb
        .read(&entry.oid)
        .context("reading object for checkout")?;

    if entry.mode == MODE_SYMLINK {
        let target =
            String::from_utf8(obj.data).map_err(|_| anyhow::anyhow!("symlink not UTF-8"))?;
        if abs_path.exists() || abs_path.is_symlink() {
            let _ = fs::remove_file(abs_path);
        }
        std::os::unix::fs::symlink(target, abs_path)?;
    } else {
        if abs_path.is_dir() {
            fs::remove_dir_all(abs_path)?;
        }
        fs::write(abs_path, &obj.data)?;
        if entry.mode == MODE_EXECUTABLE {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(abs_path)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(abs_path, perms)?;
        }
    }

    Ok(())
}
