//! `grit mktree` — build a tree object from ls-tree formatted text.
//!
//! Reads non-recursive `ls-tree` output from stdin (one entry per line:
//! `<mode> <type> <oid>\t<name>`) and creates a tree object, printing its OID.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use std::io::{self, Read, Write};

use grit_lib::objects::{serialize_tree, tree_entry_cmp, ObjectId, ObjectKind, TreeEntry};
use grit_lib::repo::Repository;

/// Arguments for `grit mktree`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Read the NUL-terminated `ls-tree -z` output.
    #[arg(short = 'z')]
    pub null_terminated: bool,

    /// Allow missing objects (objects are presumed to be of the correct type).
    #[arg(long)]
    pub missing: bool,

    /// Allow creation of more than one tree; blank lines separate trees.
    #[arg(long)]
    pub batch: bool,
}

/// Run `grit mktree`.
///
/// Reads `ls-tree`-formatted lines from stdin, creates tree objects, and
/// prints each resulting OID to stdout.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let stdout = io::stdout();
    let mut out = stdout.lock();

    let mut raw = Vec::new();
    io::stdin().lock().read_to_end(&mut raw)?;

    let delim = if args.null_terminated { 0u8 } else { b'\n' };

    let mut entries: Vec<TreeEntry> = Vec::new();
    let mut records = raw.split(|&b| b == delim).peekable();
    let mut got_eof = false;

    loop {
        // Inner loop: consume records until a blank separator or end of input.
        let _broke_blank = loop {
            match records.next() {
                None => {
                    got_eof = true;
                    break false;
                }
                Some(record) => {
                    if record.is_empty() {
                        // A trailing delimiter at EOF is not a blank separator;
                        // treat it as end-of-input.
                        if records.peek().is_none() {
                            got_eof = true;
                            break false;
                        }
                        if args.batch {
                            break true;
                        } else {
                            bail!("input format error: (blank line only valid in batch mode)");
                        }
                    } else {
                        let line =
                            std::str::from_utf8(record).context("input line is not valid UTF-8")?;
                        let entry =
                            parse_mktree_line(line, args.null_terminated, args.missing, &repo)?;
                        entries.push(entry);
                    }
                }
            }
        };

        // In batch mode, skip an empty tree only at the very end (trailing
        // newline after the last entry).  A blank separator mid-stream always
        // produces a tree (even an empty one).
        if args.batch && got_eof && entries.is_empty() {
            // skip — consistent with git's behaviour
        } else {
            let oid = write_tree_entries(&repo, &entries)?;
            writeln!(out, "{oid}")?;
        }
        entries.clear();

        if got_eof {
            break;
        }
    }

    Ok(())
}

/// Derive the expected [`ObjectKind`] from a raw Unix file mode.
///
/// - `0o160000` (gitlink / submodule) → [`ObjectKind::Commit`]
/// - `0o040000` (directory) → [`ObjectKind::Tree`]
/// - anything else → [`ObjectKind::Blob`]
fn kind_from_mode(mode: u32) -> ObjectKind {
    match mode & 0o170000 {
        0o160000 => ObjectKind::Commit,
        0o040000 => ObjectKind::Tree,
        _ => ObjectKind::Blob,
    }
}

/// Parse one `ls-tree`-format record into a [`TreeEntry`].
///
/// Expected format: `<mode> SP <type> SP <sha1> TAB <name>`
///
/// # Parameters
///
/// - `line`: the text record (delimiter already stripped)
/// - `nul_term`: `true` when reading NUL-terminated input (disables C quoting)
/// - `allow_missing`: skip object-existence checks
/// - `repo`: repository whose ODB is used for existence/type checks
///
/// # Errors
///
/// Returns an error for malformed input, type mismatches, or unavailable
/// objects (when `allow_missing` is `false`).
fn parse_mktree_line(
    line: &str,
    nul_term: bool,
    allow_missing: bool,
    repo: &Repository,
) -> Result<TreeEntry> {
    // mode SP type SP sha1 TAB name
    let (mode_str, rest) = line
        .split_once(' ')
        .ok_or_else(|| anyhow::anyhow!("input format error: {line}"))?;

    let mode =
        u32::from_str_radix(mode_str, 8).with_context(|| format!("input format error: {line}"))?;

    let (type_str, rest) = rest
        .split_once(' ')
        .ok_or_else(|| anyhow::anyhow!("input format error: {line}"))?;

    let (sha1_str, name_raw) = rest
        .split_once('\t')
        .ok_or_else(|| anyhow::anyhow!("input format error: {line}"))?;

    let oid =
        ObjectId::from_hex(sha1_str).with_context(|| format!("input format error: {line}"))?;

    // Decode name; non-NUL mode supports C-style quoting.
    let name: Vec<u8> = if !nul_term && name_raw.starts_with('"') {
        unquote_c_style(name_raw).with_context(|| format!("invalid quoting in: {line}"))?
    } else {
        name_raw.as_bytes().to_vec()
    };

    // Reject recursive ls-tree output (paths with slashes).
    if name.contains(&b'/') {
        bail!("path {} contains slash", String::from_utf8_lossy(&name));
    }

    // Validate that the declared type matches the mode-derived type.
    let mode_type = kind_from_mode(mode);
    let decl_type: ObjectKind = type_str
        .parse()
        .with_context(|| format!("unknown object type: {type_str}"))?;
    if mode_type != decl_type {
        bail!(
            "entry '{}' object type ({}) doesn't match mode type ({})",
            String::from_utf8_lossy(&name),
            decl_type,
            mode_type
        );
    }

    // Gitlinks (submodules) are always treated as potentially missing.
    let allow_missing_eff = allow_missing || (mode & 0o170000 == 0o160000);

    if repo.odb.exists(&oid) {
        // Object present: verify its stored type matches.
        if let Ok(obj) = repo.odb.read(&oid) {
            if obj.kind != mode_type {
                bail!(
                    "entry '{}' object {} is a {} but specified type was ({})",
                    String::from_utf8_lossy(&name),
                    oid,
                    obj.kind,
                    mode_type
                );
            }
        }
        // If read fails (e.g. packed object not yet supported), assume correct.
    } else if !allow_missing_eff {
        bail!(
            "entry '{}' object {} is unavailable",
            String::from_utf8_lossy(&name),
            oid
        );
    }

    Ok(TreeEntry { mode, name, oid })
}

/// Sort `entries` in Git tree order and write a tree object to the ODB.
///
/// Returns the [`ObjectId`] of the written tree.
///
/// # Errors
///
/// Propagates any ODB write error.
fn write_tree_entries(repo: &Repository, entries: &[TreeEntry]) -> Result<ObjectId> {
    let mut sorted = entries.to_vec();
    sorted.sort_by(|a, b| {
        let a_tree = a.mode & 0o170000 == 0o040000;
        let b_tree = b.mode & 0o170000 == 0o040000;
        tree_entry_cmp(&a.name, a_tree, &b.name, b_tree)
    });
    let data = serialize_tree(&sorted);
    repo.odb
        .write(ObjectKind::Tree, &data)
        .context("writing tree object")
}

/// Decode a C-style double-quoted string (as produced by `ls-tree` for paths
/// with non-ASCII or special characters).
///
/// The input must start and end with `"`.  Supported escape sequences match
/// what Git's `quote.c` emits: `\\`, `\"`, `\a`, `\b`, `\f`, `\n`, `\r`,
/// `\t`, `\v`, and `\NNN` (three-digit octal).
///
/// # Errors
///
/// Returns an error for malformed escape sequences or missing closing quote.
fn unquote_c_style(s: &str) -> Result<Vec<u8>> {
    let bytes = s.as_bytes();
    if bytes.first() != Some(&b'"') || bytes.last() != Some(&b'"') || bytes.len() < 2 {
        bail!("invalid C-style quoting: {s}");
    }
    let inner = &bytes[1..bytes.len() - 1];
    let mut out = Vec::with_capacity(inner.len());
    let mut i = 0;
    while i < inner.len() {
        if inner[i] != b'\\' {
            out.push(inner[i]);
            i += 1;
            continue;
        }
        i += 1;
        if i >= inner.len() {
            bail!("invalid escape at end of string");
        }
        match inner[i] {
            b'\\' => out.push(b'\\'),
            b'"' => out.push(b'"'),
            b'a' => out.push(7),
            b'b' => out.push(8),
            b'f' => out.push(12),
            b'n' => out.push(b'\n'),
            b'r' => out.push(b'\r'),
            b't' => out.push(b'\t'),
            b'v' => out.push(11),
            c if c.is_ascii_digit() => {
                // Three-digit octal escape \NNN
                if i + 2 >= inner.len() {
                    bail!("truncated octal escape");
                }
                let oct =
                    std::str::from_utf8(&inner[i..i + 3]).context("invalid octal escape bytes")?;
                let val = u8::from_str_radix(oct, 8).context("invalid octal escape value")?;
                out.push(val);
                i += 2; // will be incremented once more below
            }
            other => bail!("invalid escape sequence \\{}", char::from(other)),
        }
        i += 1;
    }
    Ok(out)
}
