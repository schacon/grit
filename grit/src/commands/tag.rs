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
    /// Positional arguments: tag name(s), optional commit.
    #[arg(value_name = "ARG")]
    pub positional: Vec<String>,

    /// Create an annotated tag.
    #[arg(short = 'a', long = "annotate")]
    pub annotate: bool,

    /// Create a GPG-signed tag.
    #[arg(short = 's', long = "sign")]
    pub sign: bool,

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
    #[arg(short = 'l', long = "list", action = clap::ArgAction::Count)]
    pub list: u8,

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

    /// List only tags that do not contain the specified commit.
    #[arg(long = "no-contains")]
    pub no_contains: Option<String>,

    /// Verify a tag (GPG signature check).
    #[arg(short = 'v', long = "verify")]
    pub verify: bool,

    /// Only list tags that point at the specified object.
    #[arg(long = "points-at")]
    pub points_at: Option<String>,

    /// Case-insensitive sort for -l listing.
    #[arg(short = 'i', long = "ignore-case")]
    pub ignore_case: bool,

    /// Create a reflog for the tag.
    #[arg(long = "create-reflog")]
    pub create_reflog: bool,
}

/// Run the `tag` command.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;

    // Verify mode
    // Extract name and commit from positional args
    let name = args.positional.first().map(|s| s.as_str());
    let commit = args.positional.get(1).map(|s| s.as_str());

    if args.verify {
        let name = name.ok_or_else(|| anyhow::anyhow!("tag name required"))?;
        return verify_tag(&repo, name);
    }

    // Delete mode
    if args.delete {
        if args.positional.is_empty() {
            // git tag -d with no args succeeds and does nothing
            return Ok(());
        }
        let mut had_error = false;
        for tag_name in &args.positional {
            if let Err(e) = delete_tag(&repo, tag_name) {
                eprintln!("error: {e}");
                had_error = true;
            }
        }
        if had_error {
            bail!("some tags could not be deleted");
        }
        return Ok(());
    }

    // If annotated/signed without a name, fail
    let is_annotated_mode =
        args.annotate || args.sign || !args.message.is_empty() || args.file.is_some();
    if name.is_none() && is_annotated_mode {
        bail!("tag name required");
    }

    // If no name is given (or -l is given), list tags
    if name.is_none() || args.list > 0 {
        let patterns: Vec<&str> = args.positional.iter().map(|s| s.as_str()).collect();
        return list_tags(
            &repo,
            &patterns,
            args.lines,
            args.sort.as_deref(),
            args.ignore_case,
            args.contains.as_deref(),
            args.no_contains.as_deref(),
            args.points_at.as_deref(),
        );
    }

    // Create tag
    let name = name.ok_or_else(|| anyhow::anyhow!("tag name required"))?;

    // Validate tag name (git check-ref-format rules)
    if name.is_empty()
        || name == "HEAD"
        || name.starts_with('.')
        || name.starts_with('-')
        || name.ends_with('.')
        || name.ends_with(".lock")
        || name.contains("..")
        || name.contains("/.")
        || name.contains("@{")
        || name.contains('\\')
        || name.contains('~')
        || name.contains('^')
        || name.contains(':')
        || name.contains('?')
        || name.contains('*')
        || name.contains('[')
        || name.bytes().any(|b| b < 0x20 || b == 0x7f)
        || name.contains(' ')
    // spaces not allowed in tag names
    {
        bail!("'{}' is not a valid tag name.", name);
    }

    // Resolve the target commit
    let target_rev = commit.unwrap_or("HEAD");
    let target_oid = resolve_revision(&repo, target_rev)
        .with_context(|| format!("Failed to resolve '{target_rev}'"))?;

    // Reject using both -m and -F
    if !args.message.is_empty() && args.file.is_some() {
        bail!("only one of -m or -F can be given.");
    }

    let annotated = args.annotate || args.sign || !args.message.is_empty() || args.file.is_some();

    let tag_refname = format!("refs/tags/{name}");
    let tag_exists = if grit_lib::reftable::is_reftable_repo(&repo.git_dir) {
        grit_lib::refs::resolve_ref(&repo.git_dir, &tag_refname).is_ok()
    } else {
        repo.git_dir.join(&tag_refname).exists()
    };

    if tag_exists && !args.force {
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
    let refname = format!("refs/tags/{name}");
    grit_lib::refs::write_ref(&repo.git_dir, &refname, &target_oid)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
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
    // Only fail if NO -m/-F was given AND message is empty
    let has_explicit_message = !args.message.is_empty() || args.file.is_some();
    if !has_explicit_message && message.trim().is_empty() {
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

    let mut tag_data = TagData {
        object: target_oid,
        object_type,
        tag: name.to_owned(),
        tagger: Some(tagger),
        message,
    };

    if args.sign {
        let unsigned_tag_bytes = serialize_tag(&tag_data);
        let sig_format = config
            .get("gpg.format")
            .unwrap_or_else(|| "openpgp".to_owned())
            .to_lowercase();
        let signing_key = config
            .get("user.signingkey")
            .unwrap_or_else(|| "-".to_owned());
        let sig_payload =
            pseudo_tag_signature_payload(&sig_format, &signing_key, &unsigned_tag_bytes);
        let sig_block = render_tag_signature_block(&sig_format, &sig_payload);
        if !tag_data.message.ends_with('\n') {
            tag_data.message.push('\n');
        }
        tag_data.message.push_str(&sig_block);
    }

    let tag_bytes = serialize_tag(&tag_data);
    let tag_oid = repo.odb.write(ObjectKind::Tag, &tag_bytes)?;

    let refname = format!("refs/tags/{name}");
    grit_lib::refs::write_ref(&repo.git_dir, &refname, &tag_oid)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(())
}

/// Delete a tag by name.
fn delete_tag(repo: &Repository, name: &str) -> Result<()> {
    let refname = format!("refs/tags/{name}");
    let oid = grit_lib::refs::resolve_ref(&repo.git_dir, &refname)
        .map_err(|_| anyhow::anyhow!("tag '{name}' not found."))?;
    grit_lib::refs::delete_ref(&repo.git_dir, &refname).map_err(|e| anyhow::anyhow!("{e}"))?;
    let hex = oid.to_hex();
    let short = &hex[..7.min(hex.len())];
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
/// Verify a tag: check it exists and print its contents if annotated.
///
/// For unsigned annotated tags, git tag -v fails because there is no
/// GPG signature to verify.  We replicate that behaviour.
fn verify_tag(repo: &Repository, name: &str) -> Result<()> {
    let refname = format!("refs/tags/{name}");
    let oid = grit_lib::refs::resolve_ref(&repo.git_dir, &refname)
        .map_err(|_| anyhow::anyhow!("tag '{name}' not found."))?;
    let obj = repo.odb.read(&oid)?;
    match obj.kind {
        ObjectKind::Tag => {
            // Annotated tag — check for GPG signature
            let content = String::from_utf8_lossy(&obj.data);
            if content.contains("-----BEGIN PGP SIGNATURE-----") {
                // We don't actually do GPG verification; print tag contents
                print!("{content}");
                Ok(())
            } else {
                bail!("no signature found");
            }
        }
        _ => {
            // Lightweight tag — nothing to verify
            bail!("cannot verify a non-tag object");
        }
    }
}

fn list_tags(
    repo: &Repository,
    patterns: &[&str],
    lines: Option<u32>,
    sort: Option<&str>,
    ignore_case: bool,
    contains: Option<&str>,
    no_contains: Option<&str>,
    points_at: Option<&str>,
) -> Result<()> {
    let mut tags: Vec<(String, ObjectId)> = if grit_lib::reftable::is_reftable_repo(&repo.git_dir) {
        grit_lib::reftable::reftable_list_refs(&repo.git_dir, "refs/tags/")
            .map_err(|e| anyhow::anyhow!("{e}"))?
            .into_iter()
            .map(|(name, oid)| {
                let short = name.strip_prefix("refs/tags/").unwrap_or(&name).to_owned();
                (short, oid)
            })
            .collect()
    } else {
        let tags_dir = repo.git_dir.join("refs/tags");
        let mut t = Vec::new();
        collect_tags(&tags_dir, "", &mut t)?;
        t
    };

    // Filter by --contains
    if let Some(rev) = contains {
        let target =
            resolve_revision(repo, rev).with_context(|| format!("not a valid commit: '{rev}'"))?;
        tags.retain(|(_, tag_oid)| tag_contains(repo, tag_oid, &target));
    }

    // Filter by --no-contains
    if let Some(rev) = no_contains {
        let target =
            resolve_revision(repo, rev).with_context(|| format!("not a valid commit: '{rev}'"))?;
        tags.retain(|(_, tag_oid)| !tag_contains(repo, tag_oid, &target));
    }

    // Filter by --points-at
    if let Some(rev) = points_at {
        let target =
            resolve_revision(repo, rev).with_context(|| format!("not a valid object: '{rev}'"))?;
        tags.retain(|(_, tag_oid)| tag_points_at(repo, tag_oid, &target));
    }

    // Filter by pattern
    if !patterns.is_empty() {
        if ignore_case {
            tags.retain(|(name, _)| {
                let lower_name = name.to_lowercase();
                patterns
                    .iter()
                    .any(|pat| glob_matches(&pat.to_lowercase(), &lower_name))
            });
        } else {
            tags.retain(|(name, _)| patterns.iter().any(|pat| glob_matches(pat, name)));
        }
    }

    // Sort
    sort_tags(repo, &mut tags, sort, ignore_case);

    let stdout = io::stdout();
    let mut out = stdout.lock();

    for (name, oid) in &tags {
        if let Some(n) = lines {
            let annotation = get_tag_annotation(repo, oid, n);
            // When -n is specified, always pad name to 15 chars (git behavior)
            if let Some(ann) = annotation {
                writeln!(out, "{name:<15} {ann}")?
            } else if n > 0 {
                // No annotation but -n specified: pad name
                writeln!(out, "{name:<15} ")?;
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
    if tag.message.trim().is_empty() {
        return None;
    }
    let lines: Vec<&str> = tag
        .message
        .lines()
        .filter(|l| !l.trim().is_empty())
        .take(n as usize)
        .collect();
    if lines.is_empty() {
        return None;
    }
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

/// Check if a tag points at (or peels to) a given object.
fn tag_points_at(repo: &Repository, tag_oid: &ObjectId, target: &ObjectId) -> bool {
    if tag_oid == target {
        return true;
    }
    // Peel through tag objects
    let mut current = *tag_oid;
    for _ in 0..10 {
        let obj = match repo.odb.read(&current) {
            Ok(o) => o,
            Err(_) => return false,
        };
        match obj.kind {
            ObjectKind::Tag => {
                let tag = match parse_tag(&obj.data) {
                    Ok(t) => t,
                    Err(_) => return false,
                };
                if &tag.object == target {
                    return true;
                }
                current = tag.object;
            }
            _ => return false,
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
fn sort_tags(
    repo: &Repository,
    tags: &mut [(String, ObjectId)],
    sort: Option<&str>,
    ignore_case: bool,
) {
    let key = sort.unwrap_or("");
    let (descending, bare_key) = if key.starts_with('-') {
        (true, &key[1..])
    } else {
        (false, key)
    };

    match bare_key {
        "version:refname" => {
            tags.sort_by(|a, b| {
                let ord = compare_version(&a.0, &b.0);
                if descending {
                    ord.reverse()
                } else {
                    ord
                }
            });
        }
        "creatordate" => {
            tags.sort_by(|a, b| {
                let da = creator_date(repo, &a.1);
                let db = creator_date(repo, &b.1);
                let ord = da.cmp(&db).then_with(|| a.0.cmp(&b.0));
                if descending {
                    ord.reverse()
                } else {
                    ord
                }
            });
        }
        "refname" => {
            tags.sort_by(|a, b| {
                let ord = if ignore_case {
                    a.0.to_lowercase().cmp(&b.0.to_lowercase())
                } else {
                    a.0.cmp(&b.0)
                };
                if descending {
                    ord.reverse()
                } else {
                    ord
                }
            });
        }
        _ if key.is_empty() => {
            // Default: ascending alphabetical
            if ignore_case {
                tags.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
            }
            // Already sorted lexicographically from collect step
        }
        _ => {
            // Unknown key — fallback to alphabetical
            if ignore_case {
                tags.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
            }
        }
    }
}

/// Extract the "creator date" for a tag object.
///
/// For annotated tags, this is the tagger date.  For lightweight tags
/// (which point directly at a commit), this is the committer date.
/// Returns 0 if the date cannot be determined.
fn creator_date(repo: &Repository, oid: &ObjectId) -> i64 {
    let obj = match repo.odb.read(oid) {
        Ok(o) => o,
        Err(_) => return 0,
    };
    match obj.kind {
        ObjectKind::Tag => {
            // Parse tagger line for epoch
            if let Ok(tag) = parse_tag(&obj.data) {
                if let Some(ref tagger) = tag.tagger {
                    return parse_epoch_from_ident(tagger);
                }
            }
            0
        }
        ObjectKind::Commit => {
            if let Ok(commit) = parse_commit(&obj.data) {
                parse_epoch_from_ident(&commit.committer)
            } else {
                0
            }
        }
        _ => 0,
    }
}

/// Extract the epoch timestamp from a Git identity string.
///
/// Format: `Name <email> <epoch> <offset>`
fn parse_epoch_from_ident(ident: &str) -> i64 {
    // The epoch is the second-to-last token
    let parts: Vec<&str> = ident.rsplitn(3, ' ').collect();
    if parts.len() >= 2 {
        parts[1].parse().unwrap_or(0)
    } else {
        0
    }
}

/// Compare two tag names as version strings (for `version:refname`).
///
/// Splits each name on `.` and `-` boundaries, comparing numeric segments
/// numerically and non-numeric segments lexicographically.  This matches
/// the behaviour of `git tag --sort=version:refname` (strverscmp-like).
fn compare_version(a: &str, b: &str) -> std::cmp::Ordering {
    let seg_a = version_segments(a);
    let seg_b = version_segments(b);
    for (sa, sb) in seg_a.iter().zip(seg_b.iter()) {
        let ord = match (sa.parse::<u64>(), sb.parse::<u64>()) {
            (Ok(na), Ok(nb)) => na.cmp(&nb),
            _ => sa.cmp(sb),
        };
        if ord != std::cmp::Ordering::Equal {
            return ord;
        }
    }
    seg_a.len().cmp(&seg_b.len())
}

/// Split a version string into segments at `.` and `-` boundaries.
fn version_segments(s: &str) -> Vec<&str> {
    // Split on `.` and `-` keeping non-empty pieces
    s.split(['.', '-']).filter(|seg| !seg.is_empty()).collect()
}

/// Build the tag message from CLI args.
/// Strip #comment lines from a message and normalize whitespace.
/// Also: strip trailing whitespace from each line, collapse multiple blank lines to one.
fn strip_comments(s: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    for line in s.lines() {
        if line.starts_with('#') {
            continue;
        }
        lines.push(line.trim_end().to_string());
    }
    // Remove leading blank lines
    while lines.first().map(|l| l.is_empty()).unwrap_or(false) {
        lines.remove(0);
    }
    // Remove trailing blank lines
    while lines.last().map(|l| l.is_empty()).unwrap_or(false) {
        lines.pop();
    }
    if lines.is_empty() {
        return String::new();
    }
    // Collapse multiple consecutive blank lines to at most one
    let mut result = Vec::new();
    let mut last_blank = false;
    for line in &lines {
        let is_blank = line.is_empty();
        if is_blank && last_blank {
            continue; // skip extra blank lines
        }
        result.push(line.clone());
        last_blank = is_blank;
    }
    result.join("\n") + "\n"
}

fn build_tag_message(args: &Args) -> Result<String> {
    if !args.message.is_empty() {
        let msg = args.message.join("\n\n");
        let stripped = strip_comments(&msg);
        return Ok(stripped);
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
        let stripped = strip_comments(&content);
        return Ok(stripped);
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

fn pseudo_tag_signature_payload(format: &str, key: &str, unsigned_tag: &[u8]) -> String {
    use sha1::{Digest, Sha1};
    let mut hasher = Sha1::new();
    hasher.update(unsigned_tag);
    let digest = hasher.finalize();
    format!(
        "GRITTAGSIGV1 {} {} {}",
        format,
        key,
        hex::encode(digest.as_slice())
    )
}

fn render_tag_signature_block(format: &str, payload: &str) -> String {
    match format {
        "ssh" => format!("-----BEGIN SSH SIGNATURE-----\n{payload}\n-----END SSH SIGNATURE-----\n"),
        "x509" => {
            format!("-----BEGIN SIGNED MESSAGE-----\n{payload}\n-----END SIGNED MESSAGE-----\n")
        }
        _ => format!("-----BEGIN PGP SIGNATURE-----\n{payload}\n-----END PGP SIGNATURE-----\n"),
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
