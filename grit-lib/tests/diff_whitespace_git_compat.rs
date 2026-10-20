//! Whitespace diff modes vs system `git diff` (t4015 / t4040 semantics).

mod support;

use grit_lib::diff::{line_diff_unified_hunk_body, DiffOptions, WhitespaceMode};
use support::{
    git_diff_no_index_patch, patch_hunk_body, LineDiffAlgo, WhitespaceMode as HarnessWs,
};

fn git_hunks(old: &str, new: &str, ws: HarnessWs) -> String {
    patch_hunk_body(&git_diff_no_index_patch(
        old.as_bytes(),
        new.as_bytes(),
        "f",
        LineDiffAlgo::Myers,
        ws,
        &[],
    ))
}

fn grit_hunks(old: &str, new: &str, ws: WhitespaceMode) -> String {
    let opts = DiffOptions::default().with_whitespace(ws);
    let body = line_diff_unified_hunk_body(old, new, &opts);
    if body.is_empty() {
        return String::new();
    }
    patch_hunk_body(&format!("placeholder\n{body}"))
}

#[test]
fn ignore_all_space_matches_git() {
    let old = "hello world\n";
    let new = "helloworld\n";
    assert_eq!(
        grit_hunks(old, new, WhitespaceMode::ignore_all_space()),
        git_hunks(old, new, HarnessWs::IgnoreAllSpace)
    );
}

#[test]
fn ignore_space_change_matches_git() {
    let old = "foo bar\n";
    let new = "foo  bar\n";
    assert_eq!(
        grit_hunks(old, new, WhitespaceMode::ignore_space_change()),
        git_hunks(old, new, HarnessWs::IgnoreSpaceChange)
    );
}

#[test]
fn ignore_blank_lines_matches_git() {
    let old = "a\n\nb\n";
    let new = "a\nb\n";
    assert_eq!(
        grit_hunks(old, new, WhitespaceMode::ignore_blank_lines()),
        git_hunks(old, new, HarnessWs::IgnoreBlankLines)
    );
}

#[test]
fn whitespace_only_file_omitted_like_git() {
    let old = "same\n";
    let new = "s a m e\n";
    let opts = DiffOptions::default().with_whitespace(WhitespaceMode::ignore_all_space());
    assert!(
        line_diff_unified_hunk_body(old, new, &opts).is_empty(),
        "whitespace-only change produces no hunks"
    );
    assert!(
        git_hunks(old, new, HarnessWs::IgnoreAllSpace).is_empty(),
        "git also omits patch"
    );
}

#[test]
fn ignore_all_space_nbsp_still_differs_like_git() {
    let old = "a\u{00a0}b\n";
    let new = "ab\n";
    let ws = WhitespaceMode::ignore_all_space();
    let opts = DiffOptions::default().with_whitespace(ws);
    assert!(
        !line_diff_unified_hunk_body(old, new, &opts).is_empty(),
        "NBSP removal must not be treated as ignorable -w whitespace"
    );
    assert_eq!(
        grit_hunks(old, new, ws),
        git_hunks(old, new, HarnessWs::IgnoreAllSpace)
    );
}

#[test]
fn ignore_blank_lines_respects_myers_not_histogram() {
    let old = (0..5).map(|i| format!("{}\n", char::from(b'A' + i))).collect::<String>();
    let new = (0..5).map(|i| format!("{}\n", char::from(b'A' + i))).collect::<String>();
    let mut opts = DiffOptions::default().with_whitespace(WhitespaceMode::ignore_blank_lines());
    opts.algorithm = grit_lib::diff::DiffAlgorithm::Myers;
    let body = line_diff_unified_hunk_body(&old, &new, &opts);
    assert!(body.is_empty(), "identical non-blank lines should produce no hunks");
    let git_patch = git_diff_no_index_patch(
        old.as_bytes(),
        new.as_bytes(),
        "f",
        LineDiffAlgo::Myers,
        HarnessWs::IgnoreBlankLines,
        &[],
    );
    assert!(
        patch_hunk_body(&git_patch).is_empty(),
        "git also shows no hunks for identical content with --ignore-blank-lines"
    );
}

#[test]
fn ignore_all_space_eof_newline_matches_git() {
    let old = "line\n";
    let new = "line";
    let ws = WhitespaceMode::ignore_all_space();
    assert_eq!(
        grit_hunks(old, new, ws),
        git_hunks(old, new, HarnessWs::IgnoreAllSpace),
        "git -w treats missing final newline like other whitespace-only edits"
    );
}

#[test]
fn whitespace_modes_with_crlf_match_git() {
    let old = "line one\r\nline two\r\n";
    let new = "line  one\r\nline two\r\n";
    assert_eq!(
        grit_hunks(old, new, WhitespaceMode::ignore_space_change()),
        git_hunks(old, new, HarnessWs::IgnoreSpaceChange)
    );
}
