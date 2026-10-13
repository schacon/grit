//! Revision resolution metadata and typed object contents for display (`resolve` / `cat`).
//!
//! [`Repository::resolve`] resolves a user spec to an object id plus kind and size.
//! [`Repository::read_object_for_display`] loads and parses object bytes for programmatic use.

use thiserror::Error;

use crate::error::{Error, Result};
use crate::objects::{
    parse_commit, parse_tag, parse_tree, CommitData, ObjectId, ObjectKind, TagData, TreeEntry,
};
use crate::repo::Repository;
use crate::rev_parse::{
    abbreviate_object_id, ambiguous_object_hint_lines, parse_peel_suffix, resolve_revision,
    symbolic_full_name,
};
use crate::rev_parse_error::{AmbiguousObjectHint, RevParseError};

/// Failure resolving a revision spec for object inspection.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ResolveError {
    /// The spec does not name a known revision or object.
    #[error("unknown revision '{spec}'")]
    UnknownRevision {
        /// User revision string.
        spec: String,
    },

    /// A short hex prefix matches more than one object and could not be disambiguated.
    #[error("ambiguous short object id '{spec}'")]
    AmbiguousShortObject {
        /// User revision string.
        spec: String,
        /// Candidate objects in Git hint order (type, then hex).
        candidates: Vec<AmbiguousObjectHint>,
    },

    /// A path component is missing from the tree named by the revision.
    #[error("path '{path}' does not exist in '{revision}'")]
    PathNotInTree {
        /// Path that is missing from the tree.
        path: String,
        /// Revision label for the tree that was searched.
        revision: String,
    },
}

impl From<RevParseError> for ResolveError {
    fn from(value: RevParseError) -> Self {
        match value {
            RevParseError::PathNotInRevision { path, revision }
            | RevParseError::PathOnDiskNotInRevision { path, revision } => {
                ResolveError::PathNotInTree { path, revision }
            }
            RevParseError::InvalidObjectName { name } => {
                ResolveError::UnknownRevision { spec: name }
            }
            other => ResolveError::UnknownRevision {
                spec: other.to_string(),
            },
        }
    }
}

/// Result of resolving one revision spec to a concrete object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedObject {
    /// Resolved object id (after peel suffixes in the spec).
    pub oid: ObjectId,
    /// Kind of the resolved object.
    pub kind: ObjectKind,
    /// Uncompressed object payload size in bytes.
    pub size: u64,
    /// Short ref name when the spec named a ref directly (for example `main`, `v1.0`).
    pub ref_name: Option<String>,
}

/// Parsed object payload for display or serialization.
#[derive(Debug, Clone)]
pub enum ObjectContents {
    /// Raw blob bytes.
    Blob(Vec<u8>),
    /// Tree entries in Git sort order.
    Tree(Vec<TreeEntry>),
    /// Parsed commit headers and message.
    Commit(CommitData),
    /// Parsed annotated tag.
    Tag(TagData),
}

impl Repository {
    /// Resolve `spec` to an object id, kind, and size.
    ///
    /// Supports the same revision forms as [`crate::revwalk::rev_parse::resolve_revision`]
    /// (for example `HEAD~2`, `v1^{tree}`, `main:path`, `@{1}`, abbreviated oids).
    ///
    /// # Errors
    ///
    /// Returns [`ResolveError::UnknownRevision`] when the spec cannot be resolved,
    /// [`ResolveError::AmbiguousShortObject`] when a short oid is ambiguous, and
    /// [`ResolveError::PathNotInTree`] when a `rev:path` path is missing.
    pub fn resolve(&self, spec: &str) -> std::result::Result<ResolvedObject, ResolveError> {
        let oid = match resolve_revision(self, spec) {
            Ok(o) => o,
            Err(Error::ObjectNotFound(_)) => {
                return Err(ResolveError::UnknownRevision {
                    spec: spec.to_owned(),
                });
            }
            Err(Error::InvalidRef(msg)) if msg.contains("ambiguous") => {
                let prefix = ambiguous_prefix_from_spec(spec);
                let candidates =
                    ambiguous_object_hint_lines(self, &prefix, None).unwrap_or_default();
                return Err(ResolveError::AmbiguousShortObject {
                    spec: spec.to_owned(),
                    candidates,
                });
            }
            Err(Error::RevParse(e)) => return Err(e.into()),
            Err(_) => {
                return Err(ResolveError::UnknownRevision {
                    spec: spec.to_owned(),
                });
            }
        };

        let info = self
            .odb()
            .read_info(&oid)
            .map_err(|_| ResolveError::UnknownRevision {
                spec: spec.to_owned(),
            })?;
        Ok(ResolvedObject {
            oid,
            kind: info.kind,
            size: info.size,
            ref_name: infer_ref_name(self, spec, oid),
        })
    }

    /// Read and parse the object named by `oid` for display.
    ///
    /// # Errors
    ///
    /// Returns [`Error::ObjectNotFound`] when the object is missing and
    /// [`Error::CorruptObject`] when parsing fails.
    pub fn read_object_for_display(&self, oid: &ObjectId) -> Result<ObjectContents> {
        let obj = self.read_replaced(oid)?;
        match obj.kind {
            ObjectKind::Blob => Ok(ObjectContents::Blob(obj.data)),
            ObjectKind::Tree => Ok(ObjectContents::Tree(parse_tree(&obj.data)?)),
            ObjectKind::Commit => Ok(ObjectContents::Commit(parse_commit(&obj.data)?)),
            ObjectKind::Tag => Ok(ObjectContents::Tag(parse_tag(&obj.data)?)),
        }
    }
}

/// Abbreviate `oid` for human output using repository uniqueness rules.
pub fn abbreviate_for_display(repo: &Repository, oid: ObjectId, short: bool) -> Result<String> {
    if short {
        abbreviate_object_id(repo, oid, 7)
    } else {
        Ok(oid.to_hex())
    }
}

fn ambiguous_prefix_from_spec(spec: &str) -> String {
    let rev_part = spec.split(':').next().unwrap_or(spec);
    let (base, _) = parse_peel_suffix(rev_part);
    base.to_owned()
}

fn infer_ref_name(repo: &Repository, spec: &str, oid: ObjectId) -> Option<String> {
    let rev_part = spec.split(':').next().unwrap_or(spec);
    let (base, _) = parse_peel_suffix(rev_part);
    if base.is_empty() || looks_like_abbrev_hex(base, repo) {
        return None;
    }
    if symbolic_full_name(repo, base).is_some() {
        return Some(base.to_owned());
    }
    for prefix in ["refs/heads/", "refs/tags/"] {
        let full = format!("{prefix}{base}");
        if repo.resolve_ref_name(&full).ok() == Some(oid) {
            return Some(base.to_owned());
        }
    }
    None
}

fn looks_like_abbrev_hex(base: &str, repo: &Repository) -> bool {
    if base.len() >= repo.odb().hash_algo().hex_len() {
        return base.chars().all(|c| c.is_ascii_hexdigit());
    }
    base.len() >= 4 && base.chars().all(|c| c.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_error_from_path_not_in_revision() {
        let err = ResolveError::from(RevParseError::PathNotInRevision {
            path: "missing.txt".into(),
            revision: "HEAD".into(),
        });
        assert!(matches!(err, ResolveError::PathNotInTree { .. }));
    }

    #[test]
    fn ambiguous_prefix_strips_colon_path() {
        assert_eq!(ambiguous_prefix_from_spec("abcd:file"), "abcd");
    }
}
