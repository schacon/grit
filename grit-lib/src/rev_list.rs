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
use crate::objects::{parse_commit, parse_tree, ObjectId, ObjectKind};
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

    let mut included = walk_closure(&mut graph, &include)?;
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
        OrderingMode::Default => sort_by_commit_date_desc(&mut graph, &included)?,
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

    // Collect reachable objects if --objects
    let (objects, omitted_objects) = if options.objects {
        collect_reachable_objects(repo, &mut graph, &ordered, options.filter.as_ref())?
    } else {
        (Vec::new(), Vec::new())
    };

    Ok(RevListResult {
        commits: ordered,
        objects,
        omitted_objects,
        boundary_commits: Vec::new(),
        left_right_map,
        cherry_equivalent,
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
        return (vec![rhs.to_owned()], vec![lhs.to_owned()]);
    }
    if let Some(rest) = token.strip_prefix('^') {
        return (Vec::new(), vec![rest.to_owned()]);
    }
    (vec![token.to_owned()], Vec::new())
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
            let mut rendered = String::new();
            let mut chars = fmt.chars().peekable();
            while let Some(ch) = chars.next() {
                if ch != '%' {
                    rendered.push(ch);
                    continue;
                }
                match chars.next() {
                    Some('%') => rendered.push('%'),
                    Some('H') => rendered.push_str(&oid.to_hex()),
                    Some('h') => {
                        let hex = oid.to_hex();
                        let n = abbrev_len.clamp(4, 40).min(hex.len());
                        rendered.push_str(&hex[..n]);
                    }
                    Some('s') => rendered.push_str(subject),
                    Some(other) => {
                        rendered.push('%');
                        rendered.push(other);
                    }
                    None => rendered.push('%'),
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
        ensure_commit(repo, oid)?;
        out.push(oid);
    }
    Ok(out)
}

fn all_ref_tips(repo: &Repository) -> Result<Vec<ObjectId>> {
    let mut out = Vec::new();
    if let Ok(head) = refs::resolve_ref(&repo.git_dir, "HEAD") {
        out.push(head);
    }
    out.extend(
        refs::list_refs(&repo.git_dir, "refs/")?
            .into_iter()
            .map(|(_, oid)| oid),
    );
    out.sort();
    out.dedup();
    Ok(out)
}

fn walk_closure(graph: &mut CommitGraph<'_>, starts: &[ObjectId]) -> Result<HashSet<ObjectId>> {
    let mut seen = HashSet::new();
    let mut queue = VecDeque::new();
    for &start in starts {
        queue.push_back(start);
    }
    while let Some(oid) = queue.pop_front() {
        if !seen.insert(oid) {
            continue;
        }
        for parent in graph.parents_of(oid)? {
            queue.push_back(parent);
        }
    }
    Ok(seen)
}

fn sort_by_commit_date_desc(
    graph: &mut CommitGraph<'_>,
    selected: &HashSet<ObjectId>,
) -> Result<Vec<ObjectId>> {
    let mut out = selected.iter().copied().collect::<Vec<_>>();
    out.sort_by(
        |a, b| match graph.committer_time(*b).cmp(&graph.committer_time(*a)) {
            Ordering::Equal => b.cmp(a),
            other => other,
        },
    );
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

fn ensure_commit(repo: &Repository, oid: ObjectId) -> Result<()> {
    let object = repo.odb.read(&oid)?;
    if object.kind != ObjectKind::Commit {
        return Err(Error::CorruptObject(format!(
            "object {oid} is not a commit"
        )));
    }
    Ok(())
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
}

impl<'r> CommitGraph<'r> {
    fn new(repo: &'r Repository, first_parent_only: bool) -> Self {
        Self {
            repo,
            first_parent_only,
            parents: HashMap::new(),
            committer_time: HashMap::new(),
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
        let mut parents = commit.parents;
        if self.first_parent_only && parents.len() > 1 {
            parents.truncate(1);
        }
        self.committer_time
            .insert(oid, parse_signature_time(&commit.committer));
        self.parents.insert(oid, parents);
        Ok(())
    }
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
