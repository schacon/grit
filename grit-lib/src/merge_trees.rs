//! Rename-aware three-way tree merge for cherry-pick / revert style merges.
//!
//! Flattens trees to path → [`IndexEntry`] maps, aligns paths using rename detection
//! between base↔ours and base↔theirs (same idea as Git's merge-ort rename paths), then
//! runs path-by-path three-way rules with optional content merge.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::diff::{detect_renames, diff_trees, DiffStatus};
use crate::index::{Index, IndexEntry};
use crate::merge_file::{merge, ConflictStyle, MergeFavor, MergeInput};
use crate::objects::{parse_tree, ObjectId, ObjectKind};
use crate::odb::Odb;
use crate::repo::Repository;
use crate::write_tree::write_tree_from_index;

/// Result of merging three trees with optional conflict-marker blobs for checkout.
#[derive(Debug)]
pub struct TreeMergeOutput {
    /// Merged index (may include unmerged stages).
    pub index: Index,
    /// Conflict-marker blob OIDs keyed by path (stage-2 path bytes).
    pub conflict_content: BTreeMap<Vec<u8>, ObjectId>,
}

#[derive(Clone, Copy, Debug, Default)]
/// `WhitespaceMergeOptions` struct.
pub struct WhitespaceMergeOptions {
    /// `ignore_all_space` field.
    pub ignore_all_space: bool,
    /// `ignore_space_change` field.
    pub ignore_space_change: bool,
    /// `ignore_space_at_eol` field.
    pub ignore_space_at_eol: bool,
    /// `ignore_cr_at_eol` field.
    pub ignore_cr_at_eol: bool,
}

/// How to label the "theirs" side in textual conflict markers.
#[derive(Clone, Copy, Debug)]
pub enum TheirsConflictLabel<'a> {
    /// Use the UTF-8 file path (matches cherry-pick / revert).
    PathUtf8,
    /// Fixed label (e.g. `local` for `git checkout -m`).
    Fixed(&'a str),
}

/// Labels and marker style for conflict output during tree merges.
#[derive(Clone, Copy, Debug)]
pub struct TreeMergeConflictPresentation<'a> {
    /// Label after `<<<<<<<` (Git: ours side name).
    pub label_ours: &'a str,
    /// Label after `>>>>>>>` (path-based or fixed).
    pub label_theirs: TheirsConflictLabel<'a>,
    /// Label after `|||||||` in diff3 output.
    pub label_base: &'a str,
    /// Two-way vs diff3 markers.
    pub style: ConflictStyle,
    /// `git checkout -m`: always leave unmerged index entries when ours ≠ theirs.
    pub checkout_merge: bool,
    /// Ours-side commit oid for `path~<commit>` when relocating a blocking **ours** file.
    pub df_directory_ours_commit: Option<ObjectId>,
    /// Theirs-side commit oid for `path~<commit>` when relocating a blocking **theirs** file.
    pub df_directory_theirs_commit: Option<ObjectId>,
}

impl Default for TreeMergeConflictPresentation<'_> {
    fn default() -> Self {
        Self {
            label_ours: "HEAD",
            label_theirs: TheirsConflictLabel::PathUtf8,
            label_base: "merged common ancestors",
            style: ConflictStyle::Merge,
            checkout_merge: false,
            df_directory_ours_commit: None,
            df_directory_theirs_commit: None,
        }
    }
}
#[expect(clippy::too_many_arguments)]
/// Merge `base` + `ours` + `their` trees (by OID) into an index using rename detection.
///
/// Paths in the result follow the **ours** tree naming. Rename detection uses a 50%
/// similarity threshold, matching Git's default rename detection during merges.
///
/// # Parameters
///
/// - `base_tree` — common ancestor tree (parent tree for cherry-pick, reverted commit tree for revert).
/// - `ours_tree` — current branch tree (HEAD during pick/revert).
/// - `theirs_tree` — tree being applied (picked commit for cherry-pick, parent of reverted commit for revert).
/// - `favor` / `ws` — merge strategy favour and whitespace options for textual merges.
/// - `diff_algorithm` — optional line diff algorithm name used for textual merges.
/// - `presentation` — conflict marker labels and style (`label_base` is the diff3 "base" name).
///
/// # Errors
///
/// Returns [`crate::error::Error`] on ODB read failures or corrupt trees.
pub fn merge_trees_three_way(
    repo: &Repository,
    base_tree: ObjectId,
    ours_tree: ObjectId,
    theirs_tree: ObjectId,
    favor: MergeFavor,
    ws: WhitespaceMergeOptions,
    diff_algorithm: Option<&str>,
    presentation: TreeMergeConflictPresentation<'_>,
) -> crate::error::Result<TreeMergeOutput> {
    let odb = &repo.odb();
    let base = tree_to_map(tree_to_index_entries(repo, &base_tree, "")?);
    let mut ours = tree_to_map(tree_to_index_entries(repo, &ours_tree, "")?);
    let mut theirs = tree_to_map(tree_to_index_entries(repo, &theirs_tree, "")?);

    let mut ours_old_to_new_pairs = rename_pairs_base_to_other(odb, &base_tree, &ours_tree)?;
    augment_renames_with_same_blob(&base, &ours, &mut ours_old_to_new_pairs);
    let mut theirs_pairs = rename_pairs_base_to_other(odb, &base_tree, &theirs_tree)?;
    augment_renames_with_same_blob(&base, &theirs, &mut theirs_pairs);

    let mut df_escapes = DfEscapes::default();
    if let (Some(ours_commit), Some(theirs_commit)) = (
        presentation.df_directory_ours_commit,
        presentation.df_directory_theirs_commit,
    ) {
        df_escapes = apply_df_file_directory_conflicts(
            &base,
            &mut ours,
            &mut theirs,
            ours_commit,
            theirs_commit,
        );
    }

    let mut ours_new_to_old: HashMap<Vec<u8>, Vec<u8>> = HashMap::new();
    let mut ours_best_by_dest: HashMap<Vec<u8>, (Vec<u8>, u32)> = HashMap::new();
    for (old, new, score) in &ours_old_to_new_pairs {
        let new_b = new.clone();
        let should_take = match ours_best_by_dest.get(&new_b) {
            None => true,
            Some((_, s)) => *score > *s,
        };
        if should_take {
            ours_best_by_dest.insert(new_b, (old.clone(), *score));
        }
    }
    for (new_path, (old_path, _)) in ours_best_by_dest {
        ours_new_to_old.insert(new_path, old_path);
    }
    let mut theirs_old_to_new: HashMap<Vec<u8>, Vec<u8>> = HashMap::new();
    let mut best_by_dest: HashMap<Vec<u8>, (Vec<u8>, u32)> = HashMap::new();
    for (old, new, score) in theirs_pairs {
        let new_b = new.clone();
        let should_take = match best_by_dest.get(&new_b) {
            None => true,
            Some((_, s)) => score > *s,
        };
        if should_take {
            best_by_dest.insert(new_b, (old.clone(), score));
        }
    }
    for (new_path, (old_path, _)) in best_by_dest {
        theirs_old_to_new.insert(old_path, new_path);
    }

    let mut output = three_way_on_aligned_paths(
        repo,
        &base,
        &ours,
        &theirs,
        &ours_new_to_old,
        &theirs_old_to_new,
        favor,
        ws,
        diff_algorithm,
        presentation,
    )?;
    for (path, entry) in df_escapes.stage2 {
        output.index.remove_path_all_stages(&path);
        stage_entry(&mut output.index, &path, &entry, 2);
    }
    for (path, entry) in df_escapes.stage3 {
        output.index.remove_path_all_stages(&path);
        stage_entry(&mut output.index, &path, &entry, 3);
    }
    output.index.sort();
    Ok(output)
}

fn augment_renames_with_same_blob(
    base: &HashMap<Vec<u8>, IndexEntry>,
    side: &HashMap<Vec<u8>, IndexEntry>,
    pairs: &mut Vec<RenamePathPair>,
) {
    use crate::porcelain::cherry_pick::same_blob_renames;
    for (old, new) in same_blob_renames(base, side) {
        if pairs
            .iter()
            .any(|(o, n, _)| o.as_slice() == old.as_slice() && n.as_slice() == new.as_slice())
        {
            continue;
        }
        pairs.push((old, new, 100));
    }
}

#[derive(Default)]
struct DfEscapes {
    stage2: BTreeMap<Vec<u8>, IndexEntry>,
    stage3: BTreeMap<Vec<u8>, IndexEntry>,
}

fn side_has_descendant(side: &HashMap<Vec<u8>, IndexEntry>, prefix: &[u8]) -> bool {
    side.keys()
        .any(|p| p.len() > prefix.len() && p.starts_with(prefix) && p[prefix.len()] == b'/')
}

/// Git skips D/F relocation when one side only expanded a base file into `path/…`.
fn skip_df_expansion_ours_file(
    base: &HashMap<Vec<u8>, IndexEntry>,
    ours: &HashMap<Vec<u8>, IndexEntry>,
    theirs: &HashMap<Vec<u8>, IndexEntry>,
    path: &[u8],
) -> bool {
    let Some(base_e) = base.get(path) else {
        return false;
    };
    let Some(ours_e) = ours.get(path) else {
        return false;
    };
    if base_e.mode == 0o040000 {
        return false;
    }
    if !same_blob(base_e, ours_e) {
        return false;
    }
    if theirs.contains_key(path) {
        return false;
    }
    side_has_descendant(theirs, path)
}

fn skip_df_expansion_theirs_file(
    base: &HashMap<Vec<u8>, IndexEntry>,
    ours: &HashMap<Vec<u8>, IndexEntry>,
    theirs: &HashMap<Vec<u8>, IndexEntry>,
    path: &[u8],
) -> bool {
    let Some(base_e) = base.get(path) else {
        return false;
    };
    let Some(theirs_e) = theirs.get(path) else {
        return false;
    };
    if base_e.mode == 0o040000 {
        return false;
    }
    if !same_blob(base_e, theirs_e) {
        return false;
    }
    if ours.contains_key(path) {
        return false;
    }
    side_has_descendant(ours, path)
}

fn parent_has_file(side: &HashMap<Vec<u8>, IndexEntry>, path: &[u8]) -> bool {
    let Some(pos) = path.iter().rposition(|b| *b == b'/') else {
        return false;
    };
    side.get(&path[..pos]).is_some_and(|e| e.mode != 0o040000)
}

fn escape_df_path(
    side: &mut HashMap<Vec<u8>, IndexEntry>,
    path: &[u8],
    side_commit: ObjectId,
    staged: &mut BTreeMap<Vec<u8>, IndexEntry>,
) {
    let Some(mut entry) = side.remove(path) else {
        return;
    };
    let hex = side_commit.to_hex();
    let mut escaped_path = path.to_vec();
    escaped_path.extend_from_slice(b"~");
    escaped_path.extend_from_slice(hex.as_bytes());
    entry.path = escaped_path.clone();
    entry.flags = path_len_flags(&escaped_path);
    staged.insert(escaped_path.clone(), entry.clone());
    side.insert(escaped_path, entry);
}

/// File/directory conflicts: relocate blocking files to `path~<ours-commit>` before path merge.
fn apply_df_file_directory_conflicts(
    base: &HashMap<Vec<u8>, IndexEntry>,
    ours: &mut HashMap<Vec<u8>, IndexEntry>,
    theirs: &mut HashMap<Vec<u8>, IndexEntry>,
    ours_commit: ObjectId,
    theirs_commit: ObjectId,
) -> DfEscapes {
    let mut out = DfEscapes::default();
    for path in ours.keys().cloned().collect::<Vec<_>>() {
        let Some(entry) = ours.get(&path) else {
            continue;
        };
        if entry.mode == 0o040000 {
            continue;
        }
        if !side_has_descendant(theirs, &path) {
            continue;
        }
        if skip_df_expansion_ours_file(base, ours, theirs, &path) {
            continue;
        }
        if parent_has_file(theirs, &path) {
            continue;
        }
        escape_df_path(ours, &path, ours_commit, &mut out.stage2);
    }
    for path in theirs.keys().cloned().collect::<Vec<_>>() {
        let Some(entry) = theirs.get(&path) else {
            continue;
        };
        if entry.mode == 0o040000 {
            continue;
        }
        if !side_has_descendant(ours, &path) {
            continue;
        }
        if skip_df_expansion_theirs_file(base, ours, theirs, &path) {
            continue;
        }
        if parent_has_file(ours, &path) {
            continue;
        }
        escape_df_path(theirs, &path, theirs_commit, &mut out.stage3);
    }
    out
}

type RenamePathPair = (Vec<u8>, Vec<u8>, u32);

fn rename_pairs_base_to_other(
    odb: &Odb,
    base_tree: &ObjectId,
    other_tree: &ObjectId,
) -> crate::error::Result<Vec<RenamePathPair>> {
    let mut entries = diff_trees(odb, Some(base_tree), Some(other_tree), "")?;
    entries = detect_renames(odb, None, entries, 50);
    let mut out = Vec::new();
    for e in entries {
        if e.status != DiffStatus::Renamed {
            continue;
        }
        let Some(old) = e.old_path else {
            continue;
        };
        let Some(new) = e.new_path else {
            continue;
        };
        let score = e.score.unwrap_or(0);
        out.push((old.into_bytes(), new.into_bytes(), score));
    }
    Ok(out)
}
#[expect(clippy::too_many_arguments)]
fn three_way_on_aligned_paths(
    repo: &Repository,
    base: &HashMap<Vec<u8>, IndexEntry>,
    ours: &HashMap<Vec<u8>, IndexEntry>,
    theirs: &HashMap<Vec<u8>, IndexEntry>,
    ours_new_to_old: &HashMap<Vec<u8>, Vec<u8>>,
    theirs_old_to_new: &HashMap<Vec<u8>, Vec<u8>>,
    favor: MergeFavor,
    ws: WhitespaceMergeOptions,
    diff_algorithm: Option<&str>,
    presentation: TreeMergeConflictPresentation<'_>,
) -> crate::error::Result<TreeMergeOutput> {
    let mut out = Index::new();
    let mut conflict_content = BTreeMap::new();
    let mut handled_base: HashSet<Vec<u8>> = HashSet::new();
    let mut handled_theirs: HashSet<Vec<u8>> = HashSet::new();

    for op in sorted_paths(ours.keys()) {
        let bp = if let Some(old) = ours_new_to_old.get(&op) {
            Some(old.clone())
        } else if base.contains_key(&op) {
            Some(op.clone())
        } else {
            None
        };

        let tp = if let Some(ref bpath) = bp {
            theirs_old_to_new
                .get(bpath)
                .cloned()
                .unwrap_or_else(|| bpath.clone())
        } else {
            op.clone()
        };

        let b = bp.as_ref().and_then(|p| base.get(p));
        let o = ours.get(&op);
        let t = theirs.get(&tp);
        if t.is_some() {
            handled_theirs.insert(tp.clone());
        }

        if let Some(ref p) = bp {
            handled_base.insert(p.clone());
        }

        // When our branch still has the file at the same path as the merge base, but the side
        // being applied renamed that path, the result must use their pathname (matches Git
        // cherry-pick / merge-ort: e.g. base+HEAD at `file.txt`, picked commit has `renamed.txt`).
        let out_path = if bp.as_ref().is_some_and(|b| b == &op) && tp != op {
            tp.clone()
        } else {
            op.clone()
        };

        merge_one_path(
            repo,
            &mut out,
            &mut conflict_content,
            &out_path,
            b,
            o,
            t,
            favor,
            ws,
            diff_algorithm,
            presentation,
        )?;
    }

    for bp in sorted_paths(base.keys()) {
        if handled_base.contains(&bp) {
            continue;
        }
        let tp = theirs_old_to_new
            .get(&bp)
            .cloned()
            .unwrap_or_else(|| bp.clone());
        let b = base.get(&bp);
        let o: Option<&IndexEntry> = None;
        let t = theirs.get(&tp);
        if t.is_some() {
            handled_theirs.insert(tp.clone());
        }
        merge_one_path(
            repo,
            &mut out,
            &mut conflict_content,
            &bp,
            b,
            o,
            t,
            favor,
            ws,
            diff_algorithm,
            presentation,
        )?;
    }

    for tp in sorted_paths(theirs.keys()) {
        if handled_theirs.contains(&tp) {
            continue;
        }
        let b: Option<&IndexEntry> = None;
        let o: Option<&IndexEntry> = None;
        let t = theirs.get(&tp);
        merge_one_path(
            repo,
            &mut out,
            &mut conflict_content,
            &tp,
            b,
            o,
            t,
            favor,
            ws,
            diff_algorithm,
            presentation,
        )?;
    }

    out.sort();
    Ok(TreeMergeOutput {
        index: out,
        conflict_content,
    })
}

fn sorted_paths<'a>(keys: impl Iterator<Item = &'a Vec<u8>>) -> Vec<Vec<u8>> {
    let mut v: Vec<Vec<u8>> = keys.cloned().collect();
    v.sort();
    v
}
#[expect(clippy::too_many_arguments)]
fn merge_one_path(
    repo: &Repository,
    index: &mut Index,
    conflict_content: &mut BTreeMap<Vec<u8>, ObjectId>,
    out_path: &[u8],
    b: Option<&IndexEntry>,
    o: Option<&IndexEntry>,
    t: Option<&IndexEntry>,
    favor: MergeFavor,
    ws: WhitespaceMergeOptions,
    diff_algorithm: Option<&str>,
    presentation: TreeMergeConflictPresentation<'_>,
) -> crate::error::Result<()> {
    match (b, o, t) {
        (_, Some(oe), Some(te)) if same_blob(oe, te) => {
            let mut e = oe.clone();
            e.path = out_path.to_vec();
            e.flags = path_len_flags(out_path);
            index.push_entry_unsorted(e);
        }
        (Some(be), Some(oe), Some(te)) if same_blob(be, oe) => {
            let mut e = te.clone();
            e.path = out_path.to_vec();
            e.flags = path_len_flags(out_path);
            index.push_entry_unsorted(e);
        }
        (Some(be), Some(oe), Some(te)) if same_blob(be, te) => {
            let mut e = oe.clone();
            e.path = out_path.to_vec();
            e.flags = path_len_flags(out_path);
            index.push_entry_unsorted(e);
        }
        (Some(be), Some(oe), Some(te))
            if be.mode == oe.mode
                && oe.mode == te.mode
                && matches!(be.mode, 0o120000 | 0o160000) =>
        {
            if same_blob(oe, te) {
                let mut e = oe.clone();
                e.path = out_path.to_vec();
                e.flags = path_len_flags(out_path);
                index.push_entry_unsorted(e);
            } else if same_blob(be, oe) {
                let mut e = te.clone();
                e.path = out_path.to_vec();
                e.flags = path_len_flags(out_path);
                index.push_entry_unsorted(e);
            } else if same_blob(be, te) {
                let mut e = oe.clone();
                e.path = out_path.to_vec();
                e.flags = path_len_flags(out_path);
                index.push_entry_unsorted(e);
            } else {
                stage_entry(index, out_path, be, 1);
                stage_entry(index, out_path, oe, 2);
                stage_entry(index, out_path, te, 3);
            }
        }
        (Some(be), Some(oe), Some(te)) => {
            content_merge_or_conflict(
                repo,
                index,
                conflict_content,
                out_path,
                be,
                oe,
                te,
                favor,
                ws,
                diff_algorithm,
                presentation,
            )?;
        }
        (None, Some(oe), None) => {
            let mut e = oe.clone();
            e.path = out_path.to_vec();
            e.flags = path_len_flags(out_path);
            index.push_entry_unsorted(e);
        }
        (None, None, Some(te)) => {
            let mut e = te.clone();
            e.path = out_path.to_vec();
            e.flags = path_len_flags(out_path);
            index.push_entry_unsorted(e);
        }
        (None, Some(oe), Some(te)) if same_blob(oe, te) => {
            let mut e = oe.clone();
            e.path = out_path.to_vec();
            e.flags = path_len_flags(out_path);
            index.push_entry_unsorted(e);
        }
        (None, Some(oe), Some(te)) => {
            // add/add conflict: both sides introduced the path with differing content and there
            // is no merge base. Git performs a content merge against an empty base, leaving
            // conflict markers in the working tree (and unmerged stages 2/3 in the index). We
            // previously only recorded the index stages, leaving the working-tree file as the
            // plain "ours" blob — that hid the conflict from `rerere` and the user (t3504).
            add_add_content_conflict(
                repo,
                index,
                conflict_content,
                out_path,
                oe,
                te,
                favor,
                ws,
                diff_algorithm,
                presentation,
            )?;
        }
        (Some(_), None, None) => {}
        (Some(be), Some(oe), None) if same_blob(be, oe) && be.path.as_slice() == out_path => {}
        (Some(be), None, Some(te)) if same_blob(be, te) => {}
        (Some(be), Some(oe), None) => {
            stage_entry(index, out_path, be, 1);
            stage_entry(index, out_path, oe, 2);
            // Modify/delete: deleted on the other side; leave our version in the worktree.
            conflict_content.insert(out_path.to_vec(), oe.oid);
        }
        (Some(be), None, Some(te)) => {
            stage_entry(index, out_path, be, 1);
            stage_entry(index, out_path, te, 3);
            // Modify/delete: deleted on ours; leave their version in the worktree.
            conflict_content.insert(out_path.to_vec(), te.oid);
        }
        (None, None, None) => {}
    }
    Ok(())
}

fn path_len_flags(path: &[u8]) -> u16 {
    path.len().min(0xFFF) as u16
}

fn same_blob(a: &IndexEntry, b: &IndexEntry) -> bool {
    a.oid == b.oid && a.mode == b.mode
}

fn file_attrs_for_merge_path(repo: &Repository, path: &[u8]) -> Option<crate::crlf::FileAttrs> {
    let index = repo.load_index().ok()?;
    let rules = crate::worktree_rules::WorktreeRules::from_repository(repo, &index).ok()?;
    let path_str = String::from_utf8_lossy(path);
    Some(rules.file_attrs(path_str.as_ref(), false))
}

fn merge_favor_for_path(repo: &Repository, path: &[u8], favor: MergeFavor) -> MergeFavor {
    if favor != MergeFavor::None {
        return favor;
    }
    let Some(attrs) = file_attrs_for_merge_path(repo, path) else {
        return favor;
    };
    if let crate::crlf::MergeAttr::Driver(name) = &attrs.merge {
        if name == "union" {
            return MergeFavor::Union;
        }
    }
    favor
}

fn blob_is_binary_for_merge(repo: &Repository, path: &[u8], data: &[u8]) -> bool {
    if crate::merge_file::is_binary(data) {
        return true;
    }
    let Some(attrs) = file_attrs_for_merge_path(repo, path) else {
        return false;
    };
    if matches!(attrs.merge, crate::crlf::MergeAttr::Unset) {
        return true;
    }
    if attrs.text == crate::crlf::TextAttr::Unset {
        return true;
    }
    matches!(attrs.diff_attr, crate::crlf::DiffAttr::Unset)
}

fn stage_entry(index: &mut Index, path: &[u8], src: &IndexEntry, stage: u8) {
    let mut e = src.clone();
    e.path = path.to_vec();
    e.flags = path_len_flags(path) | ((stage as u16) << 12);
    index.push_entry_unsorted(e);
}
#[expect(clippy::too_many_arguments)]
fn content_merge_or_conflict(
    repo: &Repository,
    index: &mut Index,
    conflict_content: &mut BTreeMap<Vec<u8>, ObjectId>,
    path: &[u8],
    base: &IndexEntry,
    ours: &IndexEntry,
    theirs: &IndexEntry,
    favor: MergeFavor,
    ws: WhitespaceMergeOptions,
    diff_algorithm: Option<&str>,
    presentation: TreeMergeConflictPresentation<'_>,
) -> crate::error::Result<()> {
    if matches!(base.mode, 0o120000 | 0o160000)
        || matches!(ours.mode, 0o120000 | 0o160000)
        || matches!(theirs.mode, 0o120000 | 0o160000)
    {
        stage_entry(index, path, base, 1);
        stage_entry(index, path, ours, 2);
        stage_entry(index, path, theirs, 3);
        return Ok(());
    }

    // `git checkout -m` runs the normal three-way line merge below: when the merge auto-resolves
    // (non-overlapping edits) Git records a clean stage-0 entry and reports a single `M <path>`;
    // only a genuine line conflict (`result.conflicts > 0`) records unmerged stages 1/2/3. The
    // marker style (merge vs. diff3) is taken from `presentation.style`, so `--conflict=diff3`
    // produces the `||||||| base` section (t7201 5, 6, 9).
    let base_obj = repo.odb().read(&base.oid)?;
    let ours_obj = repo.odb().read(&ours.oid)?;
    let theirs_obj = repo.odb().read(&theirs.oid)?;
    let favor = merge_favor_for_path(repo, path, favor);

    if blob_is_binary_for_merge(repo, path, &base_obj.data)
        || blob_is_binary_for_merge(repo, path, &ours_obj.data)
        || blob_is_binary_for_merge(repo, path, &theirs_obj.data)
    {
        match favor {
            MergeFavor::Theirs => {
                let mut e = theirs.clone();
                e.path = path.to_vec();
                e.flags = path_len_flags(path);
                index.push_entry_unsorted(e);
                return Ok(());
            }
            MergeFavor::Ours => {
                let mut e = ours.clone();
                e.path = path.to_vec();
                e.flags = path_len_flags(path);
                index.push_entry_unsorted(e);
                return Ok(());
            }
            _ => {
                stage_entry(index, path, base, 1);
                stage_entry(index, path, ours, 2);
                stage_entry(index, path, theirs, 3);
                return Ok(());
            }
        }
    }

    let path_label = String::from_utf8_lossy(path);
    let label_theirs: std::borrow::Cow<'_, str> = match presentation.label_theirs {
        TheirsConflictLabel::PathUtf8 => path_label.clone(),
        TheirsConflictLabel::Fixed(s) => std::borrow::Cow::Borrowed(s),
    };
    let input = MergeInput {
        base: &base_obj.data,
        ours: &ours_obj.data,
        theirs: &theirs_obj.data,
        label_ours: presentation.label_ours,
        label_base: presentation.label_base,
        label_theirs: label_theirs.as_ref(),
        favor,
        style: presentation.style,
        marker_size: 7,
        diff_algorithm: diff_algorithm.map(str::to_owned),
        ignore_all_space: ws.ignore_all_space,
        ignore_space_change: ws.ignore_space_change,
        ignore_space_at_eol: ws.ignore_space_at_eol,
        ignore_cr_at_eol: ws.ignore_cr_at_eol,
    };

    let result = merge(&input)?;

    if result.conflicts > 0 {
        let conflict_oid = repo.odb().write(ObjectKind::Blob, &result.content)?;
        conflict_content.insert(path.to_vec(), conflict_oid);
        stage_entry(index, path, base, 1);
        stage_entry(index, path, ours, 2);
        stage_entry(index, path, theirs, 3);
    } else {
        let merged_oid = repo.odb().write(ObjectKind::Blob, &result.content)?;
        let mut entry = ours.clone();
        entry.path = path.to_vec();
        entry.flags = path_len_flags(path);
        entry.oid = merged_oid;
        if base.mode == ours.mode && base.mode != theirs.mode {
            entry.mode = theirs.mode;
        }
        index.push_entry_unsorted(entry);
    }

    Ok(())
}

/// Resolve an add/add conflict (no merge base, both sides created `path`).
///
/// Mirrors Git's behaviour: a content merge with an empty base. When the line merge does not
/// auto-resolve (the common case for genuinely different content), the conflict-marker blob is
/// recorded for the working tree and the unmerged stages 2/3 are written to the index. Gitlink
/// (submodule) and binary add/adds fall back to plain unmerged stages, matching the
/// `content_merge_or_conflict` policy.
#[allow(clippy::too_many_arguments)]
fn add_add_content_conflict(
    repo: &Repository,
    index: &mut Index,
    conflict_content: &mut BTreeMap<Vec<u8>, ObjectId>,
    path: &[u8],
    ours: &IndexEntry,
    theirs: &IndexEntry,
    favor: MergeFavor,
    ws: WhitespaceMergeOptions,
    diff_algorithm: Option<&str>,
    presentation: TreeMergeConflictPresentation<'_>,
) -> crate::error::Result<()> {
    // Gitlink add/add: no textual merge is possible; leave unmerged stages.
    if matches!(ours.mode, 0o120000 | 0o160000) || matches!(theirs.mode, 0o120000 | 0o160000) {
        stage_entry(index, path, ours, 2);
        stage_entry(index, path, theirs, 3);
        return Ok(());
    }

    let ours_obj = repo.odb().read(&ours.oid)?;
    let theirs_obj = repo.odb().read(&theirs.oid)?;
    let favor = merge_favor_for_path(repo, path, favor);

    // Binary add/add: honour an explicit favour, else leave unmerged stages.
    if blob_is_binary_for_merge(repo, path, &ours_obj.data)
        || blob_is_binary_for_merge(repo, path, &theirs_obj.data)
    {
        match favor {
            MergeFavor::Theirs => {
                let mut e = theirs.clone();
                e.path = path.to_vec();
                e.flags = path_len_flags(path);
                index.push_entry_unsorted(e);
            }
            MergeFavor::Ours => {
                let mut e = ours.clone();
                e.path = path.to_vec();
                e.flags = path_len_flags(path);
                index.push_entry_unsorted(e);
            }
            _ => {
                stage_entry(index, path, ours, 2);
                stage_entry(index, path, theirs, 3);
            }
        }
        return Ok(());
    }

    let path_label = String::from_utf8_lossy(path);
    let label_theirs: std::borrow::Cow<'_, str> = match presentation.label_theirs {
        TheirsConflictLabel::PathUtf8 => path_label.clone(),
        TheirsConflictLabel::Fixed(s) => std::borrow::Cow::Borrowed(s),
    };
    let input = MergeInput {
        base: b"",
        ours: &ours_obj.data,
        theirs: &theirs_obj.data,
        label_ours: presentation.label_ours,
        label_base: presentation.label_base,
        label_theirs: label_theirs.as_ref(),
        favor,
        style: presentation.style,
        marker_size: 7,
        diff_algorithm: diff_algorithm.map(str::to_owned),
        ignore_all_space: ws.ignore_all_space,
        ignore_space_change: ws.ignore_space_change,
        ignore_space_at_eol: ws.ignore_space_at_eol,
        ignore_cr_at_eol: ws.ignore_cr_at_eol,
    };

    let result = merge(&input)?;

    // Git treats an add/add as a conflict whenever the two sides cannot be reconciled to a single
    // stage-0 entry. With no merge base there is nothing to disambiguate a mode clash, so when both
    // sides introduce the path as a regular file with a different executable bit (100644 vs 100755)
    // the result is a conflict even if the *content* merges cleanly — Git records unmerged stages
    // 2/3 with the diverging modes (t6411: double mode change with identical content). Type clashes
    // (file/symlink/submodule) are handled by the gitlink branch above / elsewhere.
    let both_regular_files =
        matches!(ours.mode, 0o100644 | 0o100755) && matches!(theirs.mode, 0o100644 | 0o100755);
    let mode_conflict = both_regular_files && ours.mode != theirs.mode && favor == MergeFavor::None;

    if result.conflicts > 0 || mode_conflict {
        let conflict_oid = repo.odb().write(ObjectKind::Blob, &result.content)?;
        conflict_content.insert(path.to_vec(), conflict_oid);
        stage_entry(index, path, ours, 2);
        stage_entry(index, path, theirs, 3);
    } else {
        // A favour (ours/theirs/union) or identical-after-normalisation merge resolved the
        // content cleanly: record a single stage-0 entry with the merged blob.
        let merged_oid = repo.odb().write(ObjectKind::Blob, &result.content)?;
        let mut entry = ours.clone();
        entry.path = path.to_vec();
        entry.flags = path_len_flags(path);
        entry.oid = merged_oid;
        index.push_entry_unsorted(entry);
    }

    Ok(())
}

fn tree_to_index_entries(
    repo: &Repository,
    oid: &ObjectId,
    prefix: &str,
) -> crate::error::Result<Vec<IndexEntry>> {
    let obj = repo.odb().read(oid)?;
    if obj.kind != ObjectKind::Tree {
        return Err(crate::error::Error::CorruptObject(format!(
            "expected tree, got {}",
            obj.kind.as_str()
        )));
    }
    let entries = parse_tree(&obj.data)?;
    let mut result = Vec::new();

    for te in entries {
        let name = String::from_utf8_lossy(&te.name).into_owned();
        let path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };

        if te.mode == 0o040000 {
            let sub = tree_to_index_entries(repo, &te.oid, &path)?;
            result.extend(sub);
        } else {
            let path_bytes = path.into_bytes();
            result.push(IndexEntry {
                ctime_sec: 0,
                ctime_nsec: 0,
                mtime_sec: 0,
                mtime_nsec: 0,
                dev: 0,
                ino: 0,
                mode: te.mode,
                uid: 0,
                gid: 0,
                size: 0,
                oid: te.oid,
                flags: path_bytes.len().min(0xFFF) as u16,
                flags_extended: None,
                path: path_bytes,
                base_index_pos: 0,
            });
        }
    }
    Ok(result)
}

fn tree_to_map(entries: Vec<IndexEntry>) -> HashMap<Vec<u8>, IndexEntry> {
    let mut out = HashMap::new();
    for e in entries {
        out.insert(e.path.clone(), e);
    }
    out
}

/// True when the index tree matches `head_tree_oid` (used for empty pick detection).
pub fn index_tree_oid_matches_head(
    odb: &Odb,
    index: &Index,
    head_tree_oid: &ObjectId,
) -> crate::error::Result<bool> {
    let merged = write_tree_from_index(odb, index, "")?;
    Ok(merged == *head_tree_oid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::zero_oid;
    use crate::environment::RepositoryOptions;
    use crate::index::{IndexEntry, MODE_REGULAR};
    use crate::objects::{parse_commit, ObjectId};
    use crate::repo::Repository;

    #[test]
    fn merge_trees_df_skips_relocation_when_theirs_expands_base_file() {
        let tmp = tempfile::tempdir().expect("tempdir");
        grit_test_support::git(tmp.path(), &["init", "-q", "-b", "main"]);
        let repo =
            Repository::discover(Some(tmp.path()), &RepositoryOptions::empty()).expect("repo");
        std::fs::write(tmp.path().join("dir"), "base\n").expect("write");
        grit_test_support::git(tmp.path(), &["add", "dir"]);
        grit_test_support::git(tmp.path(), &["commit", "-m", "base"]);
        let base_tree = {
            let base: ObjectId = grit_test_support::git(tmp.path(), &["rev-parse", "HEAD"])
                .trim()
                .parse()
                .expect("base");
            let obj = repo.odb().read(&base).expect("read");
            parse_commit(&obj.data).expect("parse").tree
        };
        grit_test_support::git(tmp.path(), &["switch", "-c", "theirs"]);
        grit_test_support::git(tmp.path(), &["rm", "-f", "dir"]);
        std::fs::create_dir_all(tmp.path().join("dir")).expect("mkdir");
        std::fs::write(tmp.path().join("dir/foo"), "foo\n").expect("write");
        grit_test_support::git(tmp.path(), &["add", "dir/foo"]);
        grit_test_support::git(tmp.path(), &["commit", "-m", "theirs"]);
        let theirs_commit: ObjectId = grit_test_support::git(tmp.path(), &["rev-parse", "HEAD"])
            .trim()
            .parse()
            .expect("theirs");
        let theirs_tree = {
            let obj = repo.odb().read(&theirs_commit).expect("read");
            parse_commit(&obj.data).expect("parse").tree
        };
        grit_test_support::git(tmp.path(), &["switch", "main"]);
        let ours_tree = base_tree;
        let ours_commit: ObjectId = grit_test_support::git(tmp.path(), &["rev-parse", "HEAD"])
            .trim()
            .parse()
            .expect("ours");
        let out = merge_trees_three_way(
            &repo,
            base_tree,
            ours_tree,
            theirs_tree,
            MergeFavor::default(),
            WhitespaceMergeOptions::default(),
            None,
            TreeMergeConflictPresentation {
                df_directory_ours_commit: Some(ours_commit),
                df_directory_theirs_commit: Some(theirs_commit),
                ..TreeMergeConflictPresentation::default()
            },
        )
        .expect("merge");
        assert!(out.index.get(b"dir/foo", 0).is_some());
        assert!(
            out.index.entries().iter().all(|e| e.stage() == 0),
            "expected clean merge without df escape entries"
        );
    }

    #[test]
    fn merge_trees_three_way_theirs_only_addition() {
        let tmp = tempfile::tempdir().expect("tempdir");
        grit_test_support::git(tmp.path(), &["init", "-q", "-b", "main"]);
        let repo =
            Repository::discover(Some(tmp.path()), &RepositoryOptions::empty()).expect("repo");
        std::fs::write(tmp.path().join("base.txt"), "b\n").expect("write");
        grit_test_support::git(tmp.path(), &["add", "base.txt"]);
        grit_test_support::git(tmp.path(), &["commit", "-m", "base"]);
        let base: ObjectId = grit_test_support::git(tmp.path(), &["rev-parse", "HEAD"])
            .trim()
            .parse()
            .expect("base");
        let base_tree = {
            let obj = repo.odb().read(&base).expect("read");
            parse_commit(&obj.data).expect("parse").tree
        };
        grit_test_support::git(tmp.path(), &["switch", "-c", "topic"]);
        std::fs::write(tmp.path().join("theirs-only.txt"), "t\n").expect("write");
        grit_test_support::git(tmp.path(), &["add", "theirs-only.txt"]);
        grit_test_support::git(tmp.path(), &["commit", "-m", "topic"]);
        let theirs: ObjectId = grit_test_support::git(tmp.path(), &["rev-parse", "HEAD"])
            .trim()
            .parse()
            .expect("theirs");
        let theirs_tree = {
            let obj = repo.odb().read(&theirs).expect("read");
            parse_commit(&obj.data).expect("parse").tree
        };
        grit_test_support::git(tmp.path(), &["switch", "main"]);
        let ours: ObjectId = grit_test_support::git(tmp.path(), &["rev-parse", "HEAD"])
            .trim()
            .parse()
            .expect("ours");
        let ours_tree = {
            let obj = repo.odb().read(&ours).expect("read");
            parse_commit(&obj.data).expect("parse").tree
        };
        let out = merge_trees_three_way(
            &repo,
            base_tree,
            ours_tree,
            theirs_tree,
            MergeFavor::default(),
            WhitespaceMergeOptions::default(),
            None,
            TreeMergeConflictPresentation::default(),
        )
        .expect("merge");
        assert!(out.index.get(b"theirs-only.txt", 0).is_some());
        assert!(out.conflict_content.is_empty());
    }

    #[test]
    fn merge_trees_clean_textual_merge_auto_resolves() {
        let tmp = tempfile::tempdir().expect("tempdir");
        grit_test_support::git(tmp.path(), &["init", "-q", "-b", "main"]);
        let repo =
            Repository::discover(Some(tmp.path()), &RepositoryOptions::empty()).expect("repo");
        std::fs::write(tmp.path().join("f.txt"), "1\n2\n3\n4\n").expect("write");
        grit_test_support::git(tmp.path(), &["add", "f.txt"]);
        grit_test_support::git(tmp.path(), &["commit", "-m", "base"]);
        let base_tree = {
            let base: ObjectId = grit_test_support::git(tmp.path(), &["rev-parse", "HEAD"])
                .trim()
                .parse()
                .expect("base");
            let obj = repo.odb().read(&base).expect("read");
            parse_commit(&obj.data).expect("parse").tree
        };
        grit_test_support::git(tmp.path(), &["switch", "-c", "topic"]);
        std::fs::write(tmp.path().join("f.txt"), "1\n2\n3\nFOUR\n").expect("write");
        grit_test_support::git(tmp.path(), &["add", "f.txt"]);
        grit_test_support::git(tmp.path(), &["commit", "-m", "theirs"]);
        let theirs_tree = {
            let oid: ObjectId = grit_test_support::git(tmp.path(), &["rev-parse", "HEAD"])
                .trim()
                .parse()
                .expect("theirs");
            let obj = repo.odb().read(&oid).expect("read");
            parse_commit(&obj.data).expect("parse").tree
        };
        grit_test_support::git(tmp.path(), &["switch", "main"]);
        std::fs::write(tmp.path().join("f.txt"), "1\nTWO\n3\n4\n").expect("write");
        grit_test_support::git(tmp.path(), &["add", "f.txt"]);
        grit_test_support::git(tmp.path(), &["commit", "-m", "ours"]);
        let ours_tree = {
            let oid: ObjectId = grit_test_support::git(tmp.path(), &["rev-parse", "HEAD"])
                .trim()
                .parse()
                .expect("ours");
            let obj = repo.odb().read(&oid).expect("read");
            parse_commit(&obj.data).expect("parse").tree
        };
        let out = merge_trees_three_way(
            &repo,
            base_tree,
            ours_tree,
            theirs_tree,
            MergeFavor::default(),
            WhitespaceMergeOptions::default(),
            None,
            TreeMergeConflictPresentation::default(),
        )
        .expect("merge");
        assert!(out.index.get(b"f.txt", 0).is_some());
        assert!(out.conflict_content.is_empty());
        let blob = repo
            .odb()
            .read(&out.index.get(b"f.txt", 0).expect("entry").oid)
            .expect("blob");
        assert_eq!(blob.data, b"1\nTWO\n3\nFOUR\n");
    }

    #[test]
    fn merge_trees_gitlink_three_way_conflict_stages() {
        let tmp = tempfile::tempdir().expect("tempdir");
        grit_test_support::git(tmp.path(), &["init", "-q", "-b", "main"]);
        let nested = tmp.path().join("nested");
        std::fs::create_dir_all(&nested).expect("mkdir");
        grit_test_support::git(&nested, &["init", "-q"]);
        std::fs::write(nested.join("inner.txt"), "x\n").expect("write");
        grit_test_support::git(&nested, &["add", "inner.txt"]);
        grit_test_support::git(&nested, &["commit", "-m", "nested"]);
        let sub_a = grit_test_support::git(&nested, &["rev-parse", "HEAD"])
            .trim()
            .to_owned();
        std::process::Command::new("git")
            .current_dir(tmp.path())
            .args([
                "update-index",
                "--add",
                "--cacheinfo",
                &format!("160000,{sub_a},sub"),
            ])
            .status()
            .expect("update-index");
        grit_test_support::git(tmp.path(), &["commit", "-m", "base"]);
        let repo =
            Repository::discover(Some(tmp.path()), &RepositoryOptions::empty()).expect("repo");
        let base_tree = {
            let base: ObjectId = grit_test_support::git(tmp.path(), &["rev-parse", "HEAD"])
                .trim()
                .parse()
                .expect("base");
            let obj = repo.odb().read(&base).expect("read");
            parse_commit(&obj.data).expect("parse").tree
        };
        grit_test_support::git(&nested, &["commit", "--allow-empty", "-m", "bump"]);
        let sub_b = grit_test_support::git(&nested, &["rev-parse", "HEAD"])
            .trim()
            .to_owned();
        grit_test_support::git(tmp.path(), &["switch", "-c", "topic"]);
        std::process::Command::new("git")
            .current_dir(tmp.path())
            .args([
                "update-index",
                "--cacheinfo",
                &format!("160000,{sub_b},sub"),
            ])
            .status()
            .expect("update-index");
        grit_test_support::git(tmp.path(), &["commit", "-m", "theirs sub"]);
        let theirs_tree = {
            let oid: ObjectId = grit_test_support::git(tmp.path(), &["rev-parse", "HEAD"])
                .trim()
                .parse()
                .expect("theirs");
            let obj = repo.odb().read(&oid).expect("read");
            parse_commit(&obj.data).expect("parse").tree
        };
        grit_test_support::git(&nested, &["commit", "--allow-empty", "-m", "bump2"]);
        let sub_c = grit_test_support::git(&nested, &["rev-parse", "HEAD"])
            .trim()
            .to_owned();
        grit_test_support::git(tmp.path(), &["switch", "main"]);
        std::process::Command::new("git")
            .current_dir(tmp.path())
            .args([
                "update-index",
                "--cacheinfo",
                &format!("160000,{sub_c},sub"),
            ])
            .status()
            .expect("update-index");
        grit_test_support::git(tmp.path(), &["commit", "-m", "ours sub"]);
        let ours_tree = {
            let oid: ObjectId = grit_test_support::git(tmp.path(), &["rev-parse", "HEAD"])
                .trim()
                .parse()
                .expect("ours");
            let obj = repo.odb().read(&oid).expect("read");
            parse_commit(&obj.data).expect("parse").tree
        };
        let out = merge_trees_three_way(
            &repo,
            base_tree,
            ours_tree,
            theirs_tree,
            MergeFavor::default(),
            WhitespaceMergeOptions::default(),
            None,
            TreeMergeConflictPresentation::default(),
        )
        .expect("merge");
        assert!(out.index.get(b"sub", 1).is_some());
        assert!(out.index.get(b"sub", 2).is_some());
        assert!(out.index.get(b"sub", 3).is_some());
    }

    #[test]
    fn merge_trees_binary_conflict_honors_ours_favor() {
        let tmp = tempfile::tempdir().expect("tempdir");
        grit_test_support::git(tmp.path(), &["init", "-q", "-b", "main"]);
        let repo =
            Repository::discover(Some(tmp.path()), &RepositoryOptions::empty()).expect("repo");
        std::fs::write(tmp.path().join("bin.dat"), [0, 1, 2]).expect("write");
        grit_test_support::git(tmp.path(), &["add", "bin.dat"]);
        grit_test_support::git(tmp.path(), &["commit", "-m", "base"]);
        let base_tree = {
            let base: ObjectId = grit_test_support::git(tmp.path(), &["rev-parse", "HEAD"])
                .trim()
                .parse()
                .expect("base");
            let obj = repo.odb().read(&base).expect("read");
            parse_commit(&obj.data).expect("parse").tree
        };
        grit_test_support::git(tmp.path(), &["switch", "-c", "topic"]);
        std::fs::write(tmp.path().join("bin.dat"), [0, 1, 3]).expect("write");
        grit_test_support::git(tmp.path(), &["add", "bin.dat"]);
        grit_test_support::git(tmp.path(), &["commit", "-m", "theirs"]);
        let theirs_tree = {
            let oid: ObjectId = grit_test_support::git(tmp.path(), &["rev-parse", "HEAD"])
                .trim()
                .parse()
                .expect("theirs");
            let obj = repo.odb().read(&oid).expect("read");
            parse_commit(&obj.data).expect("parse").tree
        };
        grit_test_support::git(tmp.path(), &["switch", "main"]);
        std::fs::write(tmp.path().join("bin.dat"), [0, 1, 4]).expect("write");
        grit_test_support::git(tmp.path(), &["add", "bin.dat"]);
        grit_test_support::git(tmp.path(), &["commit", "-m", "ours"]);
        let ours_tree = {
            let oid: ObjectId = grit_test_support::git(tmp.path(), &["rev-parse", "HEAD"])
                .trim()
                .parse()
                .expect("ours");
            let obj = repo.odb().read(&oid).expect("read");
            parse_commit(&obj.data).expect("parse").tree
        };
        let out = merge_trees_three_way(
            &repo,
            base_tree,
            ours_tree,
            theirs_tree,
            MergeFavor::Ours,
            WhitespaceMergeOptions::default(),
            None,
            TreeMergeConflictPresentation::default(),
        )
        .expect("merge");
        let entry = out.index.get(b"bin.dat", 0).expect("stage 0");
        let blob = repo.odb().read(&entry.oid).expect("blob");
        assert_eq!(blob.data, vec![0, 1, 4]);
    }

    #[test]
    fn merge_trees_binary_conflict_honors_theirs_favor() {
        let tmp = tempfile::tempdir().expect("tempdir");
        grit_test_support::git(tmp.path(), &["init", "-q", "-b", "main"]);
        let repo =
            Repository::discover(Some(tmp.path()), &RepositoryOptions::empty()).expect("repo");
        std::fs::write(tmp.path().join("bin.dat"), [0, 1, 2]).expect("write");
        grit_test_support::git(tmp.path(), &["add", "bin.dat"]);
        grit_test_support::git(tmp.path(), &["commit", "-m", "base"]);
        let base: ObjectId = grit_test_support::git(tmp.path(), &["rev-parse", "HEAD"])
            .trim()
            .parse()
            .expect("base");
        let base_tree = {
            let obj = repo.odb().read(&base).expect("read");
            parse_commit(&obj.data).expect("parse").tree
        };
        grit_test_support::git(tmp.path(), &["switch", "-c", "topic"]);
        std::fs::write(tmp.path().join("bin.dat"), [0, 1, 3]).expect("write");
        grit_test_support::git(tmp.path(), &["add", "bin.dat"]);
        grit_test_support::git(tmp.path(), &["commit", "-m", "theirs"]);
        let theirs_tree = {
            let oid: ObjectId = grit_test_support::git(tmp.path(), &["rev-parse", "HEAD"])
                .trim()
                .parse()
                .expect("theirs");
            let obj = repo.odb().read(&oid).expect("read");
            parse_commit(&obj.data).expect("parse").tree
        };
        grit_test_support::git(tmp.path(), &["switch", "main"]);
        std::fs::write(tmp.path().join("bin.dat"), [0, 1, 4]).expect("write");
        grit_test_support::git(tmp.path(), &["add", "bin.dat"]);
        grit_test_support::git(tmp.path(), &["commit", "-m", "ours"]);
        let ours_tree = {
            let oid: ObjectId = grit_test_support::git(tmp.path(), &["rev-parse", "HEAD"])
                .trim()
                .parse()
                .expect("ours");
            let obj = repo.odb().read(&oid).expect("read");
            parse_commit(&obj.data).expect("parse").tree
        };
        let out = merge_trees_three_way(
            &repo,
            base_tree,
            ours_tree,
            theirs_tree,
            MergeFavor::Theirs,
            WhitespaceMergeOptions::default(),
            None,
            TreeMergeConflictPresentation::default(),
        )
        .expect("merge");
        let entry = out.index.get(b"bin.dat", 0).expect("stage 0");
        let theirs_blob = repo.odb().read(&entry.oid).expect("blob");
        assert_eq!(theirs_blob.data, vec![0, 1, 3]);
    }

    #[test]
    fn augment_renames_includes_same_blob_pairs() {
        let mut base = HashMap::new();
        let mut side = HashMap::new();
        let oid = zero_oid();
        let mut old_e = IndexEntry {
            ctime_sec: 0,
            ctime_nsec: 0,
            mtime_sec: 0,
            mtime_nsec: 0,
            dev: 0,
            ino: 0,
            mode: MODE_REGULAR,
            uid: 0,
            gid: 0,
            size: 0,
            oid,
            flags: 0,
            flags_extended: None,
            path: b"old".to_vec(),
            base_index_pos: 0,
        };
        let mut new_e = old_e.clone();
        new_e.path = b"new".to_vec();
        base.insert(b"old".to_vec(), old_e);
        side.insert(b"new".to_vec(), new_e);
        let mut pairs = Vec::new();
        augment_renames_with_same_blob(&base, &side, &mut pairs);
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].2, 100);
    }
}
