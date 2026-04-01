//! `grit tag` — create, list, delete, and manage tags.
//!
//! Supports lightweight tags (simple refs), annotated tags (tag objects),
//! listing with optional pattern matching, and deletion.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::config::ConfigSet;
use grit_lib::objects::{parse_commit, parse_tag, serialize_tag, ObjectId, ObjectKind, TagData};
use grit_lib::repo::Repository;
use grit_lib::rev_parse::resolve_revision;
use grit_lib::state::resolve_head;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use time::OffsetDateTime;

/// Arguments for `grit tag`.
#[derive(Debug, ClapArgs)]
#[command(about = "Create, list, delete or verify a tag object signed with GPG")]
pub struct Args {
    /// Tag name to create, delete, or list.
    #[arg()]
    pub name: Option<String>,

    /// The object the tag should reference (defaults to HEAD).
    #[arg()]
    pub commit: Option<String>,

    /// Create an annotated tag.
    #[arg(short = 'a', long = "annotate")]
    pub annotate: bool,

    /// Tag message (implies `-a`).
    #[arg(short = 'm', long = "message")]
    pub message: Vec<String>,

    /// Read tag message from file.
    #[arg(short = 'F', long = "file")]
    pub file: Option<String>,

    /// Delete a tag.
    #[arg(short = 'd', long = "delete")]
    pub delete: bool,

    /// List tags matching the given pattern.
    #[arg(short = 'l', long = "list")]
    pub list: bool,

    /// Force creation (overwrite existing tag).
    #[arg(short = 'f', long = "force")]
    pub force: bool,

    /// Show N lines of annotation (default 1 when -n given alone).
    #[arg(short = 'n', default_missing_value = "1", num_args = 0..=1)]
    pub lines: Option<u32>,

    /// Sort by key (e.g. `version:refname`, `creatordate`).
    #[arg(long = "sort")]
    pub sort: Option<String>,

    /// List only tags that contain the specified commit.
    #[arg(long = "contains")]
    pub contains: Option<String>,

    /// Case-insensitive sort for -l listing.
    #[arg(short = 'i', long = "ignore-case")]
    pub ignore_case: bool,
}

/// Run the `tag` command.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;

    // Delete mode
    if args.delete {
        let name = args
            .name
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("tag name required"))?;
        return delete_tag(&repo, name);
    }

    // If no name is given (or -l is given), list tags
    if args.name.is_none() || args.list {
        let pattern = args.name.as_deref();
        return list_tags(
            &repo,
            pattern,
            args.lines,
            args.sort.as_deref(),
            args.ignore_case,
            args.contains.as_deref(),
        );
    }

    // Create tag
    let name = args
        .name
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("tag name required"))?;

    // HEAD is forbidden as a tag name
    if name == "HEAD" {
        bail!("'HEAD' is not a valid tag name.");
    }

    // Resolve the target commit
    let target_rev = args.commit.as_deref().unwrap_or("HEAD");
    let target_oid = resolve_revision(&repo, target_rev)
        .with_context(|| format!("Failed to resolve '{target_rev}'"))?;

    let annotated = args.annotate || !args.message.is_empty() || args.file.is_some();

    let tag_ref = repo.git_dir.join("refs/tags").join(name);

    if tag_ref.exists() && !args.force {
        bail!("tag '{name}' already exists");
    }

    if annotated {
        create_annotated_tag(&repo, name, target_oid, &args)?;
    } else {
        create_lightweight_tag(&repo, name, target_oid, &args)?;
    }

    Ok(())
}

/// Create a lightweight (direct ref) tag.
fn create_lightweight_tag(
    repo: &Repository,
    name: &str,
    target_oid: ObjectId,
    _args: &Args,
) -> Result<()> {
    let tag_ref = repo.git_dir.join("refs/tags").join(name);
    if let Some(parent) = tag_ref.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&tag_ref, format!("{}\n", target_oid.to_hex()))?;
    Ok(())
}

/// Create an annotated tag object and write its ref.
fn create_annotated_tag(
    repo: &Repository,
    name: &str,
    target_oid: ObjectId,
    args: &Args,
) -> Result<()> {
    // Build the message
    let message = build_tag_message(args)?;
    if message.trim().is_empty() {
        bail!("no tag message provided (use -m or -F)");
    }

    // Determine the type of the target object
    let obj = repo
        .odb
        .read(&target_oid)
        .with_context(|| format!("object {} not found", target_oid.to_hex()))?;
    let object_type = obj.kind.as_str().to_owned();

    // Build tagger identity
    let config = ConfigSet::load(Some(&repo.git_dir), true)?;
    let now = OffsetDateTime::now_utc();
    let tagger = resolve_tagger(&config, now)?;

    let tag_data = TagData {
        object: target_oid,
        object_type,
        tag: name.to_owned(),
        tagger: Some(tagger),
        message,
    };

    let tag_bytes = serialize_tag(&tag_data);
    let tag_oid = repo.odb.write(ObjectKind::Tag, &tag_bytes)?;

    let tag_ref = repo.git_dir.join("refs/tags").join(name);
    if let Some(parent) = tag_ref.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&tag_ref, format!("{}\n", tag_oid.to_hex()))?;
    Ok(())
}

/// Delete a tag by name.
fn delete_tag(repo: &Repository, name: &str) -> Result<()> {
    let tag_ref = repo.git_dir.join("refs/tags").join(name);
    if !tag_ref.exists() {
        bail!("tag '{name}' not found.");
    }
    let oid_str = fs::read_to_string(&tag_ref)?.trim().to_owned();
    fs::remove_file(&tag_ref)?;
    let short = &oid_str[..7.min(oid_str.len())];
    eprintln!("Deleted tag '{name}' (was {short})");
    Ok(())
}

/// List tags, optionally filtered by a glob pattern.
///
/// - `pattern` — shell glob pattern; `None` means list all.
/// - `lines` — number of annotation lines to show with each tag.
/// - `sort` — sort key.
/// - `ignore_case` — sort case-insensitively.
/// - `contains` — only list tags that contain this commit.
fn list_tags(
    repo: &Repository,
    pattern: Option<&str>,
    lines: Option<u32>,
    sort: Option<&str>,
    ignore_case: bool,
    contains: Option<&str>,
) -> Result<()> {
    let tags_dir = repo.git_dir.join("refs/tags");
    let mut tags: Vec<(String, ObjectId)> = Vec::new();
    collect_tags(&tags_dir, "", &mut tags)?;

    // Filter by --contains
    if let Some(rev) = contains {
        let target =
            resolve_revision(repo, rev).with_context(|| format!("not a valid commit: '{rev}'"))?;
        tags.retain(|(_, tag_oid)| tag_contains(repo, tag_oid, &target));
    }

    // Filter by pattern
    if let Some(pat) = pattern {
        tags.retain(|(name, _)| glob_matches(pat, name));
    }

    // Sort
    sort_tags(&mut tags, sort, ignore_case);

    let stdout = io::stdout();
    let mut out = stdout.lock();

    for (name, oid) in &tags {
        if let Some(n) = lines {
            let annotation = get_tag_annotation(repo, oid, n);
            if let Some(ann) = annotation {
                writeln!(out, "{name:<15} {ann}")?;
            } else {
                writeln!(out, "{name}")?;
            }
        } else {
            writeln!(out, "{name}")?;
        }
    }

    Ok(())
}

/// Collect all tag refs recursively from the tags directory.
fn collect_tags(dir: &Path, prefix: &str, out: &mut Vec<(String, ObjectId)>) -> Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };

    let mut sorted: Vec<_> = entries.filter_map(|e| e.ok()).collect();
    sorted.sort_by_key(|e| e.file_name());

    for entry in sorted {
        let path = entry.path();
        let file_name = entry.file_name().to_string_lossy().to_string();
        let full_name = if prefix.is_empty() {
            file_name
        } else {
            format!("{prefix}/{file_name}")
        };

        if path.is_dir() {
            collect_tags(&path, &full_name, out)?;
        } else if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(oid) = ObjectId::from_hex(content.trim()) {
                out.push((full_name, oid));
            }
        }
    }

    Ok(())
}

/// Get annotation text for a tag (up to `n` lines).
///
/// Returns `None` if the tag has no annotation (lightweight) or n==0.
fn get_tag_annotation(repo: &Repository, oid: &ObjectId, n: u32) -> Option<String> {
    if n == 0 {
        return None;
    }
    let obj = repo.odb.read(oid).ok()?;
    let tag = parse_tag(&obj.data).ok()?;
    if tag.message.is_empty() {
        return None;
    }
    let lines: Vec<&str> = tag.message.lines().take(n as usize).collect();
    Some(lines.join(" "))
}

/// Check if a tag contains (has reachable ancestry from) a commit.
///
/// Peels the tag ref to a commit, then walks ancestors.
fn tag_contains(repo: &Repository, tag_oid: &ObjectId, target: &ObjectId) -> bool {
    // Peel to commit
    let commit_oid = match peel_to_commit(repo, tag_oid) {
        Some(oid) => oid,
        None => return false,
    };

    if &commit_oid == target {
        return true;
    }

    // BFS/DFS walk
    let mut visited = std::collections::HashSet::new();
    let mut queue = std::collections::VecDeque::new();
    queue.push_back(commit_oid);

    while let Some(oid) = queue.pop_front() {
        if !visited.insert(oid) {
            continue;
        }
        if &oid == target {
            return true;
        }
        if let Ok(obj) = repo.odb.read(&oid) {
            if obj.kind == ObjectKind::Commit {
                if let Ok(commit) = parse_commit(&obj.data) {
                    for parent in commit.parents {
                        if !visited.contains(&parent) {
                            queue.push_back(parent);
                        }
                    }
                }
            }
        }
    }

    false
}

/// Peel an object to a commit OID (following tags).
fn peel_to_commit(repo: &Repository, oid: &ObjectId) -> Option<ObjectId> {
    let mut current = *oid;
    for _ in 0..10 {
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

/// Sort tags by the requested key.
fn sort_tags(tags: &mut [(String, ObjectId)], sort: Option<&str>, ignore_case: bool) {
    match sort {
        Some("version:refname") | Some("-version:refname") => {
            let descending = sort.is_some_and(|s| s.starts_with('-'));
            tags.sort_by(|a, b| {
                let ord = compare_version(&a.0, &b.0);
                if descending {
                    ord.reverse()
                } else {
                    ord
                }
            });
        }
        Some(key) if key.starts_with('-') => {
            // Descending alphabetical
            if ignore_case {
                tags.sort_by(|a, b| b.0.to_lowercase().cmp(&a.0.to_lowercase()));
            } else {
                tags.sort_by(|a, b| b.0.cmp(&a.0));
            }
        }
        _ => {
            // Default: ascending alphabetical (already sorted from filesystem)
            if ignore_case {
                tags.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
            }
            // Already sorted lexicographically from collect step
        }
    }
}

/// Compare two tag names as version strings (for `version:refname`).
fn compare_version(a: &str, b: &str) -> std::cmp::Ordering {
    // Simple lexicographic comparison as fallback; version-aware sort
    // is complex; this covers most practical cases
    a.cmp(b)
}

/// Build the tag message from CLI args.
fn build_tag_message(args: &Args) -> Result<String> {
    if !args.message.is_empty() {
        let msg = args.message.join("\n\n");
        return Ok(ensure_trailing_newline(&msg));
    }

    if let Some(ref file_path) = args.file {
        let content = if file_path == "-" {
            use std::io::Read;
            let mut buf = String::new();
            std::io::stdin().read_to_string(&mut buf)?;
            buf
        } else {
            fs::read_to_string(file_path)?
        };
        return Ok(ensure_trailing_newline(&content));
    }

    Ok(String::new())
}

/// Resolve the tagger identity from env and config.
fn resolve_tagger(config: &ConfigSet, now: OffsetDateTime) -> Result<String> {
    let name = std::env::var("GIT_COMMITTER_NAME")
        .ok()
        .or_else(|| config.get("user.name"))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Tagger identity unknown\n\nPlease tell me who you are.\n\n\
                 Run\n\n  git config user.email \"you@example.com\"\n  git config user.name \"Your Name\""
            )
        })?;

    let email = std::env::var("GIT_COMMITTER_EMAIL")
        .ok()
        .or_else(|| config.get("user.email"))
        .unwrap_or_default();

    let date_str = std::env::var("GIT_COMMITTER_DATE").ok();
    let timestamp = match date_str {
        Some(d) => d,
        None => format_git_timestamp(now),
    };

    Ok(format!("{name} <{email}> {timestamp}"))
}

/// Format a timestamp in Git's format: `<epoch> <offset>`.
fn format_git_timestamp(dt: OffsetDateTime) -> String {
    let epoch = dt.unix_timestamp();
    let offset = dt.offset();
    let hours = offset.whole_hours();
    let minutes = offset.minutes_past_hour().unsigned_abs();
    format!("{epoch} {hours:+03}{minutes:02}")
}

/// Ensure a string ends with exactly one newline.
fn ensure_trailing_newline(s: &str) -> String {
    if s.ends_with('\n') {
        s.to_owned()
    } else {
        format!("{s}\n")
    }
}

/// Simple glob pattern matching for tag names.
///
/// Supports `*` (matches any sequence) and `?` (matches any single character).
pub fn glob_matches(pattern: &str, name: &str) -> bool {
    glob_match_bytes(pattern.as_bytes(), name.as_bytes())
}

/// Recursive glob matcher.
fn glob_match_bytes(pat: &[u8], text: &[u8]) -> bool {
    match (pat.first(), text.first()) {
        (None, None) => true,
        (Some(&b'*'), _) => {
            // Skip consecutive stars
            let pat_rest = pat
                .iter()
                .position(|&b| b != b'*')
                .map_or(&pat[pat.len()..], |i| &pat[i..]);
            if pat_rest.is_empty() {
                return true;
            }
            for i in 0..=text.len() {
                if glob_match_bytes(pat_rest, &text[i..]) {
                    return true;
                }
            }
            false
        }
        (Some(&b'?'), Some(_)) => glob_match_bytes(&pat[1..], &text[1..]),
        (Some(p), Some(t)) if p == t => glob_match_bytes(&pat[1..], &text[1..]),
        _ => false,
    }
}

/// Resolve HEAD to the current commit OID, if any.
///
/// Used internally to ensure HEAD is valid when creating a tag.
#[allow(dead_code)]
fn resolve_head_oid(git_dir: &Path) -> Result<ObjectId> {
    let head = resolve_head(git_dir)?;
    head.oid()
        .copied()
        .ok_or_else(|| anyhow::anyhow!("not a valid object name: 'HEAD'"))
}
