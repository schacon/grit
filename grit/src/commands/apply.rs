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
use grit_lib::index::{Index, IndexEntry};
use grit_lib::objects::ObjectKind;
use grit_lib::repo::Repository;
use grit_lib::rev_parse::resolve_revision;
use std::collections::HashMap;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

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

#[derive(Debug, Clone, Copy, Default)]
struct ApplyWhitespaceMode {
    whitespace_fix: bool,
    ignore_space_change: bool,
    inaccurate_eof: bool,
    tab_width: usize,
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

fn config_tab_width() -> usize {
    let Ok(repo) = Repository::discover(None) else {
        return 8;
    };
    let config = grit_lib::config::ConfigSet::load(Some(&repo.git_dir), true)
        .unwrap_or_else(|_| grit_lib::config::ConfigSet::new());
    let Some(value) = config.get("core.whitespace") else {
        return 8;
    };

    value
        .split(',')
        .find_map(|part| {
            part.trim()
                .strip_prefix("tabwidth=")
                .and_then(|n| n.parse::<usize>().ok())
        })
        .filter(|w| *w > 0)
        .unwrap_or(8)
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
    let whitespace_fix = if whitespace_option_was_explicitly_set() {
        args.whitespace.eq_ignore_ascii_case("fix")
    } else if args.whitespace.eq_ignore_ascii_case("warn") {
        config_whitespace_fix()
    } else {
        args.whitespace.eq_ignore_ascii_case("fix")
    };
    let ignore_space_change = if args.no_ignore_whitespace {
        false
    } else if args.ignore_whitespace || args.ignore_space_change {
        true
    } else {
        config_ignore_space_change()
    };
    ApplyWhitespaceMode {
        whitespace_fix,
        ignore_space_change,
        inaccurate_eof: args.inaccurate_eof,
        tab_width: config_tab_width(),
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
            };

            // Parse "diff --git a/foo b/foo"
            let rest = &lines[i]["diff --git ".len()..];
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

            // Parse ---/+++ headers if present
            if i < lines.len() && lines[i].starts_with("--- ") {
                let old_p = &lines[i]["--- ".len()..];
                fp.old_path = Some(old_p.to_string());
                fp.saw_old_header = true;
                i += 1;
                if i < lines.len() && lines[i].starts_with("+++ ") {
                    let new_p = &lines[i]["+++ ".len()..];
                    fp.new_path = Some(new_p.to_string());
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

            patches.push(fp);
        } else if lines[i].starts_with("--- ")
            && i + 1 < lines.len()
            && lines[i + 1].starts_with("+++ ")
        {
            // Bare unified diff without "diff --git" header
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
            };

            let old_p = &lines[i]["--- ".len()..];
            fp.old_path = Some(old_p.to_string());
            fp.saw_old_header = true;
            i += 1;
            let new_p = &lines[i]["+++ ".len()..];
            fp.new_path = Some(new_p.to_string());
            fp.saw_new_header = true;
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
    let encoded = chars.as_str();
    let mut decoded = decode_base85_payload(encoded)?;
    let decode_len = expected_len.min(decoded.len());
    if decode_len == 0 {
        bail!(
            "binary patch payload decode short read: expected {expected_len}, got {}",
            decoded.len()
        );
    }
    decoded.truncate(decode_len);
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

fn decode_base85_payload(encoded: &str) -> Result<Vec<u8>> {
    const CHARS: &[u8] =
        b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz!#$%&()*+-;<=>?@^_`{|}~";
    let mut table = [255u8; 256];
    for (idx, &c) in CHARS.iter().enumerate() {
        table[c as usize] = idx as u8;
    }

    let bytes = encoded.as_bytes();
    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos < bytes.len() {
        let group_end = (pos + 5).min(bytes.len());
        let group = &bytes[pos..group_end];
        let group_len = group.len();
        if group_len < 2 {
            bail!("invalid base85 payload length");
        }

        let mut acc: u32 = 0;
        for &b in group {
            let v = table[b as usize];
            if v == 255 {
                bail!("invalid base85 digit in binary patch");
            }
            acc = acc
                .checked_mul(85)
                .and_then(|n| n.checked_add(v as u32))
                .ok_or_else(|| anyhow::anyhow!("base85 overflow"))?;
        }
        for _ in group_len..5 {
            acc = acc
                .checked_mul(85)
                .and_then(|n| n.checked_add(84))
                .ok_or_else(|| anyhow::anyhow!("base85 overflow"))?;
        }

        let raw = acc.to_be_bytes();
        let produced = if group_len == 5 { 4 } else { group_len - 1 };
        out.extend_from_slice(&raw[..produced]);
        pos += group_len;
    }

    Ok(out)
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
    if !out.is_empty() && !out.ends_with(b"\n") {
        out.push(b'\n');
    }
    Ok(out)
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
    let (old_start, old_count, new_start, new_count) =
        parse_hunk_header(header).with_context(|| format!("invalid hunk header: {header}"))?;

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

fn path_has_symlink_prefix(path: &str, symlink_overlay: &HashMap<String, bool>) -> Result<()> {
    let components: Vec<&str> = path.split('/').filter(|c| !c.is_empty()).collect();
    if components.len() <= 1 {
        return Ok(());
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
            bail!("{path}: beyond a symbolic link");
        }
    }
    Ok(())
}

fn verify_patch_paths_not_beyond_symlink(patches: &[FilePatch], args: &Args) -> Result<()> {
    let mut symlink_overlay: HashMap<String, bool> = HashMap::new();

    for fp in patches {
        if let Some(source) = fp.source_path() {
            let source_adjusted = adjust_path(source, args.strip, args.directory.as_deref());
            if !source_adjusted.is_empty() {
                path_has_symlink_prefix(&source_adjusted, &symlink_overlay)?;
            }
        }
        if let Some(target) = fp.target_path() {
            let target_adjusted = adjust_path(target, args.strip, args.directory.as_deref());
            if !target_adjusted.is_empty() {
                path_has_symlink_prefix(&target_adjusted, &symlink_overlay)?;
            }
        }

        let source_adjusted = fp
            .source_path()
            .map(|p| adjust_path(p, args.strip, args.directory.as_deref()))
            .unwrap_or_default();
        let target_adjusted = fp
            .target_path()
            .map(|p| adjust_path(p, args.strip, args.directory.as_deref()))
            .unwrap_or_default();

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

fn validate_patch_headers(patches: &[FilePatch]) -> Result<()> {
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
                if old != "/dev/null" && diff_old != old {
                    bail!("inconsistent old filename");
                }
            }
        }

        if fp.saw_new_header {
            if let (Some(diff_new), Some(new)) =
                (fp.diff_new_path.as_deref(), fp.new_path.as_deref())
            {
                if new != "/dev/null" && diff_new != new {
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
) -> usize {
    if hunk.old_count == 0 {
        return nominal.min(old_lines.len());
    }

    // Check if the nominal position matches first.
    if match_hunk_old_side_at(
        old_lines,
        hunk,
        nominal,
        ws_mode,
        allow_unidiff_zero_fallback,
    ) {
        return nominal;
    }

    // Scan outward from nominal.
    let max_scan = old_lines.len();
    for delta in 1..=max_scan {
        if nominal >= delta
            && match_hunk_old_side_at(
                old_lines,
                hunk,
                nominal - delta,
                ws_mode,
                allow_unidiff_zero_fallback,
            )
        {
            return nominal - delta;
        }
        if nominal + delta <= old_lines.len()
            && match_hunk_old_side_at(
                old_lines,
                hunk,
                nominal + delta,
                ws_mode,
                allow_unidiff_zero_fallback,
            )
        {
            return nominal + delta;
        }
    }

    // No match found — return nominal and let the hunk application fail.
    nominal.min(old_lines.len())
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
    if start > old_len {
        bail!("patch does not apply");
    }
    Ok(start)
}

/// Check whether hunk old-side lines match at the given candidate start.
fn match_hunk_old_side_at(
    old_lines: &[&str],
    hunk: &Hunk,
    start: usize,
    ws_mode: ApplyWhitespaceMode,
    allow_unidiff_zero_fallback: bool,
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

    if start + old_side.len() <= old_lines.len()
        && old_side
            .iter()
            .zip(&old_lines[start..start + old_side.len()])
            .all(|(expected, actual)| lines_equal(expected, actual, ws_mode))
    {
        return true;
    }

    allow_unidiff_zero_fallback
        && is_subsequence_match(&old_lines[start.min(old_lines.len())..], &old_side, ws_mode)
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

fn apply_hunks(old_content: &str, hunks: &[Hunk], ws_mode: ApplyWhitespaceMode) -> Result<String> {
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

    for hunk in hunks {
        let nominal_start = nominal_hunk_start(hunk, old_lines.len())? as isize;
        let hunk_start = (nominal_start + offset).max(0) as usize;

        // If context at hunk_start doesn't match, scan nearby to find it
        let actual_start = find_hunk_start(&old_lines, hunk, hunk_start, ws_mode, false);

        // Copy lines before this hunk
        while old_idx < actual_start && old_idx < old_lines.len() {
            result.push(old_lines[old_idx].to_string());
            old_idx += 1;
        }

        // Update offset based on where we actually found the hunk
        if actual_start != hunk_start {
            offset += actual_start as isize - hunk_start as isize;
        }

        // Apply hunk
        let mut remove_no_newline = false;
        let mut add_no_newline = false;
        for hl in &hunk.lines {
            match hl {
                HunkLine::Context(s) => {
                    if old_idx >= old_lines.len() {
                        bail!(
                            "context mismatch at line {}: expected {:?}, got EOF",
                            old_idx + 1,
                            s
                        );
                    }
                    let actual_line = old_lines[old_idx];
                    // Verify context matches
                    if !lines_equal(s, actual_line, ws_mode) {
                        bail!(
                            "context mismatch at line {}: expected {:?}, got {:?}",
                            old_idx + 1,
                            s,
                            actual_line
                        );
                    }
                    old_idx += 1;
                    result.push(actual_line.to_string());
                }
                HunkLine::Remove(s) => {
                    if old_idx >= old_lines.len() {
                        bail!(
                            "remove mismatch at line {}: expected {:?}, got EOF",
                            old_idx + 1,
                            s
                        );
                    }
                    if !lines_equal(s, old_lines[old_idx], ws_mode) {
                        bail!(
                            "remove mismatch at line {}: expected {:?}, got {:?}",
                            old_idx + 1,
                            s,
                            old_lines[old_idx]
                        );
                    }
                    old_idx += 1;
                    remove_no_newline = false;
                }
                HunkLine::Add(s) => {
                    if ws_mode.whitespace_fix {
                        result.push(normalize_ws_fix_line(s, ws_mode.tab_width));
                    } else {
                        result.push(s.clone());
                    }
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

    Ok(out)
}

/// Apply hunks while collecting failed hunks for `--reject` mode.
///
/// Returns `(new_content, rejected_hunks)`.
fn apply_hunks_with_reject(
    old_content: &str,
    hunks: &[Hunk],
    ws_mode: ApplyWhitespaceMode,
) -> Result<(String, Vec<Hunk>)> {
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

    for hunk in hunks {
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
            false,
        );

        let mut idx = actual_start;
        let mut replacement: Vec<String> = Vec::new();
        let mut failed = false;

        for hl in &hunk.lines {
            match hl {
                HunkLine::Context(s) => {
                    if idx >= lines.len() || !lines_equal(s, &lines[idx], ws_mode) {
                        failed = true;
                        break;
                    }
                    let actual_line = lines[idx].clone();
                    idx += 1;
                    replacement.push(actual_line);
                }
                HunkLine::Remove(s) => {
                    if idx >= lines.len() || !lines_equal(s, &lines[idx], ws_mode) {
                        failed = true;
                        break;
                    }
                    idx += 1;
                }
                HunkLine::Add(s) => {
                    if ws_mode.whitespace_fix {
                        replacement.push(normalize_ws_fix_line(s, ws_mode.tab_width));
                    } else {
                        replacement.push(s.clone());
                    }
                }
                HunkLine::NoNewline => {}
            }
        }

        if failed {
            rejected.push(hunk.clone());
            continue;
        }

        let removed_count = idx.saturating_sub(actual_start);
        lines.splice(actual_start..idx, replacement.iter().cloned());
        offset += replacement.len() as isize - removed_count as isize;
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

    let mut patches = parse_patch(&input)?;
    validate_patch_headers(&patches)?;

    if args.reverse {
        reverse_patches(&mut patches);
    }
    if patches.is_empty() && !args.allow_empty {
        bail!("No valid patches in input");
    }

    // Info-only modes unless explicitly overridden by --apply.
    let info_only = (args.stat || args.numstat || args.summary) && !args.apply;
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

    if let Some(path) = &args.build_fake_ancestor {
        build_fake_ancestor_file(&patches, &args, path)?;
        return Ok(());
    }
    verify_patch_paths_not_beyond_symlink(&patches, &args)?;
    let ws_mode = resolve_apply_whitespace_mode(&args);

    // For --cached, we need a repository and index.
    // For working tree apply, we may or may not be in a repo.
    if args.cached {
        apply_to_index(&patches, &args, ws_mode)?;
    } else {
        if args.check {
            check_patches(&patches, &args, ws_mode)?;
            if !args.apply {
                return Ok(());
            }
        }

        if args.index {
            verify_worktree_matches_index(&patches, &args)?;
            apply_to_worktree(&patches, &args, ws_mode)?;
            apply_to_index(&patches, &args, ws_mode)?;
        } else {
            apply_to_worktree(&patches, &args, ws_mode)?;
            if args.intent_to_add {
                apply_intent_to_add_entries(&patches, &args)?;
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
    let current_index = Index::load(&repo.index_path()).unwrap_or_else(|_| Index::new());
    let mut fake = Index::new();

    for fp in patches {
        let Some(raw_old_path) = fp.old_path.as_deref() else {
            continue;
        };
        if raw_old_path == "/dev/null" {
            continue;
        }
        let adjusted = adjust_path(raw_old_path, args.strip, args.directory.as_deref());
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

/// Apply patches to the working tree.
/// Verify that working tree files match the index (required for --index mode).
fn verify_worktree_matches_index(patches: &[FilePatch], args: &Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let index = match Index::load(&repo.index_path()) {
        Ok(idx) => idx,
        Err(_) => return Ok(()),
    };

    for fp in patches {
        if fp.is_new {
            if let Some(target) = fp.target_path() {
                let adjusted = adjust_path(target, args.strip, args.directory.as_deref());
                if let Some(entry) = index.get(adjusted.as_bytes(), 0) {
                    let path = PathBuf::from(&adjusted);
                    if !path.exists() {
                        bail!("{adjusted}: does not match index");
                    }
                    let wt_content = read_worktree_content_for_index(&path, entry.mode)?;
                    let wt_oid =
                        grit_lib::odb::Odb::hash_object_data(ObjectKind::Blob, &wt_content);
                    if wt_oid != entry.oid {
                        bail!("{adjusted}: does not match index");
                    }
                }
            }
            continue;
        }
        // Skip submodule entries
        if fp.old_mode.as_deref() == Some("160000") || fp.new_mode.as_deref() == Some("160000") {
            continue;
        }
        let path_str = fp
            .source_path()
            .ok_or_else(|| anyhow::anyhow!("patch has no file path"))?;
        let adjusted = adjust_path(path_str, args.strip, args.directory.as_deref());
        let path = PathBuf::from(&adjusted);

        if !path.exists() {
            continue;
        }

        // Get index entry
        if let Some(entry) = index.get(adjusted.as_bytes(), 0) {
            let wt_content = read_worktree_content_for_index(&path, entry.mode)?;
            let wt_oid = grit_lib::odb::Odb::hash_object_data(ObjectKind::Blob, &wt_content);
            if wt_oid != entry.oid {
                bail!("{adjusted}: does not match index");
            }
            // Also verify the patch's expected old OID matches the index
            if let Some(ref expected_oid) = fp.old_oid {
                let index_hex = entry.oid.to_hex();
                if !index_hex.starts_with(expected_oid.as_str()) {
                    bail!("{adjusted}: does not match index");
                }
            }
        }
    }

    Ok(())
}

fn read_worktree_content_for_index(path: &Path, mode: u32) -> Result<Vec<u8>> {
    if mode == grit_lib::index::MODE_SYMLINK {
        let target = fs::read_link(path)
            .with_context(|| format!("failed to read symlink target {}", path.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            return Ok(target.as_os_str().as_bytes().to_vec());
        }
        #[cfg(not(unix))]
        {
            return Ok(target.to_string_lossy().into_owned().into_bytes());
        }
    }
    fs::read(path).with_context(|| format!("failed to read {}", path.display()))
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

fn verify_old_oid_matches_content(expected_oid: &str, content: &str) -> Result<()> {
    let actual_oid = grit_lib::odb::Odb::hash_object_data(ObjectKind::Blob, content.as_bytes());
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
            let adjusted = adjust_path(source, args.strip, args.directory.as_deref());
            record_path_existence(&adjusted, &mut current_exists, &mut initial_exists);
        }
        if let Some(target) = fp.target_path() {
            let adjusted = adjust_path(target, args.strip, args.directory.as_deref());
            record_path_existence(&adjusted, &mut current_exists, &mut initial_exists);
        }
    }

    for fp in patches {
        let source_adjusted = fp
            .source_path()
            .map(|p| adjust_path(p, args.strip, args.directory.as_deref()))
            .unwrap_or_default();
        let target_adjusted = fp
            .target_path()
            .map(|p| adjust_path(p, args.strip, args.directory.as_deref()))
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
                bail!("{target_adjusted}: already exists");
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
) -> Result<()> {
    let mut had_rejects = false;
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
        let source_adjusted = adjust_path(source, args.strip, args.directory.as_deref());
        let target_adjusted = adjust_path(target, args.strip, args.directory.as_deref());
        if source_adjusted == target_adjusted || source_snapshots.contains_key(&source_adjusted) {
            continue;
        }
        if let Ok(content) = fs::read_to_string(&source_adjusted) {
            source_snapshots.insert(source_adjusted, content);
        }
    }
    precheck_worktree_patch_sequence(patches, args)?;

    for fp in patches {
        let path_str = fp
            .target_path()
            .ok_or_else(|| anyhow::anyhow!("patch has no file path"))?;
        let path_adjusted = adjust_path(path_str, args.strip, args.directory.as_deref());
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

        if fp.is_new {
            // Submodule: create directory
            if fp.new_mode.as_deref() == Some("160000") {
                fs::create_dir_all(&path)?;
                continue;
            }
            // Create new file
            if let Some(parent) = path.parent() {
                if !parent.as_os_str().is_empty() && !parent.exists() {
                    fs::create_dir_all(parent)?;
                }
            }
            let content = apply_hunks("", &fp.hunks, ws_mode).with_context(|| {
                format!("failed to apply hunks for new file {}", path.display())
            })?;
            fs::write(&path, content.as_bytes())
                .with_context(|| format!("failed to write {}", path.display()))?;

            // Set executable if mode is 100755
            #[cfg(unix)]
            if fp.new_mode.as_deref() == Some("100755") {
                use std::os::unix::fs::PermissionsExt;
                let perms = fs::Permissions::from_mode(0o755);
                fs::set_permissions(&path, perms)?;
            }
            continue;
        }

        // Modify existing file — read preimage from source side (important
        // for rename/copy patches where target may not exist yet).
        let source_adjusted = fp
            .source_path()
            .map(|p| adjust_path(p, args.strip, args.directory.as_deref()))
            .unwrap_or_else(|| path_adjusted.clone());
        let read_path = PathBuf::from(&source_adjusted);
        let load_old_content_from_disk = || -> Result<String> {
            match fs::read_to_string(&read_path) {
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
        if !args.reject {
            if let Some(expected_oid) = fp.old_oid.as_deref() {
                verify_old_oid_matches_content(expected_oid, &old_content)?;
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

        if let Some(binary_patch) = fp.binary_patch.as_ref() {
            if !args.allow_binary_replacement {
                bail!("cannot apply binary patch without --binary");
            }
            let new_bytes = inflate_binary_payload(&binary_patch.forward_compressed)?;
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

            if fp.is_rename && read_path != path {
                if read_path.exists() {
                    fs::remove_file(&read_path)
                        .with_context(|| format!("failed to remove {}", read_path.display()))?;
                }
                if let Some(parent) = read_path.parent() {
                    remove_empty_dirs_up(parent);
                }
            }
            continue;
        }

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

        let (new_content, rejected_hunks) = if args.reject {
            apply_hunks_with_reject(&old_content, &fp.hunks, ws_mode)
                .with_context(|| format!("failed to apply patch to {}", path.display()))?
        } else {
            let content = apply_hunks(&old_content, &fp.hunks, ws_mode)
                .with_context(|| format!("failed to apply patch to {}", path.display()))?;
            (content, Vec::new())
        };
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }
        fs::write(&path, new_content.as_bytes())
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

        if !rejected_hunks.is_empty() {
            had_rejects = true;
            let reject_path = PathBuf::from(format!("{path_adjusted}.rej"));
            write_reject_file(&reject_path, fp, &rejected_hunks)?;
        }

        if fp.is_rename && read_path != path {
            if read_path.exists() {
                fs::remove_file(&read_path)
                    .with_context(|| format!("failed to remove {}", read_path.display()))?;
            }
            if let Some(parent) = read_path.parent() {
                remove_empty_dirs_up(parent);
            }
        }
    }

    if had_rejects {
        bail!("patch failed");
    }

    Ok(())
}

/// Apply patches to the index only (--cached).
fn apply_to_index(patches: &[FilePatch], args: &Args, ws_mode: ApplyWhitespaceMode) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let mut index = match Index::load(&repo.index_path()) {
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
        let target_raw = adjust_path(target_path_str, args.strip, args.directory.as_deref());
        let target_adjusted = format!("{cwd_prefix}{target_raw}");
        let source_adjusted = fp
            .source_path()
            .map(|p| adjust_path(p, args.strip, args.directory.as_deref()))
            .map(|raw| format!("{cwd_prefix}{raw}"))
            .unwrap_or_else(|| target_adjusted.clone());

        if fp.is_deleted {
            index.remove(source_adjusted.as_bytes());
            continue;
        }

        if let Some(binary_patch) = fp.binary_patch.as_ref() {
            if !args.allow_binary_replacement {
                bail!("cannot apply binary patch without --binary");
            }
            let new_bytes = inflate_binary_payload(&binary_patch.forward_compressed)?;
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

        // Handle submodule (gitlink) entries specially
        if (fp.new_mode.as_deref() == Some("160000") || fp.old_mode.as_deref() == Some("160000"))
            && fp.is_new
        {
            let commit_hash = fp
                .hunks
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
                .unwrap_or_else(|| "0000000000000000000000000000000000000000".to_string());
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

        // Get old content from index (or empty for new files)
        let old_content = if fp.is_new {
            String::new()
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
                verify_old_oid_matches_content(expected_oid, &old_content)?;
            }
        }

        let new_content = if fp.hunks.is_empty() {
            old_content.clone()
        } else {
            apply_hunks(&old_content, &fp.hunks, ws_mode)
                .with_context(|| format!("failed to apply patch to {target_adjusted}"))?
        };

        // Write new blob to ODB
        let new_oid = repo.odb.write(ObjectKind::Blob, new_content.as_bytes())?;

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

    index.write(&repo.index_path())?;
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
    let mut index = match Index::load(&repo.index_path()) {
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
        let target_raw = adjust_path(target_path, args.strip, args.directory.as_deref());
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

    index.write(&repo.index_path())?;
    Ok(())
}

/// Check if patches apply cleanly without modifying anything.
fn check_patches(patches: &[FilePatch], args: &Args, ws_mode: ApplyWhitespaceMode) -> Result<()> {
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
            apply_hunks("", &fp.hunks, ws_mode).with_context(|| {
                format!(
                    "patch does not apply cleanly to new file {}",
                    path.display()
                )
            })?;
            continue;
        }

        let read_path = fp
            .source_path()
            .map(|p| PathBuf::from(adjust_path(p, args.strip, args.directory.as_deref())))
            .unwrap_or_else(|| path.clone());
        let old_content = match fs::read_to_string(&read_path) {
            Ok(content) => content,
            Err(err)
                if err.kind() == std::io::ErrorKind::NotFound
                    && can_apply_with_empty_preimage(fp) =>
            {
                String::new()
            }
            Err(err) => {
                return Err(err).with_context(|| format!("failed to read {}", read_path.display()))
            }
        };
        if let Some(expected_oid) = fp.old_oid.as_deref() {
            verify_old_oid_matches_content(expected_oid, &old_content)?;
        }
        apply_hunks(&old_content, &fp.hunks, ws_mode)
            .with_context(|| format!("patch does not apply cleanly to {}", path.display()))?;
    }

    Ok(())
}

/// Parse an octal mode string like "100644" to u32.
fn parse_mode(s: &str) -> u32 {
    u32::from_str_radix(s, 8).unwrap_or(0o100644)
}
