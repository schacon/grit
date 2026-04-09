//! `grit history` — history rewriting (reword, etc.).

use crate::commands::commit::{cleanup_edited_commit_message, launch_commit_editor};
use crate::commands::replay::replay_commits_onto;
use crate::commands::update_ref::resolve_reflog_identity;
use anyhow::{bail, Context, Result};
use clap::{Args as ClapArgs, Parser, Subcommand};
use grit_lib::diff::{diff_trees, DiffEntry, DiffStatus};
use grit_lib::merge_base::is_ancestor;
use grit_lib::objects::{
    parse_commit, parse_tag, serialize_commit, CommitData, ObjectId, ObjectKind,
};
use grit_lib::refs::{append_reflog, list_refs, read_head, resolve_ref, write_ref};
use grit_lib::repo::Repository;
use grit_lib::rev_parse::resolve_revision;
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet, VecDeque};
use std::fs;
use std::io::Write;

const EMPTY_TREE_OID: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

/// Arguments for `grit history`.
#[derive(Debug, Parser)]
#[command(name = "grit-history", about = "Rewrite history")]
pub struct Args {
    #[command(subcommand)]
    pub command: HistoryCommand,
}

#[derive(Debug, Subcommand)]
pub enum HistoryCommand {
    /// Change a commit message and replay descendants.
    Reword(RewordArgs),
}

#[derive(Debug, ClapArgs)]
pub struct RewordArgs {
    /// Print ref updates without modifying the repository.
    #[arg(short = 'n', long)]
    pub dry_run: bool,

    /// Limit which refs are updated: `branches` (default) or `head`.
    #[arg(long = "update-refs", value_name = "ACTION")]
    pub update_refs: Option<String>,

    /// Commit to reword.
    #[arg(value_name = "COMMIT")]
    pub commit: String,
}

/// Run `grit history`.
pub fn run(args: Args) -> Result<()> {
    match args.command {
        HistoryCommand::Reword(r) => run_reword(r),
    }
}

/// Parse `argv` after the `history` token (e.g. `["reword", "HEAD"]`) and run.
pub fn run_from_argv(rest: &[String]) -> Result<()> {
    if rest.is_empty() {
        bail!("need a subcommand");
    }
    let mut argv = vec!["grit-history".to_owned()];
    argv.extend(rest.iter().cloned());
    let args = Args::try_parse_from(&argv).map_err(|e| anyhow::anyhow!("{}", e))?;
    run(args)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UpdateRefsMode {
    Branches,
    Head,
}

pub(crate) fn run_reword(args: RewordArgs) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let mode = match args.update_refs.as_deref() {
        None | Some("branches") => UpdateRefsMode::Branches,
        Some("head") => UpdateRefsMode::Head,
        Some(_) => {
            bail!("--update-refs expects one of 'branches' or 'head'");
        }
    };

    let original = resolve_revision(&repo, &args.commit)
        .with_context(|| format!("commit cannot be found: {}", args.commit))?;

    let original_obj = repo.odb.read(&original)?;
    let original_commit = parse_commit(&original_obj.data)?;

    if original_commit.parents.len() > 1 {
        let parent_obj = repo.odb.read(&original_commit.parents[0])?;
        let parent_c = parse_commit(&parent_obj.data)?;
        if parent_c.parents.len() > 1 {
            bail!("replaying merge commits is not supported yet!");
        }
    }

    let descendants = collect_descendants_to_replay(&repo, original, mode)?;
    if descendants.iter().any(|oid| {
        repo.odb
            .read(oid)
            .ok()
            .and_then(|o| parse_commit(&o.data).ok())
            .is_some_and(|c| c.parents.len() > 1)
    }) {
        bail!("replaying merge commits is not supported yet!");
    }

    let head_oid = resolve_revision(&repo, "HEAD").context("cannot look up HEAD")?;
    if mode == UpdateRefsMode::Head && !is_ancestor(&repo, original, head_oid)? {
        bail!("rewritten commit must be an ancestor of HEAD when using --update-refs=head");
    }

    let new_message = edit_reword_message(&repo, &original_commit)?;
    if new_message.trim().is_empty() {
        eprintln!("Aborting commit due to empty commit message.");
        bail!("empty commit message");
    }

    let rewritten = write_reworded_commit(
        &repo,
        &original_commit,
        &new_message,
        descendants.as_slice(),
    )?;

    let reflog_msg = format!("reword: updating {}", args.commit);
    apply_ref_updates(
        &repo,
        original,
        rewritten,
        &descendants,
        mode,
        args.dry_run,
        &reflog_msg,
    )?;

    Ok(())
}

fn collect_descendants_to_replay(
    repo: &Repository,
    original: ObjectId,
    mode: UpdateRefsMode,
) -> Result<Vec<ObjectId>> {
    let mut tip_set: HashSet<ObjectId> = HashSet::new();
    match mode {
        UpdateRefsMode::Branches => {
            if let Ok(h) = resolve_ref(&repo.git_dir, "HEAD") {
                tip_set.insert(h);
            }
            for (_, oid) in list_refs(&repo.git_dir, "refs/")? {
                if let Ok(c) = peel_to_commit(repo, oid) {
                    tip_set.insert(c);
                }
            }
        }
        UpdateRefsMode::Head => {
            tip_set.insert(resolve_ref(&repo.git_dir, "HEAD")?);
        }
    }

    let mut seen_walk: HashSet<ObjectId> = HashSet::new();
    let mut queue: VecDeque<ObjectId> = VecDeque::new();
    for t in tip_set {
        if seen_walk.insert(t) {
            queue.push_back(t);
        }
    }

    while let Some(oid) = queue.pop_front() {
        let obj = repo.odb.read(&oid)?;
        let c = parse_commit(&obj.data)?;
        for p in &c.parents {
            if seen_walk.insert(*p) {
                queue.push_back(*p);
            }
        }
    }

    let mut selected: HashSet<ObjectId> = HashSet::new();
    for &oid in &seen_walk {
        if oid != original && is_ancestor(repo, original, oid)? {
            selected.insert(oid);
        }
    }

    let mut order = topo_sort_descendants(repo, &selected)?;
    order.reverse();
    Ok(order)
}

fn peel_to_commit(repo: &Repository, mut oid: ObjectId) -> Result<ObjectId> {
    loop {
        let object = repo.odb.read(&oid)?;
        match object.kind {
            ObjectKind::Commit => return Ok(oid),
            ObjectKind::Tag => {
                let tag = parse_tag(&object.data)?;
                oid = tag.object;
            }
            _ => {
                bail!("peel_to_commit: not a commit");
            }
        }
    }
}

#[derive(Clone, Copy)]
struct TopoKey {
    oid: ObjectId,
    time: i64,
}

impl Eq for TopoKey {}

impl PartialEq for TopoKey {
    fn eq(&self, other: &Self) -> bool {
        self.oid == other.oid
    }
}

impl Ord for TopoKey {
    fn cmp(&self, other: &Self) -> Ordering {
        self.time
            .cmp(&other.time)
            .then_with(|| self.oid.cmp(&other.oid))
    }
}

impl PartialOrd for TopoKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn committer_timestamp(data: &CommitData) -> i64 {
    fn ts_from_ident(line: &str) -> i64 {
        let parts = line.split_whitespace().collect::<Vec<_>>();
        if parts.len() < 2 {
            return 0;
        }
        let ts = parts[parts.len().saturating_sub(2)];
        ts.parse::<i64>().unwrap_or(0)
    }
    ts_from_ident(&data.committer)
}

fn topo_sort_descendants(repo: &Repository, selected: &HashSet<ObjectId>) -> Result<Vec<ObjectId>> {
    let mut child_count: HashMap<ObjectId, usize> = selected.iter().map(|&oid| (oid, 0)).collect();
    for &oid in selected {
        let obj = repo.odb.read(&oid)?;
        let c = parse_commit(&obj.data)?;
        for p in &c.parents {
            if selected.contains(p) {
                if let Some(n) = child_count.get_mut(p) {
                    *n += 1;
                }
            }
        }
    }

    let mut heap = BinaryHeap::new();
    for (&oid, &cnt) in &child_count {
        if cnt == 0 {
            let obj = repo.odb.read(&oid)?;
            let c = parse_commit(&obj.data)?;
            heap.push(TopoKey {
                oid,
                time: committer_timestamp(&c),
            });
        }
    }

    let mut out = Vec::with_capacity(selected.len());
    while let Some(item) = heap.pop() {
        let oid = item.oid;
        out.push(oid);
        let obj = repo.odb.read(&oid)?;
        let c = parse_commit(&obj.data)?;
        for p in &c.parents {
            if !selected.contains(p) {
                continue;
            }
            if let Some(cnt) = child_count.get_mut(p) {
                *cnt = cnt.saturating_sub(1);
                if *cnt == 0 {
                    let po = repo.odb.read(p)?;
                    let pc = parse_commit(&po.data)?;
                    heap.push(TopoKey {
                        oid: *p,
                        time: committer_timestamp(&pc),
                    });
                }
            }
        }
    }

    Ok(out)
}

fn edit_reword_message(repo: &Repository, commit: &CommitData) -> Result<String> {
    let parent_tree = if let Some(&p) = commit.parents.first() {
        let po = repo.odb.read(&p)?;
        parse_commit(&po.data)?.tree
    } else {
        ObjectId::from_hex(EMPTY_TREE_OID).map_err(|_| anyhow::anyhow!("bad empty tree"))?
    };

    let subject = commit.message.split('\n').next().unwrap_or("").to_owned();

    let mut buf = String::new();
    buf.push_str(&subject);
    buf.push('\n');
    buf.push('\n');
    buf.push_str(
        "# Please enter the commit message for the reworded changes. Lines starting\n\
         # with '#' will be ignored, and an empty message aborts the commit.\n",
    );

    let edit_path = repo.git_dir.join("COMMIT_EDITMSG");
    fs::write(&edit_path, &buf).context("writing COMMIT_EDITMSG")?;

    {
        let mut f = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&edit_path)?;
        append_tree_diff_status(repo, &parent_tree, &commit.tree, &mut f)?;
    }

    launch_commit_editor(repo, &edit_path)?;

    let edited = fs::read_to_string(&edit_path).context("reading COMMIT_EDITMSG")?;
    Ok(cleanup_edited_commit_message(&edited))
}

fn append_tree_diff_status(
    repo: &Repository,
    old_tree: &ObjectId,
    new_tree: &ObjectId,
    w: &mut dyn Write,
) -> Result<()> {
    writeln!(w, "# Changes to be committed:")?;
    let entries = diff_trees(&repo.odb, Some(old_tree), Some(new_tree), "")?;
    let mut paths: Vec<&DiffEntry> = entries.iter().collect();
    paths.sort_by(|a, b| {
        let pa = a
            .new_path
            .as_deref()
            .or(a.old_path.as_deref())
            .unwrap_or("");
        let pb = b
            .new_path
            .as_deref()
            .or(b.old_path.as_deref())
            .unwrap_or("");
        pa.cmp(pb)
    });
    for e in paths {
        let path = e
            .new_path
            .as_deref()
            .or(e.old_path.as_deref())
            .unwrap_or("?");
        let label = match e.status {
            DiffStatus::Added => "new file",
            DiffStatus::Deleted => "deleted",
            DiffStatus::Modified => "modified",
            DiffStatus::Renamed => "renamed",
            DiffStatus::Copied => "copied",
            DiffStatus::TypeChanged => "typechange",
            DiffStatus::Unmerged => "unmerged",
        };
        // Match `git commit` short status: "new file:   path" (three spaces after colon).
        writeln!(w, "#\t{label}:   {path}")?;
    }
    writeln!(w, "#")?;
    Ok(())
}

fn epoch_from_ident_line(ident: &str) -> i64 {
    if let Some(gt) = ident.rfind('>') {
        let after = ident[gt + 1..].trim();
        if let Some(epoch_str) = after.split_whitespace().next() {
            return epoch_str.parse::<i64>().unwrap_or(0);
        }
    }
    0
}

fn min_committer_epoch_among(repo: &Repository, oids: &[ObjectId]) -> Result<i64> {
    let mut min_e = i64::MAX;
    for oid in oids {
        let obj = repo.odb.read(oid)?;
        let c = parse_commit(&obj.data)?;
        let e = epoch_from_ident_line(&c.committer);
        min_e = min_e.min(e);
    }
    if min_e == i64::MAX {
        Ok(0)
    } else {
        Ok(min_e)
    }
}

fn committer_with_epoch(base_ident: &str, epoch: i64) -> String {
    if let Some(gt) = base_ident.rfind('>') {
        let prefix = &base_ident[..=gt];
        let after = base_ident[gt + 1..].trim();
        let tz = after
            .split_whitespace()
            .nth(1)
            .unwrap_or("+0000")
            .to_owned();
        return format!("{prefix} {epoch} {tz}");
    }
    base_ident.to_owned()
}

fn write_reworded_commit(
    repo: &Repository,
    original: &CommitData,
    message: &str,
    descendants: &[ObjectId],
) -> Result<ObjectId> {
    let mut body = message.to_owned();
    if !body.ends_with('\n') {
        body.push('\n');
    }

    let mut committer = original.committer.clone();
    if !descendants.is_empty() {
        let min_desc = min_committer_epoch_among(repo, descendants)?;
        let orig_e = epoch_from_ident_line(&original.committer);
        let target = min_desc.min(orig_e).saturating_sub(1);
        committer = committer_with_epoch(&original.committer, target);
    }

    let committer_raw = if committer == original.committer {
        original.committer_raw.clone()
    } else {
        Vec::new()
    };
    let commit = CommitData {
        tree: original.tree,
        parents: original.parents.clone(),
        author: original.author.clone(),
        committer,
        author_raw: original.author_raw.clone(),
        committer_raw,
        encoding: None,
        message: body,
        raw_message: None,
    };
    let bytes = serialize_commit(&commit);
    repo.odb
        .write(ObjectKind::Commit, &bytes)
        .context("failed writing reworded commit")
}

fn apply_ref_updates(
    repo: &Repository,
    original: ObjectId,
    rewritten: ObjectId,
    descendants: &[ObjectId],
    mode: UpdateRefsMode,
    dry_run: bool,
    reflog_msg: &str,
) -> Result<()> {
    let detached_head = read_head(&repo.git_dir)?.is_none();
    let identity = resolve_reflog_identity(repo);

    let selected_set: HashSet<ObjectId> = descendants.iter().copied().collect();

    let mut updates: Vec<(String, ObjectId, ObjectId)> = Vec::new();

    for (refname, oid) in list_refs(&repo.git_dir, "refs/")? {
        if mode == UpdateRefsMode::Head {
            continue;
        }
        if oid == original {
            updates.push((refname, rewritten, original));
            continue;
        }
        if is_ancestor(repo, original, oid)? && oid != original {
            let subset: HashSet<ObjectId> = selected_set
                .iter()
                .copied()
                .filter(|&c| is_ancestor(repo, c, oid).unwrap_or(false))
                .collect();
            if subset.is_empty() {
                continue;
            }
            let chain: Vec<ObjectId> = descendants
                .iter()
                .copied()
                .filter(|c| subset.contains(c))
                .collect();
            let (new_oid, _) = replay_commits_onto(repo, &chain, rewritten)?;
            updates.push((refname, new_oid, oid));
        }
    }

    if mode == UpdateRefsMode::Branches {
        if detached_head {
            if let Ok(head_oid) = resolve_ref(&repo.git_dir, "HEAD") {
                if head_oid == original {
                    updates.push(("HEAD".to_owned(), rewritten, original));
                } else if is_ancestor(repo, original, head_oid)? {
                    let subset: HashSet<ObjectId> = selected_set
                        .iter()
                        .copied()
                        .filter(|&c| is_ancestor(repo, c, head_oid).unwrap_or(false))
                        .collect();
                    if !subset.is_empty() {
                        let chain: Vec<ObjectId> = descendants
                            .iter()
                            .copied()
                            .filter(|c| subset.contains(c))
                            .collect();
                        let (new_oid, _) = replay_commits_onto(repo, &chain, rewritten)?;
                        updates.push(("HEAD".to_owned(), new_oid, head_oid));
                    }
                }
            }
        }
    } else if mode == UpdateRefsMode::Head {
        if let Ok(head_oid) = resolve_ref(&repo.git_dir, "HEAD") {
            let head_leaf = read_head(&repo.git_dir)?.unwrap_or_else(|| "HEAD".to_owned());
            if head_oid == original {
                updates.push((head_leaf, rewritten, original));
            } else if is_ancestor(repo, original, head_oid)? {
                let subset: HashSet<ObjectId> = selected_set
                    .iter()
                    .copied()
                    .filter(|&c| is_ancestor(repo, c, head_oid).unwrap_or(false))
                    .collect();
                if !subset.is_empty() {
                    let chain: Vec<ObjectId> = descendants
                        .iter()
                        .copied()
                        .filter(|c| subset.contains(c))
                        .collect();
                    let (new_oid, _) = replay_commits_onto(repo, &chain, rewritten)?;
                    updates.push((head_leaf, new_oid, head_oid));
                }
            }
        }
    }

    let mut by_ref: HashMap<String, (ObjectId, ObjectId)> = HashMap::new();
    for (r, n, o) in updates {
        by_ref.insert(r, (n, o));
    }

    for (refname, (new_oid, old_oid)) in &by_ref {
        if dry_run {
            println!(
                "update {} {} {}",
                refname,
                new_oid.to_hex(),
                old_oid.to_hex()
            );
        } else {
            write_ref(&repo.git_dir, refname, new_oid)
                .with_context(|| format!("failed to update ref '{refname}'"))?;
            let _ = append_reflog(
                &repo.git_dir,
                refname,
                old_oid,
                new_oid,
                &identity,
                reflog_msg,
                false,
            );
        }
    }

    if !dry_run {
        if let Ok(Some(branch)) = read_head(&repo.git_dir) {
            if let Some((new_oid, old_oid)) = by_ref.get(&branch) {
                let _ = append_reflog(
                    &repo.git_dir,
                    "HEAD",
                    old_oid,
                    new_oid,
                    &identity,
                    reflog_msg,
                    false,
                );
            }
        }
    }

    Ok(())
}
