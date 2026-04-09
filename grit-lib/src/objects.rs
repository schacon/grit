//! Git object model: object IDs, kinds, and in-memory representations.
//!
//! # Object ID
//!
//! [`ObjectId`] is a 20-byte SHA-1 digest.  It implements `Display` as
//! lowercase hex, `FromStr` from a 40-character hex string, and the standard
//! ordering traits so it can be used as a map key.
//!
//! # Object Kind
//!
//! [`ObjectKind`] represents the four Git object types: blob, tree, commit,
//! and tag.  The raw header byte-slice is parsed with [`ObjectKind::from_bytes`].
//!
//! # Parsed objects
//!
//! [`Object`] bundles a kind and its raw (decompressed, header-stripped) byte
//! content.  Higher-level parsed forms (e.g. [`TreeEntry`], [`CommitData`])
//! live in this module and are produced by fallible `TryFrom<&Object>`
//! conversions.

use std::fmt;
use std::str::FromStr;

use crate::commit_encoding;
use crate::error::{Error, Result};

/// A 20-byte SHA-1 object identifier.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjectId([u8; 20]);

impl ObjectId {
    /// The all-zero object id (Git's "null" OID).
    ///
    /// Used for index placeholders such as intent-to-add entries and for
    /// special cases in plumbing output.
    #[must_use]
    pub const fn zero() -> Self {
        Self([0u8; 20])
    }

    /// Construct from a 20-byte slice.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidObjectId`] when `bytes` is not exactly 20 bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let arr: [u8; 20] = bytes
            .try_into()
            .map_err(|_| Error::InvalidObjectId(hex::encode(bytes)))?;
        Ok(Self(arr))
    }

    /// Raw 20-byte digest.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 20] {
        &self.0
    }

    /// Check if this is the null (all-zero) object ID.
    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.0 == [0u8; 20]
    }

    /// Lowercase hex representation (40 characters).
    #[must_use]
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    /// The two-character directory prefix used by the loose object store.
    ///
    /// Returns the first two hex chars (e.g. `"ab"` for `"ab3f…"`).
    #[must_use]
    pub fn loose_prefix(&self) -> String {
        hex::encode(&self.0[..1])
    }

    /// Parse an object ID from a hex string.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidObjectId`] if the string is not a valid
    /// 40-character hex OID.
    pub fn from_hex(s: &str) -> Result<Self> {
        s.parse()
    }

    /// The 38-character suffix used as the filename inside the loose prefix dir.
    #[must_use]
    pub fn loose_suffix(&self) -> String {
        hex::encode(&self.0[1..])
    }
}

impl fmt::Display for ObjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

impl fmt::Debug for ObjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ObjectId({})", self.to_hex())
    }
}

impl FromStr for ObjectId {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        if s.len() != 40 {
            return Err(Error::InvalidObjectId(s.to_owned()));
        }
        let bytes = hex::decode(s).map_err(|_| Error::InvalidObjectId(s.to_owned()))?;
        Self::from_bytes(&bytes)
    }
}

/// The four Git object types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectKind {
    /// A raw file snapshot.
    Blob,
    /// A directory listing.
    Tree,
    /// A snapshot with metadata and parentage.
    Commit,
    /// An annotated tag.
    Tag,
}

impl ObjectKind {
    /// Parse from the ASCII keyword used in Git object headers.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnknownObjectType`] for unrecognised strings.
    pub fn from_bytes(b: &[u8]) -> Result<Self> {
        match b {
            b"blob" => Ok(Self::Blob),
            b"tree" => Ok(Self::Tree),
            b"commit" => Ok(Self::Commit),
            b"tag" => Ok(Self::Tag),
            other => Err(Error::UnknownObjectType(
                String::from_utf8_lossy(other).into_owned(),
            )),
        }
    }

    /// The ASCII keyword for this kind (used in object headers).
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Blob => "blob",
            Self::Tree => "tree",
            Self::Commit => "commit",
            Self::Tag => "tag",
        }
    }
}

impl fmt::Display for ObjectKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ObjectKind {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        Self::from_bytes(s.as_bytes())
    }
}

/// A decompressed, header-stripped Git object.
#[derive(Debug, Clone)]
pub struct Object {
    /// The type of this object.
    pub kind: ObjectKind,
    /// Raw byte content (everything after the NUL in the header).
    pub data: Vec<u8>,
}

impl Object {
    /// Construct a new object from its kind and raw data.
    #[must_use]
    pub fn new(kind: ObjectKind, data: Vec<u8>) -> Self {
        Self { kind, data }
    }

    /// Serialize to the canonical Git object format: `"<kind> <size>\0<data>"`.
    #[must_use]
    pub fn to_store_bytes(&self) -> Vec<u8> {
        let header = format!("{} {}\0", self.kind, self.data.len());
        let mut out = Vec::with_capacity(header.len() + self.data.len());
        out.extend_from_slice(header.as_bytes());
        out.extend_from_slice(&self.data);
        out
    }
}

/// A single entry in a Git tree object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeEntry {
    /// Unix file mode (e.g. `0o100644` for a regular file, `0o040000` for a tree).
    pub mode: u32,
    /// Entry name (file or directory name only, no path separators).
    pub name: Vec<u8>,
    /// The object ID of the blob or sub-tree.
    pub oid: ObjectId,
}

impl TreeEntry {
    /// Format the mode as Git does: no leading zero, minimal digits.
    ///
    /// Git uses `"40000"` for trees (not `"040000"`), and `"100644"` for blobs.
    #[must_use]
    pub fn mode_str(&self) -> String {
        // Git omits the leading zero for tree mode
        if self.mode == 0o040000 {
            "40000".to_owned()
        } else {
            format!("{:o}", self.mode)
        }
    }
}

/// Parse the raw data of a tree object into its entries.
///
/// # Format
///
/// Each entry is `"<mode> <name>\0<20-byte-sha1>"` concatenated with no
/// separator between entries.
///
/// # Errors
///
/// Returns [`Error::CorruptObject`] if the data is malformed.
pub fn parse_tree(data: &[u8]) -> Result<Vec<TreeEntry>> {
    let mut entries = Vec::new();
    let mut pos = 0;

    while pos < data.len() {
        // Find the space separating mode from name
        let sp = data[pos..]
            .iter()
            .position(|&b| b == b' ')
            .ok_or_else(|| Error::CorruptObject("tree entry missing space".to_owned()))?;
        let mode_bytes = &data[pos..pos + sp];
        let mode = std::str::from_utf8(mode_bytes)
            .ok()
            .and_then(|s| u32::from_str_radix(s, 8).ok())
            .ok_or_else(|| {
                Error::CorruptObject(format!(
                    "invalid tree mode: {}",
                    String::from_utf8_lossy(mode_bytes)
                ))
            })?;
        pos += sp + 1;

        // Find the NUL separating name from the 20-byte SHA
        let nul = data[pos..]
            .iter()
            .position(|&b| b == 0)
            .ok_or_else(|| Error::CorruptObject("tree entry missing NUL".to_owned()))?;
        let name = data[pos..pos + nul].to_vec();
        pos += nul + 1;

        if pos + 20 > data.len() {
            return Err(Error::CorruptObject("tree entry truncated SHA".to_owned()));
        }
        let oid = ObjectId::from_bytes(&data[pos..pos + 20])?;
        pos += 20;

        entries.push(TreeEntry { mode, name, oid });
    }

    Ok(entries)
}

/// Build the raw bytes of a tree object from a slice of entries.
///
/// Entries **must** already be sorted in Git tree order (see [`tree_entry_cmp`])
/// before calling this function.
#[must_use]
pub fn serialize_tree(entries: &[TreeEntry]) -> Vec<u8> {
    let mut out = Vec::new();
    for e in entries {
        out.extend_from_slice(e.mode_str().as_bytes());
        out.push(b' ');
        out.extend_from_slice(&e.name);
        out.push(0);
        out.extend_from_slice(e.oid.as_bytes());
    }
    out
}

/// Git's tree-entry sort comparator.
///
/// Trees are sorted byte-by-byte by `"<name>"` for blobs and `"<name>/"` for
/// sub-trees, so a directory `foo` sorts after a file `foo-bar` but before
/// `fooz`.  This matches `base_name_compare` in `tree.c`.
///
/// # Parameters
///
/// - `a_name`: name bytes of the first entry
/// - `a_is_tree`: whether the first entry is a sub-tree (`mode == 0o040000`)
/// - `b_name`: name bytes of the second entry
/// - `b_is_tree`: whether the second entry is a sub-tree
#[must_use]
pub fn tree_entry_cmp(
    a_name: &[u8],
    a_is_tree: bool,
    b_name: &[u8],
    b_is_tree: bool,
) -> std::cmp::Ordering {
    let a_trailer = if a_is_tree { b'/' } else { 0u8 };
    let b_trailer = if b_is_tree { b'/' } else { 0u8 };

    let min_len = a_name.len().min(b_name.len());
    let cmp = a_name[..min_len].cmp(&b_name[..min_len]);
    if cmp != std::cmp::Ordering::Equal {
        return cmp;
    }
    // Names share a prefix; compare the next character (or trailer).
    let ac = a_name.get(min_len).copied().unwrap_or(a_trailer);
    let bc = b_name.get(min_len).copied().unwrap_or(b_trailer);
    ac.cmp(&bc)
}

/// Parsed representation of a commit object.
#[derive(Debug, Clone)]
pub struct CommitData {
    /// The tree this commit points to.
    pub tree: ObjectId,
    /// Parent commit IDs (zero or more).
    pub parents: Vec<ObjectId>,
    /// Author field decoded to Unicode (using `encoding` when present, else UTF-8).
    pub author: String,
    /// Committer field decoded to Unicode.
    pub committer: String,
    /// Exact `author` header payload bytes as stored in the object (after `author `).
    ///
    /// Empty means treat [`Self::author`] as UTF-8 when serializing (new commits).
    pub author_raw: Vec<u8>,
    /// Exact `committer` header payload bytes as stored in the object.
    pub committer_raw: Vec<u8>,
    /// Optional encoding override (e.g. `"UTF-8"`).
    pub encoding: Option<String>,
    /// Commit message (everything after the blank line).
    pub message: String,
    /// Optional raw message bytes for non-UTF-8 commit messages.
    /// When set, `serialize_commit` uses these bytes instead of `message`.
    #[doc = "Optional raw message bytes for non-UTF-8 messages."]
    pub raw_message: Option<Vec<u8>>,
}

/// Parse the raw data of a commit object.
///
/// # Errors
///
/// Returns [`Error::CorruptObject`] if required headers are missing.
pub fn parse_commit(data: &[u8]) -> Result<CommitData> {
    // Header lines are mostly ASCII; author/committer payloads may match the `encoding` header.
    // Continuation lines (leading SP) append to the previous header; we only retain author/committer.
    #[derive(Clone, Copy)]
    enum Continuation {
        Author,
        Committer,
        Ignore,
    }

    let mut pos = 0usize;
    let mut tree = None;
    let mut parents = Vec::new();
    let mut author_raw: Option<Vec<u8>> = None;
    let mut committer_raw: Option<Vec<u8>> = None;
    let mut encoding = None;
    let mut cont = Continuation::Ignore;

    while pos < data.len() {
        let line_start = pos;
        let mut line_end = pos;
        while line_end < data.len() && data[line_end] != b'\n' {
            line_end += 1;
        }
        let line = &data[line_start..line_end];
        let after_nl = line_end.saturating_add(1);
        if line.is_empty() {
            let body = data.get(after_nl..).unwrap_or_default();
            let message = commit_encoding::decode_bytes(encoding.as_deref(), body);
            let raw_message = std::str::from_utf8(body).is_err().then(|| body.to_vec());
            let author_bytes = author_raw
                .ok_or_else(|| Error::CorruptObject("commit missing author header".to_owned()))?;
            let committer_bytes = committer_raw.ok_or_else(|| {
                Error::CorruptObject("commit missing committer header".to_owned())
            })?;
            let author = commit_encoding::decode_bytes(encoding.as_deref(), &author_bytes);
            let committer = commit_encoding::decode_bytes(encoding.as_deref(), &committer_bytes);
            return Ok(CommitData {
                tree: tree
                    .ok_or_else(|| Error::CorruptObject("commit missing tree header".to_owned()))?,
                parents,
                author,
                committer,
                author_raw: author_bytes,
                committer_raw: committer_bytes,
                encoding,
                message,
                raw_message,
            });
        }

        if line.first() == Some(&b' ') {
            let rest = line.get(1..).unwrap_or_default();
            match cont {
                Continuation::Author => {
                    let a = author_raw.as_mut().ok_or_else(|| {
                        Error::CorruptObject("orphan header continuation".to_owned())
                    })?;
                    a.extend_from_slice(rest);
                }
                Continuation::Committer => {
                    let c = committer_raw.as_mut().ok_or_else(|| {
                        Error::CorruptObject("orphan header continuation".to_owned())
                    })?;
                    c.extend_from_slice(rest);
                }
                Continuation::Ignore => {}
            }
            pos = after_nl;
            continue;
        }

        let key_end = line
            .iter()
            .position(|&b| b == b' ')
            .ok_or_else(|| Error::CorruptObject("malformed commit header line".to_owned()))?;
        let key = &line[..key_end];
        let rest = line.get(key_end + 1..).unwrap_or_default();

        match key {
            b"tree" => {
                let line_str = std::str::from_utf8(rest).map_err(|_| {
                    Error::CorruptObject("commit tree line is not valid UTF-8".to_owned())
                })?;
                tree = Some(line_str.trim().parse::<ObjectId>()?);
                cont = Continuation::Ignore;
            }
            b"parent" => {
                let line_str = std::str::from_utf8(rest).map_err(|_| {
                    Error::CorruptObject("commit parent line is not valid UTF-8".to_owned())
                })?;
                parents.push(line_str.trim().parse::<ObjectId>()?);
                cont = Continuation::Ignore;
            }
            b"author" => {
                author_raw = Some(rest.to_vec());
                cont = Continuation::Author;
            }
            b"committer" => {
                committer_raw = Some(rest.to_vec());
                cont = Continuation::Committer;
            }
            b"encoding" => {
                let line_str = std::str::from_utf8(rest).map_err(|_| {
                    Error::CorruptObject("commit encoding line is not valid UTF-8".to_owned())
                })?;
                encoding = Some(line_str.to_owned());
                cont = Continuation::Ignore;
            }
            _ => {
                cont = Continuation::Ignore;
            }
        }
        pos = after_nl;
    }

    Err(Error::CorruptObject(
        "commit missing blank line before message".to_owned(),
    ))
}

/// Parsed representation of an annotated tag object.
#[derive(Debug, Clone)]
pub struct TagData {
    /// The object this tag points to.
    pub object: ObjectId,
    /// The type of the tagged object (e.g. `"commit"`).
    pub object_type: String,
    /// The short tag name (without `refs/tags/` prefix).
    pub tag: String,
    /// The tagger identity and timestamp (raw Git format).
    pub tagger: Option<String>,
    /// The tag message (everything after the blank line).
    pub message: String,
}

/// Parse the raw data of a tag object.
///
/// # Errors
///
/// Returns [`Error::CorruptObject`] if required headers are missing or malformed.
pub fn parse_tag(data: &[u8]) -> Result<TagData> {
    let text = std::str::from_utf8(data)
        .map_err(|_| Error::CorruptObject("tag is not valid UTF-8".to_owned()))?;

    let mut object = None;
    let mut object_type = None;
    let mut tag_name = None;
    let mut tagger = None;
    let mut message = String::new();
    let mut in_message = false;

    for line in text.split('\n') {
        if in_message {
            message.push_str(line);
            message.push('\n');
            continue;
        }
        if line.is_empty() {
            in_message = true;
            continue;
        }
        if let Some(rest) = line.strip_prefix("object ") {
            object = Some(rest.trim().parse::<ObjectId>()?);
        } else if let Some(rest) = line.strip_prefix("type ") {
            object_type = Some(rest.trim().to_owned());
        } else if let Some(rest) = line.strip_prefix("tag ") {
            tag_name = Some(rest.trim().to_owned());
        } else if let Some(rest) = line.strip_prefix("tagger ") {
            tagger = Some(rest.to_owned());
        }
    }

    // Strip one trailing newline that split adds
    if message.ends_with('\n') {
        message.pop();
    }

    Ok(TagData {
        object: object
            .ok_or_else(|| Error::CorruptObject("tag missing object header".to_owned()))?,
        object_type: object_type
            .ok_or_else(|| Error::CorruptObject("tag missing type header".to_owned()))?,
        tag: tag_name.ok_or_else(|| Error::CorruptObject("tag missing tag header".to_owned()))?,
        tagger,
        message,
    })
}

/// Serialize a [`TagData`] into the raw bytes suitable for storage as a tag object.
///
/// The caller is responsible for supplying a correctly-formatted `tagger` string
/// (including timestamp and timezone) when present.
#[must_use]
pub fn serialize_tag(t: &TagData) -> Vec<u8> {
    let mut out = String::new();
    out.push_str(&format!("object {}\n", t.object));
    out.push_str(&format!("type {}\n", t.object_type));
    out.push_str(&format!("tag {}\n", t.tag));
    if let Some(ref tagger) = t.tagger {
        out.push_str(&format!("tagger {tagger}\n"));
    }
    out.push('\n');
    // Only add message if non-empty (don't add extra blank line for empty message)
    let msg = t.message.trim_end_matches('\n');
    if !msg.is_empty() {
        out.push_str(msg);
        out.push('\n');
    }
    out.into_bytes()
}

/// Serialize a [`CommitData`] into the raw bytes suitable for storage.
///
/// The caller is responsible for supplying a correctly-formatted `author` and
/// `committer` string (including timestamp and timezone).
///
/// The message body is written exactly as given: `git commit` and `git commit-tree -m`
/// supply a trailing LF; `git commit-tree` reading from stdin or `-F` does not add one.
#[must_use]
pub fn serialize_commit(c: &CommitData) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(format!("tree {}\n", c.tree).as_bytes());
    for p in &c.parents {
        out.extend_from_slice(format!("parent {p}\n").as_bytes());
    }
    out.extend_from_slice(b"author ");
    if c.author_raw.is_empty() {
        out.extend_from_slice(c.author.as_bytes());
    } else {
        out.extend_from_slice(&c.author_raw);
    }
    out.push(b'\n');
    out.extend_from_slice(b"committer ");
    if c.committer_raw.is_empty() {
        out.extend_from_slice(c.committer.as_bytes());
    } else {
        out.extend_from_slice(&c.committer_raw);
    }
    out.push(b'\n');
    if let Some(enc) = &c.encoding {
        out.extend_from_slice(format!("encoding {enc}\n").as_bytes());
    }
    out.push(b'\n');
    if let Some(raw) = &c.raw_message {
        out.extend_from_slice(raw);
    } else if !c.message.is_empty() {
        out.extend_from_slice(c.message.as_bytes());
    }
    out
}
