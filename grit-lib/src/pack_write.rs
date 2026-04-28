//! Minimal PACK v2 writing.
//!
//! This module intentionally starts with correctness-first full-object packs:
//! no deltas, no thin packs, and no reachability walk. Higher-level callers can
//! choose the object IDs to include.

use std::collections::HashSet;
use std::io::Write;

use flate2::write::ZlibEncoder;
use flate2::Compression;
use sha1::{Digest, Sha1};

use crate::error::{Error, Result};
use crate::objects::{parse_commit, parse_tag, parse_tree, ObjectId, ObjectKind};
use crate::storage::ObjectReader;

/// Options for writing PACK files.
#[derive(Clone, Copy, Debug, Default)]
pub struct PackWriteOptions {
    /// Placeholder for future delta support. The current writer always emits
    /// full objects and returns an error when this is set.
    pub use_deltas: bool,
    /// Placeholder for future thin-pack support. The current writer always emits
    /// self-contained packs and returns an error when this is set.
    pub thin: bool,
}

/// Summary of a written pack.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackWriteSummary {
    /// Number of objects written to the pack.
    pub object_count: usize,
    /// SHA-1 trailer of the pack contents.
    pub trailer: [u8; 20],
}

/// Write a PACK v2 containing the full objects named by `object_ids`.
///
/// Duplicate object IDs are skipped while preserving first-seen order.
///
/// # Errors
///
/// Returns an error when an object is missing, writing fails, or unsupported
/// options such as deltas/thin packs are requested.
pub fn write_pack<W: Write>(
    store: &impl ObjectReader,
    object_ids: &[ObjectId],
    out: &mut W,
    options: PackWriteOptions,
) -> Result<PackWriteSummary> {
    if options.use_deltas {
        return Err(Error::Message(
            "delta pack writing is not implemented".to_string(),
        ));
    }
    if options.thin {
        return Err(Error::Message(
            "thin pack writing is not implemented".to_string(),
        ));
    }

    let mut seen = HashSet::new();
    let mut objects = Vec::new();
    for oid in object_ids {
        if seen.insert(*oid) {
            let object = store.read_object(oid)?;
            objects.push(object);
        }
    }

    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"PACK");
    bytes.extend_from_slice(&2u32.to_be_bytes());
    bytes.extend_from_slice(&(objects.len() as u32).to_be_bytes());
    for object in &objects {
        write_object_entry(&mut bytes, object.kind, &object.data)?;
    }
    let trailer = Sha1::digest(&bytes);
    bytes.extend_from_slice(&trailer);
    out.write_all(&bytes).map_err(Error::Io)?;

    let mut trailer_bytes = [0u8; 20];
    trailer_bytes.copy_from_slice(&trailer);
    Ok(PackWriteSummary {
        object_count: objects.len(),
        trailer: trailer_bytes,
    })
}

/// Select objects reachable from `tips` and not reachable from `exclude_tips`.
///
/// The result is in traversal order with duplicates removed. It includes the
/// starting tip objects themselves.
///
/// # Errors
///
/// Returns an error when any traversed object is missing or malformed.
pub fn objects_for_push_pack(
    store: &impl ObjectReader,
    tips: &[ObjectId],
    exclude_tips: &[ObjectId],
) -> Result<Vec<ObjectId>> {
    let excluded = reachable_set(store, exclude_tips)?;
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for tip in tips {
        collect_reachable(store, *tip, &excluded, &mut seen, &mut out)?;
    }
    Ok(out)
}

fn reachable_set(store: &impl ObjectReader, tips: &[ObjectId]) -> Result<HashSet<ObjectId>> {
    let excluded = HashSet::new();
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for tip in tips {
        collect_reachable(store, *tip, &excluded, &mut seen, &mut out)?;
    }
    Ok(seen)
}

fn collect_reachable(
    store: &impl ObjectReader,
    oid: ObjectId,
    excluded: &HashSet<ObjectId>,
    seen: &mut HashSet<ObjectId>,
    out: &mut Vec<ObjectId>,
) -> Result<()> {
    if excluded.contains(&oid) || !seen.insert(oid) {
        return Ok(());
    }
    let object = store.read_object(&oid)?;
    out.push(oid);
    match object.kind {
        ObjectKind::Commit => {
            let commit = parse_commit(&object.data)?;
            collect_reachable(store, commit.tree, excluded, seen, out)?;
            for parent in commit.parents {
                collect_reachable(store, parent, excluded, seen, out)?;
            }
        }
        ObjectKind::Tree => {
            for entry in parse_tree(&object.data)? {
                collect_reachable(store, entry.oid, excluded, seen, out)?;
            }
        }
        ObjectKind::Tag => {
            let tag = parse_tag(&object.data)?;
            collect_reachable(store, tag.object, excluded, seen, out)?;
        }
        ObjectKind::Blob => {}
    }
    Ok(())
}

fn write_object_entry(out: &mut Vec<u8>, kind: ObjectKind, data: &[u8]) -> Result<()> {
    write_type_and_size(out, object_type_code(kind), data.len());
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(data)
        .map_err(|err| Error::Zlib(err.to_string()))?;
    let compressed = encoder
        .finish()
        .map_err(|err| Error::Zlib(err.to_string()))?;
    out.extend_from_slice(&compressed);
    Ok(())
}

fn object_type_code(kind: ObjectKind) -> u8 {
    match kind {
        ObjectKind::Commit => 1,
        ObjectKind::Tree => 2,
        ObjectKind::Blob => 3,
        ObjectKind::Tag => 4,
    }
}

fn write_type_and_size(out: &mut Vec<u8>, type_code: u8, mut size: usize) {
    let mut byte = ((type_code & 0x07) << 4) | ((size & 0x0f) as u8);
    size >>= 4;
    if size != 0 {
        byte |= 0x80;
    }
    out.push(byte);
    while size != 0 {
        let mut next = (size & 0x7f) as u8;
        size >>= 7;
        if size != 0 {
            next |= 0x80;
        }
        out.push(next);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::io::Cursor;

    use crate::objects::{serialize_commit, serialize_tree, CommitData, Object, TreeEntry};
    use crate::storage::{object_id_for, ObjectWriter};
    use crate::unpack_objects::{unpack_objects_into_store, UnpackOptions};

    use super::*;

    #[derive(Default)]
    struct MemoryStore {
        objects: BTreeMap<ObjectId, Object>,
    }

    impl ObjectReader for MemoryStore {
        fn read_object(&self, oid: &ObjectId) -> Result<Object> {
            self.objects
                .get(oid)
                .cloned()
                .ok_or_else(|| Error::ObjectNotFound(oid.to_hex()))
        }
    }

    impl ObjectWriter for MemoryStore {
        fn write_object(&mut self, kind: ObjectKind, data: &[u8]) -> Result<ObjectId> {
            let oid = object_id_for(kind, data);
            self.objects.insert(oid, Object::new(kind, data.to_vec()));
            Ok(oid)
        }
    }

    #[test]
    fn writes_pack_that_unpack_objects_can_read() {
        let mut source = MemoryStore::default();
        let blob = source.write_object(ObjectKind::Blob, b"hello").unwrap();
        let tree = source.write_object(ObjectKind::Tree, b"").unwrap();
        let mut pack = Vec::new();

        let summary = write_pack(
            &source,
            &[blob, tree, blob],
            &mut pack,
            PackWriteOptions::default(),
        )
        .unwrap();

        assert_eq!(summary.object_count, 2);
        assert_eq!(&pack[0..4], b"PACK");
        assert_eq!(u32::from_be_bytes(pack[4..8].try_into().unwrap()), 2);
        assert_eq!(u32::from_be_bytes(pack[8..12].try_into().unwrap()), 2);

        let mut dest = MemoryStore::default();
        let count =
            unpack_objects_into_store(&mut Cursor::new(pack), &mut dest, &UnpackOptions::default())
                .unwrap();
        assert_eq!(count, 2);
        assert_eq!(dest.read_object(&blob).unwrap().data, b"hello");
        assert_eq!(dest.read_object(&tree).unwrap().kind, ObjectKind::Tree);
    }

    #[test]
    fn selects_reachable_objects_excluding_old_tip() {
        let mut source = MemoryStore::default();
        let old_blob = source.write_object(ObjectKind::Blob, b"old").unwrap();
        let old_tree = source
            .write_object(
                ObjectKind::Tree,
                &serialize_tree(&[TreeEntry {
                    mode: 0o100644,
                    name: b"old.txt".to_vec(),
                    oid: old_blob,
                }]),
            )
            .unwrap();
        let old_commit = source
            .write_object(
                ObjectKind::Commit,
                &serialize_commit(&CommitData {
                    tree: old_tree,
                    parents: Vec::new(),
                    author: "A <a@example.com> 1 +0000".to_string(),
                    committer: "C <c@example.com> 1 +0000".to_string(),
                    author_raw: Vec::new(),
                    committer_raw: Vec::new(),
                    encoding: None,
                    message: "old\n".to_string(),
                    raw_message: None,
                }),
            )
            .unwrap();
        let new_blob = source.write_object(ObjectKind::Blob, b"new").unwrap();
        let new_tree = source
            .write_object(
                ObjectKind::Tree,
                &serialize_tree(&[
                    TreeEntry {
                        mode: 0o100644,
                        name: b"old.txt".to_vec(),
                        oid: old_blob,
                    },
                    TreeEntry {
                        mode: 0o100644,
                        name: b"new.txt".to_vec(),
                        oid: new_blob,
                    },
                ]),
            )
            .unwrap();
        let new_commit = source
            .write_object(
                ObjectKind::Commit,
                &serialize_commit(&CommitData {
                    tree: new_tree,
                    parents: vec![old_commit],
                    author: "A <a@example.com> 2 +0000".to_string(),
                    committer: "C <c@example.com> 2 +0000".to_string(),
                    author_raw: Vec::new(),
                    committer_raw: Vec::new(),
                    encoding: None,
                    message: "new\n".to_string(),
                    raw_message: None,
                }),
            )
            .unwrap();

        let selected = objects_for_push_pack(&source, &[new_commit], &[old_commit]).unwrap();

        assert!(selected.contains(&new_commit));
        assert!(selected.contains(&new_tree));
        assert!(selected.contains(&new_blob));
        assert!(!selected.contains(&old_commit));
        assert!(!selected.contains(&old_tree));
        assert!(!selected.contains(&old_blob));
    }
}
