//! `grit replay` — replay commits on a new base.
//!
//! Replays a linear range of commits onto a new base, using merge-ort style
//! tree merging for each replayed commit and printing an `update-ref --stdin`
//! command for the updated branch.

use crate::commands::merge::merge_trees_for_replay;
use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::config::ConfigSet;
use grit_lib::diff::{detect_renames, DiffEntry, DiffStatus};
use grit_lib::index::IndexEntry;
use grit_lib::merge_file::MergeFavor;
use grit_lib::objects::{
    parse_commit, parse_tree, serialize_commit, CommitData, ObjectId, ObjectKind,
};
use grit_lib::refs::resolve_ref;
use grit_lib::repo::Repository;
use grit_lib::rev_parse::resolve_revision;
use grit_lib::write_tree::write_tree_from_index;
use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::io::Write;
use std::path::Path;
use time::OffsetDateTime;

/// Arguments for `grit replay`.
#[derive(Debug, ClapArgs)]
#[command(about = "Replay commits on a new base")]
pub struct Args {
    /// Raw arguments forwarded to the system Git binary.
    #[arg(value_name = "ARG", num_args = 0.., allow_hyphen_values = true, trailing_var_arg = true)]
    pub args: Vec<String>,
}

#[derive(Debug, Default)]
struct ParsedReplayArgs {
    onto: Option<String>,
    range: Option<String>,
}

fn parse_args(raw: &[String]) -> Result<ParsedReplayArgs> {
    let mut parsed = ParsedReplayArgs::default();
    let mut i = 0usize;
    while i < raw.len() {
        let arg = &raw[i];
        if arg == "--onto" {
            let Some(value) = raw.get(i + 1) else {
                bail!("error: option '--onto' requires a value");
            };
            parsed.onto = Some(value.clone());
            i += 2;
            continue;
        }
        if let Some(value) = arg.strip_prefix("--onto=") {
            if value.is_empty() {
                bail!("error: option '--onto' requires a value");
            }
            parsed.onto = Some(value.to_owned());
            i += 1;
            continue;
        }
        if arg.starts_with('-') {
            bail!("unsupported option: {arg}");
        }
        if parsed.range.is_some() {
            bail!("error: multiple revision ranges are not supported");
        }
        parsed.range = Some(arg.clone());
        i += 1;
    }
    Ok(parsed)
}

/// Run `grit replay`.
pub fn run(args: Args) -> Result<()> {
    let parsed = parse_args(&args.args)?;
    let onto_spec = parsed
        .onto
        .ok_or_else(|| anyhow::anyhow!("error: option --onto is mandatory"))?;
    let range = parsed
        .range
        .ok_or_else(|| anyhow::anyhow!("error: replay requires a revision range"))?;
    let (upstream_spec, branch_spec) = range
        .split_once("..")
        .ok_or_else(|| anyhow::anyhow!("error: replay currently requires a <old>..<new> range"))?;
    if upstream_spec.is_empty() || branch_spec.is_empty() {
        bail!("error: invalid replay range '{range}'");
    }

    let repo = Repository::discover(None).context("not a git repository")?;
    let onto_oid = resolve_revision(&repo, &onto_spec)
        .with_context(|| format!("bad revision '{onto_spec}'"))?;
    let upstream_oid = resolve_revision(&repo, upstream_spec)
        .with_context(|| format!("bad revision '{upstream_spec}'"))?;
    let (target_ref, old_tip) = resolve_target_branch(&repo, branch_spec)?;
    let branch_tip = resolve_revision(&repo, branch_spec)
        .with_context(|| format!("bad revision '{branch_spec}'"))?;
    let commits = collect_linear_commits_to_replay(&repo, upstream_oid, branch_tip)?;
    let merge_renormalize = read_merge_renormalize(&repo);
    let directory_renames = read_directory_renames(&repo);
    let mut cached_upstream_renames: HashMap<Vec<u8>, Vec<u8>> = HashMap::new();

    let mut replayed_tip = onto_oid;
    for commit_oid in commits {
        let commit_obj = repo.odb.read(&commit_oid)?;
        let commit = parse_commit(&commit_obj.data)?;
        let parent_oid = *commit.parents.first().ok_or_else(|| {
            anyhow::anyhow!("replaying down from root commit is not supported yet!")
        })?;

        let base_tree = commit_tree(&repo, parent_oid)?;
        let ours_tree = commit_tree(&repo, replayed_tip)?;
        let theirs_tree = commit.tree;

        let base_entries_raw = tree_to_map(tree_to_index_entries(&repo, &base_tree, "")?);
        let mut ours_entries = tree_to_map(tree_to_index_entries(&repo, &ours_tree, "")?);
        let theirs_entries_raw = tree_to_map(tree_to_index_entries(&repo, &theirs_tree, "")?);

        let changed_paths = collect_changed_paths(&base_entries_raw, &theirs_entries_raw);
        let should_refresh_upstream = should_refresh_upstream_rename_cache(
            &base_entries_raw,
            &theirs_entries_raw,
            &cached_upstream_renames,
        );
        if should_refresh_upstream {
            let detected = detect_side_renames(&repo, &base_entries_raw, &ours_entries, true)?;
            cached_upstream_renames = filter_renames_for_changed_paths(detected, &changed_paths);
        }

        let mut topic_renames = HashMap::new();
        if likely_has_rename_candidates(&base_entries_raw, &theirs_entries_raw) {
            topic_renames =
                detect_side_renames(&repo, &base_entries_raw, &theirs_entries_raw, true)?;
            for (old, new) in &topic_renames {
                if cached_upstream_renames.get(old) == Some(new) {
                    cached_upstream_renames.remove(old);
                }
            }
        }

        if directory_renames {
            apply_directory_renames_to_ours_additions(
                &base_entries_raw,
                &mut ours_entries,
                &topic_renames,
                &theirs_entries_raw,
            );
        }

        let base_entries = apply_cached_renames(
            &base_entries_raw,
            &cached_upstream_renames,
            &ours_entries,
            Some(&base_entries_raw),
        );
        let theirs_entries = apply_cached_renames(
            &theirs_entries_raw,
            &cached_upstream_renames,
            &ours_entries,
            Some(&base_entries_raw),
        );

        let merge_result = merge_trees_for_replay(
            &repo,
            &base_entries,
            &ours_entries,
            &theirs_entries,
            &short_oid(commit_oid),
            &short_oid(parent_oid),
            MergeFavor::None,
            None,
            merge_renormalize,
        )?;
        if merge_result.has_conflicts {
            let reason = merge_result
                .conflict_descriptions
                .first()
                .map(|entry| entry.1.as_str())
                .unwrap_or("conflict");
            bail!("replay stopped due to merge conflict in {reason}");
        }

        let merged_tree = write_tree_from_index(&repo.odb, &merge_result.index, "")?;

        // Drop commits that become empty after replay (matching git replay).
        if merged_tree == ours_tree && theirs_tree != base_tree {
            continue;
        }

        replayed_tip = create_replayed_commit(&repo, replayed_tip, merged_tree, &commit)?;
    }

    println!(
        "update refs/heads/{} {} {}",
        target_ref,
        replayed_tip.to_hex(),
        old_tip.to_hex()
    );
    Ok(())
}

fn resolve_target_branch(repo: &Repository, spec: &str) -> Result<(String, ObjectId)> {
    let refname = if spec.starts_with("refs/") {
        spec.to_owned()
    } else {
        format!("refs/heads/{spec}")
    };
    let oid = resolve_ref(&repo.git_dir, &refname)
        .with_context(|| format!("argument to replay range must be a local branch: {spec}"))?;
    Ok((refname.trim_start_matches("refs/heads/").to_owned(), oid))
}

fn collect_linear_commits_to_replay(
    repo: &Repository,
    start_exclusive: ObjectId,
    tip: ObjectId,
) -> Result<Vec<ObjectId>> {
    let mut commits = Vec::new();
    let mut current = tip;
    while current != start_exclusive {
        let obj = repo.odb.read(&current)?;
        let commit = parse_commit(&obj.data)?;
        if commit.parents.is_empty() {
            bail!("replaying down from root commit is not supported yet!");
        }
        if commit.parents.len() > 1 {
            bail!("replaying merge commits is not supported yet!");
        }
        commits.push(current);
        current = commit.parents[0];
    }
    commits.reverse();
    Ok(commits)
}

fn commit_tree(repo: &Repository, commit_oid: ObjectId) -> Result<ObjectId> {
    let obj = repo.odb.read(&commit_oid)?;
    let commit = parse_commit(&obj.data)?;
    Ok(commit.tree)
}

fn tree_to_index_entries(
    repo: &Repository,
    oid: &ObjectId,
    prefix: &str,
) -> Result<Vec<IndexEntry>> {
    let obj = repo.odb.read(oid)?;
    if obj.kind != ObjectKind::Tree {
        bail!("expected tree object");
    }
    let entries = parse_tree(&obj.data)?;
    let mut result = Vec::new();
    for te in entries {
        let name = String::from_utf8_lossy(&te.name).into_owned();
        let path = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        if te.mode == 0o040000 {
            result.extend(tree_to_index_entries(repo, &te.oid, &path)?);
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

fn create_replayed_commit(
    repo: &Repository,
    parent: ObjectId,
    tree: ObjectId,
    based_on: &CommitData,
) -> Result<ObjectId> {
    let config = ConfigSet::load(Some(&repo.git_dir), true)?;
    let now = OffsetDateTime::now_utc();
    let committer = resolve_committer_ident(&config, now);
    let commit = CommitData {
        tree,
        parents: vec![parent],
        author: based_on.author.clone(),
        committer,
        encoding: based_on.encoding.clone(),
        message: based_on.message.clone(),
        raw_message: None,
    };
    let bytes = serialize_commit(&commit);
    repo.odb
        .write(ObjectKind::Commit, &bytes)
        .context("failed to write replayed commit")
}

fn resolve_committer_ident(config: &ConfigSet, now: OffsetDateTime) -> String {
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
    let timestamp = format!("{epoch} {hours:+03}{minutes:02}");
    format!("{name} <{email}> {timestamp}")
}

fn read_merge_renormalize(repo: &Repository) -> bool {
    ConfigSet::load(Some(&repo.git_dir), true)
        .ok()
        .and_then(|c| c.get("merge.renormalize"))
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false)
}

fn read_directory_renames(repo: &Repository) -> bool {
    ConfigSet::load(Some(&repo.git_dir), true)
        .ok()
        .and_then(|c| c.get("merge.directoryRenames"))
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false)
}

fn short_oid(oid: ObjectId) -> String {
    let hex = oid.to_hex();
    hex[..7.min(hex.len())].to_owned()
}

fn likely_has_rename_candidates(
    base: &HashMap<Vec<u8>, IndexEntry>,
    side: &HashMap<Vec<u8>, IndexEntry>,
) -> bool {
    let has_delete = base.keys().any(|path| !side.contains_key(path));
    let has_add = side.keys().any(|path| !base.contains_key(path));
    has_delete && has_add
}

fn collect_changed_paths(
    base: &HashMap<Vec<u8>, IndexEntry>,
    side: &HashMap<Vec<u8>, IndexEntry>,
) -> Vec<Vec<u8>> {
    let mut all = BTreeSet::new();
    all.extend(base.keys().cloned());
    all.extend(side.keys().cloned());

    let mut changed = Vec::new();
    for path in all {
        match (base.get(&path), side.get(&path)) {
            (Some(be), Some(se)) if be.oid == se.oid && be.mode == se.mode => {}
            _ => changed.push(path),
        }
    }
    changed
}

fn filter_renames_for_changed_paths(
    renames: HashMap<Vec<u8>, Vec<u8>>,
    changed_paths: &[Vec<u8>],
) -> HashMap<Vec<u8>, Vec<u8>> {
    if changed_paths.is_empty() {
        return renames;
    }
    let mut kept = HashMap::new();
    let mut changed_dirs: BTreeSet<Vec<u8>> = BTreeSet::new();
    for path in changed_paths {
        let dir = parent_dir(path);
        if !dir.is_empty() {
            changed_dirs.insert(dir);
        }
    }

    for (old, new) in &renames {
        let old_dir = parent_dir(old);
        let new_dir = parent_dir(new);
        let matched = changed_paths.iter().any(|path| {
            path == old
                || path == new
                || path.starts_with(old)
                || path.starts_with(new)
                || (!old_dir.is_empty() && parent_dir(path) == old_dir)
                || (!new_dir.is_empty() && parent_dir(path) == new_dir)
        });
        if matched {
            kept.insert(old.clone(), new.clone());
            if !old_dir.is_empty() {
                changed_dirs.insert(old_dir.clone());
            }
            if !new_dir.is_empty() {
                changed_dirs.insert(new_dir.clone());
            }
        }
    }

    if !changed_dirs.is_empty() {
        for (old, new) in renames {
            if kept.contains_key(&old) {
                continue;
            }
            let old_dir = parent_dir(&old);
            let new_dir = parent_dir(&new);
            if (!old_dir.is_empty() && changed_dirs.contains(&old_dir))
                || (!new_dir.is_empty() && changed_dirs.contains(&new_dir))
            {
                kept.insert(old, new);
            }
        }
    }

    kept
}

fn apply_directory_renames_to_ours_additions(
    base: &HashMap<Vec<u8>, IndexEntry>,
    ours: &mut HashMap<Vec<u8>, IndexEntry>,
    theirs_renames: &HashMap<Vec<u8>, Vec<u8>>,
    theirs: &HashMap<Vec<u8>, IndexEntry>,
) {
    if theirs_renames.is_empty() {
        return;
    }
    let dir_map = build_directory_rename_map(theirs_renames, theirs);
    if dir_map.is_empty() {
        return;
    }

    let keys: Vec<Vec<u8>> = ours.keys().cloned().collect();
    for key in keys {
        if base.contains_key(&key) {
            continue;
        }
        for (old_dir, new_dir) in &dir_map {
            if let Some(new_path) = replace_directory_prefix(&key, old_dir, new_dir) {
                if ours.contains_key(&new_path) {
                    break;
                }
                if let Some(mut entry) = ours.remove(&key) {
                    entry.path = new_path.clone();
                    ours.insert(new_path, entry);
                }
                break;
            }
        }
    }
}

fn parent_dir(path: &[u8]) -> Vec<u8> {
    match path.iter().rposition(|b| *b == b'/') {
        Some(pos) => path[..pos].to_vec(),
        None => Vec::new(),
    }
}

fn apply_cached_renames(
    entries: &HashMap<Vec<u8>, IndexEntry>,
    renames: &HashMap<Vec<u8>, Vec<u8>>,
    side_snapshot: &HashMap<Vec<u8>, IndexEntry>,
    base_snapshot: Option<&HashMap<Vec<u8>, IndexEntry>>,
) -> HashMap<Vec<u8>, IndexEntry> {
    let mut out = entries.clone();
    for (old, new) in renames {
        if let Some(mut entry) = out.remove(old) {
            if out.contains_key(new) {
                out.insert(old.clone(), entry);
                continue;
            }
            entry.path = new.clone();
            out.insert(new.clone(), entry);
        }
    }

    let dir_map = build_directory_rename_map(renames, side_snapshot);
    let exact_sources: BTreeSet<Vec<u8>> = renames.keys().cloned().collect();
    let keys: Vec<Vec<u8>> = out.keys().cloned().collect();
    for key in keys {
        if exact_sources.contains(&key) {
            continue;
        }
        for (old_dir, new_dir) in &dir_map {
            if old_dir.is_empty() || new_dir.is_empty() {
                continue;
            }
            if let Some(base) = base_snapshot {
                if !dir_exists_in_tree(base, old_dir) {
                    continue;
                }
            }
            if let Some(new_path) = replace_directory_prefix(&key, old_dir, new_dir) {
                if out.contains_key(&new_path) {
                    continue;
                }
                if let Some(mut entry) = out.remove(&key) {
                    entry.path = new_path.clone();
                    out.insert(new_path, entry);
                }
                break;
            }
        }
    }

    out
}

fn dir_exists_in_tree(entries: &HashMap<Vec<u8>, IndexEntry>, dir: &[u8]) -> bool {
    entries.keys().any(|path| {
        path.len() > dir.len() && path.starts_with(dir) && path.get(dir.len()) == Some(&b'/')
    })
}

fn replace_directory_prefix(path: &[u8], old_dir: &[u8], new_dir: &[u8]) -> Option<Vec<u8>> {
    if !path.starts_with(old_dir) {
        return None;
    }
    if path.len() == old_dir.len() || path.get(old_dir.len()) != Some(&b'/') {
        return None;
    }
    let mut out = Vec::with_capacity(new_dir.len() + (path.len() - old_dir.len()));
    out.extend_from_slice(new_dir);
    out.extend_from_slice(&path[old_dir.len()..]);
    Some(out)
}

fn build_directory_rename_map(
    renames: &HashMap<Vec<u8>, Vec<u8>>,
    side_snapshot: &HashMap<Vec<u8>, IndexEntry>,
) -> Vec<(Vec<u8>, Vec<u8>)> {
    let mut counts: HashMap<(Vec<u8>, Vec<u8>), usize> = HashMap::new();
    for (old, new) in renames {
        let old_dir = parent_dir(old);
        let new_dir = parent_dir(new);
        if old_dir == new_dir {
            continue;
        }
        *counts.entry((old_dir, new_dir)).or_insert(0) += 1;
    }

    let mut best_for_old: HashMap<Vec<u8>, (Vec<u8>, usize)> = HashMap::new();
    for ((old_dir, new_dir), count) in counts {
        if old_dir.is_empty() || old_dir_still_exists_in_side(&old_dir, side_snapshot) {
            continue;
        }
        match best_for_old.get(&old_dir) {
            Some((_, best)) if *best >= count => {}
            _ => {
                best_for_old.insert(old_dir, (new_dir, count));
            }
        }
    }

    let mut pairs: Vec<(Vec<u8>, Vec<u8>)> = best_for_old
        .into_iter()
        .map(|(old, (new, _))| (old, new))
        .collect();
    pairs.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then_with(|| a.0.cmp(&b.0)));
    pairs
}

fn build_directory_rename_map_unconditional_with_counts(
    renames: &HashMap<Vec<u8>, Vec<u8>>,
) -> Vec<(Vec<u8>, Vec<u8>, usize)> {
    let mut counts: HashMap<(Vec<u8>, Vec<u8>), usize> = HashMap::new();
    for (old, new) in renames {
        let old_dir = parent_dir(old);
        let new_dir = parent_dir(new);
        if old_dir == new_dir {
            continue;
        }
        *counts.entry((old_dir, new_dir)).or_insert(0) += 1;
    }

    let mut best_for_old: HashMap<Vec<u8>, (Vec<u8>, usize)> = HashMap::new();
    for ((old_dir, new_dir), count) in counts {
        if old_dir.is_empty() {
            continue;
        }
        match best_for_old.get(&old_dir) {
            Some((_, best)) if *best >= count => {}
            _ => {
                best_for_old.insert(old_dir, (new_dir, count));
            }
        }
    }

    let mut pairs: Vec<(Vec<u8>, Vec<u8>, usize)> = best_for_old
        .into_iter()
        .map(|(old, (new, count))| (old, new, count))
        .collect();
    pairs.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then_with(|| a.0.cmp(&b.0)));
    pairs
}

fn detect_side_renames(
    repo: &Repository,
    base: &HashMap<Vec<u8>, IndexEntry>,
    side: &HashMap<Vec<u8>, IndexEntry>,
    trace_rename_call: bool,
) -> Result<HashMap<Vec<u8>, Vec<u8>>> {
    let threshold = 50u32;
    let rename_limit: usize = {
        let config = ConfigSet::load(Some(&repo.git_dir), true).ok();
        config
            .as_ref()
            .and_then(|c| c.get("merge.renamelimit"))
            .or_else(|| config.as_ref().and_then(|c| c.get("diff.renamelimit")))
            .and_then(|v| v.parse().ok())
            .unwrap_or(1000)
    };
    let zero_oid = ObjectId::from_bytes(&[0u8; 20]).unwrap();

    let mut side_oid_to_paths: HashMap<ObjectId, Vec<Vec<u8>>> = HashMap::new();
    for (path, entry) in side {
        side_oid_to_paths
            .entry(entry.oid)
            .or_default()
            .push(path.clone());
    }

    let mut exact_renames: HashMap<Vec<u8>, Vec<u8>> = HashMap::new();
    for (base_path, base_entry) in base {
        if let Some(side_entry) = side.get(base_path) {
            if side_entry.oid == base_entry.oid && side_entry.mode == base_entry.mode {
                continue;
            }
        }
        if let Some(side_paths) = side_oid_to_paths.get(&base_entry.oid) {
            for sp in side_paths {
                if sp != base_path && !base.contains_key(sp) {
                    exact_renames.insert(base_path.clone(), sp.clone());
                    break;
                }
            }
        }
    }

    let rename_targets: BTreeSet<Vec<u8>> = exact_renames.values().cloned().collect();
    let rename_sources: BTreeSet<Vec<u8>> = exact_renames.keys().cloned().collect();

    let mut diff_entries = Vec::new();
    let mut all_paths = BTreeSet::new();
    all_paths.extend(base.keys());
    all_paths.extend(side.keys());
    for path in all_paths {
        let path_str = String::from_utf8_lossy(path).to_string();
        match (base.get(path), side.get(path)) {
            (Some(be), None) => {
                if !rename_sources.contains(path) {
                    diff_entries.push(DiffEntry {
                        status: DiffStatus::Deleted,
                        old_path: Some(path_str),
                        new_path: None,
                        old_mode: format!("{:06o}", be.mode),
                        new_mode: String::new(),
                        old_oid: be.oid,
                        new_oid: zero_oid,
                        score: None,
                    });
                }
            }
            (None, Some(se)) => {
                if !rename_targets.contains(path) {
                    diff_entries.push(DiffEntry {
                        status: DiffStatus::Added,
                        old_path: None,
                        new_path: Some(path_str),
                        old_mode: String::new(),
                        new_mode: format!("{:06o}", se.mode),
                        old_oid: zero_oid,
                        new_oid: se.oid,
                        score: None,
                    });
                }
            }
            (Some(be), Some(se)) => {
                if rename_sources.contains(path) && be.oid != se.oid {
                    diff_entries.push(DiffEntry {
                        status: DiffStatus::Deleted,
                        old_path: Some(path_str),
                        new_path: None,
                        old_mode: format!("{:06o}", be.mode),
                        new_mode: String::new(),
                        old_oid: be.oid,
                        new_oid: zero_oid,
                        score: None,
                    });
                }
            }
            _ => {}
        }
    }

    let n_deleted = diff_entries
        .iter()
        .filter(|e| matches!(e.status, DiffStatus::Deleted))
        .count();
    let n_added = diff_entries
        .iter()
        .filter(|e| matches!(e.status, DiffStatus::Added))
        .count();
    if trace_rename_call && likely_has_rename_candidates(base, side) {
        if let Ok(path) = std::env::var("GIT_TRACE2_PERF") {
            if !path.is_empty() {
                let _ = append_trace2_perf_line(&path, "region_enter", "diffcore_rename");
            }
        }
    }

    let mut renames = exact_renames;
    let mut matched_targets: BTreeSet<Vec<u8>> = renames.values().cloned().collect();
    let detected = if n_deleted > rename_limit || n_added > rename_limit {
        Vec::new()
    } else {
        detect_renames(&repo.odb, diff_entries.clone(), threshold)
    };
    for e in detected {
        if matches!(e.status, DiffStatus::Renamed) {
            if let (Some(old), Some(new)) = (e.old_path, e.new_path) {
                let old_bytes = old.as_bytes().to_vec();
                let new_bytes = new.as_bytes().to_vec();
                if !renames.contains_key(&old_bytes) && !matched_targets.contains(&new_bytes) {
                    renames.insert(old_bytes, new_bytes.clone());
                    matched_targets.insert(new_bytes);
                }
            }
        }
    }

    // Fallback: if similarity-based rename detection missed some obvious
    // one-to-one filename moves, match by identical basename.
    let mut deleted_by_name: HashMap<Vec<u8>, Vec<Vec<u8>>> = HashMap::new();
    let mut added_by_name: HashMap<Vec<u8>, Vec<Vec<u8>>> = HashMap::new();
    for entry in &diff_entries {
        match entry.status {
            DiffStatus::Deleted => {
                if let Some(old) = &entry.old_path {
                    let old_bytes = old.as_bytes().to_vec();
                    if renames.contains_key(&old_bytes) {
                        continue;
                    }
                    deleted_by_name
                        .entry(path_basename(&old_bytes))
                        .or_default()
                        .push(old_bytes);
                }
            }
            DiffStatus::Added => {
                if let Some(new) = &entry.new_path {
                    let new_bytes = new.as_bytes().to_vec();
                    if matched_targets.contains(&new_bytes) {
                        continue;
                    }
                    added_by_name
                        .entry(path_basename(&new_bytes))
                        .or_default()
                        .push(new_bytes);
                }
            }
            _ => {}
        }
    }
    for (name, deleted_paths) in deleted_by_name {
        if deleted_paths.len() != 1 {
            continue;
        }
        let Some(added_paths) = added_by_name.get(&name) else {
            continue;
        };
        if added_paths.len() != 1 {
            continue;
        }
        let old_path = deleted_paths[0].clone();
        let new_path = added_paths[0].clone();
        if !renames.contains_key(&old_path) && !matched_targets.contains(&new_path) {
            renames.insert(old_path, new_path.clone());
            matched_targets.insert(new_path);
        }
    }

    Ok(renames)
}

fn path_basename(path: &[u8]) -> Vec<u8> {
    match path.iter().rposition(|b| *b == b'/') {
        Some(pos) => path[pos + 1..].to_vec(),
        None => path.to_vec(),
    }
}

fn should_refresh_upstream_rename_cache(
    base: &HashMap<Vec<u8>, IndexEntry>,
    side: &HashMap<Vec<u8>, IndexEntry>,
    cached_renames: &HashMap<Vec<u8>, Vec<u8>>,
) -> bool {
    if cached_renames.is_empty() {
        return true;
    }

    let changed_paths = collect_changed_paths(base, side);
    let mut covered_dir_prefixes: BTreeSet<Vec<u8>> = BTreeSet::new();
    for path in changed_paths {
        if path_covered_by_cached_renames(&path, cached_renames) {
            covered_dir_prefixes.insert(parent_dir(&path));
            continue;
        }
        let path_parent = parent_dir(&path);
        if !path_parent.is_empty() && covered_dir_prefixes.contains(&path_parent) {
            continue;
        }
        if base.contains_key(&path) || parent_exists_in_base(&path, base) {
            return true;
        }
    }

    false
}

fn path_covered_by_cached_renames(path: &[u8], renames: &HashMap<Vec<u8>, Vec<u8>>) -> bool {
    if renames
        .iter()
        .any(|(old, new)| path == old.as_slice() || path == new.as_slice())
    {
        return true;
    }
    let dir_map = build_directory_rename_map_unconditional_with_counts(renames);
    dir_map.iter().any(|(old_dir, new_dir, support_count)| {
        *support_count >= 2
            && ((!old_dir.is_empty()
                && path.starts_with(old_dir)
                && path.len() > old_dir.len()
                && path.get(old_dir.len()) == Some(&b'/'))
                || (!new_dir.is_empty()
                    && path.starts_with(new_dir)
                    && path.len() > new_dir.len()
                    && path.get(new_dir.len()) == Some(&b'/')))
    })
}

fn parent_exists_in_base(path: &[u8], base: &HashMap<Vec<u8>, IndexEntry>) -> bool {
    let parent = parent_dir(path);
    if parent.is_empty() {
        return false;
    }
    base.keys().any(|candidate| {
        candidate.len() > parent.len()
            && candidate.starts_with(&parent)
            && candidate.get(parent.len()) == Some(&b'/')
    })
}

fn old_dir_still_exists_in_side(
    old_dir: &[u8],
    side_snapshot: &HashMap<Vec<u8>, IndexEntry>,
) -> bool {
    side_snapshot.keys().any(|path| {
        path.len() > old_dir.len()
            && path.starts_with(old_dir)
            && path.get(old_dir.len()) == Some(&b'/')
    })
}

fn append_trace2_perf_line(path: &str, event: &str, data: &str) -> Result<()> {
    let now = {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap_or_default();
        let total_secs = now.as_secs();
        let micros = now.subsec_micros();
        let secs_in_day = total_secs % 86400;
        let hours = secs_in_day / 3600;
        let mins = (secs_in_day % 3600) / 60;
        let secs = secs_in_day % 60;
        format!("{:02}:{:02}:{:02}.{:06}", hours, mins, secs, micros)
    };

    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(Path::new(path))?;
    writeln!(
        file,
        "{} grit:0  | d0 | main                     | {:<12} |     |           |           |              | {}",
        now, event, data
    )?;
    Ok(())
}
