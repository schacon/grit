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

    // Resolve the target commit
    let rev = args.commit.as_deref().unwrap_or("HEAD");
    let resolved_oid = resolve_revision(&repo, rev)
        .with_context(|| format!("Not a valid object name {rev}"))?;

    // Peel to commit (in case user passed a tag name which resolves to a tag object)
    let target_oid = peel_to_commit(&repo, &resolved_oid)
        .ok_or_else(|| anyhow::anyhow!("Not a valid commit: {rev}"))?;

    // Build a map from commit OID → tag name for all qualifying tags.
    let tag_map = build_tag_map(&repo, args.tags, &args.match_pattern)?;

    // Check if the target commit itself is tagged (exact match).
    if let Some(tag_name) = tag_map.get(&target_oid) {
        if args.long {
            let abbrev = abbreviate(&target_oid, args.abbrev);
            println!("{tag_name}-0-g{abbrev}");
        } else {
            println!("{tag_name}");
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
        &tag_map,
        args.candidates,
        args.first_parent,
    )?;

    match candidate {
        Some(c) => {
            let abbrev = abbreviate(&target_oid, args.abbrev);
            println!("{}-{}-g{abbrev}", c.tag_name, c.depth);
        }
        None => {
            if args.always {
                let abbrev = abbreviate(&target_oid, args.abbrev);
                println!("{abbrev}");
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

/// Build a map from commit OID → tag name for all qualifying tags.
///
/// - If `use_all_tags` is false, only annotated tags are included.
/// - Tags are peeled to their underlying commit.
/// - If `patterns` is non-empty, only tags whose short name matches one of the
///   glob patterns are included.
fn build_tag_map(
    repo: &Repository,
    use_all_tags: bool,
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
        let short_name = refname
            .strip_prefix("refs/tags/")
            .unwrap_or(refname)
            .to_string();

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
                        // Only insert if we don't already have a closer (earlier) tag for this commit
                        map.entry(commit_oid)
                            .or_insert_with(|| short_name.clone());
                    }
                }
            }
            ObjectKind::Commit => {
                // Lightweight tag pointing directly at a commit
                if use_all_tags {
                    map.entry(*oid).or_insert_with(|| short_name.clone());
                }
            }
            _ => {}
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
