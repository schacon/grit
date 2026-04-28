//! Browser-oriented WASM entry points for Grit.
//!
//! This crate starts with a small in-memory repository core so the WASM build
//! can grow around portable Git data structures before browser HTTP and
//! persistent storage are wired in.

#[cfg(target_arch = "wasm32")]
pub mod bindings;
pub mod browser_http;
pub mod persistence;
pub mod remote;

use std::collections::BTreeMap;

use grit_lib::commit::write_commit;
use grit_lib::error::{Error, Result};
use grit_lib::objects::{
    serialize_tree, tree_entry_cmp, CommitData, Object, ObjectId, ObjectKind, TreeEntry,
};
use grit_lib::pack_write::{objects_for_push_pack, write_pack, PackWriteOptions};
use grit_lib::storage::{
    object_id_for, pack_id_for, ObjectReader, ObjectWriter, PackDescriptor, PackId, PackReader,
    PackWriter, RefReader, RefWriter,
};

/// An in-memory object store used by the first WASM implementation milestone.
#[derive(Default)]
pub struct MemoryObjectStore {
    objects: BTreeMap<ObjectId, Object>,
}

impl MemoryObjectStore {
    /// Create an empty in-memory object store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Return the number of objects currently stored.
    #[must_use]
    pub fn len(&self) -> usize {
        self.objects.len()
    }

    /// Return whether the store currently contains no objects.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }

    /// Return true if `oid` is present in the store.
    #[must_use]
    pub fn contains(&self, oid: &ObjectId) -> bool {
        self.has_object(oid).unwrap_or(false)
    }

    /// Read an object from the store.
    #[must_use]
    pub fn read(&self, oid: &ObjectId) -> Option<Object> {
        self.read_object(oid).ok()
    }

    /// Write an object and return its Git object ID.
    pub fn write(&mut self, kind: ObjectKind, data: &[u8]) -> ObjectId {
        self.write_object(kind, data)
            .unwrap_or_else(|_| object_id_for(kind, data))
    }
}

impl ObjectReader for MemoryObjectStore {
    fn read_object(&self, oid: &ObjectId) -> Result<Object> {
        self.objects
            .get(oid)
            .cloned()
            .ok_or_else(|| Error::ObjectNotFound(oid.to_hex()))
    }

    fn has_object(&self, oid: &ObjectId) -> Result<bool> {
        Ok(self.objects.contains_key(oid))
    }
}

impl ObjectWriter for MemoryObjectStore {
    fn write_object(&mut self, kind: ObjectKind, data: &[u8]) -> Result<ObjectId> {
        let oid = object_id_for(kind, data);
        self.objects.insert(
            oid,
            Object {
                kind,
                data: data.to_vec(),
            },
        );
        Ok(oid)
    }
}

/// An in-memory reference store used by the first WASM implementation milestone.
#[derive(Default)]
pub struct MemoryRefStore {
    refs: BTreeMap<String, ObjectId>,
    symbolic_refs: BTreeMap<String, String>,
}

impl MemoryRefStore {
    /// Create an empty in-memory reference store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Return the number of direct references currently stored.
    #[must_use]
    pub fn len(&self) -> usize {
        self.refs.len()
    }

    /// Return whether the store currently contains no direct or symbolic refs.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.refs.is_empty() && self.symbolic_refs.is_empty()
    }

    fn resolve_ref_inner(&self, name: &str, depth: usize) -> Result<Option<ObjectId>> {
        if depth > 10 {
            return Err(Error::InvalidRef(format!("ref symlink too deep: {name}")));
        }
        if let Some(oid) = self.refs.get(name) {
            return Ok(Some(*oid));
        }
        let Some(target) = self.symbolic_refs.get(name) else {
            return Ok(None);
        };
        self.resolve_ref_inner(target, depth + 1)
    }
}

impl RefReader for MemoryRefStore {
    fn resolve_ref(&self, name: &str) -> Result<Option<ObjectId>> {
        self.resolve_ref_inner(name, 0)
    }

    fn read_symbolic_ref(&self, name: &str) -> Result<Option<String>> {
        Ok(self.symbolic_refs.get(name).cloned())
    }

    fn list_refs(&self, prefix: &str) -> Result<Vec<(String, ObjectId)>> {
        let mut refs = Vec::new();
        for name in self
            .refs
            .keys()
            .chain(self.symbolic_refs.keys())
            .filter(|name| name.starts_with(prefix))
        {
            if let Some(oid) = self.resolve_ref(name)? {
                refs.push((name.clone(), oid));
            }
        }
        refs.sort_by(|a, b| a.0.cmp(&b.0));
        refs.dedup_by(|a, b| a.0 == b.0);
        Ok(refs)
    }
}

impl RefWriter for MemoryRefStore {
    fn set_ref(&mut self, name: &str, oid: ObjectId) -> Result<()> {
        self.symbolic_refs.remove(name);
        self.refs.insert(name.to_string(), oid);
        Ok(())
    }

    fn set_symbolic_ref(&mut self, name: &str, target: &str) -> Result<()> {
        self.refs.remove(name);
        self.symbolic_refs
            .insert(name.to_string(), target.to_string());
        Ok(())
    }

    fn delete_ref(&mut self, name: &str) -> Result<()> {
        self.refs.remove(name);
        self.symbolic_refs.remove(name);
        Ok(())
    }
}

#[derive(Clone)]
struct StoredPack {
    bytes: Vec<u8>,
    promisor: bool,
}

/// An in-memory raw pack store used by early browser fetch work.
#[derive(Default)]
pub struct MemoryPackStore {
    packs: BTreeMap<PackId, StoredPack>,
}

impl MemoryPackStore {
    /// Create an empty in-memory pack store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Return the number of packs currently stored.
    #[must_use]
    pub fn len(&self) -> usize {
        self.packs.len()
    }

    /// Return whether the store currently contains no packs.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.packs.is_empty()
    }
}

impl PackReader for MemoryPackStore {
    fn list_packs(&self) -> Result<Vec<PackDescriptor>> {
        Ok(self
            .packs
            .iter()
            .map(|(id, pack)| PackDescriptor {
                id: id.clone(),
                byte_len: pack.bytes.len(),
                promisor: pack.promisor,
            })
            .collect())
    }

    fn read_pack(&self, id: &PackId) -> Result<Option<Vec<u8>>> {
        Ok(self.packs.get(id).map(|pack| pack.bytes.clone()))
    }
}

impl PackWriter for MemoryPackStore {
    fn add_pack(&mut self, bytes: Vec<u8>, promisor: bool) -> Result<PackId> {
        let id = pack_id_for(&bytes);
        self.packs
            .insert(id.clone(), StoredPack { bytes, promisor });
        Ok(id)
    }

    fn delete_pack(&mut self, id: &PackId) -> Result<()> {
        self.packs.remove(id);
        Ok(())
    }
}

/// Metadata for a promisor remote.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PromisorRemote {
    /// Remote URL used for future promised-object hydration.
    pub url: String,
    /// Partial clone filter associated with this remote, for example `blob:none`.
    pub filter: String,
}

/// A staged file entry in the browser repository.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StagedFile {
    /// Repository-relative path using `/` separators.
    pub path: String,
    /// Git tree mode for the staged file.
    pub mode: u32,
    /// Blob object ID for the staged contents.
    pub oid: ObjectId,
}

/// Browser-facing repository state for early WASM work.
#[derive(Default)]
pub struct WasmRepository {
    objects: MemoryObjectStore,
    refs: MemoryRefStore,
    packs: MemoryPackStore,
    promisor_remotes: BTreeMap<String, PromisorRemote>,
    staged: BTreeMap<String, StagedFile>,
}

impl WasmRepository {
    /// Create an empty in-memory repository.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Return the package version compiled into this WASM crate.
    #[must_use]
    pub fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    /// Return the number of stored objects.
    #[must_use]
    pub fn object_count(&self) -> usize {
        self.objects.len()
    }

    /// Return the number of stored raw packs.
    #[must_use]
    pub fn pack_count(&self) -> usize {
        self.packs.len()
    }

    /// Return the number of configured promisor remotes.
    #[must_use]
    pub fn promisor_remote_count(&self) -> usize {
        self.promisor_remotes.len()
    }

    /// Return the number of staged file entries.
    #[must_use]
    pub fn staged_count(&self) -> usize {
        self.staged.len()
    }

    /// Store a blob object and return its hex object ID.
    pub fn write_blob(&mut self, data: &[u8]) -> String {
        self.objects.write(ObjectKind::Blob, data).to_hex()
    }

    /// Store raw tree object data and return its hex object ID.
    ///
    /// This is a temporary low-level API for early WASM commit work. Higher
    /// level browser index/tree builders will replace direct tree byte writes.
    pub fn write_tree_raw(&mut self, data: &[u8]) -> String {
        self.objects.write(ObjectKind::Tree, data).to_hex()
    }

    /// Stage a regular file from browser-provided bytes.
    ///
    /// `path` must be repository-relative and use `/` separators. Set
    /// `executable` to store mode `100755`; otherwise mode `100644` is used.
    ///
    /// # Errors
    ///
    /// Returns an error when the path is empty or contains `.` / `..`
    /// components.
    pub fn stage_file(&mut self, path: &str, data: &[u8], executable: bool) -> Result<String> {
        let path = normalize_repo_path(path)?;
        let oid = self.objects.write(ObjectKind::Blob, data);
        let mode = if executable {
            MODE_EXECUTABLE
        } else {
            MODE_REGULAR
        };
        self.staged
            .insert(path.clone(), StagedFile { path, mode, oid });
        Ok(oid.to_hex())
    }

    /// Build tree objects from staged files and return the root tree ID.
    ///
    /// # Errors
    ///
    /// Returns an error when there are no staged files or the object store
    /// cannot write a tree.
    pub fn write_tree_from_staged(&mut self) -> Result<String> {
        if self.staged.is_empty() {
            return Err(Error::IndexError("no staged files".to_string()));
        }
        let entries = self.staged.values().cloned().collect::<Vec<_>>();
        let oid = build_tree_from_staged(&mut self.objects, &entries, "")?;
        Ok(oid.to_hex())
    }

    /// Return true if an object with the provided hex ID exists.
    #[must_use]
    pub fn has_object_hex(&self, oid_hex: &str) -> bool {
        ObjectId::from_hex(oid_hex)
            .map(|oid| self.objects.contains(&oid))
            .unwrap_or(false)
    }

    /// Read a blob object by hex ID.
    #[must_use]
    pub fn read_blob(&self, oid_hex: &str) -> Option<Vec<u8>> {
        let oid = ObjectId::from_hex(oid_hex).ok()?;
        let object = self.objects.read(&oid)?;
        (object.kind == ObjectKind::Blob).then(|| object.data.clone())
    }

    /// Write a commit object from explicit caller-provided commit metadata.
    ///
    /// `author` and `committer` must be complete Git identity header payloads,
    /// including timestamp and timezone, for example
    /// `"A U Thor <author@example.com> 1 +0000"`.
    #[must_use]
    pub fn write_commit(
        &mut self,
        tree_hex: &str,
        parent_hexes: Vec<String>,
        author: &str,
        committer: &str,
        message: &str,
    ) -> Option<String> {
        let tree = ObjectId::from_hex(tree_hex).ok()?;
        let parents = parent_hexes
            .iter()
            .map(|parent| ObjectId::from_hex(parent))
            .collect::<Result<Vec<_>>>()
            .ok()?;
        let commit = CommitData {
            tree,
            parents,
            author: author.to_string(),
            committer: committer.to_string(),
            author_raw: Vec::new(),
            committer_raw: Vec::new(),
            encoding: None,
            message: message.to_string(),
            raw_message: None,
        };
        write_commit(&mut self.objects, &commit)
            .ok()
            .map(|oid| oid.to_hex())
    }

    /// Commit the staged files and update the current branch.
    ///
    /// `author` and `committer` must be complete Git identity header payloads,
    /// including timestamp and timezone. The current `HEAD` commit, when set,
    /// is used as the single parent. If `HEAD` is not set, this creates
    /// `refs/heads/main` and points `HEAD` at it.
    ///
    /// # Errors
    ///
    /// Returns an error when tree or commit writing fails, or when `HEAD`
    /// points at a non-commit object.
    pub fn commit_staged(
        &mut self,
        message: &str,
        author: &str,
        committer: &str,
    ) -> Result<String> {
        let tree_hex = self.write_tree_from_staged()?;
        let tree = ObjectId::from_hex(&tree_hex)?;
        let head_target = self.refs.read_symbolic_ref("HEAD")?;
        let parent = self.refs.resolve_ref("HEAD")?;
        if let Some(parent_oid) = parent {
            let parent_object = self.objects.read_object(&parent_oid)?;
            if parent_object.kind != ObjectKind::Commit {
                return Err(Error::CorruptObject(
                    "HEAD does not point to a commit".to_string(),
                ));
            }
        }
        let commit = CommitData {
            tree,
            parents: parent.into_iter().collect(),
            author: author.to_string(),
            committer: committer.to_string(),
            author_raw: Vec::new(),
            committer_raw: Vec::new(),
            encoding: None,
            message: message.to_string(),
            raw_message: None,
        };
        let commit_oid = write_commit(&mut self.objects, &commit)?;
        let target_ref = match head_target {
            Some(target) => target,
            None => {
                let target = "refs/heads/main".to_string();
                self.refs.set_symbolic_ref("HEAD", &target)?;
                target
            }
        };
        self.refs.set_ref(&target_ref, commit_oid)?;
        self.staged.clear();
        Ok(commit_oid.to_hex())
    }

    /// Set a reference to the object named by `oid_hex`.
    ///
    /// Returns false when `oid_hex` is not a valid SHA-1 object ID.
    pub fn set_ref(&mut self, name: &str, oid_hex: &str) -> bool {
        let Ok(oid) = ObjectId::from_hex(oid_hex) else {
            return false;
        };
        self.refs.set_ref(name, oid).is_ok()
    }

    /// Read a reference as a hex object ID.
    #[must_use]
    pub fn get_ref(&self, name: &str) -> Option<String> {
        self.refs
            .resolve_ref(name)
            .ok()
            .flatten()
            .map(|oid| oid.to_hex())
    }

    /// Set symbolic `HEAD` to a reference name.
    pub fn set_head(&mut self, refname: &str) {
        let _ = self.refs.set_symbolic_ref("HEAD", refname);
    }

    /// Return the symbolic `HEAD` reference name, if one has been set.
    #[must_use]
    pub fn head(&self) -> Option<String> {
        self.refs.read_symbolic_ref("HEAD").ok().flatten()
    }

    /// Store raw pack bytes and return the pack ID.
    pub fn add_pack(&mut self, bytes: Vec<u8>, promisor: bool) -> String {
        self.packs
            .add_pack(bytes, promisor)
            .map(|id| id.as_str().to_string())
            .unwrap_or_default()
    }

    /// Return whether the pack identified by `id` is marked as promisor.
    #[must_use]
    pub fn pack_is_promisor(&self, id: &str) -> bool {
        let id = PackId::new(id);
        self.packs
            .list_packs()
            .unwrap_or_default()
            .into_iter()
            .any(|pack| pack.id == id && pack.promisor)
    }

    /// Read raw pack bytes by pack ID.
    #[must_use]
    pub fn read_pack(&self, id: &str) -> Option<Vec<u8>> {
        self.packs.read_pack(&PackId::new(id)).ok().flatten()
    }

    /// Write a non-delta PACK v2 containing the explicit object IDs.
    ///
    /// The returned bytes can be used as receive-pack input once push command
    /// construction is wired in.
    pub fn write_pack_for_oids(&self, oid_hexes: Vec<String>) -> Result<Vec<u8>> {
        let oids = oid_hexes
            .iter()
            .map(|oid| ObjectId::from_hex(oid))
            .collect::<Result<Vec<_>>>()?;
        let mut out = Vec::new();
        let _summary = write_pack(&self.objects, &oids, &mut out, PackWriteOptions::default())?;
        Ok(out)
    }

    /// Write a non-delta PACK v2 for pushing `tip_hex`.
    ///
    /// Objects reachable from `old_tip_hex`, when provided, are excluded from
    /// the pack.
    pub fn write_pack_for_push(
        &self,
        tip_hex: &str,
        old_tip_hex: Option<String>,
    ) -> Result<Vec<u8>> {
        let tip = ObjectId::from_hex(tip_hex)?;
        let old_tips = old_tip_hex
            .as_deref()
            .map(ObjectId::from_hex)
            .transpose()?
            .into_iter()
            .collect::<Vec<_>>();
        let objects = objects_for_push_pack(&self.objects, &[tip], &old_tips)?;
        let mut out = Vec::new();
        let _summary = write_pack(
            &self.objects,
            &objects,
            &mut out,
            PackWriteOptions::default(),
        )?;
        Ok(out)
    }

    /// Record promisor metadata for `name`.
    pub fn set_promisor_remote(&mut self, name: &str, url: &str, filter: &str) {
        self.promisor_remotes.insert(
            name.to_string(),
            PromisorRemote {
                url: url.to_string(),
                filter: filter.to_string(),
            },
        );
    }

    /// Return promisor remote metadata for `name`.
    #[must_use]
    pub fn promisor_remote(&self, name: &str) -> Option<PromisorRemote> {
        self.promisor_remotes.get(name).cloned()
    }
}

const MODE_REGULAR: u32 = 0o100644;
const MODE_EXECUTABLE: u32 = 0o100755;
const MODE_TREE: u32 = 0o040000;

fn normalize_repo_path(path: &str) -> Result<String> {
    let trimmed = path.trim_matches('/');
    if trimmed.is_empty() {
        return Err(Error::PathError("path is empty".to_string()));
    }
    if trimmed
        .split('/')
        .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(Error::PathError(format!("invalid repository path: {path}")));
    }
    Ok(trimmed.to_string())
}

fn build_tree_from_staged(
    store: &mut MemoryObjectStore,
    entries: &[StagedFile],
    prefix: &str,
) -> Result<ObjectId> {
    let mut files = Vec::new();
    let mut dirs: BTreeMap<String, ()> = BTreeMap::new();
    for entry in entries {
        let Some(rel) = relative_staged_path(&entry.path, prefix) else {
            continue;
        };
        if let Some((dir, _rest)) = rel.split_once('/') {
            dirs.insert(dir.to_string(), ());
        } else {
            files.push(TreeEntry {
                mode: entry.mode,
                name: rel.as_bytes().to_vec(),
                oid: entry.oid,
            });
        }
    }
    for dir in dirs.keys() {
        let sub_prefix = if prefix.is_empty() {
            dir.to_string()
        } else {
            format!("{prefix}/{dir}")
        };
        let oid = build_tree_from_staged(store, entries, &sub_prefix)?;
        files.push(TreeEntry {
            mode: MODE_TREE,
            name: dir.as_bytes().to_vec(),
            oid,
        });
    }
    files
        .sort_by(|a, b| tree_entry_cmp(&a.name, a.mode == MODE_TREE, &b.name, b.mode == MODE_TREE));
    let data = serialize_tree(&files);
    store.write_object(ObjectKind::Tree, &data)
}

fn relative_staged_path<'a>(path: &'a str, prefix: &str) -> Option<&'a str> {
    if prefix.is_empty() {
        return Some(path);
    }
    let rest = path.strip_prefix(prefix)?;
    rest.strip_prefix('/')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_git_blob_object_ids() {
        let mut repo = WasmRepository::new();
        let oid = repo.write_blob(b"hello\n");

        assert_eq!(oid, "ce013625030ba8dba906f756967f9e9ca394464a");
        assert!(repo.has_object_hex(&oid));
        assert_eq!(repo.read_blob(&oid), Some(b"hello\n".to_vec()));
    }

    #[test]
    fn tracks_refs_and_head() {
        let mut repo = WasmRepository::new();
        let oid = repo.write_blob(b"content");

        assert!(repo.set_ref("refs/heads/main", &oid));
        repo.set_head("refs/heads/main");

        assert_eq!(repo.get_ref("refs/heads/main"), Some(oid));
        assert_eq!(repo.head(), Some("refs/heads/main".to_string()));
    }

    #[test]
    fn resolves_symbolic_refs() {
        let mut refs = MemoryRefStore::new();
        let oid = match ObjectId::from_hex("67bf698f3ab735e92fb011a99cff3497c44d30c1") {
            Ok(oid) => oid,
            Err(err) => panic!("valid test object ID failed to parse: {err}"),
        };

        assert!(refs.set_ref("refs/heads/main", oid).is_ok());
        assert!(refs.set_symbolic_ref("HEAD", "refs/heads/main").is_ok());

        assert_eq!(refs.resolve_ref("HEAD").ok().flatten(), Some(oid));
        assert_eq!(
            refs.list_refs("refs/heads/").unwrap_or_default(),
            vec![("refs/heads/main".to_string(), oid)]
        );
    }

    #[test]
    fn stores_promisor_packs() {
        let mut repo = WasmRepository::new();
        let id = repo.add_pack(b"PACK test bytes".to_vec(), true);

        assert_eq!(repo.pack_count(), 1);
        assert!(repo.pack_is_promisor(&id));
        assert_eq!(repo.read_pack(&id), Some(b"PACK test bytes".to_vec()));
    }

    #[test]
    fn stores_promisor_remote_metadata() {
        let mut repo = WasmRepository::new();
        repo.set_promisor_remote("origin", "https://example.com/repo.git", "blob:none");

        let remote = repo.promisor_remote("origin");

        assert_eq!(repo.promisor_remote_count(), 1);
        assert_eq!(
            remote,
            Some(PromisorRemote {
                url: "https://example.com/repo.git".to_string(),
                filter: "blob:none".to_string(),
            })
        );
    }

    #[test]
    fn writes_commit_from_existing_tree() {
        let mut repo = WasmRepository::new();
        let tree = repo.write_tree_raw(b"");
        let commit = repo.write_commit(
            &tree,
            Vec::new(),
            "A U Thor <author@example.com> 1 +0000",
            "C O Mitter <committer@example.com> 2 +0000",
            "initial\n",
        );

        let commit = match commit {
            Some(commit) => commit,
            None => panic!("commit should be written"),
        };
        assert!(repo.has_object_hex(&commit));
    }

    #[test]
    fn builds_tree_from_staged_files() {
        let mut repo = WasmRepository::new();
        let readme = repo.stage_file("README.md", b"readme", false).unwrap();
        let lib = repo.stage_file("src/lib.rs", b"lib", true).unwrap();

        let tree = repo.write_tree_from_staged().unwrap();
        let tree_oid = ObjectId::from_hex(&tree).unwrap();
        let root = repo.objects.read_object(&tree_oid).unwrap();
        let entries = grit_lib::objects::parse_tree(&root.data).unwrap();

        assert_eq!(repo.staged_count(), 2);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, b"README.md");
        assert_eq!(entries[0].oid.to_hex(), readme);
        assert_eq!(entries[1].name, b"src");

        let src = repo.objects.read_object(&entries[1].oid).unwrap();
        let src_entries = grit_lib::objects::parse_tree(&src.data).unwrap();
        assert_eq!(src_entries.len(), 1);
        assert_eq!(src_entries[0].name, b"lib.rs");
        assert_eq!(src_entries[0].mode, MODE_EXECUTABLE);
        assert_eq!(src_entries[0].oid.to_hex(), lib);
    }

    #[test]
    fn commit_staged_updates_current_branch() {
        let mut repo = WasmRepository::new();
        repo.set_head("refs/heads/main");
        repo.stage_file("README.md", b"readme", false).unwrap();

        let commit = repo
            .commit_staged(
                "initial\n",
                "A U Thor <author@example.com> 1 +0000",
                "C O Mitter <committer@example.com> 2 +0000",
            )
            .unwrap();

        assert_eq!(repo.get_ref("refs/heads/main"), Some(commit.clone()));
        assert_eq!(repo.head(), Some("refs/heads/main".to_string()));
        assert_eq!(repo.staged_count(), 0);
        let commit_oid = ObjectId::from_hex(&commit).unwrap();
        let object = repo.objects.read_object(&commit_oid).unwrap();
        assert_eq!(object.kind, ObjectKind::Commit);
    }

    #[test]
    fn writes_pack_for_explicit_oids() {
        let mut repo = WasmRepository::new();
        let blob = repo.write_blob(b"hello");

        let pack = repo.write_pack_for_oids(vec![blob]).unwrap();

        assert_eq!(&pack[0..4], b"PACK");
        assert_eq!(u32::from_be_bytes(pack[4..8].try_into().unwrap()), 2);
        assert_eq!(u32::from_be_bytes(pack[8..12].try_into().unwrap()), 1);
    }

    #[test]
    fn writes_pack_for_push_excluding_parent_history() {
        let mut repo = WasmRepository::new();
        repo.set_head("refs/heads/main");
        repo.stage_file("old.txt", b"old", false).unwrap();
        let old_commit = repo
            .commit_staged(
                "old\n",
                "A U Thor <author@example.com> 1 +0000",
                "C O Mitter <committer@example.com> 1 +0000",
            )
            .unwrap();
        repo.stage_file("new.txt", b"new", false).unwrap();
        let new_commit = repo
            .commit_staged(
                "new\n",
                "A U Thor <author@example.com> 2 +0000",
                "C O Mitter <committer@example.com> 2 +0000",
            )
            .unwrap();

        let pack = repo
            .write_pack_for_push(&new_commit, Some(old_commit))
            .unwrap();

        assert_eq!(&pack[0..4], b"PACK");
        assert_eq!(u32::from_be_bytes(pack[4..8].try_into().unwrap()), 2);
        assert!(u32::from_be_bytes(pack[8..12].try_into().unwrap()) >= 3);
    }

    #[test]
    fn validates_staged_paths() {
        assert_eq!(normalize_repo_path("/src/lib.rs").unwrap(), "src/lib.rs");
        assert!(normalize_repo_path("").is_err());
        assert!(normalize_repo_path("src/../secret").is_err());
    }
}
