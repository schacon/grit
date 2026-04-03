//! `grit cherry-pick` — apply the changes introduced by existing commits.
//!
//! Cherry-pick applies the diff of a commit onto the current HEAD using a
//! three-way merge:
//!   - base   = parent_tree  (state before the picked commit)
//!   - ours   = HEAD_tree    (current state)
//!   - theirs = commit_tree  (the commit being picked)

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
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

/// Result of a three-way merge: the index plus any conflict content for working tree.
struct MergeResult {
    index: Index,
    /// For conflicted paths, the merged content with conflict markers (OID of blob).
    conflict_content: BTreeMap<Vec<u8>, ObjectId>,
}

/// Arguments for `grit cherry-pick`.
#[derive(Debug, ClapArgs)]
#[command(about = "Apply the changes introduced by existing commits")]
pub struct Args {
    /// Commits to cherry-pick (single commits or A..B ranges).
    #[arg(value_name = "COMMIT")]
    pub commits: Vec<String>,

    /// Append "(cherry picked from commit <sha>)" to the message.
    #[arg(short = 'x')]
    pub append_source: bool,

    /// Apply changes without committing.
    #[arg(short = 'n', long = "no-commit")]
    pub no_commit: bool,

    /// Add Signed-off-by trailer to the message.
    #[arg(short = 's', long = "signoff")]
    pub signoff: bool,

    /// For cherry-picking merge commits, specify which parent (1-based) is mainline.
    #[arg(short = 'm', long = "mainline")]
    pub mainline: Option<usize>,

    /// Continue cherry-pick after resolving conflicts.
    #[arg(long = "continue")]
    pub r#continue: bool,

    /// Abort an in-progress cherry-pick.
    #[arg(long = "abort")]
    pub abort: bool,

    /// Skip the current commit and continue.
    #[arg(long = "skip")]
    pub skip: bool,

    /// Quit the cherry-pick sequence, keeping current changes.
    #[arg(long = "quit")]
    pub quit: bool,

    /// Fast-forward if possible.
    #[arg(long = "ff")]
    pub ff: bool,

    /// Allow empty commits (already-applied content).
    #[arg(long = "allow-empty")]
    pub allow_empty: bool,
}

/// Run the `cherry-pick` command.
pub fn run(args: Args) -> Result<()> {
    if args.abort {
        return do_abort();
    }
    if args.skip {
        return do_skip(&args);
    }
    if args.quit {
        return do_quit();
    }
    if args.r#continue {
        return do_continue(&args);
    }
    if args.commits.is_empty() {
        bail!("nothing to cherry-pick; specify at least one commit");
    }
    do_cherry_pick(args)
}

// ── Main cherry-pick flow ───────────────────────────────────────────

fn do_cherry_pick(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let git_dir = &repo.git_dir;

    // Don't start a new cherry-pick if one is already in progress.
    if git_dir.join("CHERRY_PICK_HEAD").exists() {
        bail!(
            "error: a cherry-pick is already in progress\n\
             hint: use \"grit cherry-pick --continue\" to continue\n\
             hint: or \"grit cherry-pick --abort\" to abort"
        );
    }

    // Expand all commit specs (including A..B ranges) into a list of OIDs.
    let commit_oids = expand_commit_specs(&repo, &args.commits)?;

    if commit_oids.is_empty() {
        bail!("empty commit set passed");
    }

    // For multi-commit operations, save ORIG_HEAD
    if commit_oids.len() > 1 && !args.no_commit {
        save_orig_head(&repo)?;
    }

    run_commit_sequence(&repo, &commit_oids, &args)
}

/// Run a sequence of cherry-pick commits, saving sequencer state on conflict.
fn run_commit_sequence(repo: &Repository, oids: &[ObjectId], args: &Args) -> Result<()> {
    let git_dir = &repo.git_dir;

    for (i, commit_oid) in oids.iter().enumerate() {
        let remaining = &oids[i + 1..];
        match cherry_pick_one_commit(repo, *commit_oid, args) {
            Ok(()) => {}
            Err(e) => {
                let err_msg = format!("{e}");
                if err_msg.contains("CONFLICT_EXIT") {
                    // Conflict occurred — save remaining commits in sequencer
                    if !remaining.is_empty() {
                        save_sequencer_state(git_dir, remaining, args)?;
                    }
                    std::process::exit(1);
                }
                return Err(e);
            }
        }
    }

    // Clean up sequencer state on success
    cleanup_sequencer_state(git_dir);
    Ok(())
}

/// Expand commit specs, handling A..B ranges.
fn expand_commit_specs(repo: &Repository, specs: &[String]) -> Result<Vec<ObjectId>> {
    let mut oids = Vec::new();
    for spec in specs {
        if let Some((lhs, rhs)) = spec.split_once("..") {
            let exclude_oid = resolve_revision(repo, lhs)
                .with_context(|| format!("bad revision '{lhs}'"))?;
            let include_oid = resolve_revision(repo, rhs)
                .with_context(|| format!("bad revision '{rhs}'"))?;

            let range_oids = walk_commit_range(repo, exclude_oid, include_oid)?;
            oids.extend(range_oids);
        } else {
            let oid = resolve_revision(repo, spec)
                .with_context(|| format!("bad revision '{spec}'"))?;
            oids.push(oid);
        }
    }
    Ok(oids)
}

/// Walk commits reachable from `tip` but not from `base`, oldest first.
fn walk_commit_range(
    repo: &Repository,
    base: ObjectId,
    tip: ObjectId,
) -> Result<Vec<ObjectId>> {
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

fn cherry_pick_one_commit(
    repo: &Repository,
    commit_oid: ObjectId,
    args: &Args,
) -> Result<()> {
    let git_dir = &repo.git_dir;

    let commit_obj = repo.odb.read(&commit_oid)?;
    if commit_obj.kind != ObjectKind::Commit {
        bail!("object {} is not a commit", commit_oid);
    }
    let commit = parse_commit(&commit_obj.data)?;

    // Determine parent (base for the change).
    let parent_oid = if commit.parents.len() > 1 {
        let m = args.mainline.ok_or_else(|| {
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
        bail!("cannot cherry-pick a root commit (no parent)");
    } else {
        commit.parents[0]
    };

    // Read parent tree (base), commit tree (theirs), HEAD tree (ours).
    let parent_obj = repo.odb.read(&parent_oid)?;
    let parent_commit = parse_commit(&parent_obj.data)?;
    let parent_tree_oid = parent_commit.tree;

    let commit_tree_oid = commit.tree;

    let head = resolve_head(git_dir)?;
    let head_oid = head
        .oid()
        .ok_or_else(|| anyhow::anyhow!("cannot cherry-pick: HEAD does not point to a commit"))?
        .to_owned();
    let head_obj = repo.odb.read(&head_oid)?;
    let head_commit = parse_commit(&head_obj.data)?;
    let head_tree_oid = head_commit.tree;

    // Check for fast-forward possibility
    if args.ff && commit.parents.len() == 1 && commit.parents[0] == head_oid {
        update_head(git_dir, &head, &commit_oid)?;
        let entries = tree_to_index_entries(repo, &commit_tree_oid, "")?;
        let old_index = load_index(repo)?;
        let mut new_index = Index::new();
        new_index.entries = entries;
        new_index.sort();
        new_index.write(&repo.index_path()).context("writing index")?;
        if let Some(wt) = &repo.work_tree {
            checkout_merged_index(repo, wt, &old_index, &new_index, &BTreeMap::new())?;
        }

        let short = &commit_oid.to_hex()[..7];
        let branch = branch_name(&head);
        let first_line = commit.message.lines().next().unwrap_or("");
        eprintln!("[{branch} {short}] {first_line}");
        return Ok(());
    }

    // Three-way merge
    let base_entries = tree_to_map(tree_to_index_entries(repo, &parent_tree_oid, "")?);

    // For --no-commit mode, use current index as "ours" (may differ from HEAD
    // when multiple commits are being picked without committing).
    let ours_entries = if args.no_commit {
        let cur_index = load_index(repo)?;
        // If the index has only stage-0 entries and differs from HEAD tree,
        // use the index entries as ours.
        let stage0: Vec<IndexEntry> = cur_index.entries.into_iter().filter(|e| e.stage() == 0).collect();
        if !stage0.is_empty() {
            tree_to_map(stage0)
        } else {
            tree_to_map(tree_to_index_entries(repo, &head_tree_oid, "")?)
        }
    } else {
        tree_to_map(tree_to_index_entries(repo, &head_tree_oid, "")?)
    };
    let theirs_entries = tree_to_map(tree_to_index_entries(repo, &commit_tree_oid, "")?);

    let merge_result = three_way_merge_with_content(
        repo,
        &base_entries,
        &ours_entries,
        &theirs_entries,
    )?;

    let has_conflicts = merge_result.index.entries.iter().any(|e| e.stage() != 0);

    // Check for empty cherry-pick (tree unchanged from HEAD)
    if !has_conflicts && !args.allow_empty {
        let new_tree_oid = write_tree_from_index(&repo.odb, &merge_result.index, "")?;
        if new_tree_oid == head_tree_oid {
            bail!("The previous cherry-pick is now empty, possibly due to conflict resolution.\nIf you wish to commit it anyway, use --allow-empty.");
        }
    }

    let old_index = load_index(repo)?;
    merge_result.index.write(&repo.index_path()).context("writing index")?;

    let work_tree = repo
        .work_tree
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("cannot cherry-pick in a bare repository"))?;
    checkout_merged_index(repo, work_tree, &old_index, &merge_result.index, &merge_result.conflict_content)?;

    // Build the cherry-pick message.
    let mut msg = commit.message.clone();
    if args.append_source {
        let trailer = format!("\n(cherry picked from commit {})\n", commit_oid.to_hex());
        let trimmed = msg.trim_end().to_owned();
        msg = format!("{trimmed}{trailer}");
    }
    if args.signoff {
        msg = append_signoff(&msg, git_dir)?;
    }

    if has_conflicts {
        fs::write(
            git_dir.join("CHERRY_PICK_HEAD"),
            format!("{}\n", commit_oid.to_hex()),
        )?;
        fs::write(git_dir.join("MERGE_MSG"), &msg)?;

        let short_oid = &commit_oid.to_hex()[..7];
        let subject = commit.message.lines().next().unwrap_or("");
        eprintln!(
            "error: could not apply {short_oid}... {subject}\n\
             hint: after resolving the conflicts, mark the corrected paths\n\
             hint: with 'git add <paths>' or 'git rm <paths>'\n\
             hint: and commit the result with 'git cherry-pick --continue'"
        );
        bail!("CONFLICT_EXIT");
    }

    if args.no_commit {
        return Ok(());
    }

    // Create the cherry-pick commit (preserving original author).
    create_cherry_pick_commit(repo, &head, &merge_result.index, &msg, &commit)?;

    let new_head = resolve_head(git_dir)?;
    let new_oid = new_head.oid().ok_or_else(|| anyhow::anyhow!("HEAD has no OID"))?;
    let short = &new_oid.to_hex()[..7];
    let branch = branch_name(&head);
    let first_line = msg.lines().next().unwrap_or("");
    eprintln!("[{branch} {short}] {first_line}");

    Ok(())
}

fn branch_name(head: &HeadState) -> &str {
    match head {
        HeadState::Branch { short_name, .. } => short_name.as_str(),
        HeadState::Detached { .. } => "HEAD detached",
        HeadState::Invalid => "unknown",
    }
}

// ── --continue ──────────────────────────────────────────────────────

fn do_continue(args: &Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let git_dir = &repo.git_dir;

    if !git_dir.join("CHERRY_PICK_HEAD").exists() {
        bail!("error: no cherry-pick in progress");
    }

    let index = load_index(&repo)?;
    if index.entries.iter().any(|e| e.stage() != 0) {
        bail!(
            "error: commit is not possible because you have unmerged files\n\
             hint: fix conflicts and then commit the result with 'git cherry-pick --continue'"
        );
    }

    // Read the original commit for author info.
    let cp_head_content = fs::read_to_string(git_dir.join("CHERRY_PICK_HEAD"))?;
    let cp_oid = ObjectId::from_hex(cp_head_content.trim())?;
    let cp_obj = repo.odb.read(&cp_oid)?;
    let cp_commit = parse_commit(&cp_obj.data)?;

    let mut msg = match fs::read_to_string(git_dir.join("MERGE_MSG")) {
        Ok(m) => m,
        Err(_) => cp_commit.message.clone(),
    };

    if args.append_source {
        let trailer = format!("\n(cherry picked from commit {})\n", cp_oid.to_hex());
        let trimmed = msg.trim_end().to_owned();
        msg = format!("{trimmed}{trailer}");
    }
    if args.signoff {
        msg = append_signoff(&msg, git_dir)?;
    }

    let head = resolve_head(git_dir)?;
    create_cherry_pick_commit(&repo, &head, &index, &msg, &cp_commit)?;

    let new_head = resolve_head(git_dir)?;
    let new_oid = new_head.oid().ok_or_else(|| anyhow::anyhow!("HEAD has no OID"))?;
    let short = &new_oid.to_hex()[..7];
    let branch = branch_name(&head);
    let first_line = msg.lines().next().unwrap_or("");
    eprintln!("[{branch} {short}] {first_line}");

    // Now process remaining sequencer items
    let remaining = load_sequencer_todo(git_dir);
    cleanup_cherry_pick_state(git_dir);

    if !remaining.is_empty() {
        run_commit_sequence(&repo, &remaining, args)?;
    }

    Ok(())
}

// ── --abort ─────────────────────────────────────────────────────────

fn do_abort() -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let git_dir = &repo.git_dir;

    if !git_dir.join("CHERRY_PICK_HEAD").exists() {
        bail!("error: no cherry-pick in progress");
    }

    // Restore HEAD to ORIG_HEAD if available, otherwise use current HEAD tree
    let restore_oid = if let Ok(orig) = fs::read_to_string(git_dir.join("ORIG_HEAD")) {
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
        index.write(&repo.index_path())?;

        if let Some(wt) = &repo.work_tree {
            checkout_merged_index(&repo, wt, &old_idx, &index, &BTreeMap::new())?;
        }

        if let Some(orig_oid) = &restore_oid {
            update_head(git_dir, &head, orig_oid)?;
        }
    }

    cleanup_cherry_pick_state(git_dir);
    cleanup_sequencer_state(git_dir);
    let _ = fs::remove_file(git_dir.join("ORIG_HEAD"));
    Ok(())
}

// ── --skip ──────────────────────────────────────────────────────────

fn do_skip(args: &Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let git_dir = &repo.git_dir;

    if !git_dir.join("CHERRY_PICK_HEAD").exists() {
        bail!("error: no cherry-pick in progress");
    }

    // Restore HEAD tree to index and working tree (undo the conflict)
    let head = resolve_head(git_dir)?;
    if let Some(head_oid) = head.oid() {
        let obj = repo.odb.read(head_oid)?;
        let commit = parse_commit(&obj.data)?;
        let entries = tree_to_index_entries(&repo, &commit.tree, "")?;
        let old_index = load_index(&repo)?;
        let mut new_index = Index::new();
        new_index.entries = entries;
        new_index.sort();
        new_index.write(&repo.index_path())?;

        if let Some(wt) = &repo.work_tree {
            checkout_merged_index(&repo, wt, &old_index, &new_index, &BTreeMap::new())?;
        }
    }

    // Load remaining sequencer items and continue
    let remaining = load_sequencer_todo(git_dir);
    cleanup_cherry_pick_state(git_dir);

    if !remaining.is_empty() {
        run_commit_sequence(&repo, &remaining, args)?;
    } else {
        cleanup_sequencer_state(git_dir);
    }

    Ok(())
}

// ── --quit ──────────────────────────────────────────────────────────

fn do_quit() -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let git_dir = &repo.git_dir;

    if !git_dir.join("CHERRY_PICK_HEAD").exists() {
        bail!("error: no cherry-pick in progress");
    }

    cleanup_cherry_pick_state(git_dir);
    cleanup_sequencer_state(git_dir);
    Ok(())
}

// ── Sequencer state management ──────────────────────────────────────

fn save_orig_head(repo: &Repository) -> Result<()> {
    let git_dir = &repo.git_dir;
    let head = resolve_head(git_dir)?;
    if let Some(oid) = head.oid() {
        fs::write(git_dir.join("ORIG_HEAD"), format!("{}\n", oid.to_hex()))?;
    }
    Ok(())
}

fn save_sequencer_state(git_dir: &Path, remaining: &[ObjectId], args: &Args) -> Result<()> {
    let seq_dir = git_dir.join("sequencer");
    fs::create_dir_all(&seq_dir)?;

    let mut todo = String::new();
    for oid in remaining {
        todo.push_str(&format!("pick {}\n", oid.to_hex()));
    }
    fs::write(seq_dir.join("todo"), &todo)?;

    let mut opts = String::new();
    if args.append_source {
        opts.push_str("append_source\n");
    }
    if args.no_commit {
        opts.push_str("no_commit\n");
    }
    if args.signoff {
        opts.push_str("signoff\n");
    }
    if let Some(m) = args.mainline {
        opts.push_str(&format!("mainline {m}\n"));
    }
    if !opts.is_empty() {
        fs::write(seq_dir.join("opts"), &opts)?;
    }

    Ok(())
}

fn load_sequencer_todo(git_dir: &Path) -> Vec<ObjectId> {
    let todo_path = git_dir.join("sequencer").join("todo");
    match fs::read_to_string(&todo_path) {
        Ok(content) => {
            let mut oids = Vec::new();
            for line in content.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                if let Some(hex) = line.strip_prefix("pick ") {
                    if let Ok(oid) = ObjectId::from_hex(hex.trim()) {
                        oids.push(oid);
                    }
                }
            }
            oids
        }
        Err(_) => Vec::new(),
    }
}

fn cleanup_sequencer_state(git_dir: &Path) {
    let seq_dir = git_dir.join("sequencer");
    let _ = fs::remove_dir_all(&seq_dir);
}

// ── Helpers ─────────────────────────────────────────────────────────

fn cleanup_cherry_pick_state(git_dir: &Path) {
    let _ = fs::remove_file(git_dir.join("CHERRY_PICK_HEAD"));
    let _ = fs::remove_file(git_dir.join("MERGE_MSG"));
}

fn load_index(repo: &Repository) -> Result<Index> {
    let index_path = repo.index_path();
    match Index::load(&index_path) {
        Ok(idx) => Ok(idx),
        Err(GritError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => Ok(Index::new()),
        Err(e) => Err(e.into()),
    }
}

fn create_cherry_pick_commit(
    repo: &Repository,
    head: &HeadState,
    index: &Index,
    message: &str,
    original_commit: &CommitData,
) -> Result<()> {
    let tree_oid = write_tree_from_index(&repo.odb, index, "")?;
    let git_dir = &repo.git_dir;

    let mut parents = Vec::new();
    if let Some(head_oid) = head.oid() {
        parents.push(*head_oid);
    }

    let config = ConfigSet::load(Some(git_dir), true)?;
    let now = time::OffsetDateTime::now_utc();

    let author = original_commit.author.clone();
    let committer = resolve_committer_ident(&config, now)?;

    let commit_data = CommitData {
        tree: tree_oid,
        parents,
        author,
        committer,
        encoding: None,
        message: message.to_owned(),
    };

    let commit_bytes = serialize_commit(&commit_data);
    let commit_oid = repo.odb.write(ObjectKind::Commit, &commit_bytes)?;

    update_head(git_dir, head, &commit_oid)?;
    cleanup_cherry_pick_state(git_dir);

    Ok(())
}

fn resolve_committer_ident(config: &ConfigSet, now: time::OffsetDateTime) -> Result<String> {
    let name = std::env::var("GIT_COMMITTER_NAME")
        .ok()
        .or_else(|| config.get("user.name"))
        .unwrap_or_else(|| "Unknown".to_owned());
    let email = std::env::var("GIT_COMMITTER_EMAIL")
        .ok()
        .or_else(|| config.get("user.email"))
        .unwrap_or_default();

    let epoch = now.unix_timestamp();
    let offset = now.offset();
    let hours = offset.whole_hours();
    let minutes = offset.minutes_past_hour().unsigned_abs();

    let timestamp = std::env::var("GIT_COMMITTER_DATE")
        .unwrap_or_else(|_| format!("{epoch} {hours:+03}{minutes:02}"));

    Ok(format!("{name} <{email}> {timestamp}"))
}

fn append_signoff(msg: &str, git_dir: &Path) -> Result<String> {
    let config = ConfigSet::load(Some(git_dir), true)?;
    let name = std::env::var("GIT_COMMITTER_NAME")
        .ok()
        .or_else(|| config.get("user.name"))
        .unwrap_or_else(|| "Unknown".to_owned());
    let email = std::env::var("GIT_COMMITTER_EMAIL")
        .ok()
        .or_else(|| config.get("user.email"))
        .unwrap_or_default();

    let signoff_line = format!("Signed-off-by: {name} <{email}>");

    if msg.contains(&signoff_line) {
        return Ok(msg.to_owned());
    }

    let trimmed = msg.trim_end();
    Ok(format!("{trimmed}\n\n{signoff_line}\n"))
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

/// Three-way merge with content-level merging.
fn three_way_merge_with_content(
    repo: &Repository,
    base: &HashMap<Vec<u8>, IndexEntry>,
    ours: &HashMap<Vec<u8>, IndexEntry>,
    theirs: &HashMap<Vec<u8>, IndexEntry>,
) -> Result<MergeResult> {
    let mut all_paths = BTreeSet::new();
    all_paths.extend(base.keys().cloned());
    all_paths.extend(ours.keys().cloned());
    all_paths.extend(theirs.keys().cloned());

    let mut out = Index::new();
    let mut conflict_content = BTreeMap::new();

    for path in all_paths {
        let b = base.get(&path);
        let o = ours.get(&path);
        let t = theirs.get(&path);

        match (b, o, t) {
            (_, Some(oe), Some(te)) if same_blob(oe, te) => {
                out.entries.push(oe.clone());
            }
            (Some(be), Some(oe), Some(te)) if same_blob(be, oe) => {
                out.entries.push(te.clone());
            }
            (Some(be), Some(oe), Some(te)) if same_blob(be, te) => {
                out.entries.push(oe.clone());
            }
            (Some(be), Some(oe), Some(te)) => {
                content_merge_or_conflict(repo, &mut out, &mut conflict_content, &path, be, oe, te)?;
            }
            (None, Some(oe), None) => {
                out.entries.push(oe.clone());
            }
            (None, None, Some(te)) => {
                out.entries.push(te.clone());
            }
            (None, Some(oe), Some(te)) if same_blob(oe, te) => {
                out.entries.push(oe.clone());
            }
            (None, Some(oe), Some(te)) => {
                stage_entry(&mut out, oe, 2);
                stage_entry(&mut out, te, 3);
            }
            (Some(_), None, None) => {}
            (Some(be), Some(oe), None) if same_blob(be, oe) => {}
            (Some(be), None, Some(te)) if same_blob(be, te) => {}
            (Some(be), Some(oe), None) => {
                stage_entry(&mut out, be, 1);
                stage_entry(&mut out, oe, 2);
            }
            (Some(be), None, Some(te)) => {
                stage_entry(&mut out, be, 1);
                stage_entry(&mut out, te, 3);
            }
            (None, None, None) => {}
        }
    }

    out.sort();
    Ok(MergeResult { index: out, conflict_content })
}

fn content_merge_or_conflict(
    repo: &Repository,
    index: &mut Index,
    conflict_content: &mut BTreeMap<Vec<u8>, ObjectId>,
    path: &[u8],
    base: &IndexEntry,
    ours: &IndexEntry,
    theirs: &IndexEntry,
) -> Result<()> {
    let base_obj = repo.odb.read(&base.oid)?;
    let ours_obj = repo.odb.read(&ours.oid)?;
    let theirs_obj = repo.odb.read(&theirs.oid)?;

    if grit_lib::merge_file::is_binary(&base_obj.data)
        || grit_lib::merge_file::is_binary(&ours_obj.data)
        || grit_lib::merge_file::is_binary(&theirs_obj.data)
    {
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
        label_base: "parent of picked commit",
        label_theirs: &path_str,
        favor: Default::default(),
        style: Default::default(),
        marker_size: 7,
    };

    let result = merge(&input)?;

    if result.conflicts > 0 {
        // Store the conflict-marker content blob for working tree checkout
        let conflict_oid = repo.odb.write(ObjectKind::Blob, &result.content)?;
        conflict_content.insert(path.to_vec(), conflict_oid);

        stage_entry(index, base, 1);
        stage_entry(index, ours, 2);
        stage_entry(index, theirs, 3);
    } else {
        let merged_oid = repo.odb.write(ObjectKind::Blob, &result.content)?;
        let mut entry = ours.clone();
        entry.oid = merged_oid;
        if base.mode == ours.mode && base.mode != theirs.mode {
            entry.mode = theirs.mode;
        }
        index.entries.push(entry);
    }

    Ok(())
}

fn checkout_merged_index(
    repo: &Repository,
    work_tree: &Path,
    old_index: &Index,
    index: &Index,
    conflict_content: &BTreeMap<Vec<u8>, ObjectId>,
) -> Result<()> {
    let new_paths: HashSet<Vec<u8>> = index.entries.iter().map(|e| e.path.clone()).collect();

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

    let mut written = HashSet::new();
    for entry in &index.entries {
        let path_str = String::from_utf8_lossy(&entry.path).into_owned();
        let abs_path = work_tree.join(&path_str);

        if entry.stage() == 0 {
            write_entry_to_worktree(repo, &abs_path, entry)?;
            written.insert(entry.path.clone());
        } else if entry.stage() == 2 && !written.contains(&entry.path) {
            // For conflicts, prefer writing conflict-marker content if available
            if let Some(marker_oid) = conflict_content.get(&entry.path) {
                let mut marker_entry = entry.clone();
                marker_entry.oid = *marker_oid;
                write_entry_to_worktree(repo, &abs_path, &marker_entry)?;
            } else {
                write_entry_to_worktree(repo, &abs_path, entry)?;
            }
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
