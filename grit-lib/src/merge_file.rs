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
    /// Diff algorithm.
    pub diff_algorithm: Option<String>,
    /// Ignore all whitespace when comparing lines (`-w`).
    pub ignore_all_space: bool,
    /// Ignore changes in amount of whitespace (`-b`).
    pub ignore_space_change: bool,
    /// Ignore whitespace at end of line.
    pub ignore_space_at_eol: bool,
    /// Ignore CR at end of line.
    pub ignore_cr_at_eol: bool,
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
    let ws_mode = WhitespaceMode {
        ignore_all_space: input.ignore_all_space,
        ignore_space_change: input.ignore_space_change,
        ignore_space_at_eol: input.ignore_space_at_eol,
        ignore_cr_at_eol: input.ignore_cr_at_eol,
    };
    let base_compare_lines = normalize_lines_for_compare(&base_lines, &ws_mode);
    let ours_compare_lines = normalize_lines_for_compare(&ours_lines, &ws_mode);
    let theirs_compare_lines = normalize_lines_for_compare(&theirs_lines, &ws_mode);

    let algo = input
        .diff_algorithm
        .as_deref()
        .map(|name| match name.to_lowercase().as_str() {
            "histogram" | "patience" => similar::Algorithm::Patience,
            _ => similar::Algorithm::Myers,
        })
        .unwrap_or(similar::Algorithm::Myers);
    let ours_ops = similar::capture_diff_slices(algo, &base_compare_lines, &ours_compare_lines);
    let theirs_ops = similar::capture_diff_slices(algo, &base_compare_lines, &theirs_compare_lines);

    let mut hunks = compute_hunks(
        &base_lines,
        &ours_lines,
        &theirs_lines,
        &ours_ops,
        &theirs_ops,
        &ws_mode,
    );
    // Git keeps adjacent conflict regions separate when identical lines appear
    // between them (e.g. t4200-rerere); do not merge Conflict+gap+Conflict.
    hunks = coalesce_nearby_conflicts(hunks, 3, false);
    if matches!(input.style, ConflictStyle::ZealousDiff3) {
        hunks = adjust_zealous_hunks(hunks);
    }

    let marker = if input.marker_size == 0 {
        7
    } else {
        input.marker_size
    };

    let mut content: Vec<u8> = Vec::new();
    let mut conflicts = 0usize;

    for (idx, hunk) in hunks.iter().enumerate() {
        match hunk {
            Hunk::Unchanged(lines) => append_lines(&mut content, lines),
            Hunk::OnlyOurs { ours, .. } => append_lines(&mut content, ours),
            Hunk::OnlyTheirs { theirs, .. } => append_lines(&mut content, theirs),
            Hunk::Conflict { base, ours, theirs } => {
                match input.favor {
                    MergeFavor::Ours => {
                        append_lines(&mut content, ours);
                    }
                    MergeFavor::Theirs => {
                        append_lines(&mut content, theirs);
                    }
                    MergeFavor::Union => {
                        append_lines(&mut content, ours);
                        // If the ours portion doesn't end with \n and theirs is
                        // non-empty, insert a newline so both sections appear as
                        // separate lines (matches git's missing-LF handling).
                        if !theirs.is_empty()
                            && !ours.is_empty()
                            && !ours.last().map(|l| l.ends_with(b"\n")).unwrap_or(false)
                        {
                            content.push(b'\n');
                        }
                        append_lines(&mut content, theirs);
                    }
                    MergeFavor::None => {
                        conflicts += 1;
                        if matches!(input.style, ConflictStyle::ZealousDiff3) {
                            let (mut prefix_len, mut suffix_len) =
                                common_prefix_suffix(ours, theirs);

                            if prefix_len > 0
                                && idx > 0
                                && hunk_output_lines(&hunks[idx - 1])
                                    .map(|lines| lines_end_with(lines, &ours[..prefix_len]))
                                    .unwrap_or(false)
                            {
                                prefix_len = 0;
                            }

                            if suffix_len > 0
                                && idx + 1 < hunks.len()
                                && hunk_output_lines(&hunks[idx + 1])
                                    .map(|lines| {
                                        lines_start_with(lines, &ours[ours.len() - suffix_len..])
                                    })
                                    .unwrap_or(false)
                            {
                                suffix_len = 0;
                            }

                            if prefix_len > 0 {
                                append_lines(&mut content, &ours[..prefix_len]);
                            }
                            emit_conflict(
                                &mut content,
                                base,
                                &ours[prefix_len..ours.len() - suffix_len],
                                &theirs[prefix_len..theirs.len() - suffix_len],
                                input.label_ours,
                                input.label_base,
                                input.label_theirs,
                                input.style,
                                marker,
                            );
                            if suffix_len > 0 {
                                append_lines(&mut content, &ours[ours.len() - suffix_len..]);
                            }
                        } else if matches!(input.style, ConflictStyle::Merge) {
                            let (prefix_len, suffix_len) = common_prefix_suffix(ours, theirs);
                            let pre = &ours[..prefix_len];
                            let suf_start = ours.len().saturating_sub(suffix_len);
                            let o_mid = &ours[prefix_len..suf_start];
                            let t_mid =
                                &theirs[prefix_len..theirs.len().saturating_sub(suffix_len)];
                            let suf = &ours[suf_start..];
                            append_lines(&mut content, pre);
                            emit_conflict(
                                &mut content,
                                base,
                                o_mid,
                                t_mid,
                                input.label_ours,
                                input.label_base,
                                input.label_theirs,
                                input.style,
                                marker,
                            );
                            append_lines(&mut content, suf);
                        } else {
                            emit_conflict(
                                &mut content,
                                base,
                                ours,
                                theirs,
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
    }

    Ok(MergeOutput { content, conflicts })
}

fn append_lines(out: &mut Vec<u8>, lines: &[Vec<u8>]) {
    // If previous content ended without a newline and this hunk adds more
    // content, insert a newline separator to avoid merging lines together.
    if !out.is_empty() && !out.ends_with(b"\n") && !lines.is_empty() {
        out.push(b'\n');
    }
    for line in lines {
        out.extend_from_slice(line);
    }
}

fn common_prefix_suffix(ours: &[Vec<u8>], theirs: &[Vec<u8>]) -> (usize, usize) {
    let mut prefix = 0usize;
    while prefix < ours.len() && prefix < theirs.len() && ours[prefix] == theirs[prefix] {
        prefix += 1;
    }

    let mut suffix = 0usize;
    while suffix < ours.len().saturating_sub(prefix)
        && suffix < theirs.len().saturating_sub(prefix)
        && ours[ours.len() - 1 - suffix] == theirs[theirs.len() - 1 - suffix]
    {
        suffix += 1;
    }

    (prefix, suffix)
}

fn hunk_output_lines(hunk: &Hunk) -> Option<&[Vec<u8>]> {
    match hunk {
        Hunk::Unchanged(lines) => Some(lines),
        Hunk::OnlyOurs { ours, .. } => Some(ours),
        Hunk::OnlyTheirs { theirs, .. } => Some(theirs),
        Hunk::Conflict { .. } => None,
    }
}

fn lines_end_with(lines: &[Vec<u8>], suffix: &[Vec<u8>]) -> bool {
    if suffix.is_empty() || suffix.len() > lines.len() {
        return false;
    }
    lines[lines.len() - suffix.len()..] == *suffix
}

fn lines_start_with(lines: &[Vec<u8>], prefix: &[Vec<u8>]) -> bool {
    if prefix.is_empty() || prefix.len() > lines.len() {
        return false;
    }
    lines[..prefix.len()] == *prefix
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
    let marker_terminator: &[u8] = b"\n";

    // Ensure ours section starts on a new line if the previous content didn't
    // end with one.
    if !out.is_empty() && !out.ends_with(b"\n") {
        out.extend_from_slice(marker_terminator);
    }

    write_conflict_marker_line(out, &open, Some(label_ours), marker_terminator);
    for line in ours {
        out.extend_from_slice(line);
    }
    // Ensure separator starts on its own line.
    if out.last().copied() != Some(b'\n') {
        out.extend_from_slice(marker_terminator);
    }
    match style {
        ConflictStyle::Diff3 | ConflictStyle::ZealousDiff3 => {
            let pipe = "|".repeat(marker);
            write_conflict_marker_line(out, &pipe, Some(label_base), marker_terminator);
            for line in base {
                out.extend_from_slice(line);
            }
            if out.last().copied() != Some(b'\n') {
                out.extend_from_slice(marker_terminator);
            }
            write_conflict_marker_line(out, &eq, None, marker_terminator);
        }
        ConflictStyle::Merge => {
            write_conflict_marker_line(out, &eq, None, marker_terminator);
        }
    }
    for line in theirs {
        out.extend_from_slice(line);
    }
    if out.last().copied() != Some(b'\n') {
        out.extend_from_slice(marker_terminator);
    }
    write_conflict_marker_line(out, &close, Some(label_theirs), marker_terminator);
}

fn write_conflict_marker_line(out: &mut Vec<u8>, marker: &str, label: Option<&str>, eol: &[u8]) {
    out.extend_from_slice(marker.as_bytes());
    if let Some(label) = label {
        out.push(b' ');
        out.extend_from_slice(label.as_bytes());
    }
    out.extend_from_slice(eol);
}

/// A classified merge region (owns its lines).
#[derive(Debug, Clone)]
enum Hunk {
    /// Lines unchanged by both sides (base content).
    Unchanged(Vec<Vec<u8>>),
    /// Lines changed only by ours.
    OnlyOurs {
        /// Base lines for the changed region (empty for pure insertions).
        base: Vec<Vec<u8>>,
        /// Output lines from ours.
        ours: Vec<Vec<u8>>,
    },
    /// Lines changed only by theirs.
    OnlyTheirs {
        /// Base lines for the changed region (empty for pure insertions).
        base: Vec<Vec<u8>>,
        /// Output lines from theirs.
        theirs: Vec<Vec<u8>>,
    },
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
    ws_mode: &WhitespaceMode,
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
            // Unchanged run. Stop before a position that has pending insertions
            // on either side so that insertions are emitted in-order at the
            // correct base position.
            let mut end = pos + 1;
            while end < base.len()
                && !ours_changed[end]
                && !theirs_changed[end]
                && ours_inserts[end].is_empty()
                && theirs_inserts[end].is_empty()
            {
                end += 1;
            }
            let unchanged_lines = if ws_mode.ignore_all_space
                || ws_mode.ignore_space_change
                || ws_mode.ignore_space_at_eol
                || ws_mode.ignore_cr_at_eol
            {
                &ours[pos..end]
            } else {
                &base[pos..end]
            };
            hunks.push(Hunk::Unchanged(unchanged_lines.to_vec()));
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
                hunks.push(Hunk::OnlyOurs {
                    base: base[pos..end].to_vec(),
                    ours: c,
                });
            }
            (false, true) => {
                let c = collect_new_lines(theirs_ops, theirs, pos, end);
                hunks.push(Hunk::OnlyTheirs {
                    base: base[pos..end].to_vec(),
                    theirs: c,
                });
            }
            (true, true) => {
                let o = collect_new_lines(ours_ops, ours, pos, end);
                let t = collect_new_lines(theirs_ops, theirs, pos, end);
                if lines_equal_for_compare(&o, &t, ws_mode) {
                    // Both sides produce the same content — not really a conflict.
                    hunks.push(Hunk::OnlyOurs {
                        base: base[pos..end].to_vec(),
                        ours: o,
                    });
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
        // Both sides insert at the same position. If they share a common
        // prefix, emit that prefix as unchanged and only conflict on the
        // diverging remainder. This preserves superset insertions.
        let o_lines: Vec<Vec<u8>> = o_ins
            .iter()
            .flat_map(|&(s, e)| ours[s..e].to_vec())
            .collect();
        let t_lines: Vec<Vec<u8>> = t_ins
            .iter()
            .flat_map(|&(s, e)| theirs[s..e].to_vec())
            .collect();

        let mut common_len = 0usize;
        while common_len < o_lines.len()
            && common_len < t_lines.len()
            && o_lines[common_len] == t_lines[common_len]
        {
            common_len += 1;
        }

        if common_len > 0 {
            hunks.push(Hunk::Unchanged(o_lines[..common_len].to_vec()));
        }

        let ours_tail = o_lines[common_len..].to_vec();
        let theirs_tail = t_lines[common_len..].to_vec();

        let ours_has_extra = !ours_tail.is_empty();
        let theirs_has_extra = !theirs_tail.is_empty();

        if ours_has_extra && theirs_has_extra {
            hunks.push(Hunk::Conflict {
                base: Vec::new(),
                ours: ours_tail,
                theirs: theirs_tail,
            });
        } else if ours_has_extra {
            hunks.push(Hunk::OnlyOurs {
                base: Vec::new(),
                ours: ours_tail,
            });
        } else if theirs_has_extra {
            hunks.push(Hunk::OnlyTheirs {
                base: Vec::new(),
                theirs: theirs_tail,
            });
        }
    } else if has_ours {
        for &(ns, ne) in o_ins {
            let lines: Vec<Vec<u8>> = ours[ns..ne].to_vec();
            if !lines.is_empty() {
                hunks.push(Hunk::OnlyOurs {
                    base: Vec::new(),
                    ours: lines,
                });
            }
        }
    } else if has_theirs {
        for &(ns, ne) in t_ins {
            let lines: Vec<Vec<u8>> = theirs[ns..ne].to_vec();
            if !lines.is_empty() {
                hunks.push(Hunk::OnlyTheirs {
                    base: Vec::new(),
                    theirs: lines,
                });
            }
        }
    }
}

fn adjust_zealous_hunks(hunks: Vec<Hunk>) -> Vec<Hunk> {
    let mut out: Vec<Hunk> = Vec::new();
    let mut i = 0usize;

    while i < hunks.len() {
        let mut consumed = 1usize;
        let mut transformed: Option<Vec<Hunk>> = None;

        let (pre_insert, mid_idx) = match &hunks[i] {
            Hunk::OnlyTheirs { base, theirs } if base.is_empty() => {
                (Some(theirs.as_slice()), i + 1)
            }
            _ => (None, i),
        };

        if let Some(Hunk::OnlyOurs { base, ours }) = hunks.get(mid_idx) {
            if !base.is_empty() {
                let post_insert = match hunks.get(mid_idx + 1) {
                    Some(Hunk::OnlyTheirs { base, theirs }) if base.is_empty() => {
                        Some(theirs.as_slice())
                    }
                    _ => None,
                };

                let mut prefix_len = 0usize;
                if let Some(pre) = pre_insert {
                    if !pre.is_empty() && ours.starts_with(pre) {
                        prefix_len = pre.len();
                    }
                }

                let mut suffix_len = 0usize;
                if let Some(post) = post_insert {
                    if !post.is_empty() && ours[prefix_len..].ends_with(post) {
                        suffix_len = post.len();
                    }
                }

                if prefix_len > 0 || suffix_len > 0 {
                    consumed = if pre_insert.is_some() {
                        if post_insert.is_some() {
                            3
                        } else {
                            2
                        }
                    } else if post_insert.is_some() {
                        2
                    } else {
                        1
                    };

                    let mut replacement: Vec<Hunk> = Vec::new();
                    if prefix_len > 0 {
                        replacement.push(Hunk::Unchanged(ours[..prefix_len].to_vec()));
                    }
                    replacement.push(Hunk::Conflict {
                        base: base.clone(),
                        ours: ours[prefix_len..ours.len() - suffix_len].to_vec(),
                        theirs: base.clone(),
                    });
                    if suffix_len > 0 {
                        replacement.push(Hunk::Unchanged(ours[ours.len() - suffix_len..].to_vec()));
                    }
                    transformed = Some(replacement);
                }
            }
        }

        if let Some(replacement) = transformed {
            for h in replacement {
                push_hunk_with_unchanged_merge(&mut out, h);
            }
            i += consumed;
            continue;
        }

        push_hunk_with_unchanged_merge(&mut out, hunks[i].clone());
        i += 1;
    }

    out
}

/// If one side replaces base lines and the other appends an insertion immediately after
/// that base span, `compute_hunks` emits two hunks (`Only*` + trailing insert). Git treats
/// the combined region as one conflict (e.g. base `hello\n`, ours adds `hello\n`, theirs
/// replaces with `remove-conflict\n`).
fn merge_adjacent_replace_and_trailing_insert_conflicts(hunks: Vec<Hunk>) -> Vec<Hunk> {
    let mut out: Vec<Hunk> = Vec::with_capacity(hunks.len());
    let mut i = 0usize;
    while i < hunks.len() {
        let merged = match (&hunks[i], hunks.get(i + 1)) {
            (Hunk::OnlyTheirs { base, theirs }, Some(Hunk::OnlyOurs { base: bo, ours: o }))
                if !base.is_empty() && bo.is_empty() && !o.is_empty() && !theirs.is_empty() =>
            {
                Some(Hunk::Conflict {
                    base: base.clone(),
                    ours: o.clone(),
                    theirs: theirs.clone(),
                })
            }
            (
                Hunk::OnlyOurs { base, ours },
                Some(Hunk::OnlyTheirs {
                    base: bt,
                    theirs: t,
                }),
            ) if !base.is_empty() && bt.is_empty() && !t.is_empty() && !ours.is_empty() => {
                Some(Hunk::Conflict {
                    base: base.clone(),
                    ours: ours.clone(),
                    theirs: t.clone(),
                })
            }
            _ => None,
        };
        if let Some(h) = merged {
            out.push(h);
            i += 2;
        } else {
            out.push(hunks[i].clone());
            i += 1;
        }
    }
    out
}

fn coalesce_nearby_conflicts(hunks: Vec<Hunk>, max_gap_lines: usize, enable: bool) -> Vec<Hunk> {
    if !enable {
        return hunks;
    }
    let mut out: Vec<Hunk> = Vec::new();
    let mut i = 0usize;

    while i < hunks.len() {
        let Some(Hunk::Conflict { base, ours, theirs }) = hunks.get(i) else {
            out.push(hunks[i].clone());
            i += 1;
            continue;
        };

        let mut merged_base = base.clone();
        let mut merged_ours = ours.clone();
        let mut merged_theirs = theirs.clone();
        let mut j = i;

        loop {
            let Some(Hunk::Unchanged(gap)) = hunks.get(j + 1) else {
                break;
            };
            let Some(Hunk::Conflict {
                base: next_base,
                ours: next_ours,
                theirs: next_theirs,
            }) = hunks.get(j + 2)
            else {
                break;
            };
            if gap.len() > max_gap_lines {
                break;
            }

            merged_base.extend(gap.iter().cloned());
            merged_base.extend(next_base.iter().cloned());
            merged_ours.extend(gap.iter().cloned());
            merged_ours.extend(next_ours.iter().cloned());
            merged_theirs.extend(gap.iter().cloned());
            merged_theirs.extend(next_theirs.iter().cloned());
            j += 2;
        }

        out.push(Hunk::Conflict {
            base: merged_base,
            ours: merged_ours,
            theirs: merged_theirs,
        });
        i = j + 1;
    }

    out
}

fn push_hunk_with_unchanged_merge(out: &mut Vec<Hunk>, hunk: Hunk) {
    match hunk {
        Hunk::Unchanged(mut lines) => {
            if let Some(Hunk::Unchanged(prev)) = out.last_mut() {
                prev.append(&mut lines);
            } else if !lines.is_empty() {
                out.push(Hunk::Unchanged(lines));
            }
        }
        other => out.push(other),
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
fn _diff_ops(old: &[Vec<u8>], new: &[Vec<u8>]) -> Vec<DiffOp> {
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

#[derive(Clone, Copy, Debug, Default)]
struct WhitespaceMode {
    ignore_all_space: bool,
    ignore_space_change: bool,
    ignore_space_at_eol: bool,
    ignore_cr_at_eol: bool,
}

fn normalize_lines_for_compare(lines: &[Vec<u8>], mode: &WhitespaceMode) -> Vec<Vec<u8>> {
    lines
        .iter()
        .map(|line| normalize_line_for_compare(line, mode))
        .collect()
}

fn normalize_line_for_compare(line: &[u8], mode: &WhitespaceMode) -> Vec<u8> {
    let mut bytes = line.to_vec();

    if mode.ignore_cr_at_eol && bytes.ends_with(b"\r\n") {
        let len = bytes.len();
        bytes.remove(len - 2);
    }

    if mode.ignore_all_space {
        return bytes
            .into_iter()
            .filter(|b| !b.is_ascii_whitespace())
            .collect();
    }

    if mode.ignore_space_change {
        let mut out = Vec::with_capacity(bytes.len());
        let mut in_ws = false;
        for ch in bytes {
            if ch.is_ascii_whitespace() {
                if !in_ws {
                    out.push(b' ');
                    in_ws = true;
                }
            } else {
                out.push(ch);
                in_ws = false;
            }
        }
        while out.last().is_some_and(|b| b.is_ascii_whitespace()) {
            out.pop();
        }
        return out;
    }

    if mode.ignore_space_at_eol {
        if bytes.last().copied() == Some(b'\n') {
            let mut body = bytes[..bytes.len() - 1].to_vec();
            while body.last().is_some_and(|b| b.is_ascii_whitespace()) {
                body.pop();
            }
            body.push(b'\n');
            bytes = body;
        } else {
            while bytes.last().is_some_and(|b| b.is_ascii_whitespace()) {
                bytes.pop();
            }
        }
    }

    bytes
}

fn lines_equal_for_compare(left: &[Vec<u8>], right: &[Vec<u8>], mode: &WhitespaceMode) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .all(|(a, b)| normalize_line_for_compare(a, mode) == normalize_line_for_compare(b, mode))
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
            diff_algorithm: None,
            ignore_all_space: false,
            ignore_space_change: false,
            ignore_space_at_eol: false,
            ignore_cr_at_eol: false,
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
    fn conflict_replace_vs_insert_after_same_line() {
        // Base one line; ours duplicates it; theirs replaces it — Git reports content conflict.
        let base = "hello\n";
        let ours = "hello\nhello\n";
        let theirs = "remove-conflict\n";
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
            diff_algorithm: None,
            ignore_all_space: false,
            ignore_space_change: false,
            ignore_space_at_eol: false,
            ignore_cr_at_eol: false,
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
            diff_algorithm: None,
            ignore_all_space: false,
            ignore_space_change: false,
            ignore_space_at_eol: false,
            ignore_cr_at_eol: false,
        };
        let out = merge(&input).unwrap();
        // union: line3x\nline3y (newline inserted between no-LF lines)
        assert_eq!(out.content, b"line1\nline2\nline3x\nline3y");
    }

    #[test]
    fn zdiff3_interesting_conflict_shape() {
        let input = MergeInput {
            base: b"1\n2\n3\n4\n5\n6\n7\n8\n9\n",
            ours: b"1\n2\n3\n4\nA\nB\nC\nD\nE\nF\nG\nH\nI\nJ\n7\n8\n9\n",
            theirs: b"1\n2\n3\n4\nA\nB\nC\n5\n6\nG\nH\nI\nJ\n7\n8\n9\n",
            label_ours: "HEAD",
            label_base: "base",
            label_theirs: "right^0",
            favor: MergeFavor::None,
            style: ConflictStyle::ZealousDiff3,
            marker_size: 7,
            diff_algorithm: None,
            ignore_all_space: false,
            ignore_space_change: false,
            ignore_space_at_eol: false,
            ignore_cr_at_eol: false,
        };
        let out = merge(&input).unwrap();
        let rendered = String::from_utf8(out.content).unwrap();
        assert_eq!(out.conflicts, 1, "{rendered}");
        assert!(rendered.contains("<<<<<<< HEAD\nD\nE\nF\n"), "{rendered}");
    }

    #[test]
    fn preserves_shared_and_superset_insertions() {
        let input = MergeInput {
            base: b"1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n",
            ours: b"1\n2\n3\n4\n5\n5.5\n6\n7\n8\n9\n10\n",
            theirs: b"1\n2\n3\n4\n5\n5.5\n6\n7\n8\n9\n10\n10.5\n",
            label_ours: "ours",
            label_base: "base",
            label_theirs: "theirs",
            favor: MergeFavor::None,
            style: ConflictStyle::Merge,
            marker_size: 7,
            diff_algorithm: None,
            ignore_all_space: false,
            ignore_space_change: false,
            ignore_space_at_eol: false,
            ignore_cr_at_eol: false,
        };
        let base_lines = split_lines(input.base);
        let ours_lines = split_lines(input.ours);
        let theirs_lines = split_lines(input.theirs);
        let ws_mode = WhitespaceMode::default();
        let base_compare_lines = normalize_lines_for_compare(&base_lines, &ws_mode);
        let ours_compare_lines = normalize_lines_for_compare(&ours_lines, &ws_mode);
        let theirs_compare_lines = normalize_lines_for_compare(&theirs_lines, &ws_mode);
        let ours_ops = similar::capture_diff_slices(
            Algorithm::Myers,
            &base_compare_lines,
            &ours_compare_lines,
        );
        let theirs_ops = similar::capture_diff_slices(
            Algorithm::Myers,
            &base_compare_lines,
            &theirs_compare_lines,
        );
        let hunks = compute_hunks(
            &base_lines,
            &ours_lines,
            &theirs_lines,
            &ours_ops,
            &theirs_ops,
            &ws_mode,
        );
        assert_eq!(
            hunks.len(),
            4,
            "expected unchanged shared insertion and theirs-only tail insertion"
        );
        let out = merge(&input).unwrap();
        let rendered = String::from_utf8(out.content).unwrap();
        assert_eq!(out.conflicts, 0, "{rendered}");
        assert_eq!(
            rendered, "1\n2\n3\n4\n5\n5.5\n6\n7\n8\n9\n10\n10.5\n",
            "{rendered}"
        );
    }
}
