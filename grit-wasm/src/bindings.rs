//! `wasm-bindgen` exports for browser callers.

use wasm_bindgen::prelude::*;

use grit_lib::objects::parse_commit;
use grit_lib::storage::{ObjectReader, RefReader};

use crate::WasmRepository;

/// Browser-facing in-memory Grit repository.
#[wasm_bindgen]
pub struct BrowserRepository {
    inner: WasmRepository,
}

#[wasm_bindgen]
impl BrowserRepository {
    /// Create an empty in-memory repository.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            inner: WasmRepository::new(),
        }
    }

    /// Return the compiled package version.
    #[wasm_bindgen]
    pub fn version() -> String {
        WasmRepository::version().to_string()
    }

    /// Return the number of stored objects.
    #[wasm_bindgen(js_name = objectCount)]
    pub fn object_count(&self) -> usize {
        self.inner.object_count()
    }

    /// Return the number of staged file entries.
    #[wasm_bindgen(js_name = stagedCount)]
    pub fn staged_count(&self) -> usize {
        self.inner.staged_count()
    }

    /// Return the number of stored raw packs.
    #[wasm_bindgen(js_name = packCount)]
    pub fn pack_count(&self) -> usize {
        self.inner.pack_count()
    }

    /// Return whether an object exists.
    #[wasm_bindgen(js_name = hasObject)]
    pub fn has_object(&self, oid_hex: &str) -> bool {
        self.inner.has_object_hex(oid_hex)
    }

    /// Write a blob and return its hex object ID.
    #[wasm_bindgen(js_name = writeBlob)]
    pub fn write_blob(&mut self, data: &[u8]) -> String {
        self.inner.write_blob(data)
    }

    /// Read a blob by object ID.
    #[wasm_bindgen(js_name = readBlob)]
    pub fn read_blob(&self, oid_hex: &str) -> Result<Vec<u8>, JsValue> {
        self.inner
            .read_blob(oid_hex)
            .ok_or_else(|| JsValue::from_str("blob not found"))
    }

    /// Stage a file from bytes and return the blob object ID.
    #[wasm_bindgen(js_name = stageFile)]
    pub fn stage_file(
        &mut self,
        path: &str,
        data: &[u8],
        executable: bool,
    ) -> Result<String, JsValue> {
        self.inner
            .stage_file(path, data, executable)
            .map_err(|err| JsValue::from_str(&err.to_string()))
    }

    /// Commit staged files and update the current branch.
    #[wasm_bindgen(js_name = commitStaged)]
    pub fn commit_staged(
        &mut self,
        message: &str,
        author: &str,
        committer: &str,
    ) -> Result<String, JsValue> {
        self.inner
            .commit_staged(message, author, committer)
            .map_err(|err| JsValue::from_str(&err.to_string()))
    }

    /// Set symbolic `HEAD`.
    #[wasm_bindgen(js_name = setHead)]
    pub fn set_head(&mut self, refname: &str) {
        self.inner.set_head(refname);
    }

    /// Return symbolic `HEAD`, if set.
    #[wasm_bindgen]
    pub fn head(&self) -> Option<String> {
        self.inner.head()
    }

    /// Return a ref's object ID, if it resolves.
    #[wasm_bindgen(js_name = getRef)]
    pub fn get_ref(&self, name: &str) -> Option<String> {
        self.inner.get_ref(name)
    }

    /// Write a non-delta pack for an explicit object list.
    #[wasm_bindgen(js_name = writePackForOids)]
    pub fn write_pack_for_oids(&self, oid_hexes: Vec<String>) -> Result<Vec<u8>, JsValue> {
        self.inner
            .write_pack_for_oids(oid_hexes)
            .map_err(|err| JsValue::from_str(&err.to_string()))
    }

    /// Blobless-clone a smart HTTP remote into this in-memory repository.
    #[wasm_bindgen(js_name = cloneBlobless)]
    pub async fn clone_blobless(
        &mut self,
        repo_url: String,
        branch_or_ref: Option<String>,
    ) -> Result<JsValue, JsValue> {
        crate::remote::clone_blobless(&mut self.inner, &repo_url, branch_or_ref)
            .await
            .map(Into::into)
    }

    /// Return recent commits reachable by following first parents from `HEAD`.
    #[wasm_bindgen(js_name = recentCommits)]
    pub fn recent_commits(&self, limit: usize) -> Result<js_sys::Array, JsValue> {
        let head = self
            .inner
            .refs
            .resolve_ref("HEAD")
            .map_err(|err| JsValue::from_str(&err.to_string()))?
            .ok_or_else(|| JsValue::from_str("HEAD is not set"))?;
        let out = js_sys::Array::new();
        let mut current = Some(head);
        for _ in 0..limit {
            let Some(oid) = current else {
                break;
            };
            let object = self
                .inner
                .objects
                .read_object(&oid)
                .map_err(|err| JsValue::from_str(&err.to_string()))?;
            let commit =
                parse_commit(&object.data).map_err(|err| JsValue::from_str(&err.to_string()))?;
            let entry = js_sys::Object::new();
            js_sys::Reflect::set(
                &entry,
                &JsValue::from_str("oid"),
                &JsValue::from_str(&oid.to_hex()),
            )?;
            js_sys::Reflect::set(
                &entry,
                &JsValue::from_str("summary"),
                &JsValue::from_str(commit_summary(&commit.message)),
            )?;
            js_sys::Reflect::set(
                &entry,
                &JsValue::from_str("message"),
                &JsValue::from_str(&commit.message),
            )?;
            js_sys::Reflect::set(
                &entry,
                &JsValue::from_str("author"),
                &JsValue::from_str(&commit.author),
            )?;
            js_sys::Reflect::set(
                &entry,
                &JsValue::from_str("committer"),
                &JsValue::from_str(&commit.committer),
            )?;
            out.push(&entry);
            current = commit.parents.first().copied();
        }
        Ok(out)
    }

    /// Export repository state as a JSON snapshot.
    #[wasm_bindgen(js_name = exportJson)]
    pub fn export_json(&self) -> Result<String, JsValue> {
        crate::persistence::export_repository_json(&self.inner)
            .map_err(|err| JsValue::from_str(&err.to_string()))
    }

    /// Replace repository state from a JSON snapshot.
    #[wasm_bindgen(js_name = importJson)]
    pub fn import_json(&mut self, json: &str) -> Result<(), JsValue> {
        self.inner = crate::persistence::import_repository_json(json)
            .map_err(|err| JsValue::from_str(&err.to_string()))?;
        Ok(())
    }
}

fn commit_summary(message: &str) -> &str {
    message
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("")
}

impl Default for BrowserRepository {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(all(test, target_arch = "wasm32"))]
mod tests {
    use wasm_bindgen_test::*;

    use super::*;

    wasm_bindgen_test_configure!(run_in_browser);

    #[wasm_bindgen_test]
    fn exported_repo_writes_reads_and_commits() {
        let mut repo = BrowserRepository::new();
        let blob = repo.write_blob(b"hello");

        assert!(repo.has_object(&blob));
        assert_eq!(repo.read_blob(&blob).unwrap(), b"hello");

        repo.set_head("refs/heads/main");
        repo.stage_file("README.md", b"readme", false).unwrap();
        let commit = repo
            .commit_staged(
                "initial\n",
                "A U Thor <author@example.com> 1 +0000",
                "C O Mitter <committer@example.com> 1 +0000",
            )
            .unwrap();

        assert!(repo.has_object(&commit));
        assert_eq!(repo.get_ref("refs/heads/main"), Some(commit));
        assert_eq!(repo.staged_count(), 0);
    }

    #[wasm_bindgen_test]
    fn exported_repo_snapshot_round_trips() {
        let mut repo = BrowserRepository::new();
        repo.set_head("refs/heads/main");
        repo.stage_file("README.md", b"readme", false).unwrap();

        let json = repo.export_json().unwrap();
        let mut restored = BrowserRepository::new();
        restored.import_json(&json).unwrap();

        assert_eq!(restored.head(), Some("refs/heads/main".to_string()));
        assert_eq!(restored.staged_count(), 1);
    }

    #[wasm_bindgen_test]
    fn exported_repo_lists_recent_commits() {
        let mut repo = BrowserRepository::new();
        repo.set_head("refs/heads/main");
        repo.stage_file("README.md", b"readme", false).unwrap();
        let commit = repo
            .commit_staged(
                "initial commit\n\nbody\n",
                "A U Thor <author@example.com> 1 +0000",
                "C O Mitter <committer@example.com> 1 +0000",
            )
            .unwrap();

        let commits = repo.recent_commits(5).unwrap();

        assert_eq!(commits.length(), 1);
        let first = commits.get(0);
        assert_eq!(
            js_sys::Reflect::get(&first, &JsValue::from_str("oid"))
                .unwrap()
                .as_string(),
            Some(commit)
        );
        assert_eq!(
            js_sys::Reflect::get(&first, &JsValue::from_str("summary"))
                .unwrap()
                .as_string(),
            Some("initial commit".to_string())
        );
    }
}
