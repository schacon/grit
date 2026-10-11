//! Shallow `deepen 1` on a single-commit repository must still advertise a border.

use grit_lib::objects::{ObjectId, ObjectKind};
use grit_lib::repo::Repository;
use grit_lib::rev_list::shallow_grafts_for_upload_pack_deepen;

#[test]
fn deepen_one_on_root_commit_returns_shallow_border() {
    let dir = tempfile::tempdir().expect("tempdir");
    grit_lib::repo::init_bare_clone_minimal(
        dir.path(),
        "main",
        grit_lib::ref_storage::RefStorageFormat::Files,
    )
    .expect("init");
    let repo = Repository::open(dir.path(), None).expect("open");
    let tree = ObjectId::from_hex("4b825dc642cb6eb9a060e54bf8d69288fbee4904").expect("empty tree");
    let commit_body = format!(
        "tree {}\nauthor T <t@x> 1 +0000\ncommitter T <t@x> 1 +0000\n\nonly\n",
        tree.to_hex()
    );
    let tip = repo
        .odb
        .write_loose_materialize(ObjectKind::Commit, commit_body.as_bytes())
        .expect("commit");
    grit_lib::refs::write_ref(dir.path(), "refs/heads/main", &tip).expect("ref");

    let borders = shallow_grafts_for_upload_pack_deepen(&repo, &[tip], &[], 1);
    assert_eq!(
        borders,
        vec![tip],
        "root commit with no parents is the shallow border for deepen 1"
    );
}
