//! `ls-remote` — enumerate references from a local repository.
//!
//! This module provides the core logic for `grit ls-remote` when targeting a
//! **local** path.  Network transports are out of scope for v1.
//!
//! # Output format
//!
//! Each entry is a `(oid, refname)` pair.  HEAD appears first (when included),
//! followed by all other refs in lexicographic order.  Annotated tags are
//! optionally followed by a peeled entry whose name ends in `^{}`.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::objects::{ObjectId, ObjectKind};
use crate::odb::Odb;

/// A single reference entry produced by [`ls_remote`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefEntry {
    /// Full reference name, e.g. `refs/heads/main`, `HEAD`, or
    /// `refs/tags/v1.0^{}` for a peeled tag.
    pub name: String,
    /// The object ID the reference resolves to.
    pub oid: ObjectId,
    /// Symbolic-ref target for `HEAD` when [`Options::symref`] is set.
    ///
    /// `Some("refs/heads/main")` when HEAD is symbolic; `None` otherwise.
    pub symref_target: Option<String>,
}

/// Options controlling which references [`ls_remote`] returns.
#[derive(Debug, Default)]
pub struct Options {
    /// Restrict output to `refs/heads/` entries only.
    pub heads: bool,
    /// Restrict output to `refs/tags/` entries only.
    pub tags: bool,
    /// Exclude pseudo-refs (HEAD) and peeled tag `^{}` entries.
    pub refs_only: bool,
    /// Annotate symbolic refs (HEAD) with their `ref: <target>` line.
    pub symref: bool,
    /// If non-empty, only return refs matching one of these patterns.
    ///
    /// A ref matches when it equals the pattern exactly **or** when its name
    /// ends with `/<pattern>`.
    pub patterns: Vec<String>,
}

/// List references from the repository at `git_dir`.
///
/// Returns entries with HEAD first (when not suppressed), then all other refs
/// sorted lexicographically.  Annotated tags are followed by a peeled entry
/// (`refs/tags/name^{}`) unless [`Options::refs_only`] is set.
///
/// # Parameters
///
/// - `git_dir` — path to the `.git` directory or bare repository root.
/// - `odb` — object database, used to peel annotated tag objects.
/// - `opts` — filtering and output options.
///
/// # Errors
///
/// Returns [`Error::Io`] on filesystem errors during ref traversal.
pub fn ls_remote(git_dir: &Path, odb: &Odb, opts: &Options) -> Result<Vec<RefEntry>> {
    let mut entries = Vec::new();

    let include_head = !opts.heads && !opts.tags && !opts.refs_only;
    if include_head {
        if let Ok(head_oid) = crate::refs::resolve_ref(git_dir, "HEAD") {
            let symref_target = if opts.symref {
                crate::refs::read_symbolic_ref(git_dir, "HEAD")?
            } else {
                None
            };
            if pattern_matches("HEAD", &opts.patterns) {
                entries.push(RefEntry {
                    name: "HEAD".to_owned(),
                    oid: head_oid,
                    symref_target,
                });
            }
        }
    }

    // Linked worktrees store user-visible refs in the common git directory.
    // Enumerate refs from that common directory when present; otherwise use
    // the provided git_dir directly.
    let refs_dir_root = resolve_common_git_dir(git_dir).unwrap_or_else(|| git_dir.to_path_buf());

    let mut all_refs: BTreeMap<String, ObjectId> = BTreeMap::new();
    collect_loose_refs(
        &refs_dir_root,
        &refs_dir_root.join("refs"),
        "refs",
        &mut all_refs,
    )?;
    for (name, oid) in read_packed_refs(&refs_dir_root)? {
        all_refs.entry(name).or_insert(oid);
    }

    for (name, oid) in &all_refs {
        if opts.heads && !name.starts_with("refs/heads/") {
            continue;
        }
        if opts.tags && !name.starts_with("refs/tags/") {
            continue;
        }
        if !pattern_matches(name, &opts.patterns) {
            continue;
        }

        entries.push(RefEntry {
            name: name.clone(),
            oid: *oid,
            symref_target: None,
        });

        if !opts.refs_only && name.starts_with("refs/tags/") {
            if let Some(peeled) = peel_tag(odb, oid) {
                entries.push(RefEntry {
                    name: format!("{name}^{{}}"),
                    oid: peeled,
                    symref_target: None,
                });
            }
        }
    }

    Ok(entries)
}

/// Resolve the common git directory for linked worktrees.
///
/// Returns `None` when `git_dir/commondir` is absent or invalid.
fn resolve_common_git_dir(git_dir: &Path) -> Option<PathBuf> {
    let raw = fs::read_to_string(git_dir.join("commondir")).ok()?;
    let rel = raw.trim();
    if rel.is_empty() {
        return None;
    }
    let candidate = if Path::new(rel).is_absolute() {
        PathBuf::from(rel)
    } else {
        git_dir.join(rel)
    };
    candidate.canonicalize().ok()
}

/// Returns `true` when `refname` matches one of `patterns`, or when `patterns`
/// is empty (no filtering applied).
///
/// A match occurs when:
/// - `refname == pattern` exactly, **or**
/// - `refname` ends with `/<pattern>` (suffix component match).
fn pattern_matches(refname: &str, patterns: &[String]) -> bool {
    if patterns.is_empty() {
        return true;
    }
    patterns.iter().any(|pat| {
        if pat.contains('*') || pat.contains('?') {
            // Glob-style matching: '*' matches any sequence, '?' matches one char
            glob_match(pat, refname)
        } else {
            refname == pat
                || refname
                    .strip_suffix(pat.as_str())
                    .is_some_and(|prefix| prefix.ends_with('/'))
        }
    })
}

/// Simple glob matching supporting `*` (any sequence) and `?` (single char).
fn glob_match(pattern: &str, text: &str) -> bool {
    let pat: Vec<char> = pattern.chars().collect();
    let txt: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0, 0);
    let (mut star_pi, mut star_ti) = (usize::MAX, 0);
    while ti < txt.len() {
        if pi < pat.len() && (pat[pi] == '?' || pat[pi] == txt[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < pat.len() && pat[pi] == '*' {
            star_pi = pi;
            star_ti = ti;
            pi += 1;
        } else if star_pi != usize::MAX {
            pi = star_pi + 1;
            star_ti += 1;
            ti = star_ti;
        } else {
            return false;
        }
    }
    while pi < pat.len() && pat[pi] == '*' {
        pi += 1;
    }
    pi == pat.len()
}

/// Recursively collect all loose refs under `path` into `out`.
///
/// `relative` is the ref-name prefix corresponding to `path`
/// (e.g. `"refs"` for `<git-dir>/refs`).
fn collect_loose_refs(
    git_dir: &Path,
    path: &Path,
    relative: &str,
    out: &mut BTreeMap<String, ObjectId>,
) -> Result<()> {
    let read_dir = match fs::read_dir(path) {
        Ok(rd) => rd,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(Error::Io(e)),
    };
    for entry in read_dir {
        let entry = entry?;
        let file_name = entry.file_name().to_string_lossy().to_string();
        let next_relative = format!("{relative}/{file_name}");
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_loose_refs(git_dir, &entry.path(), &next_relative, out)?;
        } else if file_type.is_file() {
            if let Ok(oid) = crate::refs::resolve_ref(git_dir, &next_relative) {
                out.insert(next_relative, oid);
            }
        }
    }
    Ok(())
}

/// Parse `<git-dir>/packed-refs` and return all `(name, oid)` pairs.
///
/// Comment lines (`#`) and peeling lines (`^`) are skipped.
/// Returns an empty `Vec` when the file does not exist.
///
/// # Errors
///
/// Returns [`Error::Io`] on read errors other than `NotFound`.
fn read_packed_refs(git_dir: &Path) -> Result<Vec<(String, ObjectId)>> {
    let path = git_dir.join("packed-refs");
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(Error::Io(e)),
    };
    let mut entries = Vec::new();
    for line in text.lines() {
        if line.is_empty() || line.starts_with('#') || line.starts_with('^') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let Some(oid_str) = parts.next() else {
            continue;
        };
        let Some(name) = parts.next() else {
            continue;
        };
        if let Ok(oid) = oid_str.parse::<ObjectId>() {
            entries.push((name.to_owned(), oid));
        }
    }
    Ok(entries)
}

/// Attempt to peel an annotated tag object to the object it points at.
///
/// Returns `Some(target_oid)` when `oid` is a tag object that contains an
/// `object <hex>` header.  Returns `None` for non-tag objects, unreadable
/// objects, or malformed tag data.
fn peel_tag(odb: &Odb, oid: &ObjectId) -> Option<ObjectId> {
    let obj = odb.read(oid).ok()?;
    if obj.kind != ObjectKind::Tag {
        return None;
    }
    let text = std::str::from_utf8(&obj.data).ok()?;
    for line in text.lines() {
        if let Some(target) = line.strip_prefix("object ") {
            return target.trim().parse::<ObjectId>().ok();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::pattern_matches;

    #[test]
    fn pattern_matches_empty_allows_all() {
        assert!(pattern_matches("refs/heads/main", &[]));
        assert!(pattern_matches("HEAD", &[]));
    }

    #[test]
    fn pattern_matches_exact() {
        let pats = vec!["HEAD".to_owned()];
        assert!(pattern_matches("HEAD", &pats));
        assert!(!pattern_matches("refs/heads/main", &pats));
    }

    #[test]
    fn pattern_matches_suffix_component() {
        let pats = vec!["main".to_owned()];
        assert!(pattern_matches("refs/heads/main", &pats));
        assert!(!pattern_matches("refs/heads/notmain", &pats));
        assert!(!pattern_matches("main-branch", &pats));
    }
}
