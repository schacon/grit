//! Commit traversal and output planning for `rev-list`.
//!
//! This module implements a focused `rev-list` subset used by the v2 test
//! wave: revision ranges, `--all`, `--stdin` argument ingestion, commit walk
//! limits, ordering (`--topo-order`, `--date-order`, `--reverse`), and basic
//! output shaping (`--count`, `--parents`, `--format`).

use std::cmp::Ordering;
use std::collections::{BTreeSet, BinaryHeap, HashMap, HashSet, VecDeque};
use std::fs;
use std::path::Path;

use crate::error::{Error, Result};
use crate::ignore::{parse_sparse_patterns_from_blob, path_matches_sparse_pattern_list};
use crate::objects::{parse_commit, parse_tag, parse_tree, ObjectId, ObjectKind};
use crate::pack;
use crate::patch_ids::compute_patch_id;
use crate::refs;
use crate::repo::Repository;
use crate::rev_parse::{resolve_revision_for_range_end, resolve_treeish_path, split_treeish_spec};

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

/// Behavior when reachable objects are missing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MissingAction {
    /// Fail traversal when a referenced object is missing.
    Error,
    /// Continue traversal and report each missing object.
    Print,
    /// Continue traversal and silently ignore missing objects.
    Allow,
}

/// Kind selector for `object:type=<kind>` filters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterObjectKind {
    Blob,
    Tree,
    Commit,
    Tag,
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
    /// `sparse:oid=<hex>` — sparse-checkout style path filter from a blob.
    SparseOid(ObjectId),
    /// `object:type=(blob|tree|commit|tag)` — keep only objects of that type.
    ObjectType(FilterObjectKind),
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
        if let Some(rest) = spec.strip_prefix("object:type=") {
            let kind = match rest {
                "blob" => FilterObjectKind::Blob,
                "tree" => FilterObjectKind::Tree,
                "commit" => FilterObjectKind::Commit,
                "tag" => FilterObjectKind::Tag,
                "" => return Err("invalid object type".to_owned()),
                _ => return Err(format!("invalid object type: {rest}")),
            };
            return Ok(ObjectFilter::ObjectType(kind));
        }
        if let Some(rest) = spec.strip_prefix("sparse:oid=") {
            let oid = rest
                .parse::<ObjectId>()
                .map_err(|_| format!("invalid sparse:oid value: {rest}"))?;
            return Ok(ObjectFilter::SparseOid(oid));
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

    /// Merge another `--filter` argument (Git joins multiple filters with AND).
    #[must_use]
    pub fn merge_with(self, other: Self) -> Self {
        match (self, other) {
            (ObjectFilter::Combine(mut a), ObjectFilter::Combine(mut b)) => {
                a.append(&mut b);
                ObjectFilter::Combine(a)
            }
            (ObjectFilter::Combine(mut a), b) => {
                a.push(b);
                ObjectFilter::Combine(a)
            }
            (a, ObjectFilter::Combine(mut b)) => {
                let mut v = vec![a];
                v.append(&mut b);
                ObjectFilter::Combine(v)
            }
            (a, b) => ObjectFilter::Combine(vec![a, b]),
        }
    }

    /// Check if a blob should be included given its size.
    pub fn includes_blob(&self, size: u64) -> bool {
        match self {
            ObjectFilter::BlobNone => false,
            ObjectFilter::BlobLimit(limit) => size <= *limit,
            // Depth is applied via [`ObjectFilter::includes_blob_under_tree`]; this stays permissive
            // for callers that only have a size (e.g. loose-object scans).
            ObjectFilter::TreeDepth(_) => true,
            ObjectFilter::SparseOid(_) => true,
            ObjectFilter::ObjectType(kind) => *kind == FilterObjectKind::Blob,
            ObjectFilter::Combine(filters) => filters.iter().all(|f| f.includes_blob(size)),
        }
    }

    /// Whether a blob that lives directly under a tree at `parent_tree_depth` passes this filter.
    ///
    /// For `tree:<n>` filters, Git assigns blobs the traversal depth after entering the parent tree,
    /// which matches `parent_tree_depth + 1` in our walk where the commit root tree is depth `0`.
    #[must_use]
    pub fn includes_blob_under_tree(&self, size: u64, parent_tree_depth: u64) -> bool {
        match self {
            ObjectFilter::BlobNone => false,
            ObjectFilter::BlobLimit(limit) => size <= *limit,
            ObjectFilter::TreeDepth(max_depth) => parent_tree_depth.saturating_add(1) < *max_depth,
            ObjectFilter::SparseOid(_) => true,
            ObjectFilter::ObjectType(kind) => *kind == FilterObjectKind::Blob,
            ObjectFilter::Combine(filters) => filters
                .iter()
                .all(|f| f.includes_blob_under_tree(size, parent_tree_depth)),
        }
    }

    /// Check if a tree at given depth should be included.
    pub fn includes_tree(&self, depth: u64) -> bool {
        match self {
            ObjectFilter::BlobNone => true,
            ObjectFilter::BlobLimit(_) => true,
            ObjectFilter::TreeDepth(max_depth) => depth < *max_depth,
            ObjectFilter::SparseOid(_) => true,
            ObjectFilter::ObjectType(kind) => *kind == FilterObjectKind::Tree,
            ObjectFilter::Combine(filters) => filters.iter().all(|f| f.includes_tree(depth)),
        }
    }

    /// Whether a commit or tag object should appear in a flat object scan (e.g. `cat-file --batch-all-objects`).
    pub fn includes_commit_or_tag_object(&self, kind: ObjectKind) -> bool {
        let expected = match kind {
            ObjectKind::Commit => Some(FilterObjectKind::Commit),
            ObjectKind::Tag => Some(FilterObjectKind::Tag),
            _ => None,
        };
        match self {
            ObjectFilter::BlobNone | ObjectFilter::BlobLimit(_) => true,
            ObjectFilter::TreeDepth(_) => true,
            ObjectFilter::SparseOid(_) => true,
            ObjectFilter::ObjectType(t) => expected == Some(*t),
            ObjectFilter::Combine(filters) => filters
                .iter()
                .all(|f| f.includes_commit_or_tag_object(kind)),
        }
    }

    /// True if `kind` / `size` pass this filter when enumerating a single object (no tree path).
    pub fn includes_loose_object(&self, kind: ObjectKind, size: u64) -> bool {
        match kind {
            ObjectKind::Blob => self.includes_blob(size),
            ObjectKind::Tree => self.includes_tree(0),
            ObjectKind::Commit | ObjectKind::Tag => self.includes_commit_or_tag_object(kind),
        }
    }

    /// Whether an object passes this filter for direct OID lookup (`git cat-file --filter`).
    #[must_use]
    pub fn passes_for_object(&self, kind: ObjectKind, size: usize) -> bool {
        self.includes_loose_object(kind, size as u64)
    }
}

/// Reachable object IDs enumerated the same way as `git rev-list --objects --no-object-names --all`,
/// optionally with `--filter` and `--filter-provided-objects` (used by `git cat-file --batch-all-objects`).
#[must_use]
pub fn reachable_object_ids_for_cat_file(
    repo: &Repository,
    filter: Option<&ObjectFilter>,
    filter_provided_objects: bool,
) -> Result<Vec<ObjectId>> {
    let opts = RevListOptions {
        all_refs: true,
        objects: true,
        no_object_names: true,
        quiet: true,
        filter: filter.cloned(),
        filter_provided_objects,
        ..Default::default()
    };
    let result = rev_list(repo, &[], &[], &opts)?;
    let mut set = BTreeSet::new();
    for oid in &result.commits {
        set.insert(*oid);
    }
    for (oid, _) in &result.objects {
        set.insert(*oid);
    }
    Ok(set.into_iter().collect())
}

/// Objects matching `filter`, for `cat-file --batch-all-objects --filter` (same set as
/// `rev-list --objects --all --filter --filter-provided-objects`).
#[must_use]
pub fn object_ids_for_cat_file_filtered(
    repo: &Repository,
    filter: &ObjectFilter,
) -> Result<Vec<ObjectId>> {
    reachable_object_ids_for_cat_file(repo, Some(filter), true)
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
    let chars = spec.chars().peekable();
    for ch in chars {
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
    /// When set with `--filter`, explicitly given revision objects are filtered too.
    pub filter_provided_objects: bool,
    /// Print omitted objects prefixed with `~`.
    pub filter_print_omitted: bool,
    /// Emit objects interleaved with their introducing commit.
    pub in_commit_order: bool,
    /// Exclude objects in `.keep` pack files.
    pub no_kept_objects: bool,
    /// Behavior when referenced objects are missing.
    pub missing_action: MissingAction,
    /// When set with `--objects`, omit path names from non-commit object lines (bitmap-style output).
    pub use_bitmap_index: bool,
    /// When set with `--objects`, list only objects not present in any pack file.
    pub unpacked_only: bool,
    /// With `--use-bitmap-index`, emit OID-only object lines (no paths / trailing space) for filters
    /// that match Git's bitmap object formatting.
    pub bitmap_oid_only_objects: bool,
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
            filter_provided_objects: false,
            filter_print_omitted: false,
            in_commit_order: false,
            no_kept_objects: false,
            missing_action: MissingAction::Error,
            use_bitmap_index: false,
            unpacked_only: false,
            bitmap_oid_only_objects: false,
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
    /// Referenced objects missing from the object database.
    pub missing_objects: Vec<ObjectId>,
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
    /// Commit OIDs given as positive revision tips (for Git `USER_GIVEN` / filter edge cases).
    pub object_walk_tips: Vec<ObjectId>,
    /// When `--objects` is active, whether to print the commit line before that commit's objects.
    /// Aligns with Git marking user-given tips vs `NOT_USER_GIVEN` commits in list-objects.
    pub objects_print_commit: Vec<bool>,
    /// When `--objects` is active and not `--in-commit-order`, objects grouped per commit walk plus
    /// a final segment for explicit `object_roots` (length `commits.len() + 1`).
    pub object_segments: Vec<Vec<(ObjectId, String)>>,
    /// True when `--use-bitmap-index --objects` should format trees/blobs as bare OIDs (no paths).
    pub bitmap_object_format: bool,
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

    let (mut include, object_roots) = if options.objects {
        resolve_specs_for_objects(repo, positive_specs)?
    } else {
        (resolve_specs(repo, positive_specs)?, Vec::new())
    };
    let exclude = resolve_specs(repo, negative_specs)?;

    if options.all_refs {
        include.extend(all_ref_tips(repo)?);
    }

    let object_walk_tip_commits: Vec<ObjectId> = if options.objects {
        include.clone()
    } else {
        Vec::new()
    };

    if include.is_empty() && object_roots.is_empty() {
        return Err(Error::InvalidRef("no revisions specified".to_owned()));
    }

    let (mut included, _discovery_order) = if include.is_empty() {
        (HashSet::new(), Vec::new())
    } else {
        walk_closure_ordered(&mut graph, &include)?
    };
    let excluded = if exclude.is_empty() {
        HashSet::new()
    } else {
        walk_closure(&mut graph, &exclude)?
    };
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
        OrderingMode::Default => {
            let tips: Vec<ObjectId> = include
                .iter()
                .copied()
                .filter(|oid| included.contains(oid))
                .collect();
            date_order_walk(&mut graph, &tips, &included)?
        }
        OrderingMode::Topo | OrderingMode::Date => topo_sort(&mut graph, &included)?,
    };

    // Path filtering: keep only commits that modify given paths
    if !options.paths.is_empty() {
        let paths = &options.paths;
        ordered.retain(|oid| {
            commit_touches_paths(
                repo,
                &mut graph,
                *oid,
                paths,
                options.full_history,
                options.sparse,
            )
            .unwrap_or(false)
        });
    }

    // Left-right classification for symmetric diffs
    let mut left_right_map = HashMap::new();
    if options.left_right
        || options.left_only
        || options.right_only
        || options.cherry_mark
        || options.cherry_pick
    {
        if let (Some(left_oid), Some(right_oid)) = (options.symmetric_left, options.symmetric_right)
        {
            // Match Git's `SYMMETRIC_LEFT` / right-only classification (`revision.c`): a commit is
            // "left" iff it is reachable from the left tip but not from the right tip, and vice
            // versa.  Using plain set intersection incorrectly labels the shared spine as "right"
            // only, which breaks `--cherry-pick` on `A...B` (t3419-rebase-patch-id).
            let left_reach = walk_closure(&mut graph, &[left_oid])?;
            let right_reach = walk_closure(&mut graph, &[right_oid])?;
            for &oid in &ordered {
                let from_left = left_reach.contains(&oid);
                let from_right = right_reach.contains(&oid);
                if from_left && !from_right {
                    left_right_map.insert(oid, true);
                } else if from_right && !from_left {
                    left_right_map.insert(oid, false);
                } else {
                    left_right_map.insert(oid, false);
                }
            }
        }
    }

    // Cherry-pick / cherry-mark: match commits by Git-compatible patch-id (see `git revision.c`
    // `cherry_pick_list`, used by `git rebase` todo generation).
    let mut cherry_equivalent = HashSet::new();
    if options.cherry_pick || options.cherry_mark {
        let left_commits: Vec<_> = ordered
            .iter()
            .filter(|o| left_right_map.get(o) == Some(&true))
            .copied()
            .collect();
        let right_commits: Vec<_> = ordered
            .iter()
            .filter(|o| left_right_map.get(o) == Some(&false))
            .copied()
            .collect();
        let left_first = !left_commits.is_empty()
            && !right_commits.is_empty()
            && left_commits.len() < right_commits.len();

        let mut by_patch: HashMap<ObjectId, ObjectId> = HashMap::new();
        if left_first {
            for oid in &left_commits {
                if let Ok(Some(pid)) = compute_patch_id(&repo.odb, oid) {
                    by_patch.entry(pid).or_insert(*oid);
                }
            }
            for oid in &right_commits {
                if let Ok(Some(pid)) = compute_patch_id(&repo.odb, oid) {
                    if let Some(&other) = by_patch.get(&pid) {
                        cherry_equivalent.insert(*oid);
                        cherry_equivalent.insert(other);
                    }
                }
            }
        } else {
            for oid in &right_commits {
                if let Ok(Some(pid)) = compute_patch_id(&repo.odb, oid) {
                    by_patch.entry(pid).or_insert(*oid);
                }
            }
            for oid in &left_commits {
                if let Ok(Some(pid)) = compute_patch_id(&repo.odb, oid) {
                    if let Some(&other) = by_patch.get(&pid) {
                        cherry_equivalent.insert(*oid);
                        cherry_equivalent.insert(other);
                    }
                }
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

    if options.unpacked_only {
        let packed = packed_object_set(repo);
        ordered.retain(|oid| !packed.contains(oid));
    }

    let commit_tips_set: HashSet<ObjectId> = object_walk_tip_commits.iter().copied().collect();
    let objects_print_commit: Vec<bool> = if options.objects {
        ordered
            .iter()
            .map(|&c| {
                let user_given = !options.filter_provided_objects && commit_tips_set.contains(&c);
                user_given || filter_shows_commit_line_when_not_user_given(options.filter.as_ref())
            })
            .collect()
    } else {
        Vec::new()
    };

    let sparse_lines = sparse_oid_lines_from_filter(repo, options.filter.as_ref());
    let skip_trees = skip_tree_descent_for_object_type_filter(options.filter.as_ref());
    let bitmap_object_format = options.objects
        && options.use_bitmap_index
        && (options.bitmap_oid_only_objects || !object_roots.is_empty() || options.unpacked_only);
    let omit_object_paths = bitmap_object_format;
    let packed_set = if options.objects && options.unpacked_only {
        Some(packed_object_set(repo))
    } else {
        None
    };

    // Collect reachable objects if --objects
    let (objects, omitted_objects, missing_objects, per_commit_object_counts, object_segments) =
        if options.objects {
            let filter_provided = options.filter_provided_objects;
            let (mut objs, omit, miss, counts, segments) = if options.in_commit_order {
                let (o, om, mi, c) = collect_reachable_objects_in_commit_order(
                    repo,
                    &mut graph,
                    &ordered,
                    &object_roots,
                    options.filter.as_ref(),
                    filter_provided,
                    options.missing_action,
                    sparse_lines.as_deref(),
                    skip_trees,
                    omit_object_paths,
                    packed_set.as_ref(),
                )?;
                (o, om, mi, c, Vec::new())
            } else {
                let (o, om, mi, seg) = collect_reachable_objects_segmented(
                    repo,
                    &mut graph,
                    &ordered,
                    &object_roots,
                    options.filter.as_ref(),
                    filter_provided,
                    options.missing_action,
                    sparse_lines.as_deref(),
                    skip_trees,
                    omit_object_paths,
                    packed_set.as_ref(),
                )?;
                (o, om, mi, Vec::new(), seg)
            };
            if options.no_kept_objects {
                objs.retain(|(oid, _)| !kept_set.contains(oid));
            }
            (objs, omit, miss, counts, segments)
        } else {
            (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new())
        };

    Ok(RevListResult {
        commits: ordered,
        objects,
        omitted_objects,
        missing_objects,
        boundary_commits,
        left_right_map,
        cherry_equivalent,
        per_commit_object_counts,
        object_walk_tips: object_walk_tip_commits,
        objects_print_commit,
        object_segments,
        bitmap_object_format,
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
    if let Some((lhs, rhs)) = crate::rev_parse::split_double_dot_range(token) {
        let positive = if rhs.is_empty() {
            "HEAD".to_owned()
        } else {
            rhs.to_owned()
        };
        let negative = if lhs.is_empty() {
            "HEAD".to_owned()
        } else {
            lhs.to_owned()
        };
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
        "black" => Some(0),
        "red" => Some(1),
        "green" => Some(2),
        "yellow" => Some(3),
        "blue" => Some(4),
        "magenta" => Some(5),
        "cyan" => Some(6),
        "white" => Some(7),
        "default" => Some(9),
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
        if m == 1 {
            "1 minute ago".to_owned()
        } else {
            format!("{m} minutes ago")
        }
    } else if diff < 86400 {
        let h = diff / 3600;
        if h == 1 {
            "1 hour ago".to_owned()
        } else {
            format!("{h} hours ago")
        }
    } else if diff < 86400 * 30 {
        let d = diff / 86400;
        if d == 1 {
            "1 day ago".to_owned()
        } else {
            format!("{d} days ago")
        }
    } else if diff < 86400 * 365 {
        let months = diff / (86400 * 30);
        if months == 1 {
            "1 month ago".to_owned()
        } else {
            format!("{months} months ago")
        }
    } else {
        let years = diff / (86400 * 365);
        if years == 1 {
            "1 year ago".to_owned()
        } else {
            format!("{years} years ago")
        }
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
                            } else {
                                ""
                            }
                        } else {
                            ""
                        };
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
                            } else {
                                ""
                            }
                        } else {
                            ""
                        };
                        format!("{} {}", name, email)
                    }
                    fn format_default_date(ident: &str) -> String {
                        let parts: Vec<&str> = ident.rsplitn(3, ' ').collect();
                        if parts.len() < 2 {
                            return String::new();
                        }
                        let ts_str = parts[1];
                        let offset_str = parts[0];
                        let ts: i64 = match ts_str.parse() {
                            Ok(v) => v,
                            Err(_) => return format!("{ts_str} {offset_str}"),
                        };
                        let tz_bytes = offset_str.as_bytes();
                        let tz_secs: i64 = if tz_bytes.len() >= 5 {
                            let sign = if tz_bytes[0] == b'-' { -1i64 } else { 1i64 };
                            let h: i64 = offset_str[1..3].parse().unwrap_or(0);
                            let m: i64 = offset_str[3..5].parse().unwrap_or(0);
                            sign * (h * 3600 + m * 60)
                        } else {
                            0
                        };
                        let adjusted = ts + tz_secs;
                        let dt = time::OffsetDateTime::from_unix_timestamp(adjusted)
                            .unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
                        let weekday = match dt.weekday() {
                            time::Weekday::Monday => "Mon",
                            time::Weekday::Tuesday => "Tue",
                            time::Weekday::Wednesday => "Wed",
                            time::Weekday::Thursday => "Thu",
                            time::Weekday::Friday => "Fri",
                            time::Weekday::Saturday => "Sat",
                            time::Weekday::Sunday => "Sun",
                        };
                        let month = match dt.month() {
                            time::Month::January => "Jan",
                            time::Month::February => "Feb",
                            time::Month::March => "Mar",
                            time::Month::April => "Apr",
                            time::Month::May => "May",
                            time::Month::June => "Jun",
                            time::Month::July => "Jul",
                            time::Month::August => "Aug",
                            time::Month::September => "Sep",
                            time::Month::October => "Oct",
                            time::Month::November => "Nov",
                            time::Month::December => "Dec",
                        };
                        format!(
                            "{} {} {} {:02}:{:02}:{:02} {} {}",
                            weekday,
                            month,
                            dt.day(),
                            dt.hour(),
                            dt.minute(),
                            dt.second(),
                            dt.year(),
                            offset_str
                        )
                    }
                    let mut out = String::new();
                    out.push_str(&format!(
                        "Author: {}\n",
                        extract_ident_display(&commit.author)
                    ));
                    out.push_str(&format!(
                        "Date:   {}\n",
                        format_default_date(&commit.author)
                    ));
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
                        std::iter::once(blank)
                            .chain(lines)
                            .collect::<Vec<_>>()
                            .join("\n")
                    }
                } else {
                    String::new()
                }
            };
            let tree_hex = commit.tree.to_hex();
            let parent_hexes: Vec<String> = commit.parents.iter().map(|p| p.to_hex()).collect();
            let parent_abbrevs: Vec<String> = commit
                .parents
                .iter()
                .map(|p| {
                    let hex = p.to_hex();
                    let n = abbrev_len.clamp(4, 40).min(hex.len());
                    hex[..n].to_string()
                })
                .collect();

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
                if parts.len() >= 2 {
                    parts[1]
                } else {
                    ""
                }
            }
            fn parse_ident_date(ident: &str) -> Option<(i64, &str)> {
                let parts: Vec<&str> = ident.rsplitn(3, ' ').collect();
                if parts.len() < 2 {
                    return None;
                }
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
                } else {
                    0
                }
            }
            fn weekday_str(dt: &time::OffsetDateTime) -> &'static str {
                match dt.weekday() {
                    time::Weekday::Monday => "Mon",
                    time::Weekday::Tuesday => "Tue",
                    time::Weekday::Wednesday => "Wed",
                    time::Weekday::Thursday => "Thu",
                    time::Weekday::Friday => "Fri",
                    time::Weekday::Saturday => "Sat",
                    time::Weekday::Sunday => "Sun",
                }
            }
            fn month_str(dt: &time::OffsetDateTime) -> &'static str {
                match dt.month() {
                    time::Month::January => "Jan",
                    time::Month::February => "Feb",
                    time::Month::March => "Mar",
                    time::Month::April => "Apr",
                    time::Month::May => "May",
                    time::Month::June => "Jun",
                    time::Month::July => "Jul",
                    time::Month::August => "Aug",
                    time::Month::September => "Sep",
                    time::Month::October => "Oct",
                    time::Month::November => "Nov",
                    time::Month::December => "Dec",
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
                let Some((ts, offset_str)) = parse_ident_date(ident) else {
                    return String::new();
                };
                let adjusted = ts + parse_tz(offset_str);
                let dt = time::OffsetDateTime::from_unix_timestamp(adjusted)
                    .unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
                format!(
                    "{} {} {} {:02}:{:02}:{:02} {} {}",
                    weekday_str(&dt),
                    month_str(&dt),
                    dt.day(),
                    dt.hour(),
                    dt.minute(),
                    dt.second(),
                    dt.year(),
                    offset_str
                )
            }
            fn extract_date_rfc2822(ident: &str) -> String {
                let Some((ts, offset_str)) = parse_ident_date(ident) else {
                    return String::new();
                };
                let adjusted = ts + parse_tz(offset_str);
                let dt = time::OffsetDateTime::from_unix_timestamp(adjusted)
                    .unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
                format!(
                    "{}, {} {} {} {:02}:{:02}:{:02} {}",
                    weekday_str(&dt),
                    dt.day(),
                    month_str(&dt),
                    dt.year(),
                    dt.hour(),
                    dt.minute(),
                    dt.second(),
                    offset_str
                )
            }
            fn extract_date_short(ident: &str) -> String {
                let Some((ts, offset_str)) = parse_ident_date(ident) else {
                    return String::new();
                };
                let adjusted = ts + parse_tz(offset_str);
                let dt = time::OffsetDateTime::from_unix_timestamp(adjusted)
                    .unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
                format!("{:04}-{:02}-{:02}", dt.year(), dt.month() as u8, dt.day())
            }
            fn extract_date_iso(ident: &str) -> String {
                let Some((ts, offset_str)) = parse_ident_date(ident) else {
                    return String::new();
                };
                let adjusted = ts + parse_tz(offset_str);
                let dt = time::OffsetDateTime::from_unix_timestamp(adjusted)
                    .unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
                format!(
                    "{:04}-{:02}-{:02} {:02}:{:02}:{:02} {}",
                    dt.year(),
                    dt.month() as u8,
                    dt.day(),
                    dt.hour(),
                    dt.minute(),
                    dt.second(),
                    offset_str
                )
            }

            // Alignment/truncation state for %<(N), %>(N), %><(N) directives
            #[derive(Clone, Copy)]
            enum Align {
                Left,
                Right,
                Center,
            }
            #[derive(Clone, Copy)]
            enum Trunc {
                None,
                Trunc,
                LTrunc,
                MTrunc,
            }
            struct ColSpec {
                width: usize,
                align: Align,
                trunc: Trunc,
            }
            fn apply_col(spec: &ColSpec, s: &str) -> String {
                let char_len = s.chars().count();
                if char_len > spec.width {
                    match spec.trunc {
                        Trunc::None => s.to_owned(),
                        Trunc::Trunc => {
                            let mut out: String =
                                s.chars().take(spec.width.saturating_sub(2)).collect();
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
                            for _ in 0..pad {
                                out.push(' ');
                            }
                            out
                        }
                        Align::Right => {
                            let mut out = String::new();
                            for _ in 0..pad {
                                out.push(' ');
                            }
                            out.push_str(s);
                            out
                        }
                        Align::Center => {
                            let left = pad / 2;
                            let right = pad - left;
                            let mut out = String::new();
                            for _ in 0..left {
                                out.push(' ');
                            }
                            out.push_str(s);
                            for _ in 0..right {
                                out.push(' ');
                            }
                            out
                        }
                    }
                }
            }
            fn parse_col_spec(
                chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
                align: Align,
            ) -> Option<ColSpec> {
                // Consume '('
                if chars.peek() != Some(&'(') {
                    return None;
                }
                chars.next();
                let mut num_str = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_ascii_digit() {
                        num_str.push(c);
                        chars.next();
                    } else {
                        break;
                    }
                }
                let width: usize = num_str.parse().ok()?;
                let trunc = if chars.peek() == Some(&',') {
                    chars.next(); // consume comma
                    let mut mode = String::new();
                    while let Some(&c) = chars.peek() {
                        if c == ')' {
                            break;
                        }
                        mode.push(c);
                        chars.next();
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
                if chars.peek() == Some(&')') {
                    chars.next();
                }
                Some(ColSpec {
                    width,
                    align,
                    trunc,
                })
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
                let target = if pending_col.is_some() {
                    &mut expanded
                } else {
                    &mut rendered
                };
                match chars.peek() {
                    Some('%') => {
                        chars.next();
                        target.push('%');
                    }
                    Some('H') => {
                        chars.next();
                        target.push_str(&oid.to_hex());
                    }
                    Some('h') => {
                        chars.next();
                        let hex = oid.to_hex();
                        let n = abbrev_len.clamp(4, 40).min(hex.len());
                        target.push_str(&hex[..n]);
                    }
                    Some('T') => {
                        chars.next();
                        target.push_str(&tree_hex);
                    }
                    Some('t') => {
                        chars.next();
                        let n = abbrev_len.clamp(4, 40).min(tree_hex.len());
                        target.push_str(&tree_hex[..n]);
                    }
                    Some('P') => {
                        chars.next();
                        target.push_str(&parent_hexes.join(" "));
                    }
                    Some('p') => {
                        chars.next();
                        target.push_str(&parent_abbrevs.join(" "));
                    }
                    Some('n') => {
                        chars.next();
                        target.push('\n');
                    }
                    Some('s') => {
                        chars.next();
                        target.push_str(subject);
                    }
                    Some('b') => {
                        chars.next();
                        target.push_str(&body);
                        if !body.is_empty() {
                            target.push('\n');
                        }
                    }
                    Some('B') => {
                        chars.next();
                        target.push_str(&commit.message);
                    }
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
                                let Some((ts, offset_str)) = parse_ident_date(&commit.author)
                                else {
                                    break;
                                };
                                let adjusted = ts + parse_tz(offset_str);
                                let dt = time::OffsetDateTime::from_unix_timestamp(adjusted)
                                    .unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
                                let sign_ch = if parse_tz(offset_str) >= 0 { '+' } else { '-' };
                                let abs_off = parse_tz(offset_str).unsigned_abs();
                                target.push_str(&format!(
                                    "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}{}{:02}:{:02}",
                                    dt.year(),
                                    dt.month() as u8,
                                    dt.day(),
                                    dt.hour(),
                                    dt.minute(),
                                    dt.second(),
                                    sign_ch,
                                    abs_off / 3600,
                                    (abs_off % 3600) / 60
                                ));
                            }
                            Some('r') => {
                                let Some((ts, _)) = parse_ident_date(&commit.author) else {
                                    break;
                                };
                                let now = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_secs() as i64;
                                target.push_str(&format_relative_date(now - ts));
                            }
                            Some(other) => {
                                target.push('%');
                                target.push('a');
                                target.push(other);
                            }
                            None => {
                                target.push('%');
                                target.push('a');
                            }
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
                                let Some((ts, offset_str)) = parse_ident_date(&commit.committer)
                                else {
                                    break;
                                };
                                let adjusted = ts + parse_tz(offset_str);
                                let dt = time::OffsetDateTime::from_unix_timestamp(adjusted)
                                    .unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
                                let sign_ch = if parse_tz(offset_str) >= 0 { '+' } else { '-' };
                                let abs_off = parse_tz(offset_str).unsigned_abs();
                                target.push_str(&format!(
                                    "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}{}{:02}:{:02}",
                                    dt.year(),
                                    dt.month() as u8,
                                    dt.day(),
                                    dt.hour(),
                                    dt.minute(),
                                    dt.second(),
                                    sign_ch,
                                    abs_off / 3600,
                                    (abs_off % 3600) / 60
                                ));
                            }
                            Some('r') => {
                                let Some((ts, _)) = parse_ident_date(&commit.committer) else {
                                    break;
                                };
                                let now = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_secs() as i64;
                                target.push_str(&format_relative_date(now - ts));
                            }
                            Some(other) => {
                                target.push('%');
                                target.push('c');
                                target.push(other);
                            }
                            None => {
                                target.push('%');
                                target.push('c');
                            }
                        }
                    }
                    Some('x') => {
                        // Hex escape: %xNN
                        chars.next();
                        let mut hex_str = String::new();
                        if let Some(&c1) = chars.peek() {
                            if c1.is_ascii_hexdigit() {
                                hex_str.push(c1);
                                chars.next();
                            }
                        }
                        if let Some(&c2) = chars.peek() {
                            if c2.is_ascii_hexdigit() {
                                hex_str.push(c2);
                                chars.next();
                            }
                        }
                        if let Ok(byte) = u8::from_str_radix(&hex_str, 16) {
                            target.push(byte as char);
                        }
                    }
                    Some('C') => {
                        chars.next();
                        if chars.peek() == Some(&'(') {
                            chars.next();
                            let mut spec = String::new();
                            for c in chars.by_ref() {
                                if c == ')' {
                                    break;
                                }
                                spec.push(c);
                            }
                            let (force, color_spec) =
                                if let Some(rest) = spec.strip_prefix("always,") {
                                    (true, rest)
                                } else if let Some(rest) = spec.strip_prefix("auto,") {
                                    (false, rest)
                                } else if spec == "auto" {
                                    if use_color {
                                        target.push_str("\x1b[m");
                                    }
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
                            let known = [
                                "reset", "red", "green", "blue", "yellow", "magenta", "cyan",
                                "white", "bold", "dim", "ul",
                            ];
                            let mut matched = false;
                            for name in &known {
                                if remaining.starts_with(name) {
                                    for _ in 0..name.len() {
                                        chars.next();
                                    }
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
                                    if c.is_alphanumeric() {
                                        chars.next();
                                    } else {
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    Some('w') => {
                        // %w(...) — wrapping directive, consume and ignore for now
                        chars.next();
                        if chars.peek() == Some(&'(') {
                            chars.next();
                            for c in chars.by_ref() {
                                if c == ')' {
                                    break;
                                }
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
                                    if !body.is_empty() {
                                        sub.push('\n');
                                    }
                                }
                                's' => {
                                    chars.next();
                                    sub.push_str(subject);
                                }
                                _ => {
                                    chars.next();
                                    sub.push('%');
                                    sub.push('+');
                                    sub.push(nc);
                                }
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
                                's' => {
                                    chars.next();
                                    target.push_str(subject);
                                }
                                _ => {
                                    chars.next();
                                    target.push('%');
                                    target.push('-');
                                    target.push(nc);
                                }
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExpectedObjectKind {
    Commit,
    Tree,
    Blob,
}

impl ExpectedObjectKind {
    fn from_tag_type(kind: &str) -> Option<Self> {
        match kind {
            "commit" => Some(Self::Commit),
            "tree" => Some(Self::Tree),
            "blob" => Some(Self::Blob),
            _ => None,
        }
    }

    fn matches(self, kind: ObjectKind) -> bool {
        matches!(
            (self, kind),
            (Self::Commit, ObjectKind::Commit)
                | (Self::Tree, ObjectKind::Tree)
                | (Self::Blob, ObjectKind::Blob)
        )
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Commit => "commit",
            Self::Tree => "tree",
            Self::Blob => "blob",
        }
    }
}

#[derive(Clone, Debug)]
struct RootObject {
    oid: ObjectId,
    input: String,
    expected_kind: Option<ExpectedObjectKind>,
    /// Path within the tree for `rev:path` blob roots (for correct `--objects` names).
    root_path: Option<String>,
}

/// Whether `LOFS_COMMIT` would receive `LOFR_DO_SHOW` when the commit is `NOT_USER_GIVEN` (Git list-objects-filter).
fn filter_shows_commit_line_when_not_user_given(filter: Option<&ObjectFilter>) -> bool {
    match filter {
        None => true,
        Some(ObjectFilter::BlobNone)
        | Some(ObjectFilter::BlobLimit(_))
        | Some(ObjectFilter::TreeDepth(_))
        | Some(ObjectFilter::SparseOid(_)) => true,
        Some(ObjectFilter::ObjectType(FilterObjectKind::Commit)) => true,
        Some(ObjectFilter::ObjectType(
            FilterObjectKind::Blob | FilterObjectKind::Tree | FilterObjectKind::Tag,
        )) => false,
        Some(ObjectFilter::Combine(parts)) => parts
            .iter()
            .all(|p| filter_shows_commit_line_when_not_user_given(Some(p))),
    }
}

fn skip_tree_descent_for_object_type_filter(filter: Option<&ObjectFilter>) -> bool {
    match filter {
        Some(ObjectFilter::ObjectType(FilterObjectKind::Commit | FilterObjectKind::Tag)) => true,
        Some(ObjectFilter::Combine(parts)) => parts
            .iter()
            .any(|p| skip_tree_descent_for_object_type_filter(Some(p))),
        _ => false,
    }
}

fn sparse_oid_lines_from_filter(
    repo: &Repository,
    filter: Option<&ObjectFilter>,
) -> Option<Vec<String>> {
    let f = filter?;
    match f {
        ObjectFilter::SparseOid(oid) => {
            let obj = repo.odb.read(oid).ok()?;
            if obj.kind != ObjectKind::Blob {
                return None;
            }
            let text = std::str::from_utf8(&obj.data).ok()?;
            Some(parse_sparse_patterns_from_blob(text))
        }
        ObjectFilter::Combine(parts) => {
            for p in parts {
                if let Some(lines) = sparse_oid_lines_from_filter(repo, Some(p)) {
                    return Some(lines);
                }
            }
            None
        }
        _ => None,
    }
}

fn packed_object_set(repo: &Repository) -> HashSet<ObjectId> {
    let mut out = HashSet::new();
    let objects_dir = repo.odb.objects_dir();
    if let Ok(indexes) = pack::read_local_pack_indexes(objects_dir) {
        for idx in indexes {
            for e in idx.entries {
                out.insert(e.oid);
            }
        }
    }
    out
}

fn resolve_specs(repo: &Repository, specs: &[String]) -> Result<Vec<ObjectId>> {
    let mut out = Vec::with_capacity(specs.len());
    for spec in specs {
        let oid = resolve_revision_for_range_end(repo, spec)?;
        let commit_oid = peel_to_commit(repo, oid)?;
        out.push(commit_oid);
    }
    Ok(out)
}

fn resolve_specs_for_objects(
    repo: &Repository,
    specs: &[String],
) -> Result<(Vec<ObjectId>, Vec<RootObject>)> {
    let mut commits = Vec::new();
    let mut roots = Vec::new();

    for spec in specs {
        if let Ok(raw_oid) = spec.parse::<ObjectId>() {
            let raw_object = repo.odb.read(&raw_oid)?;
            match raw_object.kind {
                ObjectKind::Commit => {
                    commits.push(raw_oid);
                }
                ObjectKind::Tag => {
                    let tag = parse_tag(&raw_object.data)?;
                    let expected_kind = ExpectedObjectKind::from_tag_type(&tag.object_type)
                        .ok_or_else(|| {
                            Error::CorruptObject(format!(
                                "object {spec} has unsupported tag type '{}'",
                                tag.object_type
                            ))
                        })?;
                    roots.push(RootObject {
                        oid: tag.object,
                        input: spec.clone(),
                        expected_kind: Some(expected_kind),
                        root_path: None,
                    });
                }
                ObjectKind::Tree | ObjectKind::Blob => roots.push(RootObject {
                    oid: raw_oid,
                    input: spec.clone(),
                    expected_kind: None,
                    root_path: None,
                }),
            }
            continue;
        }

        if let Some((treeish, path)) = split_treeish_spec(spec) {
            if !path.is_empty() {
                let treeish_oid = resolve_revision_for_range_end(repo, treeish)?;
                let blob_oid = resolve_treeish_path(repo, treeish_oid, path)?;
                roots.push(RootObject {
                    oid: blob_oid,
                    input: spec.clone(),
                    expected_kind: Some(ExpectedObjectKind::Blob),
                    root_path: Some(path.to_owned()),
                });
                continue;
            }
        }

        let oid = resolve_revision_for_range_end(repo, spec)?;
        match peel_to_commit(repo, oid) {
            Ok(commit_oid) => commits.push(commit_oid),
            Err(Error::CorruptObject(_)) | Err(Error::ObjectNotFound(_)) => {
                roots.push(RootObject {
                    oid,
                    input: spec.clone(),
                    expected_kind: None,
                    root_path: None,
                })
            }
            Err(err) => return Err(err),
        }
    }

    Ok((commits, roots))
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
    let mut seen = HashSet::new();
    for oid in raw {
        match peel_to_commit(repo, oid) {
            Ok(commit_oid) if seen.insert(commit_oid) => out.push(commit_oid),
            Err(_) => {} // skip non-commit refs
            _ => {}
        }
    }
    out.sort();
    Ok(out)
}

fn walk_closure(graph: &mut CommitGraph<'_>, starts: &[ObjectId]) -> Result<HashSet<ObjectId>> {
    let (seen, _) = walk_closure_ordered(graph, starts)?;
    Ok(seen)
}

/// BFS walk that returns both the set and the discovery order.
fn walk_closure_ordered(
    graph: &mut CommitGraph<'_>,
    starts: &[ObjectId],
) -> Result<(HashSet<ObjectId>, Vec<ObjectId>)> {
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

/// Git-style default ordering: among commits ready to print, pick the one with the
/// greatest committer timestamp; a parent becomes ready only after all of its
/// children that remain in the walk have been emitted.
///
/// This matches `git rev-list` behavior (and differs from sorting the whole set by
/// date, which can surface ancestors before descendants when dates are skewed).
fn date_order_walk(
    graph: &mut CommitGraph<'_>,
    tips: &[ObjectId],
    selected: &HashSet<ObjectId>,
) -> Result<Vec<ObjectId>> {
    let mut unfinished_children: HashMap<ObjectId, usize> =
        selected.iter().map(|&oid| (oid, 0usize)).collect();
    for &child in selected {
        for parent in graph.parents_of(child)? {
            if selected.contains(&parent) {
                if let Some(count) = unfinished_children.get_mut(&parent) {
                    *count += 1;
                }
            }
        }
    }

    let mut heap = BinaryHeap::new();
    for &tip in tips {
        if selected.contains(&tip) {
            heap.push(CommitDateKey {
                oid: tip,
                date: graph.committer_time(tip),
            });
        }
    }

    let mut emitted = HashSet::new();
    let mut out = Vec::with_capacity(selected.len());
    while let Some(item) = heap.pop() {
        if !emitted.insert(item.oid) {
            continue;
        }
        out.push(item.oid);
        for parent in graph.parents_of(item.oid)? {
            if !selected.contains(&parent) {
                continue;
            }
            let Some(count) = unfinished_children.get_mut(&parent) else {
                continue;
            };
            *count = count.saturating_sub(1);
            if *count == 0 {
                heap.push(CommitDateKey {
                    oid: parent,
                    date: graph.committer_time(parent),
                });
            }
        }
    }

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
    full_history: bool,
    sparse: bool,
) -> Result<bool> {
    let commit = load_commit(repo, oid)?;
    let parents = graph.parents_of(oid)?;
    let commit_entries = flatten_tree(repo, commit.tree, "")?;
    let commit_map: HashMap<String, ObjectId> = commit_entries.into_iter().collect();

    // Root commit: include only when any requested pathspec exists.
    if parents.is_empty() {
        if sparse {
            return Ok(true);
        }
        return Ok(commit_map.keys().any(|path| {
            paths.iter().any(|spec| {
                crate::pathspec::matches_pathspec_with_context(
                    spec,
                    path,
                    crate::pathspec::PathspecMatchContext {
                        is_directory: false,
                        is_git_submodule: false,
                    },
                )
            })
        }));
    }

    // Single-parent commit: include only when requested paths changed.
    if parents.len() == 1 {
        let parent = load_commit(repo, parents[0])?;
        let parent_map: HashMap<String, ObjectId> =
            flatten_tree(repo, parent.tree, "")?.into_iter().collect();
        let differs = path_differs_for_specs(&commit_map, &parent_map, paths);
        if differs {
            return Ok(true);
        }
        if sparse {
            return Ok(true);
        }
        return Ok(false);
    }

    // Merge commit simplification for default dense history:
    // if exactly one parent is TREESAME for the requested paths, omit this
    // merge commit and let traversal effectively follow that parent.
    let mut treesame_parents = 0usize;
    let mut differs_any = false;
    for parent_oid in &parents {
        let parent = load_commit(repo, *parent_oid)?;
        let parent_map: HashMap<String, ObjectId> =
            flatten_tree(repo, parent.tree, "")?.into_iter().collect();
        let differs = path_differs_for_specs(&commit_map, &parent_map, paths);
        if differs {
            differs_any = true;
        } else {
            treesame_parents += 1;
        }
    }

    if !full_history && treesame_parents == 1 {
        return Ok(false);
    }

    if differs_any {
        return Ok(true);
    }

    Ok(sparse)
}

fn path_differs_for_specs(
    current: &HashMap<String, ObjectId>,
    parent: &HashMap<String, ObjectId>,
    specs: &[String],
) -> bool {
    let mut paths = std::collections::BTreeSet::new();
    paths.extend(current.keys().cloned());
    paths.extend(parent.keys().cloned());

    for path in &paths {
        if !specs
            .iter()
            .any(|spec| crate::pathspec::matches_pathspec(spec, path))
        {
            continue;
        }
        if current.get(path) != parent.get(path) {
            return true;
        }
    }
    false
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
    graft_parents: HashMap<ObjectId, Vec<ObjectId>>,
}

impl<'r> CommitGraph<'r> {
    fn new(repo: &'r Repository, first_parent_only: bool) -> Self {
        let shallow_boundaries = load_shallow_boundaries(&repo.git_dir);
        let graft_parents = load_graft_parents(&repo.git_dir);
        Self {
            repo,
            first_parent_only,
            parents: HashMap::new(),
            committer_time: HashMap::new(),
            shallow_boundaries,
            graft_parents,
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
        if let Some(graft_parents) = self.graft_parents.get(&oid) {
            parents = graft_parents.clone();
        }
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

/// Load commit parent overrides from `.git/info/grafts`.
fn load_graft_parents(git_dir: &Path) -> HashMap<ObjectId, Vec<ObjectId>> {
    let graft_path = git_dir.join("info/grafts");
    let mut grafts = HashMap::new();
    let Ok(contents) = fs::read_to_string(&graft_path) else {
        return grafts;
    };
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
    token
        .split_once("...")
        .map(|(l, r)| (l.to_owned(), r.to_owned()))
}

/// Tracks which tree OIDs have been traversed, matching Git list-objects behavior.
///
/// For `tree:<n>` filters, the same tree OID may be revisited from a shallower path; otherwise
/// each tree is walked at most once per top-level walk.
#[derive(Debug)]
enum TreeWalkState {
    Simple(HashSet<ObjectId>),
    TreeDepth(HashMap<ObjectId, u64>),
}

impl TreeWalkState {
    fn new(filter: Option<&ObjectFilter>) -> Self {
        if filter_uses_tree_depth(filter) {
            Self::TreeDepth(HashMap::new())
        } else {
            Self::Simple(HashSet::new())
        }
    }

    /// Returns `true` if this tree at `depth` should be skipped (already handled sufficiently).
    fn should_skip_tree(&mut self, oid: ObjectId, depth: u64) -> bool {
        match self {
            TreeWalkState::Simple(set) => !set.insert(oid),
            TreeWalkState::TreeDepth(map) => match map.get(&oid).copied() {
                None => {
                    map.insert(oid, depth);
                    false
                }
                Some(prev) if depth >= prev => true,
                Some(_) => {
                    map.insert(oid, depth);
                    false
                }
            },
        }
    }
}

fn filter_uses_tree_depth(filter: Option<&ObjectFilter>) -> bool {
    match filter {
        Some(ObjectFilter::TreeDepth(_)) => true,
        Some(ObjectFilter::Combine(parts)) => parts.iter().any(|p| filter_uses_tree_depth(Some(p))),
        _ => false,
    }
}

/// All tree and blob OIDs reachable from `tree_oid` (including `tree_oid` itself).
fn collect_tree_closure_objects(
    repo: &Repository,
    tree_oid: ObjectId,
    into: &mut HashSet<ObjectId>,
    missing_action: MissingAction,
    missing: &mut Vec<ObjectId>,
    missing_seen: &mut HashSet<ObjectId>,
) -> Result<()> {
    let mut stack = vec![tree_oid];
    let mut expanded_trees = HashSet::new();
    while let Some(t) = stack.pop() {
        if !expanded_trees.insert(t) {
            continue;
        }
        into.insert(t);
        let object = match repo.odb.read(&t) {
            Ok(o) => o,
            Err(Error::ObjectNotFound(_)) if missing_action != MissingAction::Error => {
                if missing_action == MissingAction::Print && missing_seen.insert(t) {
                    missing.push(t);
                }
                continue;
            }
            Err(e) => return Err(e),
        };
        if object.kind != ObjectKind::Tree {
            continue;
        }
        let entries = parse_tree(&object.data)?;
        for entry in entries {
            if entry.mode == 0o160000 {
                continue;
            }
            into.insert(entry.oid);
            if entry.mode == 0o040000 {
                stack.push(entry.oid);
            }
        }
    }
    Ok(())
}

fn union_parent_reachable_objects(
    repo: &Repository,
    parents: &[ObjectId],
    missing_action: MissingAction,
    missing: &mut Vec<ObjectId>,
    missing_seen: &mut HashSet<ObjectId>,
) -> Result<HashSet<ObjectId>> {
    let mut out = HashSet::new();
    for &p in parents {
        let commit = match load_commit(repo, p) {
            Ok(c) => c,
            Err(Error::ObjectNotFound(_)) if missing_action != MissingAction::Error => {
                if missing_action == MissingAction::Print && missing_seen.insert(p) {
                    missing.push(p);
                }
                continue;
            }
            Err(e) => return Err(e),
        };
        collect_tree_closure_objects(
            repo,
            commit.tree,
            &mut out,
            missing_action,
            missing,
            missing_seen,
        )?;
    }
    Ok(out)
}

/// Collect all reachable non-commit objects (trees and blobs) from a set of commits.
/// Returns (included, omitted) object lists.
#[allow(dead_code)]
fn collect_reachable_objects(
    repo: &Repository,
    graph: &mut CommitGraph<'_>,
    commits: &[ObjectId],
    object_roots: &[RootObject],
    filter: Option<&ObjectFilter>,
    filter_provided: bool,
    missing_action: MissingAction,
    sparse_lines: Option<&[String]>,
    skip_trees_for_type_filter: bool,
    omit_object_paths: bool,
    packed_set: Option<&HashSet<ObjectId>>,
) -> Result<(Vec<(ObjectId, String)>, Vec<ObjectId>, Vec<ObjectId>)> {
    let mut tree_state = TreeWalkState::new(filter);
    let mut emitted = HashSet::new();
    let mut result = Vec::new();
    let mut omitted = Vec::new();
    let mut missing = Vec::new();
    let mut missing_seen = HashSet::new();
    for &commit_oid in commits {
        let commit = match load_commit(repo, commit_oid) {
            Ok(commit) => commit,
            Err(Error::ObjectNotFound(_)) if missing_action != MissingAction::Error => {
                if missing_seen.insert(commit_oid) && missing_action == MissingAction::Print {
                    missing.push(commit_oid);
                }
                continue;
            }
            Err(err) => return Err(err),
        };
        let parents = graph.parents_of(commit_oid)?;
        let parent_union = union_parent_reachable_objects(
            repo,
            &parents,
            missing_action,
            &mut missing,
            &mut missing_seen,
        )?;
        collect_tree_objects_filtered(
            repo,
            commit.tree,
            "",
            0,
            false,
            Some(&parent_union),
            &mut tree_state,
            &mut emitted,
            &mut result,
            &mut omitted,
            &mut missing,
            &mut missing_seen,
            filter,
            filter_provided,
            missing_action,
            sparse_lines,
            skip_trees_for_type_filter,
            omit_object_paths,
            packed_set,
        )?;
    }

    for root in object_roots {
        collect_root_object(
            repo,
            root,
            &mut tree_state,
            &mut emitted,
            &mut result,
            &mut omitted,
            &mut missing,
            &mut missing_seen,
            filter,
            filter_provided,
            missing_action,
            sparse_lines,
            skip_trees_for_type_filter,
            omit_object_paths,
            packed_set,
        )?;
    }

    Ok((result, omitted, missing))
}

/// Like [`collect_reachable_objects`], but also returns objects newly discovered per commit walk
/// plus one trailing segment for `object_roots`.
///
/// Matches Git `traverse_commit_list_filtered`: each commit's tree is processed before moving to
/// the next commit, with global de-duplication of emitted object OIDs across the full walk.
fn collect_reachable_objects_segmented(
    repo: &Repository,
    graph: &mut CommitGraph<'_>,
    commits: &[ObjectId],
    object_roots: &[RootObject],
    filter: Option<&ObjectFilter>,
    filter_provided: bool,
    missing_action: MissingAction,
    sparse_lines: Option<&[String]>,
    skip_trees_for_type_filter: bool,
    omit_object_paths: bool,
    packed_set: Option<&HashSet<ObjectId>>,
) -> Result<(
    Vec<(ObjectId, String)>,
    Vec<ObjectId>,
    Vec<ObjectId>,
    Vec<Vec<(ObjectId, String)>>,
)> {
    let mut emitted = HashSet::new();
    let mut result = Vec::new();
    let mut omitted = Vec::new();
    let mut missing = Vec::new();
    let mut missing_seen = HashSet::new();
    let mut segments: Vec<Vec<(ObjectId, String)>> = Vec::with_capacity(commits.len() + 1);

    for &commit_oid in commits {
        let start = result.len();
        let commit = match load_commit(repo, commit_oid) {
            Ok(commit) => commit,
            Err(Error::ObjectNotFound(_)) if missing_action != MissingAction::Error => {
                if missing_action == MissingAction::Print && missing_seen.insert(commit_oid) {
                    missing.push(commit_oid);
                }
                segments.push(Vec::new());
                continue;
            }
            Err(err) => return Err(err),
        };
        let mut tree_state = TreeWalkState::new(filter);
        let parents = graph.parents_of(commit_oid)?;
        let parent_union = union_parent_reachable_objects(
            repo,
            &parents,
            missing_action,
            &mut missing,
            &mut missing_seen,
        )?;
        collect_tree_objects_filtered(
            repo,
            commit.tree,
            "",
            0,
            false,
            Some(&parent_union),
            &mut tree_state,
            &mut emitted,
            &mut result,
            &mut omitted,
            &mut missing,
            &mut missing_seen,
            filter,
            filter_provided,
            missing_action,
            sparse_lines,
            skip_trees_for_type_filter,
            omit_object_paths,
            packed_set,
        )?;
        segments.push(result[start..].to_vec());
    }

    let roots_start = result.len();
    let mut root_tree_state = TreeWalkState::new(filter);
    for root in object_roots {
        collect_root_object(
            repo,
            root,
            &mut root_tree_state,
            &mut emitted,
            &mut result,
            &mut omitted,
            &mut missing,
            &mut missing_seen,
            filter,
            filter_provided,
            missing_action,
            sparse_lines,
            skip_trees_for_type_filter,
            omit_object_paths,
            packed_set,
        )?;
    }
    segments.push(result[roots_start..].to_vec());

    Ok((result, omitted, missing, segments))
}

fn collect_root_object(
    repo: &Repository,
    root: &RootObject,
    tree_state: &mut TreeWalkState,
    emitted: &mut HashSet<ObjectId>,
    result: &mut Vec<(ObjectId, String)>,
    omitted: &mut Vec<ObjectId>,
    missing: &mut Vec<ObjectId>,
    missing_seen: &mut HashSet<ObjectId>,
    filter: Option<&ObjectFilter>,
    filter_provided: bool,
    missing_action: MissingAction,
    sparse_lines: Option<&[String]>,
    skip_trees_for_type_filter: bool,
    omit_object_paths: bool,
    packed_set: Option<&HashSet<ObjectId>>,
) -> Result<()> {
    let object = match repo.odb.read(&root.oid) {
        Ok(object) => object,
        Err(Error::ObjectNotFound(_)) if missing_action != MissingAction::Error => {
            if missing_action == MissingAction::Print && missing_seen.insert(root.oid) {
                missing.push(root.oid);
            }
            return Ok(());
        }
        Err(err) => return Err(err),
    };

    if let Some(expected) = root.expected_kind {
        if !expected.matches(object.kind) {
            return Err(Error::CorruptObject(format!(
                "object {} is not a {}",
                root.input,
                expected.as_str()
            )));
        }
    }

    match object.kind {
        ObjectKind::Commit => {
            let commit = parse_commit(&object.data)?;
            let parent_union = union_parent_reachable_objects(
                repo,
                &commit.parents,
                missing_action,
                missing,
                missing_seen,
            )?;
            collect_tree_objects_filtered(
                repo,
                commit.tree,
                "",
                0,
                false,
                Some(&parent_union),
                tree_state,
                emitted,
                result,
                omitted,
                missing,
                missing_seen,
                filter,
                filter_provided,
                missing_action,
                sparse_lines,
                skip_trees_for_type_filter,
                omit_object_paths,
                packed_set,
            )?;
        }
        ObjectKind::Tree => {
            collect_tree_objects_filtered(
                repo,
                root.oid,
                "",
                0,
                true,
                None,
                tree_state,
                emitted,
                result,
                omitted,
                missing,
                missing_seen,
                filter,
                filter_provided,
                missing_action,
                sparse_lines,
                skip_trees_for_type_filter,
                omit_object_paths,
                packed_set,
            )?;
        }
        ObjectKind::Blob => {
            let path_for_sparse = root.root_path.as_deref().unwrap_or("");
            let blob_included = match filter {
                None => true,
                Some(f) => {
                    if !filter_provided {
                        true
                    } else {
                        f.includes_blob_under_tree(object.data.len() as u64, 0)
                    }
                }
            };
            let blob_included = blob_included
                && sparse_lines.is_none_or(|lines| {
                    path_matches_sparse_pattern_list(path_for_sparse, lines) != Some(false)
                });
            if !blob_included {
                omitted.push(root.oid);
                return Ok(());
            }
            if packed_set.is_some_and(|p| p.contains(&root.oid)) {
                return Ok(());
            }
            if !emitted.insert(root.oid) {
                return Ok(());
            }
            let out_path = if omit_object_paths {
                String::new()
            } else {
                path_for_sparse.to_owned()
            };
            result.push((root.oid, out_path));
        }
        ObjectKind::Tag => {
            let tag = parse_tag(&object.data)?;
            let expected_kind =
                ExpectedObjectKind::from_tag_type(&tag.object_type).ok_or_else(|| {
                    Error::CorruptObject(format!(
                        "object {} has unsupported tag type '{}'",
                        root.input, tag.object_type
                    ))
                })?;
            let nested = RootObject {
                oid: tag.object,
                input: root.input.clone(),
                expected_kind: Some(expected_kind),
                root_path: None,
            };
            collect_root_object(
                repo,
                &nested,
                tree_state,
                emitted,
                result,
                omitted,
                missing,
                missing_seen,
                filter,
                filter_provided,
                missing_action,
                sparse_lines,
                skip_trees_for_type_filter,
                omit_object_paths,
                packed_set,
            )?;
        }
    }

    Ok(())
}

#[allow(dead_code)]
fn collect_tree_objects_filtered(
    repo: &Repository,
    tree_oid: ObjectId,
    prefix: &str,
    depth: u64,
    explicit_root: bool,
    parent_union: Option<&HashSet<ObjectId>>,
    tree_state: &mut TreeWalkState,
    emitted: &mut HashSet<ObjectId>,
    result: &mut Vec<(ObjectId, String)>,
    omitted: &mut Vec<ObjectId>,
    missing: &mut Vec<ObjectId>,
    missing_seen: &mut HashSet<ObjectId>,
    filter: Option<&ObjectFilter>,
    filter_provided: bool,
    missing_action: MissingAction,
    sparse_lines: Option<&[String]>,
    skip_trees_for_type_filter: bool,
    omit_object_paths: bool,
    packed_set: Option<&HashSet<ObjectId>>,
) -> Result<()> {
    if tree_state.should_skip_tree(tree_oid, depth) {
        return Ok(());
    }
    if !explicit_root {
        if let Some(pu) = parent_union {
            if pu.contains(&tree_oid) {
                return Ok(());
            }
        }
    }
    let object = match repo.odb.read(&tree_oid) {
        Ok(object) => object,
        Err(Error::ObjectNotFound(_)) if missing_action != MissingAction::Error => {
            if missing_action == MissingAction::Print && missing_seen.insert(tree_oid) {
                missing.push(tree_oid);
            }
            return Ok(());
        }
        Err(err) => return Err(err),
    };
    if object.kind != ObjectKind::Tree {
        return Err(Error::CorruptObject(format!(
            "object {tree_oid} is not a tree"
        )));
    }
    let tree_included = match filter {
        None => true,
        Some(f) => {
            if explicit_root && !filter_provided {
                true
            } else {
                f.includes_tree(depth)
            }
        }
    };
    let tree_included = tree_included
        && sparse_lines
            .is_none_or(|lines| path_matches_sparse_pattern_list(prefix, lines) != Some(false));
    if tree_included {
        if !packed_set.is_some_and(|p| p.contains(&tree_oid)) && emitted.insert(tree_oid) {
            let out_path = if omit_object_paths {
                String::new()
            } else {
                prefix.to_owned()
            };
            result.push((tree_oid, out_path));
        }
    } else {
        omitted.push(tree_oid);
    }
    if skip_trees_for_type_filter && depth == 0 && !explicit_root {
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
        let child_obj = match repo.odb.read(&entry.oid) {
            Ok(object) => object,
            Err(Error::ObjectNotFound(_)) if missing_action != MissingAction::Error => {
                if missing_action == MissingAction::Print && missing_seen.insert(entry.oid) {
                    missing.push(entry.oid);
                }
                continue;
            }
            Err(err) => return Err(err),
        };
        if entry.mode == 0o040000 {
            if child_obj.kind != ObjectKind::Tree {
                return Err(Error::CorruptObject(format!(
                    "object {} is not a tree",
                    entry.oid
                )));
            }
            if let Some(pu) = parent_union {
                if pu.contains(&entry.oid) {
                    continue;
                }
            }
            let child_tree_depth = depth + 1;
            collect_tree_objects_filtered(
                repo,
                entry.oid,
                &path,
                child_tree_depth,
                false,
                parent_union,
                tree_state,
                emitted,
                result,
                omitted,
                missing,
                missing_seen,
                filter,
                filter_provided,
                missing_action,
                sparse_lines,
                skip_trees_for_type_filter,
                omit_object_paths,
                packed_set,
            )?;
        } else {
            if let Some(pu) = parent_union {
                if pu.contains(&entry.oid) {
                    continue;
                }
            }
            if child_obj.kind == ObjectKind::Blob {
                let blob_included = match filter {
                    None => true,
                    Some(f) => {
                        if explicit_root && !filter_provided {
                            true
                        } else {
                            f.includes_blob_under_tree(child_obj.data.len() as u64, depth)
                        }
                    }
                };
                let blob_included = blob_included
                    && sparse_lines.is_none_or(|lines| {
                        path_matches_sparse_pattern_list(&path, lines) != Some(false)
                    });
                if blob_included {
                    if !packed_set.is_some_and(|p| p.contains(&entry.oid))
                        && emitted.insert(entry.oid)
                    {
                        let out_path = if omit_object_paths {
                            String::new()
                        } else {
                            path.clone()
                        };
                        result.push((entry.oid, out_path));
                    }
                } else {
                    omitted.push(entry.oid);
                }
            } else {
                // Git historically tolerates lone non-blob entries in blob slots.
                if emitted.insert(entry.oid) {
                    result.push((entry.oid, path));
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
    graph: &mut CommitGraph<'_>,
    commits: &[ObjectId],
    object_roots: &[RootObject],
    filter: Option<&ObjectFilter>,
    filter_provided: bool,
    missing_action: MissingAction,
    sparse_lines: Option<&[String]>,
    skip_trees_for_type_filter: bool,
    omit_object_paths: bool,
    packed_set: Option<&HashSet<ObjectId>>,
) -> Result<(
    Vec<(ObjectId, String)>,
    Vec<ObjectId>,
    Vec<ObjectId>,
    Vec<usize>,
)> {
    let mut tree_state = TreeWalkState::new(filter);
    let mut emitted = HashSet::new();
    let mut result = Vec::new();
    let mut omitted = Vec::new();
    let mut missing = Vec::new();
    let mut missing_seen = HashSet::new();
    let mut counts = Vec::with_capacity(commits.len());
    for &commit_oid in commits {
        let commit = match load_commit(repo, commit_oid) {
            Ok(commit) => commit,
            Err(Error::ObjectNotFound(_)) if missing_action != MissingAction::Error => {
                if missing_action == MissingAction::Print && missing_seen.insert(commit_oid) {
                    missing.push(commit_oid);
                }
                counts.push(0);
                continue;
            }
            Err(err) => return Err(err),
        };
        let before = result.len();
        let parents = graph.parents_of(commit_oid)?;
        let parent_union = union_parent_reachable_objects(
            repo,
            &parents,
            missing_action,
            &mut missing,
            &mut missing_seen,
        )?;
        collect_tree_objects_filtered(
            repo,
            commit.tree,
            "",
            0,
            false,
            Some(&parent_union),
            &mut tree_state,
            &mut emitted,
            &mut result,
            &mut omitted,
            &mut missing,
            &mut missing_seen,
            filter,
            filter_provided,
            missing_action,
            sparse_lines,
            skip_trees_for_type_filter,
            omit_object_paths,
            packed_set,
        )?;
        counts.push(result.len() - before);
    }

    for root in object_roots {
        collect_root_object(
            repo,
            root,
            &mut tree_state,
            &mut emitted,
            &mut result,
            &mut omitted,
            &mut missing,
            &mut missing_seen,
            filter,
            filter_provided,
            missing_action,
            sparse_lines,
            skip_trees_for_type_filter,
            omit_object_paths,
            packed_set,
        )?;
    }

    Ok((result, omitted, missing, counts))
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
        if path.extension().is_some_and(|ext| ext == "keep") {
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
        let child = match repo.odb.read(&entry.oid) {
            Ok(o) => o,
            Err(Error::ObjectNotFound(_)) => continue,
            Err(err) => return Err(err),
        };
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
            if other == c {
                return false;
            }
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
