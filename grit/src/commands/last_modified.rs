//! `grit last-modified` — show when files were last modified.
//!
//! Walks the commit history and finds the most recent commit that touched
//! each tracked file, printing `<date> <path>` for each file.
//!
//!     grit last-modified [<path>...]

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use grit_lib::objects::parse_commit;
use grit_lib::objects::ObjectId;
use grit_lib::odb::Odb;
use grit_lib::repo::Repository;
use grit_lib::state::resolve_head;
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{self, Write};

/// Arguments for `grit last-modified`.
#[derive(Debug, ClapArgs)]
#[command(about = "Show when files were last modified")]
pub struct Args {
    /// Restrict output to these paths.
    #[arg(value_name = "PATH")]
    pub paths: Vec<String>,
}

/// Run `grit last-modified`.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None)
        .context("not a git repository (or any parent up to mount point)")?;

    let head = resolve_head(&repo.git_dir)?;
    let head_oid = match head.oid() {
        Some(oid) => *oid,
        None => return Ok(()), // Unborn branch
    };

    // Walk commits from HEAD, tracking which files each commit touches
    let mut last_modified: HashMap<String, String> = HashMap::new();
    let mut visited: HashSet<ObjectId> = HashSet::new();
    let mut queue: VecDeque<ObjectId> = VecDeque::new();
    queue.push_back(head_oid);

    let filter_paths: HashSet<String> = args.paths.into_iter().collect();

    while let Some(oid) = queue.pop_front() {
        if !visited.insert(oid) {
            continue;
        }

        let obj = match repo.odb.read(&oid) {
            Ok(d) => d,
            Err(_) => continue,
        };

        let commit = match parse_commit(&obj.data) {
            Ok(c) => c,
            Err(_) => continue,
        };

        // Extract author date
        let author_date = commit.author.split('>')
            .nth(1)
            .map(|s| s.trim())
            .unwrap_or("")
            .to_string();

        // Get this commit's tree and parent trees to diff
        let tree_oid = commit.tree;
        let files = collect_tree_paths(&repo.odb, &tree_oid, "");

        // Get parent tree files
        let parent_files: HashSet<(String, ObjectId)> = if let Some(parent_oid) = commit.parents.first() {
            if let Ok(parent_obj) = repo.odb.read(parent_oid) {
                if let Ok(parent_commit) = parse_commit(&parent_obj.data) {
                    collect_tree_paths(&repo.odb, &parent_commit.tree, "")
                        .into_iter()
                        .collect()
                } else {
                    HashSet::new()
                }
            } else {
                HashSet::new()
            }
        } else {
            // Root commit — all files are new
            HashSet::new()
        };

        // Files changed in this commit
        for (path, blob_oid) in &files {
            if !parent_files.contains(&(path.clone(), *blob_oid)) {
                if !filter_paths.is_empty() && !filter_paths.contains(path) {
                    continue;
                }
                last_modified.entry(path.clone())
                    .or_insert_with(|| author_date.clone());
            }
        }

        // Queue parents
        for parent in &commit.parents {
            queue.push_back(*parent);
        }
    }

    // Sort by path and print
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let mut entries: Vec<_> = last_modified.into_iter().collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    for (path, date) in entries {
        writeln!(out, "{} {}", date, path)?;
    }

    Ok(())
}

/// Recursively collect `(path, blob_oid)` pairs from a tree.
fn collect_tree_paths(odb: &Odb, tree_oid: &ObjectId, prefix: &str) -> Vec<(String, ObjectId)> {
    let mut result = Vec::new();

    let data = match odb.read(tree_oid) {
        Ok(d) => d,
        Err(_) => return result,
    };

    // Parse tree entries: each entry is `<mode> <name>\0<20-byte sha>`
    let mut pos = 0;
    let bytes = &data.data;
    while pos < bytes.len() {
        // Find the space separating mode from name
        let space_pos = match bytes[pos..].iter().position(|&b| b == b' ') {
            Some(p) => pos + p,
            None => break,
        };
        let mode = std::str::from_utf8(&bytes[pos..space_pos]).unwrap_or("");

        // Find the null byte after the name
        let null_pos = match bytes[space_pos + 1..].iter().position(|&b| b == 0) {
            Some(p) => space_pos + 1 + p,
            None => break,
        };
        let name = std::str::from_utf8(&bytes[space_pos + 1..null_pos]).unwrap_or("");

        // Read the 20-byte OID
        if null_pos + 1 + 20 > bytes.len() {
            break;
        }
        let oid = match ObjectId::from_bytes(&bytes[null_pos + 1..null_pos + 21]) {
            Ok(o) => o,
            Err(_) => { pos = null_pos + 21; continue; }
        };

        let full_path = if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{}/{}", prefix, name)
        };

        if mode.starts_with("40") {
            // Directory/subtree — recurse
            result.extend(collect_tree_paths(odb, &oid, &full_path));
        } else {
            result.push((full_path, oid));
        }

        pos = null_pos + 21;
    }

    result
}
