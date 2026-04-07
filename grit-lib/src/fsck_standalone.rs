//! Standalone object fsck for `hash-object` and similar entry points.
//!
//! Mirrors the buffer-safe checks in Git's `fsck.c` (`verify_headers`,
//! `fsck_commit`, `fsck_tag_standalone`, `fsck_tree`) so error messages match
//! `error: object fails fsck: <camelCaseId>: <detail>`.

use crate::check_ref_format::{check_refname_format, RefNameOptions};
use crate::objects::{ObjectId, ObjectKind};

/// Git-compatible fsck failure for loose object validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FsckError {
    /// CamelCase message id (e.g. `missingTree`).
    pub id: &'static str,
    /// Human-readable detail after `id: `.
    pub detail: String,
}

impl FsckError {
    fn new(id: &'static str, detail: impl Into<String>) -> Self {
        Self {
            id,
            detail: detail.into(),
        }
    }

    /// Full line after `error: object fails fsck: ` (matches Git).
    #[must_use]
    pub fn report_line(&self) -> String {
        format!("{}: {}", self.id, self.detail)
    }
}

/// Validate raw object bytes the same way `git hash-object` does before hashing.
///
/// Returns `Ok(())` when the object is well-formed, or the first fsck error Git
/// would report for truncated or malformed buffers.
pub fn fsck_object(kind: ObjectKind, data: &[u8]) -> Result<(), FsckError> {
    match kind {
        ObjectKind::Blob => Ok(()),
        ObjectKind::Commit => fsck_commit(data),
        ObjectKind::Tag => fsck_tag(data),
        ObjectKind::Tree => fsck_tree(data),
    }
}

fn verify_headers(data: &[u8], nul_msg_id: &'static str) -> Result<(), FsckError> {
    for (i, &b) in data.iter().enumerate() {
        if b == 0 {
            return Err(FsckError::new(
                nul_msg_id,
                format!("unterminated header: NUL at offset {i}"),
            ));
        }
        if b == b'\n' && i + 1 < data.len() && data[i + 1] == b'\n' {
            return Ok(());
        }
    }
    if !data.is_empty() && data[data.len() - 1] == b'\n' {
        Ok(())
    } else {
        Err(FsckError::new("unterminatedHeader", "unterminated header"))
    }
}

fn is_hex_lower(b: u8) -> bool {
    matches!(b, b'0'..=b'9' | b'a'..=b'f')
}

/// Parse a 40-character lowercase hex object id at the start of `buf`, requiring
/// the next byte to be `\n`. Returns bytes consumed (41).
fn parse_oid_line(buf: &[u8], bad_sha1_id: &'static str) -> Result<usize, FsckError> {
    if buf.len() < 41 {
        return Err(FsckError::new(
            bad_sha1_id,
            format!(
                "invalid '{}' line format - bad sha1",
                line_kind(bad_sha1_id)
            ),
        ));
    }
    let hex = &buf[..40];
    if !hex.iter().copied().all(is_hex_lower) {
        return Err(FsckError::new(
            bad_sha1_id,
            format!(
                "invalid '{}' line format - bad sha1",
                line_kind(bad_sha1_id)
            ),
        ));
    }
    if buf[40] != b'\n' {
        return Err(FsckError::new(
            bad_sha1_id,
            format!(
                "invalid '{}' line format - bad sha1",
                line_kind(bad_sha1_id)
            ),
        ));
    }
    let hex_str = std::str::from_utf8(hex).map_err(|_| {
        FsckError::new(
            bad_sha1_id,
            format!(
                "invalid '{}' line format - bad sha1",
                line_kind(bad_sha1_id)
            ),
        )
    })?;
    hex_str.parse::<ObjectId>().map_err(|_| {
        FsckError::new(
            bad_sha1_id,
            format!(
                "invalid '{}' line format - bad sha1",
                line_kind(bad_sha1_id)
            ),
        )
    })?;
    Ok(41)
}

fn line_kind(bad_sha1_id: &'static str) -> &'static str {
    match bad_sha1_id {
        "badObjectSha1" => "object",
        "badParentSha1" => "parent",
        _ => "tree",
    }
}

fn fsck_ident(
    data: &[u8],
    start: usize,
    buffer_end: usize,
    oid_line: &'static str,
) -> Result<usize, FsckError> {
    let mut p = start;
    if p >= buffer_end {
        return Err(FsckError::new(
            "missingEmail",
            format!("invalid {oid_line} line - missing email"),
        ));
    }

    let line_end = data[p..buffer_end]
        .iter()
        .position(|&b| b == b'\n')
        .map(|rel| p + rel)
        .ok_or_else(|| {
            FsckError::new(
                "missingEmail",
                format!("invalid {oid_line} line - missing email"),
            )
        })?;

    if data[p] == b'<' {
        return Err(FsckError::new(
            "missingNameBeforeEmail",
            format!("invalid {oid_line} line - missing space before email"),
        ));
    }

    let ident_end = line_end;
    while p < ident_end {
        if data[p] == b'\n' {
            return Err(FsckError::new(
                "missingEmail",
                format!("invalid {oid_line} line - missing email"),
            ));
        }
        if data[p] == b'>' {
            return Err(FsckError::new(
                "badName",
                format!("invalid {oid_line} line - bad name"),
            ));
        }
        if data[p] == b'<' {
            break;
        }
        p += 1;
    }

    if p >= ident_end {
        return Err(FsckError::new(
            "missingEmail",
            format!("invalid {oid_line} line - missing email"),
        ));
    }

    if p == start || data[p - 1] != b' ' {
        return Err(FsckError::new(
            "missingSpaceBeforeEmail",
            format!("invalid {oid_line} line - missing space before email"),
        ));
    }
    p += 1; // skip '<'

    let email_start = p;
    while p < ident_end {
        if data[p] == b'<' || data[p] == b'\n' {
            return Err(FsckError::new(
                "badEmail",
                format!("invalid {oid_line} line - bad email"),
            ));
        }
        if data[p] == b'>' {
            break;
        }
        p += 1;
    }

    if p >= ident_end || p == email_start {
        return Err(FsckError::new(
            "badEmail",
            format!("invalid {oid_line} line - bad email"),
        ));
    }
    p += 1; // skip '>'

    if p >= ident_end || data[p] != b' ' {
        return Err(FsckError::new(
            "missingSpaceBeforeDate",
            format!("invalid {oid_line} line - missing space before date"),
        ));
    }
    p += 1;

    while p < ident_end && (data[p] == b' ' || data[p] == b'\t') {
        p += 1;
    }

    if p >= ident_end || !data[p].is_ascii_digit() {
        return Err(FsckError::new(
            "badDate",
            format!("invalid {oid_line} line - bad date"),
        ));
    }

    if data[p] == b'0' && p + 1 < ident_end && data[p + 1] != b' ' {
        return Err(FsckError::new(
            "zeroPaddedDate",
            format!("invalid {oid_line} line - zero-padded date"),
        ));
    }

    while p < ident_end && data[p].is_ascii_digit() {
        p += 1;
    }

    if p >= ident_end || data[p] != b' ' {
        return Err(FsckError::new(
            "badDate",
            format!("invalid {oid_line} line - bad date"),
        ));
    }
    p += 1;

    if p + 5 > ident_end
        || (data[p] != b'+' && data[p] != b'-')
        || !data[p + 1..p + 5].iter().all(|b| b.is_ascii_digit())
        || data[p + 5] != b'\n'
    {
        return Err(FsckError::new(
            "badTimezone",
            format!("invalid {oid_line} line - bad time zone"),
        ));
    }

    Ok(line_end + 1)
}

fn fsck_commit(data: &[u8]) -> Result<(), FsckError> {
    verify_headers(data, "nulInHeader")?;

    let buffer_end = data.len();
    let mut i = 0usize;

    if i >= buffer_end || !data[i..].starts_with(b"tree ") {
        return Err(FsckError::new(
            "missingTree",
            "invalid format - expected 'tree' line",
        ));
    }
    i += 5;
    let n = parse_oid_line(&data[i..], "badTreeSha1")?;
    i += n;

    while i < buffer_end && data[i..].starts_with(b"parent ") {
        i += 7;
        let n = parse_oid_line(&data[i..], "badParentSha1")?;
        i += n;
    }

    let mut author_count = 0usize;
    while i < buffer_end && data[i..].starts_with(b"author ") {
        author_count += 1;
        i += 7;
        i = fsck_ident(data, i, buffer_end, "author/committer")?;
    }

    if author_count < 1 {
        return Err(FsckError::new(
            "missingAuthor",
            "invalid format - expected 'author' line",
        ));
    }
    if author_count > 1 {
        return Err(FsckError::new(
            "multipleAuthors",
            "invalid format - multiple 'author' lines",
        ));
    }

    if i >= buffer_end || !data[i..].starts_with(b"committer ") {
        return Err(FsckError::new(
            "missingCommitter",
            "invalid format - expected 'committer' line",
        ));
    }
    i += 10;
    fsck_ident(data, i, buffer_end, "author/committer")?;

    if data.contains(&0) {
        return Err(FsckError::new(
            "nulInCommit",
            "NUL byte in the commit object body",
        ));
    }

    Ok(())
}

fn object_type_from_tag_type_line(s: &str) -> Option<ObjectKind> {
    match s {
        "blob" => Some(ObjectKind::Blob),
        "tree" => Some(ObjectKind::Tree),
        "commit" => Some(ObjectKind::Commit),
        "tag" => Some(ObjectKind::Tag),
        _ => None,
    }
}

fn fsck_tag(data: &[u8]) -> Result<(), FsckError> {
    verify_headers(data, "nulInHeader")?;

    let buffer_end = data.len();
    let mut i = 0usize;

    if i >= buffer_end || !data[i..].starts_with(b"object ") {
        return Err(FsckError::new(
            "missingObject",
            "invalid format - expected 'object' line",
        ));
    }
    i += 7;
    let n = parse_oid_line(&data[i..], "badObjectSha1")?;
    i += n;

    if i >= buffer_end || !data[i..].starts_with(b"type ") {
        return Err(FsckError::new(
            "missingTypeEntry",
            "invalid format - expected 'type' line",
        ));
    }
    i += 5;
    let type_start = i;
    let eol = data[type_start..buffer_end]
        .iter()
        .position(|&b| b == b'\n')
        .map(|rel| type_start + rel)
        .ok_or_else(|| {
            FsckError::new(
                "missingType",
                "invalid format - unexpected end after 'type' line",
            )
        })?;

    let type_str = std::str::from_utf8(&data[type_start..eol])
        .map_err(|_| FsckError::new("badType", "invalid 'type' value"))?;
    if object_type_from_tag_type_line(type_str).is_none() {
        return Err(FsckError::new("badType", "invalid 'type' value"));
    }
    i = eol + 1;

    if i >= buffer_end || !data[i..].starts_with(b"tag ") {
        return Err(FsckError::new(
            "missingTagEntry",
            "invalid format - expected 'tag' line",
        ));
    }
    i += 4;
    let tag_start = i;
    let eol = data[tag_start..buffer_end]
        .iter()
        .position(|&b| b == b'\n')
        .map(|rel| tag_start + rel)
        .ok_or_else(|| {
            FsckError::new(
                "missingTag",
                "invalid format - unexpected end after 'type' line",
            )
        })?;

    let tag_name = std::str::from_utf8(&data[tag_start..eol])
        .map_err(|_| FsckError::new("badTagName", "invalid 'tag' name"))?;
    let refname = format!("refs/tags/{tag_name}");
    if check_refname_format(&refname, &RefNameOptions::default()).is_err() {
        return Err(FsckError::new(
            "badTagName",
            format!("invalid 'tag' name: {tag_name}"),
        ));
    }
    i = eol + 1;

    if i >= buffer_end || !data[i..].starts_with(b"tagger ") {
        return Err(FsckError::new(
            "missingTaggerEntry",
            "invalid format - expected 'tagger' line",
        ));
    }
    i += 7;
    fsck_ident(data, i, buffer_end, "author/committer")?;

    Ok(())
}

fn fsck_tree(data: &[u8]) -> Result<(), FsckError> {
    if parse_tree_gently(data).is_err() {
        return Err(FsckError::new("badTree", "cannot be parsed as a tree"));
    }
    Ok(())
}

fn parse_tree_gently(data: &[u8]) -> Result<(), ()> {
    let mut pos = 0usize;
    while pos < data.len() {
        let sp = data[pos..].iter().position(|&b| b == b' ').ok_or(())?;
        let mode_bytes = &data[pos..pos + sp];
        let mode_ok = std::str::from_utf8(mode_bytes)
            .ok()
            .and_then(|s| u32::from_str_radix(s, 8).ok())
            .is_some();
        if !mode_ok {
            return Err(());
        }
        pos += sp + 1;

        let nul = data[pos..].iter().position(|&b| b == 0).ok_or(())?;
        pos += nul + 1;

        if pos + 20 > data.len() {
            return Err(());
        }
        if ObjectId::from_bytes(&data[pos..pos + 20]).is_err() {
            return Err(());
        }
        pos += 20;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_commit_is_unterminated_header() {
        let e = fsck_object(ObjectKind::Commit, b"").unwrap_err();
        assert_eq!(e.id, "unterminatedHeader");
    }

    #[test]
    fn commit_missing_tree_matches_git() {
        let e = fsck_object(ObjectKind::Commit, b"\n\n").unwrap_err();
        assert_eq!(e.id, "missingTree");
    }

    #[test]
    fn tree_truncated_is_bad_tree() {
        let e = fsck_object(ObjectKind::Tree, b"100644 foo\0\x01\x01\x01\x01").unwrap_err();
        assert_eq!(e.id, "badTree");
    }
}
