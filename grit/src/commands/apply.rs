//! `grit apply` — apply a unified diff/patch to the working tree or index.
//!
//! Modes:
//! - `grit apply <patch>` — apply patch to the working tree
//! - `grit apply --cached <patch>` — apply patch to the index only
//! - `grit apply --stat <patch>` — show diffstat without applying
//! - `grit apply --numstat <patch>` — show numstat without applying
//! - `grit apply --summary <patch>` — show summary without applying
//! - `grit apply --check <patch>` — check if patch applies cleanly
//! - `grit apply -R / --reverse` — reverse the patch
//! - `grit apply -p<n>` — strip leading path components (default 1)
//! - `grit apply --directory=<dir>` — prepend directory to paths
//! - Reads from stdin if no file argument given

use crate::explicit_exit::ExplicitExit;
use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::config::ConfigSet;
use grit_lib::crlf::{self, FileAttrs, MergeAttr};
use grit_lib::index::{Index, IndexEntry};
use grit_lib::merge_file::{merge, ConflictStyle, MergeFavor, MergeInput};
use grit_lib::objects::ObjectId;
use grit_lib::objects::ObjectKind;
use grit_lib::quote_path::quote_c_style;
use grit_lib::repo::Repository;
use grit_lib::rerere::{repo_rerere, rerere_enabled, RerereAutoupdate};
use grit_lib::rev_parse::{resolve_revision, resolve_revision_for_patch_old_blob};
use grit_lib::ws::{self, WhitespaceGitAttr, WS_BLANK_AT_EOF, WS_DEFAULT_RULE, WS_INCOMPLETE_LINE};
use regex::Regex;
use std::borrow::Cow;
use std::collections::HashMap;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Arguments for `grit apply`.
#[derive(Debug, ClapArgs)]
#[command(about = "Apply a patch to files and/or to the index")]
pub struct Args {
    /// Apply the patch to the index instead of the working tree.
    #[arg(long)]
    pub cached: bool,

    /// Show a diffstat of the patch without applying.
    #[arg(long)]
    pub stat: bool,

    /// Show machine-readable stat (additions/deletions per file).
    #[arg(long)]
    pub numstat: bool,

    /// Show a condensed summary of extended header information.
    #[arg(long)]
    pub summary: bool,

    /// Apply the patch even when combined with --stat/--summary output modes.
    #[arg(long = "apply")]
    pub apply: bool,

    /// Check if the patch applies cleanly without modifying anything.
    #[arg(long)]
    pub check: bool,

    /// Apply to both the working tree and the index.
    #[arg(long)]
    pub index: bool,

    /// Mark new files as intent-to-add when applying to the working tree.
    #[arg(short = 'N', long = "intent-to-add")]
    pub intent_to_add: bool,

    /// Apply the patch in reverse.
    #[arg(short = 'R', long = "reverse")]
    pub reverse: bool,

    /// Allow empty patches (or patch input with no diff hunks).
    #[arg(long = "allow-empty")]
    pub allow_empty: bool,

    /// Strip N leading path components from diff paths (default: 1).
    #[arg(short = 'p', default_value = "1")]
    pub strip: usize,

    /// Prepend directory to all file paths in the patch.
    #[arg(long = "directory", value_name = "DIR")]
    pub directory: Option<String>,

    /// Build a temporary index with preimage blobs from patch index lines.
    #[arg(long = "build-fake-ancestor", value_name = "FILE")]
    pub build_fake_ancestor: Option<PathBuf>,

    /// Ensure at least `<n>` lines of surrounding context match (git apply `-C<n>`).
    #[arg(short = 'C', value_name = "N")]
    pub context: Option<usize>,

    /// Recount hunk line counts (for corrupted patches).
    #[arg(long = "recount")]
    pub recount: bool,

    /// Apply with unidiff-zero context.
    #[arg(long = "unidiff-zero")]
    pub unidiff_zero: bool,

    /// Allow binary patches.
    #[arg(long = "allow-binary-replacement", alias = "binary")]
    pub allow_binary_replacement: bool,

    /// Verbose output.
    #[arg(short = 'v', long = "verbose")]
    pub verbose: bool,

    /// Leave rejected hunks in corresponding *.rej files.
    #[arg(long = "reject")]
    pub reject: bool,

    /// How to handle whitespace errors.
    #[arg(long = "whitespace", value_name = "ACTION", default_value = "warn")]
    pub whitespace: String,

    /// Ignore changes in whitespace when matching context lines.
    #[arg(long = "ignore-whitespace")]
    pub ignore_whitespace: bool,

    /// Ignore changes in amount of whitespace when matching context lines.
    #[arg(long = "ignore-space-change")]
    pub ignore_space_change: bool,

    /// Disable whitespace-ignoring context matching.
    #[arg(long = "no-ignore-whitespace")]
    pub no_ignore_whitespace: bool,

    /// Attempt a three-way merge if the patch does not apply cleanly.
    #[arg(long = "3way")]
    pub three_way: bool,

    /// With `--3way`, resolve conflicts using our version (`git apply --ours`).
    #[arg(long = "ours", requires = "three_way", conflicts_with_all = ["theirs", "union"])]
    pub ours: bool,

    /// With `--3way`, resolve conflicts using their version (`git apply --theirs`).
    #[arg(long = "theirs", requires = "three_way", conflicts_with_all = ["ours", "union"])]
    pub theirs: bool,

    /// With `--3way`, resolve conflicts using a union merge (`git apply --union`).
    #[arg(long = "union", requires = "three_way", conflicts_with_all = ["ours", "theirs"])]
    pub union: bool,

    /// Include context and removed lines in the output.
    #[arg(long = "include")]
    pub include: Option<String>,

    /// Exclude paths from the patch.
    #[arg(long = "exclude")]
    pub exclude: Option<String>,

    /// Do not trust the line counts in the hunk headers.
    #[arg(long = "inaccurate-eof")]
    pub inaccurate_eof: bool,

    /// Patch file(s). Reads from stdin if none given.
    #[arg(value_name = "PATCH")]
    pub patches: Vec<PathBuf>,
}

// ---------------------------------------------------------------------------
// Parsed patch types
// ---------------------------------------------------------------------------

/// A single hunk in a unified diff.
#[derive(Debug, Clone)]
struct Hunk {
    /// 1-based line number in the old file.
    old_start: usize,
    /// Number of lines in the old side.
    old_count: usize,
    /// 1-based line number in the new file.
    new_start: usize,
    /// Number of lines on the new side.
    new_count: usize,
    /// 1-based line number in the patch file of the first hunk body line (line after `@@`).
    first_body_line: usize,
    /// Lines of the hunk body (' ', '+', '-' prefixed, or bare '\' no newline).
    lines: Vec<HunkLine>,
}

#[derive(Debug, Clone)]
enum HunkLine {
    Context(String),
    Add(String),
    Remove(String),
    /// "\ No newline at end of file"
    NoNewline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WsCliMode {
    Warn,
    NoWarn,
    Error,
    Fix,
}

#[derive(Debug, Clone, Copy)]
struct ApplyWhitespaceMode {
    whitespace_fix: bool,
    ignore_space_change: bool,
    inaccurate_eof: bool,
    tab_width: usize,
    context: Option<usize>,
    ws_cli: WsCliMode,
    /// Git suppresses excess whitespace diagnostics after this many (0 = never squelch).
    ws_squelch: u32,
    /// `git apply --unidiff-zero`: do not require trailing context to pin the hunk end.
    unidiff_zero: bool,
}

fn config_ignore_space_change() -> bool {
    let Ok(repo) = Repository::discover(None) else {
        return false;
    };
    let config = grit_lib::config::ConfigSet::load(Some(&repo.git_dir), true)
        .unwrap_or_else(|_| grit_lib::config::ConfigSet::new());
    let Some(value) = config.get("apply.ignorewhitespace") else {
        return false;
    };
    matches!(
        value.to_ascii_lowercase().as_str(),
        "change" | "true" | "yes" | "on" | "1"
    )
}

fn config_whitespace_rule_bits() -> u32 {
    let Ok(repo) = Repository::discover(None) else {
        return WS_DEFAULT_RULE;
    };
    let config = grit_lib::config::ConfigSet::load(Some(&repo.git_dir), true)
        .unwrap_or_else(|_| grit_lib::config::ConfigSet::new());
    let Some(value) = config.get("core.whitespace") else {
        return WS_DEFAULT_RULE;
    };
    ws::parse_whitespace_rule(&value).unwrap_or(WS_DEFAULT_RULE)
}

fn config_tab_width() -> usize {
    ws::ws_tab_width(config_whitespace_rule_bits()).max(1)
}

fn config_whitespace_fix() -> bool {
    let Ok(repo) = Repository::discover(None) else {
        return false;
    };
    let config = grit_lib::config::ConfigSet::load(Some(&repo.git_dir), true)
        .unwrap_or_else(|_| grit_lib::config::ConfigSet::new());
    let Some(value) = config.get("apply.whitespace") else {
        return false;
    };
    value.eq_ignore_ascii_case("fix")
}

fn whitespace_option_was_explicitly_set() -> bool {
    std::env::args().any(|arg| arg == "--whitespace" || arg.starts_with("--whitespace="))
}

fn resolve_apply_whitespace_mode(args: &Args) -> ApplyWhitespaceMode {
    let ws_cli = if args.whitespace.eq_ignore_ascii_case("nowarn") {
        WsCliMode::NoWarn
    } else if args.whitespace.eq_ignore_ascii_case("warn") {
        WsCliMode::Warn
    } else if args.whitespace.eq_ignore_ascii_case("error")
        || args.whitespace.eq_ignore_ascii_case("error-all")
    {
        WsCliMode::Error
    } else if args.whitespace.eq_ignore_ascii_case("fix")
        || args.whitespace.eq_ignore_ascii_case("strip")
    {
        WsCliMode::Fix
    } else {
        WsCliMode::Warn
    };

    // When `--whitespace` is omitted, Git defaults to `warn` but may still enable fix via
    // `apply.whitespace=fix`. When the user passes `--whitespace=...`, that choice wins
    // (t4125: `--whitespace=nowarn` must not strip added lines because config says fix).
    let whitespace_fix = if whitespace_option_was_explicitly_set() {
        matches!(ws_cli, WsCliMode::Fix)
    } else if matches!(ws_cli, WsCliMode::Warn) && config_whitespace_fix() {
        true
    } else {
        matches!(ws_cli, WsCliMode::Fix)
    };
    let ignore_space_change = if args.no_ignore_whitespace {
        false
    } else if args.ignore_whitespace || args.ignore_space_change {
        true
    } else {
        config_ignore_space_change()
    };
    let ws_squelch = if args.whitespace.eq_ignore_ascii_case("error-all") {
        0u32
    } else {
        5u32
    };

    ApplyWhitespaceMode {
        whitespace_fix,
        ignore_space_change,
        inaccurate_eof: args.inaccurate_eof,
        tab_width: config_tab_width(),
        context: args.context,
        ws_cli,
        ws_squelch,
        unidiff_zero: args.unidiff_zero,
    }
}

/// Represents one file in a unified diff.
#[derive(Debug, Clone)]
struct FilePatch {
    /// Path from `diff --git` old side (`a/...`) when present.
    diff_old_path: Option<String>,
    /// Path from `diff --git` new side (`b/...`) when present.
    diff_new_path: Option<String>,
    /// Path on the old side (None for new files).
    old_path: Option<String>,
    /// Path on the new side (None for deleted files).
    new_path: Option<String>,
    /// Whether an explicit `---` header line was present.
    saw_old_header: bool,
    /// Whether an explicit `+++` header line was present.
    saw_new_header: bool,
    /// Old mode from extended header.
    old_mode: Option<String>,
    /// New mode from extended header.
    new_mode: Option<String>,
    /// Whether this file is being newly created.
    is_new: bool,
    /// Whether this file is being deleted.
    is_deleted: bool,
    /// Whether this is a rename.
    is_rename: bool,
    /// Whether this is a copy.
    is_copy: bool,
    /// Similarity index (e.g., 90 for 90%).
    similarity_index: Option<u32>,
    /// Dissimilarity index for rewrites.
    dissimilarity_index: Option<u32>,
    /// Old blob OID from the index header (abbreviated).
    old_oid: Option<String>,
    /// New blob OID from the index header (abbreviated).
    new_oid: Option<String>,
    /// Parsed binary patch payload (`GIT binary patch`) if present.
    binary_patch: Option<BinaryPatchPayload>,
    /// Hunks to apply.
    hunks: Vec<Hunk>,
    /// Merged `core.whitespace` + `whitespace` attribute (Git `ws_rule`); `0` before assignment.
    ws_rule: u32,
}

/// Binary patch payload as compressed base85 chunks for forward/reverse apply.
#[derive(Debug, Clone)]
struct BinaryPatchPayload {
    #[allow(dead_code)]
    forward_compressed: Vec<u8>,
    #[allow(dead_code)]
    forward_declared_size: usize,
    #[allow(dead_code)]
    reverse_compressed: Vec<u8>,
    #[allow(dead_code)]
    reverse_declared_size: usize,
}

impl FilePatch {
    /// Effective path for the file.
    /// For deletions, use old_path (new is /dev/null).
    /// For additions, use new_path (old is /dev/null).
    /// Otherwise prefer new_path.
    fn effective_path(&self) -> Option<&str> {
        if self.is_deleted {
            return self
                .old_path
                .as_deref()
                .filter(|p| *p != "/dev/null")
                .or(self.new_path.as_deref().filter(|p| *p != "/dev/null"));
        }
        if self.is_new {
            return self
                .new_path
                .as_deref()
                .filter(|p| *p != "/dev/null")
                .or(self.old_path.as_deref().filter(|p| *p != "/dev/null"));
        }
        self.new_path
            .as_deref()
            .filter(|p| *p != "/dev/null")
            .or(self.old_path.as_deref().filter(|p| *p != "/dev/null"))
    }

    /// Source path to read preimage content from.
    ///
    /// For rename/copy patches this is the old path, otherwise this is the
    /// effective path.
    fn source_path(&self) -> Option<&str> {
        if self.is_rename || self.is_copy {
            self.old_path
                .as_deref()
                .filter(|p| *p != "/dev/null")
                .or(self.effective_path())
        } else if let (Some(old), Some(new)) = (self.old_path.as_deref(), self.new_path.as_deref())
        {
            if old != "/dev/null" && new != "/dev/null" && old != new {
                Some(old)
            } else {
                self.effective_path()
            }
        } else {
            self.effective_path()
        }
    }

    /// Destination path to write postimage content to.
    ///
    /// For additions/renames/copies this is the new path, otherwise this is
    /// the effective path.
    fn target_path(&self) -> Option<&str> {
        if self.is_new || self.is_rename || self.is_copy {
            self.new_path
                .as_deref()
                .filter(|p| *p != "/dev/null")
                .or(self.effective_path())
        } else {
            self.effective_path()
        }
    }

    /// True when this patch touches a gitlink/submodule (mode `160000`).
    fn involves_gitlink(&self) -> bool {
        self.old_mode.as_deref() == Some("160000") || self.new_mode.as_deref() == Some("160000")
    }
}

/// Outcome of a `--3way` attempt for one file (blob-level).
#[derive(Debug)]
struct ThreeWayBlobResult {
    merged_bytes: Vec<u8>,
    conflicted: bool,
    /// Stage blobs when `conflicted` (base, ours, theirs); unused entries are zero OID for add/add base.
    stages: [ObjectId; 3],
}

fn resolve_apply_conflict_style(repo: &Repository) -> ConflictStyle {
    let Ok(config) = ConfigSet::load(Some(&repo.git_dir), true) else {
        return ConflictStyle::Merge;
    };
    match config
        .get("merge.conflictstyle")
        .unwrap_or_default()
        .to_lowercase()
        .as_str()
    {
        "diff3" => ConflictStyle::Diff3,
        "zdiff3" => ConflictStyle::ZealousDiff3,
        _ => ConflictStyle::Merge,
    }
}

fn merge_favor_from_apply_args(args: &Args) -> MergeFavor {
    if args.union {
        MergeFavor::Union
    } else if args.theirs {
        MergeFavor::Theirs
    } else if args.ours {
        MergeFavor::Ours
    } else {
        MergeFavor::None
    }
}

fn effective_merge_favor_for_path(file_attrs: &FileAttrs, cli: MergeFavor) -> MergeFavor {
    if matches!(file_attrs.merge, MergeAttr::Driver(ref s) if s == "union") {
        MergeFavor::Union
    } else {
        cli
    }
}

fn conflict_marker_size_for_path(file_attrs: &FileAttrs) -> usize {
    file_attrs
        .conflict_marker_size
        .as_deref()
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(7)
}

fn count_hunk_line_changes(fp: &FilePatch) -> (usize, usize) {
    let mut add = 0usize;
    let mut del = 0usize;
    for h in &fp.hunks {
        for line in &h.lines {
            match line {
                HunkLine::Add(_) => add += 1,
                HunkLine::Remove(_) => del += 1,
                _ => {}
            }
        }
    }
    (add, del)
}

/// Try Git's `apply --3way` in-core merge: base = patch preimage, theirs = patch postimage, ours = `our_bytes`.
fn try_three_way_merge_blob(
    repo: &Repository,
    fp: &FilePatch,
    our_bytes: &[u8],
    ws_mode: ApplyWhitespaceMode,
    forward_apply: bool,
    patch_input_display: &str,
    favor: MergeFavor,
    style: ConflictStyle,
    file_attrs: &FileAttrs,
    direct_to_threeway: bool,
) -> Result<Option<ThreeWayBlobResult>> {
    if !forward_apply {
        return Ok(None);
    }
    if fp.is_deleted {
        return Ok(None);
    }
    if fp.involves_gitlink() {
        return Ok(None);
    }
    if fp.is_rename {
        let (a, d) = count_hunk_line_changes(fp);
        if a == 0 && d == 0 {
            return Ok(None);
        }
    }
    if fp.is_new && !direct_to_threeway {
        return Ok(None);
    }
    let (pre_bytes, preimage_oid): (Vec<u8>, Option<ObjectId>) = if fp.is_new {
        (Vec::new(), None)
    } else {
        let Some(prefix) = fp.old_oid.as_deref() else {
            return Ok(None);
        };
        let oid = resolve_revision_for_patch_old_blob(repo, prefix)?;
        let obj = repo.odb.read(&oid)?;
        (obj.data, Some(oid))
    };

    let post_bytes: Vec<u8> = if let Some(bin) = fp.binary_patch.as_ref() {
        inflate_binary_payload(&bin.forward_compressed)?
    } else if fp.hunks.is_empty() {
        pre_bytes.clone()
    } else {
        let pre_text = String::from_utf8_lossy(&pre_bytes);
        let post_text = apply_hunks(
            pre_text.as_ref(),
            fp,
            ws_mode,
            forward_apply,
            patch_input_display,
        )?;
        post_text.into_bytes()
    };

    if pre_bytes == our_bytes {
        let oid_base = preimage_oid.unwrap_or(ObjectId::zero());
        let oid_ours = repo.odb.write(ObjectKind::Blob, our_bytes)?;
        let oid_theirs = repo.odb.write(ObjectKind::Blob, &post_bytes)?;
        return Ok(Some(ThreeWayBlobResult {
            merged_bytes: post_bytes,
            conflicted: false,
            stages: [oid_base, oid_ours, oid_theirs],
        }));
    }
    if pre_bytes == post_bytes || our_bytes == post_bytes {
        let oid_base = preimage_oid.unwrap_or(ObjectId::zero());
        let oid_ours = repo.odb.write(ObjectKind::Blob, our_bytes)?;
        let oid_theirs = repo.odb.write(ObjectKind::Blob, &post_bytes)?;
        return Ok(Some(ThreeWayBlobResult {
            merged_bytes: our_bytes.to_vec(),
            conflicted: false,
            stages: [oid_base, oid_ours, oid_theirs],
        }));
    }

    let base_hex = preimage_oid.map(|o| o.to_hex()).unwrap_or_default();
    let abbrev_len = 7usize;
    let base_abbrev = if base_hex.is_empty() {
        String::new()
    } else if base_hex.len() > abbrev_len {
        base_hex[..abbrev_len].to_string()
    } else {
        base_hex.clone()
    };
    let label_base = match style {
        ConflictStyle::ZealousDiff3 if base_abbrev.chars().all(|c| c.is_ascii_hexdigit()) => {
            base_abbrev.clone()
        }
        ConflictStyle::Diff3 | ConflictStyle::ZealousDiff3 if !base_abbrev.is_empty() => {
            format!("{base_abbrev}:content")
        }
        _ => "base".to_string(),
    };

    let merged = merge(&MergeInput {
        base: &pre_bytes,
        ours: our_bytes,
        theirs: &post_bytes,
        label_ours: "ours",
        label_base: &label_base,
        label_theirs: "theirs",
        favor,
        style,
        marker_size: conflict_marker_size_for_path(file_attrs),
        diff_algorithm: None,
        ignore_all_space: false,
        ignore_space_change: false,
        ignore_space_at_eol: false,
        ignore_cr_at_eol: false,
    })
    .map_err(|e| anyhow::anyhow!("{e}"))?;

    let oid_base = preimage_oid.unwrap_or(ObjectId::zero());
    let oid_ours = repo.odb.write(ObjectKind::Blob, our_bytes)?;
    let oid_theirs = repo.odb.write(ObjectKind::Blob, &post_bytes)?;

    Ok(Some(ThreeWayBlobResult {
        merged_bytes: merged.content,
        conflicted: merged.conflicts > 0,
        stages: [oid_base, oid_ours, oid_theirs],
    }))
}

fn add_conflicted_index_stages(
    index: &mut Index,
    path_bytes: &[u8],
    mode: u32,
    stages: &[ObjectId; 3],
) {
    index.entries.retain(|e| e.path != path_bytes);
    for (stage_idx, oid) in stages.iter().enumerate() {
        if oid.is_zero() {
            continue;
        }
        let stage = (stage_idx + 1) as u8;
        let entry = IndexEntry {
            ctime_sec: 0,
            ctime_nsec: 0,
            mtime_sec: 0,
            mtime_nsec: 0,
            dev: 0,
            ino: 0,
            mode,
            uid: 0,
            gid: 0,
            size: 0,
            oid: *oid,
            flags: ((path_bytes.len().min(0xFFF)) as u16 & 0x0FFF) | ((stage as u16) << 12),
            flags_extended: None,
            path: path_bytes.to_vec(),
        };
        index.add_or_replace(entry);
    }
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// Strip trailing `\r` and surrounding whitespace from parsed header tokens.
///
/// `git diff` may emit CRLF line endings; without this, `new mode 160000\r` fails to match
/// submodule handling (`t4137-apply-submodule`).
fn sanitize_patch_header_value(s: &mut String) {
    *s = s.trim().trim_end_matches('\r').to_string();
}

/// Strip Git's `diff --git a/... b/...` path prefix when it leaked into stored paths.
///
/// Binary patches often omit `---`/`+++` lines that would normally resynchronize names; without
/// this, paths like `a/bin.png` are misinterpreted as real file paths (`t4108-apply-threeway`).
fn strip_git_diff_path_prefix(path: &str) -> String {
    if path == "/dev/null" {
        return path.to_string();
    }
    let p = path.trim_start_matches("./");
    if let Some(rest) = p.strip_prefix("a/") {
        return rest.to_string();
    }
    if let Some(rest) = p.strip_prefix("b/") {
        return rest.to_string();
    }
    path.to_string()
}

fn sanitize_file_patch_headers(fp: &mut FilePatch) {
    if let Some(ref mut s) = fp.old_mode {
        sanitize_patch_header_value(s);
    }
    if let Some(ref mut s) = fp.new_mode {
        sanitize_patch_header_value(s);
    }
    if let Some(ref mut s) = fp.old_oid {
        sanitize_patch_header_value(s);
    }
    if let Some(ref mut s) = fp.new_oid {
        sanitize_patch_header_value(s);
    }
    for ref mut s in [
        &mut fp.diff_old_path,
        &mut fp.diff_new_path,
        &mut fp.old_path,
        &mut fp.new_path,
    ]
    .into_iter()
    .flatten()
    {
        sanitize_patch_header_value(s);
        **s = strip_git_diff_path_prefix(s);
    }
}

/// Collapse runs of `/` to a single slash (Git `squash_slash`).
fn squash_slash_path(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_slash = false;
    for ch in s.chars() {
        if ch == '/' {
            if !prev_slash {
                out.push('/');
            }
            prev_slash = true;
        } else {
            prev_slash = false;
            out.push(ch);
        }
    }
    out
}

/// Unquote a leading C-style `"..."` from `line`; returns decoded bytes and remainder after closing `"`.
/// Matches Git `unquote_c_style` / `quote.c` escapes used in diff headers.
fn unquote_c_style_diff_prefix(line: &str) -> Option<(Vec<u8>, &str)> {
    let b = line.as_bytes();
    if b.first() != Some(&b'"') {
        return None;
    }
    let mut q = &b[1..];
    let mut out = Vec::new();
    loop {
        let len = q
            .iter()
            .position(|&c| c == b'"' || c == b'\\')
            .unwrap_or(q.len());
        out.extend_from_slice(&q[..len]);
        q = &q[len..];
        if q.is_empty() {
            return None;
        }
        match q[0] {
            b'"' => {
                let rest = std::str::from_utf8(&q[1..]).ok()?;
                return Some((out, rest));
            }
            b'\\' => {
                q = &q[1..];
                if q.is_empty() {
                    return None;
                }
                let ch = q[0];
                q = &q[1..];
                match ch {
                    b'a' => out.push(0x07),
                    b'b' => out.push(0x08),
                    b'f' => out.push(0x0c),
                    b'n' => out.push(b'\n'),
                    b'r' => out.push(b'\r'),
                    b't' => out.push(b'\t'),
                    b'v' => out.push(0x0b),
                    b'\\' => out.push(b'\\'),
                    b'"' => out.push(b'"'),
                    b'0'..=b'3' => {
                        if q.len() < 2 {
                            return None;
                        }
                        let ch2 = q[0];
                        let ch3 = q[1];
                        if !(b'0'..=b'7').contains(&ch2) || !(b'0'..=b'7').contains(&ch3) {
                            return None;
                        }
                        let ac = u32::from(ch - b'0') * 64
                            + u32::from(ch2 - b'0') * 8
                            + u32::from(ch3 - b'0');
                        out.push(ac as u8);
                        q = &q[2..];
                    }
                    _ => return None,
                }
            }
            _ => return None,
        }
    }
}

fn bytes_to_path_string(bytes: &[u8]) -> Result<String> {
    let s = String::from_utf8(bytes.to_vec()).context("diff path is not valid UTF-8")?;
    Ok(squash_slash_path(&s))
}

/// Skip `p_value` leading path components (Git `skip_tree_prefix`); `p_value == 0` allows absolute paths.
fn skip_tree_prefix_bytes(line: &[u8], p_value: usize) -> Option<&[u8]> {
    if p_value == 0 {
        return Some(line);
    }
    let mut nslash = p_value;
    let mut i = 0usize;
    while i < line.len() {
        if line[i] == b'/' {
            nslash = nslash.saturating_sub(1);
            if nslash == 0 {
                return if i == 0 { None } else { Some(&line[i + 1..]) };
            }
        }
        i += 1;
    }
    None
}

/// Strip `p_value` leading `/`-separated components from a UTF-8 path (for `rename from` etc.).
fn skip_tree_prefix_str(path: &str, p_value: usize) -> Option<String> {
    let stripped = skip_tree_prefix_bytes(path.as_bytes(), p_value)?;
    Some(String::from_utf8_lossy(stripped).into_owned())
}

fn sane_tz_len(line: &[u8]) -> usize {
    const SUFFIX: &[u8] = b" +0500";
    if line.len() < SUFFIX.len() || line[line.len() - SUFFIX.len()] != b' ' {
        return 0;
    }
    let tz = &line[line.len() - SUFFIX.len()..];
    if tz[1] != b'+' && tz[1] != b'-' {
        return 0;
    }
    for p in &tz[2..] {
        if !p.is_ascii_digit() {
            return 0;
        }
    }
    SUFFIX.len()
}

fn tz_with_colon_len(line: &[u8]) -> usize {
    // Git: suffix is ` ±HH:MM` (space, sign, two hour digits, colon, two minute digits) = 7 bytes.
    const SUFFIX_LEN: usize = 7;
    if line.len() < SUFFIX_LEN || line[line.len() - 3] != b':' {
        return 0;
    }
    let tz = &line[line.len() - SUFFIX_LEN..];
    if tz[0] != b' ' || (tz[1] != b'+' && tz[1] != b'-') {
        return 0;
    }
    let p = &tz[2..];
    if p.len() != 5
        || !p[0].is_ascii_digit()
        || !p[1].is_ascii_digit()
        || p[2] != b':'
        || !p[3].is_ascii_digit()
        || !p[4].is_ascii_digit()
    {
        return 0;
    }
    SUFFIX_LEN
}

fn date_len(line: &[u8]) -> usize {
    const SHORT: &[u8] = b"72-02-05";
    if line.len() < SHORT.len() || line[line.len() - 3] != b'-' {
        return 0;
    }
    let mut p = line.len() - SHORT.len();
    let date = &line[p..];
    if !date[0].is_ascii_digit()
        || !date[1].is_ascii_digit()
        || date[2] != b'-'
        || !date[3].is_ascii_digit()
        || !date[4].is_ascii_digit()
        || date[5] != b'-'
        || !date[6].is_ascii_digit()
        || !date[7].is_ascii_digit()
    {
        return 0;
    }
    if p >= 2 {
        let y1 = line[p - 1];
        let y2 = line[p - 2];
        if y1.is_ascii_digit() && y2.is_ascii_digit() {
            p -= 2;
        }
    }
    line.len() - p
}

fn short_time_len(line: &[u8]) -> usize {
    const PAT: &[u8] = b" 07:01:32";
    if line.len() < PAT.len() || line[line.len() - 3] != b':' {
        return 0;
    }
    let p = line.len() - PAT.len();
    let time = &line[p..];
    if time[0] != b' '
        || !time[1].is_ascii_digit()
        || !time[2].is_ascii_digit()
        || time[3] != b':'
        || !time[4].is_ascii_digit()
        || !time[5].is_ascii_digit()
        || time[6] != b':'
        || !time[7].is_ascii_digit()
        || !time[8].is_ascii_digit()
    {
        return 0;
    }
    PAT.len()
}

fn fractional_time_len(line: &[u8]) -> usize {
    if line.is_empty() || !line[line.len() - 1].is_ascii_digit() {
        return 0;
    }
    let mut p = line.len() - 1;
    while p > 0 && line[p].is_ascii_digit() {
        p -= 1;
    }
    if p == 0 || line[p] != b'.' {
        return 0;
    }
    let n = short_time_len(&line[..p]);
    if n == 0 {
        return 0;
    }
    line.len() - p + n
}

fn trailing_spaces_len(line: &[u8]) -> usize {
    if line.is_empty() || line[line.len() - 1] != b' ' {
        return 0;
    }
    let mut p = line.len();
    while p > 0 {
        p -= 1;
        if line[p] != b' ' {
            return line.len() - (p + 1);
        }
    }
    line.len()
}

fn diff_timestamp_len(line: &[u8]) -> usize {
    if line.is_empty() || !line[line.len() - 1].is_ascii_digit() {
        return 0;
    }
    let mut end = line.len();
    let mut n = sane_tz_len(&line[..end]);
    if n == 0 {
        n = tz_with_colon_len(&line[..end]);
    }
    if n == 0 {
        return 0;
    }
    end -= n;

    n = short_time_len(&line[..end]);
    if n == 0 {
        n = fractional_time_len(&line[..end]);
    }
    if n == 0 {
        return 0;
    }
    end -= n;

    n = date_len(&line[..end]);
    if n == 0 {
        return 0;
    }
    end -= n;

    if end == 0 {
        return 0;
    }
    match line[end - 1] {
        b'\t' => {
            end -= 1;
            line.len() - end
        }
        b' ' => {
            end -= trailing_spaces_len(&line[..end]);
            line.len() - end
        }
        _ => 0,
    }
}

/// Git `find_name_common` with optional `end` bound (exclusive).
fn find_name_common_bounded(
    line: &[u8],
    def: Option<&[u8]>,
    p_value: usize,
    end: usize,
) -> Option<Vec<u8>> {
    let end = end.min(line.len());
    let mut start: Option<usize> = if p_value == 0 { Some(0) } else { None };
    let mut p = p_value;
    let mut i = 0usize;
    while i < end {
        let c = line[i];
        i += 1;
        if c == b'/' && p > 0 {
            p -= 1;
            if p == 0 {
                start = Some(i);
            }
        }
    }
    let start = start?;
    let len = i - start;
    if len == 0 {
        return def.map(|d| d.to_vec());
    }
    let slice = &line[start..i];
    if let Some(d) = def {
        if d.len() < len && slice.starts_with(d) {
            return Some(d.to_vec());
        }
    }
    Some(slice.to_vec())
}

/// Git `find_name_traditional` on the line after `--- ` / `+++ ` (no prefix).
fn find_name_traditional(line: &[u8], def: Option<&[u8]>, p_value: usize) -> Option<Vec<u8>> {
    if line.first() == Some(&b'"') {
        let (decoded, _) = unquote_c_style_diff_prefix(std::str::from_utf8(line).ok()?)?;
        let skip = skip_tree_prefix_bytes(&decoded, p_value)?;
        return Some(skip.to_vec());
    }
    let ts = diff_timestamp_len(line);
    let name_end = line.len().saturating_sub(ts);
    find_name_common_bounded(line, def, p_value, name_end)
}

fn find_name_tab_terminated(line: &[u8], p_value: usize) -> Option<Vec<u8>> {
    if line.first() == Some(&b'"') {
        let (decoded, _) = unquote_c_style_diff_prefix(std::str::from_utf8(line).ok()?)?;
        let skip = skip_tree_prefix_bytes(&decoded, p_value)?;
        return Some(skip.to_vec());
    }
    let end = line
        .iter()
        .position(|&b| b == b'\t' || b == b'\n' || b == b'\r')
        .unwrap_or(line.len());
    find_name_common_bounded(line, None, p_value, end)
}

fn is_dev_null_nameline(line: &[u8]) -> bool {
    line.strip_prefix(b"/dev/null")
        .map(|rest| rest.is_empty() || rest.first().is_some_and(|b| b.is_ascii_whitespace()))
        .unwrap_or(false)
}

fn guess_p_value_from_nameline(line: &[u8]) -> Option<usize> {
    if is_dev_null_nameline(line) {
        return None;
    }
    let name = find_name_traditional(line, None, 0)?;
    let name_str = String::from_utf8_lossy(&name);
    if !name_str.contains('/') {
        return Some(0);
    }
    None
}

fn epoch_stamp_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^([0-2][0-9]):([0-5][0-9]):00(?:\.0+)? ([-+][0-2][0-9]:?[0-5][0-9])")
            .expect("epoch stamp regex")
    })
}

/// True when the `---`/`+++` line has a tab-separated epoch timestamp (Git `has_epoch_timestamp`).
fn has_epoch_timestamp(nameline: &[u8]) -> bool {
    let Some(tab) = nameline.iter().position(|&b| b == b'\t') else {
        return false;
    };
    let mut ts = &nameline[tab + 1..];
    let epoch_hour = if let Some(r) = ts.strip_prefix(b"1969-12-31 ") {
        ts = r;
        24i32
    } else if let Some(r) = ts.strip_prefix(b"1970-01-01 ") {
        ts = r;
        0i32
    } else {
        return false;
    };
    let end = ts.iter().position(|&b| b == b'\n').unwrap_or(ts.len());
    let stamp = &ts[..end];
    let stamp_str = match std::str::from_utf8(stamp) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let caps = match epoch_stamp_regex().captures(stamp_str) {
        Some(c) => c,
        None => return false,
    };
    let hour: i32 = caps
        .get(1)
        .and_then(|m| m.as_str().parse().ok())
        .unwrap_or(-1);
    let minute: i32 = caps
        .get(2)
        .and_then(|m| m.as_str().parse().ok())
        .unwrap_or(-1);
    let tz_s = match caps.get(3).map(|m| m.as_str()) {
        Some(s) if !s.is_empty() => s,
        _ => return false,
    };
    if hour < 0 || minute < 0 {
        return false;
    }
    let tz_byte = tz_s.as_bytes()[0];
    let tz_rest = &tz_s[1..];
    let zoneoffset: i32 = if let Some(colon_pos) = tz_rest.find(':') {
        let h: i32 = tz_rest[..colon_pos].parse().unwrap_or(0);
        let mm: i32 = tz_rest[colon_pos + 1..].parse().unwrap_or(0);
        h * 60 + mm
    } else if tz_rest.len() >= 4 {
        let n: i32 = tz_rest[..4].parse().unwrap_or(0);
        (n / 100) * 60 + (n % 100)
    } else {
        return false;
    };
    let zoneoffset = if tz_byte == b'-' {
        -zoneoffset
    } else {
        zoneoffset
    };
    hour * 60 + minute - zoneoffset == epoch_hour * 60
}

/// Parse `---` / `+++` pair for a traditional unified diff (Git `parse_traditional_patch`).
fn parse_traditional_patch_pair(
    old_line: &[u8],
    new_line: &[u8],
    strip: usize,
    p_guess: &mut Option<usize>,
) -> Result<FilePatch> {
    let old_p = old_line.strip_prefix(b"--- ").unwrap_or(old_line);
    let new_p = new_line.strip_prefix(b"+++ ").unwrap_or(new_line);

    if p_guess.is_none() {
        let p = guess_p_value_from_nameline(old_p);
        let q = guess_p_value_from_nameline(new_p);
        let chosen = match (p, q) {
            (None, None) => None,
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (Some(a), Some(b)) if a == b => Some(a),
            _ => None,
        };
        *p_guess = chosen;
    }
    let p_val = p_guess.unwrap_or(strip);

    let mut fp = FilePatch {
        diff_old_path: None,
        diff_new_path: None,
        old_path: None,
        new_path: None,
        saw_old_header: true,
        saw_new_header: true,
        old_mode: None,
        new_mode: None,
        is_new: false,
        is_deleted: false,
        is_rename: false,
        is_copy: false,
        similarity_index: None,
        dissimilarity_index: None,
        old_oid: None,
        new_oid: None,
        binary_patch: None,
        hunks: Vec::new(),
        ws_rule: 0,
    };

    if is_dev_null_nameline(old_p) {
        fp.is_new = true;
        let name = find_name_traditional(new_p, None, p_val)
            .ok_or_else(|| anyhow::anyhow!("unable to find filename in traditional patch"))?;
        fp.new_path = Some(bytes_to_path_string(&name)?);
    } else if is_dev_null_nameline(new_p) {
        fp.is_deleted = true;
        let name = find_name_traditional(old_p, None, p_val)
            .ok_or_else(|| anyhow::anyhow!("unable to find filename in traditional patch"))?;
        fp.old_path = Some(bytes_to_path_string(&name)?);
    } else {
        let first_name = find_name_traditional(old_p, None, p_val)
            .ok_or_else(|| anyhow::anyhow!("unable to find filename in traditional patch"))?;
        let name = find_name_traditional(new_p, Some(&first_name), p_val)
            .ok_or_else(|| anyhow::anyhow!("unable to find filename in traditional patch"))?;
        let name_str = bytes_to_path_string(&name)?;
        if has_epoch_timestamp(old_p) {
            fp.is_new = true;
            fp.new_path = Some(name_str);
        } else if has_epoch_timestamp(new_p) {
            fp.is_deleted = true;
            fp.old_path = Some(name_str);
        } else {
            // Git uses the `+++` filename for both sides when neither line carries an epoch
            // marker; the `---` line only participates via `def` when shortening `.orig` etc.
            fp.old_path = Some(name_str.clone());
            fp.new_path = Some(name_str);
        }
    }

    Ok(fp)
}

/// Default filename from `diff --git` when both sides agree (Git `git_header_name`).
fn git_header_def_name(line: &str, p_value: usize) -> Option<String> {
    let rest = line.strip_prefix("diff --git ").unwrap_or(line);
    let rest_b = rest.as_bytes();

    if rest_b.first() == Some(&b'"') {
        let (first_decoded, second_raw) = unquote_c_style_diff_prefix(rest)?;
        let rel_first = skip_tree_prefix_bytes(&first_decoded, p_value)?;
        let second = second_raw.trim_start_matches(|c: char| c.is_ascii_whitespace());
        if second.is_empty() {
            return None;
        }
        if second.as_bytes().first() == Some(&b'"') {
            let (second_decoded, _) = unquote_c_style_diff_prefix(second)?;
            let rel2 = skip_tree_prefix_bytes(&second_decoded, p_value)?;
            if rel2 != rel_first {
                return None;
            }
        } else {
            let rel2 = skip_tree_prefix_bytes(second.as_bytes(), p_value)?;
            if rel2.len() != rel_first.len() || rel2 != rel_first {
                return None;
            }
        }
        return bytes_to_path_string(rel_first).ok();
    }

    let name = skip_tree_prefix_bytes(rest_b, p_value)?;
    let name_start = name.as_ptr() as usize - rest_b.as_ptr() as usize;

    for offset in 0..name.len() {
        if name[offset] != b'"' {
            continue;
        }
        let second_slice = &rest_b[name_start + offset..];
        let (decoded, _) = unquote_c_style_diff_prefix(std::str::from_utf8(second_slice).ok()?)?;
        let np = skip_tree_prefix_bytes(&decoded, p_value)?;
        let plen = np.len();
        if plen < offset
            && name.len() > plen
            && &name[..plen] == np
            && name[plen].is_ascii_whitespace()
        {
            return bytes_to_path_string(np).ok();
        }
        return None;
    }

    let line_len = rest.len().saturating_sub(name_start);
    let mut len = 0usize;
    while len < line_len {
        match rest_b[name_start + len] {
            b'\n' => return None,
            b'\t' | b' ' => {
                let after = name_start + len + 1;
                if after > name_start + line_len {
                    return None;
                }
                let second =
                    skip_tree_prefix_bytes(&rest_b[after..name_start + line_len], p_value)?;
                let names_match =
                    name.len() >= len && second.len() >= len && name[..len] == second[..len];
                let boundary_ok = second.get(len) == Some(&b'\n') || second.len() == len;
                if names_match && boundary_ok {
                    return bytes_to_path_string(&name[..len]).ok();
                }
            }
            _ => {}
        }
        len += 1;
    }
    None
}

/// Git `name_terminate` for `---`/`+++` parsing (`TERM_TAB` only).
fn diff_header_name_terminate(c: u8) -> bool {
    const TERM_SPACE: u8 = 1;
    const TERM_TAB: u8 = 2;
    let terminate = TERM_TAB;
    if c == b' ' && (terminate & TERM_SPACE) == 0 {
        return false;
    }
    if c == b'\t' && (terminate & TERM_TAB) == 0 {
        return false;
    }
    true
}

/// Extract the file path from the remainder of a `---` / `+++` header line,
/// matching Git's `find_name(..., TERM_TAB)`.
///
/// Returns `None` for quoted GNU-style names (not handled here).
fn find_name_in_diff_header_line(line: &str, p_value: usize) -> Option<String> {
    let b = line.as_bytes();
    if b.first() == Some(&b'"') {
        return None;
    }

    let mut i = 0usize;
    let mut start = if p_value == 0 { Some(0usize) } else { None };
    let mut p = p_value;

    while i < b.len() {
        let c = b[i];
        if c.is_ascii_whitespace() {
            if c == b'\n' || c == b'\r' {
                break;
            }
            if diff_header_name_terminate(c) {
                break;
            }
        }
        i += 1;
        if c == b'/' && p > 0 {
            p -= 1;
            if p == 0 {
                start = Some(i);
            }
        }
    }

    let start = start?;
    let end = i;
    (end > start).then(|| line[start..end].to_string())
}

/// True when the name portion of a `---`/`+++` line is Git's `/dev/null` sentinel.
fn is_dev_null_diff_name(line: &str) -> bool {
    line.strip_prefix("/dev/null")
        .map(|rest| {
            rest.is_empty()
                || rest
                    .as_bytes()
                    .first()
                    .is_some_and(|b| b.is_ascii_whitespace())
        })
        .unwrap_or(false)
}

/// Like [`find_name_in_diff_header_line`], but never returns `None` for normal
/// unquoted lines (falls back to text before the first tab).
fn header_line_file_path(line: &str, p_value: usize) -> String {
    let line = line.trim_end_matches(['\r', '\n']);
    if is_dev_null_diff_name(line) {
        return "/dev/null".to_string();
    }
    find_name_in_diff_header_line(line, p_value)
        .unwrap_or_else(|| line.split('\t').next().unwrap_or(line).to_string())
}

/// Path from `rename from` / `copy from` lines (Git `find_name` with `terminate == 0`).
fn find_name_extended_header(rest: &str, p_extended: usize) -> Option<String> {
    let rest = rest.trim_end_matches(['\r', '\n']);
    let b = rest.as_bytes();
    if b.first() == Some(&b'"') {
        let (decoded, tail) = unquote_c_style_diff_prefix(rest)?;
        if !tail.trim().is_empty() {
            return None;
        }
        let skip = skip_tree_prefix_bytes(&decoded, p_extended)?;
        return bytes_to_path_string(skip).ok();
    }
    let end = b
        .iter()
        .position(|&c| c == b'\t' || c == b'\n' || c == b'\r' || c == b' ')
        .unwrap_or(b.len());
    let name = find_name_common_bounded(b, None, p_extended, end)?;
    bytes_to_path_string(&name).ok()
}

/// Parse a unified diff into a list of `FilePatch` entries.
///
/// `strip` is Git's `p_value` (`-p` count, default 1).
fn parse_patch(input: &str, strip: usize) -> Result<Vec<FilePatch>> {
    let lines: Vec<&str> = input.lines().collect();
    let mut patches = Vec::new();
    let mut i = 0;
    let mut p_guess_for_traditional: Option<usize> = None;

    let p_strip = strip;
    let p_extended = strip.saturating_sub(1);

    while i < lines.len() {
        // Look for "diff --git" header or a bare ---/+++ pair.
        if lines[i].starts_with("diff --git ") {
            let mut fp = FilePatch {
                diff_old_path: None,
                diff_new_path: None,
                old_path: None,
                new_path: None,
                saw_old_header: false,
                saw_new_header: false,
                old_mode: None,
                new_mode: None,
                is_new: false,
                is_deleted: false,
                is_rename: false,
                is_copy: false,
                similarity_index: None,
                dissimilarity_index: None,
                old_oid: None,
                new_oid: None,
                binary_patch: None,
                hunks: Vec::new(),
                ws_rule: 0,
            };

            let header_line = lines[i];
            let def_name = git_header_def_name(header_line, p_strip);

            // Parse "diff --git a/foo b/foo"
            let rest = &header_line["diff --git ".len()..];
            if let Some((a, b)) = split_diff_git_paths(rest) {
                fp.diff_old_path = Some(a.clone());
                fp.diff_new_path = Some(b.clone());
                fp.old_path = Some(a);
                fp.new_path = Some(b);
            }
            i += 1;

            // Parse extended headers
            while i < lines.len()
                && !lines[i].starts_with("--- ")
                && !lines[i].starts_with("diff --git ")
                && !lines[i].starts_with("@@ ")
            {
                let line = lines[i];
                if let Some(val) = line.strip_prefix("old mode ") {
                    fp.old_mode = Some(val.to_string());
                } else if let Some(val) = line.strip_prefix("new mode ") {
                    fp.new_mode = Some(val.to_string());
                } else if let Some(val) = line.strip_prefix("new file mode ") {
                    fp.is_new = true;
                    fp.new_mode = Some(val.to_string());
                } else if let Some(val) = line.strip_prefix("deleted file mode ") {
                    fp.is_deleted = true;
                    fp.old_mode = Some(val.to_string());
                } else if let Some(val) = line.strip_prefix("rename from ") {
                    fp.is_rename = true;
                    if let Some(p) = find_name_extended_header(val, p_extended) {
                        fp.old_path = Some(p);
                    }
                } else if let Some(val) = line.strip_prefix("rename to ") {
                    fp.is_rename = true;
                    if let Some(p) = find_name_extended_header(val, p_extended) {
                        fp.new_path = Some(p);
                    }
                } else if let Some(val) = line.strip_prefix("copy from ") {
                    fp.is_copy = true;
                    if let Some(p) = find_name_extended_header(val, p_extended) {
                        fp.old_path = Some(p);
                    }
                } else if let Some(val) = line.strip_prefix("copy to ") {
                    fp.is_copy = true;
                    if let Some(p) = find_name_extended_header(val, p_extended) {
                        fp.new_path = Some(p);
                    }
                } else if let Some(val) = line.strip_prefix("similarity index ") {
                    fp.similarity_index = val.trim_end_matches('%').parse().ok();
                } else if let Some(val) = line.strip_prefix("dissimilarity index ") {
                    fp.dissimilarity_index = val.trim_end_matches('%').parse().ok();
                } else if let Some(val) = line.strip_prefix("index ") {
                    // Parse "index abc123..def456 100644" or "index abc123..def456"
                    let hash_part = val.split_whitespace().next().unwrap_or("");
                    if let Some((old, new)) = hash_part.split_once("..") {
                        fp.old_oid = Some(old.to_string());
                        fp.new_oid = Some(new.to_string());
                    }
                } else if line == "GIT binary patch" {
                    let (binary_patch, next_i) = parse_binary_patch(&lines, i + 1)?;
                    fp.binary_patch = Some(binary_patch);
                    i = next_i;
                    break;
                }
                // skip other extended headers
                i += 1;
            }

            if let Some(dn) = def_name {
                if fp.old_path.is_none() {
                    fp.old_path = Some(dn.clone());
                }
                if fp.new_path.is_none() {
                    fp.new_path = Some(dn);
                }
            }

            // Parse ---/+++ headers if present
            if i < lines.len() && lines[i].starts_with("--- ") {
                let old_p = lines[i]["--- ".len()..].trim_end_matches(['\r', '\n']);
                let old_b = old_p.as_bytes();
                if is_dev_null_nameline(old_b) {
                    fp.old_path = Some("/dev/null".to_string());
                } else if let Some(p) = find_name_tab_terminated(old_b, p_strip) {
                    fp.old_path = Some(bytes_to_path_string(&p)?);
                }
                fp.saw_old_header = true;
                i += 1;
                if i < lines.len() && lines[i].starts_with("+++ ") {
                    let new_p = lines[i]["+++ ".len()..].trim_end_matches(['\r', '\n']);
                    let new_b = new_p.as_bytes();
                    if is_dev_null_nameline(new_b) {
                        fp.new_path = Some("/dev/null".to_string());
                    } else if let Some(p) = find_name_tab_terminated(new_b, p_strip) {
                        fp.new_path = Some(bytes_to_path_string(&p)?);
                    }
                    fp.saw_new_header = true;
                    i += 1;
                }
            }

            // Parse hunks
            while i < lines.len() && lines[i].starts_with("@@ ") {
                let (hunk, next_i) = parse_hunk(&lines, i)?;
                fp.hunks.push(hunk);
                i = next_i;
            }

            sanitize_file_patch_headers(&mut fp);
            patches.push(fp);
        } else if lines[i].starts_with("--- ")
            && i + 1 < lines.len()
            && lines[i + 1].starts_with("+++ ")
        {
            let old_line = lines[i].as_bytes();
            let new_line = lines[i + 1].as_bytes();
            let mut fp = parse_traditional_patch_pair(
                old_line,
                new_line,
                strip,
                &mut p_guess_for_traditional,
            )?;
            i += 2;

            // Parse hunks
            while i < lines.len() && lines[i].starts_with("@@ ") {
                let (hunk, next_i) = parse_hunk(&lines, i)?;
                fp.hunks.push(hunk);
                i = next_i;
            }

            sanitize_file_patch_headers(&mut fp);
            patches.push(fp);
        } else {
            i += 1;
        }
    }

    Ok(patches)
}

/// Infer `is_new` / `is_deleted` / `is_rename` for submodule diffs that only use mode lines and
/// `---`/`+++` paths (no `new file mode` / `deleted file mode` headers).
fn postprocess_gitlink_file_patches(patches: &mut [FilePatch]) {
    for fp in patches.iter_mut() {
        if !fp.involves_gitlink() {
            continue;
        }
        if fp.is_rename || fp.is_copy || fp.is_new || fp.is_deleted {
            continue;
        }
        let old_p = fp.old_path.as_deref();
        let new_p = fp.new_path.as_deref();
        let old_ok = old_p.is_some_and(|p| p != "/dev/null");
        let new_ok = new_p.is_some_and(|p| p != "/dev/null");
        match (old_ok, new_ok) {
            (true, false) => fp.is_deleted = true,
            (false, true) => fp.is_new = true,
            (true, true) => {
                if old_p != new_p {
                    fp.is_rename = true;
                }
            }
            (false, false) => {}
        }
    }
}

/// `git diff` represents replacing a tracked file with a submodule as two hunks: delete the blob,
/// then add the gitlink. Applying them separately removes the file then fails to create the empty
/// submodule directory (`t4137-apply-submodule`). Merge into one logical patch.
fn merge_adjacent_blob_to_gitlink_patches(patches: &mut Vec<FilePatch>) {
    let mut i = 0usize;
    while i + 1 < patches.len() {
        let del = &patches[i];
        let add = &patches[i + 1];
        let del_path = del
            .old_path
            .as_deref()
            .filter(|p| *p != "/dev/null")
            .or(del.effective_path());
        let add_path = add
            .new_path
            .as_deref()
            .filter(|p| *p != "/dev/null")
            .or(add.effective_path());
        let same_path = del_path.zip(add_path).is_some_and(|(a, b)| a == b);
        let del_blob = del.is_deleted && del.old_mode.as_deref() != Some("160000");
        let add_gitlink = add.is_new && add.new_mode.as_deref() == Some("160000");
        if same_path && del_blob && add_gitlink {
            let mut merged = del.clone();
            merged.new_path = add.new_path.clone();
            merged.saw_new_header = add.saw_new_header;
            merged.new_mode = Some("160000".to_string());
            merged.is_deleted = false;
            merged.is_new = false;
            merged.new_oid = add.new_oid.clone();
            merged.hunks.extend(add.hunks.clone());
            patches[i] = merged;
            patches.remove(i + 1);
            continue;
        }
        i += 1;
    }
}

fn subproject_commit_from_hunks(fp: &FilePatch) -> String {
    fp.hunks
        .iter()
        .flat_map(|h| h.lines.iter())
        .find_map(|l| {
            if let HunkLine::Add(s) = l {
                s.strip_prefix("Subproject commit ")
                    .map(|h| h.trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "0000000000000000000000000000000000000000".to_string())
}

/// Parse a `GIT binary patch` payload.
fn parse_binary_patch(lines: &[&str], mut i: usize) -> Result<(BinaryPatchPayload, usize)> {
    let (forward_compressed, forward_declared_size) = parse_binary_literal(lines, &mut i)?;
    let (reverse_compressed, reverse_declared_size) =
        if i < lines.len() && lines[i].starts_with("literal ") {
            parse_binary_literal(lines, &mut i)?
        } else {
            (Vec::new(), 0)
        };

    Ok((
        BinaryPatchPayload {
            forward_compressed,
            forward_declared_size,
            reverse_compressed,
            reverse_declared_size,
        },
        i,
    ))
}

/// Parse one `literal <size>` block from a binary patch.
fn parse_binary_literal(lines: &[&str], i: &mut usize) -> Result<(Vec<u8>, usize)> {
    let header = lines.get(*i).copied().unwrap_or_default();
    let Some(size_str) = header.strip_prefix("literal ") else {
        bail!("unsupported binary patch section: '{header}'");
    };
    let declared_size: usize = size_str
        .trim()
        .parse()
        .context("invalid binary patch literal size")?;
    *i += 1;

    let mut compressed = Vec::new();
    while *i < lines.len() {
        let line = lines[*i];
        if line.is_empty() {
            *i += 1;
            break;
        }
        decode_binary_patch_line(line, &mut compressed)?;
        *i += 1;
    }

    Ok((compressed, declared_size))
}

/// Decode one binary patch payload line into compressed bytes.
fn decode_binary_patch_line(line: &str, out: &mut Vec<u8>) -> Result<()> {
    let mut chars = line.chars();
    let Some(len_ch) = chars.next() else {
        bail!("empty binary patch payload line");
    };
    let expected_len = decode_binary_line_len(len_ch)?;
    let body = chars.as_str().as_bytes();
    let decoded = crate::git_binary_base85::decode_body(body, expected_len)
        .context("invalid binary patch base85")?;
    out.extend_from_slice(&decoded);
    Ok(())
}

fn decode_binary_line_len(ch: char) -> Result<usize> {
    if ch.is_ascii_uppercase() {
        return Ok((ch as u8 - b'A' + 1) as usize);
    }
    if ch.is_ascii_lowercase() {
        return Ok((ch as u8 - b'a' + 27) as usize);
    }
    bail!("invalid binary patch line length marker: '{ch}'")
}

/// Inflate zlib-compressed binary payload.
fn inflate_binary_payload(compressed: &[u8]) -> Result<Vec<u8>> {
    use flate2::read::ZlibDecoder;
    use std::io::Read;

    let mut decoder = ZlibDecoder::new(compressed);
    let mut out = Vec::new();
    decoder
        .read_to_end(&mut out)
        .context("failed to inflate binary patch payload")?;
    Ok(out)
}

/// Split the two path tokens from the remainder of a `diff --git` line (after `diff --git `).
fn split_diff_git_paths(s: &str) -> Option<(String, String)> {
    let s = s.trim_end_matches(['\r', '\n']);

    if s.as_bytes().first() == Some(&b'"') {
        let (first, rest_raw) = unquote_c_style_diff_prefix(s)?;
        let rest = rest_raw.trim_start_matches(|c: char| c.is_ascii_whitespace());
        if rest.is_empty() {
            return None;
        }
        if rest.as_bytes().first() == Some(&b'"') {
            let (second, _) = unquote_c_style_diff_prefix(rest)?;
            return Some((
                String::from_utf8_lossy(&first).into_owned(),
                String::from_utf8_lossy(&second).into_owned(),
            ));
        }
        let second = rest;
        if second.len() != first.len() || second.as_bytes() != first.as_slice() {
            return None;
        }
        return Some((
            String::from_utf8_lossy(&first).into_owned(),
            second.to_string(),
        ));
    }

    if let Some(pos) = s.find(" b/") {
        let a = &s[..pos];
        let b = &s[pos + 1..];
        return Some((a.to_string(), b.to_string()));
    }
    if s.starts_with("a/") {
        if let Some(pos) = s.find(" /dev/null") {
            let a = &s[..pos];
            return Some((a.to_string(), "/dev/null".to_string()));
        }
    }
    if let Some(b) = s.strip_prefix("/dev/null ") {
        return Some(("/dev/null".to_string(), b.to_string()));
    }

    let name = s.as_bytes();
    let line_len = name.len();
    let mut len = 0usize;
    while len < line_len {
        match name[len] {
            b'\n' => return None,
            b'\t' | b' ' => {
                if len + 1 > line_len {
                    return None;
                }
                let second = &name[len + 1..line_len];
                let names_match =
                    name.len() >= len && second.len() >= len && name[..len] == second[..len];
                let boundary_ok = second.get(len) == Some(&b'\n') || second.len() == len;
                if names_match && boundary_ok {
                    return Some((
                        String::from_utf8_lossy(&name[..len]).into_owned(),
                        String::from_utf8_lossy(second).into_owned(),
                    ));
                }
            }
            _ => {}
        }
        len += 1;
    }
    None
}

/// Parse a single hunk starting at line `i` (which should be an `@@` line).
fn parse_hunk(lines: &[&str], start: usize) -> Result<(Hunk, usize)> {
    let header = lines[start];
    let (old_start, old_count, new_start, new_count) =
        parse_hunk_header(header).with_context(|| format!("invalid hunk header: {header}"))?;

    let mut hunk = Hunk {
        old_start,
        old_count,
        new_start,
        new_count,
        first_body_line: start + 2,
        lines: Vec::new(),
    };

    let mut i = start + 1;
    while i < lines.len() {
        let line = lines[i];
        if line.starts_with("@@ ") || line.starts_with("diff --git ") {
            break;
        }
        // `---` / `+++` with a space begin a new file header; do not treat `---` as a `-` hunk line
        // (would misparse `--- /dev/null` as a remove of `-- /dev/null` and merge the next file).
        if line.starts_with("--- ") || line.starts_with("+++ ") {
            break;
        }
        if line == "-- " {
            // format-patch signature separator; not part of hunk body
            break;
        }
        if let Some(rest) = line.strip_prefix('+') {
            hunk.lines.push(HunkLine::Add(rest.to_string()));
        } else if let Some(rest) = line.strip_prefix('-') {
            hunk.lines.push(HunkLine::Remove(rest.to_string()));
        } else if line.is_empty() {
            hunk.lines.push(HunkLine::Context(String::new()));
        } else if let Some(rest) = line.strip_prefix(' ') {
            // context line
            hunk.lines.push(HunkLine::Context(rest.to_string()));
        } else if line.starts_with('\\') {
            hunk.lines.push(HunkLine::NoNewline);
        } else {
            // Unknown line type — could be start of something else
            break;
        }
        i += 1;
    }

    Ok((hunk, i))
}

/// Parse "@@ -old_start[,old_count] +new_start[,new_count] @@..."
fn parse_hunk_header(line: &str) -> Result<(usize, usize, usize, usize)> {
    // Find the range part between @@ markers
    let trimmed = line.trim_start_matches('@').trim_start();
    let end = trimmed.find(" @@").unwrap_or(trimmed.len());
    let range_part = &trimmed[..end];

    let parts: Vec<&str> = range_part.split_whitespace().collect();
    if parts.len() < 2 {
        bail!("expected old and new range in hunk header");
    }

    let (old_start, old_count) = parse_range(parts[0].trim_start_matches('-'))?;
    let (new_start, new_count) = parse_range(parts[1].trim_start_matches('+'))?;

    Ok((old_start, old_count, new_start, new_count))
}

/// Parse "N" or "N,M" into (start, count).
fn parse_range(s: &str) -> Result<(usize, usize)> {
    if let Some((start_s, count_s)) = s.split_once(',') {
        Ok((start_s.parse()?, count_s.parse()?))
    } else {
        let n: usize = s.parse()?;
        Ok((n, 1))
    }
}

// ---------------------------------------------------------------------------
// Strip / directory adjustment
// ---------------------------------------------------------------------------

/// Strip `n` leading path components.
fn strip_components(path: &str, n: usize) -> String {
    if n == 0 {
        return path.to_string();
    }
    let mut remaining = path;
    for _ in 0..n {
        if let Some(pos) = remaining.find('/') {
            remaining = &remaining[pos + 1..];
        } else {
            return remaining.to_string();
        }
    }
    remaining.to_string()
}

/// Strip `-p` prefix from a path (matches parsing-time strip, for validating headers).
fn path_after_strip(path: &str, strip: usize) -> String {
    if path == "/dev/null" {
        return path.to_string();
    }
    strip_components(path, strip)
}

/// Create compact rename path: "dir/{old => new}" or "old => new".
fn compact_rename_path(old: &str, new: &str) -> String {
    // Find common prefix
    let old_parts: Vec<&str> = old.split('/').collect();
    let new_parts: Vec<&str> = new.split('/').collect();
    let mut prefix_len = 0;
    for (a, b) in old_parts.iter().zip(new_parts.iter()) {
        if a == b {
            prefix_len += 1;
        } else {
            break;
        }
    }
    // Find common suffix
    let mut suffix_len = 0;
    let old_rev: Vec<&str> = old_parts.iter().rev().cloned().collect();
    let new_rev: Vec<&str> = new_parts.iter().rev().cloned().collect();
    for (a, b) in old_rev.iter().zip(new_rev.iter()) {
        if a == b && prefix_len + suffix_len < old_parts.len().min(new_parts.len()) {
            suffix_len += 1;
        } else {
            break;
        }
    }

    let prefix: String = old_parts[..prefix_len].join("/");
    let suffix: String = old_parts[old_parts.len() - suffix_len..].join("/");
    let old_mid: String = old_parts[prefix_len..old_parts.len() - suffix_len].join("/");
    let new_mid: String = new_parts[prefix_len..new_parts.len() - suffix_len].join("/");

    // If no common prefix or suffix, just use "old => new" without braces
    if prefix.is_empty() && suffix.is_empty() {
        return format!("{old_mid} => {new_mid}");
    }

    let mut result = String::new();
    if !prefix.is_empty() {
        result.push_str(&prefix);
        result.push('/');
    }
    result.push('{');
    result.push_str(&old_mid);
    result.push_str(" => ");
    result.push_str(&new_mid);
    result.push('}');
    if !suffix.is_empty() {
        result.push('/');
        result.push_str(&suffix);
    }
    result
}

/// Normalize `--directory` like Git (`strbuf_normalize_path` + trailing `/` when non-empty).
fn normalize_apply_directory(raw: &str) -> Result<String> {
    let normalized = crate::git_path::normalize_path_copy(raw)
        .map_err(|_| anyhow::anyhow!("unable to normalize directory: '{raw}'"))?;
    if normalized.is_empty() {
        return Ok(String::new());
    }
    Ok(if normalized.ends_with('/') {
        normalized
    } else {
        format!("{normalized}/")
    })
}

/// Prepend normalized `--directory` to a path already adjusted for `-p` during parsing.
fn adjust_path(path: &str, directory: Option<&str>) -> String {
    if path == "/dev/null" {
        return path.to_string();
    }
    let Some(dir) = directory.filter(|d| !d.is_empty()) else {
        return path.to_string();
    };
    format!("{dir}{path}")
}

fn symlink_prefix(path: &str, symlink_overlay: &HashMap<String, bool>) -> Option<String> {
    let components: Vec<&str> = path.split('/').filter(|c| !c.is_empty()).collect();
    if components.len() <= 1 {
        return None;
    }

    let mut prefix = PathBuf::new();
    for component in &components[..components.len() - 1] {
        prefix.push(component);
        let prefix_str = prefix.to_string_lossy().into_owned();
        let is_symlink = symlink_overlay
            .get(&prefix_str)
            .copied()
            .unwrap_or_else(|| {
                fs::symlink_metadata(&prefix)
                    .map(|meta| meta.file_type().is_symlink())
                    .unwrap_or(false)
            });
        if is_symlink {
            return Some(prefix_str);
        }
    }
    None
}

fn verify_patch_paths_not_beyond_symlink(patches: &[FilePatch], args: &Args) -> Result<()> {
    let mut symlink_overlay: HashMap<String, bool> = HashMap::new();
    let mut replaced_directories_with_symlink: HashMap<String, bool> = HashMap::new();

    for fp in patches {
        if let Some(source) = fp.source_path() {
            let source_adjusted = adjust_path(source, args.directory.as_deref());
            if !source_adjusted.is_empty() {
                if let Some(prefix) = symlink_prefix(&source_adjusted, &symlink_overlay) {
                    let allow_delete_under_replaced_dir = fp.is_deleted
                        && source_adjusted.starts_with(&format!("{prefix}/"))
                        && replaced_directories_with_symlink
                            .get(&prefix)
                            .copied()
                            .unwrap_or(false);
                    if !allow_delete_under_replaced_dir {
                        bail!("{source_adjusted}: beyond a symbolic link");
                    }
                }
            }
        }
        if let Some(target) = fp.target_path() {
            let target_adjusted = adjust_path(target, args.directory.as_deref());
            if !target_adjusted.is_empty() {
                if let Some(prefix) = symlink_prefix(&target_adjusted, &symlink_overlay) {
                    let allow_delete_under_replaced_dir = fp.is_deleted
                        && target_adjusted.starts_with(&format!("{prefix}/"))
                        && replaced_directories_with_symlink
                            .get(&prefix)
                            .copied()
                            .unwrap_or(false);
                    if !allow_delete_under_replaced_dir {
                        bail!("{target_adjusted}: beyond a symbolic link");
                    }
                }
            }
        }

        let source_adjusted = fp
            .source_path()
            .map(|p| adjust_path(p, args.directory.as_deref()))
            .unwrap_or_default();
        let target_adjusted = fp
            .target_path()
            .map(|p| adjust_path(p, args.directory.as_deref()))
            .unwrap_or_default();
        let target_is_existing_dir =
            !target_adjusted.is_empty() && Path::new(&target_adjusted).is_dir();

        if fp.is_deleted {
            if !source_adjusted.is_empty() {
                symlink_overlay.insert(source_adjusted, false);
            }
            continue;
        }

        if fp.is_rename && source_adjusted != target_adjusted && !source_adjusted.is_empty() {
            symlink_overlay.insert(source_adjusted, false);
        }

        if !target_adjusted.is_empty() {
            if fp.new_mode.as_deref() == Some("120000") {
                if target_is_existing_dir {
                    replaced_directories_with_symlink.insert(target_adjusted.clone(), true);
                }
                symlink_overlay.insert(target_adjusted, true);
            } else if fp.new_mode.is_some() || fp.is_new {
                symlink_overlay.insert(target_adjusted, false);
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Reverse
// ---------------------------------------------------------------------------

/// Reverse a patch: swap old/new paths, swap +/- in hunks.
fn reverse_patches(patches: &mut [FilePatch]) {
    for fp in patches.iter_mut() {
        std::mem::swap(&mut fp.old_path, &mut fp.new_path);
        std::mem::swap(&mut fp.old_mode, &mut fp.new_mode);
        std::mem::swap(&mut fp.old_oid, &mut fp.new_oid);
        std::mem::swap(&mut fp.is_new, &mut fp.is_deleted);
        if let Some(binary) = fp.binary_patch.as_mut() {
            std::mem::swap(
                &mut binary.forward_compressed,
                &mut binary.reverse_compressed,
            );
            std::mem::swap(
                &mut binary.forward_declared_size,
                &mut binary.reverse_declared_size,
            );
        }

        for hunk in &mut fp.hunks {
            std::mem::swap(&mut hunk.old_start, &mut hunk.new_start);
            std::mem::swap(&mut hunk.old_count, &mut hunk.new_count);
            let new_lines: Vec<HunkLine> = hunk
                .lines
                .drain(..)
                .map(|hl| match hl {
                    HunkLine::Add(s) => HunkLine::Remove(s),
                    HunkLine::Remove(s) => HunkLine::Add(s),
                    other => other,
                })
                .collect();
            hunk.lines = new_lines;
        }
    }

    // Apply reversed patchsets in reverse file order so that a later patch in
    // the forward direction is undone first. This matches Git's reverse-apply
    // behavior for multi-part patches touching the same path.
    patches.reverse();
}

/// When `diff --git` lists two different paths but `---` / `+++` agree on the
/// real file (same path after stripping `a/` / `b/`), treat the patch as a
/// normal same-file diff.
///
/// This matches Git's tolerance for a common mistake: `sed` with a single
/// substitution per line only rewrites the first `a/file` or `b/file` on the
/// `diff --git` line, leaving `a/target b/file` while both traditional headers
/// say `target` (see `t4124-apply-ws-rule.sh`).
fn assign_ws_rules(patches: &mut [FilePatch], args: &Args) {
    let cfg_rule = config_whitespace_rule_bits();
    let Ok(repo) = Repository::discover(None) else {
        for fp in patches {
            fp.ws_rule = cfg_rule;
        }
        return;
    };
    let config = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_else(|_| ConfigSet::new());
    let rules = repo
        .work_tree
        .as_ref()
        .map(|wt| crlf::load_gitattributes(wt))
        .unwrap_or_default();

    for fp in patches.iter_mut() {
        let path_for_attr = fp
            .target_path()
            .or_else(|| fp.effective_path())
            .unwrap_or("");
        let adjusted = adjust_path(path_for_attr, args.directory.as_deref());
        let fa = crlf::get_file_attrs(&rules, &adjusted, false, &config);
        let attr = match fa.whitespace.as_deref() {
            None => WhitespaceGitAttr::Unspecified,
            Some("unset") => WhitespaceGitAttr::False,
            Some("set") => WhitespaceGitAttr::True,
            Some(s) => WhitespaceGitAttr::String(s.to_owned()),
        };
        fp.ws_rule = attr.merge_with_config(cfg_rule).unwrap_or(cfg_rule);

        let mode = fp.new_mode.as_deref().or(fp.old_mode.as_deref());
        if mode == Some("120000") {
            fp.ws_rule &= !WS_INCOMPLETE_LINE;
        }
    }
}

fn normalize_mismatched_diff_git_paths(patches: &mut [FilePatch], strip: usize) {
    fn strip_leading_ab(p: &str) -> &str {
        p.strip_prefix("a/")
            .or_else(|| p.strip_prefix("b/"))
            .unwrap_or(p)
    }

    for fp in patches.iter_mut() {
        let (Some(d_old_raw), Some(d_new_raw)) =
            (fp.diff_old_path.as_deref(), fp.diff_new_path.as_deref())
        else {
            continue;
        };
        let d_old = path_after_strip(d_old_raw, strip);
        let d_new = path_after_strip(d_new_raw, strip);
        if d_old == d_new {
            continue;
        }
        let (Some(o), Some(n)) = (fp.old_path.as_deref(), fp.new_path.as_deref()) else {
            continue;
        };
        if o == "/dev/null" || n == "/dev/null" {
            continue;
        }
        let ho = strip_leading_ab(o);
        let hn = strip_leading_ab(n);
        if ho == hn {
            fp.diff_old_path = Some(format!("a/{ho}"));
            fp.diff_new_path = Some(format!("b/{hn}"));
        }
    }
}

fn validate_patch_headers(patches: &[FilePatch], strip: usize) -> Result<()> {
    for fp in patches {
        if (fp.diff_old_path.is_some() || fp.diff_new_path.is_some())
            && !fp.hunks.is_empty()
            && (!fp.saw_old_header || !fp.saw_new_header)
        {
            bail!("patch lacks filename information");
        }

        if fp.saw_old_header {
            if let (Some(diff_old), Some(old)) =
                (fp.diff_old_path.as_deref(), fp.old_path.as_deref())
            {
                if old != "/dev/null" && path_after_strip(diff_old, strip) != old {
                    bail!("inconsistent old filename");
                }
            }
        }

        if fp.saw_new_header {
            if let (Some(diff_new), Some(new)) =
                (fp.diff_new_path.as_deref(), fp.new_path.as_deref())
            {
                if new != "/dev/null" && path_after_strip(diff_new, strip) != new {
                    bail!("inconsistent new filename");
                }
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Applying hunks to content
// ---------------------------------------------------------------------------

/// Apply hunks to file content (a list of lines). Returns new content.
///
/// Find the best starting position for a hunk by scanning around the nominal
/// position. When `allow_unidiff_zero_fallback` is true, a `--unidiff-zero`
/// style fallback is used where all old-side lines (context/remove) may match
/// as an ordered subsequence (not necessarily contiguous) at a candidate
/// location.
fn find_hunk_start(
    old_lines: &[&str],
    hunk: &Hunk,
    nominal: usize,
    ws_mode: ApplyWhitespaceMode,
    allow_unidiff_zero_fallback: bool,
    fp: &FilePatch,
) -> usize {
    let adjusted_nominal = if hunk.old_count > 0 {
        let required_context = ws_mode
            .context
            .unwrap_or(hunk.old_count)
            .min(hunk.old_count);
        nominal.saturating_sub(hunk.old_count - required_context)
    } else {
        nominal
    };

    if hunk.old_count == 0 {
        return adjusted_nominal.min(old_lines.len());
    }

    if match_hunk_old_side_at(
        old_lines,
        hunk,
        adjusted_nominal,
        ws_mode,
        allow_unidiff_zero_fallback,
        fp,
    ) {
        return adjusted_nominal;
    }

    let max_scan = old_lines.len();
    for delta in 1..=max_scan {
        if adjusted_nominal >= delta
            && match_hunk_old_side_at(
                old_lines,
                hunk,
                adjusted_nominal - delta,
                ws_mode,
                allow_unidiff_zero_fallback,
                fp,
            )
        {
            return adjusted_nominal - delta;
        }
        if adjusted_nominal + delta <= old_lines.len()
            && match_hunk_old_side_at(
                old_lines,
                hunk,
                adjusted_nominal + delta,
                ws_mode,
                allow_unidiff_zero_fallback,
                fp,
            )
        {
            return adjusted_nominal + delta;
        }
    }

    adjusted_nominal.min(old_lines.len())
}

fn hunk_matches_at_position(
    old_lines: &[&str],
    hunk: &Hunk,
    start: usize,
    match_beginning: bool,
    match_end: bool,
    ws_mode: ApplyWhitespaceMode,
    fp: &FilePatch,
) -> bool {
    let old_n = old_side_line_count(hunk);
    if old_n == 0 {
        return true;
    }
    if start + old_n > old_lines.len() {
        return false;
    }
    if match_beginning && start != 0 {
        return false;
    }
    if match_end && start + old_n != old_lines.len() {
        return false;
    }
    hunk_old_side_matches_at(old_lines, hunk, start, ws_mode, fp)
}

/// Git `find_pos`: alternate scan from nominal line until a match or exhaustion.
fn find_pos_scan(
    old_lines: &[&str],
    hunk: &Hunk,
    ws_mode: ApplyWhitespaceMode,
    fp: &FilePatch,
    match_beginning: bool,
    match_end: bool,
    start_line: usize,
) -> Option<usize> {
    let old_n = old_side_line_count(hunk);
    let line = if match_beginning {
        0usize
    } else if match_end {
        old_lines.len().saturating_sub(old_n)
    } else {
        start_line.min(old_lines.len())
    };
    if line > old_lines.len() {
        return None;
    }

    let mut backwards_lno = line;
    let mut forwards_lno = line;
    let mut current_lno = line;
    let mut i = 0usize;
    loop {
        if hunk_matches_at_position(
            old_lines,
            hunk,
            current_lno,
            match_beginning,
            match_end,
            ws_mode,
            fp,
        ) {
            return Some(current_lno);
        }
        if backwards_lno == 0 && forwards_lno >= old_lines.len() {
            break;
        }
        if i & 1 == 1 {
            if backwards_lno == 0 {
                i += 1;
                continue;
            }
            backwards_lno -= 1;
            current_lno = backwards_lno;
        } else {
            if forwards_lno >= old_lines.len() {
                i += 1;
                continue;
            }
            forwards_lno += 1;
            current_lno = forwards_lno;
        }
        i += 1;
    }
    None
}

fn nominal_hunk_start(hunk: &Hunk, old_len: usize) -> Result<usize> {
    if hunk.old_count == 0 {
        let start = if hunk.old_start == 0 {
            0
        } else {
            hunk.old_start
        };
        if start > old_len {
            bail!("patch does not apply");
        }
        return Ok(start);
    }

    if hunk.old_start == 0 {
        bail!("patch does not apply");
    }
    let start = hunk.old_start - 1;
    Ok(start.min(old_len))
}

fn old_side_line_count(hunk: &Hunk) -> usize {
    hunk.lines
        .iter()
        .filter(|hl| matches!(hl, HunkLine::Context(_) | HunkLine::Remove(_)))
        .count()
}

/// Check whether hunk old-side lines match at the given candidate start.
fn match_hunk_old_side_at(
    old_lines: &[&str],
    hunk: &Hunk,
    start: usize,
    ws_mode: ApplyWhitespaceMode,
    allow_unidiff_zero_fallback: bool,
    fp: &FilePatch,
) -> bool {
    let old_side: Vec<&str> = hunk
        .lines
        .iter()
        .filter_map(|hl| match hl {
            HunkLine::Context(s) | HunkLine::Remove(s) => Some(s.as_str()),
            _ => None,
        })
        .collect();

    if old_side.is_empty() {
        return start <= old_lines.len();
    }

    if let Some(required) = ws_mode.context {
        let required = required.min(old_side.len());
        let max_leading_fuzz = old_side.len().saturating_sub(required);
        for leading_fuzz in 0..=max_leading_fuzz {
            let candidate_start = start + leading_fuzz;
            if let Some(sub) = hunk_slice_from_old_side_skip(hunk, leading_fuzz) {
                let slice_hunk = sub.as_hunk_ref();
                if candidate_start + sub.old_n <= old_lines.len()
                    && hunk_old_side_matches_at(
                        old_lines,
                        &slice_hunk,
                        candidate_start,
                        ws_mode,
                        fp,
                    )
                {
                    return true;
                }
            }
        }
    }

    if start + old_side.len() <= old_lines.len()
        && hunk_old_side_matches_at(old_lines, hunk, start, ws_mode, fp)
    {
        return true;
    }

    allow_unidiff_zero_fallback
        && is_subsequence_match_ws(
            &old_lines[start.min(old_lines.len())..],
            &old_side,
            ws_mode,
            fp,
        )
}

/// Hunk view that is `original` with the first `skip` old-side lines removed from the body.
#[derive(Debug, Clone)]
struct HunkSliceView {
    lines: Vec<HunkLine>,
    old_start: usize,
    new_start: usize,
    old_count: usize,
    new_count: usize,
    first_body_line: usize,
    old_n: usize,
}

fn hunk_slice_from_old_side_skip(hunk: &Hunk, skip: usize) -> Option<HunkSliceView> {
    if skip == 0 {
        return Some(HunkSliceView {
            lines: hunk.lines.clone(),
            old_start: hunk.old_start,
            new_start: hunk.new_start,
            old_count: hunk.old_count,
            new_count: hunk.new_count,
            first_body_line: hunk.first_body_line,
            old_n: old_side_line_count(hunk),
        });
    }
    let mut li = 0usize;
    let mut skipped = 0usize;
    let mut old_skipped = 0usize;
    let mut new_skipped = 0usize;
    while li < hunk.lines.len() && skipped < skip {
        match &hunk.lines[li] {
            HunkLine::Context(_) => {
                let has_nl = !matches!(hunk.lines.get(li + 1), Some(HunkLine::NoNewline));
                old_skipped += 1;
                new_skipped += 1;
                li += 1;
                if !has_nl {
                    li += 1;
                }
                skipped += 1;
            }
            HunkLine::Remove(_) => {
                let has_nl = !matches!(hunk.lines.get(li + 1), Some(HunkLine::NoNewline));
                old_skipped += 1;
                li += 1;
                if !has_nl {
                    li += 1;
                }
                skipped += 1;
            }
            HunkLine::Add(_) => {
                return None;
            }
            HunkLine::NoNewline => {
                li += 1;
            }
        }
    }
    if skipped != skip || li > hunk.lines.len() {
        return None;
    }
    let rest = hunk.lines[li..].to_vec();
    if rest.is_empty() {
        return None;
    }
    let old_n = old_side_line_count(&Hunk {
        old_start: 0,
        old_count: 0,
        new_start: 0,
        new_count: 0,
        first_body_line: 0,
        lines: rest.clone(),
    });
    Some(HunkSliceView {
        lines: rest,
        old_start: hunk.old_start.saturating_add(old_skipped),
        new_start: hunk.new_start.saturating_add(new_skipped),
        old_count: hunk.old_count.saturating_sub(old_skipped),
        new_count: hunk.new_count.saturating_sub(new_skipped),
        first_body_line: hunk.first_body_line,
        old_n,
    })
}

impl HunkSliceView {
    fn as_hunk_ref(&self) -> Hunk {
        Hunk {
            old_start: self.old_start,
            old_count: self.old_count,
            new_start: self.new_start,
            new_count: self.new_count,
            first_body_line: self.first_body_line,
            lines: self.lines.clone(),
        }
    }
}

/// Git `match_fragment` whitespace-fix check: `ws_fix_copy(preimage)==ws_fix_copy(target)` per line.
fn ws_fix_lines_match_patch_to_actual(
    patch_line: &str,
    actual_line: &str,
    has_nl: bool,
    ws_rule: u32,
) -> bool {
    let pre_body = patch_line_body_for_ws(patch_line, has_nl);
    let tgt_body = patch_line_body_for_ws(actual_line, has_nl);
    let (pre_fixed, _) = ws::ws_fix_copy_line(&pre_body, ws_rule);
    let (tgt_fixed, _) = ws::ws_fix_copy_line(&tgt_body, ws_rule);
    pre_fixed == tgt_fixed
}

fn patch_line_matches_actual(
    patch_line: &str,
    actual_line: &str,
    has_nl: bool,
    ws_mode: ApplyWhitespaceMode,
    fp: &FilePatch,
) -> bool {
    if ws_mode.whitespace_fix {
        ws_fix_lines_match_patch_to_actual(patch_line, actual_line, has_nl, fp.ws_rule)
    } else {
        lines_equal(patch_line, actual_line, ws_mode)
    }
}

fn hunk_old_side_matches_at(
    old_lines: &[&str],
    hunk: &Hunk,
    start: usize,
    ws_mode: ApplyWhitespaceMode,
    fp: &FilePatch,
) -> bool {
    hunk_old_side_matches_at_impl(old_lines, &hunk.lines, start, ws_mode, fp)
}

fn hunk_old_side_matches_at_impl(
    old_lines: &[&str],
    hunk_lines: &[HunkLine],
    start: usize,
    ws_mode: ApplyWhitespaceMode,
    fp: &FilePatch,
) -> bool {
    let mut old_idx = start;
    let mut li = 0usize;
    while li < hunk_lines.len() {
        match &hunk_lines[li] {
            HunkLine::Context(s) => {
                let has_nl = !matches!(hunk_lines.get(li + 1), Some(HunkLine::NoNewline));
                if old_idx >= old_lines.len() {
                    return false;
                }
                let ok = patch_line_matches_actual(s, old_lines[old_idx], has_nl, ws_mode, fp);
                if !ok {
                    return false;
                }
                old_idx += 1;
                li += 1;
                if !has_nl {
                    li += 1;
                }
            }
            HunkLine::Remove(s) => {
                let has_nl = !matches!(hunk_lines.get(li + 1), Some(HunkLine::NoNewline));
                if old_idx >= old_lines.len() {
                    return false;
                }
                let ok = patch_line_matches_actual(s, old_lines[old_idx], has_nl, ws_mode, fp);
                if !ok {
                    return false;
                }
                old_idx += 1;
                li += 1;
                if !has_nl {
                    li += 1;
                }
            }
            HunkLine::Add(_) => {
                let has_nl = !matches!(hunk_lines.get(li + 1), Some(HunkLine::NoNewline));
                li += 1;
                if !has_nl {
                    li += 1;
                }
            }
            HunkLine::NoNewline => {
                li += 1;
            }
        }
    }
    true
}

fn is_subsequence_match_ws(
    haystack: &[&str],
    needle: &[&str],
    ws_mode: ApplyWhitespaceMode,
    fp: &FilePatch,
) -> bool {
    if needle.is_empty() {
        return true;
    }

    let mut pos = 0usize;
    for expected in needle {
        while pos < haystack.len()
            && !line_matches_patch_to_actual(expected, haystack[pos], ws_mode, fp)
        {
            pos += 1;
        }
        if pos == haystack.len() {
            return false;
        }
        pos += 1;
    }

    true
}

fn line_matches_patch_to_actual(
    patch_line: &str,
    actual_line: &str,
    ws_mode: ApplyWhitespaceMode,
    fp: &FilePatch,
) -> bool {
    patch_line_matches_actual(patch_line, actual_line, true, ws_mode, fp)
}

/// Returns true if `needle` appears in `haystack` in order (not necessarily
/// contiguously), using whitespace-aware line comparison.
fn is_subsequence_match(haystack: &[&str], needle: &[&str], ws_mode: ApplyWhitespaceMode) -> bool {
    if needle.is_empty() {
        return true;
    }

    let mut pos = 0usize;
    for expected in needle {
        while pos < haystack.len() && !lines_equal(expected, haystack[pos], ws_mode) {
            pos += 1;
        }
        if pos == haystack.len() {
            return false;
        }
        pos += 1;
    }

    true
}

fn canonicalize_ws_line(line: &str) -> String {
    let mut end = line.len();
    while end > 0 {
        let b = line.as_bytes()[end - 1];
        if b == b' ' || b == b'\t' {
            end -= 1;
        } else {
            break;
        }
    }
    line[..end].to_string()
}

fn canonicalize_space_change_line(line: &str) -> String {
    let mut normalized = String::with_capacity(line.len());
    let mut in_space = false;
    for c in line.chars() {
        if c.is_whitespace() {
            if !in_space {
                normalized.push(' ');
                in_space = true;
            }
        } else {
            normalized.push(c);
            in_space = false;
        }
    }
    normalized.trim_end().to_owned()
}

fn expand_tabs_for_compare(line: &str, tab_width: usize) -> String {
    let mut out = String::with_capacity(line.len());
    let mut col = 0usize;
    for ch in line.chars() {
        match ch {
            '\t' => {
                let stop = tab_width.saturating_sub(col % tab_width).max(1);
                out.push_str(&" ".repeat(stop));
                col += stop;
            }
            _ => {
                out.push(ch);
                col += 1;
            }
        }
    }
    out
}

fn normalize_ws_fix_line(line: &str, tab_width: usize) -> String {
    let mut out = String::with_capacity(line.len());
    let mut in_indent = true;
    let mut col = 0usize;

    for ch in line.chars() {
        if in_indent {
            match ch {
                ' ' => {
                    out.push(' ');
                    col += 1;
                    continue;
                }
                '\t' => {
                    let stop = tab_width.saturating_sub(col % tab_width).max(1);
                    out.push_str(&" ".repeat(stop));
                    col += stop;
                    continue;
                }
                _ => in_indent = false,
            }
        }
        out.push(ch);
        col += 1;
    }

    canonicalize_ws_line(&out)
}

fn patch_line_body_for_ws(s: &str, has_newline: bool) -> String {
    if has_newline {
        format!("{s}\n")
    } else {
        s.to_string()
    }
}

fn record_ws_error(
    ws_mode: &ApplyWhitespaceMode,
    ws_errors: &mut u32,
    flags: u32,
    patch_file: &str,
    patch_linenr: usize,
    incomplete_line_body: Option<&str>,
) {
    if flags == 0 {
        return;
    }
    *ws_errors += 1;
    if ws_mode.ws_squelch > 0 && ws_mode.ws_squelch < *ws_errors {
        return;
    }
    if flags & WS_INCOMPLETE_LINE != 0 {
        if let Some(body) = incomplete_line_body {
            eprintln!("{patch_file}:{patch_linenr}: no newline at the end of file.\n{body}");
        }
    } else if flags & WS_BLANK_AT_EOF != 0 {
        eprintln!("{patch_file}:{patch_linenr}: new blank line at EOF.");
    } else {
        eprintln!(
            "{patch_file}:{patch_linenr}: {}.",
            ws::whitespace_error_string(flags)
        );
    }
}

/// Count leading and trailing context lines in a hunk body (Git `fragment->leading` / `trailing`).
fn hunk_leading_trailing_context(hunk: &Hunk) -> (usize, usize) {
    let mut leading = 0usize;
    let mut trailing = 0usize;
    let mut saw_change = false;
    let mut li = 0usize;
    while li < hunk.lines.len() {
        match &hunk.lines[li] {
            HunkLine::Context(_) => {
                if !saw_change {
                    leading += 1;
                }
                trailing += 1;
                li += 1;
                if matches!(hunk.lines.get(li), Some(HunkLine::NoNewline)) {
                    li += 1;
                }
            }
            HunkLine::Remove(_) | HunkLine::Add(_) => {
                saw_change = true;
                trailing = 0;
                li += 1;
                if matches!(hunk.lines.get(li), Some(HunkLine::NoNewline)) {
                    li += 1;
                }
            }
            HunkLine::NoNewline => {
                li += 1;
            }
        }
    }
    (leading, trailing)
}

/// Remove one leading context line (and optional `\ No newline`) from a hunk; adjust header counts.
fn pop_leading_context_line(hunk: &mut Hunk) -> bool {
    if !matches!(hunk.lines.first(), Some(HunkLine::Context(_))) {
        return false;
    }
    let mut end = 1usize;
    if matches!(hunk.lines.get(1), Some(HunkLine::NoNewline)) {
        end = 2;
    }
    hunk.lines.drain(0..end);
    hunk.old_count = hunk.old_count.saturating_sub(1);
    hunk.new_count = hunk.new_count.saturating_sub(1);
    hunk.old_start = hunk.old_start.saturating_add(1);
    hunk.new_start = hunk.new_start.saturating_add(1);
    true
}

/// Remove one trailing context line (and optional `\ No newline`) from a hunk; adjust header counts.
/// After a position is found, drop trailing context from the hunk while it still matches there.
/// This mirrors Git shrinking the preimage/postimage once a match is known: lines removed from
/// the hunk are not re-emitted (they stay as raw worktree bytes), which preserves trailing
/// whitespace beyond the "context horizon" (t4125).
fn minimize_trailing_context_at(
    hunk: &mut Hunk,
    old_lines: &[&str],
    start: usize,
    ws_mode: ApplyWhitespaceMode,
    fp: &FilePatch,
) {
    // Git may drop at most one trailing context line past the "horizon" while still matching
    // (t4125: only the final `h` line stays unfixed). Removing more would leave earlier trailing
    // context unprocessed while still satisfying a shortened preimage match.
    if hunk_leading_trailing_context(hunk).1 == 0 {
        return;
    }
    let mut trial = hunk.clone();
    if !pop_trailing_context_line(&mut trial) {
        return;
    }
    if hunk_old_side_matches_at(old_lines, &trial, start, ws_mode, fp) {
        *hunk = trial;
    }
}

fn pop_trailing_context_line(hunk: &mut Hunk) -> bool {
    if hunk.lines.is_empty() {
        return false;
    }
    let mut i = hunk.lines.len();
    if matches!(hunk.lines.last(), Some(HunkLine::NoNewline)) {
        i -= 1;
    }
    if i == 0 {
        return false;
    }
    i -= 1;
    if !matches!(hunk.lines.get(i), Some(HunkLine::Context(_))) {
        return false;
    }
    let end = if matches!(hunk.lines.get(i + 1), Some(HunkLine::NoNewline)) {
        i + 2
    } else {
        i + 1
    };
    hunk.lines.drain(i..end);
    hunk.old_count = hunk.old_count.saturating_sub(1);
    hunk.new_count = hunk.new_count.saturating_sub(1);
    true
}

fn lines_equal(expected: &str, actual: &str, ws_mode: ApplyWhitespaceMode) -> bool {
    if expected == actual {
        return true;
    }
    if ws_mode.whitespace_fix
        && expand_tabs_for_compare(expected, ws_mode.tab_width)
            == expand_tabs_for_compare(actual, ws_mode.tab_width)
    {
        return true;
    }
    if ws_mode.ignore_space_change
        && canonicalize_space_change_line(expected) == canonicalize_space_change_line(actual)
    {
        return true;
    }
    ws_mode.whitespace_fix
        && normalize_ws_fix_line(expected, ws_mode.tab_width)
            == normalize_ws_fix_line(actual, ws_mode.tab_width)
}

fn apply_hunks(
    old_content: &str,
    fp: &FilePatch,
    ws_mode: ApplyWhitespaceMode,
    forward: bool,
    patch_input_display: &str,
) -> Result<String> {
    let hunks = &fp.hunks;
    // Split into lines, keeping track of trailing newline
    let has_trailing_newline = old_content.is_empty() || old_content.ends_with('\n');
    let old_lines: Vec<&str> = if old_content.is_empty() {
        Vec::new()
    } else {
        old_content.lines().collect()
    };

    let mut result: Vec<String> = Vec::new();
    let mut old_idx: usize = 0; // 0-based index into old_lines
    let mut offset: isize = 0; // accumulated offset from previous hunks
    let mut ws_errors: u32 = 0;

    for hunk in hunks {
        let mut hunk_to_apply = hunk.clone();
        let p_ctx = ws_mode.context.unwrap_or(usize::MAX);

        loop {
            let required_context = ws_mode
                .context
                .unwrap_or(hunk_to_apply.old_count)
                .min(hunk_to_apply.old_count);
            let mut leading_context_fuzz_remaining = if ws_mode.context.is_some() {
                hunk_to_apply.old_count.saturating_sub(required_context)
            } else {
                0
            };
            let mut matched_old_side = false;

            let mut eof_new_blank = 0usize;
            let mut eof_found_line = 0usize;

            let nominal_start = nominal_hunk_start(&hunk_to_apply, old_lines.len())? as isize;
            let hunk_start = (nominal_start + offset).max(0) as usize;

            let (mut leading, mut trailing) = hunk_leading_trailing_context(&hunk_to_apply);
            let mut match_beginning = hunk_to_apply.old_start == 0
                || (hunk_to_apply.old_start == 1 && !ws_mode.unidiff_zero);
            let mut match_end =
                !ws_mode.unidiff_zero && trailing == 0 && hunk_to_apply.old_count > 0;

            let mut applied_pos: Option<usize> = None;
            loop {
                if !match_beginning && !match_end {
                    let adjusted_nominal = if hunk_to_apply.old_count > 0 {
                        let required_context = ws_mode
                            .context
                            .unwrap_or(hunk_to_apply.old_count)
                            .min(hunk_to_apply.old_count);
                        hunk_start.saturating_sub(hunk_to_apply.old_count - required_context)
                    } else {
                        hunk_start
                    };
                    if match_hunk_old_side_at(
                        old_lines.as_slice(),
                        &hunk_to_apply,
                        adjusted_nominal,
                        ws_mode,
                        ws_mode.context.is_some(),
                        fp,
                    ) {
                        applied_pos = Some(adjusted_nominal);
                    } else {
                        let max_scan = old_lines.len();
                        for delta in 1..=max_scan {
                            if adjusted_nominal >= delta
                                && match_hunk_old_side_at(
                                    old_lines.as_slice(),
                                    &hunk_to_apply,
                                    adjusted_nominal - delta,
                                    ws_mode,
                                    ws_mode.context.is_some(),
                                    fp,
                                )
                            {
                                applied_pos = Some(adjusted_nominal - delta);
                                break;
                            }
                            if adjusted_nominal + delta <= old_lines.len()
                                && match_hunk_old_side_at(
                                    old_lines.as_slice(),
                                    &hunk_to_apply,
                                    adjusted_nominal + delta,
                                    ws_mode,
                                    ws_mode.context.is_some(),
                                    fp,
                                )
                            {
                                applied_pos = Some(adjusted_nominal + delta);
                                break;
                            }
                        }
                    }
                }

                if applied_pos.is_none() {
                    applied_pos = find_pos_scan(
                        old_lines.as_slice(),
                        &hunk_to_apply,
                        ws_mode,
                        fp,
                        match_beginning,
                        match_end,
                        hunk_start,
                    );
                }

                if applied_pos.is_some() {
                    break;
                }

                if leading <= p_ctx && trailing <= p_ctx {
                    break;
                }

                if match_beginning || match_end {
                    match_beginning = false;
                    match_end = false;
                    continue;
                }

                let mut did_reduce = false;
                if leading >= trailing {
                    did_reduce |= pop_leading_context_line(&mut hunk_to_apply);
                }
                let (l_after_lead, t_after_lead) = hunk_leading_trailing_context(&hunk_to_apply);
                if t_after_lead > l_after_lead {
                    did_reduce |= pop_trailing_context_line(&mut hunk_to_apply);
                }
                if !did_reduce {
                    break;
                }
                (leading, trailing) = hunk_leading_trailing_context(&hunk_to_apply);
                match_end = !ws_mode.unidiff_zero && trailing == 0 && hunk_to_apply.old_count > 0;
            }

            let Some(actual_start) = applied_pos else {
                bail!("patch does not apply");
            };

            if ws_mode.whitespace_fix {
                minimize_trailing_context_at(
                    &mut hunk_to_apply,
                    old_lines.as_slice(),
                    actual_start,
                    ws_mode,
                    fp,
                );
            }

            while old_idx < actual_start && old_idx < old_lines.len() {
                result.push(old_lines[old_idx].to_string());
                old_idx += 1;
            }

            if actual_start != hunk_start {
                offset += actual_start as isize - hunk_start as isize;
            }

            let apply_outcome = (|| -> Result<()> {
                let mut li = 0usize;
                let mut cur_patch_line = hunk_to_apply.first_body_line;
                let mut pending_incomplete_body: Option<String> = None;
                while li < hunk_to_apply.lines.len() {
                    match &hunk_to_apply.lines[li] {
                        HunkLine::Context(s) => {
                            pending_incomplete_body = None;
                            let has_nl = !matches!(
                                hunk_to_apply.lines.get(li + 1),
                                Some(HunkLine::NoNewline)
                            );
                            let body = patch_line_body_for_ws(s, has_nl);
                            let plen = if has_nl {
                                body.len().saturating_sub(1)
                            } else {
                                body.len()
                            };
                            let is_blank_context = (fp.ws_rule & WS_BLANK_AT_EOF) != 0
                                && (plen == 0 || ws::ws_blank_line(&body[..plen]));
                            if !is_blank_context {
                                eof_new_blank = 0;
                            }
                            if ws_mode.whitespace_fix {
                                let flags = ws::ws_check(&body, fp.ws_rule);
                                record_ws_error(
                                    &ws_mode,
                                    &mut ws_errors,
                                    flags,
                                    patch_input_display,
                                    cur_patch_line,
                                    None,
                                );
                            }
                            if old_idx >= old_lines.len() {
                                bail!(
                                    "context mismatch at line {}: expected {:?}, got EOF",
                                    old_idx + 1,
                                    s
                                );
                            }
                            let actual_line = old_lines[old_idx];
                            if !patch_line_matches_actual(s, actual_line, has_nl, ws_mode, fp) {
                                if ws_mode.context.is_some()
                                    && !matched_old_side
                                    && leading_context_fuzz_remaining > 0
                                {
                                    result.push(actual_line.to_string());
                                    old_idx += 1;
                                    leading_context_fuzz_remaining -= 1;
                                    cur_patch_line += 1;
                                    li += 1;
                                    if !has_nl {
                                        cur_patch_line += 1;
                                        li += 1;
                                    }
                                    continue;
                                }
                                bail!(
                                    "context mismatch at line {}: expected {:?}, got {:?}",
                                    old_idx + 1,
                                    s,
                                    actual_line
                                );
                            }
                            old_idx += 1;
                            let out_line = if ws_mode.whitespace_fix {
                                let body_in = patch_line_body_for_ws(actual_line, has_nl);
                                let (fixed, _) = ws::ws_fix_copy_line(&body_in, fp.ws_rule);
                                fixed.trim_end_matches('\n').to_string()
                            } else {
                                actual_line.to_string()
                            };
                            result.push(out_line);
                            matched_old_side = true;
                            cur_patch_line += 1;
                            li += 1;
                            if !has_nl {
                                cur_patch_line += 1;
                                li += 1;
                            }
                        }
                        HunkLine::Remove(s) => {
                            eof_new_blank = 0;
                            let has_nl = !matches!(
                                hunk_to_apply.lines.get(li + 1),
                                Some(HunkLine::NoNewline)
                            );
                            let check_side = if forward {
                                false
                            } else {
                                ws_mode.ws_cli != WsCliMode::NoWarn
                            };
                            if check_side {
                                let body = patch_line_body_for_ws(s, has_nl);
                                let flags = ws::ws_check(&body, fp.ws_rule);
                                if !has_nl && (flags & WS_INCOMPLETE_LINE) != 0 {
                                    pending_incomplete_body = Some(s.clone());
                                    let rest = flags & !WS_INCOMPLETE_LINE;
                                    record_ws_error(
                                        &ws_mode,
                                        &mut ws_errors,
                                        rest,
                                        patch_input_display,
                                        cur_patch_line,
                                        None,
                                    );
                                } else {
                                    pending_incomplete_body = None;
                                    record_ws_error(
                                        &ws_mode,
                                        &mut ws_errors,
                                        flags,
                                        patch_input_display,
                                        cur_patch_line,
                                        None,
                                    );
                                }
                            } else if !has_nl {
                                pending_incomplete_body = None;
                            }
                            if old_idx >= old_lines.len() {
                                bail!(
                                    "remove mismatch at line {}: expected {:?}, got EOF",
                                    old_idx + 1,
                                    s
                                );
                            }
                            if !patch_line_matches_actual(
                                s,
                                old_lines[old_idx],
                                has_nl,
                                ws_mode,
                                fp,
                            ) {
                                bail!(
                                    "remove mismatch at line {}: expected {:?}, got {:?}",
                                    old_idx + 1,
                                    s,
                                    old_lines[old_idx]
                                );
                            }
                            old_idx += 1;
                            matched_old_side = true;
                            if !has_nl {
                                if let Some(body) = pending_incomplete_body.take() {
                                    if ws_mode.ws_cli != WsCliMode::NoWarn {
                                        record_ws_error(
                                            &ws_mode,
                                            &mut ws_errors,
                                            WS_INCOMPLETE_LINE,
                                            patch_input_display,
                                            cur_patch_line + 1,
                                            Some(body.as_str()),
                                        );
                                    }
                                }
                                cur_patch_line += 2;
                                li += 2;
                            } else {
                                cur_patch_line += 1;
                                li += 1;
                            }
                        }
                        HunkLine::Add(s) => {
                            let has_nl = !matches!(
                                hunk_to_apply.lines.get(li + 1),
                                Some(HunkLine::NoNewline)
                            );
                            let body = patch_line_body_for_ws(s, has_nl);
                            let plen = if has_nl {
                                body.len().saturating_sub(1)
                            } else {
                                body.len()
                            };
                            let added_blank = (fp.ws_rule & WS_BLANK_AT_EOF) != 0
                                && ws::ws_blank_line(&body[..plen]);
                            if added_blank {
                                if eof_new_blank == 0 {
                                    eof_found_line = cur_patch_line;
                                }
                                eof_new_blank += 1;
                            } else {
                                eof_new_blank = 0;
                            }
                            let check_side = if forward {
                                ws_mode.ws_cli != WsCliMode::NoWarn
                            } else {
                                false
                            };
                            if check_side {
                                let flags = ws::ws_check(&body, fp.ws_rule);
                                if !has_nl && (flags & WS_INCOMPLETE_LINE) != 0 {
                                    pending_incomplete_body = Some(s.clone());
                                    let rest = flags & !WS_INCOMPLETE_LINE;
                                    record_ws_error(
                                        &ws_mode,
                                        &mut ws_errors,
                                        rest,
                                        patch_input_display,
                                        cur_patch_line,
                                        None,
                                    );
                                } else {
                                    pending_incomplete_body = None;
                                    record_ws_error(
                                        &ws_mode,
                                        &mut ws_errors,
                                        flags,
                                        patch_input_display,
                                        cur_patch_line,
                                        None,
                                    );
                                }
                            } else if !has_nl {
                                pending_incomplete_body = None;
                            }
                            let out_line = if ws_mode.whitespace_fix {
                                let body_in = patch_line_body_for_ws(s, has_nl);
                                let (fixed, _) = ws::ws_fix_copy_line(&body_in, fp.ws_rule);
                                fixed.trim_end_matches('\n').to_string()
                            } else {
                                s.clone()
                            };
                            result.push(out_line);
                            matched_old_side = true;
                            if !has_nl {
                                if let Some(body) = pending_incomplete_body.take() {
                                    if ws_mode.ws_cli != WsCliMode::NoWarn {
                                        record_ws_error(
                                            &ws_mode,
                                            &mut ws_errors,
                                            WS_INCOMPLETE_LINE,
                                            patch_input_display,
                                            cur_patch_line + 1,
                                            Some(body.as_str()),
                                        );
                                    }
                                }
                                cur_patch_line += 2;
                                li += 2;
                            } else {
                                cur_patch_line += 1;
                                li += 1;
                            }
                        }
                        HunkLine::NoNewline => {
                            cur_patch_line += 1;
                            li += 1;
                        }
                    }
                }

                if old_idx == old_lines.len()
                    && eof_new_blank > 0
                    && (fp.ws_rule & WS_BLANK_AT_EOF) != 0
                    && ws_mode.ws_cli != WsCliMode::NoWarn
                {
                    record_ws_error(
                        &ws_mode,
                        &mut ws_errors,
                        WS_BLANK_AT_EOF,
                        patch_input_display,
                        eof_found_line,
                        None,
                    );
                    if ws_mode.whitespace_fix {
                        for _ in 0..eof_new_blank {
                            if result.last().is_some_and(|l| ws::ws_blank_line(l)) {
                                result.pop();
                            }
                        }
                    }
                }
                Ok(())
            })();
            apply_outcome?;
            break;
        }
    }

    // Copy remaining lines after the last hunk
    while old_idx < old_lines.len() {
        result.push(old_lines[old_idx].to_string());
        old_idx += 1;
    }

    // Reconstruct with newlines
    if result.is_empty() {
        return Ok(String::new());
    }

    // Check if the last hunk has a no-newline marker for the new side.
    // The new side ends without newline if the last line that contributes
    // to the new side (Add or Context) is immediately followed by NoNewline.
    let ends_no_newline = hunks.last().is_some_and(|h| {
        let mut last_was_new_side = false; // true if last meaningful line goes to new side
        let mut saw_no_newline = false;
        for hl in &h.lines {
            match hl {
                HunkLine::Add(_) | HunkLine::Context(_) => {
                    last_was_new_side = true;
                    saw_no_newline = false;
                }
                HunkLine::NoNewline if last_was_new_side => {
                    saw_no_newline = true;
                }
                HunkLine::Remove(_) => {
                    last_was_new_side = false;
                    saw_no_newline = false;
                }
                _ => {}
            }
        }
        saw_no_newline
    });

    let mut out = result.join("\n");
    let force_no_trailing_newline =
        ws_mode.inaccurate_eof || (ws_mode.ignore_space_change && !has_trailing_newline);
    if !ends_no_newline && !force_no_trailing_newline && (has_trailing_newline || !hunks.is_empty())
    {
        out.push('\n');
    }

    if ws_errors > 0 && matches!(ws_mode.ws_cli, WsCliMode::Error) {
        bail!("{} line adds whitespace errors.", ws_errors);
    }

    Ok(out)
}

/// Apply hunks while collecting failed hunks for `--reject` mode.
///
/// Returns `(new_content, rejected_hunks)`.
fn apply_hunks_with_reject(
    old_content: &str,
    fp: &FilePatch,
    ws_mode: ApplyWhitespaceMode,
    forward: bool,
    patch_input_display: &str,
) -> Result<(String, Vec<Hunk>)> {
    let hunks = &fp.hunks;
    let has_trailing_newline = old_content.is_empty() || old_content.ends_with('\n');
    let mut lines: Vec<String> = if old_content.is_empty() {
        Vec::new()
    } else {
        old_content
            .lines()
            .map(std::string::ToString::to_string)
            .collect()
    };

    let mut offset: isize = 0;
    let mut rejected: Vec<Hunk> = Vec::new();
    let mut ws_errors: u32 = 0;

    for hunk in hunks {
        let mut eof_new_blank = 0usize;
        let mut eof_found_line = 0usize;

        let nominal_start = match nominal_hunk_start(hunk, lines.len()) {
            Ok(start) => start as isize,
            Err(_) => {
                rejected.push(hunk.clone());
                continue;
            }
        };
        let hunk_start = (nominal_start + offset).max(0) as usize;

        let old_lines: Vec<&str> = lines.iter().map(String::as_str).collect();
        let actual_start = find_hunk_start(
            &old_lines,
            hunk,
            hunk_start.min(old_lines.len()),
            ws_mode,
            ws_mode.context.is_some(),
            fp,
        );

        let mut idx = actual_start;
        let mut replacement: Vec<String> = Vec::new();
        let mut failed = false;

        let mut li = 0usize;
        let mut cur_patch_line = hunk.first_body_line;
        let mut pending_incomplete_body: Option<String> = None;
        while li < hunk.lines.len() {
            match &hunk.lines[li] {
                HunkLine::Context(s) => {
                    pending_incomplete_body = None;
                    let has_nl = !matches!(hunk.lines.get(li + 1), Some(HunkLine::NoNewline));
                    let body = patch_line_body_for_ws(s, has_nl);
                    let plen = if has_nl {
                        body.len().saturating_sub(1)
                    } else {
                        body.len()
                    };
                    let is_blank_context = (fp.ws_rule & WS_BLANK_AT_EOF) != 0
                        && (plen == 0 || ws::ws_blank_line(&body[..plen]));
                    if !is_blank_context {
                        eof_new_blank = 0;
                    }
                    if ws_mode.whitespace_fix {
                        let flags = ws::ws_check(&body, fp.ws_rule);
                        record_ws_error(
                            &ws_mode,
                            &mut ws_errors,
                            flags,
                            patch_input_display,
                            cur_patch_line,
                            None,
                        );
                    }
                    if idx >= lines.len()
                        || !patch_line_matches_actual(s, &lines[idx], has_nl, ws_mode, fp)
                    {
                        failed = true;
                        break;
                    }
                    let out_line = if ws_mode.whitespace_fix {
                        let body_in = patch_line_body_for_ws(&lines[idx], has_nl);
                        let (fixed, _) = ws::ws_fix_copy_line(&body_in, fp.ws_rule);
                        fixed.trim_end_matches('\n').to_string()
                    } else {
                        lines[idx].clone()
                    };
                    idx += 1;
                    replacement.push(out_line);
                    cur_patch_line += 1;
                    li += 1;
                    if !has_nl {
                        cur_patch_line += 1;
                        li += 1;
                    }
                }
                HunkLine::Remove(s) => {
                    eof_new_blank = 0;
                    let has_nl = !matches!(hunk.lines.get(li + 1), Some(HunkLine::NoNewline));
                    let check_side = if forward {
                        false
                    } else {
                        ws_mode.ws_cli != WsCliMode::NoWarn
                    };
                    if check_side {
                        let body = patch_line_body_for_ws(s, has_nl);
                        let flags = ws::ws_check(&body, fp.ws_rule);
                        if !has_nl && (flags & WS_INCOMPLETE_LINE) != 0 {
                            pending_incomplete_body = Some(s.clone());
                            let rest = flags & !WS_INCOMPLETE_LINE;
                            record_ws_error(
                                &ws_mode,
                                &mut ws_errors,
                                rest,
                                patch_input_display,
                                cur_patch_line,
                                None,
                            );
                        } else {
                            pending_incomplete_body = None;
                            record_ws_error(
                                &ws_mode,
                                &mut ws_errors,
                                flags,
                                patch_input_display,
                                cur_patch_line,
                                None,
                            );
                        }
                    } else if !has_nl {
                        pending_incomplete_body = None;
                    }
                    if idx >= lines.len()
                        || !patch_line_matches_actual(s, &lines[idx], has_nl, ws_mode, fp)
                    {
                        failed = true;
                        break;
                    }
                    idx += 1;
                    if !has_nl {
                        if let Some(body) = pending_incomplete_body.take() {
                            if ws_mode.ws_cli != WsCliMode::NoWarn {
                                record_ws_error(
                                    &ws_mode,
                                    &mut ws_errors,
                                    WS_INCOMPLETE_LINE,
                                    patch_input_display,
                                    cur_patch_line + 1,
                                    Some(body.as_str()),
                                );
                            }
                        }
                        cur_patch_line += 2;
                        li += 2;
                    } else {
                        cur_patch_line += 1;
                        li += 1;
                    }
                }
                HunkLine::Add(s) => {
                    let has_nl = !matches!(hunk.lines.get(li + 1), Some(HunkLine::NoNewline));
                    let body = patch_line_body_for_ws(s, has_nl);
                    let plen = if has_nl {
                        body.len().saturating_sub(1)
                    } else {
                        body.len()
                    };
                    let added_blank =
                        (fp.ws_rule & WS_BLANK_AT_EOF) != 0 && ws::ws_blank_line(&body[..plen]);
                    if added_blank {
                        if eof_new_blank == 0 {
                            eof_found_line = cur_patch_line;
                        }
                        eof_new_blank += 1;
                    } else {
                        eof_new_blank = 0;
                    }
                    let check_side = if forward {
                        ws_mode.ws_cli != WsCliMode::NoWarn
                    } else {
                        false
                    };
                    if check_side {
                        let flags = ws::ws_check(&body, fp.ws_rule);
                        if !has_nl && (flags & WS_INCOMPLETE_LINE) != 0 {
                            pending_incomplete_body = Some(s.clone());
                            let rest = flags & !WS_INCOMPLETE_LINE;
                            record_ws_error(
                                &ws_mode,
                                &mut ws_errors,
                                rest,
                                patch_input_display,
                                cur_patch_line,
                                None,
                            );
                        } else {
                            pending_incomplete_body = None;
                            record_ws_error(
                                &ws_mode,
                                &mut ws_errors,
                                flags,
                                patch_input_display,
                                cur_patch_line,
                                None,
                            );
                        }
                    } else if !has_nl {
                        pending_incomplete_body = None;
                    }
                    let out_line = if ws_mode.whitespace_fix {
                        let body_in = patch_line_body_for_ws(s, has_nl);
                        let (fixed, _) = ws::ws_fix_copy_line(&body_in, fp.ws_rule);
                        fixed.trim_end_matches('\n').to_string()
                    } else {
                        s.clone()
                    };
                    replacement.push(out_line);
                    if !has_nl {
                        if let Some(body) = pending_incomplete_body.take() {
                            if ws_mode.ws_cli != WsCliMode::NoWarn {
                                record_ws_error(
                                    &ws_mode,
                                    &mut ws_errors,
                                    WS_INCOMPLETE_LINE,
                                    patch_input_display,
                                    cur_patch_line + 1,
                                    Some(body.as_str()),
                                );
                            }
                        }
                        cur_patch_line += 2;
                        li += 2;
                    } else {
                        cur_patch_line += 1;
                        li += 1;
                    }
                }
                HunkLine::NoNewline => {
                    cur_patch_line += 1;
                    li += 1;
                }
            }
        }

        if failed {
            rejected.push(hunk.clone());
            continue;
        }

        if idx == lines.len()
            && eof_new_blank > 0
            && (fp.ws_rule & WS_BLANK_AT_EOF) != 0
            && ws_mode.ws_cli != WsCliMode::NoWarn
        {
            record_ws_error(
                &ws_mode,
                &mut ws_errors,
                WS_BLANK_AT_EOF,
                patch_input_display,
                eof_found_line,
                None,
            );
            if ws_mode.whitespace_fix {
                for _ in 0..eof_new_blank {
                    if replacement.last().is_some_and(|l| ws::ws_blank_line(l)) {
                        replacement.pop();
                    }
                }
            }
        }

        let removed_count = idx.saturating_sub(actual_start);
        lines.splice(actual_start..idx, replacement.iter().cloned());
        offset += replacement.len() as isize - removed_count as isize;
    }

    if ws_errors > 0 && matches!(ws_mode.ws_cli, WsCliMode::Error) {
        bail!("{} line adds whitespace errors.", ws_errors);
    }

    if lines.is_empty() {
        return Ok((String::new(), rejected));
    }

    let mut out = lines.join("\n");
    if has_trailing_newline && !ws_mode.inaccurate_eof {
        out.push('\n');
    }
    Ok((out, rejected))
}

fn render_hunk_line(line: &HunkLine) -> String {
    match line {
        HunkLine::Context(s) => format!(" {s}"),
        HunkLine::Add(s) => format!("+{s}"),
        HunkLine::Remove(s) => format!("-{s}"),
        HunkLine::NoNewline => "\\ No newline at end of file".to_string(),
    }
}

fn write_reject_file(path: &Path, patch: &FilePatch, rejected_hunks: &[Hunk]) -> Result<()> {
    if rejected_hunks.is_empty() {
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            fs::create_dir_all(parent)?;
        }
    }

    let old_hdr = patch.old_path.as_deref().unwrap_or("/dev/null");
    let new_hdr = patch.new_path.as_deref().unwrap_or("/dev/null");

    let mut out = String::new();
    out.push_str(&format!("--- {old_hdr}\n"));
    out.push_str(&format!("+++ {new_hdr}\n"));

    for hunk in rejected_hunks {
        out.push_str(&format!(
            "@@ -{},{} +{},{} @@\n",
            hunk.old_start, hunk.old_count, hunk.new_start, hunk.new_count
        ));
        for line in &hunk.lines {
            out.push_str(&render_hunk_line(line));
            out.push('\n');
        }
    }

    fs::write(path, out.as_bytes()).with_context(|| format!("failed to write {}", path.display()))
}

// ---------------------------------------------------------------------------
// Stat / numstat / summary output
// ---------------------------------------------------------------------------

fn apply_quote_path_fully() -> bool {
    Repository::discover(None)
        .ok()
        .and_then(|r| ConfigSet::load(Some(&r.git_dir), true).ok())
        .map(|c| c.quote_path_fully())
        .unwrap_or(true)
}

fn show_stat(patches: &[FilePatch], directory: Option<&str>, quote_fully: bool) {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    let mut total_add = 0usize;
    let mut total_del = 0usize;
    let mut max_path_len = 0usize;
    let mut entries: Vec<(String, usize, usize)> = Vec::new();

    for fp in patches {
        let path = fp
            .effective_path()
            .map(|p| adjust_path(p, directory))
            .unwrap_or_else(|| "(unknown)".to_string());
        let display = quote_c_style(&path, quote_fully);
        let (add, del) = count_hunk_changes(&fp.hunks);
        if display.len() > max_path_len {
            max_path_len = display.len();
        }
        total_add += add;
        total_del += del;
        entries.push((display, add, del));
    }

    for (path, add, del) in &entries {
        let total = add + del;
        let bar = make_stat_bar(*add, *del, 40);
        let _ = writeln!(
            out,
            " {path:<width$} | {total:>4} {bar}",
            width = max_path_len
        );
    }
    let n = entries.len();
    let _ = writeln!(
        out,
        " {n} file{s} changed, {total_add} insertion{si}(+), {total_del} deletion{sd}(-)",
        s = if n == 1 { "" } else { "s" },
        si = if total_add == 1 { "" } else { "s" },
        sd = if total_del == 1 { "" } else { "s" },
    );
}

fn show_numstat(patches: &[FilePatch], directory: Option<&str>, quote_fully: bool) {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    for fp in patches {
        let path = fp
            .effective_path()
            .map(|p| adjust_path(p, directory))
            .unwrap_or_else(|| "(unknown)".to_string());
        let display = quote_c_style(&path, quote_fully);
        let (add, del) = count_hunk_changes(&fp.hunks);
        let _ = writeln!(out, "{add}\t{del}\t{display}");
    }
}

fn show_summary(patches: &[FilePatch], directory: Option<&str>) {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    for fp in patches {
        let path = fp
            .effective_path()
            .map(|p| adjust_path(p, directory))
            .unwrap_or_else(|| "(unknown)".to_string());

        if fp.is_new {
            let mode = fp.new_mode.as_deref().unwrap_or("100644");
            let _ = writeln!(out, " create mode {mode} {path}");
        } else if fp.is_deleted {
            let mode = fp.old_mode.as_deref().unwrap_or("100644");
            let _ = writeln!(out, " delete mode {mode} {path}");
        } else if fp.is_rename || fp.is_copy {
            let old = fp
                .old_path
                .as_deref()
                .map(|p| adjust_path(p, directory))
                .unwrap_or_else(|| "(unknown)".to_string());
            let kind = if fp.is_copy { "copy" } else { "rename" };
            let pct = fp.similarity_index.unwrap_or(100);
            let compact = compact_rename_path(&old, &path);
            let _ = writeln!(out, " {kind} {compact} ({pct}%)");
        } else if fp.dissimilarity_index.is_some() {
            let pct = fp.dissimilarity_index.unwrap();
            let _ = writeln!(out, " rewrite {path} ({pct}%)");
        } else if fp.old_mode.is_some() && fp.new_mode.is_some() && fp.old_mode != fp.new_mode {
            let _ = writeln!(
                out,
                " mode change {} => {} {path}",
                fp.old_mode.as_deref().unwrap_or(""),
                fp.new_mode.as_deref().unwrap_or("")
            );
        }
    }
}

fn count_hunk_changes(hunks: &[Hunk]) -> (usize, usize) {
    let mut add = 0;
    let mut del = 0;
    for hunk in hunks {
        for hl in &hunk.lines {
            match hl {
                HunkLine::Add(_) => add += 1,
                HunkLine::Remove(_) => del += 1,
                _ => {}
            }
        }
    }
    (add, del)
}

fn make_stat_bar(add: usize, del: usize, max_width: usize) -> String {
    let total = add + del;
    if total == 0 {
        return String::new();
    }
    let width = total.min(max_width);
    let plus_width = if total <= max_width {
        add
    } else {
        (add as f64 / total as f64 * max_width as f64).round() as usize
    };
    let minus_width = width - plus_width;
    format!("{}{}", "+".repeat(plus_width), "-".repeat(minus_width))
}

// ---------------------------------------------------------------------------
// Main run
// ---------------------------------------------------------------------------

pub fn run(mut args: Args) -> Result<()> {
    if let Some(dir) = args.directory.take() {
        let norm = normalize_apply_directory(&dir)?;
        args.directory = if norm.is_empty() { None } else { Some(norm) };
    }

    // Validate repository format if operating on the index or doing a check that requires it
    if args.cached || args.index || args.check {
        if let Some(git_dir) = crate::commands::config::resolve_git_dir_pub() {
            if let Err(e) = grit_lib::repo::validate_repo_format(&git_dir) {
                bail!("{}", e);
            }
        }
    }

    // Read patch input
    let input = if args.patches.is_empty() {
        let mut buf = String::new();
        io::stdin()
            .read_to_string(&mut buf)
            .context("failed to read patch from stdin")?;
        buf
    } else {
        let mut buf = String::new();
        for path in &args.patches {
            let content = if path.as_os_str() == "-" {
                let mut s = String::new();
                io::stdin()
                    .read_to_string(&mut s)
                    .context("failed to read patch from stdin")?;
                s
            } else {
                fs::read_to_string(path)
                    .with_context(|| format!("cannot read {}", path.display()))?
            };
            buf.push_str(&content);
            if !content.ends_with('\n') {
                buf.push('\n');
            }
        }
        buf
    };

    let mut patches = parse_patch(&input, args.strip)?;
    normalize_mismatched_diff_git_paths(&mut patches, args.strip);
    postprocess_gitlink_file_patches(&mut patches);
    merge_adjacent_blob_to_gitlink_patches(&mut patches);
    validate_patch_headers(&patches, args.strip)?;

    if args.reverse {
        reverse_patches(&mut patches);
    }
    assign_ws_rules(&mut patches, &args);
    if patches.is_empty() && !args.allow_empty {
        bail!("No valid patches in input");
    }

    let patch_input_display = if args.patches.len() == 1 && args.patches[0].as_os_str() != "-" {
        args.patches[0].to_string_lossy().into_owned()
    } else {
        "<stdin>".to_string()
    };

    // Info-only modes unless explicitly overridden by --apply.
    let info_only = (args.stat || args.numstat || args.summary) && !args.apply;
    let quote_fully = if args.stat || args.numstat {
        apply_quote_path_fully()
    } else {
        true
    };
    if args.stat {
        show_stat(&patches, args.directory.as_deref(), quote_fully);
    }
    if args.numstat {
        show_numstat(&patches, args.directory.as_deref(), quote_fully);
    }
    if args.summary {
        show_summary(&patches, args.directory.as_deref());
    }
    if info_only {
        return Ok(());
    }

    if let Some(path) = &args.build_fake_ancestor {
        build_fake_ancestor_file(&patches, &args, path)?;
        return Ok(());
    }
    verify_patch_paths_not_beyond_symlink(&patches, &args)?;
    let ws_mode = resolve_apply_whitespace_mode(&args);

    if args.three_way && args.reject {
        bail!("options '--reject' and '--3way' cannot be used together");
    }
    if args.three_way && Repository::discover(None).is_err() {
        bail!("'--3way' outside a repository");
    }

    // For --cached, we need a repository and index.
    // For working tree apply, we may or may not be in a repo.
    if args.cached {
        apply_to_index(&patches, &args, ws_mode, &patch_input_display)?;
    } else {
        if args.check {
            check_patches(&patches, &args, ws_mode, &patch_input_display)?;
            if !args.apply {
                return Ok(());
            }
        }

        if args.index {
            if let Ok(repo) = Repository::discover(None) {
                if let Some(wt) = repo.work_tree.as_deref() {
                    let index = repo.load_index().unwrap_or_else(|_| Index::new());
                    for fp in &patches {
                        verify_worktree_matches_index_for_patch(fp, &args)?;
                        if fp.involves_gitlink() {
                            verify_worktree_gitlink_patch(fp, &args, wt, &index)?;
                        }
                    }
                }
            } else {
                for fp in &patches {
                    verify_worktree_matches_index_for_patch(fp, &args)?;
                }
            }
            apply_to_worktree(&patches, &args, ws_mode, &patch_input_display)?;
            apply_to_index(&patches, &args, ws_mode, &patch_input_display)?;
            ensure_gitlink_placeholder_dirs(&patches, &args)?;
        } else {
            apply_to_worktree(&patches, &args, ws_mode, &patch_input_display)?;
            if args.intent_to_add {
                apply_intent_to_add_entries(&patches, &args)?;
            }
        }
    }

    if args.three_way && !args.cached {
        if let Ok(repo) = Repository::discover(None) {
            let config = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
            if rerere_enabled(&config, &repo.git_dir) {
                let _ = repo_rerere(&repo, RerereAutoupdate::FromConfig);
            }
        }
    }

    Ok(())
}

/// Build a temporary index file containing original blob versions referenced by the patch.
///
/// This implements `git apply --build-fake-ancestor=<file>` behavior.
fn build_fake_ancestor_file(patches: &[FilePatch], args: &Args, out_path: &Path) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let current_index = repo.load_index().unwrap_or_else(|_| Index::new());
    let mut fake = Index::new();

    for fp in patches {
        let Some(raw_old_path) = fp.old_path.as_deref() else {
            continue;
        };
        if raw_old_path == "/dev/null" {
            continue;
        }
        let adjusted = adjust_path(raw_old_path, args.directory.as_deref());
        if adjusted.is_empty() {
            continue;
        }

        let resolved = if let Some(old_oid) = fp.old_oid.as_deref() {
            let oid = resolve_revision(&repo, old_oid)
                .with_context(|| format!("resolving old blob id `{old_oid}` for `{adjusted}`"))?;
            let mode = fp.old_mode.as_deref().map(parse_mode).unwrap_or(0o100644);
            Some((mode, oid))
        } else {
            current_index
                .get(adjusted.as_bytes(), 0)
                .map(|entry| (entry.mode, entry.oid))
        };

        let Some((mode, oid)) = resolved else {
            continue;
        };

        let entry = IndexEntry {
            ctime_sec: 0,
            ctime_nsec: 0,
            mtime_sec: 0,
            mtime_nsec: 0,
            dev: 0,
            ino: 0,
            mode,
            uid: 0,
            gid: 0,
            size: 0,
            oid,
            flags: ((adjusted.len().min(0xFFF)) as u16) & 0x0FFF,
            flags_extended: None,
            path: adjusted.into_bytes(),
        };
        fake.add_or_replace(entry);
    }

    if let Some(parent) = out_path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
    }
    fake.write(out_path)
        .with_context(|| format!("writing {}", out_path.display()))?;
    Ok(())
}

/// Repository + index state for CRLF-aware apply (clean/smudge around unified diffs).
struct ApplyCrlfContext {
    repo: Repository,
    work_tree: PathBuf,
    index: Index,
    config: ConfigSet,
    conv: crlf::ConversionConfig,
}

impl ApplyCrlfContext {
    fn load() -> Option<Self> {
        let repo = Repository::discover(None).ok()?;
        let work_tree = repo.work_tree.clone()?;
        let index = repo.load_index().unwrap_or_else(|_| Index::new());
        let config = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
        let conv = crlf::ConversionConfig::from_config(&config);
        Some(Self {
            repo,
            work_tree,
            index,
            config,
            conv,
        })
    }

    /// Bytes that hash to the same blob OID as `git add` / the index (clean direction).
    fn blob_for_index_hash(&self, path: &Path, rel_path: &str, mode: u32) -> Result<Vec<u8>> {
        if mode == grit_lib::index::MODE_SYMLINK {
            return read_symlink_target_bytes(path);
        }
        let raw = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
        let rules = crlf::load_gitattributes_for_checkout(
            &self.work_tree,
            rel_path,
            &self.index,
            &self.repo.odb,
        );
        let file_attrs = crlf::get_file_attrs(&rules, rel_path, false, &self.config);
        crlf::convert_to_git(&raw, rel_path, &self.conv, &file_attrs)
            .map_err(|e| anyhow::anyhow!("{e}"))
    }

    /// Normalized LF text for unified-diff hunk matching (same pipeline as index blobs).
    fn normalized_text(&self, path: &Path, rel_path: &str) -> Result<String> {
        let meta = fs::symlink_metadata(path)?;
        if meta.file_type().is_symlink() {
            let b = read_symlink_target_bytes(path)?;
            return Ok(String::from_utf8_lossy(&b).into_owned());
        }
        let raw = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
        let rules = crlf::load_gitattributes_for_checkout(
            &self.work_tree,
            rel_path,
            &self.index,
            &self.repo.odb,
        );
        let file_attrs = crlf::get_file_attrs(&rules, rel_path, false, &self.config);
        let normalized = crlf::convert_to_git(&raw, rel_path, &self.conv, &file_attrs)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(String::from_utf8_lossy(&normalized).into_owned())
    }
}

fn file_attrs_for_apply_path(ctx: &ApplyCrlfContext, rel_path: &str) -> FileAttrs {
    let rules =
        crlf::load_gitattributes_for_checkout(&ctx.work_tree, rel_path, &ctx.index, &ctx.repo.odb);
    crlf::get_file_attrs(&rules, rel_path, false, &ctx.config)
}

fn file_attrs_for_index_apply(repo: &Repository, index: &Index, rel_path: &str) -> FileAttrs {
    let Ok(config) = ConfigSet::load(Some(&repo.git_dir), true) else {
        return FileAttrs::default();
    };
    let Some(wt) = repo.work_tree.as_ref() else {
        return FileAttrs::default();
    };
    let rules = crlf::load_gitattributes_for_checkout(wt, rel_path, index, &repo.odb);
    crlf::get_file_attrs(&rules, rel_path, false, &config)
}

/// Submodule/gitlink patches: ensure the work tree state matches what `--index` apply expects.
fn verify_worktree_gitlink_patch(
    fp: &FilePatch,
    args: &Args,
    work_tree: &Path,
    index: &Index,
) -> Result<()> {
    if fp.is_deleted && fp.old_mode.as_deref() == Some("160000") {
        return Ok(());
    }
    let blob_to_gitlink = fp.new_mode.as_deref() == Some("160000")
        && fp.old_mode.as_deref() != Some("160000")
        && !fp.is_deleted;
    if (fp.is_new && fp.new_mode.as_deref() == Some("160000")) || blob_to_gitlink {
        let Some(target) = fp.target_path() else {
            return Ok(());
        };
        let adjusted = adjust_path(target, args.directory.as_deref());
        let path = work_tree.join(&adjusted);
        if path.exists() && path.is_file() {
            // Replacing a tracked regular file (or symlink) with a submodule: the work tree must
            // still match the index entry for that path (`replace_sub1_with_file` → submodule).
            let Some(entry) = index.get(adjusted.as_bytes(), 0) else {
                bail!("{}: already exists", adjusted);
            };
            if entry.mode == grit_lib::index::MODE_GITLINK {
                bail!("{}: already exists", adjusted);
            }
            let wt_oid = if let Some(ctx) = ApplyCrlfContext::load() {
                let bytes = ctx.blob_for_index_hash(&path, &adjusted, entry.mode)?;
                grit_lib::odb::Odb::hash_object_data(ObjectKind::Blob, &bytes)
            } else if entry.mode == grit_lib::index::MODE_SYMLINK {
                let b = read_symlink_target_bytes(&path)?;
                grit_lib::odb::Odb::hash_object_data(ObjectKind::Blob, &b)
            } else {
                let raw = fs::read(&path)
                    .with_context(|| format!("failed to read {}", path.display()))?;
                grit_lib::odb::Odb::hash_object_data(ObjectKind::Blob, &raw)
            };
            if wt_oid != entry.oid {
                bail!("{adjusted}: does not match index");
            }
            return Ok(());
        }
        if path.exists() && !path.is_dir() {
            bail!("{}: already exists", adjusted);
        }
        return Ok(());
    }
    if fp.old_mode.as_deref() == Some("160000") && !fp.is_deleted {
        let Some(source) = fp.source_path() else {
            return Ok(());
        };
        let adjusted = adjust_path(source, args.directory.as_deref());
        let path = work_tree.join(&adjusted);
        if !path.exists() {
            bail!("{}: does not exist", adjusted);
        }
        if !path.is_dir() {
            bail!("{}: does not match index", adjusted);
        }
    }
    Ok(())
}

/// After `--index` apply, nested file patches may have removed an empty gitlink placeholder
/// directory via `remove_empty_dirs_up`; recreate empty dirs for new/changed gitlinks.
fn ensure_gitlink_placeholder_dirs(patches: &[FilePatch], args: &Args) -> Result<()> {
    for fp in patches {
        let target_is_gitlink = fp.new_mode.as_deref() == Some("160000") && !fp.is_deleted;
        let need_empty_submodule_dir =
            target_is_gitlink && (fp.is_new || fp.old_mode.as_deref() != Some("160000"));
        if !need_empty_submodule_dir {
            continue;
        }
        let Some(path_str) = fp.target_path() else {
            continue;
        };
        let adjusted = adjust_path(path_str, args.directory.as_deref());
        let path = PathBuf::from(&adjusted);
        if path.is_dir() {
            continue;
        }
        if path.is_file() || path.is_symlink() {
            fs::remove_file(&path).with_context(|| {
                format!("failed to remove file at submodule path {}", path.display())
            })?;
        }
        if !path.exists() {
            fs::create_dir_all(&path)
                .with_context(|| format!("failed to create {}", path.display()))?;
        }
    }
    Ok(())
}

/// Verify the working tree matches the index for paths touched by one patch (`git apply --index`).
///
/// Git checks each patch in sequence; unrelated dirty files must not abort the whole apply
/// (`t4108-apply-threeway` dirty working tree case).
fn verify_worktree_matches_index_for_patch(fp: &FilePatch, args: &Args) -> Result<()> {
    let Some(ctx) = ApplyCrlfContext::load() else {
        return Ok(());
    };

    if fp.is_new {
        if let Some(target) = fp.target_path() {
            let adjusted = adjust_path(target, args.directory.as_deref());
            if let Some(entry) = ctx.index.get(adjusted.as_bytes(), 0) {
                let path = PathBuf::from(&adjusted);
                if !path.exists() {
                    bail!("{adjusted}: does not match index");
                }
                let wt_content = ctx.blob_for_index_hash(&path, &adjusted, entry.mode)?;
                let wt_oid = grit_lib::odb::Odb::hash_object_data(ObjectKind::Blob, &wt_content);
                if wt_oid != entry.oid {
                    bail!("{adjusted}: does not match index");
                }
            }
        }
        return Ok(());
    }
    if fp.old_mode.as_deref() == Some("160000")
        || (fp.new_mode.as_deref() == Some("160000") && fp.old_mode.as_deref() == Some("160000"))
    {
        return Ok(());
    }
    let path_str = fp
        .source_path()
        .ok_or_else(|| anyhow::anyhow!("patch has no file path"))?;
    let adjusted = adjust_path(path_str, args.directory.as_deref());
    let path = PathBuf::from(&adjusted);

    if !path.exists() {
        return Ok(());
    }

    if let Some(entry) = ctx.index.get(adjusted.as_bytes(), 0) {
        let wt_content = ctx.blob_for_index_hash(&path, &adjusted, entry.mode)?;
        let wt_oid = grit_lib::odb::Odb::hash_object_data(ObjectKind::Blob, &wt_content);
        if wt_oid != entry.oid {
            bail!("{adjusted}: does not match index");
        }
    }

    Ok(())
}

fn read_symlink_target_bytes(path: &Path) -> Result<Vec<u8>> {
    let target = fs::read_link(path)
        .with_context(|| format!("failed to read symlink target {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        Ok(target.as_os_str().as_bytes().to_vec())
    }
    #[cfg(not(unix))]
    {
        Ok(target.to_string_lossy().into_owned().into_bytes())
    }
}

fn read_worktree_blob_as_text(path: &Path) -> io::Result<String> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        let target =
            read_symlink_target_bytes(path).map_err(|err| io::Error::other(err.to_string()))?;
        return Ok(String::from_utf8_lossy(&target).into_owned());
    }
    let bytes = fs::read(path)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Remove empty directories upward from path.
fn remove_empty_dirs_up(dir: &Path) {
    let mut current = dir.to_path_buf();
    while current.as_os_str() != "." && !current.as_os_str().is_empty() {
        if fs::remove_dir(&current).is_err() {
            break;
        }
        match current.parent() {
            Some(parent) => current = parent.to_path_buf(),
            None => break,
        }
    }
}

fn remove_path_for_replacement(path: &Path) -> Result<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(err)
            if matches!(
                err.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            ) =>
        {
            return Ok(());
        }
        Err(err) => return Err(err).with_context(|| format!("failed to stat {}", path.display())),
    };
    if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() {
        fs::remove_dir_all(path)
            .with_context(|| format!("failed to remove directory {}", path.display()))?;
    } else {
        fs::remove_file(path).with_context(|| format!("failed to remove {}", path.display()))?;
    }
    Ok(())
}

fn target_is_inside_source(target: &Path, source: &Path) -> bool {
    target
        .strip_prefix(source)
        .map(|suffix| !suffix.as_os_str().is_empty())
        .unwrap_or(false)
}

fn write_worktree_path(
    path: &Path,
    content: &str,
    mode: Option<&str>,
    source_exec_bit: Option<bool>,
    crlf_ctx: Option<&ApplyCrlfContext>,
    rel_path: &str,
) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            fs::create_dir_all(parent)?;
        }
    }

    if mode == Some("120000") {
        remove_path_for_replacement(path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            symlink(content, path)
                .with_context(|| format!("failed to create symlink {}", path.display()))?;
            return Ok(());
        }
        #[cfg(not(unix))]
        {
            fs::write(path, content.as_bytes())
                .with_context(|| format!("failed to write {}", path.display()))?;
            return Ok(());
        }
    }

    if path.is_dir() {
        fs::remove_dir_all(path)
            .with_context(|| format!("failed to remove directory {}", path.display()))?;
    }

    let bytes: Cow<'_, [u8]> = if let Some(ctx) = crlf_ctx {
        let rules = crlf::load_gitattributes_for_checkout(
            &ctx.work_tree,
            rel_path,
            &ctx.index,
            &ctx.repo.odb,
        );
        let file_attrs = crlf::get_file_attrs(&rules, rel_path, false, &ctx.config);
        Cow::Owned(
            crlf::convert_to_worktree(
                content.as_bytes(),
                rel_path,
                &ctx.conv,
                &file_attrs,
                None,
                None,
            )
            .map_err(|e| anyhow::anyhow!("{e}"))?,
        )
    } else {
        Cow::Borrowed(content.as_bytes())
    };
    fs::write(path, &bytes).with_context(|| format!("failed to write {}", path.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Some(mode) = mode {
            let perm = if mode == "100755" { 0o755 } else { 0o644 };
            fs::set_permissions(path, fs::Permissions::from_mode(perm))?;
        } else if let Some(executable) = source_exec_bit {
            let perm = if executable { 0o755 } else { 0o644 };
            fs::set_permissions(path, fs::Permissions::from_mode(perm))?;
        }
    }

    Ok(())
}

fn verify_old_oid_matches_content(expected_oid: &str, content: &str) -> Result<()> {
    verify_old_oid_matches_bytes(expected_oid, content.as_bytes())
}

fn verify_old_oid_matches_bytes(expected_oid: &str, content: &[u8]) -> Result<()> {
    let actual_oid = grit_lib::odb::Odb::hash_object_data(ObjectKind::Blob, content);
    let actual_hex = actual_oid.to_hex();
    if !actual_hex.starts_with(expected_oid) {
        bail!("patch does not apply");
    }
    Ok(())
}

fn record_path_existence(
    path: &str,
    current: &mut HashMap<String, bool>,
    initial: &mut HashMap<String, bool>,
) {
    if current.contains_key(path) {
        return;
    }
    let exists = Path::new(path).exists();
    current.insert(path.to_string(), exists);
    initial.insert(path.to_string(), exists);
}

/// Return true when a patch can apply to a missing preimage as empty content.
///
/// This matches hunks whose old side starts from line 0 with count 0 and
/// contains only additions.
fn can_apply_with_empty_preimage(fp: &FilePatch) -> bool {
    if fp.hunks.is_empty() {
        return false;
    }

    fp.hunks.iter().all(|hunk| {
        hunk.old_start == 0
            && hunk.old_count == 0
            && hunk
                .lines
                .iter()
                .all(|line| matches!(line, HunkLine::Add(_) | HunkLine::NoNewline))
    })
}

/// Preflight worktree patch ordering/path availability before writing.
///
/// This catches invalid sequences (e.g. later patches reading a path that was
/// moved away by an earlier rename) and prevents partially-applied worktree
/// state when such sequences are detected.
fn precheck_worktree_patch_sequence(patches: &[FilePatch], args: &Args) -> Result<()> {
    let mut current_exists: HashMap<String, bool> = HashMap::new();
    let mut initial_exists: HashMap<String, bool> = HashMap::new();

    for fp in patches {
        if let Some(source) = fp.source_path() {
            let adjusted = adjust_path(source, args.directory.as_deref());
            record_path_existence(&adjusted, &mut current_exists, &mut initial_exists);
        }
        if let Some(target) = fp.target_path() {
            let adjusted = adjust_path(target, args.directory.as_deref());
            record_path_existence(&adjusted, &mut current_exists, &mut initial_exists);
        }
    }

    for fp in patches {
        let source_adjusted = fp
            .source_path()
            .map(|p| adjust_path(p, args.directory.as_deref()))
            .unwrap_or_default();
        let target_adjusted = fp
            .target_path()
            .map(|p| adjust_path(p, args.directory.as_deref()))
            .unwrap_or_default();

        let source_exists_now = current_exists
            .get(&source_adjusted)
            .copied()
            .unwrap_or_else(|| Path::new(&source_adjusted).exists());
        let source_existed_initially = initial_exists
            .get(&source_adjusted)
            .copied()
            .unwrap_or(source_exists_now);

        if fp.is_deleted {
            if !source_exists_now {
                if source_existed_initially {
                    continue;
                }
                bail!(
                    "failed to read {}: No such file or directory (os error 2)",
                    source_adjusted
                );
            }
            set_path_and_descendants_state(&mut current_exists, &source_adjusted, false);
            if source_adjusted != target_adjusted {
                set_path_and_descendants_state(&mut current_exists, &target_adjusted, false);
            }
            continue;
        }

        if fp.is_new {
            let target_exists_now = current_exists
                .get(&target_adjusted)
                .copied()
                .unwrap_or_else(|| Path::new(&target_adjusted).exists());
            if target_exists_now {
                if Path::new(&target_adjusted).is_dir() {
                    set_path_and_descendants_state(&mut current_exists, &target_adjusted, false);
                } else if !args.three_way {
                    // With `--3way`, Git may emit multiple add/add hunks for the same path
                    // (`t4108-apply-threeway`); the real check happens during apply.
                    bail!("{target_adjusted}: already exists");
                }
            }
            current_exists.insert(target_adjusted.clone(), true);
            continue;
        }

        if !source_exists_now {
            let can_use_initial_snapshot = source_adjusted != target_adjusted
                && (fp.is_copy || fp.is_rename)
                && source_existed_initially;
            if !can_use_initial_snapshot && !can_apply_with_empty_preimage(fp) {
                bail!(
                    "failed to read {}: No such file or directory (os error 2)",
                    source_adjusted
                );
            }
        }

        if fp.is_rename && source_adjusted != target_adjusted {
            set_path_and_descendants_state(&mut current_exists, &source_adjusted, false);
        }
        current_exists.insert(target_adjusted, true);
    }

    Ok(())
}

fn set_path_and_descendants_state(
    current_exists: &mut HashMap<String, bool>,
    path: &str,
    exists: bool,
) {
    current_exists.insert(path.to_string(), exists);
    let prefix = format!("{path}/");
    let affected: Vec<String> = current_exists
        .keys()
        .filter(|key| key.starts_with(&prefix))
        .cloned()
        .collect();
    for key in affected {
        current_exists.insert(key, exists);
    }
}

fn apply_to_worktree(
    patches: &[FilePatch],
    args: &Args,
    ws_mode: ApplyWhitespaceMode,
    patch_input_display: &str,
) -> Result<()> {
    let crlf_ctx = ApplyCrlfContext::load();
    let mut had_rejects = false;
    let mut apply_nonzero = false;
    let three_way_allowed = crlf_ctx.is_some();
    let conflict_style = crlf_ctx
        .as_ref()
        .map(|c| resolve_apply_conflict_style(&c.repo))
        .unwrap_or(ConflictStyle::Merge);
    let merge_favor_cli = merge_favor_from_apply_args(args);
    // Snapshot source-side file contents used by cross-path rename/copy patches
    // so later modifications/removals do not affect subsequent patch sections.
    let mut source_snapshots: HashMap<String, String> = HashMap::new();
    for fp in patches {
        let Some(source) = fp.source_path() else {
            continue;
        };
        let Some(target) = fp.target_path() else {
            continue;
        };
        let source_adjusted = adjust_path(source, args.directory.as_deref());
        let target_adjusted = adjust_path(target, args.directory.as_deref());
        if source_adjusted == target_adjusted || source_snapshots.contains_key(&source_adjusted) {
            continue;
        }
        let snap_ok = match &crlf_ctx {
            Some(ctx) => ctx.normalized_text(Path::new(&source_adjusted), &source_adjusted),
            None => read_worktree_blob_as_text(Path::new(&source_adjusted))
                .map_err(|e| anyhow::anyhow!("{e}")),
        };
        if let Ok(content) = snap_ok {
            source_snapshots.insert(source_adjusted, content);
        }
    }
    precheck_worktree_patch_sequence(patches, args)?;

    for fp in patches {
        let path_str = fp
            .target_path()
            .ok_or_else(|| anyhow::anyhow!("patch has no file path"))?;
        let path_adjusted = adjust_path(path_str, args.directory.as_deref());
        let path = PathBuf::from(&path_adjusted);

        if fp.is_deleted {
            // Delete the file (or directory for submodules)
            if path.is_dir() {
                fs::remove_dir_all(&path)
                    .with_context(|| format!("failed to remove directory {}", path.display()))?;
            } else if path.exists() {
                fs::remove_file(&path)
                    .with_context(|| format!("failed to remove {}", path.display()))?;
            }
            // Clean up empty parent directories
            if let Some(parent) = path.parent() {
                remove_empty_dirs_up(parent);
            }
            continue;
        }

        if fp.new_mode.as_deref() == Some("160000")
            && fp.old_mode.as_deref() != Some("160000")
            && !fp.is_new
        {
            if path.is_file() || path.is_symlink() {
                fs::remove_file(&path).with_context(|| {
                    format!(
                        "failed to remove file before submodule dir {}",
                        path.display()
                    )
                })?;
            }
            fs::create_dir_all(&path)
                .with_context(|| format!("failed to create {}", path.display()))?;
            continue;
        }

        if fp.is_new {
            // Submodule: create directory
            if fp.new_mode.as_deref() == Some("160000") {
                if path.is_file() || path.is_symlink() {
                    fs::remove_file(&path).with_context(|| {
                        format!(
                            "failed to remove file before submodule dir {}",
                            path.display()
                        )
                    })?;
                }
                fs::create_dir_all(&path)?;
                continue;
            }
            let path_exists = path.exists();
            let add_add_threeway =
                args.three_way && path_exists && fp.old_path.as_deref() == Some("/dev/null");
            if add_add_threeway {
                let Some(ctx) = crlf_ctx.as_ref() else {
                    bail!("not a git repository");
                };
                let repo = &ctx.repo;
                let mode_for_read = ctx
                    .index
                    .get(path_adjusted.as_bytes(), 0)
                    .map(|e| e.mode)
                    .unwrap_or_else(|| parse_mode(fp.new_mode.as_deref().unwrap_or("100644")));
                let our_bytes = if path.is_file() || path.is_symlink() {
                    ctx.blob_for_index_hash(&path, &path_adjusted, mode_for_read)?
                } else {
                    bail!("cannot read the current contents of '{}'", path_str);
                };
                let file_attrs = file_attrs_for_apply_path(ctx, &path_adjusted);
                let favor = effective_merge_favor_for_path(&file_attrs, merge_favor_cli);
                if let Some(tw) = try_three_way_merge_blob(
                    repo,
                    fp,
                    &our_bytes,
                    ws_mode,
                    !args.reverse,
                    patch_input_display,
                    favor,
                    conflict_style,
                    &file_attrs,
                    true,
                )? {
                    if tw.conflicted {
                        apply_nonzero = true;
                    }
                    let merged_text = String::from_utf8_lossy(&tw.merged_bytes).into_owned();
                    write_worktree_path(
                        &path,
                        &merged_text,
                        fp.new_mode.as_deref(),
                        None,
                        crlf_ctx.as_ref(),
                        &path_adjusted,
                    )?;
                    continue;
                }
            }
            let content = apply_hunks("", fp, ws_mode, !args.reverse, patch_input_display)
                .with_context(|| {
                    format!("failed to apply hunks for new file {}", path.display())
                })?;
            write_worktree_path(
                &path,
                &content,
                fp.new_mode.as_deref(),
                None,
                crlf_ctx.as_ref(),
                &path_adjusted,
            )?;
            continue;
        }

        // Modify existing file — read preimage from source side (important
        // for rename/copy patches where target may not exist yet).
        let source_adjusted = fp
            .source_path()
            .map(|p| adjust_path(p, args.directory.as_deref()))
            .unwrap_or_else(|| path_adjusted.clone());
        let read_path = PathBuf::from(&source_adjusted);
        let source_contains_target =
            fp.is_rename && read_path != path && target_is_inside_source(&path, &read_path);

        if fp.binary_patch.is_some() {
            let old_bytes: Vec<u8> = if source_adjusted != path_adjusted {
                if !read_path.exists() && can_apply_with_empty_preimage(fp) {
                    Vec::new()
                } else if read_path.symlink_metadata()?.file_type().is_symlink() {
                    read_symlink_target_bytes(&read_path)?
                } else {
                    fs::read(&read_path)
                        .with_context(|| format!("failed to read {}", read_path.display()))?
                }
            } else if !read_path.exists() && can_apply_with_empty_preimage(fp) {
                Vec::new()
            } else if read_path.symlink_metadata()?.file_type().is_symlink() {
                read_symlink_target_bytes(&read_path)?
            } else {
                fs::read(&read_path)
                    .with_context(|| format!("failed to read {}", read_path.display()))?
            };
            if !args.reject {
                if let Some(expected_oid) = fp.old_oid.as_deref() {
                    if !ws_mode.whitespace_fix && !args.three_way {
                        verify_old_oid_matches_bytes(expected_oid, &old_bytes)?;
                    }
                }
            }
            #[cfg(unix)]
            let source_exec_bit = if source_adjusted != path_adjusted {
                use std::os::unix::fs::PermissionsExt;
                fs::metadata(&read_path)
                    .ok()
                    .map(|meta| meta.permissions().mode() & 0o111 != 0)
            } else {
                None
            };
            let binary_patch = fp.binary_patch.as_ref().expect("checked");
            let new_bytes = inflate_binary_payload(&binary_patch.forward_compressed)?;
            if let Some(expected_new) = fp.new_oid.as_deref() {
                let actual = grit_lib::odb::Odb::hash_object_data(ObjectKind::Blob, &new_bytes);
                if !actual.to_hex().starts_with(expected_new) {
                    bail!("patch does not apply");
                }
            }
            if source_contains_target {
                remove_path_for_replacement(&read_path)?;
            }
            if let Some(parent) = path.parent() {
                if !parent.as_os_str().is_empty() && !parent.exists() {
                    fs::create_dir_all(parent)?;
                }
            }
            fs::write(&path, &new_bytes)
                .with_context(|| format!("failed to write {}", path.display()))?;

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Some(mode) = fp.new_mode.as_deref() {
                    let perm = if mode == "100755" { 0o755 } else { 0o644 };
                    fs::set_permissions(&path, fs::Permissions::from_mode(perm))?;
                } else if let Some(executable) = source_exec_bit {
                    let perm = if executable { 0o755 } else { 0o644 };
                    fs::set_permissions(&path, fs::Permissions::from_mode(perm))?;
                }
            }

            if fp.is_rename && read_path != path && !source_contains_target {
                remove_path_for_replacement(&read_path)?;
                if let Some(parent) = read_path.parent() {
                    remove_empty_dirs_up(parent);
                }
            }
            continue;
        }

        let load_old_content_from_disk = || -> Result<String> {
            match &crlf_ctx {
                Some(ctx) => {
                    if !read_path.exists() && can_apply_with_empty_preimage(fp) {
                        Ok(String::new())
                    } else {
                        ctx.normalized_text(&read_path, &source_adjusted)
                    }
                }
                None => match read_worktree_blob_as_text(&read_path) {
                    Ok(content) => Ok(content),
                    Err(err)
                        if err.kind() == std::io::ErrorKind::NotFound
                            && can_apply_with_empty_preimage(fp) =>
                    {
                        Ok(String::new())
                    }
                    Err(err) => {
                        Err(err).with_context(|| format!("failed to read {}", read_path.display()))
                    }
                },
            }
        };
        let old_content = if source_adjusted != path_adjusted {
            if let Some(snapshot) = source_snapshots.get(&source_adjusted) {
                snapshot.clone()
            } else {
                load_old_content_from_disk()?
            }
        } else {
            load_old_content_from_disk()?
        };
        // With `--whitespace=fix`, a prior apply can normalize trailing whitespace so the
        // worktree no longer hashes to the patch's `index` line preimage (see t4125). Git
        // matches hunks against file content instead; skip strict OID checks in that mode.
        if !args.reject {
            if let Some(expected_oid) = fp.old_oid.as_deref() {
                if !ws_mode.whitespace_fix && !args.three_way {
                    verify_old_oid_matches_content(expected_oid, &old_content)?;
                }
            }
        }
        #[cfg(unix)]
        let source_exec_bit = if source_adjusted != path_adjusted {
            use std::os::unix::fs::PermissionsExt;
            fs::metadata(&read_path)
                .ok()
                .map(|meta| meta.permissions().mode() & 0o111 != 0)
        } else {
            None
        };

        if fp.hunks.is_empty() {
            if source_adjusted != path_adjusted {
                if source_contains_target {
                    remove_path_for_replacement(&read_path)?;
                }
                write_worktree_path(
                    &path,
                    &old_content,
                    fp.new_mode.as_deref(),
                    source_exec_bit,
                    crlf_ctx.as_ref(),
                    &path_adjusted,
                )?;
                if fp.is_rename && read_path != path && !source_contains_target {
                    remove_path_for_replacement(&read_path)?;
                    if let Some(parent) = read_path.parent() {
                        remove_empty_dirs_up(parent);
                    }
                }
                continue;
            }

            #[cfg(unix)]
            if let Some(mode) = fp.new_mode.as_deref() {
                use std::os::unix::fs::PermissionsExt;
                let perm = if mode == "100755" { 0o755 } else { 0o644 };
                fs::set_permissions(&path, fs::Permissions::from_mode(perm))?;
            }
            continue;
        }

        let (new_content, rejected_hunks) = if args.reject {
            apply_hunks_with_reject(
                &old_content,
                fp,
                ws_mode,
                !args.reverse,
                patch_input_display,
            )
            .with_context(|| format!("failed to apply patch to {}", path.display()))?
        } else if args.three_way && three_way_allowed {
            let ctx = crlf_ctx.as_ref().context("not a git repository")?;
            let repo = &ctx.repo;
            let file_attrs = file_attrs_for_apply_path(ctx, &path_adjusted);
            let favor = effective_merge_favor_for_path(&file_attrs, merge_favor_cli);
            if let Some(tw) = try_three_way_merge_blob(
                repo,
                fp,
                old_content.as_bytes(),
                ws_mode,
                !args.reverse,
                patch_input_display,
                favor,
                conflict_style,
                &file_attrs,
                false,
            )
            .with_context(|| format!("failed to apply patch to {}", path.display()))?
            {
                if tw.conflicted {
                    apply_nonzero = true;
                }
                (
                    String::from_utf8_lossy(&tw.merged_bytes).into_owned(),
                    Vec::new(),
                )
            } else {
                let content = apply_hunks(
                    &old_content,
                    fp,
                    ws_mode,
                    !args.reverse,
                    patch_input_display,
                )
                .with_context(|| format!("failed to apply patch to {}", path.display()))?;
                (content, Vec::new())
            }
        } else {
            let content = apply_hunks(
                &old_content,
                fp,
                ws_mode,
                !args.reverse,
                patch_input_display,
            )
            .with_context(|| format!("failed to apply patch to {}", path.display()))?;
            (content, Vec::new())
        };
        if source_contains_target {
            remove_path_for_replacement(&read_path)?;
        }
        write_worktree_path(
            &path,
            &new_content,
            fp.new_mode.as_deref(),
            source_exec_bit,
            crlf_ctx.as_ref(),
            &path_adjusted,
        )?;

        if !rejected_hunks.is_empty() {
            had_rejects = true;
            let reject_path = PathBuf::from(format!("{path_adjusted}.rej"));
            write_reject_file(&reject_path, fp, &rejected_hunks)?;
        }

        if fp.is_rename && read_path != path && !source_contains_target {
            remove_path_for_replacement(&read_path)?;
            if let Some(parent) = read_path.parent() {
                remove_empty_dirs_up(parent);
            }
        }
    }

    if had_rejects {
        bail!("patch failed");
    }

    // With `--index`, the index is updated in a second phase (`apply_to_index`). Git still writes
    // unmerged entries even when the work tree contains conflict markers; do not exit early.
    if apply_nonzero && !args.index {
        return Err(anyhow::Error::new(ExplicitExit {
            code: 1,
            message: String::new(),
        }));
    }

    Ok(())
}

/// Apply patches to the index only (--cached).
fn apply_to_index(
    patches: &[FilePatch],
    args: &Args,
    ws_mode: ApplyWhitespaceMode,
    patch_input_display: &str,
) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let conflict_style = resolve_apply_conflict_style(&repo);
    let merge_favor_cli = merge_favor_from_apply_args(args);
    let mut apply_nonzero = false;
    let mut index = match repo.load_index() {
        Ok(idx) => idx,
        Err(_) => Index::new(),
    };
    let original_index = index.clone();
    // CWD prefix for subdir apply
    let cwd_prefix = if let Some(ref wt) = repo.work_tree {
        if let Ok(cwd) = std::env::current_dir() {
            if let Ok(rel) = cwd.strip_prefix(wt) {
                let s = rel.to_string_lossy().to_string();
                if s.is_empty() {
                    String::new()
                } else {
                    format!("{s}/")
                }
            } else {
                String::new()
            }
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    for fp in patches {
        let target_path_str = fp
            .target_path()
            .ok_or_else(|| anyhow::anyhow!("patch has no file path"))?;
        let target_raw = adjust_path(target_path_str, args.directory.as_deref());
        let target_adjusted = format!("{cwd_prefix}{target_raw}");
        let source_adjusted = fp
            .source_path()
            .map(|p| adjust_path(p, args.directory.as_deref()))
            .map(|raw| format!("{cwd_prefix}{raw}"))
            .unwrap_or_else(|| target_adjusted.clone());

        if fp.is_deleted {
            index.remove(source_adjusted.as_bytes());
            continue;
        }

        if let Some(binary_patch) = fp.binary_patch.as_ref() {
            let source_index = if source_adjusted != target_adjusted {
                &original_index
            } else {
                &index
            };
            let Some(old_ent) = source_index.get(source_adjusted.as_bytes(), 0) else {
                bail!("{source_adjusted} not found in index");
            };
            if let Some(expected_oid) = fp.old_oid.as_deref() {
                if !ws_mode.whitespace_fix && !args.three_way {
                    let index_hex = old_ent.oid.to_hex();
                    if !index_hex.starts_with(expected_oid) {
                        bail!("patch does not apply");
                    }
                }
            }
            let new_bytes = inflate_binary_payload(&binary_patch.forward_compressed)?;
            if let Some(expected_new) = fp.new_oid.as_deref() {
                let actual = grit_lib::odb::Odb::hash_object_data(ObjectKind::Blob, &new_bytes);
                if !actual.to_hex().starts_with(expected_new) {
                    bail!("patch does not apply");
                }
            }
            let new_oid = repo.odb.write(ObjectKind::Blob, &new_bytes)?;

            let mode = if let Some(m) = fp.new_mode.as_deref() {
                parse_mode(m)
            } else if source_adjusted != target_adjusted {
                original_index
                    .get(source_adjusted.as_bytes(), 0)
                    .map(|entry| entry.mode)
                    .unwrap_or(0o100644)
            } else if let Some(entry) = index.get(source_adjusted.as_bytes(), 0) {
                entry.mode
            } else {
                0o100644
            };

            let entry = grit_lib::index::IndexEntry {
                ctime_sec: 0,
                ctime_nsec: 0,
                mtime_sec: 0,
                mtime_nsec: 0,
                dev: 0,
                ino: 0,
                mode,
                uid: 0,
                gid: 0,
                size: new_bytes.len() as u32,
                oid: new_oid,
                flags: ((target_adjusted.len().min(0xFFF)) as u16) & 0x0FFF,
                flags_extended: None,
                path: target_adjusted.clone().into_bytes(),
            };
            if fp.is_rename && source_adjusted != target_adjusted {
                index.remove(source_adjusted.as_bytes());
            }
            index.add_or_replace(entry);
            continue;
        }

        if fp.new_mode.as_deref() == Some("160000") && !fp.is_deleted {
            if fp.is_new {
                let commit_hash = subproject_commit_from_hunks(fp);
                let oid = grit_lib::objects::ObjectId::from_hex(&commit_hash)?;
                let mode = grit_lib::index::MODE_GITLINK;
                let entry = grit_lib::index::IndexEntry {
                    ctime_sec: 0,
                    ctime_nsec: 0,
                    mtime_sec: 0,
                    mtime_nsec: 0,
                    dev: 0,
                    ino: 0,
                    mode,
                    uid: 0,
                    gid: 0,
                    size: 0,
                    oid,
                    flags: ((target_adjusted.len().min(0xFFF)) as u16) & 0x0FFF,
                    flags_extended: None,
                    path: target_adjusted.into_bytes(),
                };
                index.add_or_replace(entry);
                continue;
            }

            let source_index = if source_adjusted != target_adjusted {
                &original_index
            } else {
                &index
            };
            if fp.old_mode.as_deref() == Some("160000") {
                let Some(old_ent) = source_index.get(source_adjusted.as_bytes(), 0) else {
                    bail!("{source_adjusted} not found in index");
                };
                if old_ent.mode != grit_lib::index::MODE_GITLINK {
                    bail!("{source_adjusted} not found in index");
                }
                if let Some(expected_oid) = fp.old_oid.as_deref() {
                    let index_hex = old_ent.oid.to_hex();
                    if !index_hex.starts_with(expected_oid) {
                        bail!("patch does not apply");
                    }
                }
            } else {
                let Some(entry) = source_index.get(source_adjusted.as_bytes(), 0) else {
                    bail!("{source_adjusted} not found in index");
                };
                if entry.mode == grit_lib::index::MODE_GITLINK {
                    bail!("{source_adjusted} not found in index");
                }
                let obj = repo.odb.read(&entry.oid)?;
                let old_content = String::from_utf8_lossy(&obj.data).into_owned();
                if let Some(expected_oid) = fp.old_oid.as_deref() {
                    verify_old_oid_matches_content(expected_oid, &old_content)?;
                }
            }

            let commit_hash = subproject_commit_from_hunks(fp);
            let oid = grit_lib::objects::ObjectId::from_hex(&commit_hash)?;
            let mode = grit_lib::index::MODE_GITLINK;
            let entry = grit_lib::index::IndexEntry {
                ctime_sec: 0,
                ctime_nsec: 0,
                mtime_sec: 0,
                mtime_nsec: 0,
                dev: 0,
                ino: 0,
                mode,
                uid: 0,
                gid: 0,
                size: 0,
                oid,
                flags: ((target_adjusted.len().min(0xFFF)) as u16) & 0x0FFF,
                flags_extended: None,
                path: target_adjusted.clone().into_bytes(),
            };
            if fp.is_rename && source_adjusted != target_adjusted {
                index.remove(source_adjusted.as_bytes());
            }
            index.add_or_replace(entry);
            continue;
        }

        let path_bytes = target_adjusted.as_bytes();
        // Index preimage: for new-file patches, read the target path when present (add/add --3way).
        let old_content = if fp.is_new {
            if let Some(entry) = index.get(path_bytes, 0) {
                let obj = repo.odb.read(&entry.oid)?;
                String::from_utf8_lossy(&obj.data).into_owned()
            } else {
                String::new()
            }
        } else {
            let source_index = if source_adjusted != target_adjusted {
                &original_index
            } else {
                &index
            };
            if let Some(entry) = source_index.get(source_adjusted.as_bytes(), 0) {
                let obj = repo.odb.read(&entry.oid)?;
                String::from_utf8_lossy(&obj.data).into_owned()
            } else if can_apply_with_empty_preimage(fp) {
                String::new()
            } else {
                bail!("{source_adjusted} not found in index");
            }
        };
        if !fp.is_new {
            if let Some(expected_oid) = fp.old_oid.as_deref() {
                if !ws_mode.whitespace_fix && !args.three_way {
                    verify_old_oid_matches_content(expected_oid, &old_content)?;
                }
            }
        }

        let file_attrs = file_attrs_for_index_apply(&repo, &index, &target_raw);
        let favor = effective_merge_favor_for_path(&file_attrs, merge_favor_cli);
        let direct_to_threeway = fp.is_new && !old_content.is_empty();

        let (new_content, threeway_out) = if fp.hunks.is_empty() {
            (old_content.clone(), None)
        } else if args.three_way {
            if let Some(tw) = try_three_way_merge_blob(
                &repo,
                fp,
                old_content.as_bytes(),
                ws_mode,
                !args.reverse,
                patch_input_display,
                favor,
                conflict_style,
                &file_attrs,
                direct_to_threeway,
            )
            .with_context(|| format!("failed to apply patch to {target_adjusted}"))?
            {
                if tw.conflicted {
                    apply_nonzero = true;
                }
                (
                    String::from_utf8_lossy(&tw.merged_bytes).into_owned(),
                    Some(tw),
                )
            } else {
                let s = apply_hunks(
                    &old_content,
                    fp,
                    ws_mode,
                    !args.reverse,
                    patch_input_display,
                )
                .with_context(|| format!("failed to apply patch to {target_adjusted}"))?;
                (s, None)
            }
        } else {
            let s = apply_hunks(
                &old_content,
                fp,
                ws_mode,
                !args.reverse,
                patch_input_display,
            )
            .with_context(|| format!("failed to apply patch to {target_adjusted}"))?;
            (s, None)
        };

        // Determine mode
        let mode = if let Some(m) = fp.new_mode.as_deref() {
            parse_mode(m)
        } else if source_adjusted != target_adjusted {
            original_index
                .get(source_adjusted.as_bytes(), 0)
                .map(|entry| entry.mode)
                .unwrap_or(0o100644)
        } else if let Some(entry) = index.get(source_adjusted.as_bytes(), 0) {
            entry.mode
        } else {
            0o100644
        };

        if let Some(tw) = threeway_out.as_ref() {
            if tw.conflicted {
                add_conflicted_index_stages(&mut index, path_bytes, mode, &tw.stages);
                if fp.is_rename && source_adjusted != target_adjusted {
                    index.remove(source_adjusted.as_bytes());
                }
                continue;
            }
        }

        // Write new blob to ODB
        let new_oid = repo.odb.write(ObjectKind::Blob, new_content.as_bytes())?;

        // Update index entry
        let size = new_content.len() as u32;
        let entry = grit_lib::index::IndexEntry {
            ctime_sec: 0,
            ctime_nsec: 0,
            mtime_sec: 0,
            mtime_nsec: 0,
            dev: 0,
            ino: 0,
            mode,
            uid: 0,
            gid: 0,
            size,
            oid: new_oid,
            flags: ((target_adjusted.len().min(0xFFF)) as u16) & 0x0FFF,
            flags_extended: None,
            path: target_adjusted.clone().into_bytes(),
        };

        if fp.is_rename && source_adjusted != target_adjusted {
            index.remove(source_adjusted.as_bytes());
        }
        index.add_or_replace(entry);
    }

    repo.write_index(&mut index)?;
    if apply_nonzero {
        if args.three_way {
            let config = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
            if rerere_enabled(&config, &repo.git_dir) {
                let _ = repo_rerere(&repo, RerereAutoupdate::FromConfig);
            }
        }
        return Err(anyhow::Error::new(ExplicitExit {
            code: 1,
            message: String::new(),
        }));
    }
    Ok(())
}

/// Mark patch-created paths as intent-to-add entries in the index.
///
/// This implements `git apply -N/--intent-to-add` for worktree applies.
/// The option is ignored outside a repository and when `--index`/`--cached`
/// modes are active.
fn apply_intent_to_add_entries(patches: &[FilePatch], args: &Args) -> Result<()> {
    if args.cached || args.index {
        return Ok(());
    }

    let repo = match Repository::discover(None) {
        Ok(repo) => repo,
        Err(_) => return Ok(()),
    };
    let mut index = match repo.load_index() {
        Ok(idx) => idx,
        Err(_) => Index::new(),
    };

    let cwd_prefix = if let Some(ref wt) = repo.work_tree {
        if let Ok(cwd) = std::env::current_dir() {
            if let Ok(rel) = cwd.strip_prefix(wt) {
                let s = rel.to_string_lossy().to_string();
                if s.is_empty() {
                    String::new()
                } else {
                    format!("{s}/")
                }
            } else {
                String::new()
            }
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    for fp in patches {
        if !fp.is_new {
            continue;
        }
        let Some(target_path) = fp.target_path() else {
            continue;
        };
        let target_raw = adjust_path(target_path, args.directory.as_deref());
        let target_adjusted = format!("{cwd_prefix}{target_raw}");
        if target_adjusted.is_empty() {
            continue;
        }

        let mode = fp.new_mode.as_deref().map(parse_mode).unwrap_or(0o100644);
        let mut entry = IndexEntry {
            ctime_sec: 0,
            ctime_nsec: 0,
            mtime_sec: 0,
            mtime_nsec: 0,
            dev: 0,
            ino: 0,
            mode,
            uid: 0,
            gid: 0,
            size: 0,
            oid: grit_lib::diff::empty_blob_oid(),
            flags: ((target_adjusted.len().min(0xFFF)) as u16) & 0x0FFF,
            flags_extended: None,
            path: target_adjusted.into_bytes(),
        };
        entry.set_intent_to_add(true);
        index.add_or_replace(entry);
    }

    repo.write_index(&mut index)?;
    Ok(())
}

/// Check if patches apply cleanly without modifying anything.
fn check_patches(
    patches: &[FilePatch],
    args: &Args,
    ws_mode: ApplyWhitespaceMode,
    patch_input_display: &str,
) -> Result<()> {
    let crlf_ctx = ApplyCrlfContext::load();
    for fp in patches {
        let path_str = fp
            .effective_path()
            .ok_or_else(|| anyhow::anyhow!("patch has no file path"))?;
        let path_adjusted = adjust_path(path_str, args.directory.as_deref());
        let path = PathBuf::from(&path_adjusted);

        if fp.is_deleted {
            if !path.exists() {
                bail!("{}: does not exist", path.display());
            }
            continue;
        }

        if fp.is_new {
            if path.exists() {
                bail!("{}: already exists", path.display());
            }
            // Verify hunks apply to empty content
            apply_hunks("", fp, ws_mode, !args.reverse, patch_input_display)?;
            continue;
        }

        let read_path = fp
            .source_path()
            .map(|p| PathBuf::from(adjust_path(p, args.directory.as_deref())))
            .unwrap_or_else(|| path.clone());
        let source_adjusted = fp
            .source_path()
            .map(|p| adjust_path(p, args.directory.as_deref()))
            .unwrap_or_else(|| path_adjusted.clone());
        let old_content = match &crlf_ctx {
            Some(ctx) => {
                if !read_path.exists() && can_apply_with_empty_preimage(fp) {
                    String::new()
                } else {
                    ctx.normalized_text(&read_path, &source_adjusted)?
                }
            }
            None => match fs::read_to_string(&read_path) {
                Ok(content) => content,
                Err(err)
                    if err.kind() == std::io::ErrorKind::NotFound
                        && can_apply_with_empty_preimage(fp) =>
                {
                    String::new()
                }
                Err(err) => {
                    return Err(err)
                        .with_context(|| format!("failed to read {}", read_path.display()))
                }
            },
        };
        if let Some(expected_oid) = fp.old_oid.as_deref() {
            if !ws_mode.whitespace_fix {
                verify_old_oid_matches_content(expected_oid, &old_content)?;
            }
        }
        apply_hunks(
            &old_content,
            fp,
            ws_mode,
            !args.reverse,
            patch_input_display,
        )?;
    }

    Ok(())
}

/// Parse an octal mode string like "100644" to u32.
fn parse_mode(s: &str) -> u32 {
    u32::from_str_radix(s, 8).unwrap_or(0o100644)
}
