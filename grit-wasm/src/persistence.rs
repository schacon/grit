//! Snapshot and browser persistence helpers.

use std::collections::BTreeMap;

use base64::prelude::*;
use grit_lib::error::{Error, Result};
use grit_lib::objects::{Object, ObjectId, ObjectKind};
use serde::{Deserialize, Serialize};

use grit_lib::storage::PackId;

use crate::{
    MemoryObjectStore, MemoryPackStore, MemoryRefStore, PromisorRemote, StagedFile, StoredPack,
    WasmRepository,
};

/// Serializable repository snapshot.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RepositorySnapshot {
    objects: Vec<ObjectSnapshot>,
    refs: Vec<RefSnapshot>,
    symbolic_refs: Vec<SymbolicRefSnapshot>,
    packs: Vec<PackSnapshot>,
    promisor_remotes: Vec<PromisorRemoteSnapshot>,
    staged: Vec<StagedFileSnapshot>,
}

/// Browser storage quota estimate.
#[derive(Clone, Debug, PartialEq)]
pub struct StorageQuota {
    /// Estimated bytes currently used by this origin.
    pub usage: Option<f64>,
    /// Estimated bytes available to this origin.
    pub quota: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ObjectSnapshot {
    oid: String,
    kind: String,
    data_base64: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct RefSnapshot {
    name: String,
    oid: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SymbolicRefSnapshot {
    name: String,
    target: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct PackSnapshot {
    id: String,
    bytes_base64: String,
    promisor: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct PromisorRemoteSnapshot {
    name: String,
    url: String,
    filter: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StagedFileSnapshot {
    path: String,
    mode: u32,
    oid: String,
}

/// Export a repository snapshot to JSON.
///
/// # Errors
///
/// Returns an error when JSON serialization fails.
pub fn export_repository_json(repo: &WasmRepository) -> Result<String> {
    serde_json::to_string(&snapshot_repository(repo)).map_err(|err| Error::Message(err.to_string()))
}

/// Import a repository snapshot from JSON.
///
/// # Errors
///
/// Returns an error when JSON parsing fails or the snapshot contains invalid
/// object IDs/object kinds.
pub fn import_repository_json(json: &str) -> Result<WasmRepository> {
    let snapshot: RepositorySnapshot =
        serde_json::from_str(json).map_err(|err| Error::Message(err.to_string()))?;
    restore_repository(snapshot)
}

fn snapshot_repository(repo: &WasmRepository) -> RepositorySnapshot {
    RepositorySnapshot {
        objects: repo
            .objects
            .objects
            .iter()
            .map(|(oid, object)| ObjectSnapshot {
                oid: oid.to_hex(),
                kind: object.kind.as_str().to_string(),
                data_base64: BASE64_STANDARD.encode(&object.data),
            })
            .collect(),
        refs: repo
            .refs
            .refs
            .iter()
            .map(|(name, oid)| RefSnapshot {
                name: name.clone(),
                oid: oid.to_hex(),
            })
            .collect(),
        symbolic_refs: repo
            .refs
            .symbolic_refs
            .iter()
            .map(|(name, target)| SymbolicRefSnapshot {
                name: name.clone(),
                target: target.clone(),
            })
            .collect(),
        packs: repo
            .packs
            .packs
            .iter()
            .map(|(id, pack)| PackSnapshot {
                id: id.as_str().to_string(),
                bytes_base64: BASE64_STANDARD.encode(&pack.bytes),
                promisor: pack.promisor,
            })
            .collect(),
        promisor_remotes: repo
            .promisor_remotes
            .iter()
            .map(|(name, remote)| PromisorRemoteSnapshot {
                name: name.clone(),
                url: remote.url.clone(),
                filter: remote.filter.clone(),
            })
            .collect(),
        staged: repo
            .staged
            .values()
            .map(|entry| StagedFileSnapshot {
                path: entry.path.clone(),
                mode: entry.mode,
                oid: entry.oid.to_hex(),
            })
            .collect(),
    }
}

fn restore_repository(snapshot: RepositorySnapshot) -> Result<WasmRepository> {
    let mut objects = BTreeMap::new();
    for object in snapshot.objects {
        let oid = ObjectId::from_hex(&object.oid)?;
        let kind = object_kind_from_str(&object.kind)?;
        let data = BASE64_STANDARD
            .decode(object.data_base64)
            .map_err(|err| Error::Message(err.to_string()))?;
        objects.insert(oid, Object::new(kind, data));
    }

    let mut refs = BTreeMap::new();
    for entry in snapshot.refs {
        refs.insert(entry.name, ObjectId::from_hex(&entry.oid)?);
    }

    let symbolic_refs = snapshot
        .symbolic_refs
        .into_iter()
        .map(|entry| (entry.name, entry.target))
        .collect();

    let mut packs = BTreeMap::new();
    for pack in snapshot.packs {
        let bytes = BASE64_STANDARD
            .decode(pack.bytes_base64)
            .map_err(|err| Error::Message(err.to_string()))?;
        packs.insert(
            PackId::new(pack.id),
            StoredPack {
                bytes,
                promisor: pack.promisor,
            },
        );
    }

    let promisor_remotes = snapshot
        .promisor_remotes
        .into_iter()
        .map(|entry| {
            (
                entry.name,
                PromisorRemote {
                    url: entry.url,
                    filter: entry.filter,
                },
            )
        })
        .collect();

    let mut staged = BTreeMap::new();
    for entry in snapshot.staged {
        let staged_file = StagedFile {
            path: entry.path.clone(),
            mode: entry.mode,
            oid: ObjectId::from_hex(&entry.oid)?,
        };
        staged.insert(entry.path, staged_file);
    }

    Ok(WasmRepository {
        objects: MemoryObjectStore { objects },
        refs: MemoryRefStore {
            refs,
            symbolic_refs,
        },
        packs: MemoryPackStore { packs },
        promisor_remotes,
        staged,
    })
}

fn object_kind_from_str(kind: &str) -> Result<ObjectKind> {
    match kind {
        "blob" => Ok(ObjectKind::Blob),
        "tree" => Ok(ObjectKind::Tree),
        "commit" => Ok(ObjectKind::Commit),
        "tag" => Ok(ObjectKind::Tag),
        other => Err(Error::UnknownObjectType(other.to_string())),
    }
}

/// Save a repository snapshot to IndexedDB.
#[cfg(target_arch = "wasm32")]
pub async fn save_repository(
    db_name: &str,
    repo: &WasmRepository,
) -> std::result::Result<(), wasm_bindgen::JsValue> {
    let json = export_repository_json(repo)
        .map_err(|err| wasm_bindgen::JsValue::from_str(&err.to_string()))?;
    let db = open_database(db_name).await?;
    let tx =
        db.transaction_with_str_and_mode(STORE_NAME, web_sys::IdbTransactionMode::Readwrite)?;
    let store = tx.object_store(STORE_NAME)?;
    let request = store.put_with_key(
        &wasm_bindgen::JsValue::from_str(&json),
        &wasm_bindgen::JsValue::from_str(DEFAULT_KEY),
    )?;
    let _ = idb_request_result(&request).await?;
    Ok(())
}

/// Load a repository snapshot from IndexedDB.
#[cfg(target_arch = "wasm32")]
pub async fn load_repository(
    db_name: &str,
) -> std::result::Result<Option<WasmRepository>, wasm_bindgen::JsValue> {
    let db = open_database(db_name).await?;
    let tx = db.transaction_with_str(STORE_NAME)?;
    let store = tx.object_store(STORE_NAME)?;
    let request = store.get(&wasm_bindgen::JsValue::from_str(DEFAULT_KEY))?;
    let value = idb_request_result(&request).await?;
    if value.is_undefined() {
        return Ok(None);
    }
    let Some(json) = value.as_string() else {
        return Err(wasm_bindgen::JsValue::from_str(
            "stored repository snapshot is not a string",
        ));
    };
    import_repository_json(&json)
        .map(Some)
        .map_err(|err| wasm_bindgen::JsValue::from_str(&err.to_string()))
}

/// Delete the saved repository snapshot from IndexedDB.
#[cfg(target_arch = "wasm32")]
pub async fn delete_repository_snapshot(
    db_name: &str,
) -> std::result::Result<(), wasm_bindgen::JsValue> {
    let db = open_database(db_name).await?;
    let tx =
        db.transaction_with_str_and_mode(STORE_NAME, web_sys::IdbTransactionMode::Readwrite)?;
    let store = tx.object_store(STORE_NAME)?;
    let request = store.delete(&wasm_bindgen::JsValue::from_str(DEFAULT_KEY))?;
    let _ = idb_request_result(&request).await?;
    Ok(())
}

/// Delete the entire IndexedDB database used for repository storage.
#[cfg(target_arch = "wasm32")]
pub async fn delete_repository_database(
    db_name: &str,
) -> std::result::Result<(), wasm_bindgen::JsValue> {
    let window =
        web_sys::window().ok_or_else(|| wasm_bindgen::JsValue::from_str("missing window"))?;
    let factory = window
        .indexed_db()?
        .ok_or_else(|| wasm_bindgen::JsValue::from_str("IndexedDB is unavailable"))?;
    let request = factory.delete_database(db_name)?;
    let _ = idb_open_request_result(&request).await?;
    Ok(())
}

/// Estimate browser storage usage/quota for this origin.
#[cfg(target_arch = "wasm32")]
pub async fn estimate_storage_quota(
) -> std::result::Result<Option<StorageQuota>, wasm_bindgen::JsValue> {
    use js_sys::{Function, Promise, Reflect};
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;

    let window =
        web_sys::window().ok_or_else(|| wasm_bindgen::JsValue::from_str("missing window"))?;
    let navigator = Reflect::get(&window, &wasm_bindgen::JsValue::from_str("navigator"))?;
    let storage = Reflect::get(&navigator, &wasm_bindgen::JsValue::from_str("storage"))?;
    if storage.is_undefined() || storage.is_null() {
        return Ok(None);
    }
    let estimate = Reflect::get(&storage, &wasm_bindgen::JsValue::from_str("estimate"))?;
    if !estimate.is_function() {
        return Ok(None);
    }
    let estimate_fn: Function = estimate.dyn_into()?;
    let promise = estimate_fn.call0(&storage)?;
    let value = JsFuture::from(Promise::from(promise)).await?;
    let usage = Reflect::get(&value, &wasm_bindgen::JsValue::from_str("usage"))?.as_f64();
    let quota = Reflect::get(&value, &wasm_bindgen::JsValue::from_str("quota"))?.as_f64();
    Ok(Some(StorageQuota { usage, quota }))
}

#[cfg(target_arch = "wasm32")]
const STORE_NAME: &str = "repositories";
#[cfg(target_arch = "wasm32")]
const DEFAULT_KEY: &str = "default";

#[cfg(target_arch = "wasm32")]
async fn open_database(
    db_name: &str,
) -> std::result::Result<web_sys::IdbDatabase, wasm_bindgen::JsValue> {
    use wasm_bindgen::closure::Closure;
    use wasm_bindgen::JsCast;
    use web_sys::IdbDatabase;

    let window =
        web_sys::window().ok_or_else(|| wasm_bindgen::JsValue::from_str("missing window"))?;
    let factory = window
        .indexed_db()?
        .ok_or_else(|| wasm_bindgen::JsValue::from_str("IndexedDB is unavailable"))?;
    let request = factory.open_with_u32(db_name, 1)?;
    let upgrade_request = request.clone();
    let on_upgrade = Closure::<dyn FnMut(_)>::new(move |_event: web_sys::Event| {
        if let Ok(result) = upgrade_request.result() {
            if let Ok(db) = result.dyn_into::<IdbDatabase>() {
                let _ = db.create_object_store(STORE_NAME);
            }
        }
    });
    request.set_onupgradeneeded(Some(on_upgrade.as_ref().unchecked_ref()));
    on_upgrade.forget();

    let value = idb_open_request_result(&request).await?;
    value.dyn_into::<IdbDatabase>()
}

#[cfg(target_arch = "wasm32")]
async fn idb_open_request_result(
    request: &web_sys::IdbOpenDbRequest,
) -> std::result::Result<wasm_bindgen::JsValue, wasm_bindgen::JsValue> {
    use js_sys::Promise;
    use wasm_bindgen::closure::Closure;
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;

    let success_request = request.clone();
    let error_request = request.clone();
    let promise = Promise::new(&mut |resolve, reject| {
        let success_request = success_request.clone();
        let error_request = error_request.clone();
        let on_success = Closure::<dyn FnMut(_)>::new(move |_event: web_sys::Event| {
            let value = success_request
                .result()
                .unwrap_or(wasm_bindgen::JsValue::UNDEFINED);
            let _ = resolve.call1(&wasm_bindgen::JsValue::UNDEFINED, &value);
        });
        let on_error = Closure::<dyn FnMut(_)>::new(move |_event: web_sys::Event| {
            let value = error_request
                .error()
                .map(wasm_bindgen::JsValue::from)
                .unwrap_or_else(|_| wasm_bindgen::JsValue::from_str("IndexedDB request failed"));
            let _ = reject.call1(&wasm_bindgen::JsValue::UNDEFINED, &value);
        });
        request.set_onsuccess(Some(on_success.as_ref().unchecked_ref()));
        request.set_onerror(Some(on_error.as_ref().unchecked_ref()));
        on_success.forget();
        on_error.forget();
    });
    JsFuture::from(promise).await
}

#[cfg(target_arch = "wasm32")]
async fn idb_request_result(
    request: &web_sys::IdbRequest,
) -> std::result::Result<wasm_bindgen::JsValue, wasm_bindgen::JsValue> {
    use js_sys::Promise;
    use wasm_bindgen::closure::Closure;
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;

    let success_request = request.clone();
    let error_request = request.clone();
    let promise = Promise::new(&mut |resolve, reject| {
        let success_request = success_request.clone();
        let error_request = error_request.clone();
        let on_success = Closure::<dyn FnMut(_)>::new(move |_event: web_sys::Event| {
            let value = success_request
                .result()
                .unwrap_or(wasm_bindgen::JsValue::UNDEFINED);
            let _ = resolve.call1(&wasm_bindgen::JsValue::UNDEFINED, &value);
        });
        let on_error = Closure::<dyn FnMut(_)>::new(move |_event: web_sys::Event| {
            let value = error_request
                .error()
                .map(wasm_bindgen::JsValue::from)
                .unwrap_or_else(|_| wasm_bindgen::JsValue::from_str("IndexedDB request failed"));
            let _ = reject.call1(&wasm_bindgen::JsValue::UNDEFINED, &value);
        });
        request.set_onsuccess(Some(on_success.as_ref().unchecked_ref()));
        request.set_onerror(Some(on_error.as_ref().unchecked_ref()));
        on_success.forget();
        on_error.forget();
    });
    JsFuture::from(promise).await
}

#[cfg(test)]
mod tests {
    use grit_lib::storage::RefWriter;

    use super::*;

    #[test]
    fn repository_snapshot_round_trips() {
        let mut repo = WasmRepository::new();
        let blob = repo.stage_file("README.md", b"hello", false).unwrap();
        repo.refs
            .set_symbolic_ref("HEAD", "refs/heads/main")
            .unwrap();
        repo.set_promisor_remote("origin", "https://example.com/repo.git", "blob:none");
        let pack_id = repo.add_pack(b"PACK bytes".to_vec(), true);

        let json = export_repository_json(&repo).unwrap();
        let restored = import_repository_json(&json).unwrap();

        assert_eq!(restored.staged_count(), 1);
        assert_eq!(restored.promisor_remote_count(), 1);
        assert!(restored.has_object_hex(&blob));
        assert!(restored.pack_is_promisor(&pack_id));
        assert_eq!(restored.head(), Some("refs/heads/main".to_string()));
    }
}
