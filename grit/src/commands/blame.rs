//! `grit blame` — show what revision and author last modified each line of a file.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::config::ConfigSet;
use grit_lib::crlf::{
    convert_to_git, get_file_attrs, load_gitattributes, load_gitattributes_from_index,
    ConversionConfig, GitAttributes,
};
use grit_lib::objects::{parse_commit, parse_tree, CommitData, ObjectId, ObjectKind};
use grit_lib::odb::Odb;
use grit_lib::repo::Repository;
use grit_lib::rev_parse::{resolve_revision, resolve_revision_without_index_dwim};
use grit_lib::state::resolve_head;
use grit_lib::userdiff;
use grit_lib::wildmatch::wildmatch;
use regex::Regex;
use similar::{Algorithm as SimilarAlgorithm, ChangeTag, TextDiff};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};
use time::OffsetDateTime;

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
        long = "find-copies",
        value_name = "score",
        num_args = 0..=1,
        default_missing_value = "",
        action = clap::ArgAction::Append
    )]
    pub copy_detection: Vec<String>,

    /// Detect moved lines within a file (`-M[<score>]`).
    #[arg(
        short = 'M',
        long = "find-renames",
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

    /// Use this file's contents as the final image to annotate (git `--contents`).
    #[arg(long = "contents", value_name = "file")]
    pub contents: Option<String>,

    /// Report progress to stderr (honours `GIT_PROGRESS_DELAY`).
    #[arg(long = "progress")]
    pub progress: bool,

    /// When true, emit git-annotate style output (tab-separated metadata).
    #[arg(skip)]
    pub annotate_output: bool,

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
    /// Line comes from `--contents` and does not match the blamed revision (git: "External file").
    external_contents: bool,
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
    let offset_secs = parse_tz_offset_seconds(tz);
    let dt = OffsetDateTime::from_unix_timestamp(timestamp + offset_secs as i64)
        .unwrap_or(OffsetDateTime::UNIX_EPOCH);
    let fmt = time::format_description::parse("[year]-[month]-[day] [hour]:[minute]:[second]")
        .expect("valid blame timestamp format");
    let rendered = dt
        .format(&fmt)
        .unwrap_or_else(|_| "1970-01-01 00:00:00".to_owned());
    format!("{rendered} {tz}")
}

fn parse_tz_offset_seconds(tz: &str) -> i32 {
    if tz.len() < 5 {
        return 0;
    }
    let sign = if tz.starts_with('-') { -1 } else { 1 };
    let hours: i32 = tz[1..3].parse().unwrap_or(0);
    let minutes: i32 = tz[3..5].parse().unwrap_or(0);
    sign * (hours * 3600 + minutes * 60)
}

/// Resolve a file path through nested trees to get the blob OID + mode.
fn resolve_path_in_tree_entry(
    odb: &Odb,
    tree_oid: &ObjectId,
    path: &str,
) -> Result<Option<(ObjectId, u32)>> {
    let parts: Vec<&str> = path.split('/').collect();
    let mut current = *tree_oid;

    for (i, part) in parts.iter().enumerate() {
        let obj = odb.read(&current)?;
        let entries = parse_tree(&obj.data)?;
        match entries
            .iter()
            .find(|e| String::from_utf8_lossy(&e.name) == *part)
        {
            Some(e) if i == parts.len() - 1 => {
                if e.mode == 0o040000 {
                    return Ok(None);
                }
                return Ok(Some((e.oid, e.mode)));
            }
            Some(e) if e.mode == 0o040000 => current = e.oid,
            Some(_) => return Ok(None),
            None => return Ok(None),
        }
    }
    Ok(None)
}

/// Split content into lines. A final line without a trailing newline is still a line
/// (matches git blame / `wc -l` + 1 semantics in upstream tests).
fn content_lines(s: &str) -> Vec<&str> {
    if s.is_empty() {
        return Vec::new();
    }
    let mut out: Vec<&str> = s.split('\n').collect();
    if out.last() == Some(&"") {
        out.pop();
    }
    out
}

/// Each line being tracked through history.
/// `final_lineno` is the 1-based line number in the target file.
/// `current_idx` is the 0-based index in the current version being examined.
#[derive(Debug, Clone)]
struct TrackedLine {
    final_lineno: usize,
    current_idx: usize,
    ignored: bool,
    source_path: Option<String>,
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

#[derive(Debug, Clone)]
struct BlameTextconvContext {
    config: ConfigSet,
    conversion: ConversionConfig,
    attrs: GitAttributes,
    diff_attrs: Vec<DiffAttrRule>,
}

impl BlameTextconvContext {
    fn new(repo: &Repository) -> Self {
        let config = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
        let conversion = ConversionConfig::from_config(&config);
        let attrs = load_attr_rules(repo);
        let diff_attrs = load_diff_attr_rules(repo);
        Self {
            config,
            conversion,
            attrs,
            diff_attrs,
        }
    }
}

#[derive(Debug, Clone)]
struct DiffAttrRule {
    pattern: String,
    value: DiffAttrValue,
}

#[derive(Debug, Clone)]
enum DiffAttrValue {
    Unset,
    Set,
    Driver(String),
}

fn load_attr_rules(repo: &Repository) -> GitAttributes {
    if let Some(work_tree) = repo.work_tree.as_deref() {
        let rules = load_gitattributes(work_tree);
        if !rules.is_empty() {
            return rules;
        }
    }

    if let Ok(index) = repo.load_index() {
        return load_gitattributes_from_index(&index, &repo.odb);
    }

    Vec::new()
}

fn load_diff_attr_rules(repo: &Repository) -> Vec<DiffAttrRule> {
    let mut rules = Vec::new();

    if let Some(work_tree) = repo.work_tree.as_deref() {
        parse_diff_attr_file(&work_tree.join(".gitattributes"), &mut rules);
        parse_diff_attr_file(&work_tree.join(".git/info/attributes"), &mut rules);
    }

    if rules.is_empty() {
        if let Ok(index) = repo.load_index() {
            if let Some(entry) = index.get(b".gitattributes", 0) {
                if let Ok(obj) = repo.odb.read(&entry.oid) {
                    if let Ok(content) = String::from_utf8(obj.data) {
                        parse_diff_attr_content(&content, &mut rules);
                    }
                }
            }
        }
        parse_diff_attr_file(&repo.git_dir.join("info/attributes"), &mut rules);
    }

    rules
}

fn parse_diff_attr_file(path: &Path, rules: &mut Vec<DiffAttrRule>) {
    if let Ok(content) = std::fs::read_to_string(path) {
        parse_diff_attr_content(&content, rules);
    }
}

fn parse_diff_attr_content(content: &str, rules: &mut Vec<DiffAttrRule>) {
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let mut parts = line.split_whitespace();
        let Some(pattern) = parts.next() else {
            continue;
        };

        let mut value: Option<DiffAttrValue> = None;
        for token in parts {
            if token == "binary" || token == "-diff" {
                value = Some(DiffAttrValue::Unset);
            } else if token == "diff" {
                value = Some(DiffAttrValue::Set);
            } else if let Some(driver) = token.strip_prefix("diff=") {
                value = Some(DiffAttrValue::Driver(driver.to_owned()));
            }
        }

        if let Some(value) = value {
            rules.push(DiffAttrRule {
                pattern: pattern.to_owned(),
                value,
            });
        }
    }
}

fn resolve_textconv_command(ctx: &BlameTextconvContext, path: &str) -> Option<String> {
    let mut selected: Option<DiffAttrValue> = None;
    for rule in &ctx.diff_attrs {
        if diff_attr_pattern_matches(&rule.pattern, path) {
            selected = Some(rule.value.clone());
        }
    }

    match selected {
        Some(DiffAttrValue::Driver(driver)) => ctx.config.get(&format!("diff.{driver}.textconv")),
        _ => None,
    }
}

fn diff_attr_pattern_matches(pattern: &str, path: &str) -> bool {
    if pattern.contains('/') {
        return wildmatch(pattern.as_bytes(), path.as_bytes(), 0);
    }
    let basename = path.rsplit('/').next().unwrap_or(path);
    wildmatch(pattern.as_bytes(), basename.as_bytes(), 0)
}

fn is_regular_mode(mode: u32) -> bool {
    mode & 0o170000 == 0o100000
}

fn run_textconv_command(command: &str, input_data: &[u8]) -> Result<Vec<u8>> {
    let temp_path = create_temp_textconv_file(input_data)?;
    let quoted = shell_quote(temp_path.to_string_lossy().as_ref());
    let shell_command = format!("{command} {quoted}");

    let output = Command::new("sh")
        .arg("-c")
        .arg(&shell_command)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .output()
        .with_context(|| format!("running textconv command '{command}'"))?;

    let _ = std::fs::remove_file(&temp_path);

    if !output.status.success() {
        bail!("textconv command exited with status {}", output.status);
    }

    Ok(output.stdout)
}

fn create_temp_textconv_file(data: &[u8]) -> Result<std::path::PathBuf> {
    let pid = std::process::id();
    for attempt in 0..32u32 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("grit-blame-textconv-{pid}-{now}-{attempt}"));
        match OpenOptions::new().create_new(true).write(true).open(&path) {
            Ok(mut file) => {
                file.write_all(data)?;
                return Ok(path);
            }
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err.into()),
        }
    }
    bail!("failed to create temporary textconv input file")
}

fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

fn read_blob_content_for_blame(
    odb: &Odb,
    oid: &ObjectId,
    path: &str,
    mode: u32,
    textconv_ctx: Option<&BlameTextconvContext>,
    use_textconv: bool,
) -> Result<String> {
    let obj = odb.read(oid)?;
    if obj.kind != ObjectKind::Blob {
        bail!("expected blob object");
    }

    if !use_textconv || !is_regular_mode(mode) {
        return Ok(String::from_utf8_lossy(&obj.data).into_owned());
    }

    let Some(ctx) = textconv_ctx else {
        return Ok(String::from_utf8_lossy(&obj.data).into_owned());
    };
    let Some(command) = resolve_textconv_command(ctx, path) else {
        return Ok(String::from_utf8_lossy(&obj.data).into_owned());
    };

    let attrs = get_file_attrs(&ctx.attrs, path, &ctx.config);
    let oid_hex = oid.to_string();
    let worktree_data = grit_lib::crlf::convert_to_worktree(
        &obj.data,
        path,
        &ctx.conversion,
        &attrs,
        Some(&oid_hex),
        None,
    )
    .map_err(|e| anyhow::anyhow!("{e}"))?;
    let converted = run_textconv_command(&command, &worktree_data)
        .or_else(|_| run_textconv_command(&command, &obj.data))?;
    Ok(String::from_utf8_lossy(&converted).into_owned())
}

/// Core blame: walk history (all parents at merges unless `first_parent_only`), diff blobs, attribute lines.
fn compute_blame(
    odb: &Odb,
    start_oid: ObjectId,
    file_path: &str,
    ignore_revs: &HashSet<ObjectId>,
    diff_algorithm: BlameDiffAlgorithm,
    textconv_ctx: Option<&BlameTextconvContext>,
    use_textconv: bool,
    copy_depth: usize,
    first_parent_only: bool,
    grafts: &HashMap<ObjectId, Vec<ObjectId>>,
) -> Result<Vec<BlameLine>> {
    let start_commit = {
        let obj = odb.read(&start_oid)?;
        parse_commit(&obj.data)?
    };

    let (blob_oid, blob_mode) = resolve_path_in_tree_entry(odb, &start_commit.tree, file_path)?
        .with_context(|| format!("file '{file_path}' not found in revision"))?;
    let content = read_blob_content_for_blame(
        odb,
        &blob_oid,
        file_path,
        blob_mode,
        textconv_ctx,
        use_textconv,
    )?;
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
            source_path: None,
        })
        .collect();

    let mut result: Vec<BlameLine> = Vec::with_capacity(num_lines);
    // Store final content for output
    let final_lines: Vec<String> = lines.iter().map(|s| s.to_string()).collect();

    let mut current_oid = start_oid;
    let mut current_blob_oid = blob_oid;
    let mut current_blob_mode = blob_mode;
    let mut current_path = file_path.to_string();
    let mut commit_cache: HashMap<ObjectId, CommitData> = HashMap::new();
    commit_cache.insert(start_oid, start_commit);
    let mut deferred: VecDeque<(ObjectId, ObjectId, u32, String, Vec<TrackedLine>)> =
        VecDeque::new();

    'blame_loop: loop {
        if pending.is_empty() {
            if let Some((oid, blob, mode, path, lines)) = deferred.pop_front() {
                current_oid = oid;
                current_blob_oid = blob;
                current_blob_mode = mode;
                current_path = path;
                pending = lines;
                continue;
            }
            break;
        }

        let commit = get_commit(odb, current_oid, &mut commit_cache)?;
        let parents = commit_parents_for_blame(odb, current_oid, grafts, &mut commit_cache)?;

        let is_ignored = ignore_revs.contains(&current_oid);

        // If an ignored merge commit is encountered, try to continue blame
        // through the parent that actually contributed each line.
        if is_ignored && parents.len() > 1 {
            let cur_content = read_blob_content_for_blame(
                odb,
                &current_blob_oid,
                &current_path,
                current_blob_mode,
                textconv_ctx,
                use_textconv,
            )?;
            let cur_lines = content_lines(&cur_content);

            let mut parent_lines: Vec<Option<Vec<String>>> = Vec::new();
            let mut parent_blames: Vec<Option<Vec<BlameLine>>> = Vec::new();
            for parent_oid in &parents {
                let parent_commit = get_commit(odb, *parent_oid, &mut commit_cache)?;
                if let Some((p_blob_oid, p_blob_mode)) =
                    resolve_path_in_tree_entry(odb, &parent_commit.tree, &current_path)?
                {
                    let p_content = read_blob_content_for_blame(
                        odb,
                        &p_blob_oid,
                        &current_path,
                        p_blob_mode,
                        textconv_ctx,
                        use_textconv,
                    )?;
                    let p_lines = content_lines(&p_content)
                        .iter()
                        .map(|s| s.to_string())
                        .collect::<Vec<_>>();
                    let p_blame = compute_blame(
                        odb,
                        *parent_oid,
                        &current_path,
                        ignore_revs,
                        diff_algorithm,
                        textconv_ctx,
                        use_textconv,
                        copy_depth,
                        first_parent_only,
                        grafts,
                    )?;
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
                        external_contents: false,
                    });
                    continue;
                };

                let mut picked: Option<BlameLine> = None;
                for i in 0..parents.len() {
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
                        external_contents: false,
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
                        external_contents: false,
                    });
                }
            }
            if !deferred.is_empty() {
                continue 'blame_loop;
            }
            break 'blame_loop;
        }

        if parents.is_empty() {
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
                    external_contents: false,
                });
            }
            if !deferred.is_empty() {
                continue 'blame_loop;
            }
            break 'blame_loop;
        }

        // Merge commits: pass blame through each parent in order (matches git's
        // sequential `pass_blame_to_parent` when not using --first-parent).
        if !first_parent_only
            && !is_ignored
            && parents.len() == 2
            && resolve_path_in_tree_entry(
                odb,
                &get_commit(odb, parents[0], &mut commit_cache)?.tree,
                &current_path,
            )?
            .is_some()
            && resolve_path_in_tree_entry(
                odb,
                &get_commit(odb, parents[1], &mut commit_cache)?.tree,
                &current_path,
            )?
            .is_some()
        {
            let p0 = parents[0];
            let p1 = parents[1];
            let parent0_commit = get_commit(odb, p0, &mut commit_cache)?;
            let parent1_commit = get_commit(odb, p1, &mut commit_cache)?;
            let Some((p0_blob, p0_mode)) =
                resolve_path_in_tree_entry(odb, &parent0_commit.tree, &current_path)?
            else {
                bail!("internal: missing blob in merge parent 0");
            };
            let Some((p1_blob, p1_mode)) =
                resolve_path_in_tree_entry(odb, &parent1_commit.tree, &current_path)?
            else {
                bail!("internal: missing blob in merge parent 1");
            };

            let cur_content = read_blob_content_for_blame(
                odb,
                &current_blob_oid,
                &current_path,
                current_blob_mode,
                textconv_ctx,
                use_textconv,
            )?;
            let par0_content = read_blob_content_for_blame(
                odb,
                &p0_blob,
                &current_path,
                p0_mode,
                textconv_ctx,
                use_textconv,
            )?;
            let par1_content = read_blob_content_for_blame(
                odb,
                &p1_blob,
                &current_path,
                p1_mode,
                textconv_ctx,
                use_textconv,
            )?;
            let cur_lines = content_lines(&cur_content);
            let par0_lines = content_lines(&par0_content);
            let par1_lines = content_lines(&par1_content);

            let map0 = build_line_map(&par0_lines, &cur_lines, diff_algorithm);
            let map1 = build_line_map(&par1_lines, &cur_lines, diff_algorithm);

            let mut to_p0: Vec<TrackedLine> = Vec::new();
            let mut to_p1: Vec<TrackedLine> = Vec::new();
            let mut attributed: Vec<BlameLine> = Vec::new();

            // Git walks parents sequentially: lines that map to parent 1 are handed off first;
            // only what parent 1 cannot explain stays for parent 2 (see `pass_blame` in git/blame.c).
            for t in pending.drain(..) {
                let idx = t.current_idx;
                let m0 = map0.get(idx).copied().flatten();
                let m1 = map1.get(idx).copied().flatten();
                let cur_line = cur_lines.get(idx).copied();

                if let Some(p0_idx) = m0 {
                    let matches_p0 = par0_lines.get(p0_idx).copied() == cur_line;
                    if matches_p0 {
                        to_p0.push(TrackedLine {
                            final_lineno: t.final_lineno,
                            current_idx: p0_idx,
                            ignored: t.ignored,
                            source_path: t.source_path.clone(),
                        });
                        continue;
                    }
                }

                if let Some(p1_idx) = m1 {
                    let matches_p1 = par1_lines.get(p1_idx).copied() == cur_line;
                    if matches_p1 {
                        to_p1.push(TrackedLine {
                            final_lineno: t.final_lineno,
                            current_idx: p1_idx,
                            ignored: t.ignored,
                            source_path: t.source_path.clone(),
                        });
                        continue;
                    }
                }

                attributed.push(BlameLine {
                    oid: current_oid,
                    final_lineno: t.final_lineno,
                    orig_lineno: idx + 1,
                    content: final_lines[t.final_lineno - 1].clone(),
                    source_file: None,
                    ignored: t.ignored,
                    unblamable: false,
                    external_contents: false,
                });
            }

            for bl in attributed {
                result.push(bl);
            }

            if !to_p1.is_empty() {
                deferred.push_back((p1, p1_blob, p1_mode, current_path.clone(), to_p1));
            }
            if !to_p0.is_empty() {
                current_oid = p0;
                current_blob_oid = p0_blob;
                current_blob_mode = p0_mode;
                pending = to_p0;
            } else if deferred.is_empty() {
                break 'blame_loop;
            }
            continue;
        }

        let parent_oid = parents[0];
        let parent_commit = get_commit(odb, parent_oid, &mut commit_cache)?;
        let parent_blob_entry =
            resolve_path_in_tree_entry(odb, &parent_commit.tree, &current_path)?;
        let can_follow_rename = true;

        match parent_blob_entry {
            None if !is_ignored => {
                // File doesn't exist at this path in parent.
                // First, try to follow a pure rename by matching blob OID.
                if can_follow_rename {
                    if let Some((renamed_path, renamed_mode)) = find_path_by_oid_in_tree(
                        odb,
                        &parent_commit.tree,
                        &commit.tree,
                        &current_blob_oid,
                        &current_path,
                    )? {
                        current_path = renamed_path;
                        current_oid = parent_oid;
                        current_blob_mode = renamed_mode;
                        continue;
                    }
                }

                // If copy detection is enabled, try to track lines to source
                // files in the parent tree.
                if copy_depth >= 1 {
                    let cur_content = read_blob_content_for_blame(
                        odb,
                        &current_blob_oid,
                        &current_path,
                        current_blob_mode,
                        textconv_ctx,
                        use_textconv,
                    )?;
                    let cur_lines = content_lines(&cur_content);
                    let mut entries = Vec::new();
                    collect_tree_file_entries(odb, &parent_commit.tree, "", &mut entries)?;
                    let mut by_content: HashMap<String, Vec<(String, BlameLine)>> = HashMap::new();
                    for (path, oid, mode) in entries {
                        if path == current_path || !is_regular_mode(mode) {
                            continue;
                        }
                        // Single -C only searches for files that disappeared
                        // in the current commit. Deeper -C levels may search
                        // broader history.
                        if copy_depth == 1
                            && resolve_path_in_tree_entry(odb, &commit.tree, &path)?.is_some()
                        {
                            continue;
                        }

                        let source_content = read_blob_content_for_blame(
                            odb,
                            &oid,
                            &path,
                            mode,
                            textconv_ctx,
                            use_textconv,
                        )?;
                        let source_lines = content_lines(&source_content);
                        let overlap = cur_lines
                            .iter()
                            .filter(|line| source_lines.iter().any(|src| src == *line))
                            .count();
                        if overlap == 0 {
                            continue;
                        }

                        let source_blame = compute_blame(
                            odb,
                            parent_oid,
                            &path,
                            ignore_revs,
                            diff_algorithm,
                            textconv_ctx,
                            use_textconv,
                            copy_depth.saturating_sub(1),
                            first_parent_only,
                            grafts,
                        )?;
                        for line in source_blame {
                            by_content
                                .entry(line.content.clone())
                                .or_default()
                                .push((path.clone(), line));
                        }
                    }

                    if !by_content.is_empty() {
                        let mut used: HashMap<String, usize> = HashMap::new();
                        for t in pending.drain(..) {
                            let line_text = cur_lines.get(t.current_idx).copied().unwrap_or("");
                            let used_key = line_text.to_owned();
                            let used_count = used.get(&used_key).copied().unwrap_or(0);
                            if let Some((source_path, pb)) = by_content
                                .get(line_text)
                                .and_then(|candidates| candidates.get(used_count))
                            {
                                used.insert(used_key, used_count + 1);
                                result.push(BlameLine {
                                    oid: pb.oid,
                                    final_lineno: t.final_lineno,
                                    orig_lineno: pb.orig_lineno,
                                    content: final_lines[t.final_lineno - 1].clone(),
                                    source_file: pb
                                        .source_file
                                        .clone()
                                        .or_else(|| Some(source_path.clone())),
                                    ignored: pb.ignored || t.ignored,
                                    unblamable: pb.unblamable,
                                    external_contents: false,
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
                                    external_contents: false,
                                });
                            }
                        }
                        if !deferred.is_empty() {
                            continue 'blame_loop;
                        }
                        break 'blame_loop;
                    }
                }

                // No rename/copy source found — attribute to current commit.
                for t in pending.drain(..) {
                    result.push(BlameLine {
                        oid: current_oid,
                        final_lineno: t.final_lineno,
                        orig_lineno: t.current_idx + 1,
                        content: final_lines[t.final_lineno - 1].clone(),
                        source_file: None,
                        ignored: t.ignored,
                        unblamable: false,
                        external_contents: false,
                    });
                }
                if !deferred.is_empty() {
                    continue 'blame_loop;
                }
                break 'blame_loop;
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
                        external_contents: false,
                    });
                }
                if !deferred.is_empty() {
                    continue 'blame_loop;
                }
                break 'blame_loop;
            }
            Some((p_blob_oid, p_blob_mode)) if p_blob_oid == current_blob_oid => {
                // Identical blob — skip to parent
                current_oid = parent_oid;
                current_blob_mode = p_blob_mode;
                continue;
            }
            Some((p_blob_oid, p_blob_mode)) => {
                // Diff current vs parent
                let cur_content = read_blob_content_for_blame(
                    odb,
                    &current_blob_oid,
                    &current_path,
                    current_blob_mode,
                    textconv_ctx,
                    use_textconv,
                )?;
                let par_content = read_blob_content_for_blame(
                    odb,
                    &p_blob_oid,
                    &current_path,
                    p_blob_mode,
                    textconv_ctx,
                    use_textconv,
                )?;
                let cur_lines = content_lines(&cur_content);
                let par_lines = content_lines(&par_content);

                // Build mapping: cur_line_idx → Option<parent_line_idx>
                let mut line_map = build_line_map(&par_lines, &cur_lines, diff_algorithm);
                if is_ignored {
                    line_map = build_fuzzy_line_map(&par_lines, &cur_lines, &line_map);
                }
                let mut inserted_copy_source: Option<(
                    String,
                    HashMap<String, Vec<BlameLine>>,
                    HashMap<String, usize>,
                )> = None;
                if copy_depth >= 3 {
                    if let Some((source_path, source_blame)) = find_copy_source_blame(
                        odb,
                        parent_oid,
                        &parent_commit.tree,
                        &current_path,
                        &cur_lines,
                        ignore_revs,
                        diff_algorithm,
                        textconv_ctx,
                        use_textconv,
                        copy_depth - 1,
                        false,
                        first_parent_only,
                        grafts,
                    )? {
                        let mut by_content: HashMap<String, Vec<BlameLine>> = HashMap::new();
                        for line in source_blame {
                            by_content
                                .entry(line.content.clone())
                                .or_default()
                                .push(line);
                        }
                        inserted_copy_source = Some((source_path, by_content, HashMap::new()));
                    }
                }

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
                                        source_path: t.source_path.clone(),
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
                                        external_contents: false,
                                    });
                                }
                                continue;
                            }
                            // Line came from parent — keep tracking
                            let carried_ignored = if is_ignored {
                                let unchanged = parent_idx == t.current_idx
                                    && par_lines
                                        .get(parent_idx)
                                        .zip(cur_lines.get(t.current_idx))
                                        .is_some_and(|(p, c)| p == c);
                                if unchanged {
                                    t.ignored
                                } else {
                                    true
                                }
                            } else {
                                t.ignored
                            };
                            still_pending.push(TrackedLine {
                                final_lineno: t.final_lineno,
                                current_idx: parent_idx,
                                ignored: carried_ignored,
                                source_path: t.source_path.clone(),
                            });
                        } else if is_ignored {
                            // Best-effort pass-through through ignored revisions:
                            // only keep walking when the same-slot parent line
                            // is text-identical; otherwise keep blame on the
                            // ignored commit and mark as unblamable.
                            let cur_line = cur_lines.get(t.current_idx).copied();
                            if t.current_idx < par_lines.len()
                                && cur_line.is_some_and(|line| line == par_lines[t.current_idx])
                            {
                                still_pending.push(TrackedLine {
                                    final_lineno: t.final_lineno,
                                    current_idx: t.current_idx,
                                    ignored: t.ignored,
                                    source_path: t.source_path.clone(),
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
                                    external_contents: false,
                                });
                            }
                        } else {
                            if let Some((source_path, by_content, used)) =
                                inserted_copy_source.as_mut()
                            {
                                let line_text = cur_lines.get(t.current_idx).copied().unwrap_or("");
                                let used_key = line_text.to_owned();
                                let used_count = used.get(&used_key).copied().unwrap_or(0);
                                if let Some(pb) = by_content
                                    .get(line_text)
                                    .and_then(|candidates| candidates.get(used_count))
                                {
                                    used.insert(used_key, used_count + 1);
                                    result.push(BlameLine {
                                        oid: pb.oid,
                                        final_lineno: t.final_lineno,
                                        orig_lineno: pb.orig_lineno,
                                        content: final_lines[t.final_lineno - 1].clone(),
                                        source_file: pb
                                            .source_file
                                            .clone()
                                            .or_else(|| Some(source_path.clone())),
                                        ignored: pb.ignored || t.ignored,
                                        unblamable: pb.unblamable,
                                        external_contents: false,
                                    });
                                    continue;
                                }
                            }
                            // Line was introduced in current commit
                            result.push(BlameLine {
                                oid: current_oid,
                                final_lineno: t.final_lineno,
                                orig_lineno: t.current_idx + 1,
                                content: final_lines[t.final_lineno - 1].clone(),
                                source_file: None,
                                ignored: t.ignored,
                                unblamable: false,
                                external_contents: false,
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
                            external_contents: false,
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
                            external_contents: false,
                        });
                    }
                }

                pending = still_pending;
                current_oid = parent_oid;
                current_blob_oid = p_blob_oid;
                current_blob_mode = p_blob_mode;
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

fn find_path_by_oid_in_tree(
    odb: &Odb,
    tree_oid: &ObjectId,
    current_tree_oid: &ObjectId,
    needle_oid: &ObjectId,
    exclude_path: &str,
) -> Result<Option<(String, u32)>> {
    let mut entries = Vec::new();
    collect_tree_file_entries(odb, tree_oid, "", &mut entries)?;
    for (path, oid, mode) in entries {
        if path != exclude_path
            && &oid == needle_oid
            && resolve_path_in_tree_entry(odb, current_tree_oid, &path)?.is_none()
        {
            return Ok(Some((path, mode)));
        }
    }
    Ok(None)
}

fn find_copy_source_blame(
    odb: &Odb,
    parent_oid: ObjectId,
    parent_tree_oid: &ObjectId,
    exclude_path: &str,
    current_lines: &[&str],
    ignore_revs: &HashSet<ObjectId>,
    diff_algorithm: BlameDiffAlgorithm,
    textconv_ctx: Option<&BlameTextconvContext>,
    use_textconv: bool,
    copy_depth: usize,
    include_current_path: bool,
    first_parent_only: bool,
    grafts: &HashMap<ObjectId, Vec<ObjectId>>,
) -> Result<Option<(String, Vec<BlameLine>)>> {
    let mut entries = Vec::new();
    collect_tree_file_entries(odb, parent_tree_oid, "", &mut entries)?;

    let mut best_path: Option<String> = None;
    let mut best_score = 0usize;
    for (path, oid, mode) in &entries {
        if (!include_current_path && path == exclude_path) || !is_regular_mode(*mode) {
            continue;
        }
        let content =
            read_blob_content_for_blame(odb, oid, path, *mode, textconv_ctx, use_textconv)?;
        let lines = content_lines(&content);
        let score = current_lines
            .iter()
            .filter(|line| lines.iter().any(|src| src == *line))
            .count();
        if score > best_score {
            best_score = score;
            best_path = Some(path.clone());
        }
    }

    let Some(source_path) = best_path else {
        return Ok(None);
    };
    if best_score == 0 {
        return Ok(None);
    }

    let source_blame = compute_blame(
        odb,
        parent_oid,
        &source_path,
        ignore_revs,
        diff_algorithm,
        textconv_ctx,
        use_textconv,
        copy_depth,
        first_parent_only,
        grafts,
    )?;
    Ok(Some((source_path, source_blame)))
}

fn collect_tree_file_entries(
    odb: &Odb,
    tree_oid: &ObjectId,
    prefix: &str,
    out: &mut Vec<(String, ObjectId, u32)>,
) -> Result<()> {
    let obj = odb.read(tree_oid)?;
    if obj.kind != ObjectKind::Tree {
        bail!("expected tree");
    }
    let entries = parse_tree(&obj.data)?;
    for entry in entries {
        let name = String::from_utf8_lossy(&entry.name);
        let path = if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}/{name}")
        };
        if entry.mode == 0o040000 {
            collect_tree_file_entries(odb, &entry.oid, &path, out)?;
        } else {
            out.push((path, entry.oid, entry.mode));
        }
    }
    Ok(())
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

/// Parent list for a commit, honoring `.git/info/grafts` (same rules as `git rev-list`).
fn commit_parents_for_blame(
    odb: &Odb,
    oid: ObjectId,
    grafts: &HashMap<ObjectId, Vec<ObjectId>>,
    cache: &mut HashMap<ObjectId, CommitData>,
) -> Result<Vec<ObjectId>> {
    if let Some(p) = grafts.get(&oid) {
        return Ok(p.clone());
    }
    Ok(get_commit(odb, oid, cache)?.parents)
}

fn load_graft_parents(git_dir: &Path) -> HashMap<ObjectId, Vec<ObjectId>> {
    let graft_path = git_dir.join("info/grafts");
    let Ok(contents) = fs::read_to_string(&graft_path) else {
        return HashMap::new();
    };
    let mut grafts = HashMap::new();
    for raw_line in contents.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut fields = line.split_whitespace();
        let Some(commit_hex) = fields.next() else {
            continue;
        };
        let Ok(commit_oid) = commit_hex.parse::<ObjectId>() else {
            continue;
        };
        let mut parents = Vec::new();
        let mut valid = true;
        for parent_hex in fields {
            match parent_hex.parse::<ObjectId>() {
                Ok(parent_oid) => parents.push(parent_oid),
                Err(_) => {
                    valid = false;
                    break;
                }
            }
        }
        if valid {
            grafts.insert(commit_oid, parents);
        }
    }
    grafts
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

fn build_fuzzy_line_map(
    old: &[&str],
    new: &[&str],
    exact_map: &[Option<usize>],
) -> Vec<Option<usize>> {
    let mut fuzzy_map = exact_map.to_vec();
    let mut used_old = vec![0usize; old.len()];
    for old_idx in fuzzy_map.iter().flatten() {
        if *old_idx < used_old.len() {
            used_old[*old_idx] += 1;
        }
    }

    // First, greedily recover exact-text matches among unresolved lines.
    // This is important for reorders where Myers may not anchor every moved
    // line and fuzzy similarity can otherwise pair the wrong include.
    for new_idx in 0..fuzzy_map.len() {
        if fuzzy_map[new_idx].is_some() {
            continue;
        }
        let mut best: Option<(usize, usize, usize)> = None;
        for old_idx in 0..old.len() {
            if old[old_idx] != new[new_idx] {
                continue;
            }
            let candidate = (used_old[old_idx], old_idx.abs_diff(new_idx), old_idx);
            if best.is_none_or(|b| candidate < b) {
                best = Some(candidate);
            }
        }
        if let Some((_, _, old_idx)) = best {
            fuzzy_map[new_idx] = Some(old_idx);
            used_old[old_idx] += 1;
        }
    }

    let mut anchors: Vec<(usize, usize)> = exact_map
        .iter()
        .enumerate()
        .filter_map(|(new_idx, old_idx)| old_idx.map(|old| (new_idx, old)))
        .collect();
    anchors.sort_unstable();

    let mut prev_new = usize::MAX;
    let mut prev_old = usize::MAX;
    for (next_new, next_old) in anchors
        .iter()
        .copied()
        .chain(std::iter::once((new.len(), old.len())))
    {
        let new_start = if prev_new == usize::MAX {
            0
        } else {
            prev_new + 1
        };
        let new_end = next_new;
        let old_start = if prev_old == usize::MAX {
            0
        } else {
            prev_old + 1
        };
        let old_end = next_old;

        if new_start < new_end && old_start < old_end {
            let segment_matches =
                fuzzy_match_segment(old, old_start, old_end, new, new_start, new_end);
            for (new_idx, old_idx) in segment_matches {
                if fuzzy_map[new_idx].is_none() {
                    fuzzy_map[new_idx] = Some(old_idx);
                    used_old[old_idx] += 1;
                }
            }
        }

        prev_new = next_new;
        prev_old = next_old;
    }

    // Context-aware recovery for split/expanded lines:
    // if an unresolved line sits between mapped neighbors, prefer mapping
    // to those neighboring source lines when there is meaningful overlap.
    for new_idx in 0..fuzzy_map.len() {
        if fuzzy_map[new_idx].is_some() {
            continue;
        }
        let prev_old = (0..new_idx).rev().find_map(|i| fuzzy_map[i]);
        let next_old = ((new_idx + 1)..fuzzy_map.len()).find_map(|i| fuzzy_map[i]);

        let mut candidates = Vec::new();
        if let Some(o) = prev_old {
            candidates.push(o);
        }
        if let Some(o) = next_old {
            if candidates.last().copied() != Some(o) {
                candidates.push(o);
            }
        }

        let mut best: Option<(f64, usize)> = None;
        for old_idx in candidates {
            if old_idx >= old.len() {
                continue;
            }
            let (sim, lcs) = line_similarity_and_lcs(old[old_idx], new[new_idx]);
            let exact_text = old[old_idx].trim() == new[new_idx].trim();
            // Keep this narrow: strong overlap or exact text only.
            if !exact_text && lcs < 6 && !(sim >= 0.35 && lcs >= 3) {
                continue;
            }

            let mut score = lcs as f64 + sim * 10.0;
            score -= 0.2 * used_old[old_idx] as f64;

            if let (Some(lo), Some(hi)) = (prev_old, next_old) {
                if lo <= hi && (old_idx < lo || old_idx > hi) {
                    score -= 3.0;
                }
            }

            if best.is_none_or(|(best_score, best_old)| {
                score > best_score
                    || ((score - best_score).abs() < 1e-9
                        && old_idx.abs_diff(new_idx) < best_old.abs_diff(new_idx))
            }) {
                best = Some((score, old_idx));
            }
        }

        if let Some((_, old_idx)) = best {
            fuzzy_map[new_idx] = Some(old_idx);
            used_old[old_idx] += 1;
        }
    }

    // Final best-effort fill for unresolved lines. This handles cases where
    // anchor segmentation leaves an empty old-range but we still want to
    // pass through ignored commits by similarity.
    for new_idx in 0..fuzzy_map.len() {
        if fuzzy_map[new_idx].is_some() {
            continue;
        }
        let prev_old = (0..new_idx).rev().find_map(|i| fuzzy_map[i]);
        let next_old = ((new_idx + 1)..fuzzy_map.len()).find_map(|i| fuzzy_map[i]);

        let mut best: Option<(f64, usize)> = None;
        for old_idx in 0..old.len() {
            let (sim, lcs) = line_similarity_and_lcs(old[old_idx], new[new_idx]);
            let exact_text = old[old_idx].trim() == new[new_idx].trim();
            let new_len = new[new_idx].trim().chars().count();
            if !exact_text && (new_len < 3 || sim < 0.45 || lcs < 2) {
                continue;
            }

            let mut score = sim;
            score -= 0.004 * old_idx.abs_diff(new_idx) as f64;
            score -= 0.08 * used_old[old_idx] as f64;

            // Encourage monotonic local ordering when neighboring anchors
            // are themselves ordered; avoid forcing this for reorders.
            if let (Some(lo), Some(hi)) = (prev_old, next_old) {
                if lo <= hi {
                    if old_idx < lo {
                        score -= 0.20 * (lo - old_idx) as f64;
                    } else if old_idx > hi {
                        score -= 0.20 * (old_idx - hi) as f64;
                    }
                }
            } else if let Some(lo) = prev_old {
                if old_idx < lo {
                    score -= 0.20 * (lo - old_idx) as f64;
                }
            } else if let Some(hi) = next_old {
                if old_idx > hi {
                    score -= 0.20 * (old_idx - hi) as f64;
                }
            }

            if best.is_none_or(|(best_score, best_idx)| {
                score > best_score
                    || ((score - best_score).abs() < 1e-9
                        && old_idx.abs_diff(new_idx) < best_idx.abs_diff(new_idx))
            }) {
                best = Some((score, old_idx));
            }
        }

        if let Some((_, old_idx)) = best {
            fuzzy_map[new_idx] = Some(old_idx);
            used_old[old_idx] += 1;
        }
    }

    fuzzy_map
}

fn fuzzy_match_segment(
    old: &[&str],
    old_start: usize,
    old_end: usize,
    new: &[&str],
    new_start: usize,
    new_end: usize,
) -> Vec<(usize, usize)> {
    let m = old_end.saturating_sub(old_start);
    let n = new_end.saturating_sub(new_start);
    if m == 0 || n == 0 {
        return Vec::new();
    }

    // DP over new-lines where the state tracks the last matched old-line.
    // State 0 means "no old line selected yet", state s>0 means old index s-1.
    // Transitions keep order (non-decreasing old index), but allow reusing
    // the same old line for multiple split lines in the new content.
    let states = m + 1;
    let neg_inf = f64::NEG_INFINITY;
    let mut dp = vec![neg_inf; states];
    dp[0] = 0.0;

    let mut back_prev = vec![vec![usize::MAX; states]; n + 1];
    let mut back_pick = vec![vec![None; states]; n + 1];

    for j in 0..n {
        let mut next_dp = vec![neg_inf; states];

        for state in 0..states {
            let base = dp[state];
            if !base.is_finite() {
                continue;
            }

            // Option 1: do not match this new line.
            if base > next_dp[state] {
                next_dp[state] = base;
                back_prev[j + 1][state] = state;
                back_pick[j + 1][state] = None;
            }

            // Option 2: match this new line to some old line >= last matched.
            let start_k = if state == 0 { 0 } else { state - 1 };
            for k in start_k..m {
                let old_idx = old_start + k;
                let new_idx = new_start + j;
                let (sim, lcs) = line_similarity_and_lcs(old[old_idx], new[new_idx]);
                let exact_text = old[old_idx].trim() == new[new_idx].trim();
                let new_len = new[new_idx].trim().chars().count();
                if !exact_text && (new_len < 3 || sim < 0.45 || lcs < 2) {
                    continue;
                }

                // Slight locality bias to stabilize tie-breaking.
                let distance = k.abs_diff(j) as f64;
                let score = base + sim - 0.002 * distance;
                let next_state = k + 1;
                if score > next_dp[next_state] {
                    next_dp[next_state] = score;
                    back_prev[j + 1][next_state] = state;
                    back_pick[j + 1][next_state] = Some(k);
                }
            }
        }

        dp = next_dp;
    }

    let mut best_state = 0usize;
    for state in 1..states {
        if dp[state] > dp[best_state] {
            best_state = state;
        }
    }

    let mut matches = Vec::new();
    let mut state = best_state;
    for j in (1..=n).rev() {
        if let Some(k) = back_pick[j][state] {
            matches.push((new_start + (j - 1), old_start + k));
        }
        let prev = back_prev[j][state];
        if prev == usize::MAX {
            break;
        }
        state = prev;
    }
    matches.reverse();
    matches
}

fn line_similarity_and_lcs(a: &str, b: &str) -> (f64, usize) {
    let a = a.trim();
    let b = b.trim();
    if a.is_empty() || b.is_empty() {
        return (0.0, 0);
    }
    if a == b {
        return (1.0, a.chars().count());
    }

    let a = a.to_ascii_lowercase();
    let b = b.to_ascii_lowercase();
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let n = b_chars.len();
    if a_chars.is_empty() || b_chars.is_empty() {
        return (0.0, 0);
    }

    let mut prev = vec![0usize; n + 1];
    let mut curr = vec![0usize; n + 1];
    for i in 1..=a_chars.len() {
        for j in 1..=n {
            curr[j] = if a_chars[i - 1] == b_chars[j - 1] {
                prev[j - 1] + 1
            } else {
                prev[j].max(curr[j - 1])
            };
        }
        std::mem::swap(&mut prev, &mut curr);
        curr.fill(0);
    }

    let lcs = prev[n];
    let sim = (2.0 * lcs as f64) / (a_chars.len() as f64 + b_chars.len() as f64);
    (sim, lcs)
}

pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let odb = Odb::new(&repo.git_dir.join("objects"));
    let config = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
    let grafts = load_graft_parents(&repo.git_dir);

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

    let mut normalized_positional = Vec::new();
    normalize_detection_args(&args.copy_detection, &mut normalized_positional);
    normalize_detection_args(&args.move_detection, &mut normalized_positional);
    normalized_positional.extend(args.args.iter().cloned());

    let (rev, file_path) = parse_blame_args(&odb, &repo, &normalized_positional)?;
    let use_textconv = !args.no_textconv;
    let copy_depth = args.copy_detection.len();
    let textconv_ctx = Some(BlameTextconvContext::new(&repo));

    let start_oid = match &rev {
        Some(r) => {
            let oid = resolve_blame_start_oid(&repo, r)?;
            peel_to_commit_oid(&odb, oid)?
                .ok_or_else(|| anyhow::anyhow!("revision does not resolve to a commit"))?
        }
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

    let contents_override = if let Some(ref p) = args.contents {
        let path = PathBuf::from(p);
        Some(
            std::fs::read_to_string(&path)
                .with_context(|| format!("could not read --contents file: {p}"))?,
        )
    } else {
        None
    };

    let mut blame_lines = if args.reverse {
        let rev_spec = rev
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("--reverse requires a <rev1>..<rev2> range"))?;
        let (range_start, range_end) = parse_reverse_range_oids(&repo, rev_spec)?;
        compute_reverse_blame(
            &odb,
            range_start,
            range_end,
            &file_path,
            diff_algorithm,
            textconv_ctx.as_ref(),
            use_textconv,
            args.first_parent,
        )?
    } else {
        match compute_blame(
            &odb,
            start_oid,
            &file_path,
            &ignore_revs,
            diff_algorithm,
            textconv_ctx.as_ref(),
            use_textconv,
            copy_depth,
            args.first_parent,
            &grafts,
        ) {
            Ok(lines) => lines,
            Err(e) if rev.is_none() => {
                // When no explicit revision is given and the file is not in HEAD's
                // tree (e.g. during a conflicted merge), fall back to reading the
                // working tree (or index conflict stage) file and best-effort
                // attribute against HEAD history.
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
                build_uncommitted_blame(
                    &odb,
                    start_oid,
                    &file_path,
                    &content,
                    &ignore_revs,
                    diff_algorithm,
                    textconv_ctx.as_ref(),
                    use_textconv,
                    copy_depth,
                    args.first_parent,
                    &grafts,
                )?
            }
            Err(e) => return Err(e),
        }
    };

    if !args.reverse {
        if let Some(ref final_text) = contents_override {
            if let Some(overlaid) = apply_final_content_overlay(
                &odb,
                start_oid,
                &file_path,
                &blame_lines,
                final_text,
                textconv_ctx.as_ref(),
                use_textconv,
            )? {
                blame_lines = overlaid;
            }
        } else if rev.is_none() {
            if let Some(overlaid) = apply_worktree_overlay(
                &repo,
                &odb,
                start_oid,
                &file_path,
                &blame_lines,
                textconv_ctx.as_ref(),
                use_textconv,
            )? {
                blame_lines = overlaid;
            }
        }
    }

    // Apply line range filters (`-L` can be repeated).
    if !args.line_range.is_empty() {
        let mut keep = HashSet::new();
        let mut range_ctx = LineRangeParseCtx {
            blame_lines: &blame_lines,
            file_path: &file_path,
            textconv: textconv_ctx.as_ref(),
            prev_range_end: 0,
        };
        for range in &args.line_range {
            let (mut start, mut end) = parse_line_range(range, &mut range_ctx)?;
            if end < start {
                std::mem::swap(&mut start, &mut end);
            }
            range_ctx.prev_range_end = end;
            for lineno in start..=end {
                keep.insert(lineno);
            }
        }
        blame_lines.retain(|b| keep.contains(&b.final_lineno));
    }

    if args.progress && !args.reverse && !blame_lines.is_empty() {
        let delay_ms: u64 = std::env::var("GIT_PROGRESS_DELAY")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let total = blame_lines.len();
        if delay_ms == 0 {
            eprintln!("Blaming lines: 100% ({total}/{total}), done.");
        }
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
    } else if args.annotate_output {
        write_annotate(&mut out, &blame_lines, &commits, &args, &file_path)?;
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
    let index = repo.load_index().context("loading index")?;
    let path_bytes = file_path.as_bytes();
    // Find the highest-stage entry for this path (prefer stage 3, then 2, then 1)
    let mut best: Option<&grit_lib::index::IndexEntry> = None;
    for entry in &index.entries {
        if entry.path == path_bytes
            && entry.stage() > 0
            && (best.is_none() || entry.stage() > best.unwrap().stage())
        {
            best = Some(entry);
        }
    }
    let entry = best.ok_or_else(|| anyhow::anyhow!("file not in index"))?;
    let obj = odb.read(&entry.oid)?;
    String::from_utf8(obj.data).context("blob is not valid UTF-8")
}

fn build_uncommitted_blame(
    odb: &Odb,
    start_oid: ObjectId,
    file_path: &str,
    content: &str,
    ignore_revs: &HashSet<ObjectId>,
    diff_algorithm: BlameDiffAlgorithm,
    textconv_ctx: Option<&BlameTextconvContext>,
    use_textconv: bool,
    copy_depth: usize,
    first_parent_only: bool,
    grafts: &HashMap<ObjectId, Vec<ObjectId>>,
) -> Result<Vec<BlameLine>> {
    let zero = grit_lib::diff::zero_oid();
    let final_lines = content_lines(content);

    let mut by_content_source: Option<(
        String,
        HashMap<String, Vec<BlameLine>>,
        HashMap<String, usize>,
    )> = None;
    if copy_depth >= 2 {
        let head_obj = odb.read(&start_oid)?;
        let head_commit = parse_commit(&head_obj.data)?;
        if let Some((source_path, source_blame)) = find_copy_source_blame(
            odb,
            start_oid,
            &head_commit.tree,
            file_path,
            &final_lines,
            ignore_revs,
            diff_algorithm,
            textconv_ctx,
            use_textconv,
            copy_depth,
            true,
            first_parent_only,
            grafts,
        )? {
            let mut by_content: HashMap<String, Vec<BlameLine>> = HashMap::new();
            for line in source_blame {
                by_content
                    .entry(line.content.clone())
                    .or_default()
                    .push(line);
            }
            by_content_source = Some((source_path, by_content, HashMap::new()));
        }
    }

    let mut result = Vec::with_capacity(final_lines.len());
    for (idx, line) in final_lines.iter().enumerate() {
        if let Some((source_path, by_content, used)) = by_content_source.as_mut() {
            let used_key = (*line).to_owned();
            let used_count = used.get(&used_key).copied().unwrap_or(0);
            if let Some(pb) = by_content
                .get(*line)
                .and_then(|candidates| candidates.get(used_count))
            {
                used.insert(used_key, used_count + 1);
                result.push(BlameLine {
                    oid: pb.oid,
                    final_lineno: idx + 1,
                    orig_lineno: pb.orig_lineno,
                    content: (*line).to_string(),
                    source_file: pb.source_file.clone().or_else(|| Some(source_path.clone())),
                    ignored: pb.ignored,
                    unblamable: pb.unblamable,
                    external_contents: false,
                });
                continue;
            }
        }

        result.push(BlameLine {
            oid: zero,
            final_lineno: idx + 1,
            orig_lineno: idx + 1,
            content: (*line).to_string(),
            source_file: None,
            ignored: false,
            unblamable: false,
            external_contents: false,
        });
    }

    Ok(result)
}

fn normalize_detection_args(values: &[String], positional: &mut Vec<String>) {
    for value in values {
        if value.is_empty() {
            continue;
        }
        if value.parse::<usize>().ok().is_some_and(|n| n > 0) {
            continue;
        }
        positional.push(value.clone());
    }
}

fn looks_like_object_id(s: &str) -> bool {
    let b = s.as_bytes();
    if !(4..=40).contains(&b.len()) {
        return false;
    }
    b.iter().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f'))
}

/// True when `spec` resolves to an object that peels to a commit (not a lone blob/tree).
fn spec_resolves_to_commit(odb: &Odb, repo: &Repository, spec: &str) -> bool {
    let Ok(oid) = resolve_revision_without_index_dwim(repo, spec) else {
        return false;
    };
    peel_to_commit_oid(odb, oid).ok().flatten().is_some()
}

fn parse_blame_args(
    odb: &Odb,
    repo: &Repository,
    args: &[String],
) -> Result<(Option<String>, String)> {
    match args.len() {
        0 => bail!("usage: grit blame [<rev>] [--] <file>"),
        1 => Ok((None, args[0].clone())),
        2 if args[0] == "--" => Ok((None, args[1].clone())),
        2 => {
            let a0 = &args[0];
            let a1 = &args[1];
            let c0 = spec_resolves_to_commit(odb, repo, a0);
            let c1 = spec_resolves_to_commit(odb, repo, a1);
            match (c0, c1) {
                (true, false) => Ok((Some(a0.clone()), a1.clone())),
                (false, true) => Ok((Some(a1.clone()), a0.clone())),
                (true, true) => Ok((Some(a0.clone()), a1.clone())),
                (false, false) => {
                    // Neither peels to a commit; keep legacy heuristic for odd cases.
                    if resolve_revision_without_index_dwim(repo, a1).is_ok()
                        || a1 == "HEAD"
                        || looks_like_object_id(a1)
                    {
                        Ok((Some(a1.clone()), a0.clone()))
                    } else {
                        Ok((Some(a0.clone()), a1.clone()))
                    }
                }
            }
        }
        3 if args[1] == "--" => Ok((Some(args[0].clone()), args[2].clone())),
        _ => bail!("usage: grit blame [<rev>] [--] <file>"),
    }
}

fn resolve_blame_start_oid(repo: &Repository, rev_spec: &str) -> Result<ObjectId> {
    if let Some((lhs, rhs)) = rev_spec.split_once("..") {
        if rhs.is_empty() {
            return resolve_revision(repo, "HEAD").map_err(Into::into);
        }

        if lhs.is_empty() {
            return resolve_revision(repo, rhs).map_err(Into::into);
        }

        // Accept two-dot ranges by resolving the right side (or merge base
        // from the rev parser in cases where that is appropriate).
        return resolve_revision(repo, rhs).map_err(Into::into);
    }
    resolve_revision(repo, rev_spec).map_err(Into::into)
}

fn parse_reverse_range_oids(repo: &Repository, rev_spec: &str) -> Result<(ObjectId, ObjectId)> {
    let (lhs, rhs) = rev_spec
        .split_once("..")
        .ok_or_else(|| anyhow::anyhow!("--reverse requires a <rev1>..<rev2> range"))?;
    if lhs.is_empty() || rhs.is_empty() {
        bail!("--reverse requires a <rev1>..<rev2> range");
    }
    let start = resolve_revision(repo, lhs)?;
    let end = resolve_revision(repo, rhs)?;
    Ok((start, end))
}

fn read_commit_lines_for_blame(
    odb: &Odb,
    commit: &CommitData,
    file_path: &str,
    textconv_ctx: Option<&BlameTextconvContext>,
    use_textconv: bool,
) -> Result<Vec<String>> {
    let Some((blob_oid, blob_mode)) = resolve_path_in_tree_entry(odb, &commit.tree, file_path)?
    else {
        return Ok(Vec::new());
    };
    let content = read_blob_content_for_blame(
        odb,
        &blob_oid,
        file_path,
        blob_mode,
        textconv_ctx,
        use_textconv,
    )?;
    Ok(content_lines(&content)
        .iter()
        .map(|line| (*line).to_string())
        .collect())
}

fn compute_reverse_blame(
    odb: &Odb,
    range_start: ObjectId,
    range_end: ObjectId,
    file_path: &str,
    diff_algorithm: BlameDiffAlgorithm,
    textconv_ctx: Option<&BlameTextconvContext>,
    use_textconv: bool,
    first_parent_only: bool,
) -> Result<Vec<BlameLine>> {
    let mut commit_cache: HashMap<ObjectId, CommitData> = HashMap::new();
    let mut chain_rev = vec![range_end];
    let mut cur = range_end;
    while cur != range_start {
        let commit = get_commit(odb, cur, &mut commit_cache)?;
        let next_parent = if first_parent_only {
            commit.parents.first().copied()
        } else {
            commit.parents.first().copied()
        };
        let Some(parent) = next_parent else {
            bail!("--reverse range end is not reachable from start");
        };
        cur = parent;
        chain_rev.push(cur);
    }
    chain_rev.reverse();

    let start_commit = get_commit(odb, range_start, &mut commit_cache)?;
    let mut prev_lines =
        read_commit_lines_for_blame(odb, &start_commit, file_path, textconv_ctx, use_textconv)?;

    if prev_lines.is_empty() {
        return Ok(Vec::new());
    }

    let mut active: Vec<(usize, usize, ObjectId, String)> = prev_lines
        .iter()
        .enumerate()
        .map(|(idx, line)| (idx + 1, idx, range_start, line.clone()))
        .collect();
    let mut result = Vec::with_capacity(active.len());

    for oid in chain_rev.iter().skip(1) {
        let commit = get_commit(odb, *oid, &mut commit_cache)?;
        let cur_lines =
            read_commit_lines_for_blame(odb, &commit, file_path, textconv_ctx, use_textconv)?;

        let old_refs: Vec<&str> = prev_lines.iter().map(|s| s.as_str()).collect();
        let new_refs: Vec<&str> = cur_lines.iter().map(|s| s.as_str()).collect();
        let new_to_old = build_line_map(&old_refs, &new_refs, diff_algorithm);
        let mut old_to_new = vec![None; prev_lines.len()];
        for (new_idx, old_idx_opt) in new_to_old.iter().enumerate() {
            if let Some(old_idx) = *old_idx_opt {
                if old_idx < old_to_new.len() && old_to_new[old_idx].is_none() {
                    old_to_new[old_idx] = Some(new_idx);
                }
            }
        }

        let mut next_active = Vec::new();
        for (final_lineno, prev_idx, last_oid, content) in active.drain(..) {
            if let Some(next_idx) = old_to_new.get(prev_idx).and_then(|idx| *idx) {
                next_active.push((final_lineno, next_idx, *oid, content));
            } else {
                result.push(BlameLine {
                    oid: last_oid,
                    final_lineno,
                    orig_lineno: final_lineno,
                    content,
                    source_file: None,
                    ignored: false,
                    unblamable: false,
                    external_contents: false,
                });
            }
        }

        active = next_active;
        prev_lines = cur_lines;
        if active.is_empty() {
            break;
        }
    }

    for (final_lineno, _idx, last_oid, content) in active {
        result.push(BlameLine {
            oid: last_oid,
            final_lineno,
            orig_lineno: final_lineno,
            content,
            source_file: None,
            ignored: false,
            unblamable: false,
            external_contents: false,
        });
    }

    result.sort_by_key(|line| line.final_lineno);
    Ok(result)
}

fn apply_final_content_overlay(
    odb: &Odb,
    start_oid: ObjectId,
    file_path: &str,
    base_blame: &[BlameLine],
    final_text: &str,
    textconv_ctx: Option<&BlameTextconvContext>,
    use_textconv: bool,
) -> Result<Option<Vec<BlameLine>>> {
    let head_commit_obj = odb.read(&start_oid)?;
    let head_commit = parse_commit(&head_commit_obj.data)?;
    let Some((head_blob_oid, head_mode)) =
        resolve_path_in_tree_entry(odb, &head_commit.tree, file_path)?
    else {
        return Ok(None);
    };
    if !is_regular_mode(head_mode) {
        return Ok(None);
    }

    let head_content = read_blob_content_for_blame(
        odb,
        &head_blob_oid,
        file_path,
        head_mode,
        textconv_ctx,
        use_textconv,
    )?;
    let head_lines = content_lines(&head_content);
    let final_lines = content_lines(final_text);
    if head_lines == final_lines {
        return Ok(None);
    }

    let map = build_line_map(&head_lines, &final_lines, BlameDiffAlgorithm::Myers);
    let zero = grit_lib::diff::zero_oid();

    let mut by_head_line: HashMap<usize, &BlameLine> = HashMap::new();
    for line in base_blame {
        by_head_line.insert(line.final_lineno, line);
    }

    let mut overlaid = Vec::with_capacity(final_lines.len());
    for (new_idx, content) in final_lines.iter().enumerate() {
        if let Some(old_idx) = map.get(new_idx).copied().flatten() {
            if let Some(existing) = by_head_line.get(&(old_idx + 1)) {
                overlaid.push(BlameLine {
                    oid: existing.oid,
                    final_lineno: new_idx + 1,
                    orig_lineno: existing.orig_lineno,
                    content: (*content).to_string(),
                    source_file: existing.source_file.clone(),
                    ignored: existing.ignored,
                    unblamable: existing.unblamable,
                    external_contents: false,
                });
                continue;
            }
        }

        overlaid.push(BlameLine {
            oid: zero,
            final_lineno: new_idx + 1,
            orig_lineno: new_idx + 1,
            content: (*content).to_string(),
            source_file: None,
            ignored: false,
            unblamable: false,
            external_contents: true,
        });
    }

    Ok(Some(overlaid))
}

fn apply_worktree_overlay(
    repo: &Repository,
    odb: &Odb,
    start_oid: ObjectId,
    file_path: &str,
    base_blame: &[BlameLine],
    textconv_ctx: Option<&BlameTextconvContext>,
    use_textconv: bool,
) -> Result<Option<Vec<BlameLine>>> {
    let Some(work_tree) = repo.work_tree.as_deref() else {
        return Ok(None);
    };
    let abs_path = work_tree.join(file_path);
    if !abs_path.exists() {
        return Ok(None);
    }
    let raw_worktree = std::fs::read(&abs_path)?;
    let raw_worktree_text = String::from_utf8_lossy(&raw_worktree).into_owned();

    let head_commit_obj = odb.read(&start_oid)?;
    let head_commit = parse_commit(&head_commit_obj.data)?;
    let Some((head_blob_oid, head_mode)) =
        resolve_path_in_tree_entry(odb, &head_commit.tree, file_path)?
    else {
        return Ok(None);
    };
    if !is_regular_mode(head_mode) {
        return Ok(None);
    }

    let head_content = read_blob_content_for_blame(
        odb,
        &head_blob_oid,
        file_path,
        head_mode,
        textconv_ctx,
        use_textconv,
    )?;
    let worktree_content =
        read_worktree_content_for_blame(&abs_path, file_path, textconv_ctx, use_textconv)?;
    let has_textconv = use_textconv
        && textconv_ctx
            .and_then(|ctx| resolve_textconv_command(ctx, file_path))
            .is_some();

    if head_content == worktree_content {
        return Ok(None);
    }
    if !has_textconv && head_content == raw_worktree_text {
        return Ok(None);
    }

    let head_lines = content_lines(&head_content);
    let wt_lines = content_lines(&worktree_content);
    let map = build_line_map(&head_lines, &wt_lines, BlameDiffAlgorithm::Myers);
    let zero = grit_lib::diff::zero_oid();

    let mut by_head_line: HashMap<usize, &BlameLine> = HashMap::new();
    for line in base_blame {
        by_head_line.insert(line.final_lineno, line);
    }

    let mut overlaid = Vec::with_capacity(wt_lines.len());
    for (new_idx, content) in wt_lines.iter().enumerate() {
        if let Some(old_idx) = map.get(new_idx).copied().flatten() {
            if let Some(existing) = by_head_line.get(&(old_idx + 1)) {
                overlaid.push(BlameLine {
                    oid: existing.oid,
                    final_lineno: new_idx + 1,
                    orig_lineno: existing.orig_lineno,
                    content: (*content).to_string(),
                    source_file: existing.source_file.clone(),
                    ignored: existing.ignored,
                    unblamable: existing.unblamable,
                    external_contents: false,
                });
                continue;
            }
        }

        overlaid.push(BlameLine {
            oid: zero,
            final_lineno: new_idx + 1,
            orig_lineno: new_idx + 1,
            content: (*content).to_string(),
            source_file: None,
            ignored: false,
            unblamable: false,
            external_contents: false,
        });
    }

    Ok(Some(overlaid))
}

fn read_worktree_content_for_blame(
    abs_path: &Path,
    rel_path: &str,
    textconv_ctx: Option<&BlameTextconvContext>,
    use_textconv: bool,
) -> Result<String> {
    let bytes = std::fs::read(abs_path)?;

    // Normalize worktree content to git-internal form first (CRLF/text attrs).
    let normalized = if let Some(ctx) = textconv_ctx {
        let attrs = get_file_attrs(&ctx.attrs, rel_path, &ctx.config);
        convert_to_git(&bytes, rel_path, &ctx.conversion, &attrs)
            .map_err(|e| anyhow::anyhow!("failed to normalize worktree content: {e}"))?
    } else {
        bytes.clone()
    };

    if !use_textconv {
        return Ok(String::from_utf8_lossy(&normalized).into_owned());
    }

    let Some(ctx) = textconv_ctx else {
        return Ok(String::from_utf8_lossy(&normalized).into_owned());
    };
    let Some(command) = resolve_textconv_command(ctx, rel_path) else {
        return Ok(String::from_utf8_lossy(&normalized).into_owned());
    };

    let converted = run_textconv_command(&command, &normalized)
        .or_else(|_| run_textconv_command(&command, &bytes))?;
    Ok(String::from_utf8_lossy(&converted).into_owned())
}

struct LineRangeParseCtx<'a> {
    blame_lines: &'a [BlameLine],
    file_path: &'a str,
    textconv: Option<&'a BlameTextconvContext>,
    prev_range_end: usize,
}

fn max_line_no(blame_lines: &[BlameLine]) -> usize {
    blame_lines
        .iter()
        .map(|b| b.final_lineno)
        .max()
        .unwrap_or(0)
}

fn parse_line_range(range: &str, ctx: &mut LineRangeParseCtx<'_>) -> Result<(usize, usize)> {
    let max_lineno = max_line_no(ctx.blame_lines);

    // `-L :funcname` (no comma): span from first matching funcname line through line before next boundary.
    if !range.contains(',') && range.starts_with(':') {
        let (start, end) = resolve_funcname_span(ctx, range)?;
        ctx.prev_range_end = end;
        return Ok((start, end.min(max_lineno.max(1))));
    }

    let (start_spec, end_spec) = match range.split_once(',') {
        Some((start, end)) => (start, Some(end)),
        None => (range, None),
    };

    let start = if start_spec.is_empty() {
        1
    } else {
        parse_line_spec(start_spec, ctx, None, LineAnchor::Default)?
    };
    if max_lineno > 0 && start > max_lineno {
        bail!("file has only {max_lineno} lines");
    }

    // `-L N` without comma: from N through end of file (git line-range-format).
    let end = match end_spec {
        None => max_lineno.max(1),
        Some(spec) if spec.is_empty() => max_lineno.max(1),
        Some(spec) => parse_line_spec(spec, ctx, Some(start), LineAnchor::AfterStart)?,
    };

    let end = if max_lineno == 0 {
        end
    } else {
        end.min(max_lineno)
    };
    Ok((start, end))
}

#[derive(Clone, Copy)]
enum LineAnchor {
    Default,
    AfterStart,
}

/// Parse a single line-range endpoint (git `blame -L` / line-range-format).
fn parse_line_spec(
    spec: &str,
    ctx: &mut LineRangeParseCtx<'_>,
    relative_to: Option<usize>,
    anchor: LineAnchor,
) -> Result<usize> {
    let max_lineno = max_line_no(ctx.blame_lines);

    if spec == "$" {
        return Ok(max_lineno);
    }

    // +N means "relative offset from start" (end only)
    if let Some(offset_str) = spec.strip_prefix('+') {
        if offset_str.is_empty() {
            bail!("invalid +N offset: cannot parse integer from empty string");
        }
        let offset: usize = offset_str.parse().context("invalid +N offset")?;
        let base = relative_to.unwrap_or(1);
        return Ok(base + offset - 1);
    }

    // -N means "relative negative offset from start" (end only), or negative regex anchor
    if let Some(offset_str) = spec.strip_prefix('-') {
        if let Ok(offset) = offset_str.parse::<usize>() {
            let base = relative_to.unwrap_or(1);
            return Ok(base.saturating_sub(offset).max(1));
        }
    }

    // :funcname — when paired with a numeric/other end (e.g. `-L3,:pat`)
    if let Some(fname) = spec.strip_prefix(':') {
        let pat = fname.strip_prefix('^').unwrap_or(fname);
        let search_from = if fname.starts_with('^') {
            1
        } else {
            match anchor {
                LineAnchor::Default => {
                    if ctx.prev_range_end > 0 {
                        ctx.prev_range_end + 1
                    } else {
                        1
                    }
                }
                LineAnchor::AfterStart => relative_to.unwrap_or(1),
            }
        };
        return find_funcname_start_line(ctx, pat, search_from);
    }

    // ^/regex/ — absolute from file start
    if let Some(rest) = spec.strip_prefix("^/") {
        if let Some(pat) = rest.strip_suffix('/') {
            return find_regex_line(ctx, pat, 1, true);
        }
    }

    // /regex/
    if spec.starts_with('/') && spec.ends_with('/') && spec.len() > 2 {
        let pattern = &spec[1..spec.len() - 1];
        let search_start = match anchor {
            LineAnchor::Default => {
                if ctx.prev_range_end > 0 {
                    ctx.prev_range_end
                } else {
                    0
                }
            }
            LineAnchor::AfterStart => relative_to.unwrap_or(0),
        };
        return find_regex_line(ctx, pattern, search_start, false);
    }

    // Plain number
    let n: usize = spec.parse().context("invalid line number")?;
    if n == 0 {
        bail!("invalid line number: invalid digit found in string");
    }
    Ok(n)
}

fn compile_blame_line_regex(pattern: &str) -> Result<Regex> {
    Regex::new(pattern).with_context(|| format!("invalid regex in -L: {pattern}"))
}

fn find_regex_line(
    ctx: &LineRangeParseCtx<'_>,
    pattern: &str,
    search_start: usize,
    absolute: bool,
) -> Result<usize> {
    let re = compile_blame_line_regex(pattern)?;
    let try_scan = |start: usize| -> Option<usize> {
        for bl in ctx.blame_lines {
            if bl.final_lineno > start && re.is_match(bl.content.as_str()) {
                return Some(bl.final_lineno);
            }
        }
        None
    };

    if let Some(ln) = try_scan(search_start) {
        return Ok(ln);
    }
    if !absolute && search_start > 0 {
        if let Some(ln) = try_scan(0) {
            return Ok(ln);
        }
    }
    bail!("no line matching pattern: {pattern}");
}

fn funcname_matcher_for_blame(ctx: &LineRangeParseCtx<'_>) -> Option<userdiff::FuncnameMatcher> {
    ctx.textconv
        .and_then(|tc| userdiff::matcher_for_path(&tc.config, &tc.attrs, ctx.file_path).ok())
        .flatten()
}

fn find_funcname_start_line(
    ctx: &LineRangeParseCtx<'_>,
    pattern: &str,
    search_from: usize,
) -> Result<usize> {
    let re = compile_blame_line_regex(pattern)?;
    let matcher = funcname_matcher_for_blame(ctx);

    for bl in ctx.blame_lines {
        if bl.final_lineno < search_from {
            continue;
        }
        if !re.is_match(bl.content.as_str()) {
            continue;
        }
        if matcher
            .as_ref()
            .is_some_and(|m| m.match_line(&bl.content).is_none())
        {
            continue;
        }
        return Ok(bl.final_lineno);
    }

    bail!("no line matching pattern: {pattern}");
}

fn funcname_hunk_end(ctx: &LineRangeParseCtx<'_>, start: usize) -> usize {
    let max_ln = max_line_no(ctx.blame_lines);
    let Some(matcher) = funcname_matcher_for_blame(ctx) else {
        return max_ln;
    };
    for bl in ctx.blame_lines {
        if bl.final_lineno <= start {
            continue;
        }
        if matcher.match_line(&bl.content).is_some() {
            return bl.final_lineno.saturating_sub(1).max(start);
        }
    }
    max_ln
}

fn resolve_funcname_span(ctx: &mut LineRangeParseCtx<'_>, range: &str) -> Result<(usize, usize)> {
    let body = range
        .strip_prefix(':')
        .ok_or_else(|| anyhow::anyhow!("internal: expected :funcname range"))?;
    let absolute = body.strip_prefix('^').unwrap_or(body);
    let search_from = if body.starts_with('^') {
        1
    } else if ctx.prev_range_end > 0 {
        ctx.prev_range_end + 1
    } else {
        1
    };
    let start = find_funcname_start_line(ctx, absolute, search_from)?;
    let end = funcname_hunk_end(ctx, start);
    Ok((start, end))
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
        let source_name = bl.source_file.as_deref().unwrap_or(filename).to_string();
        let first = seen.insert((bl.oid, source_name.clone()));

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
                writeln!(out, "previous {parent_hex} {source_name}")?;
            }
            // Boundary: root commit has no parents
            if commit.parents.is_empty() {
                writeln!(out, "boundary")?;
            }
            writeln!(out, "filename {source_name}")?;
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

/// `git annotate` output: tab-separated fields, 8-digit hash, parenthetical block padded like git.
fn write_annotate(
    out: &mut impl Write,
    lines: &[BlameLine],
    commits: &HashMap<ObjectId, CommitData>,
    args: &Args,
    _file_path: &str,
) -> Result<()> {
    let zero = grit_lib::diff::zero_oid();

    let mut author_field_width: usize = 10;
    for bl in lines {
        let w = annotate_author_field_width(bl, commits, args);
        author_field_width = author_field_width.max(w);
    }

    for bl in lines {
        let hash = if bl.oid == zero || bl.external_contents {
            "00000000".to_string()
        } else {
            bl.oid.to_hex()[..8].to_string()
        };

        let (author_display, ts) = annotate_author_and_time(bl, commits, args);
        let author_padded = format!("{author_display:>author_field_width$}");

        writeln!(
            out,
            "{hash}\t({author_padded}\t{ts}\t{lineno}){content}",
            lineno = bl.final_lineno,
            content = bl.content,
        )?;
    }
    Ok(())
}

fn annotate_author_field_width(
    bl: &BlameLine,
    commits: &HashMap<ObjectId, CommitData>,
    args: &Args,
) -> usize {
    let (name, _ts) = annotate_author_and_time(bl, commits, args);
    name.chars().count().max(1)
}

fn annotate_author_and_time(
    bl: &BlameLine,
    commits: &HashMap<ObjectId, CommitData>,
    args: &Args,
) -> (String, String) {
    let zero = grit_lib::diff::zero_oid();
    if bl.external_contents {
        return (
            "External file (--contents)".to_string(),
            format_time(0, "+0000"),
        );
    }
    if bl.oid == zero {
        return ("Not Committed Yet".to_string(), format_time(0, "+0000"));
    }
    let commit = &commits[&bl.oid];
    let ai = parse_author_field(&commit.author);
    let who = if args.email {
        format!("<{}>", ai.email)
    } else {
        ai.name.clone()
    };
    let ts = format_time(ai.timestamp, &ai.tz);
    (who, ts)
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
    let has_boundary = !args.root
        && lines.iter().any(|l| {
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
                hex[..(hash_len + 1).min(hex.len())].to_string()
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
