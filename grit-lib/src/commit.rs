//! Commit object writing helpers.

use crate::error::{Error, Result};
use crate::objects::{serialize_commit, CommitData, ObjectId, ObjectKind};
use crate::storage::ObjectWriter;

/// Write a commit object to `store` and return its object ID.
///
/// This validates that the referenced tree exists and is a tree object, and
/// that every listed parent exists and is a commit object. The caller provides
/// complete author and committer identity lines, including timestamp and
/// timezone.
///
/// # Errors
///
/// Returns an error when the tree or a parent is missing, has the wrong object
/// kind, or when the store cannot write the new commit object.
pub fn write_commit(store: &mut impl ObjectWriter, commit: &CommitData) -> Result<ObjectId> {
    ensure_object_kind(store, &commit.tree, ObjectKind::Tree, "commit tree")?;
    for parent in &commit.parents {
        ensure_object_kind(store, parent, ObjectKind::Commit, "commit parent")?;
    }
    store.write_object(ObjectKind::Commit, &serialize_commit(commit))
}

fn ensure_object_kind(
    store: &impl ObjectWriter,
    oid: &ObjectId,
    expected: ObjectKind,
    label: &str,
) -> Result<()> {
    let object = store.read_object(oid)?;
    if object.kind != expected {
        return Err(Error::CorruptObject(format!(
            "{label} {oid} is {}, expected {expected}",
            object.kind
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use tempfile::TempDir;

    use super::*;
    use crate::odb::Odb;
    use crate::storage::ObjectReader;

    #[test]
    fn writes_commit_through_object_store() {
        let temp_dir = TempDir::new().expect("temp dir");
        let mut odb = Odb::new(temp_dir.path());
        let tree = odb
            .write_object(ObjectKind::Tree, b"")
            .expect("write empty tree");
        let commit = CommitData {
            tree,
            parents: Vec::new(),
            author: "A U Thor <author@example.com> 1 +0000".to_string(),
            committer: "C O Mitter <committer@example.com> 2 +0000".to_string(),
            author_raw: Vec::new(),
            committer_raw: Vec::new(),
            encoding: None,
            message: "initial\n".to_string(),
            raw_message: None,
        };

        let oid = write_commit(&mut odb, &commit).expect("write commit");
        let stored = odb.read_object(&oid).expect("read commit");

        assert_eq!(stored.kind, ObjectKind::Commit);
        assert!(String::from_utf8_lossy(&stored.data).contains("initial\n"));
    }
}
