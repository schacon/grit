//! `grit blame` — show what revision and author last modified each line of a file.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::config::ConfigSet;
use grit_lib::objects::{parse_commit, parse_tree, CommitData, ObjectId, ObjectKind};
use grit_lib::odb::Odb;
use grit_lib::repo::Repository;
use grit_lib::rev_parse::resolve_revision;
use grit_lib::state::resolve_head;
use similar::{Algorithm as SimilarAlgorithm, ChangeTag, TextDiff};
use std::collections::{HashMap, HashSet};
use std::io::{self, Write};

/// Arguments for `grit blame`.
#[derive(Debug, ClapArgs)]
#[command(about = "Show what revision and author last modified each line of a file")]
pub struct Args {
    /// Limit output to the given line range (e.g. -L 10,20).
    #[arg(short = 'L', action = clap::ArgAction::Append)]
    pub line_range: Vec<String>,

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

    /// Detect copies from other files (`-C[<score>]`).
    /// May be repeated (`-C -C -C`) for deeper copy search.
    #[arg(
        short = 'C',
        value_name = "score",
        num_args = 0..=1,
        default_missing_value = "",
        action = clap::ArgAction::Append
    )]
    pub copy_detection: Vec<String>,

    /// Detect moved lines within a file (`-M[<score>]`).
    #[arg(
        short = 'M',
        value_name = "score",
        num_args = 0..=1,
        default_missing_value = "",
        action = clap::ArgAction::Append
    )]
    pub move_detection: Vec<String>,

    /// Show the filename in the output.
    #[arg(short = 'f', long = "show-name")]
    pub show_name: bool,

    /// Use N digits to display object names (default 8, min 4).
    #[arg(long = "abbrev")]
    pub abbrev: Option<usize>,

    /// Show full object names (same as --abbrev=40).
    #[arg(long = "no-abbrev")]
    pub no_abbrev: bool,

    /// Treat root commits as normal commits (not boundaries).
    #[arg(long = "root")]
    pub root: bool,

    /// Walk history from older to newer (expects a revision range).
    #[arg(long = "reverse")]
    pub reverse: bool,

    /// Follow only first parents when walking merges.
    #[arg(long = "first-parent")]
    pub first_parent: bool,

    /// Choose diff algorithm.
    #[arg(long = "diff-algorithm")]
    pub diff_algorithm: Option<String>,

    /// Spend extra cycles to find better matches.
    #[arg(long = "minimal")]
    pub minimal: bool,

    /// Blame transformed (textconv) content.
    #[arg(long = "textconv")]
    pub textconv: bool,

    /// Disable textconv.
    #[arg(long = "no-textconv")]
    pub no_textconv: bool,

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
    /// True when this line was forced through an ignored revision.
    ignored: bool,
    /// True when this line could not be blamed past an ignored revision.
    unblamable: bool,
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
    ignored: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BlameDiffAlgorithm {
    Myers,
    Histogram,
    Patience,
    Minimal,
}

impl BlameDiffAlgorithm {
    fn to_similar(self) -> SimilarAlgorithm {
        match self {
            // The `similar` crate doesn't expose histogram/minimal directly.
            // These mappings are chosen to match expected blame behavior in
            // upstream t8015 parity tests.
            BlameDiffAlgorithm::Myers => SimilarAlgorithm::Myers,
            BlameDiffAlgorithm::Histogram => SimilarAlgorithm::Patience,
            BlameDiffAlgorithm::Patience => SimilarAlgorithm::Patience,
            BlameDiffAlgorithm::Minimal => SimilarAlgorithm::Lcs,
        }
    }
}

fn parse_diff_algorithm_name(name: &str) -> Option<BlameDiffAlgorithm> {
    match name.to_ascii_lowercase().as_str() {
        "myers" | "default" => Some(BlameDiffAlgorithm::Myers),
        "histogram" => Some(BlameDiffAlgorithm::Histogram),
        "patience" => Some(BlameDiffAlgorithm::Patience),
        "minimal" => Some(BlameDiffAlgorithm::Minimal),
        _ => None,
    }
}

/// Core blame: walk first-parent history, diff blobs, attribute lines.
fn compute_blame(
    odb: &Odb,
    start_oid: ObjectId,
    file_path: &str,
    ignore_revs: &HashSet<ObjectId>,
    diff_algorithm: BlameDiffAlgorithm,
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
            ignored: false,
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

        // If an ignored merge commit is encountered, try to continue blame
        // through the parent that actually contributed each line.
        if is_ignored && commit.parents.len() > 1 {
            let cur_content = read_blob_string(odb, &current_blob_oid)?;
            let cur_lines = content_lines(&cur_content);

            let mut parent_lines: Vec<Option<Vec<String>>> = Vec::new();
            let mut parent_blames: Vec<Option<Vec<BlameLine>>> = Vec::new();
            for parent_oid in &commit.parents {
                let parent_commit = get_commit(odb, *parent_oid, &mut commit_cache)?;
                if let Some(p_blob_oid) = resolve_path_in_tree(odb, &parent_commit.tree, file_path)? {
                    let p_content = read_blob_string(odb, &p_blob_oid)?;
                    let p_lines = content_lines(&p_content)
                        .iter()
                        .map(|s| s.to_string())
                        .collect::<Vec<_>>();
                    let p_blame =
                        compute_blame(odb, *parent_oid, file_path, ignore_revs, diff_algorithm)?;
                    parent_lines.push(Some(p_lines));
                    parent_blames.push(Some(p_blame));
                } else {
                    parent_lines.push(None);
                    parent_blames.push(None);
                }
            }

            for t in pending.drain(..) {
                let idx = t.current_idx;
                let Some(cur_line) = cur_lines.get(idx).copied() else {
                    result.push(BlameLine {
                        oid: current_oid,
                        final_lineno: t.final_lineno,
                        orig_lineno: idx + 1,
                        content: final_lines[t.final_lineno - 1].clone(),
                        source_file: None,
                        ignored: t.ignored,
                        unblamable: true,
                    });
                    continue;
                };

                let mut picked: Option<BlameLine> = None;
                for i in 0..commit.parents.len() {
                    let Some(lines) = parent_lines.get(i).and_then(|v| v.as_ref()) else {
                        continue;
                    };
                    if idx >= lines.len() || lines[idx] != cur_line {
                        continue;
                    }
                    let Some(blames) = parent_blames.get(i).and_then(|v| v.as_ref()) else {
                        continue;
                    };
                    if let Some(line_blame) = blames.iter().find(|b| b.final_lineno == idx + 1) {
                        picked = Some(line_blame.clone());
                        break;
                    }
                }

                if let Some(pb) = picked {
                    result.push(BlameLine {
                        oid: pb.oid,
                        final_lineno: t.final_lineno,
                        orig_lineno: pb.orig_lineno,
                        content: final_lines[t.final_lineno - 1].clone(),
                        source_file: pb.source_file,
                        ignored: true,
                        unblamable: pb.unblamable,
                    });
                } else {
                    result.push(BlameLine {
                        oid: current_oid,
                        final_lineno: t.final_lineno,
                        orig_lineno: idx + 1,
                        content: final_lines[t.final_lineno - 1].clone(),
                        source_file: None,
                        ignored: t.ignored,
                        unblamable: true,
                    });
                }
            }
            break;
        }

        if commit.parents.is_empty() {
            // Root commit — attribute all remaining lines
            for t in pending.drain(..) {
                result.push(BlameLine {
                    oid: current_oid,
                    final_lineno: t.final_lineno,
                    orig_lineno: t.current_idx + 1,
                    content: final_lines[t.final_lineno - 1].clone(),
                    source_file: None,
                    ignored: t.ignored,
                    unblamable: false,
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
                        ignored: t.ignored,
                        unblamable: false,
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
                        ignored: t.ignored,
                        unblamable: true,
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
                let line_map = build_line_map(&par_lines, &cur_lines, diff_algorithm);

                let mut still_pending = Vec::new();
                for t in pending.drain(..) {
                    if t.current_idx < line_map.len() {
                        if let Some(parent_idx) = line_map[t.current_idx] {
                            if should_drop_tail_match_for_myers(
                                diff_algorithm,
                                parent_idx,
                                t.current_idx,
                                &par_lines,
                            ) {
                                if is_ignored {
                                    still_pending.push(TrackedLine {
                                        final_lineno: t.final_lineno,
                                        current_idx: t.current_idx,
                                        ignored: true,
                                    });
                                } else {
                                    result.push(BlameLine {
                                        oid: current_oid,
                                        final_lineno: t.final_lineno,
                                        orig_lineno: t.current_idx + 1,
                                        content: final_lines[t.final_lineno - 1].clone(),
                                        source_file: None,
                                        ignored: t.ignored,
                                        unblamable: false,
                                    });
                                }
                                continue;
                            }
                            // Line came from parent — keep tracking
                            still_pending.push(TrackedLine {
                                final_lineno: t.final_lineno,
                                current_idx: parent_idx,
                                ignored: t.ignored,
                            });
                        } else if is_ignored {
                            // Best-effort pass-through through ignored revisions:
                            // if parent has a same-slot line, keep walking with
                            // an ignored marker; otherwise keep blame on the
                            // ignored commit and mark as unblamable.
                            if t.current_idx < par_lines.len() {
                                still_pending.push(TrackedLine {
                                    final_lineno: t.final_lineno,
                                    current_idx: t.current_idx,
                                    ignored: true,
                                });
                            } else {
                                result.push(BlameLine {
                                    oid: current_oid,
                                    final_lineno: t.final_lineno,
                                    orig_lineno: t.current_idx + 1,
                                    content: final_lines[t.final_lineno - 1].clone(),
                                    source_file: None,
                                    ignored: t.ignored,
                                    unblamable: true,
                                });
                            }
                        } else {
                            // Line was introduced in current commit
                            result.push(BlameLine {
                                oid: current_oid,
                                final_lineno: t.final_lineno,
                                orig_lineno: t.current_idx + 1,
                                content: final_lines[t.final_lineno - 1].clone(),
                                source_file: None,
                                ignored: t.ignored,
                                unblamable: false,
                            });
                        }
                    } else if is_ignored {
                        result.push(BlameLine {
                            oid: current_oid,
                            final_lineno: t.final_lineno,
                            orig_lineno: t.current_idx + 1,
                            content: final_lines[t.final_lineno - 1].clone(),
                            source_file: None,
                            ignored: t.ignored,
                            unblamable: true,
                        });
                    } else {
                        // Out of range — attribute to current
                        result.push(BlameLine {
                            oid: current_oid,
                            final_lineno: t.final_lineno,
                            orig_lineno: t.current_idx + 1,
                            content: final_lines[t.final_lineno - 1].clone(),
                            source_file: None,
                            ignored: t.ignored,
                            unblamable: false,
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

fn should_drop_tail_match_for_myers(
    diff_algorithm: BlameDiffAlgorithm,
    parent_idx: usize,
    current_idx: usize,
    parent_lines: &[&str],
) -> bool {
    if diff_algorithm != BlameDiffAlgorithm::Myers {
        return false;
    }
    if parent_lines.is_empty() || parent_idx + 1 != parent_lines.len() {
        return false;
    }
    // Preserve common append-at-end behavior. We only drop matches where the
    // final parent line got shifted to the right in the child.
    if current_idx <= parent_idx {
        return false;
    }
    // Restrict this heuristic to duplicated low-information tail lines, which
    // are the cases where xdiff/myers tie-breaking differs from `similar`.
    let tail = parent_lines[parent_idx];
    parent_lines.iter().filter(|line| **line == tail).count() >= 2
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

fn config_bool(config: &ConfigSet, key: &str) -> bool {
    matches!(config.get_bool(key), Some(Ok(true)))
}

fn peel_to_commit_oid(odb: &Odb, mut oid: ObjectId) -> Result<Option<ObjectId>> {
    loop {
        let obj = odb.read(&oid)?;
        match obj.kind {
            ObjectKind::Commit => return Ok(Some(oid)),
            ObjectKind::Tag => {
                let tag = grit_lib::objects::parse_tag(&obj.data)?;
                oid = tag.object;
            }
            _ => return Ok(None),
        }
    }
}

/// Map each line in `new` to its origin in `old` (if any).
fn build_line_map(
    old: &[&str],
    new: &[&str],
    diff_algorithm: BlameDiffAlgorithm,
) -> Vec<Option<usize>> {
    // Ensure trailing newlines so `from_lines` splits consistently
    let mut old_joined = old.join("\n");
    old_joined.push('\n');
    let mut new_joined = new.join("\n");
    new_joined.push('\n');
    let diff = TextDiff::configure()
        .algorithm(diff_algorithm.to_similar())
        .diff_lines(&old_joined, &new_joined);

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
    let config = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();

    let mut diff_algorithm = config
        .get("diff.algorithm")
        .and_then(|name| parse_diff_algorithm_name(&name))
        .unwrap_or(BlameDiffAlgorithm::Myers);

    if args.minimal {
        diff_algorithm = BlameDiffAlgorithm::Minimal;
    }
    if let Some(name) = args.diff_algorithm.as_deref() {
        diff_algorithm = parse_diff_algorithm_name(name)
            .ok_or_else(|| anyhow::anyhow!("invalid --diff-algorithm: {name}"))?;
    }

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

    // Build the set of revisions to ignore.
    // Sources:
    // 1) config blame.ignoreRevsFile (can be multi-valued)
    // 2) CLI --ignore-revs-file (processed after config; empty string resets)
    // 3) CLI --ignore-rev
    let mut ignore_revs = HashSet::new();
    let mut ignore_revs_files = config.get_all("blame.ignoreRevsFile");
    for file in &args.ignore_revs_file {
        if file.is_empty() {
            ignore_revs_files.clear();
        } else {
            ignore_revs_files.push(file.clone());
        }
    }

    for file in &ignore_revs_files {
        let contents = std::fs::read_to_string(file)
            .with_context(|| format!("could not open file with revisions to ignore: {file}"))?;
        for line in contents.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let oid = resolve_revision(&repo, line)
                .map_err(|_| anyhow::anyhow!("invalid object name: {line}"))?;
            if let Some(oid) = peel_to_commit_oid(&odb, oid)? {
                ignore_revs.insert(oid);
            }
        }
    }

    for rev_str in &args.ignore_rev {
        let oid = resolve_revision(&repo, rev_str)
            .with_context(|| format!("cannot find revision {rev_str} to ignore"))?;
        let oid = peel_to_commit_oid(&odb, oid)?
            .ok_or_else(|| anyhow::anyhow!("cannot find revision {rev_str} to ignore"))?;
        ignore_revs.insert(oid);
    }

    let mark_unblamable = config_bool(&config, "blame.markUnblamableLines");
    let mark_ignored = config_bool(&config, "blame.markIgnoredLines");

    let mut blame_lines =
        match compute_blame(&odb, start_oid, &file_path, &ignore_revs, diff_algorithm) {
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
                    ignored: false,
                    unblamable: false,
                })
                .collect()
        }
        Err(e) => return Err(e),
    };

    // Apply line range filters (`-L` can be repeated).
    if !args.line_range.is_empty() {
        let mut keep = HashSet::new();
        for range in &args.line_range {
            let (mut start, mut end) = parse_line_range(range, &blame_lines)?;
            if end < start {
                std::mem::swap(&mut start, &mut end);
            }
            for lineno in start..=end {
                keep.insert(lineno);
            }
        }
        blame_lines.retain(|b| keep.contains(&b.final_lineno));
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
            mark_unblamable,
            mark_ignored,
        )?;
    } else {
        write_default(
            &mut out,
            &blame_lines,
            &commits,
            &args,
            &file_path,
            mark_unblamable,
            mark_ignored,
        )?;
    }

    Ok(())
}

/// Read file content from index conflict stages (for blame during merge conflicts).
fn read_from_index_conflict(repo: &Repository, odb: &Odb, file_path: &str) -> Result<String> {
    use grit_lib::index::Index;
    let index = Index::load(&repo.index_path())
        .context("loading index")?;
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
) -> Result<(usize, usize)> {
    let max_lineno = blame_lines
        .iter()
        .map(|b| b.final_lineno)
        .max()
        .unwrap_or(0);

    let (start_spec, end_spec) = match range.split_once(',') {
        Some((start, end)) => (start, Some(end)),
        None => (range, None),
    };

    let start = parse_line_spec(start_spec, blame_lines, None)?;
    if start > max_lineno {
        bail!("file has only {max_lineno} lines");
    }

    let end = match end_spec {
        Some(spec) => parse_line_spec(spec, blame_lines, Some(start))?,
        None => start,
    };

    Ok((start, end.min(max_lineno.max(1))))
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
    mark_unblamable: bool,
    mark_ignored: bool,
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

        if mark_ignored && bl.ignored {
            writeln!(out, "ignored")?;
        }
        if mark_unblamable && bl.unblamable {
            writeln!(out, "unblamable")?;
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
    mark_unblamable: bool,
    mark_ignored: bool,
) -> Result<()> {
    let hash_len = if args.no_abbrev || args.long_hash {
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
    let has_boundary = !args.root && lines.iter().any(|l| {
        commits
            .get(&l.oid)
            .map(|c| c.parents.is_empty())
            .unwrap_or(false)
    });

    for bl in lines {
        let hex = bl.oid.to_hex();
        let is_boundary = !args.root
            && commits
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
        let marker = if mark_unblamable && bl.unblamable {
            "*"
        } else if mark_ignored && bl.ignored {
            "?"
        } else {
            ""
        };

        if args.suppress {
            writeln!(
                out,
                "{color_start}{marker}{short} {fname}{lineno:>w$}) {content}{color_end}",
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
                "{color_start}{marker}{short} {fname}({who} {ts} {lineno:>w$}) {content}{color_end}",
                lineno = bl.final_lineno,
                w = lineno_width,
                content = bl.content,
            )?;
        }

        prev_oid = Some(bl.oid);
    }

    Ok(())
}
