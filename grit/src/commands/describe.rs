//! `grit describe` — give a human-readable name to a commit based on the nearest tag.
//!
//! Walks backwards from a commit via BFS to find the most recent reachable tag,
//! then outputs `<tag>-<n>-g<abbrev>` where n is the number of commits since
//! that tag and abbrev is the abbreviated commit SHA.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::objects::{parse_commit, parse_tag, ObjectId, ObjectKind};
use grit_lib::refs::list_refs;
use grit_lib::repo::Repository;
use grit_lib::rev_parse::resolve_revision;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fs;
use std::path::Path;
use std::process::Command;

/// Arguments for `grit describe`.
#[derive(Debug, ClapArgs)]
#[command(about = "Give an object a human readable name based on an available ref")]
pub struct Args {
    /// Commit-ish to describe (defaults to HEAD).
    #[arg()]
    pub commit: Option<String>,

    /// Instead of using only annotated tags, use any tag found in `refs/tags/`.
    #[arg(long)]
    pub tags: bool,

    /// If no tag is found, show the abbreviated commit object as fallback.
    #[arg(long)]
    pub always: bool,

    /// Always output the long format (the tag, the number of commits, and the
    /// abbreviated commit name) even when it matches a tag.
    #[arg(long)]
    pub long: bool,

    /// Use <n> digits (or as many as needed) to form the abbreviated object
    /// name. A value of 0 suppresses the long format.
    #[arg(long, default_value = "7")]
    pub abbrev: usize,

    /// Instead of considering only the 10 most recent tags as candidates,
    /// consider this many. Increasing above 10 takes proportionally longer
    /// but may give a more accurate result.
    #[arg(long, default_value = "10")]
    pub candidates: usize,

    /// Only consider tags matching the given glob(7) pattern.
    #[arg(long = "match")]
    pub match_pattern: Vec<String>,

    /// Only output exact matches (a tag directly references the commit).
    #[arg(long)]
    pub exact_match: bool,

    /// Display the first-parent chain only.
    #[arg(long)]
    pub first_parent: bool,

    /// Instead of using only annotated tags, use any ref found in
    /// `refs/heads/` and `refs/remotes/` in addition to `refs/tags/`.
    #[arg(long)]
    pub all: bool,

    /// Instead of finding the tag that is an ancestor, find the tag
    /// that contains the commit (i.e., is a descendant).
    #[arg(long)]
    pub contains: bool,

    /// Describe the working tree.  After the version string, append
    /// the given mark (default: "-dirty") if the working tree has
    /// local modifications.
    #[arg(long, default_missing_value = "-dirty", num_args = 0..=1)]
    pub dirty: Option<String>,

    /// Describe the working tree.  After the version string, append
    /// the given mark (default: "-broken") if the working tree cannot
    /// be described (e.g. HEAD points to a broken commit).
    #[arg(long, default_missing_value = "-broken", num_args = 0..=1)]
    pub broken: Option<String>,
}

/// A candidate tag found during the BFS walk.
#[derive(Debug, Clone)]
struct Candidate {
    /// The short tag name (e.g. `v1.0`).
    tag_name: String,
    /// Number of commits between the tagged commit and the target.
    depth: usize,
}

/// Run the `describe` command.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;

    // --broken: if HEAD cannot be resolved, output the broken suffix and return
    if args.broken.is_some() {
        let rev = args.commit.as_deref().unwrap_or("HEAD");
        let broken_suffix = args.broken.as_deref().unwrap_or("-broken");
        match resolve_revision(&repo, rev) {
            Ok(oid) => {
                if peel_to_commit(&repo, &oid).is_none() {
                    // HEAD is not a valid commit
                    let abbrev = abbreviate(&oid, args.abbrev);
                    println!("{abbrev}{broken_suffix}");
                    return Ok(());
                }
            }
            Err(_) => {
                // Can't even resolve HEAD
                println!("HEAD{broken_suffix}");
                return Ok(());
            }
        }
    }

    // Resolve the target commit
    let rev = args.commit.as_deref().unwrap_or("HEAD");
    let resolved_oid = resolve_revision(&repo, rev)
        .with_context(|| format!("Not a valid object name {rev}"))?;

    // Peel to commit (in case user passed a tag name which resolves to a tag object)
    let target_oid = peel_to_commit(&repo, &resolved_oid)
        .ok_or_else(|| anyhow::anyhow!("Not a valid commit: {rev}"))?;

    // Build a map from commit OID → ref name for all qualifying refs.
    let ref_map = build_ref_map(&repo, args.tags, args.all, &args.match_pattern)?;

    // --contains mode: find the nearest tag that is a descendant of the target
    if args.contains {
        return run_contains(&repo, &target_oid, &ref_map);
    }

    // Determine the dirty suffix (if applicable)
    let dirty_suffix = if args.dirty.is_some() || args.broken.is_some() {
        if is_worktree_dirty(&repo) {
            args.dirty.as_deref().unwrap_or("-dirty").to_string()
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    // Check if the target commit itself is tagged (exact match).
    if let Some(ref_name) = ref_map.get(&target_oid) {
        if args.long {
            let abbrev = abbreviate(&target_oid, args.abbrev);
            println!("{ref_name}-0-g{abbrev}{dirty_suffix}");
        } else {
            println!("{ref_name}{dirty_suffix}");
        }
        return Ok(());
    }

    // If --exact-match, we must have found it above.
    if args.exact_match {
        bail!("no tag exactly matches '{}'", target_oid.to_hex());
    }

    // BFS walk backwards from target to find the nearest tagged ancestor.
    let candidate = bfs_find_tag(
        &repo,
        &target_oid,
        &ref_map,
        args.candidates,
        args.first_parent,
    )?;

    match candidate {
        Some(c) => {
            let abbrev = abbreviate(&target_oid, args.abbrev);
            println!("{}-{}-g{abbrev}{dirty_suffix}", c.tag_name, c.depth);
        }
        None => {
            if args.always {
                let abbrev = abbreviate(&target_oid, args.abbrev);
                println!("{abbrev}{dirty_suffix}");
            } else {
                bail!(
                    "No names found, cannot describe anything.\n\
                     \n\
                     How would you describe a commit without any tags?\n\
                     Use --always to fall back to abbreviated commit."
                );
            }
        }
    }

    Ok(())
}

/// Check if the working tree has uncommitted changes.
/// --contains: find the nearest tag that is a descendant of (contains) the target commit.
/// Walk forward from each tag's commit to check if the target is an ancestor.
fn run_contains(
    repo: &Repository,
    target_oid: &ObjectId,
    ref_map: &HashMap<ObjectId, String>,
) -> Result<()> {
    // For each tag, check if target is reachable from the tag commit.
    // Track the best (shortest path) tag.
    let mut best: Option<(String, usize)> = None;

    for (tag_oid, tag_name) in ref_map {
        if let Some(depth) = ancestor_depth(repo, tag_oid, target_oid) {
            if best.as_ref().map_or(true, |(_, d)| depth < *d) {
                best = Some((tag_name.clone(), depth));
            }
        }
    }

    match best {
        Some((name, depth)) => {
            if depth == 0 {
                println!("{name}");
            } else {
                println!("{name}~{depth}");
            }
            Ok(())
        }
        None => {
            bail!("fatal: cannot describe '{}'", target_oid.to_hex());
        }
    }
}

/// Check if `ancestor` is reachable from `descendant` by walking parents.
/// Returns Some(depth) if reachable, None otherwise.
fn ancestor_depth(
    repo: &Repository,
    descendant: &ObjectId,
    ancestor: &ObjectId,
) -> Option<usize> {
    if descendant == ancestor {
        return Some(0);
    }
    let mut queue: VecDeque<(ObjectId, usize)> = VecDeque::new();
    let mut visited = HashSet::new();
    queue.push_back((*descendant, 0));
    visited.insert(*descendant);

    while let Some((oid, depth)) = queue.pop_front() {
        let obj = repo.odb.read(&oid).ok()?;
        if obj.kind != ObjectKind::Commit {
            continue;
        }
        let commit = parse_commit(&obj.data).ok()?;
        for parent in &commit.parents {
            if parent == ancestor {
                return Some(depth + 1);
            }
            if visited.insert(*parent) {
                queue.push_back((*parent, depth + 1));
            }
        }
    }
    None
}

fn is_worktree_dirty(repo: &Repository) -> bool {
    // Use `git diff-index --quiet HEAD` approach:
    // Check if there are any modified/added/deleted files in the index or working tree.
    // We run diff-index via our own status-like check on the workdir.
    let workdir = match &repo.work_tree {
        Some(d) => d,
        None => return false, // bare repo
    };
    // Simple approach: run `git status --porcelain` via Command
    // to detect dirty state, falling back to checking index.
    let output = Command::new("git")
        .args(["diff-index", "--quiet", "HEAD", "--"])
        .current_dir(workdir)
        .output();
    match output {
        Ok(o) => !o.status.success(), // non-zero exit = dirty
        Err(_) => false,
    }
}

/// Build a map from commit OID → ref display name for all qualifying refs.
///
/// - `use_all_tags`: include lightweight tags (not just annotated).
/// - `use_all_refs`: include refs/heads/ and refs/remotes/ too (--all).
/// - If `patterns` is non-empty, only refs whose short name matches one of the
///   glob patterns are included.
fn build_ref_map(
    repo: &Repository,
    use_all_tags: bool,
    use_all_refs: bool,
    patterns: &[String],
) -> Result<HashMap<ObjectId, String>> {
    let mut map: HashMap<ObjectId, String> = HashMap::new();

    // Collect all refs under refs/tags/ (loose)
    let loose_tags = list_refs(&repo.git_dir, "refs/tags/").unwrap_or_default();

    // Also collect tags from packed-refs
    let packed_tags = read_packed_tags(&repo.git_dir)?;

    // Merge: loose refs take priority
    let mut all_tags: BTreeMap<String, ObjectId> = BTreeMap::new();
    for (refname, oid) in packed_tags {
        all_tags.insert(refname, oid);
    }
    for (refname, oid) in loose_tags {
        all_tags.insert(refname, oid);
    }

    for (refname, oid) in &all_tags {
        // When --all is active, preserve the `tags/` prefix (strip only `refs/`)
        // to match git's behavior. Otherwise, strip `refs/tags/` entirely.
        let short_name = if use_all_refs {
            refname.strip_prefix("refs/").unwrap_or(refname).to_string()
        } else {
            refname.strip_prefix("refs/tags/").unwrap_or(refname).to_string()
        };

        // Filter by glob patterns
        if !patterns.is_empty()
            && !patterns
                .iter()
                .any(|p| crate::commands::tag::glob_matches(p, &short_name))
        {
            continue;
        }

        // Read the object to check if it's an annotated tag or a direct commit ref
        let obj = match repo.odb.read(oid) {
            Ok(o) => o,
            Err(_) => continue,
        };

        match obj.kind {
            ObjectKind::Tag => {
                // Annotated tag — peel to commit
                if let Ok(tag_data) = parse_tag(&obj.data) {
                    if let Some(commit_oid) = peel_to_commit(repo, &tag_data.object) {
                        map.entry(commit_oid)
                            .or_insert_with(|| short_name.clone());
                    }
                }
            }
            ObjectKind::Commit => {
                // Lightweight tag pointing directly at a commit
                if use_all_tags || use_all_refs {
                    map.entry(*oid).or_insert_with(|| short_name.clone());
                }
            }
            _ => {}
        }
    }

    // --all: also include branches and remote tracking refs
    if use_all_refs {
        for prefix in &["refs/heads/", "refs/remotes/"] {
            let refs = list_refs(&repo.git_dir, prefix).unwrap_or_default();
            for (refname, oid) in &refs {
                // Display name for --all is the refname with "refs/" stripped
                let display = refname.strip_prefix("refs/").unwrap_or(refname).to_string();

                if !patterns.is_empty()
                    && !patterns
                        .iter()
                        .any(|p| crate::commands::tag::glob_matches(p, &display))
                {
                    continue;
                }

                // Peel to commit
                if let Some(commit_oid) = peel_to_commit(repo, oid) {
                    // Tags have higher priority — only insert if not already present
                    map.entry(commit_oid)
                        .or_insert_with(|| display.clone());
                }
            }
        }
    }

    Ok(map)
}

/// Read tag refs from packed-refs file.
fn read_packed_tags(git_dir: &Path) -> Result<Vec<(String, ObjectId)>> {
    let packed_path = git_dir.join("packed-refs");
    let content = match fs::read_to_string(&packed_path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };

    let mut results = Vec::new();
    for line in content.lines() {
        if line.starts_with('#') || line.starts_with('^') {
            continue;
        }
        let mut parts = line.splitn(2, ' ');
        let hash = parts.next().unwrap_or("");
        let name = parts.next().unwrap_or("").trim();
        if name.starts_with("refs/tags/") && hash.len() == 40 {
            if let Ok(oid) = hash.parse::<ObjectId>() {
                results.push((name.to_string(), oid));
            }
        }
    }
    Ok(results)
}

/// Peel an object to a commit OID (following tag objects).
fn peel_to_commit(repo: &Repository, oid: &ObjectId) -> Option<ObjectId> {
    let mut current = *oid;
    for _ in 0..20 {
        let obj = repo.odb.read(&current).ok()?;
        match obj.kind {
            ObjectKind::Commit => return Some(current),
            ObjectKind::Tag => {
                let tag = parse_tag(&obj.data).ok()?;
                current = tag.object;
            }
            _ => return None,
        }
    }
    None
}

/// BFS walk backwards from `start` to find the nearest tagged commit.
///
/// Returns `None` if no tagged ancestor is found.
fn bfs_find_tag(
    repo: &Repository,
    start: &ObjectId,
    tag_map: &HashMap<ObjectId, String>,
    max_candidates: usize,
    first_parent: bool,
) -> Result<Option<Candidate>> {
    // BFS with distance tracking
    let mut visited: HashSet<ObjectId> = HashSet::new();
    let mut queue: VecDeque<(ObjectId, usize)> = VecDeque::new();
    let mut candidates: Vec<Candidate> = Vec::new();

    queue.push_back((*start, 0));
    visited.insert(*start);

    while let Some((oid, depth)) = queue.pop_front() {
        // If we already have enough candidates and this depth exceeds the worst,
        // we can stop.
        if candidates.len() >= max_candidates {
            // All candidates at this point are at depth <= this depth,
            // since BFS explores in order. We can stop.
            break;
        }

        // Read the commit
        let obj = match repo.odb.read(&oid) {
            Ok(o) => o,
            Err(_) => continue,
        };

        if obj.kind != ObjectKind::Commit {
            continue;
        }

        let commit = match parse_commit(&obj.data) {
            Ok(c) => c,
            Err(_) => continue,
        };

        // Check parents for tags
        let parents = if first_parent {
            commit.parents.into_iter().take(1).collect::<Vec<_>>()
        } else {
            commit.parents
        };

        for parent_oid in parents {
            if !visited.insert(parent_oid) {
                continue;
            }

            let parent_depth = depth + 1;

            if let Some(tag_name) = tag_map.get(&parent_oid) {
                candidates.push(Candidate {
                    tag_name: tag_name.clone(),
                    depth: parent_depth,
                });
                if candidates.len() >= max_candidates {
                    break;
                }
                // Don't enqueue this commit's parents — we found a tag here
                // but we continue BFS to find potentially closer tags on other branches
                continue;
            }

            queue.push_back((parent_oid, parent_depth));
        }
    }

    // Pick the candidate with the smallest depth (nearest tag)
    candidates.sort_by_key(|c| c.depth);
    Ok(candidates.into_iter().next())
}

/// Abbreviate an OID to `n` hex characters.
fn abbreviate(oid: &ObjectId, n: usize) -> String {
    let hex = oid.to_hex();
    if n == 0 {
        return String::new();
    }
    hex[..n.min(40)].to_string()
}
