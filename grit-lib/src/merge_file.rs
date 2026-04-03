//! Three-way file merge — the engine behind `grit merge-file`.
//!
//! Performs a line-level three-way merge of *base*, *ours*, and *theirs*
//! using the Myers diff algorithm (via the `similar` crate).
//!
//! # Algorithm
//!
//! 1. Split each file into lines, preserving line endings.
//! 2. Annotate each base line with its diff status in `ours` and `theirs`.
//! 3. For each non-Unchanged position, expand the region to fully cover every
//!    overlapping Changed op from either side, then classify as
//!    OnlyOurs / OnlyTheirs / Conflict based on which sides are changed.
//! 4. Emit each hunk: unchanged → base; single-side → that side's content;
//!    two-side → conflict markers (or resolved via `favor`).
//!
//! Pure insertions (ops with empty old_range) are attached to the adjacent
//! base position and emitted as OnlyOurs / OnlyTheirs hunks.

use crate::error::Result;
use similar::{Algorithm, DiffOp, DiffTag};

/// How conflict regions should be resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MergeFavor {
    /// Leave conflict markers in the output (default).
    #[default]
    None,
    /// For conflicts, keep our version.
    Ours,
    /// For conflicts, keep their version.
    Theirs,
    /// For conflicts, concatenate both versions.
    Union,
}

/// Conflict-marker output style.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConflictStyle {
    /// Standard two-section markers (`<<<<<<<`, `=======`, `>>>>>>>`).
    #[default]
    Merge,
    /// Three-section markers including the base section.
    Diff3,
    /// Zealous diff3 (same as Diff3 for our purposes).
    ZealousDiff3,
}

/// Input and options for a three-way merge.
pub struct MergeInput<'a> {
    /// Base (ancestor) content.
    pub base: &'a [u8],
    /// Our version of the file.
    pub ours: &'a [u8],
    /// Their version of the file.
    pub theirs: &'a [u8],
    /// Label for the ours conflict marker line.
    pub label_ours: &'a str,
    /// Label for the base conflict marker line (diff3 only).
    pub label_base: &'a str,
    /// Label for the theirs conflict marker line.
    pub label_theirs: &'a str,
    /// Conflict resolution strategy.
    pub favor: MergeFavor,
    /// Conflict marker style.
    pub style: ConflictStyle,
    /// Width of conflict markers in characters (0 → use default of 7).
    pub marker_size: usize,
}

/// Result of a three-way merge.
pub struct MergeOutput {
    /// Merged file content.
    pub content: Vec<u8>,
    /// Number of conflict regions (0 = clean merge).
    pub conflicts: usize,
}

/// Perform a three-way line-level merge.
///
/// # Errors
///
/// Currently infallible but returns `Result` for future extension.
pub fn merge(input: &MergeInput<'_>) -> Result<MergeOutput> {
    let base_lines = split_lines(input.base);
    let ours_lines = split_lines(input.ours);
    let theirs_lines = split_lines(input.theirs);

    let ours_ops = diff_ops(&base_lines, &ours_lines);
    let theirs_ops = diff_ops(&base_lines, &theirs_lines);

    let hunks = compute_hunks(
        &base_lines,
        &ours_lines,
        &theirs_lines,
        &ours_ops,
        &theirs_ops,
    );

    let marker = if input.marker_size == 0 {
        7
    } else {
        input.marker_size
    };

    let mut content: Vec<u8> = Vec::new();
    let mut conflicts = 0usize;

    for hunk in hunks {
        match hunk {
            Hunk::Unchanged(lines) | Hunk::OnlyOurs(lines) | Hunk::OnlyTheirs(lines) => {
                for line in lines {
                    content.extend_from_slice(&line);
                }
            }
            Hunk::Conflict { base, ours, theirs } => {
                match input.favor {
                    MergeFavor::Ours => {
                        for line in ours {
                            content.extend_from_slice(&line);
                        }
                    }
                    MergeFavor::Theirs => {
                        for line in theirs {
                            content.extend_from_slice(&line);
                        }
                    }
                    MergeFavor::Union => {
                        for line in &ours {
                            content.extend_from_slice(line);
                        }
                        // If the ours portion doesn't end with \n and theirs is
                        // non-empty, insert a newline so both sections appear as
                        // separate lines (matches git's missing-LF handling).
                        if !theirs.is_empty()
                            && !ours.is_empty()
                            && !ours.last().map(|l| l.ends_with(b"\n")).unwrap_or(false)
                        {
                            content.push(b'\n');
                        }
                        for line in &theirs {
                            content.extend_from_slice(line);
                        }
                    }
                    MergeFavor::None => {
                        conflicts += 1;
                        emit_conflict(
                            &mut content,
                            &base,
                            &ours,
                            &theirs,
                            input.label_ours,
                            input.label_base,
                            input.label_theirs,
                            input.style,
                            marker,
                        );
                    }
                }
            }
        }
    }

    Ok(MergeOutput { content, conflicts })
}

/// Emit conflict markers into `out`.
#[allow(clippy::too_many_arguments)]
fn emit_conflict(
    out: &mut Vec<u8>,
    base: &[Vec<u8>],
    ours: &[Vec<u8>],
    theirs: &[Vec<u8>],
    label_ours: &str,
    label_base: &str,
    label_theirs: &str,
    style: ConflictStyle,
    marker: usize,
) {
    let open = "<".repeat(marker);
    let eq = "=".repeat(marker);
    let close = ">".repeat(marker);

    // Ensure ours section starts on a new line if the previous content didn't
    // end with one.
    if !out.is_empty() && !out.ends_with(b"\n") {
        out.push(b'\n');
    }

    out.extend_from_slice(format!("{open} {label_ours}\n").as_bytes());
    for line in ours {
        out.extend_from_slice(line);
    }
    // Ensure separator starts on its own line.
    if out.last().copied() != Some(b'\n') {
        out.push(b'\n');
    }
    match style {
        ConflictStyle::Diff3 | ConflictStyle::ZealousDiff3 => {
            let pipe = "|".repeat(marker);
            out.extend_from_slice(format!("{pipe} {label_base}\n").as_bytes());
            for line in base {
                out.extend_from_slice(line);
            }
            if out.last().copied() != Some(b'\n') {
                out.push(b'\n');
            }
            out.extend_from_slice(format!("{eq}\n").as_bytes());
        }
        ConflictStyle::Merge => {
            out.extend_from_slice(format!("{eq}\n").as_bytes());
        }
    }
    for line in theirs {
        out.extend_from_slice(line);
    }
    if out.last().copied() != Some(b'\n') {
        out.push(b'\n');
    }
    out.extend_from_slice(format!("{close} {label_theirs}\n").as_bytes());
}

/// A classified merge region (owns its lines).
#[derive(Debug)]
enum Hunk {
    /// Lines unchanged by both sides (base content).
    Unchanged(Vec<Vec<u8>>),
    /// Lines changed only by ours.
    OnlyOurs(Vec<Vec<u8>>),
    /// Lines changed only by theirs.
    OnlyTheirs(Vec<Vec<u8>>),
    /// Lines changed by both sides — a conflict.
    Conflict {
        base: Vec<Vec<u8>>,
        ours: Vec<Vec<u8>>,
        theirs: Vec<Vec<u8>>,
    },
}

/// Compute the merge hunks from two diff sequences against the same base.
///
/// Uses a position-by-position scan with op-based expansion to ensure that
/// changed regions spanning multiple lines are not split mid-op.
fn compute_hunks(
    base: &[Vec<u8>],
    ours: &[Vec<u8>],
    theirs: &[Vec<u8>],
    ours_ops: &[DiffOp],
    theirs_ops: &[DiffOp],
) -> Vec<Hunk> {
    let ours_changed = changed_mask(ours_ops, base.len());
    let theirs_changed = changed_mask(theirs_ops, base.len());

    let ours_inserts = collect_inserts(ours_ops, base.len());
    let theirs_inserts = collect_inserts(theirs_ops, base.len());

    let mut hunks: Vec<Hunk> = Vec::new();
    let mut pos = 0usize;

    while pos <= base.len() {
        // Emit pure insertions before this base position.
        emit_inserts_at(
            pos,
            &ours_inserts,
            &theirs_inserts,
            ours,
            theirs,
            &mut hunks,
        );

        if pos == base.len() {
            break;
        }

        let o = ours_changed[pos];
        let t = theirs_changed[pos];

        if !o && !t {
            // Unchanged run: extend while both sides are still equal.
            let mut end = pos + 1;
            while end < base.len() && !ours_changed[end] && !theirs_changed[end] {
                end += 1;
            }
            hunks.push(Hunk::Unchanged(base[pos..end].to_vec()));
            pos = end;
            continue;
        }

        // Changed region: expand end until all overlapping Changed ops from
        // either side are fully consumed.  Repeat until stable.
        let mut end = pos + 1;
        loop {
            let new_end = furthest_changed_op_end(ours_ops, pos, end)
                .max(furthest_changed_op_end(theirs_ops, pos, end));
            if new_end <= end {
                break;
            }
            end = new_end;
        }

        // Classify the full range [pos..end).
        let any_ours = (pos..end).any(|p| ours_changed[p]);
        let any_theirs = (pos..end).any(|p| theirs_changed[p]);

        match (any_ours, any_theirs) {
            (true, false) => {
                let c = collect_new_lines(ours_ops, ours, pos, end);
                hunks.push(Hunk::OnlyOurs(c));
            }
            (false, true) => {
                let c = collect_new_lines(theirs_ops, theirs, pos, end);
                hunks.push(Hunk::OnlyTheirs(c));
            }
            (true, true) => {
                let o = collect_new_lines(ours_ops, ours, pos, end);
                let t = collect_new_lines(theirs_ops, theirs, pos, end);
                if o == t {
                    // Both sides produce the same content — not really a conflict.
                    hunks.push(Hunk::OnlyOurs(o));
                } else {
                    hunks.push(Hunk::Conflict {
                        base: base[pos..end].to_vec(),
                        ours: o,
                        theirs: t,
                    });
                }
            }
            (false, false) => {
                // Should not happen, but treat as unchanged.
                hunks.push(Hunk::Unchanged(base[pos..end].to_vec()));
            }
        }

        pos = end;
    }

    hunks
}

/// Build a boolean mask: `mask[i]` is `true` if base line `i` is covered by
/// a non-Equal op.
fn changed_mask(ops: &[DiffOp], base_len: usize) -> Vec<bool> {
    let mut mask = vec![false; base_len];
    for op in ops {
        if op.tag() == DiffTag::Equal {
            continue;
        }
        for p in op.old_range() {
            if p < base_len {
                mask[p] = true;
            }
        }
    }
    mask
}

/// Collect pure insertions (ops with empty old_range) at each base position.
///
/// Returns a `Vec` of length `base_len + 1`; entry `i` holds all
/// `(new_start, new_end)` ranges inserted before base line `i`.
fn collect_inserts(ops: &[DiffOp], base_len: usize) -> Vec<Vec<(usize, usize)>> {
    let mut result: Vec<Vec<(usize, usize)>> = vec![Vec::new(); base_len + 1];
    for op in ops {
        let old = op.old_range();
        let new_ = op.new_range();
        if old.is_empty() && !new_.is_empty() {
            let pos = old.start.min(base_len);
            result[pos].push((new_.start, new_.end));
        }
    }
    result
}

/// Emit hunks for pure insertions at base position `pos`.
fn emit_inserts_at(
    pos: usize,
    ours_inserts: &[Vec<(usize, usize)>],
    theirs_inserts: &[Vec<(usize, usize)>],
    ours: &[Vec<u8>],
    theirs: &[Vec<u8>],
    hunks: &mut Vec<Hunk>,
) {
    let o_ins = &ours_inserts[pos];
    let t_ins = &theirs_inserts[pos];

    let has_ours = !o_ins.is_empty();
    let has_theirs = !t_ins.is_empty();

    if has_ours && has_theirs {
        // Both sides insert at the same position — check if identical.
        let o_lines: Vec<Vec<u8>> = o_ins.iter()
            .flat_map(|&(s, e)| ours[s..e].to_vec())
            .collect();
        let t_lines: Vec<Vec<u8>> = t_ins.iter()
            .flat_map(|&(s, e)| theirs[s..e].to_vec())
            .collect();

        if o_lines == t_lines {
            // Identical insertions — treat as unchanged.
            if !o_lines.is_empty() {
                hunks.push(Hunk::Unchanged(o_lines));
            }
        } else {
            // Different insertions at the same point — conflict.
            if !o_lines.is_empty() || !t_lines.is_empty() {
                hunks.push(Hunk::Conflict {
                    base: Vec::new(),
                    ours: o_lines,
                    theirs: t_lines,
                });
            }
        }
    } else if has_ours {
        for &(ns, ne) in o_ins {
            let lines: Vec<Vec<u8>> = ours[ns..ne].to_vec();
            if !lines.is_empty() {
                hunks.push(Hunk::OnlyOurs(lines));
            }
        }
    } else if has_theirs {
        for &(ns, ne) in t_ins {
            let lines: Vec<Vec<u8>> = theirs[ns..ne].to_vec();
            if !lines.is_empty() {
                hunks.push(Hunk::OnlyTheirs(lines));
            }
        }
    }
}

/// Return the maximum `old_range().end` among all Changed ops that overlap
/// with `[run_start..current_end)`.  Returns `current_end` if nothing
/// extends further.
fn furthest_changed_op_end(ops: &[DiffOp], run_start: usize, current_end: usize) -> usize {
    let mut max = current_end;
    for op in ops {
        if op.tag() == DiffTag::Equal {
            continue;
        }
        let old = op.old_range();
        if old.start < current_end && old.end > run_start && old.end > max {
            max = old.end;
        }
    }
    max
}

/// Collect new (output) lines for base range `[base_start..base_end)`.
///
/// For each op whose `old_range` overlaps the range:
/// - Equal ops contribute their corresponding new-side lines.
/// - Changed ops contribute their full replacement.
fn collect_new_lines(
    ops: &[DiffOp],
    new: &[Vec<u8>],
    base_start: usize,
    base_end: usize,
) -> Vec<Vec<u8>> {
    let mut lines = Vec::new();
    for op in ops {
        let old = op.old_range();
        let new_ = op.new_range();
        if old.is_empty() {
            continue; // pure insertion, handled separately
        }
        if old.end <= base_start || old.start >= base_end {
            continue; // no overlap
        }
        if op.tag() == DiffTag::Equal {
            let overlap_start = base_start.max(old.start);
            let overlap_end = base_end.min(old.end);
            let offset = overlap_start - old.start;
            let len = overlap_end - overlap_start;
            for i in offset..offset + len {
                if new_.start + i < new_.end {
                    lines.push(new[new_.start + i].clone());
                }
            }
        } else {
            for i in new_.clone() {
                lines.push(new[i].clone());
            }
        }
    }
    lines
}

/// Run Myers diff on two line slices.
fn diff_ops(old: &[Vec<u8>], new: &[Vec<u8>]) -> Vec<DiffOp> {
    similar::capture_diff_slices(Algorithm::Myers, old, new)
}

/// Split a byte slice into lines, each including its trailing `\n` if present.
fn split_lines(data: &[u8]) -> Vec<Vec<u8>> {
    if data.is_empty() {
        return Vec::new();
    }
    let mut lines = Vec::new();
    let mut start = 0;
    for i in 0..data.len() {
        if data[i] == b'\n' {
            lines.push(data[start..=i].to_vec());
            start = i + 1;
        }
    }
    if start < data.len() {
        lines.push(data[start..].to_vec());
    }
    lines
}

/// Returns `true` if the byte slice appears to be binary content.
///
/// Uses the same heuristic as git: any NUL byte means binary.
#[must_use]
pub fn is_binary(data: &[u8]) -> bool {
    data.contains(&0u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn do_merge(base: &str, ours: &str, theirs: &str) -> (String, usize) {
        let input = MergeInput {
            base: base.as_bytes(),
            ours: ours.as_bytes(),
            theirs: theirs.as_bytes(),
            label_ours: "ours",
            label_base: "base",
            label_theirs: "theirs",
            favor: MergeFavor::None,
            style: ConflictStyle::Merge,
            marker_size: 7,
        };
        let out = merge(&input).unwrap();
        (String::from_utf8(out.content).unwrap(), out.conflicts)
    }

    #[test]
    fn no_changes() {
        let t = "line1\nline2\nline3\n";
        let (r, c) = do_merge(t, t, t);
        assert_eq!(r, t);
        assert_eq!(c, 0);
    }

    #[test]
    fn only_ours() {
        let (r, c) = do_merge("a\nb\nc\n", "a\nB\nc\n", "a\nb\nc\n");
        assert_eq!(r, "a\nB\nc\n");
        assert_eq!(c, 0);
    }

    #[test]
    fn only_theirs() {
        let (r, c) = do_merge("a\nb\nc\n", "a\nb\nc\n", "a\nB\nc\n");
        assert_eq!(r, "a\nB\nc\n");
        assert_eq!(c, 0);
    }

    #[test]
    fn conflict() {
        let (r, c) = do_merge("a\nb\nc\n", "a\nX\nc\n", "a\nY\nc\n");
        assert_eq!(c, 1);
        assert!(r.contains("<<<<<<< ours\nX\n=======\nY\n>>>>>>> theirs"));
    }

    #[test]
    fn conflict_delete_vs_change() {
        // ours deletes lines 1-2, theirs changes line 1 → conflict
        let base = "a\nb\nc\n";
        let ours = "c\n"; // deleted a and b
        let theirs = "A\nb\nc\n"; // changed a → A
        let (r, c) = do_merge(base, ours, theirs);
        assert_eq!(c, 1, "expected conflict, got: {r:?}");
    }

    #[test]
    fn favor_ours() {
        let input = MergeInput {
            base: b"a\nb\nc\n",
            ours: b"a\nX\nc\n",
            theirs: b"a\nY\nc\n",
            label_ours: "o",
            label_base: "b",
            label_theirs: "t",
            favor: MergeFavor::Ours,
            style: ConflictStyle::Merge,
            marker_size: 7,
        };
        let out = merge(&input).unwrap();
        assert_eq!(out.content, b"a\nX\nc\n");
        assert_eq!(out.conflicts, 0);
    }

    #[test]
    fn union_missing_lf() {
        let input = MergeInput {
            base: b"line1\nline2\nline3",
            ours: b"line1\nline2\nline3x",
            theirs: b"line1\nline2\nline3y",
            label_ours: "o",
            label_base: "b",
            label_theirs: "t",
            favor: MergeFavor::Union,
            style: ConflictStyle::Merge,
            marker_size: 7,
        };
        let out = merge(&input).unwrap();
        // union: line3x\nline3y (newline inserted between no-LF lines)
        assert_eq!(out.content, b"line1\nline2\nline3x\nline3y");
    }
}
