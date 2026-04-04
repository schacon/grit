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

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::index::Index;
use grit_lib::objects::ObjectKind;
use grit_lib::repo::Repository;
use std::fs;
use std::io::{self, Read, Write};
use std::path::PathBuf;

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

    /// Check if the patch applies cleanly without modifying anything.
    #[arg(long)]
    pub check: bool,

    /// Apply to both the working tree and the index.
    #[arg(long)]
    pub index: bool,

    /// Apply the patch in reverse.
    #[arg(short = 'R', long = "reverse")]
    pub reverse: bool,

    /// Strip N leading path components from diff paths (default: 1).
    #[arg(short = 'p', default_value = "1")]
    pub strip: usize,

    /// Prepend directory to all file paths in the patch.
    #[arg(long = "directory", value_name = "DIR")]
    pub directory: Option<String>,

    /// Recount hunk line counts (for corrupted patches).
    #[arg(long = "recount")]
    pub recount: bool,

    /// Apply with unidiff-zero context.
    #[arg(long = "unidiff-zero")]
    pub unidiff_zero: bool,

    /// Allow binary patches.
    #[arg(long = "allow-binary-replacement")]
    pub allow_binary_replacement: bool,

    /// Verbose output.
    #[arg(short = 'v', long = "verbose")]
    pub verbose: bool,

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
    /// Number of lines in the new side.
    new_count: usize,
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

/// Represents one file in a unified diff.
#[derive(Debug, Clone)]
struct FilePatch {
    /// Path on the old side (None for new files).
    old_path: Option<String>,
    /// Path on the new side (None for deleted files).
    new_path: Option<String>,
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
    /// Hunks to apply.
    hunks: Vec<Hunk>,
}

impl FilePatch {
    /// Effective path for the file.
    /// For deletions, use old_path (new is /dev/null).
    /// For additions, use new_path (old is /dev/null).
    /// Otherwise prefer new_path.
    fn effective_path(&self) -> Option<&str> {
        if self.is_deleted {
            return self.old_path.as_deref().filter(|p| *p != "/dev/null")
                .or(self.new_path.as_deref().filter(|p| *p != "/dev/null"));
        }
        if self.is_new {
            return self.new_path.as_deref().filter(|p| *p != "/dev/null")
                .or(self.old_path.as_deref().filter(|p| *p != "/dev/null"));
        }
        self.new_path.as_deref().filter(|p| *p != "/dev/null")
            .or(self.old_path.as_deref().filter(|p| *p != "/dev/null"))
    }
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// Parse a unified diff into a list of `FilePatch` entries.
fn parse_patch(input: &str) -> Result<Vec<FilePatch>> {
    let lines: Vec<&str> = input.lines().collect();
    let mut patches = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        // Look for "diff --git" header or a bare ---/+++ pair.
        if lines[i].starts_with("diff --git ") {
            let mut fp = FilePatch {
                old_path: None,
                new_path: None,
                old_mode: None,
                new_mode: None,
                is_new: false,
                is_deleted: false,
                is_rename: false,
                is_copy: false,
                similarity_index: None,
                dissimilarity_index: None,
                hunks: Vec::new(),
            };

            // Parse "diff --git a/foo b/foo"
            let rest = &lines[i]["diff --git ".len()..];
            if let Some((a, b)) = split_diff_git_paths(rest) {
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
                    fp.old_path = Some(val.to_string());
                } else if let Some(val) = line.strip_prefix("rename to ") {
                    fp.is_rename = true;
                    fp.new_path = Some(val.to_string());
                } else if let Some(val) = line.strip_prefix("copy from ") {
                    fp.is_copy = true;
                    fp.old_path = Some(val.to_string());
                } else if let Some(val) = line.strip_prefix("copy to ") {
                    fp.is_copy = true;
                    fp.new_path = Some(val.to_string());
                } else if let Some(val) = line.strip_prefix("similarity index ") {
                    fp.similarity_index = val.trim_end_matches('%').parse().ok();
                } else if let Some(val) = line.strip_prefix("dissimilarity index ") {
                    fp.dissimilarity_index = val.trim_end_matches('%').parse().ok();
                }
                // skip other extended headers (index, etc.)
                i += 1;
            }

            // Parse ---/+++ headers if present
            if i < lines.len() && lines[i].starts_with("--- ") {
                let old_p = &lines[i]["--- ".len()..];
                fp.old_path = Some(old_p.to_string());
                i += 1;
                if i < lines.len() && lines[i].starts_with("+++ ") {
                    let new_p = &lines[i]["+++ ".len()..];
                    fp.new_path = Some(new_p.to_string());
                    i += 1;
                }
            }

            // Parse hunks
            while i < lines.len() && lines[i].starts_with("@@ ") {
                let (hunk, next_i) = parse_hunk(&lines, i)?;
                fp.hunks.push(hunk);
                i = next_i;
            }

            patches.push(fp);
        } else if lines[i].starts_with("--- ") && i + 1 < lines.len() && lines[i + 1].starts_with("+++ ") {
            // Bare unified diff without "diff --git" header
            let mut fp = FilePatch {
                old_path: None,
                new_path: None,
                old_mode: None,
                new_mode: None,
                is_new: false,
                is_deleted: false,
                is_rename: false,
                is_copy: false,
                similarity_index: None,
                dissimilarity_index: None,
                hunks: Vec::new(),
            };

            let old_p = &lines[i]["--- ".len()..];
            fp.old_path = Some(old_p.to_string());
            i += 1;
            let new_p = &lines[i]["+++ ".len()..];
            fp.new_path = Some(new_p.to_string());
            i += 1;

            // Check for /dev/null
            if fp.old_path.as_deref() == Some("/dev/null") {
                fp.is_new = true;
            }
            if fp.new_path.as_deref() == Some("/dev/null") {
                fp.is_deleted = true;
            }

            // Parse hunks
            while i < lines.len() && lines[i].starts_with("@@ ") {
                let (hunk, next_i) = parse_hunk(&lines, i)?;
                fp.hunks.push(hunk);
                i = next_i;
            }

            patches.push(fp);
        } else {
            i += 1;
        }
    }

    Ok(patches)
}

/// Split "a/path b/path" from `diff --git` line. Handles spaces in paths
/// by scanning for ` b/` boundary. Falls back if that fails.
fn split_diff_git_paths(s: &str) -> Option<(String, String)> {
    // Keep raw paths (with a/ b/ prefix) so -p<n> stripping works correctly.
    if let Some(pos) = s.find(" b/") {
        let a = &s[..pos];
        let b = &s[pos + 1..];
        return Some((a.to_string(), b.to_string()));
    }
    // Also handle /dev/null cases
    if s.starts_with("a/") {
        if let Some(pos) = s.find(" /dev/null") {
            let a = &s[..pos];
            return Some((a.to_string(), "/dev/null".to_string()));
        }
    }
    if let Some(b) = s.strip_prefix("/dev/null ") {
        return Some(("/dev/null".to_string(), b.to_string()));
    }
    None
}

/// Parse a single hunk starting at line `i` (which should be an `@@` line).
fn parse_hunk(lines: &[&str], start: usize) -> Result<(Hunk, usize)> {
    let header = lines[start];
    let (old_start, old_count, new_start, new_count) = parse_hunk_header(header)
        .with_context(|| format!("invalid hunk header: {header}"))?;

    let mut hunk = Hunk {
        old_start,
        old_count,
        new_start,
        new_count,
        lines: Vec::new(),
    };

    let mut i = start + 1;
    while i < lines.len() {
        let line = lines[i];
        if line.starts_with("@@ ") || line.starts_with("diff --git ") {
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

/// Apply -p and --directory transforms to a path.
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

fn adjust_path(path: &str, strip: usize, directory: Option<&str>) -> String {
    if path == "/dev/null" {
        return path.to_string();
    }
    let stripped = strip_components(path, strip);
    if let Some(dir) = directory {
        format!("{dir}/{stripped}")
    } else {
        stripped
    }
}

// ---------------------------------------------------------------------------
// Reverse
// ---------------------------------------------------------------------------

/// Reverse a patch: swap old/new paths, swap +/- in hunks.
fn reverse_patches(patches: &mut [FilePatch]) {
    for fp in patches.iter_mut() {
        std::mem::swap(&mut fp.old_path, &mut fp.new_path);
        std::mem::swap(&mut fp.old_mode, &mut fp.new_mode);
        std::mem::swap(&mut fp.is_new, &mut fp.is_deleted);

        for hunk in &mut fp.hunks {
            std::mem::swap(&mut hunk.old_start, &mut hunk.new_start);
            std::mem::swap(&mut hunk.old_count, &mut hunk.new_count);
            let new_lines: Vec<HunkLine> = hunk.lines.drain(..).map(|hl| match hl {
                HunkLine::Add(s) => HunkLine::Remove(s),
                HunkLine::Remove(s) => HunkLine::Add(s),
                other => other,
            }).collect();
            hunk.lines = new_lines;
        }
    }
}

// ---------------------------------------------------------------------------
// Applying hunks to content
// ---------------------------------------------------------------------------

/// Apply hunks to file content (a list of lines). Returns new content.
fn apply_hunks(old_content: &str, hunks: &[Hunk]) -> Result<String> {
    // Split into lines, keeping track of trailing newline
    let has_trailing_newline = old_content.is_empty() || old_content.ends_with('\n');
    let old_lines: Vec<&str> = if old_content.is_empty() {
        Vec::new()
    } else {
        old_content.lines().collect()
    };

    let mut result: Vec<String> = Vec::new();
    let mut old_idx: usize = 0; // 0-based index into old_lines

    for hunk in hunks {
        let hunk_start = if hunk.old_start == 0 { 0 } else { hunk.old_start - 1 };

        // Copy lines before this hunk
        while old_idx < hunk_start && old_idx < old_lines.len() {
            result.push(old_lines[old_idx].to_string());
            old_idx += 1;
        }

        // Apply hunk
        let mut remove_no_newline = false;
        let mut add_no_newline = false;
        for hl in &hunk.lines {
            match hl {
                HunkLine::Context(s) => {
                    if old_idx < old_lines.len() {
                        // Verify context matches
                        if old_lines[old_idx] != s.as_str() {
                            bail!(
                                "context mismatch at line {}: expected {:?}, got {:?}",
                                old_idx + 1,
                                s,
                                old_lines[old_idx]
                            );
                        }
                        old_idx += 1;
                    }
                    result.push(s.clone());
                }
                HunkLine::Remove(s) => {
                    if old_idx < old_lines.len() {
                        if old_lines[old_idx] != s.as_str() {
                            bail!(
                                "remove mismatch at line {}: expected {:?}, got {:?}",
                                old_idx + 1,
                                s,
                                old_lines[old_idx]
                            );
                        }
                        old_idx += 1;
                    }
                    remove_no_newline = false;
                }
                HunkLine::Add(s) => {
                    result.push(s.clone());
                    add_no_newline = false;
                }
                HunkLine::NoNewline => {
                    // This applies to whichever side the previous line was on.
                    // We track it but the main effect is on trailing newline.
                    remove_no_newline = true;
                    add_no_newline = true;
                }
            }
        }
        let _ = (remove_no_newline, add_no_newline);
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
    let ends_no_newline = hunks.last().map_or(false, |h| {
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
    if !ends_no_newline && (has_trailing_newline || !hunks.is_empty()) {
        out.push('\n');
    }

    Ok(out)
}

// ---------------------------------------------------------------------------
// Stat / numstat / summary output
// ---------------------------------------------------------------------------

fn show_stat(patches: &[FilePatch], strip: usize, directory: Option<&str>) {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    let mut total_add = 0usize;
    let mut total_del = 0usize;
    let mut max_path_len = 0usize;
    let mut entries: Vec<(String, usize, usize)> = Vec::new();

    for fp in patches {
        let path = fp
            .effective_path()
            .map(|p| adjust_path(p, strip, directory))
            .unwrap_or_else(|| "(unknown)".to_string());
        let (add, del) = count_hunk_changes(&fp.hunks);
        if path.len() > max_path_len {
            max_path_len = path.len();
        }
        total_add += add;
        total_del += del;
        entries.push((path, add, del));
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

fn show_numstat(patches: &[FilePatch], strip: usize, directory: Option<&str>) {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    for fp in patches {
        let path = fp
            .effective_path()
            .map(|p| adjust_path(p, strip, directory))
            .unwrap_or_else(|| "(unknown)".to_string());
        let (add, del) = count_hunk_changes(&fp.hunks);
        let _ = writeln!(out, "{add}\t{del}\t{path}");
    }
}

fn show_summary(patches: &[FilePatch], strip: usize, directory: Option<&str>) {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    for fp in patches {
        let path = fp
            .effective_path()
            .map(|p| adjust_path(p, strip, directory))
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
                .map(|p| adjust_path(p, strip, directory))
                .unwrap_or_else(|| "(unknown)".to_string());
            let kind = if fp.is_copy { "copy" } else { "rename" };
            let pct = fp.similarity_index.unwrap_or(100);
            let compact = compact_rename_path(&old, &path);
            let _ = writeln!(out, " {kind} {compact} ({pct}%)");
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
    format!(
        "{}{}",
        "+".repeat(plus_width),
        "-".repeat(minus_width)
    )
}

// ---------------------------------------------------------------------------
// Main run
// ---------------------------------------------------------------------------

pub fn run(args: Args) -> Result<()> {
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
                fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?
            };
            buf.push_str(&content);
            if !content.ends_with('\n') {
                buf.push('\n');
            }
        }
        buf
    };

    let mut patches = parse_patch(&input)?;

    if args.reverse {
        reverse_patches(&mut patches);
    }

    // Info-only modes
    let info_only = args.stat || args.numstat || args.summary;
    if args.stat {
        show_stat(&patches, args.strip, args.directory.as_deref());
    }
    if args.numstat {
        show_numstat(&patches, args.strip, args.directory.as_deref());
    }
    if args.summary {
        show_summary(&patches, args.strip, args.directory.as_deref());
    }
    if info_only {
        return Ok(());
    }

    // For --cached, we need a repository and index.
    // For working tree apply, we may or may not be in a repo.
    if args.cached {
        apply_to_index(&patches, &args)?;
    } else if args.check {
        check_patches(&patches, &args)?;
    } else if args.index {
        apply_to_worktree(&patches, &args)?;
        apply_to_index(&patches, &args)?;
    } else {
        apply_to_worktree(&patches, &args)?;
    }

    Ok(())
}

/// Apply patches to the working tree.
fn apply_to_worktree(patches: &[FilePatch], args: &Args) -> Result<()> {
    for fp in patches {
        let path_str = fp
            .effective_path()
            .ok_or_else(|| anyhow::anyhow!("patch has no file path"))?;
        let path = PathBuf::from(adjust_path(path_str, args.strip, args.directory.as_deref()));

        if fp.is_deleted {
            // Delete the file
            if path.exists() {
                fs::remove_file(&path)
                    .with_context(|| format!("failed to remove {}", path.display()))?;
            }
            continue;
        }

        if fp.is_new {
            // Create new file
            if let Some(parent) = path.parent() {
                if !parent.as_os_str().is_empty() && !parent.exists() {
                    fs::create_dir_all(parent)?;
                }
            }
            let content = apply_hunks("", &fp.hunks)
                .with_context(|| format!("failed to apply hunks for new file {}", path.display()))?;
            fs::write(&path, content.as_bytes())
                .with_context(|| format!("failed to write {}", path.display()))?;

            // Set executable if mode is 100755
            #[cfg(unix)]
            if fp
                .new_mode
                .as_deref()
                .map_or(false, |m| m == "100755")
            {
                use std::os::unix::fs::PermissionsExt;
                let perms = fs::Permissions::from_mode(0o755);
                fs::set_permissions(&path, perms)?;
            }
            continue;
        }

        // Modify existing file — read from old_path if it differs from new_path
        let read_path = if let Some(old_p) = fp.old_path.as_deref().filter(|p| *p != "/dev/null") {
            let adj = PathBuf::from(adjust_path(old_p, args.strip, args.directory.as_deref()));
            if adj != path && adj.exists() {
                adj
            } else {
                path.clone()
            }
        } else {
            path.clone()
        };
        let old_content = fs::read_to_string(&read_path)
            .with_context(|| format!("failed to read {}", read_path.display()))?;

        if fp.hunks.is_empty() {
            // Mode-only change
            #[cfg(unix)]
            if let Some(mode) = fp.new_mode.as_deref() {
                use std::os::unix::fs::PermissionsExt;
                let perm = if mode == "100755" { 0o755 } else { 0o644 };
                fs::set_permissions(&path, fs::Permissions::from_mode(perm))?;
            }
            continue;
        }

        let new_content = apply_hunks(&old_content, &fp.hunks)
            .with_context(|| format!("failed to apply patch to {}", path.display()))?;
        fs::write(&path, new_content.as_bytes())
            .with_context(|| format!("failed to write {}", path.display()))?;
    }

    Ok(())
}

/// Apply patches to the index only (--cached).
fn apply_to_index(patches: &[FilePatch], args: &Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let mut index = match Index::load(&repo.index_path()) {
        Ok(idx) => idx,
        Err(_) => Index::new(),
    };

    for fp in patches {
        let path_str = fp
            .effective_path()
            .ok_or_else(|| anyhow::anyhow!("patch has no file path"))?;
        let adjusted = adjust_path(path_str, args.strip, args.directory.as_deref());

        if fp.is_deleted {
            index.remove(adjusted.as_bytes());
            continue;
        }

        // Get old content from index (or empty for new files)
        let old_content = if fp.is_new {
            String::new()
        } else {
            let entry = index
                .get(adjusted.as_bytes(), 0)
                .ok_or_else(|| anyhow::anyhow!("{adjusted} not found in index"))?;
            let obj = repo.odb.read(&entry.oid)?;
            String::from_utf8_lossy(&obj.data).into_owned()
        };

        let new_content = if fp.hunks.is_empty() {
            old_content.clone()
        } else {
            apply_hunks(&old_content, &fp.hunks)
                .with_context(|| format!("failed to apply patch to {adjusted}"))?
        };

        // Write new blob to ODB
        let new_oid = repo.odb.write(ObjectKind::Blob, new_content.as_bytes())?;

        // Determine mode
        let mode = if let Some(m) = fp.new_mode.as_deref() {
            parse_mode(m)
        } else if let Some(entry) = index.get(adjusted.as_bytes(), 0) {
            entry.mode
        } else {
            0o100644
        };

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
            flags: ((adjusted.len().min(0xFFF)) as u16) & 0x0FFF,
            flags_extended: None,
            path: adjusted.into_bytes(),
        };
        index.add_or_replace(entry);
    }

    index.write(&repo.index_path())?;
    Ok(())
}

/// Check if patches apply cleanly without modifying anything.
fn check_patches(patches: &[FilePatch], args: &Args) -> Result<()> {
    for fp in patches {
        let path_str = fp
            .effective_path()
            .ok_or_else(|| anyhow::anyhow!("patch has no file path"))?;
        let path = PathBuf::from(adjust_path(path_str, args.strip, args.directory.as_deref()));

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
            apply_hunks("", &fp.hunks)
                .with_context(|| format!("patch does not apply cleanly to new file {}", path.display()))?;
            continue;
        }

        let old_content = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        apply_hunks(&old_content, &fp.hunks)
            .with_context(|| format!("patch does not apply cleanly to {}", path.display()))?;
    }

    Ok(())
}

/// Parse an octal mode string like "100644" to u32.
fn parse_mode(s: &str) -> u32 {
    u32::from_str_radix(s, 8).unwrap_or(0o100644)
}
