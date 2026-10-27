//! Deterministic mapped-pack fixture shared by descriptor-pressure regressions.

use std::fs;
use std::io;

use super::{
    hash_loose_object, write_pack_and_index, HashAlgo, IndexPackOptions, ObjectKind, PackBuilder,
    RepoFixture,
};

/// Create `count` packs, each with a random blob, tree, commit and annotated tag.
///
/// Packs exceed the owned-buffer threshold. Commits form one history and all tags
/// stay loose, so reading decorations must visit every mapped pack. Returns the
/// repository plus `(tag oid, commit oid)` pairs; reports fixture I/O failures.
pub fn many_tag_packs(count: usize) -> io::Result<(RepoFixture, Vec<(String, String)>)> {
    let repo = RepoFixture::init(HashAlgo::Sha1)?;
    let mut refs = Vec::new();
    let mut state = 4751u64;
    fs::create_dir_all(repo.path().join(".git/refs/tags"))?;
    for i in 0..count {
        let mut blob = vec![0u8; 12_000];
        for byte in &mut blob {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            *byte = state as u8;
        }
        let blob_oid = hash_loose_object(HashAlgo::Sha1, "blob", &blob);
        let mut tree = b"100644 data\0".to_vec();
        tree.extend(hex::decode(blob_oid).map_err(io::Error::other)?);
        let tree_oid = hash_loose_object(HashAlgo::Sha1, "tree", &tree);
        let parent = refs
            .last()
            .map(|(_, oid)| format!("parent {oid}\n"))
            .unwrap_or_default();
        let commit = format!("tree {tree_oid}\n{parent}author T <t@example.com> {} +0000\ncommitter T <t@example.com> {} +0000\n\ncommit {i}\n", 1700000000 + i, 1700000000 + i);
        let commit_oid = hash_loose_object(HashAlgo::Sha1, "commit", commit.as_bytes());
        let tag = format!("object {commit_oid}\ntype commit\ntag t{i}\ntagger T <t@example.com> 1700000000 +0000\n\ntag {i}\n");
        let tag_oid = hash_loose_object(HashAlgo::Sha1, "tag", tag.as_bytes());
        let mut pack = PackBuilder::new(HashAlgo::Sha1);
        pack.add_full(ObjectKind::Blob, &blob);
        pack.add_full(ObjectKind::Tree, &tree);
        pack.add_full(ObjectKind::Commit, commit.as_bytes());
        pack.add_full(ObjectKind::Tag, tag.as_bytes());
        let pack = pack.build();
        assert!(pack.bytes.len() > 8192);
        let stem = format!("pack-{}", hex::encode(&pack.bytes[pack.bytes.len() - 20..]));
        let installed = write_pack_and_index(
            &repo.objects_dir(),
            &stem,
            &pack.bytes,
            HashAlgo::Sha1,
            &IndexPackOptions::default(),
        );
        if !installed.index_ok {
            return Err(io::Error::other(installed.index_stderr));
        }
        fs::write(
            repo.path().join(format!(".git/refs/tags/t{i}")),
            format!("{tag_oid}\n"),
        )?;
        refs.push((tag_oid, commit_oid));
    }
    if let Some((_, oid)) = refs.last() {
        fs::write(repo.path().join(".git/refs/heads/main"), format!("{oid}\n"))?;
        fs::write(repo.path().join(".git/HEAD"), "ref: refs/heads/main\n")?;
    }
    let fsck = repo.git(&["fsck", "--strict"]);
    if !fsck.ok {
        return Err(io::Error::other(fsck.stderr));
    }
    Ok((repo, refs))
}
