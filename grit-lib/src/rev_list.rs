//! Commit traversal and output planning for `rev-list`.
//!
//! This module implements a focused `rev-list` subset used by the v2 test
//! wave: revision ranges, `--all`, `--stdin` argument ingestion, commit walk
//! limits, ordering (`--topo-order`, `--date-order`, `--reverse`), and basic
//! output shaping (`--count`, `--parents`, `--format`).

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet, VecDeque};
use std::fs;
use std::path::Path;

use crate::error::{Error, Result};
use crate::objects::{parse_commit, parse_tag, parse_tree, ObjectId, ObjectKind};
use crate::refs;
use crate::repo::Repository;
use crate::rev_parse::resolve_revision;

/// User-facing output mode for `rev-list`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputMode {
    /// Print only object IDs.
    OidOnly,
    /// Print object ID followed by all parent IDs.
    Parents,
    /// Print a custom `%` placeholder format.
    Format(String),
}

/// Object filter specification for `--filter=`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectFilter {
    /// `blob:none` — omit all blobs.
    BlobNone,
    /// `blob:limit=<n>` — omit blobs larger than `n` bytes.
    BlobLimit(u64),
    /// `tree:<depth>` — omit trees deeper than `depth`.
    TreeDepth(u64),
    /// `combine:<filter>+<filter>+…` — apply multiple filters.
    Combine(Vec<ObjectFilter>),
}

impl ObjectFilter {
    /// Parse a `--filter=<spec>` value.
    pub fn parse(spec: &str) -> std::result::Result<Self, String> {
        if spec == "blob:none" {
            return Ok(ObjectFilter::BlobNone);
        }
        if let Some(rest) = spec.strip_prefix("blob:limit=") {
            let bytes = parse_size_suffix(rest)
                .ok_or_else(|| format!("invalid blob:limit value: {rest}"))?;
            return Ok(ObjectFilter::BlobLimit(bytes));
        }
        if let Some(rest) = spec.strip_prefix("tree:") {
            let depth: u64 = rest
                .parse()
                .map_err(|_| format!("invalid tree depth: {rest}"))?;
            return Ok(ObjectFilter::TreeDepth(depth));
        }
        if let Some(rest) = spec.strip_prefix("combine:") {
            let parts = split_combine(rest);
            let mut filters = Vec::new();
            for part in parts {
                filters.push(ObjectFilter::parse(&part)?);
            }
            return Ok(ObjectFilter::Combine(filters));
        }
        Err(format!("unsupported filter spec: {spec}"))
    }

    /// Check if a blob should be included given its size.
    pub fn includes_blob(&self, size: u64) -> bool {
        match self {
            ObjectFilter::BlobNone => false,
            ObjectFilter::BlobLimit(limit) => size <= *limit,
            ObjectFilter::TreeDepth(_) => true,
            ObjectFilter::Combine(filters) => {
                filters.iter().all(|f| f.includes_blob(size))
            }
        }
    }

    /// Check if a tree at given depth should be included.
    pub fn includes_tree(&self, depth: u64) -> bool {
        match self {
            ObjectFilter::BlobNone => true,
            ObjectFilter::BlobLimit(_) => true,
            ObjectFilter::TreeDepth(max_depth) => depth < *max_depth,
            ObjectFilter::Combine(filters) => {
                filters.iter().all(|f| f.includes_tree(depth))
            }
        }
    }
}

/// Parse a size with optional k/m/g suffix.
fn parse_size_suffix(s: &str) -> Option<u64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let (num_str, multiplier) = match s.as_bytes().last()? {
        b'k' | b'K' => (&s[..s.len() - 1], 1024u64),
        b'm' | b'M' => (&s[..s.len() - 1], 1024 * 1024),
        b'g' | b'G' => (&s[..s.len() - 1], 1024 * 1024 * 1024),
        _ => (s, 1u64),
    };
    let num: u64 = num_str.parse().ok()?;
    Some(num * multiplier)
}

/// Split a combine filter spec on `+`, handling URL encoding.
fn split_combine(spec: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut chars = spec.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '+' {
            if !current.is_empty() {
                parts.push(url_decode(&current));
                current.clear();
            }
        } else {
            current.push(ch);
        }
    }
    if !current.is_empty() {
        parts.push(url_decode(&current));
    }
    parts
}

/// Simple URL percent-decoding.
fn url_decode(s: &str) -> String {
    let mut result = String::new();
    let mut chars = s.chars();
    while let Some(ch) = chars.next() {
        if ch == '%' {
            let hi = chars.next().unwrap_or('0');
            let lo = chars.next().unwrap_or('0');
            let byte = u8::from_str_radix(&format!("{hi}{lo}"), 16).unwrap_or(b'?');
            result.push(byte as char);
        } else {
            result.push(ch);
        }
    }
    result
}

/// Ordering mode for commit output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderingMode {
    /// Reverse-chronological by commit date.
    Default,
    /// Topological ordering with date tie-breaks.
    Topo,
    /// Date-order variant (same constraints as topo for this subset).
    Date,
}

/// Parsed and normalized options for rev-list traversal.
#[derive(Debug, Clone)]
pub struct RevListOptions {
    /// Include all refs (`--all`) as positive tips.
    pub all_refs: bool,
    /// Follow only first parent when walking merges.
    pub first_parent: bool,
    /// Enable ancestry-path filtering.
    pub ancestry_path: bool,
    /// Optional explicit ancestry-path pivot commits.
    pub ancestry_path_bottoms: Vec<ObjectId>,
    /// Keep only decorated commits after traversal.
    pub simplify_by_decoration: bool,
    /// Commit output mode.
    pub output_mode: OutputMode,
    /// Suppress commit output.
    pub quiet: bool,
    /// Print only final count.
    pub count: bool,
    /// Skip N commits from selected list.
    pub skip: usize,
    /// Optional maximum selected commits.
    pub max_count: Option<usize>,
    /// Ordering strategy.
    pub ordering: OrderingMode,
    /// Reverse selected output order.
    pub reverse: bool,
    /// List reachable objects (trees, blobs) in addition to commits.
    pub objects: bool,
    /// Suppress object path names in --objects output.
    pub no_object_names: bool,
    /// Show boundary commits with `-` prefix.
    pub boundary: bool,
    /// Show left/right markers for symmetric diff.
    pub left_right: bool,
    /// Filter to left-only commits in symmetric diff.
    pub left_only: bool,
    /// Filter to right-only commits in symmetric diff.
    pub right_only: bool,
    /// Cherry-mark equivalent commits with `=` instead of `+`.
    pub cherry_mark: bool,
    /// Cherry-pick: omit equivalent commits from output.
    pub cherry_pick: bool,
    /// Minimum number of parents a commit must have to be included.
    pub min_parents: Option<usize>,
    /// Maximum number of parents a commit may have to be included.
    pub max_parents: Option<usize>,
    /// Symmetric-diff left OID (set by caller when A...B is used).
    pub symmetric_left: Option<ObjectId>,
    /// Symmetric-diff right OID (set by caller when A...B is used).
    pub symmetric_right: Option<ObjectId>,
    /// Path filters (files after `--`).
    pub paths: Vec<String>,
    /// Show full history (don't simplify) for path-limited walks.
    pub full_history: bool,
    /// Sparse mode: don't prune non-matching commits.
    pub sparse: bool,
    /// Object filter for `--filter=<spec>`.
    pub filter: Option<ObjectFilter>,
    /// Print omitted objects prefixed with `~`.
    pub filter_print_omitted: bool,
    /// Emit objects interleaved with their introducing commit.
    pub in_commit_order: bool,
    /// Exclude objects in `.keep` pack files.
    pub no_kept_objects: bool,
}

impl Default for RevListOptions {
    fn default() -> Self {
        Self {
            all_refs: false,
            first_parent: false,
            ancestry_path: false,
            ancestry_path_bottoms: Vec::new(),
            simplify_by_decoration: false,
            output_mode: OutputMode::OidOnly,
            quiet: false,
            count: false,
            skip: 0,
            max_count: None,
            ordering: OrderingMode::Default,
            reverse: false,
            objects: false,
            no_object_names: false,
            boundary: false,
            left_right: false,
            left_only: false,
            right_only: false,
            cherry_mark: false,
            cherry_pick: false,
            min_parents: None,
            max_parents: None,
            symmetric_left: None,
            symmetric_right: None,
            paths: Vec::new(),
            full_history: false,
            sparse: false,
            filter: None,
            filter_print_omitted: false,
            in_commit_order: false,
            no_kept_objects: false,
        }
    }
}

/// Final commit selection result.
#[derive(Debug, Clone)]
pub struct RevListResult {
    /// Selected commits in final output order, after skip/max/reverse.
    pub commits: Vec<ObjectId>,
    /// Reachable non-commit objects when `--objects` is active.
    /// Each entry is `(oid, optional_path)`.
    pub objects: Vec<(ObjectId, String)>,
    /// Objects omitted by `--filter` (for `--filter-print-omitted`).
    pub omitted_objects: Vec<ObjectId>,
    /// Boundary commits (excluded parents shown with `-` prefix).
    pub boundary_commits: Vec<ObjectId>,
    /// For `--left-right`: mapping commit OID -> true=left, false=right.
    pub left_right_map: HashMap<ObjectId, bool>,
    /// For `--cherry-mark`: set of commits that are equivalent (patch-id match).
    pub cherry_equivalent: HashSet<ObjectId>,
    /// Per-commit object counts (parallel to `commits`) for `--in-commit-order`.
    /// When non-empty, `objects[sum(counts[..i])..sum(counts[..=i])]` are the objects
    /// introduced by `commits[i]`.
    pub per_commit_object_counts: Vec<usize>,
}

/// Resolve and walk revisions for the requested options.
///
/// # Parameters
///
/// - `repo` - repository used for ref/object lookup.
/// - `positive_specs` - positive revision tokens (e.g. `HEAD`, `A..B` rhs).
/// - `negative_specs` - negative revision tokens (`^A`, `A..B` lhs).
/// - `options` - traversal and output selection options.
///
/// # Errors
///
/// Returns [`Error::ObjectNotFound`] / [`Error::InvalidRef`] for bad revision
/// specs and [`Error::CorruptObject`] for non-commit or malformed commit data.
pub fn rev_list(
    repo: &Repository,
    positive_specs: &[String],
    negative_specs: &[String],
    options: &RevListOptions,
) -> Result<RevListResult> {
    let mut graph = CommitGraph::new(repo, options.first_parent);

    let mut include = resolve_specs(repo, positive_specs)?;
    let exclude = resolve_specs(repo, negative_specs)?;

    if options.all_refs {
        include.extend(all_ref_tips(repo)?);
    }

    if include.is_empty() {
        return Err(Error::InvalidRef("no revisions specified".to_owned()));
    }

    let (mut included, discovery_order) = walk_closure_ordered(&mut graph, &include)?;
    let excluded = walk_closure(&mut graph, &exclude)?;
    included.retain(|oid| !excluded.contains(oid));

    if options.simplify_by_decoration {
        let decorated = all_ref_tips(repo)?;
        included.retain(|oid| decorated.contains(oid));
    }

    if options.ancestry_path {
        let mut bottoms = options.ancestry_path_bottoms.clone();
        if bottoms.is_empty() {
            bottoms.extend(exclude.iter().copied());
        }
        if bottoms.is_empty() {
            return Err(Error::InvalidRef(
                "--ancestry-path requires a range with excluded tips".to_owned(),
            ));
        }
        limit_to_ancestry(&mut graph, &mut included, &bottoms)?;
    }

    // Filter by parent count (--merges, --no-merges, --min-parents, --max-parents)
    if options.min_parents.is_some() || options.max_parents.is_some() {
        let min_p = options.min_parents.unwrap_or(0);
        let max_p = options.max_parents.unwrap_or(usize::MAX);
        included.retain(|oid| {
            let count = graph.parents_of(*oid).map(|p| p.len()).unwrap_or(0);
            count >= min_p && count <= max_p
        });
    }

    let mut ordered = match options.ordering {
        OrderingMode::Default => sort_by_commit_date_desc(&mut graph, &included, &discovery_order)?,
        OrderingMode::Topo | OrderingMode::Date => topo_sort(&mut graph, &included)?,
    };

    // Path filtering: keep only commits that modify given paths
    if !options.paths.is_empty() && !options.sparse {
        let paths = &options.paths;
        ordered.retain(|oid| {
            commit_touches_paths(repo, &mut graph, *oid, paths).unwrap_or(false)
        });
    }

    // Left-right classification for symmetric diffs
    let mut left_right_map = HashMap::new();
    if options.left_right || options.left_only || options.right_only || options.cherry_mark || options.cherry_pick {
        if let (Some(left_oid), Some(right_oid)) = (options.symmetric_left, options.symmetric_right) {
            let left_closure = walk_closure(&mut graph, &[left_oid])?;
            let right_closure = walk_closure(&mut graph, &[right_oid])?;
            for &oid in &ordered {
                let in_left = left_closure.contains(&oid);
                let in_right = right_closure.contains(&oid);
                if in_left && !in_right {
                    left_right_map.insert(oid, true);
                } else if in_right && !in_left {
                    left_right_map.insert(oid, false);
                } else {
                    left_right_map.insert(oid, false);
                }
            }
        }
    }

    // Cherry-pick / cherry-mark: compute patch-ids and find equivalents
    let mut cherry_equivalent = HashSet::new();
    if options.cherry_pick || options.cherry_mark {
        let patch_ids = compute_patch_ids(repo, &mut graph, &ordered)?;
        let left_commits: Vec<_> = ordered.iter().filter(|o| left_right_map.get(o) == Some(&true)).copied().collect();
        let right_commits: Vec<_> = ordered.iter().filter(|o| left_right_map.get(o) == Some(&false)).copied().collect();
        let left_patches: HashMap<&str, ObjectId> = left_commits.iter()
            .filter_map(|o| patch_ids.get(o).map(|p| (p.as_str(), *o)))
            .collect();
        let right_patches: HashMap<&str, ObjectId> = right_commits.iter()
            .filter_map(|o| patch_ids.get(o).map(|p| (p.as_str(), *o)))
            .collect();
        for (&pid, &oid) in &left_patches {
            if !pid.is_empty() && right_patches.contains_key(pid) {
                cherry_equivalent.insert(oid);
                cherry_equivalent.insert(right_patches[pid]);
            }
        }
        for (&pid, &oid) in &right_patches {
            if !pid.is_empty() && left_patches.contains_key(pid) {
                cherry_equivalent.insert(oid);
                cherry_equivalent.insert(left_patches[pid]);
            }
        }
    }

    // Filter left-only / right-only
    if options.left_only {
        ordered.retain(|oid| left_right_map.get(oid) == Some(&true));
    }
    if options.right_only {
        ordered.retain(|oid| left_right_map.get(oid) == Some(&false));
    }

    // Cherry-pick: remove equivalent commits
    if options.cherry_pick {
        ordered.retain(|oid| !cherry_equivalent.contains(oid));
    }

    if options.skip > 0 {
        ordered = ordered.into_iter().skip(options.skip).collect();
    }
    if let Some(max_count) = options.max_count {
        ordered.truncate(max_count);
    }
    if options.reverse {
        ordered.reverse();
    }

    // Collect boundary commits: parents of included commits that are in the excluded set
    let boundary_commits = if options.boundary {
        let included_set: HashSet<ObjectId> = ordered.iter().copied().collect();
        let mut boundary = Vec::new();
        let mut boundary_seen = HashSet::new();
        for &oid in &ordered {
            if let Ok(parents) = graph.parents_of(oid).map(|p| p.to_vec()) {
                for parent in parents {
                    if !included_set.contains(&parent) && boundary_seen.insert(parent) {
                        boundary.push(parent);
                    }
                }
            }
        }
        boundary
    } else {
        Vec::new()
    };

    // Filter kept objects when --no-kept-objects is set
    let kept_set = if options.no_kept_objects {
        kept_object_ids(repo).unwrap_or_default()
    } else {
        HashSet::new()
    };

    if options.no_kept_objects {
        ordered.retain(|oid| !kept_set.contains(oid));
    }

    // Collect reachable objects if --objects
    let (objects, omitted_objects, per_commit_object_counts) = if options.objects {
        let (mut objs, omit, counts) = if options.in_commit_order {
            collect_reachable_objects_in_commit_order(repo, &mut graph, &ordered, options.filter.as_ref())?
        } else {
            let (objs, omit) = collect_reachable_objects(repo, &mut graph, &ordered, options.filter.as_ref())?;
            (objs, omit, Vec::new())
        };
        if options.no_kept_objects {
            objs.retain(|(oid, _)| !kept_set.contains(oid));
        }
        (objs, omit, counts)
    } else {
        (Vec::new(), Vec::new(), Vec::new())
    };

    Ok(RevListResult {
        commits: ordered,
        objects,
        omitted_objects,
        boundary_commits,
        left_right_map,
        cherry_equivalent,
        per_commit_object_counts,
    })
}

/// Parse a raw revision token into positive and negative specs.
///
/// Supports:
/// - `<a>..<b>` => negative `<a>`, positive `<b>`
/// - `^<rev>` => negative `<rev>`
/// - `<rev>` => positive `<rev>`
#[must_use]
pub fn split_revision_token(token: &str) -> (Vec<String>, Vec<String>) {
    if let Some((lhs, rhs)) = token.split_once("..") {
        let positive = if rhs.is_empty() { "HEAD".to_owned() } else { rhs.to_owned() };
        let negative = if lhs.is_empty() { "HEAD".to_owned() } else { lhs.to_owned() };
        return (vec![positive], vec![negative]);
    }
    if let Some(rest) = token.strip_prefix('^') {
        return (Vec::new(), vec![rest.to_owned()]);
    }
    (vec![token.to_owned()], Vec::new())
}

fn ansi_color_from_name(name: &str) -> String {
    match name {
        "red" => "\x1b[31m".to_owned(),
        "green" => "\x1b[32m".to_owned(),
        "yellow" => "\x1b[33m".to_owned(),
        "blue" => "\x1b[34m".to_owned(),
        "magenta" => "\x1b[35m".to_owned(),
        "cyan" => "\x1b[36m".to_owned(),
        "white" => "\x1b[37m".to_owned(),
        "bold" => "\x1b[1m".to_owned(),
        "dim" => "\x1b[2m".to_owned(),
        "ul" | "underline" => "\x1b[4m".to_owned(),
        "blink" => "\x1b[5m".to_owned(),
        "reverse" => "\x1b[7m".to_owned(),
        "reset" => "\x1b[m".to_owned(),
        _ => String::new(),
    }
}

fn color_name_to_code(name: &str) -> Option<u8> {
    match name {
        "black" => Some(0), "red" => Some(1), "green" => Some(2),
        "yellow" => Some(3), "blue" => Some(4), "magenta" => Some(5),
        "cyan" => Some(6), "white" => Some(7), "default" => Some(9),
        _ => None,
    }
}

fn ansi_color_from_spec(spec: &str) -> String {
    if spec == "reset" {
        return "\x1b[m".to_owned();
    }
    let mut codes = Vec::new();
    let mut fg_set = false;
    for part in spec.split_whitespace() {
        match part {
            "bold" => codes.push("1".to_owned()),
            "dim" => codes.push("2".to_owned()),
            "italic" => codes.push("3".to_owned()),
            "ul" | "underline" => codes.push("4".to_owned()),
            "blink" => codes.push("5".to_owned()),
            "reverse" => codes.push("7".to_owned()),
            "strike" => codes.push("9".to_owned()),
            "nobold" | "nodim" => codes.push("22".to_owned()),
            "noitalic" => codes.push("23".to_owned()),
            "noul" | "nounderline" => codes.push("24".to_owned()),
            "noblink" => codes.push("25".to_owned()),
            "noreverse" => codes.push("27".to_owned()),
            "nostrike" => codes.push("29".to_owned()),
            _ => {
                if let Some(code) = color_name_to_code(part) {
                    if !fg_set {
                        codes.push(format!("{}", 30 + code));
                        fg_set = true;
                    } else {
                        codes.push(format!("{}", 40 + code));
                    }
                }
            }
        }
    }
    if codes.is_empty() {
        String::new()
    } else {
        format!("\x1b[{}m", codes.join(";"))
    }
}

fn format_relative_date(diff: i64) -> String {
    if diff < 0 {
        "in the future".to_owned()
    } else if diff < 60 {
        format!("{} seconds ago", diff)
    } else if diff < 3600 {
        let m = diff / 60;
        if m == 1 { "1 minute ago".to_owned() } else { format!("{m} minutes ago") }
    } else if diff < 86400 {
        let h = diff / 3600;
        if h == 1 { "1 hour ago".to_owned() } else { format!("{h} hours ago") }
    } else if diff < 86400 * 30 {
        let d = diff / 86400;
        if d == 1 { "1 day ago".to_owned() } else { format!("{d} days ago") }
    } else if diff < 86400 * 365 {
        let months = diff / (86400 * 30);
        if months == 1 { "1 month ago".to_owned() } else { format!("{months} months ago") }
    } else {
        let years = diff / (86400 * 365);
        if years == 1 { "1 year ago".to_owned() } else { format!("{years} years ago") }
    }
}

/// Render one commit according to the selected output mode.
///
/// # Errors
///
/// Returns object decode errors when commit metadata is required.
pub fn render_commit(
    repo: &Repository,
    oid: ObjectId,
    mode: &OutputMode,
    abbrev_len: usize,
) -> Result<String> {
    render_commit_with_color(repo, oid, mode, abbrev_len, false)
}

/// Render one commit, optionally with ANSI color for `%C` placeholders.
pub fn render_commit_with_color(
    repo: &Repository,
    oid: ObjectId,
    mode: &OutputMode,
    abbrev_len: usize,
    use_color: bool,
) -> Result<String> {
    match mode {
        OutputMode::OidOnly => Ok(format!("{oid}")),
        OutputMode::Parents => {
            let mut out = format!("{oid}");
            let commit = load_commit(repo, oid)?;
            for parent in commit.parents {
                out.push(' ');
                out.push_str(&parent.to_hex());
            }
            Ok(out)
        }
        OutputMode::Format(fmt) => {
            let commit = load_commit(repo, oid)?;
            let subject = commit.message.lines().next().unwrap_or_default();
            let hex = oid.to_hex();

            // Handle named pretty formats
            match fmt.as_str() {
                "oneline" => {
                    return Ok(format!("{} {}", hex, subject));
                }
                "short" => {
                    fn fmt_ident(ident: &str) -> String {
                        let name = if let Some(bracket) = ident.find('<') {
                            ident[..bracket].trim()
                        } else {
                            ident.trim()
                        };
                        let email = if let Some(start) = ident.find('<') {
                            if let Some(end) = ident.find('>') {
                                &ident[start..=end]
                            } else { "" }
                        } else { "" };
                        format!("{} {}", name, email)
                    }
                    let mut out = String::new();
                    out.push_str(&format!("Author: {}\n", fmt_ident(&commit.author)));
                    out.push('\n');
                    out.push_str(&format!("    {}\n", subject));
                    out.push('\n');
                    return Ok(out);
                }
                "medium" => {
                    fn extract_ident_display(ident: &str) -> String {
                        let name = if let Some(bracket) = ident.find('<') {
                            ident[..bracket].trim()
                        } else {
                            ident.trim()
                        };
                        let email = if let Some(start) = ident.find('<') {
                            if let Some(end) = ident.find('>') {
                                &ident[start..=end]
                            } else { "" }
                        } else { "" };
                        format!("{} {}", name, email)
                    }
                    fn format_default_date(ident: &str) -> String {
                        let parts: Vec<&str> = ident.rsplitn(3, ' ').collect();
                        if parts.len() < 2 { return String::new(); }
                        let ts_str = parts[1];
                        let offset_str = parts[0];
                        let ts: i64 = match ts_str.parse() { Ok(v) => v, Err(_) => return format!("{ts_str} {offset_str}") };
                        let tz_bytes = offset_str.as_bytes();
                        let tz_secs: i64 = if tz_bytes.len() >= 5 {
                            let sign = if tz_bytes[0] == b'-' { -1i64 } else { 1i64 };
                            let h: i64 = offset_str[1..3].parse().unwrap_or(0);
                            let m: i64 = offset_str[3..5].parse().unwrap_or(0);
                            sign * (h * 3600 + m * 60)
                        } else { 0 };
                        let adjusted = ts + tz_secs;
                        let dt = time::OffsetDateTime::from_unix_timestamp(adjusted)
                            .unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
                        let weekday = match dt.weekday() {
                            time::Weekday::Monday => "Mon", time::Weekday::Tuesday => "Tue",
                            time::Weekday::Wednesday => "Wed", time::Weekday::Thursday => "Thu",
                            time::Weekday::Friday => "Fri", time::Weekday::Saturday => "Sat",
                            time::Weekday::Sunday => "Sun",
                        };
                        let month = match dt.month() {
                            time::Month::January => "Jan", time::Month::February => "Feb",
                            time::Month::March => "Mar", time::Month::April => "Apr",
                            time::Month::May => "May", time::Month::June => "Jun",
                            time::Month::July => "Jul", time::Month::August => "Aug",
                            time::Month::September => "Sep", time::Month::October => "Oct",
                            time::Month::November => "Nov", time::Month::December => "Dec",
                        };
                        format!("{} {} {} {:02}:{:02}:{:02} {} {}",
                            weekday, month, dt.day(),
                            dt.hour(), dt.minute(), dt.second(),
                            dt.year(), offset_str)
                    }
                    let mut out = String::new();
                    out.push_str(&format!("Author: {}\n", extract_ident_display(&commit.author)));
                    out.push_str(&format!("Date:   {}\n", format_default_date(&commit.author)));
                    out.push('\n');
                    for line in commit.message.lines() {
                        out.push_str(&format!("    {}\n", line));
                    }
                    return Ok(out);
                }
                _ => {}
            }

            let raw_fmt = if let Some(t) = fmt.strip_prefix("format:") {
                t
            } else if let Some(t) = fmt.strip_prefix("tformat:") {
                t
            } else {
                fmt.as_str()
            };
            // Body: everything after the first line (skip blank separator line)
            let body = {
                let mut lines = commit.message.lines();
                lines.next(); // skip subject
                // Skip optional blank line after subject
                if let Some(blank) = lines.next() {
                    if blank.is_empty() {
                        lines.collect::<Vec<_>>().join("\n")
                    } else {
                        std::iter::once(blank).chain(lines).collect::<Vec<_>>().join("\n")
                    }
                } else {
                    String::new()
                }
            };
            let tree_hex = commit.tree.to_hex();
            let parent_hexes: Vec<String> = commit.parents.iter().map(|p| p.to_hex()).collect();
            let parent_abbrevs: Vec<String> = commit.parents.iter().map(|p| {
                let hex = p.to_hex();
                let n = abbrev_len.clamp(4, 40).min(hex.len());
                hex[..n].to_string()
            }).collect();

            // Extract name/email components from ident strings
            fn extract_name(ident: &str) -> &str {
                if let Some(bracket) = ident.find('<') {
                    ident[..bracket].trim()
                } else {
                    ident.trim()
                }
            }
            fn extract_email(ident: &str) -> &str {
                if let Some(start) = ident.find('<') {
                    if let Some(end) = ident.find('>') {
                        return &ident[start + 1..end];
                    }
                }
                ""
            }
            fn extract_timestamp(ident: &str) -> &str {
                let parts: Vec<&str> = ident.rsplitn(3, ' ').collect();
                if parts.len() >= 2 { parts[1] } else { "" }
            }
            fn parse_ident_date(ident: &str) -> Option<(i64, &str)> {
                let parts: Vec<&str> = ident.rsplitn(3, ' ').collect();
                if parts.len() < 2 { return None; }
                let ts: i64 = parts[1].parse().ok()?;
                Some((ts, parts[0]))
            }
            fn parse_tz(offset_str: &str) -> i64 {
                let tz_bytes = offset_str.as_bytes();
                if tz_bytes.len() >= 5 {
                    let sign = if tz_bytes[0] == b'-' { -1i64 } else { 1i64 };
                    let h: i64 = offset_str[1..3].parse().unwrap_or(0);
                    let m: i64 = offset_str[3..5].parse().unwrap_or(0);
                    sign * (h * 3600 + m * 60)
                } else { 0 }
            }
            fn weekday_str(dt: &time::OffsetDateTime) -> &'static str {
                match dt.weekday() {
                    time::Weekday::Monday => "Mon", time::Weekday::Tuesday => "Tue",
                    time::Weekday::Wednesday => "Wed", time::Weekday::Thursday => "Thu",
                    time::Weekday::Friday => "Fri", time::Weekday::Saturday => "Sat",
                    time::Weekday::Sunday => "Sun",
                }
            }
            fn month_str(dt: &time::OffsetDateTime) -> &'static str {
                match dt.month() {
                    time::Month::January => "Jan", time::Month::February => "Feb",
                    time::Month::March => "Mar", time::Month::April => "Apr",
                    time::Month::May => "May", time::Month::June => "Jun",
                    time::Month::July => "Jul", time::Month::August => "Aug",
                    time::Month::September => "Sep", time::Month::October => "Oct",
                    time::Month::November => "Nov", time::Month::December => "Dec",
                }
            }
            fn extract_email_local(ident: &str) -> &str {
                let email = extract_email(ident);
                if let Some(at) = email.find('@') {
                    &email[..at]
                } else {
                    email
                }
            }
            fn extract_date_default(ident: &str) -> String {
                let Some((ts, offset_str)) = parse_ident_date(ident) else { return String::new() };
                let adjusted = ts + parse_tz(offset_str);
                let dt = time::OffsetDateTime::from_unix_timestamp(adjusted)
                    .unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
                format!("{} {} {} {:02}:{:02}:{:02} {} {}",
                    weekday_str(&dt), month_str(&dt), dt.day(),
                    dt.hour(), dt.minute(), dt.second(),
                    dt.year(), offset_str)
            }
            fn extract_date_rfc2822(ident: &str) -> String {
                let Some((ts, offset_str)) = parse_ident_date(ident) else { return String::new() };
                let adjusted = ts + parse_tz(offset_str);
                let dt = time::OffsetDateTime::from_unix_timestamp(adjusted)
                    .unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
                format!("{}, {} {} {} {:02}:{:02}:{:02} {}",
                    weekday_str(&dt), dt.day(), month_str(&dt), dt.year(),
                    dt.hour(), dt.minute(), dt.second(), offset_str)
            }
            fn extract_date_short(ident: &str) -> String {
                let Some((ts, offset_str)) = parse_ident_date(ident) else { return String::new() };
                let adjusted = ts + parse_tz(offset_str);
                let dt = time::OffsetDateTime::from_unix_timestamp(adjusted)
                    .unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
                format!("{:04}-{:02}-{:02}", dt.year(), dt.month() as u8, dt.day())
            }
            fn extract_date_iso(ident: &str) -> String {
                let Some((ts, offset_str)) = parse_ident_date(ident) else { return String::new() };
                let adjusted = ts + parse_tz(offset_str);
                let dt = time::OffsetDateTime::from_unix_timestamp(adjusted)
                    .unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
                format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02} {}",
                    dt.year(), dt.month() as u8, dt.day(),
                    dt.hour(), dt.minute(), dt.second(), offset_str)
            }

            // Alignment/truncation state for %<(N), %>(N), %><(N) directives
            #[derive(Clone, Copy)]
            enum Align { Left, Right, Center }
            #[derive(Clone, Copy)]
            enum Trunc { None, Trunc, LTrunc, MTrunc }
            struct ColSpec { width: usize, align: Align, trunc: Trunc }
            fn apply_col(spec: &ColSpec, s: &str) -> String {
                let char_len = s.chars().count();
                if char_len > spec.width {
                    match spec.trunc {
                        Trunc::None => s.to_owned(),
                        Trunc::Trunc => {
                            let mut out: String = s.chars().take(spec.width.saturating_sub(2)).collect();
                            out.push_str("..");
                            out
                        }
                        Trunc::LTrunc => {
                            let skip = char_len - spec.width + 2;
                            let mut out = String::from("..");
                            out.extend(s.chars().skip(skip));
                            out
                        }
                        Trunc::MTrunc => {
                            let keep = spec.width.saturating_sub(2);
                            let left_half = keep / 2;
                            let right_half = keep - left_half;
                            let mut out: String = s.chars().take(left_half).collect();
                            out.push_str("..");
                            out.extend(s.chars().skip(char_len - right_half));
                            out
                        }
                    }
                } else {
                    let pad = spec.width - char_len;
                    match spec.align {
                        Align::Left => {
                            let mut out = s.to_owned();
                            for _ in 0..pad { out.push(' '); }
                            out
                        }
                        Align::Right => {
                            let mut out = String::new();
                            for _ in 0..pad { out.push(' '); }
                            out.push_str(s);
                            out
                        }
                        Align::Center => {
                            let left = pad / 2;
                            let right = pad - left;
                            let mut out = String::new();
                            for _ in 0..left { out.push(' '); }
                            out.push_str(s);
                            for _ in 0..right { out.push(' '); }
                            out
                        }
                    }
                }
            }
            fn parse_col_spec(chars: &mut std::iter::Peekable<std::str::Chars<'_>>, align: Align) -> Option<ColSpec> {
                // Consume '('
                if chars.peek() != Some(&'(') { return None; }
                chars.next();
                let mut num_str = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_ascii_digit() { num_str.push(c); chars.next(); } else { break; }
                }
                let width: usize = num_str.parse().ok()?;
                let trunc = if chars.peek() == Some(&',') {
                    chars.next(); // consume comma
                    let mut mode = String::new();
                    while let Some(&c) = chars.peek() {
                        if c == ')' { break; }
                        mode.push(c); chars.next();
                    }
                    match mode.as_str() {
                        "trunc" => Trunc::Trunc,
                        "ltrunc" => Trunc::LTrunc,
                        "mtrunc" => Trunc::MTrunc,
                        _ => Trunc::None,
                    }
                } else {
                    Trunc::None
                };
                // Consume ')'
                if chars.peek() == Some(&')') { chars.next(); }
                Some(ColSpec { width, align, trunc })
            }

            let mut pending_col: Option<ColSpec> = None;
            let mut rendered = String::new();
            let mut chars = raw_fmt.chars().peekable();
            while let Some(ch) = chars.next() {
                if ch != '%' {
                    rendered.push(ch);
                    continue;
                }
                // Check for alignment directives: %<(...), %>(...), %><(...)
                if chars.peek() == Some(&'<') {
                    chars.next();
                    if let Some(spec) = parse_col_spec(&mut chars, Align::Left) {
                        pending_col = Some(spec);
                    }
                    continue;
                }
                if chars.peek() == Some(&'>') {
                    chars.next();
                    if chars.peek() == Some(&'<') {
                        chars.next(); // %><(...)
                        if let Some(spec) = parse_col_spec(&mut chars, Align::Center) {
                            pending_col = Some(spec);
                        }
                    } else if chars.peek() == Some(&'>') {
                        chars.next(); // %>>(...)
                        if let Some(spec) = parse_col_spec(&mut chars, Align::Right) {
                            pending_col = Some(spec);
                        }
                    } else if let Some(spec) = parse_col_spec(&mut chars, Align::Right) {
                        pending_col = Some(spec);
                    }
                    continue;
                }

                // Helper macro-like: expand the placeholder, then apply pending_col
                let mut expanded = String::new();
                let target = if pending_col.is_some() { &mut expanded } else { &mut rendered };
                match chars.peek() {
                    Some('%') => { chars.next(); target.push('%'); }
                    Some('H') => { chars.next(); target.push_str(&oid.to_hex()); }
                    Some('h') => {
                        chars.next();
                        let hex = oid.to_hex();
                        let n = abbrev_len.clamp(4, 40).min(hex.len());
                        target.push_str(&hex[..n]);
                    }
                    Some('T') => { chars.next(); target.push_str(&tree_hex); }
                    Some('t') => {
                        chars.next();
                        let n = abbrev_len.clamp(4, 40).min(tree_hex.len());
                        target.push_str(&tree_hex[..n]);
                    }
                    Some('P') => { chars.next(); target.push_str(&parent_hexes.join(" ")); }
                    Some('p') => { chars.next(); target.push_str(&parent_abbrevs.join(" ")); }
                    Some('n') => { chars.next(); target.push('\n'); }
                    Some('s') => { chars.next(); target.push_str(subject); }
                    Some('b') => { chars.next(); target.push_str(&body); if !body.is_empty() { target.push('\n'); } }
                    Some('B') => { chars.next(); target.push_str(&commit.message); }
                    Some('a') => {
                        chars.next();
                        match chars.next() {
                            Some('n') => target.push_str(extract_name(&commit.author)),
                            Some('N') => target.push_str(extract_name(&commit.author)),
                            Some('e') => target.push_str(extract_email(&commit.author)),
                            Some('E') => target.push_str(extract_email(&commit.author)),
                            Some('l') => target.push_str(extract_email_local(&commit.author)),
                            Some('d') => target.push_str(&extract_date_default(&commit.author)),
                            Some('D') => target.push_str(&extract_date_rfc2822(&commit.author)),
                            Some('t') => target.push_str(extract_timestamp(&commit.author)),
                            Some('s') => target.push_str(&extract_date_short(&commit.author)),
                            Some('i') => target.push_str(&extract_date_iso(&commit.author)),
                            Some('I') => {
                                let Some((ts, offset_str)) = parse_ident_date(&commit.author) else { break };
                                let adjusted = ts + parse_tz(offset_str);
                                let dt = time::OffsetDateTime::from_unix_timestamp(adjusted)
                                    .unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
                                let sign_ch = if parse_tz(offset_str) >= 0 { '+' } else { '-' };
                                let abs_off = parse_tz(offset_str).unsigned_abs();
                                target.push_str(&format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}{}{:02}:{:02}",
                                    dt.year(), dt.month() as u8, dt.day(),
                                    dt.hour(), dt.minute(), dt.second(),
                                    sign_ch, abs_off / 3600, (abs_off % 3600) / 60));
                            }
                            Some('r') => {
                                let Some((ts, _)) = parse_ident_date(&commit.author) else { break };
                                let now = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default().as_secs() as i64;
                                target.push_str(&format_relative_date(now - ts));
                            }
                            Some(other) => { target.push('%'); target.push('a'); target.push(other); }
                            None => { target.push('%'); target.push('a'); }
                        }
                    }
                    Some('c') => {
                        chars.next();
                        match chars.next() {
                            Some('n') => target.push_str(extract_name(&commit.committer)),
                            Some('N') => target.push_str(extract_name(&commit.committer)),
                            Some('e') => target.push_str(extract_email(&commit.committer)),
                            Some('E') => target.push_str(extract_email(&commit.committer)),
                            Some('l') => target.push_str(extract_email_local(&commit.committer)),
                            Some('d') => target.push_str(&extract_date_default(&commit.committer)),
                            Some('D') => target.push_str(&extract_date_rfc2822(&commit.committer)),
                            Some('t') => target.push_str(extract_timestamp(&commit.committer)),
                            Some('s') => target.push_str(&extract_date_short(&commit.committer)),
                            Some('i') => target.push_str(&extract_date_iso(&commit.committer)),
                            Some('I') => {
                                let Some((ts, offset_str)) = parse_ident_date(&commit.committer) else { break };
                                let adjusted = ts + parse_tz(offset_str);
                                let dt = time::OffsetDateTime::from_unix_timestamp(adjusted)
                                    .unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
                                let sign_ch = if parse_tz(offset_str) >= 0 { '+' } else { '-' };
                                let abs_off = parse_tz(offset_str).unsigned_abs();
                                target.push_str(&format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}{}{:02}:{:02}",
                                    dt.year(), dt.month() as u8, dt.day(),
                                    dt.hour(), dt.minute(), dt.second(),
                                    sign_ch, abs_off / 3600, (abs_off % 3600) / 60));
                            }
                            Some('r') => {
                                let Some((ts, _)) = parse_ident_date(&commit.committer) else { break };
                                let now = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default().as_secs() as i64;
                                target.push_str(&format_relative_date(now - ts));
                            }
                            Some(other) => { target.push('%'); target.push('c'); target.push(other); }
                            None => { target.push('%'); target.push('c'); }
                        }
                    }
                    Some('x') => {
                        // Hex escape: %xNN
                        chars.next();
                        let mut hex_str = String::new();
                        if let Some(&c1) = chars.peek() { if c1.is_ascii_hexdigit() { hex_str.push(c1); chars.next(); } }
                        if let Some(&c2) = chars.peek() { if c2.is_ascii_hexdigit() { hex_str.push(c2); chars.next(); } }
                        if let Ok(byte) = u8::from_str_radix(&hex_str, 16) {
                            target.push(byte as char);
                        }
                    }
                    Some('C') => {
                        chars.next();
                        if chars.peek() == Some(&'(') {
                            chars.next();
                            let mut spec = String::new();
                            while let Some(c) = chars.next() {
                                if c == ')' { break; }
                                spec.push(c);
                            }
                            let (force, color_spec) = if let Some(rest) = spec.strip_prefix("always,") {
                                (true, rest)
                            } else if let Some(rest) = spec.strip_prefix("auto,") {
                                (false, rest)
                            } else if spec == "auto" {
                                if use_color { target.push_str("\x1b[m"); }
                                continue;
                            } else {
                                (false, spec.as_str())
                            };
                            if use_color || force {
                                target.push_str(&ansi_color_from_spec(color_spec));
                            }
                        } else {
                            // Named colors: %Cred, %Cgreen, %Cblue, %Creset, %Cbold
                            // Must match known names only, not consume trailing text
                            let remaining: String = chars.clone().collect();
                            let known = ["reset", "red", "green", "blue", "yellow", "magenta", "cyan", "white", "bold", "dim", "ul"];
                            let mut matched = false;
                            for name in &known {
                                if remaining.starts_with(name) {
                                    for _ in 0..name.len() { chars.next(); }
                                    if use_color {
                                        target.push_str(&ansi_color_from_name(name));
                                    }
                                    matched = true;
                                    break;
                                }
                            }
                            if !matched {
                                // Unknown color name — consume alphanumerics
                                while let Some(&c) = chars.peek() {
                                    if c.is_alphanumeric() { chars.next(); }
                                    else { break; }
                                }
                            }
                        }
                    }
                    Some('w') => {
                        // %w(...) — wrapping directive, consume and ignore for now
                        chars.next();
                        if chars.peek() == Some(&'(') {
                            chars.next();
                            while let Some(c) = chars.next() {
                                if c == ')' { break; }
                            }
                        }
                    }
                    Some('+') => {
                        // %+x — conditional newline: if next placeholder is non-empty, prepend newline
                        chars.next();
                        // Expand the following placeholder
                        if chars.peek() == Some(&'%') {
                            // The %+ applies to the NEXT expanded value
                            // For simplicity, treat %+x as: if %x is non-empty, emit '\n' + value
                            // This needs the *next* placeholder's value
                        }
                        // Simple: consume the next char as a format code; prepend \n if non-empty
                        let mut sub = String::new();
                        if let Some(&nc) = chars.peek() {
                            match nc {
                                'b' => {
                                    chars.next();
                                    sub.push_str(&body);
                                    if !body.is_empty() { sub.push('\n'); }
                                }
                                's' => { chars.next(); sub.push_str(subject); }
                                _ => { chars.next(); sub.push('%'); sub.push('+'); sub.push(nc); }
                            }
                        }
                        if !sub.is_empty() {
                            target.push('\n');
                            target.push_str(&sub);
                        }
                    }
                    Some('-') => {
                        // %-x — conditional: suppress newline before placeholder if empty
                        chars.next();
                        // Consume the next format code
                        if let Some(&nc) = chars.peek() {
                            match nc {
                                'b' => {
                                    chars.next();
                                    if !body.is_empty() {
                                        target.push_str(&body);
                                        target.push('\n');
                                    }
                                }
                                's' => { chars.next(); target.push_str(subject); }
                                _ => { chars.next(); target.push('%'); target.push('-'); target.push(nc); }
                            }
                        }
                    }
                    Some('d') => {
                        // Decorations — output empty for now
                        chars.next();
                    }
                    Some('D') => {
                        // Decorations without parens — output empty for now
                        chars.next();
                    }
                    Some('e') => {
                        // Encoding
                        chars.next();
                    }
                    Some('g') => {
                        // Reflog placeholders: %gD, %gd, %gs, %gn, %ge, etc.
                        chars.next();
                        if let Some(&_nc) = chars.peek() {
                            chars.next(); // consume the sub-specifier
                            // For non-reflog commits, these expand to empty
                        }
                    }
                    Some(&other) => {
                        chars.next();
                        target.push('%');
                        target.push(other);
                    }
                    None => target.push('%'),
                }
                // Apply pending column formatting
                if let Some(spec) = pending_col.take() {
                    let formatted = apply_col(&spec, &expanded);
                    rendered.push_str(&formatted);
                }
            }
            Ok(rendered)
        }
    }
}

fn resolve_specs(repo: &Repository, specs: &[String]) -> Result<Vec<ObjectId>> {
    let mut out = Vec::with_capacity(specs.len());
    for spec in specs {
        let oid = resolve_revision(repo, spec)?;
        let commit_oid = peel_to_commit(repo, oid)?;
        out.push(commit_oid);
    }
    Ok(out)
}

/// Peel an object (possibly a tag) to the underlying commit.
fn peel_to_commit(repo: &Repository, mut oid: ObjectId) -> Result<ObjectId> {
    loop {
        let object = repo.odb.read(&oid)?;
        match object.kind {
            ObjectKind::Commit => return Ok(oid),
            ObjectKind::Tag => {
                let tag = parse_tag(&object.data)?;
                oid = tag.object;
            }
            other => {
                return Err(Error::CorruptObject(format!(
                    "object {oid} is a {other:?}, not a commit"
                )));
            }
        }
    }
}

fn all_ref_tips(repo: &Repository) -> Result<Vec<ObjectId>> {
    let mut raw = Vec::new();
    if let Ok(head) = refs::resolve_ref(&repo.git_dir, "HEAD") {
        raw.push(head);
    }
    raw.extend(
        refs::list_refs(&repo.git_dir, "refs/")?
            .into_iter()
            .map(|(_, oid)| oid),
    );
    // Peel tags to commits; skip non-commit objects (e.g. tags of blobs/trees)
    let mut out = Vec::new();
    for oid in raw {
        match peel_to_commit(repo, oid) {
            Ok(commit_oid) => out.push(commit_oid),
            Err(_) => {} // skip non-commit refs
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}

fn walk_closure(graph: &mut CommitGraph<'_>, starts: &[ObjectId]) -> Result<HashSet<ObjectId>> {
    let (seen, _) = walk_closure_ordered(graph, starts)?;
    Ok(seen)
}

/// BFS walk that returns both the set and the discovery order.
fn walk_closure_ordered(graph: &mut CommitGraph<'_>, starts: &[ObjectId]) -> Result<(HashSet<ObjectId>, Vec<ObjectId>)> {
    let mut seen = HashSet::new();
    let mut order = Vec::new();
    let mut queue = VecDeque::new();
    for &start in starts {
        queue.push_back(start);
    }
    while let Some(oid) = queue.pop_front() {
        if !seen.insert(oid) {
            continue;
        }
        order.push(oid);
        for parent in graph.parents_of(oid)? {
            queue.push_back(parent);
        }
    }
    Ok((seen, order))
}

fn sort_by_commit_date_desc(
    graph: &mut CommitGraph<'_>,
    selected: &HashSet<ObjectId>,
    discovery_order: &[ObjectId],
) -> Result<Vec<ObjectId>> {
    // Build a map from OID to BFS discovery index for stable tiebreaking
    let disc_idx: HashMap<ObjectId, usize> = discovery_order
        .iter()
        .enumerate()
        .map(|(i, oid)| (*oid, i))
        .collect();
    let mut out: Vec<ObjectId> = selected.iter().copied().collect();
    out.sort_by(|a, b| {
        match graph.committer_time(*b).cmp(&graph.committer_time(*a)) {
            Ordering::Equal => {
                // Preserve BFS discovery order as tiebreaker
                let da = disc_idx.get(a).copied().unwrap_or(usize::MAX);
                let db = disc_idx.get(b).copied().unwrap_or(usize::MAX);
                da.cmp(&db)
            }
            other => other,
        }
    });
    Ok(out)
}

fn topo_sort(graph: &mut CommitGraph<'_>, selected: &HashSet<ObjectId>) -> Result<Vec<ObjectId>> {
    let mut child_count: HashMap<ObjectId, usize> = selected.iter().map(|&oid| (oid, 0)).collect();

    for &oid in selected {
        for parent in graph.parents_of(oid)? {
            if !selected.contains(&parent) {
                continue;
            }
            if let Some(count) = child_count.get_mut(&parent) {
                *count += 1;
            }
        }
    }

    let mut ready = BinaryHeap::new();
    for (&oid, &count) in &child_count {
        if count == 0 {
            ready.push(CommitDateKey {
                oid,
                date: graph.committer_time(oid),
            });
        }
    }

    let mut out = Vec::with_capacity(selected.len());
    while let Some(item) = ready.pop() {
        let oid = item.oid;
        out.push(oid);
        for parent in graph.parents_of(oid)? {
            if !selected.contains(&parent) {
                continue;
            }
            if let Some(count) = child_count.get_mut(&parent) {
                *count = count.saturating_sub(1);
                if *count == 0 {
                    ready.push(CommitDateKey {
                        oid: parent,
                        date: graph.committer_time(parent),
                    });
                }
            }
        }
    }

    Ok(out)
}

fn limit_to_ancestry(
    graph: &mut CommitGraph<'_>,
    selected: &mut HashSet<ObjectId>,
    bottoms: &[ObjectId],
) -> Result<()> {
    let mut keep = HashSet::new();
    for &bottom in bottoms {
        let ancestors = walk_closure(graph, &[bottom])?;
        keep.extend(
            ancestors
                .iter()
                .copied()
                .filter(|oid| selected.contains(oid)),
        );

        for &candidate in selected.iter() {
            if candidate == bottom {
                keep.insert(candidate);
                continue;
            }
            let closure = walk_closure(graph, &[candidate])?;
            if closure.contains(&bottom) {
                keep.insert(candidate);
            }
        }
    }
    selected.retain(|oid| keep.contains(oid));
    Ok(())
}

/// Check if a commit modifies any of the given paths compared to its first parent.
fn commit_touches_paths(
    repo: &Repository,
    graph: &mut CommitGraph<'_>,
    oid: ObjectId,
    paths: &[String],
) -> Result<bool> {
    let commit = load_commit(repo, oid)?;
    let parents = graph.parents_of(oid)?;
    let parent_tree = if let Some(&parent) = parents.first() {
        Some(load_commit(repo, parent)?.tree)
    } else {
        None
    };
    let commit_entries = flatten_tree(repo, commit.tree, "")?;
    let commit_map: HashMap<String, ObjectId> = commit_entries.into_iter().collect();
    let parent_map: HashMap<String, ObjectId> = if let Some(pt) = parent_tree {
        flatten_tree(repo, pt, "")?.into_iter().collect()
    } else {
        HashMap::new()
    };
    for path in paths {
        let c_oid = commit_map.get(path.as_str());
        let p_oid = parent_map.get(path.as_str());
        if c_oid != p_oid {
            return Ok(true);
        }
    }
    Ok(false)
}

fn load_commit(repo: &Repository, oid: ObjectId) -> Result<crate::objects::CommitData> {
    let object = repo.odb.read(&oid)?;
    if object.kind != ObjectKind::Commit {
        return Err(Error::CorruptObject(format!(
            "object {oid} is not a commit"
        )));
    }
    parse_commit(&object.data)
}


fn parse_signature_time(sig: &str) -> i64 {
    let mut parts = sig.split_whitespace().collect::<Vec<_>>();
    if parts.len() < 2 {
        return 0;
    }
    let ts = parts.remove(parts.len().saturating_sub(2));
    ts.parse::<i64>().unwrap_or(0)
}

/// Merge command-line arguments and `--stdin` input lines for this subset.
///
/// Returns `(positive_specs, negative_specs)`.
///
/// # Errors
///
/// Returns [`Error::InvalidRef`] when stdin provides invalid pseudo-options.
pub fn collect_revision_specs_with_stdin(
    args_specs: &[String],
    read_stdin: bool,
) -> Result<(Vec<String>, Vec<String>, bool)> {
    let mut positive = Vec::new();
    let mut negative = Vec::new();
    let mut not_mode = false;

    for spec in args_specs {
        let (pos, neg) = split_revision_token(spec);
        if not_mode {
            positive.extend(neg);
            negative.extend(pos);
        } else {
            positive.extend(pos);
            negative.extend(neg);
        }
    }

    if !read_stdin {
        return Ok((positive, negative, false));
    }

    let mut in_paths = false;
    let mut stdin_all_refs = false;
    let stdin = std::io::read_to_string(std::io::stdin()).map_err(Error::Io)?;
    for raw_line in stdin.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        if in_paths {
            continue;
        }
        if line == "--" {
            in_paths = true;
            continue;
        }
        if line == "--not" {
            not_mode = !not_mode;
            continue;
        }
        if line == "--all" {
            stdin_all_refs = true;
            continue;
        }
        if line.starts_with("--") {
            return Err(Error::InvalidRef(format!(
                "invalid option '{line}' in --stdin mode"
            )));
        }
        if line.starts_with('-') {
            return Err(Error::InvalidRef(format!(
                "invalid option '{line}' in --stdin mode"
            )));
        }
        let (pos, neg) = split_revision_token(line);
        if not_mode {
            positive.extend(neg);
            negative.extend(pos);
        } else {
            positive.extend(pos);
            negative.extend(neg);
        }
    }

    Ok((positive, negative, stdin_all_refs))
}

/// Resolve every local tag object ID.
pub fn tag_targets(git_dir: &Path) -> Result<HashSet<ObjectId>> {
    Ok(refs::list_refs(git_dir, "refs/tags/")?
        .into_iter()
        .map(|(_, oid)| oid)
        .collect())
}

struct CommitGraph<'r> {
    repo: &'r Repository,
    first_parent_only: bool,
    parents: HashMap<ObjectId, Vec<ObjectId>>,
    committer_time: HashMap<ObjectId, i64>,
    shallow_boundaries: HashSet<ObjectId>,
}

impl<'r> CommitGraph<'r> {
    fn new(repo: &'r Repository, first_parent_only: bool) -> Self {
        let shallow_boundaries = load_shallow_boundaries(&repo.git_dir);
        Self {
            repo,
            first_parent_only,
            parents: HashMap::new(),
            committer_time: HashMap::new(),
            shallow_boundaries,
        }
    }

    fn parents_of(&mut self, oid: ObjectId) -> Result<Vec<ObjectId>> {
        self.populate(oid)?;
        Ok(self.parents.get(&oid).cloned().unwrap_or_default())
    }

    fn committer_time(&mut self, oid: ObjectId) -> i64 {
        if self.populate(oid).is_err() {
            return 0;
        }
        self.committer_time.get(&oid).copied().unwrap_or(0)
    }

    fn populate(&mut self, oid: ObjectId) -> Result<()> {
        if self.parents.contains_key(&oid) {
            return Ok(());
        }
        let commit = load_commit(self.repo, oid)?;
        // Shallow boundaries: treat commit as having no parents
        let mut parents = if self.shallow_boundaries.contains(&oid) {
            Vec::new()
        } else {
            commit.parents
        };
        if self.first_parent_only && parents.len() > 1 {
            parents.truncate(1);
        }
        self.committer_time
            .insert(oid, parse_signature_time(&commit.committer));
        self.parents.insert(oid, parents);
        Ok(())
    }
}

/// Load shallow boundary commit OIDs from `.git/shallow`.
fn load_shallow_boundaries(git_dir: &Path) -> HashSet<ObjectId> {
    let shallow_path = git_dir.join("shallow");
    let mut set = HashSet::new();
    if let Ok(contents) = fs::read_to_string(&shallow_path) {
        for line in contents.lines() {
            let line = line.trim();
            if !line.is_empty() {
                if let Ok(oid) = line.parse::<ObjectId>() {
                    set.insert(oid);
                }
            }
        }
    }
    set
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CommitDateKey {
    oid: ObjectId,
    date: i64,
}

impl Ord for CommitDateKey {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.date.cmp(&other.date) {
            Ordering::Equal => self.oid.cmp(&other.oid),
            ord => ord,
        }
    }
}

impl PartialOrd for CommitDateKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Read every line from a newline-delimited file.
///
/// # Errors
///
/// Returns [`Error::Io`] when the file cannot be read.
pub fn read_lines(path: &Path) -> Result<Vec<String>> {
    let content = fs::read_to_string(path)?;
    Ok(content.lines().map(|line| line.to_owned()).collect())
}

/// Check if a token uses the symmetric diff `...` notation.
#[must_use]
pub fn is_symmetric_diff(token: &str) -> bool {
    token.contains("...") && !token.contains("....")
}

/// Split a symmetric diff token into (lhs, rhs).
#[must_use]
pub fn split_symmetric_diff(token: &str) -> Option<(String, String)> {
    token.split_once("...").map(|(l, r)| (l.to_owned(), r.to_owned()))
}

/// Collect all reachable non-commit objects (trees and blobs) from a set of commits.
/// Returns (included, omitted) object lists.
#[allow(dead_code)]
fn collect_reachable_objects(
    repo: &Repository,
    _graph: &mut CommitGraph<'_>,
    commits: &[ObjectId],
    filter: Option<&ObjectFilter>,
) -> Result<(Vec<(ObjectId, String)>, Vec<ObjectId>)> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    let mut omitted = Vec::new();
    for &commit_oid in commits {
        let commit = load_commit(repo, commit_oid)?;
        collect_tree_objects_filtered(
            repo, commit.tree, "", 0, &mut seen, &mut result, &mut omitted, filter,
        )?;
    }
    Ok((result, omitted))
}

#[allow(dead_code)]
fn collect_tree_objects_filtered(
    repo: &Repository,
    tree_oid: ObjectId,
    prefix: &str,
    depth: u64,
    seen: &mut HashSet<ObjectId>,
    result: &mut Vec<(ObjectId, String)>,
    omitted: &mut Vec<ObjectId>,
    filter: Option<&ObjectFilter>,
) -> Result<()> {
    if !seen.insert(tree_oid) {
        return Ok(());
    }
    // Check if this tree passes the filter
    let tree_included = filter.map_or(true, |f| f.includes_tree(depth));
    if tree_included {
        result.push((tree_oid, prefix.to_owned()));
    } else {
        omitted.push(tree_oid);
    }
    let object = repo.odb.read(&tree_oid)?;
    if object.kind != ObjectKind::Tree {
        return Ok(());
    }
    let entries = parse_tree(&object.data)?;
    for entry in entries {
        // Skip gitlink (submodule) entries — their OIDs reference commits
        // in the submodule's object store, not the parent repo.
        if entry.mode == 0o160000 {
            continue;
        }
        let name = String::from_utf8_lossy(&entry.name).to_string();
        let path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        if !seen.insert(entry.oid) {
            continue;
        }
        let child_obj = repo.odb.read(&entry.oid)?;
        match child_obj.kind {
            ObjectKind::Tree => {
                // Recurse into subtrees; the filter check happens at the recursive call
                seen.remove(&entry.oid);
                collect_tree_objects_filtered(
                    repo,
                    entry.oid,
                    &path,
                    depth + 1,
                    seen,
                    result,
                    omitted,
                    filter,
                )?;
            }
            _ => {
                // Blob: check filter
                let blob_included = filter.map_or(true, |f| {
                    f.includes_blob(child_obj.data.len() as u64)
                });
                if blob_included {
                    result.push((entry.oid, path));
                } else {
                    omitted.push(entry.oid);
                }
            }
        }
    }
    Ok(())
}

/// Collect reachable objects in commit order: objects for each commit are emitted
/// right after that commit, rather than all objects after all commits.
/// Returns (objects, omitted, per_commit_counts).
fn collect_reachable_objects_in_commit_order(
    repo: &Repository,
    _graph: &mut CommitGraph<'_>,
    commits: &[ObjectId],
    filter: Option<&ObjectFilter>,
) -> Result<(Vec<(ObjectId, String)>, Vec<ObjectId>, Vec<usize>)> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    let mut omitted = Vec::new();
    let mut counts = Vec::with_capacity(commits.len());
    for &commit_oid in commits {
        let commit = load_commit(repo, commit_oid)?;
        let before = result.len();
        collect_tree_objects_filtered(
            repo, commit.tree, "", 0, &mut seen, &mut result, &mut omitted, filter,
        )?;
        counts.push(result.len() - before);
    }
    Ok((result, omitted, counts))
}

/// Collect OIDs of all objects in packs that have a `.keep` file.
fn kept_object_ids(repo: &Repository) -> Result<HashSet<ObjectId>> {
    let pack_dir = repo.git_dir.join("objects/pack");
    let mut kept = HashSet::new();
    if !pack_dir.is_dir() {
        return Ok(kept);
    }
    for entry in std::fs::read_dir(&pack_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().map_or(false, |ext| ext == "keep") {
            // Find the corresponding .idx file
            let idx_path = path.with_extension("idx");
            if idx_path.exists() {
                if let Ok(oids) = crate::pack::read_idx_object_ids(&idx_path) {
                    kept.extend(oids);
                }
            }
        }
    }
    Ok(kept)
}

/// Compute a simple patch-id for each commit.
#[allow(dead_code)]
fn compute_patch_ids(
    repo: &Repository,
    graph: &mut CommitGraph<'_>,
    commits: &[ObjectId],
) -> Result<HashMap<ObjectId, String>> {
    let mut result = HashMap::new();
    for &oid in commits {
        let commit = load_commit(repo, oid)?;
        let parents = graph.parents_of(oid)?;
        let parent_tree = if let Some(&parent) = parents.first() {
            load_commit(repo, parent)?.tree
        } else {
            ObjectId::from_hex("4b825dc642cb6eb9a060e54bf8d69288fbee4904")?
        };
        let patch_id = compute_tree_diff_id(repo, parent_tree, commit.tree)?;
        result.insert(oid, patch_id);
    }
    Ok(result)
}

#[allow(dead_code)]
fn compute_tree_diff_id(
    repo: &Repository,
    tree_a: ObjectId,
    tree_b: ObjectId,
) -> Result<String> {
    use std::collections::BTreeMap;
    let entries_a = flatten_tree(repo, tree_a, "")?;
    let entries_b = flatten_tree(repo, tree_b, "")?;
    let map_a: BTreeMap<_, _> = entries_a.into_iter().collect();
    let map_b: BTreeMap<_, _> = entries_b.into_iter().collect();
    let mut diff_parts = Vec::new();
    for (path, oid_b) in &map_b {
        match map_a.get(path) {
            Some(oid_a) if oid_a != oid_b => {
                diff_parts.push(format!("+{path}:{oid_b}"));
            }
            None => {
                diff_parts.push(format!("A{path}:{oid_b}"));
            }
            _ => {}
        }
    }
    for (path, oid_a) in &map_a {
        if !map_b.contains_key(path) {
            diff_parts.push(format!("D{path}:{oid_a}"));
        }
    }
    diff_parts.sort();
    Ok(diff_parts.join("\n"))
}

#[allow(dead_code)]
fn flatten_tree(
    repo: &Repository,
    tree_oid: ObjectId,
    prefix: &str,
) -> Result<Vec<(String, ObjectId)>> {
    let mut result = Vec::new();
    let object = match repo.odb.read(&tree_oid) {
        Ok(o) => o,
        Err(_) => return Ok(result),
    };
    if object.kind != ObjectKind::Tree {
        return Ok(result);
    }
    let entries = parse_tree(&object.data)?;
    for entry in entries {
        let name = String::from_utf8_lossy(&entry.name).to_string();
        let path = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        let child = repo.odb.read(&entry.oid)?;
        if child.kind == ObjectKind::Tree {
            result.extend(flatten_tree(repo, entry.oid, &path)?);
        } else {
            result.push((path, entry.oid));
        }
    }
    Ok(result)
}

/// Compute merge bases between two commits.
pub fn merge_bases(
    repo: &Repository,
    a: ObjectId,
    b: ObjectId,
    first_parent_only: bool,
) -> Result<Vec<ObjectId>> {
    let mut graph = CommitGraph::new(repo, first_parent_only);
    let ancestors_a = walk_closure(&mut graph, &[a])?;
    let ancestors_b = walk_closure(&mut graph, &[b])?;
    let common: HashSet<ObjectId> = ancestors_a.intersection(&ancestors_b).copied().collect();
    if common.is_empty() {
        return Ok(Vec::new());
    }
    // Merge bases: common ancestors not dominated by other common ancestors
    let mut bases = Vec::new();
    for &c in &common {
        let is_dominated = common.iter().any(|&other| {
            if other == c { return false; }
            let other_anc = walk_closure(&mut graph, &[other]).unwrap_or_default();
            other_anc.contains(&c)
        });
        if !is_dominated {
            bases.push(c);
        }
    }
    if bases.is_empty() {
        let mut sorted: Vec<_> = common.into_iter().collect();
        sorted.sort_by_key(|b| std::cmp::Reverse(graph.committer_time(*b)));
        bases.push(sorted[0]);
    }
    Ok(bases)
}
