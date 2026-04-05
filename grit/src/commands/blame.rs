//! `grit blame` — show what revision and author last modified each line of a file.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::objects::{parse_commit, parse_tree, CommitData, ObjectId, ObjectKind};
use grit_lib::odb::Odb;
use grit_lib::repo::Repository;
use grit_lib::rev_parse::resolve_revision;
use grit_lib::state::resolve_head;
use similar::{ChangeTag, TextDiff};
use std::collections::{HashMap, HashSet};
use std::io::{self, Write};

/// Arguments for `grit blame`.
#[derive(Debug, ClapArgs)]
#[command(about = "Show what revision and author last modified each line of a file")]
pub struct Args {
    /// Limit output to the given line range (e.g. -L 10,20).
    #[arg(short = 'L')]
    pub line_range: Option<String>,

    /// Show long (full) commit hashes.
    #[arg(short = 'l')]
    pub long_hash: bool,

    /// Suppress author name and timestamp.
    #[arg(short = 's')]
    pub suppress: bool,

    /// Show author email instead of name.
    #[arg(short = 'e', long = "show-email")]
    pub email: bool,

    /// Porcelain format for machine consumption.
    #[arg(short = 'p', long = "porcelain")]
    pub porcelain: bool,

    /// Like --porcelain but outputs header for every line.
    #[arg(long = "line-porcelain")]
    pub line_porcelain: bool,

    /// Ignore a specific revision when assigning blame.
    #[arg(long = "ignore-rev")]
    pub ignore_rev: Vec<String>,

    /// File listing revisions to ignore (one hex SHA per line).
    #[arg(long = "ignore-revs-file")]
    pub ignore_revs_file: Vec<String>,

    /// Color lines from the same commit in alternating colors.
    #[arg(long = "color-lines")]
    pub color_lines: bool,

    /// Color lines by age of the commit.
    #[arg(long = "color-by-age")]
    pub color_by_age: bool,

    /// Detect lines moved/copied from other files.
    /// Can be given up to 3 times (-C -C -C) for deeper detection.
    #[arg(short = 'C', action = clap::ArgAction::Count)]
    pub copy_detection: u8,

    /// Show the filename in the output.
    #[arg(short = 'f', long = "show-name")]
    pub show_name: bool,

    /// Use N digits to display object names (default 8, min 4).
    #[arg(long = "abbrev")]
    pub abbrev: Option<usize>,

    /// Revision to blame from (and optional file after `--`).
    #[arg()]
    pub args: Vec<String>,
}

/// A single line attribution.
#[derive(Debug, Clone)]
struct BlameLine {
    oid: ObjectId,
    /// 1-based line number in the final file.
    final_lineno: usize,
    /// 1-based line number in the originating commit.
    orig_lineno: usize,
    content: String,
    /// Source filename (differs from target when -C detects a copy).
    source_file: Option<String>,
}

/// Parsed author/committer string.
#[derive(Debug, Clone)]
struct AuthorInfo {
    name: String,
    email: String,
    timestamp: i64,
    tz: String,
}

fn parse_author_field(raw: &str) -> AuthorInfo {
    // "Name <email> timestamp tz"
    let (name, rest) = match raw.find('<') {
        Some(lt) => (raw[..lt].trim().to_string(), &raw[lt..]),
        None => (raw.to_string(), ""),
    };
    let (email, rest) = match rest.find('>') {
        Some(gt) => (rest[1..gt].to_string(), rest[gt + 1..].trim()),
        None => (String::new(), ""),
    };
    let parts: Vec<&str> = rest.split_whitespace().collect();
    let timestamp = parts.first().and_then(|s| s.parse().ok()).unwrap_or(0);
    let tz = parts.get(1).unwrap_or(&"+0000").to_string();
    AuthorInfo {
        name,
        email,
        timestamp,
        tz,
    }
}

fn format_time(timestamp: i64, tz: &str) -> String {
    let tz_sign: i64 = if tz.starts_with('-') { -1 } else { 1 };
    let tz_digits = tz.trim_start_matches(['+', '-']);
    let tz_hours: i64 = tz_digits.get(..2).and_then(|s| s.parse().ok()).unwrap_or(0);
    let tz_mins: i64 = tz_digits
        .get(2..4)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let tz_offset = tz_sign * (tz_hours * 3600 + tz_mins * 60);

    let adjusted = timestamp + tz_offset;
    let days = adjusted.div_euclid(86400);
    let day_secs = adjusted.rem_euclid(86400);
    let hours = day_secs / 3600;
    let mins = (day_secs % 3600) / 60;
    let secs = day_secs % 60;
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02} {hours:02}:{mins:02}:{secs:02} {tz}")
}

/// Howard Hinnant's algorithm: days since epoch → (year, month, day).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

/// Resolve a file path through nested trees to get the blob OID.
fn resolve_path_in_tree(odb: &Odb, tree_oid: &ObjectId, path: &str) -> Result<Option<ObjectId>> {
    let parts: Vec<&str> = path.split('/').collect();
    let mut current = *tree_oid;

    for (i, part) in parts.iter().enumerate() {
        let obj = odb.read(&current)?;
        let entries = parse_tree(&obj.data)?;
        match entries
            .iter()
            .find(|e| String::from_utf8_lossy(&e.name) == *part)
        {
            Some(e) if i == parts.len() - 1 => return Ok(Some(e.oid)),
            Some(e) => current = e.oid,
            None => return Ok(None),
        }
    }
    Ok(None)
}

fn read_blob_string(odb: &Odb, oid: &ObjectId) -> Result<String> {
    let obj = odb.read(oid)?;
    if obj.kind != ObjectKind::Blob {
        bail!("expected blob object");
    }
    Ok(String::from_utf8_lossy(&obj.data).into_owned())
}

/// Split content into lines, handling trailing newline consistently.
fn content_lines(s: &str) -> Vec<&str> {
    let lines: Vec<&str> = s.split('\n').collect();
    if lines.last() == Some(&"") && lines.len() > 1 {
        lines[..lines.len() - 1].to_vec()
    } else {
        lines
    }
}

/// Each line being tracked through history.
/// `final_lineno` is the 1-based line number in the target file.
/// `current_idx` is the 0-based index in the current version being examined.
#[derive(Debug, Clone)]
struct TrackedLine {
    final_lineno: usize,
    current_idx: usize,
}

/// Core blame: walk first-parent history, diff blobs, attribute lines.
fn compute_blame(
    odb: &Odb,
    start_oid: ObjectId,
    file_path: &str,
    ignore_revs: &HashSet<ObjectId>,
) -> Result<Vec<BlameLine>> {
    let start_commit = {
        let obj = odb.read(&start_oid)?;
        parse_commit(&obj.data)?
    };

    let blob_oid = resolve_path_in_tree(odb, &start_commit.tree, file_path)?
        .with_context(|| format!("file '{file_path}' not found in revision"))?;
    let content = read_blob_string(odb, &blob_oid)?;
    let lines = content_lines(&content);
    let num_lines = lines.len();

    if num_lines == 0 {
        return Ok(Vec::new());
    }

    // Lines still needing attribution
    let mut pending: Vec<TrackedLine> = (0..num_lines)
        .map(|i| TrackedLine {
            final_lineno: i + 1,
            current_idx: i,
        })
        .collect();

    let mut result: Vec<BlameLine> = Vec::with_capacity(num_lines);
    // Store final content for output
    let final_lines: Vec<String> = lines.iter().map(|s| s.to_string()).collect();

    let mut current_oid = start_oid;
    let mut current_blob_oid = blob_oid;
    let mut commit_cache: HashMap<ObjectId, CommitData> = HashMap::new();
    commit_cache.insert(start_oid, start_commit);

    loop {
        if pending.is_empty() {
            break;
        }

        let commit = get_commit(odb, current_oid, &mut commit_cache)?;

        let is_ignored = ignore_revs.contains(&current_oid);

        if commit.parents.is_empty() {
            // Root commit — attribute all remaining lines
            for t in pending.drain(..) {
                result.push(BlameLine {
                    oid: current_oid,
                    final_lineno: t.final_lineno,
                    orig_lineno: t.current_idx + 1,
                    content: final_lines[t.final_lineno - 1].clone(),
                    source_file: None,
                });
            }
            break;
        }

        let parent_oid = commit.parents[0];
        let parent_commit = get_commit(odb, parent_oid, &mut commit_cache)?;
        let parent_blob_oid = resolve_path_in_tree(odb, &parent_commit.tree, file_path)?;

        match parent_blob_oid {
            None if !is_ignored => {
                // File doesn't exist in parent — attribute all remaining
                for t in pending.drain(..) {
                    result.push(BlameLine {
                        oid: current_oid,
                        final_lineno: t.final_lineno,
                        orig_lineno: t.current_idx + 1,
                        content: final_lines[t.final_lineno - 1].clone(),
                        source_file: None,
                    });
                }
                break;
            }
            None => {
                // Ignored commit but file doesn't exist in parent.
                // Attribute to current anyway (can't go further back).
                for t in pending.drain(..) {
                    result.push(BlameLine {
                        oid: current_oid,
                        final_lineno: t.final_lineno,
                        orig_lineno: t.current_idx + 1,
                        content: final_lines[t.final_lineno - 1].clone(),
                        source_file: None,
                    });
                }
                break;
            }
            Some(p_blob_oid) if p_blob_oid == current_blob_oid => {
                // Identical blob — skip to parent
                current_oid = parent_oid;
                continue;
            }
            Some(p_blob_oid) => {
                // Diff current vs parent
                let cur_content = read_blob_string(odb, &current_blob_oid)?;
                let par_content = read_blob_string(odb, &p_blob_oid)?;
                let cur_lines = content_lines(&cur_content);
                let par_lines = content_lines(&par_content);

                // Build mapping: cur_line_idx → Option<parent_line_idx>
                let line_map = build_line_map(&par_lines, &cur_lines);

                let mut still_pending = Vec::new();
                for t in pending.drain(..) {
                    if t.current_idx < line_map.len() {
                        if let Some(parent_idx) = line_map[t.current_idx] {
                            // Line came from parent — keep tracking
                            still_pending.push(TrackedLine {
                                final_lineno: t.final_lineno,
                                current_idx: parent_idx,
                            });
                        } else if is_ignored {
                            // Commit is ignored — pass line through to parent
                            // even though it was "introduced" here.
                            // Use a best-effort mapping: keep the same index.
                            let par_idx = t.current_idx.min(par_lines.len().saturating_sub(1));
                            still_pending.push(TrackedLine {
                                final_lineno: t.final_lineno,
                                current_idx: par_idx,
                            });
                        } else {
                            // Line was introduced in current commit
                            result.push(BlameLine {
                                oid: current_oid,
                                final_lineno: t.final_lineno,
                                orig_lineno: t.current_idx + 1,
                                content: final_lines[t.final_lineno - 1].clone(),
                                source_file: None,
                            });
                        }
                    } else if is_ignored {
                        let par_idx = par_lines.len().saturating_sub(1);
                        still_pending.push(TrackedLine {
                            final_lineno: t.final_lineno,
                            current_idx: par_idx,
                        });
                    } else {
                        // Out of range — attribute to current
                        result.push(BlameLine {
                            oid: current_oid,
                            final_lineno: t.final_lineno,
                            orig_lineno: t.current_idx + 1,
                            content: final_lines[t.final_lineno - 1].clone(),
                            source_file: None,
                        });
                    }
                }

                pending = still_pending;
                current_oid = parent_oid;
                current_blob_oid = p_blob_oid;
            }
        }
    }

    result.sort_by_key(|b| b.final_lineno);
    Ok(result)
}

fn get_commit(
    odb: &Odb,
    oid: ObjectId,
    cache: &mut HashMap<ObjectId, CommitData>,
) -> Result<CommitData> {
    if let Some(c) = cache.get(&oid) {
        return Ok(c.clone());
    }
    let obj = odb.read(&oid)?;
    let c = parse_commit(&obj.data)?;
    cache.insert(oid, c.clone());
    Ok(c)
}

/// Map each line in `new` to its origin in `old` (if any).
fn build_line_map(old: &[&str], new: &[&str]) -> Vec<Option<usize>> {
    // Ensure trailing newlines so `from_lines` splits consistently
    let mut old_joined = old.join("\n");
    old_joined.push('\n');
    let mut new_joined = new.join("\n");
    new_joined.push('\n');
    let diff = TextDiff::from_lines(&old_joined, &new_joined);

    let mut result = vec![None; new.len()];
    let mut old_idx: usize = 0;
    let mut new_idx: usize = 0;

    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Equal => {
                if new_idx < result.len() {
                    result[new_idx] = Some(old_idx);
                }
                old_idx += 1;
                new_idx += 1;
            }
            ChangeTag::Delete => {
                old_idx += 1;
            }
            ChangeTag::Insert => {
                new_idx += 1;
            }
        }
    }

    result
}

// ── CLI entry point ──────────────────────────────────────────────────

pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let odb = Odb::new(&repo.git_dir.join("objects"));

    let (rev, file_path) = parse_blame_args(&args.args)?;

    let start_oid = match &rev {
        Some(r) => resolve_revision(&repo, r)?,
        None => {
            let head = resolve_head(&repo.git_dir)?;
            match head.oid() {
                Some(oid) => *oid,
                None => bail!("cannot blame on unborn branch"),
            }
        }
    };

    // Build the set of revisions to ignore
    let mut ignore_revs = HashSet::new();
    for rev_str in &args.ignore_rev {
        let oid = ObjectId::from_hex(rev_str)
            .with_context(|| format!("invalid --ignore-rev: {rev_str}"))?;
        ignore_revs.insert(oid);
    }
    for file in &args.ignore_revs_file {
        let contents = std::fs::read_to_string(file)
            .with_context(|| format!("cannot read --ignore-revs-file: {file}"))?;
        for line in contents.lines() {
            let line = line.trim();
            // Skip blank lines and comments
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let oid = ObjectId::from_hex(line)
                .with_context(|| format!("invalid rev in {file}: {line}"))?;
            ignore_revs.insert(oid);
        }
    }

    let mut blame_lines = match compute_blame(&odb, start_oid, &file_path, &ignore_revs) {
        Ok(lines) => lines,
        Err(e) if rev.is_none() => {
            // When no explicit revision is given and the file is not in HEAD's
            // tree (e.g. during a conflicted merge), fall back to reading the
            // working tree file and attributing all lines to the zero OID.
            // Try working tree first, then fall back to index conflict stages
            let content = if let Some(work_tree) = repo.work_tree.as_deref() {
                let abs_path = work_tree.join(&file_path);
                if abs_path.exists() {
                    std::fs::read_to_string(&abs_path)
                        .with_context(|| format!("file '{file_path}' not found"))?
                } else {
                    // File not in worktree; try reading from highest conflict stage in index
                    read_from_index_conflict(&repo, &odb, &file_path)
                        .with_context(|| format!("file '{file_path}' not found in revision"))?
                }
            } else {
                return Err(e);
            };
            let zero = grit_lib::diff::zero_oid();
            content_lines(&content)
                .iter()
                .enumerate()
                .map(|(i, line)| BlameLine {
                    oid: zero,
                    final_lineno: i + 1,
                    orig_lineno: i + 1,
                    content: line.to_string(),
                    source_file: None,
                })
                .collect()
        }
        Err(e) => return Err(e),
    };

    // Apply line range filter
    if let Some(ref range) = args.line_range {
        let line_contents: Vec<&str> = blame_lines.iter().map(|b| b.content.as_str()).collect();
        let (start, end) = parse_line_range(range, &blame_lines, &line_contents)?;
        blame_lines.retain(|b| b.final_lineno >= start && b.final_lineno <= end);
    }

    let stdout = io::stdout();
    let mut out = stdout.lock();

    // Preload commits for display
    let zero = grit_lib::diff::zero_oid();
    let mut commits: HashMap<ObjectId, CommitData> = HashMap::new();
    for bl in &blame_lines {
        if let std::collections::hash_map::Entry::Vacant(e) = commits.entry(bl.oid) {
            if bl.oid == zero {
                // Fake commit for uncommitted/conflicted content
                e.insert(CommitData {
                    tree: zero,
                    parents: vec![],
                    author: "Not Committed Yet <not.committed.yet> 0 +0000".to_string(),
                    committer: "Not Committed Yet <not.committed.yet> 0 +0000".to_string(),
                    encoding: None,
                    message: String::new(),
                    raw_message: None,
                });
            } else {
                let obj = odb.read(&bl.oid)?;
                e.insert(parse_commit(&obj.data)?);
            }
        }
    }

    if args.porcelain || args.line_porcelain {
        write_porcelain(
            &mut out,
            &blame_lines,
            &commits,
            &file_path,
            args.line_porcelain,
        )?;
    } else {
        write_default(&mut out, &blame_lines, &commits, &args, &file_path)?;
    }

    Ok(())
}

/// Read file content from index conflict stages (for blame during merge conflicts).
fn read_from_index_conflict(repo: &Repository, odb: &Odb, file_path: &str) -> Result<String> {
    use grit_lib::index::Index;
    let index = Index::load(&repo.index_path()).context("loading index")?;
    let path_bytes = file_path.as_bytes();
    // Find the highest-stage entry for this path (prefer stage 3, then 2, then 1)
    let mut best: Option<&grit_lib::index::IndexEntry> = None;
    for entry in &index.entries {
        if entry.path == path_bytes && entry.stage() > 0 {
            if best.is_none() || entry.stage() > best.unwrap().stage() {
                best = Some(entry);
            }
        }
    }
    let entry = best.ok_or_else(|| anyhow::anyhow!("file not in index"))?;
    let obj = odb.read(&entry.oid)?;
    String::from_utf8(obj.data).context("blob is not valid UTF-8")
}

fn parse_blame_args(args: &[String]) -> Result<(Option<String>, String)> {
    match args.len() {
        0 => bail!("usage: grit blame [<rev>] [--] <file>"),
        1 => Ok((None, args[0].clone())),
        2 if args[0] == "--" => Ok((None, args[1].clone())),
        2 => Ok((Some(args[0].clone()), args[1].clone())),
        3 if args[1] == "--" => Ok((Some(args[0].clone()), args[2].clone())),
        _ => bail!("usage: grit blame [<rev>] [--] <file>"),
    }
}

fn parse_line_range(
    range: &str,
    blame_lines: &[BlameLine],
    _line_contents: &[&str],
) -> Result<(usize, usize)> {
    let total = blame_lines.len();
    // Find the max final_lineno for $ handling
    let max_lineno = blame_lines
        .iter()
        .map(|b| b.final_lineno)
        .max()
        .unwrap_or(total);

    let parts: Vec<&str> = range.splitn(2, ',').collect();
    if parts.len() != 2 {
        bail!("invalid line range: expected start,end");
    }

    let start = parse_line_spec(parts[0], blame_lines, None)?;
    let end = parse_line_spec(parts[1], blame_lines, Some(start))?;

    Ok((start, end.min(max_lineno)))
}

/// Parse a single line-range endpoint.
/// `relative_to` is `Some(start)` when parsing the end portion (to support `+N`).
fn parse_line_spec(
    spec: &str,
    blame_lines: &[BlameLine],
    relative_to: Option<usize>,
) -> Result<usize> {
    let max_lineno = blame_lines
        .iter()
        .map(|b| b.final_lineno)
        .max()
        .unwrap_or(0);

    if spec == "$" {
        return Ok(max_lineno);
    }

    // +N means "relative offset from start"
    if let Some(offset_str) = spec.strip_prefix('+') {
        let offset: usize = offset_str.parse().context("invalid +N offset")?;
        let base = relative_to.unwrap_or(1);
        // git semantics: -L N,+M means lines N through N+M-1
        return Ok(base + offset - 1);
    }

    // -N means "relative negative offset from start"
    if let Some(offset_str) = spec.strip_prefix('-') {
        if let Ok(offset) = offset_str.parse::<usize>() {
            let base = relative_to.unwrap_or(1);
            return Ok(base.saturating_sub(offset).max(1));
        }
    }

    // /regex/ — find first line matching the pattern
    if spec.starts_with('/') && spec.ends_with('/') && spec.len() > 2 {
        let pattern = &spec[1..spec.len() - 1];
        let search_start = relative_to.unwrap_or(0);
        for bl in blame_lines {
            if bl.final_lineno > search_start && bl.content.contains(pattern) {
                return Ok(bl.final_lineno);
            }
        }
        // If nothing found searching forward, search from beginning
        if search_start > 0 {
            for bl in blame_lines {
                if bl.content.contains(pattern) {
                    return Ok(bl.final_lineno);
                }
            }
        }
        bail!("no line matching pattern: {pattern}");
    }

    // Plain number
    spec.parse().context("invalid line number")
}

fn write_porcelain(
    out: &mut impl Write,
    lines: &[BlameLine],
    commits: &HashMap<ObjectId, CommitData>,
    filename: &str,
    line_porcelain: bool,
) -> Result<()> {
    let mut seen = std::collections::HashSet::new();

    // Pre-compute group counts: for each position, how many consecutive lines
    // share the same oid starting from the first occurrence in the group.
    let mut group_counts: Vec<Option<usize>> = vec![None; lines.len()];
    let mut i = 0;
    while i < lines.len() {
        let oid = lines[i].oid;
        let start = i;
        while i < lines.len() && lines[i].oid == oid {
            i += 1;
        }
        group_counts[start] = Some(i - start);
    }

    for (idx, bl) in lines.iter().enumerate() {
        let hex = bl.oid.to_hex();
        let first = seen.insert(bl.oid);

        // Header line: hash orig_lineno final_lineno [group_count]
        if let Some(count) = group_counts[idx] {
            writeln!(out, "{hex} {} {} {count}", bl.orig_lineno, bl.final_lineno)?;
        } else {
            writeln!(out, "{hex} {} {}", bl.orig_lineno, bl.final_lineno)?;
        }

        if first || line_porcelain {
            let commit = &commits[&bl.oid];
            let author = parse_author_field(&commit.author);
            let committer = parse_author_field(&commit.committer);

            writeln!(out, "author {}", author.name)?;
            writeln!(out, "author-mail <{}>", author.email)?;
            writeln!(out, "author-time {}", author.timestamp)?;
            writeln!(out, "author-tz {}", author.tz)?;
            writeln!(out, "committer {}", committer.name)?;
            writeln!(out, "committer-mail <{}>", committer.email)?;
            writeln!(out, "committer-time {}", committer.timestamp)?;
            writeln!(out, "committer-tz {}", committer.tz)?;
            // Summary: first non-blank line of the commit message
            let summary = commit
                .message
                .lines()
                .find(|l| !l.trim().is_empty())
                .unwrap_or("");
            writeln!(out, "summary {summary}")?;
            // Previous commit (parent) if not a root commit
            if !commit.parents.is_empty() {
                let parent_hex = commit.parents[0].to_hex();
                writeln!(out, "previous {parent_hex} {filename}")?;
            }
            // Boundary: root commit has no parents
            if commit.parents.is_empty() {
                writeln!(out, "boundary")?;
            }
            writeln!(out, "filename {filename}")?;
        }

        writeln!(out, "\t{}", bl.content)?;
    }

    Ok(())
}

/// ANSI color codes.
const RESET: &str = "\x1b[0m";
const YELLOW: &str = "\x1b[33m";
const BLUE: &str = "\x1b[34m";
const CYAN: &str = "\x1b[36m";
const WHITE: &str = "\x1b[37m";

/// Classify a commit's age for --color-by-age.
fn age_color(timestamp: i64) -> &'static str {
    // Use a rough "now" based on the system clock
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let age_secs = now - timestamp;
    let one_month = 30 * 24 * 3600;
    let one_year = 365 * 24 * 3600;

    if age_secs < one_month {
        CYAN
    } else if age_secs < one_year {
        WHITE
    } else {
        BLUE
    }
}

fn write_default(
    out: &mut impl Write,
    lines: &[BlameLine],
    commits: &HashMap<ObjectId, CommitData>,
    args: &Args,
    file_path: &str,
) -> Result<()> {
    let hash_len = if args.long_hash {
        40
    } else if let Some(n) = args.abbrev {
        n.max(4)
    } else {
        8
    };
    let max_lineno = lines.iter().map(|b| b.final_lineno).max().unwrap_or(1);
    let lineno_width = format!("{max_lineno}").len();
    let use_color = args.color_lines || args.color_by_age;

    let mut prev_oid: Option<ObjectId> = None;

    // Check if any blame line is a boundary (root commit)
    let has_boundary = lines.iter().any(|l| {
        commits
            .get(&l.oid)
            .map(|c| c.parents.is_empty())
            .unwrap_or(false)
    });

    for bl in lines {
        let hex = bl.oid.to_hex();
        let is_boundary = commits
            .get(&bl.oid)
            .map(|c| c.parents.is_empty())
            .unwrap_or(false);
        let short = if has_boundary {
            if is_boundary {
                format!("^{}", &hex[..hash_len.min(hex.len())])
            } else {
                // Extra char width to align with ^ prefix lines
                format!("{}", &hex[..(hash_len + 1).min(hex.len())])
            }
        } else {
            hex[..hash_len.min(hex.len())].to_string()
        };

        // Determine color prefix/suffix
        let (color_start, color_end) = if args.color_by_age {
            let commit = &commits[&bl.oid];
            let ai = parse_author_field(&commit.author);
            (age_color(ai.timestamp), RESET)
        } else if args.color_lines && prev_oid == Some(bl.oid) {
            // Repeated (contiguous) commit — highlight
            (YELLOW, RESET)
        } else if use_color {
            ("", "")
        } else {
            ("", "")
        };

        // Filename field for -f / --show-name
        let fname = if args.show_name {
            let name = bl.source_file.as_deref().unwrap_or(file_path);
            format!("{name} ")
        } else {
            String::new()
        };

        if args.suppress {
            writeln!(
                out,
                "{color_start}{short} {fname}{lineno:>w$}) {content}{color_end}",
                lineno = bl.final_lineno,
                w = lineno_width,
                content = bl.content,
            )?;
        } else {
            let commit = &commits[&bl.oid];
            let ai = parse_author_field(&commit.author);
            let who = if args.email {
                format!("<{}>", ai.email)
            } else {
                ai.name.clone()
            };
            let ts = format_time(ai.timestamp, &ai.tz);

            writeln!(
                out,
                "{color_start}{short} {fname}({who} {ts} {lineno:>w$}) {content}{color_end}",
                lineno = bl.final_lineno,
                w = lineno_width,
                content = bl.content,
            )?;
        }

        prev_oid = Some(bl.oid);
    }

    Ok(())
}
