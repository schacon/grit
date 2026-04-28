//! Browser smart-HTTP operations.

use grit_lib::error::{Error, Result};
#[cfg(any(target_arch = "wasm32", test))]
use grit_lib::objects::ObjectId;
#[cfg(any(target_arch = "wasm32", test))]
use grit_lib::smart_protocol::AdvertisedRef;
#[cfg(target_arch = "wasm32")]
use grit_lib::storage::{ObjectReader, PackWriter, RefReader, RefWriter};

/// Build the smart-HTTP discovery URL for `service`.
///
/// `service` is usually `git-upload-pack` or `git-receive-pack`.
///
/// # Errors
///
/// Returns an error when the repository URL is empty.
pub fn info_refs_url(repo_url: &str, service: &str) -> Result<String> {
    let base = repo_url.trim_end_matches('/');
    if base.is_empty() {
        return Err(Error::Message("remote URL is empty".to_string()));
    }
    let sep = if base.contains('?') { "&" } else { "?" };
    Ok(format!("{base}/info/refs{sep}service={service}"))
}

/// Build the smart-HTTP RPC endpoint URL for `service`.
///
/// `service` is usually `git-upload-pack` or `git-receive-pack`.
///
/// # Errors
///
/// Returns an error when the repository URL is empty.
pub fn service_url(repo_url: &str, service: &str) -> Result<String> {
    let base = repo_url.trim_end_matches('/');
    if base.is_empty() {
        return Err(Error::Message("remote URL is empty".to_string()));
    }
    Ok(format!("{base}/{service}"))
}

#[cfg(target_arch = "wasm32")]
async fn fetch_refs(
    repo_url: &str,
) -> std::result::Result<Vec<AdvertisedRef>, wasm_bindgen::JsValue> {
    use grit_lib::smart_protocol::{
        build_ls_refs_v2_request, capability_lines_for_client_request, parse_ls_refs_v2_response,
        parse_upload_pack_advertisement, strip_http_service_advertisement_if_present,
        UploadPackAdvertisement,
    };
    use wasm_bindgen::JsValue;

    let discovery_url = info_refs_url(repo_url, "git-upload-pack")
        .map_err(|err| JsValue::from_str(&err.to_string()))?;
    let discovery =
        crate::browser_http::fetch_git_discovery(&discovery_url, Some("version=2")).await?;
    let pkt_body = strip_http_service_advertisement_if_present(&discovery)
        .map_err(|err| JsValue::from_str(&err.to_string()))?;
    match parse_upload_pack_advertisement(pkt_body)
        .map_err(|err| JsValue::from_str(&err.to_string()))?
    {
        UploadPackAdvertisement::V2 {
            caps,
            object_format,
        } => {
            let request = build_ls_refs_v2_request(
                &object_format,
                &capability_lines_for_client_request(&caps),
                true,
                true,
            )
            .map_err(|err| JsValue::from_str(&err.to_string()))?;
            let rpc_url = service_url(repo_url, "git-upload-pack")
                .map_err(|err| JsValue::from_str(&err.to_string()))?;
            let response = crate::browser_http::fetch_git_rpc(
                &rpc_url,
                "git-upload-pack",
                &request,
                Some("version=2"),
            )
            .await?;
            parse_ls_refs_v2_response(&response).map_err(|err| JsValue::from_str(&err.to_string()))
        }
        UploadPackAdvertisement::V0V1 { refs, .. } => Ok(refs),
    }
}

#[cfg(target_arch = "wasm32")]
async fn discover_receive_pack(
    repo_url: &str,
) -> std::result::Result<grit_lib::smart_protocol::ReceivePackAdvertisement, wasm_bindgen::JsValue>
{
    use grit_lib::smart_protocol::{
        parse_receive_pack_advertisement, strip_http_service_advertisement_if_present,
    };
    use wasm_bindgen::JsValue;

    let discovery_url = info_refs_url(repo_url, "git-receive-pack")
        .map_err(|err| JsValue::from_str(&err.to_string()))?;
    let discovery = crate::browser_http::fetch_git_discovery(&discovery_url, None).await?;
    let pkt_body = strip_http_service_advertisement_if_present(&discovery)
        .map_err(|err| JsValue::from_str(&err.to_string()))?;
    parse_receive_pack_advertisement(pkt_body).map_err(|err| JsValue::from_str(&err.to_string()))
}

/// Fetch refs from a smart-HTTP remote in the browser.
///
/// The return value is a JavaScript array of `{ name, oid }` objects.
#[cfg(target_arch = "wasm32")]
pub async fn ls_refs(repo_url: &str) -> std::result::Result<js_sys::Array, wasm_bindgen::JsValue> {
    use js_sys::{Array, Object, Reflect};
    use wasm_bindgen::JsValue;

    let refs = fetch_refs(repo_url).await?;

    let out = Array::new();
    for advertised in refs {
        let obj = Object::new();
        Reflect::set(
            &obj,
            &JsValue::from_str("name"),
            &JsValue::from_str(&advertised.name),
        )?;
        Reflect::set(
            &obj,
            &JsValue::from_str("oid"),
            &JsValue::from_str(&advertised.oid.to_hex()),
        )?;
        if let Some(symref_target) = advertised.symref_target {
            Reflect::set(
                &obj,
                &JsValue::from_str("symrefTarget"),
                &JsValue::from_str(&symref_target),
            )?;
        }
        out.push(&obj);
    }
    Ok(out)
}

/// Fetch a blobless pack containing `want_oids` into an in-memory repository.
///
/// The fetched pack is stored as a promisor pack and unpacked into the object
/// store. The returned string is the stored pack ID.
#[cfg(target_arch = "wasm32")]
pub async fn fetch_blobless(
    repo: &mut crate::WasmRepository,
    repo_url: &str,
    want_oids: Vec<String>,
) -> std::result::Result<String, wasm_bindgen::JsValue> {
    use wasm_bindgen::JsValue;

    let wants = parse_want_oids(&want_oids).map_err(|err| JsValue::from_str(&err.to_string()))?;
    if wants.is_empty() {
        return Err(JsValue::from_str(
            "blobless fetch requires at least one want",
        ));
    }
    fetch_upload_pack(repo, repo_url, wants, Some("blob:none".to_string()), true).await
}

#[cfg(target_arch = "wasm32")]
fn parse_want_oids(want_oids: &[String]) -> Result<Vec<ObjectId>> {
    want_oids
        .iter()
        .map(|oid| ObjectId::from_hex(oid))
        .collect::<Result<Vec<_>>>()
}

#[cfg(target_arch = "wasm32")]
async fn fetch_upload_pack(
    repo: &mut crate::WasmRepository,
    repo_url: &str,
    wants: Vec<ObjectId>,
    filter_spec: Option<String>,
    require_filter: bool,
) -> std::result::Result<String, wasm_bindgen::JsValue> {
    use grit_lib::smart_protocol::{
        build_fetch_v2_request, capability_lines_for_client_request,
        extract_packfile_from_fetch_response, fetch_features_from_caps,
        parse_upload_pack_advertisement, strip_http_service_advertisement_if_present,
        FetchRequestOptions, UploadPackAdvertisement,
    };
    use grit_lib::unpack_objects::{unpack_objects_into_store, UnpackOptions};
    use std::io::Cursor;
    use wasm_bindgen::JsValue;

    if wants.is_empty() {
        return Err(JsValue::from_str(
            "upload-pack fetch requires at least one want",
        ));
    }

    let discovery_url = info_refs_url(repo_url, "git-upload-pack")
        .map_err(|err| JsValue::from_str(&err.to_string()))?;
    let discovery =
        crate::browser_http::fetch_git_discovery(&discovery_url, Some("version=2")).await?;
    let pkt_body = strip_http_service_advertisement_if_present(&discovery)
        .map_err(|err| JsValue::from_str(&err.to_string()))?;
    let (caps, object_format) = match parse_upload_pack_advertisement(pkt_body)
        .map_err(|err| JsValue::from_str(&err.to_string()))?
    {
        UploadPackAdvertisement::V2 {
            caps,
            object_format,
        } => (caps, object_format),
        UploadPackAdvertisement::V0V1 { .. } => {
            return Err(JsValue::from_str("blobless fetch requires protocol v2"));
        }
    };
    let fetch_features = fetch_features_from_caps(&caps);
    if require_filter && !fetch_features.contains("filter") {
        return Err(JsValue::from_str(
            "remote upload-pack does not advertise filter support",
        ));
    }
    let request = build_fetch_v2_request(
        &fetch_features,
        &FetchRequestOptions {
            object_format,
            capability_lines: capability_lines_for_client_request(&caps),
            wants,
            include_done: true,
            filter_spec,
            ..FetchRequestOptions::default()
        },
    )
    .map_err(|err| JsValue::from_str(&err.to_string()))?;
    let rpc_url = service_url(repo_url, "git-upload-pack")
        .map_err(|err| JsValue::from_str(&err.to_string()))?;
    let response = crate::browser_http::fetch_git_rpc(
        &rpc_url,
        "git-upload-pack",
        &request,
        Some("version=2"),
    )
    .await?;
    let pack = extract_packfile_from_fetch_response(&response)
        .map_err(|err| JsValue::from_str(&err.to_string()))?;
    let pack_id = repo
        .packs
        .add_pack(pack.clone(), true)
        .map_err(|err| JsValue::from_str(&err.to_string()))?;
    let mut cursor = Cursor::new(pack);
    unpack_objects_into_store(
        &mut cursor,
        &mut repo.objects,
        &UnpackOptions {
            quiet: true,
            ..UnpackOptions::default()
        },
    )
    .map_err(|err| JsValue::from_str(&err.to_string()))?;
    Ok(pack_id.as_str().to_string())
}

/// Fetch a promised object by ID from the recorded `origin` promisor remote.
///
/// Returns `true` when the object is available after the fetch.
#[cfg(target_arch = "wasm32")]
pub async fn fetch_promised_object(
    repo: &mut crate::WasmRepository,
    oid_hex: &str,
) -> std::result::Result<bool, wasm_bindgen::JsValue> {
    use wasm_bindgen::JsValue;

    if repo.has_object_hex(oid_hex) {
        return Ok(true);
    }
    let oid = ObjectId::from_hex(oid_hex).map_err(|err| JsValue::from_str(&err.to_string()))?;
    let remote = repo
        .promisor_remote("origin")
        .ok_or_else(|| JsValue::from_str("missing origin promisor remote"))?;
    let _pack_id = fetch_upload_pack(repo, &remote.url, vec![oid], None, false).await?;
    if repo.has_object_hex(oid_hex) {
        Ok(true)
    } else {
        Err(JsValue::from_str(
            "promisor fetch did not hydrate requested object",
        ))
    }
}

/// Read a blob, fetching it from the promisor remote first if it is missing.
#[cfg(target_arch = "wasm32")]
pub async fn read_blob_promised(
    repo: &mut crate::WasmRepository,
    oid_hex: &str,
) -> std::result::Result<Vec<u8>, wasm_bindgen::JsValue> {
    use wasm_bindgen::JsValue;

    if let Some(bytes) = repo.read_blob(oid_hex) {
        return Ok(bytes);
    }
    fetch_promised_object(repo, oid_hex).await?;
    repo.read_blob(oid_hex)
        .ok_or_else(|| JsValue::from_str("hydrated object is not a blob"))
}

#[cfg(target_arch = "wasm32")]
async fn read_object_promised(
    repo: &mut crate::WasmRepository,
    oid: ObjectId,
) -> std::result::Result<grit_lib::objects::Object, wasm_bindgen::JsValue> {
    use wasm_bindgen::JsValue;

    if !repo.has_object_hex(&oid.to_hex()) {
        fetch_promised_object(repo, &oid.to_hex()).await?;
    }
    repo.objects
        .read_object(&oid)
        .map_err(|err| JsValue::from_str(&err.to_string()))
}

/// Read a file at `path` from `HEAD`, lazily hydrating missing tree/blob objects.
///
/// The path is repository-relative and uses `/` separators. The returned bytes
/// are the raw blob contents.
#[cfg(target_arch = "wasm32")]
pub async fn read_path_promised(
    repo: &mut crate::WasmRepository,
    path: &str,
) -> std::result::Result<Vec<u8>, wasm_bindgen::JsValue> {
    use grit_lib::objects::{parse_commit, parse_tree, ObjectKind};
    use wasm_bindgen::JsValue;

    const TREE_MODE: u32 = 0o040000;

    let components = path_components(path).map_err(|err| JsValue::from_str(&err.to_string()))?;
    let head_oid = repo
        .refs
        .resolve_ref("HEAD")
        .map_err(|err| JsValue::from_str(&err.to_string()))?
        .ok_or_else(|| JsValue::from_str("HEAD is not set"))?;
    let head = read_object_promised(repo, head_oid).await?;
    if head.kind != ObjectKind::Commit {
        return Err(JsValue::from_str("HEAD does not point to a commit"));
    }
    let commit = parse_commit(&head.data).map_err(|err| JsValue::from_str(&err.to_string()))?;
    let mut current_tree_oid = commit.tree;

    for (idx, component) in components.iter().enumerate() {
        let tree = read_object_promised(repo, current_tree_oid).await?;
        if tree.kind != ObjectKind::Tree {
            return Err(JsValue::from_str("tree walk reached a non-tree object"));
        }
        let entries = parse_tree(&tree.data).map_err(|err| JsValue::from_str(&err.to_string()))?;
        let entry = entries
            .iter()
            .find(|entry| entry.name.as_slice() == component.as_bytes())
            .ok_or_else(|| JsValue::from_str(&format!("path not found: {path}")))?;
        let is_last = idx + 1 == components.len();
        if is_last {
            if entry.mode == TREE_MODE {
                return Err(JsValue::from_str("path points to a tree"));
            }
            let object = read_object_promised(repo, entry.oid).await?;
            if object.kind != ObjectKind::Blob {
                return Err(JsValue::from_str("path does not point to a blob"));
            }
            return Ok(object.data);
        }
        if entry.mode != TREE_MODE {
            return Err(JsValue::from_str(&format!(
                "path component is not a tree: {component}"
            )));
        }
        current_tree_oid = entry.oid;
    }

    Err(JsValue::from_str("path not found"))
}

#[cfg(any(target_arch = "wasm32", test))]
fn path_components(path: &str) -> Result<Vec<String>> {
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
    Ok(trimmed.split('/').map(ToOwned::to_owned).collect())
}

/// Result metadata for a blobless clone.
#[cfg(any(target_arch = "wasm32", test))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CloneSelection {
    /// Local branch name, without `refs/heads/`.
    pub branch: String,
    /// Local branch reference updated by the clone.
    pub local_ref: String,
    /// Remote-tracking reference updated by the clone.
    pub remote_ref: String,
    /// Commit OID fetched for the selected branch.
    pub oid: ObjectId,
}

#[cfg(any(target_arch = "wasm32", test))]
fn select_clone_ref(refs: &[AdvertisedRef], branch_or_ref: Option<&str>) -> Result<CloneSelection> {
    let target_ref = if let Some(requested) = branch_or_ref.filter(|value| !value.trim().is_empty())
    {
        if requested.starts_with("refs/") {
            requested.to_string()
        } else {
            format!("refs/heads/{requested}")
        }
    } else if let Some(head_target) = refs
        .iter()
        .find(|entry| entry.name == "HEAD")
        .and_then(|entry| entry.symref_target.clone())
    {
        head_target
    } else if refs.iter().any(|entry| entry.name == "refs/heads/main") {
        "refs/heads/main".to_string()
    } else if refs.iter().any(|entry| entry.name == "refs/heads/master") {
        "refs/heads/master".to_string()
    } else {
        refs.iter()
            .find(|entry| entry.name.starts_with("refs/heads/"))
            .map(|entry| entry.name.clone())
            .ok_or_else(|| Error::Message("remote did not advertise any branches".to_string()))?
    };

    let advertised = refs
        .iter()
        .find(|entry| entry.name == target_ref)
        .ok_or_else(|| Error::Message(format!("remote did not advertise {target_ref}")))?;
    let branch = target_ref
        .strip_prefix("refs/heads/")
        .ok_or_else(|| Error::Message(format!("{target_ref} is not a branch ref")))?
        .to_string();
    Ok(CloneSelection {
        branch: branch.clone(),
        local_ref: target_ref,
        remote_ref: format!("refs/remotes/origin/{branch}"),
        oid: advertised.oid,
    })
}

/// Perform a blobless clone into an in-memory repository.
///
/// This fetches the selected branch with `filter blob:none`, updates local and
/// remote-tracking refs, points `HEAD` at the local branch, and records `origin`
/// as a promisor remote with the `blob:none` filter.
#[cfg(target_arch = "wasm32")]
pub async fn clone_blobless(
    repo: &mut crate::WasmRepository,
    repo_url: &str,
    branch_or_ref: Option<String>,
) -> std::result::Result<js_sys::Object, wasm_bindgen::JsValue> {
    use js_sys::{Object, Reflect};
    use wasm_bindgen::JsValue;

    let refs = fetch_refs(repo_url).await?;
    let selection = select_clone_ref(&refs, branch_or_ref.as_deref())
        .map_err(|err| JsValue::from_str(&err.to_string()))?;
    let pack_id = fetch_blobless(repo, repo_url, vec![selection.oid.to_hex()]).await?;

    repo.refs
        .set_ref(&selection.local_ref, selection.oid)
        .map_err(|err| JsValue::from_str(&err.to_string()))?;
    repo.refs
        .set_ref(&selection.remote_ref, selection.oid)
        .map_err(|err| JsValue::from_str(&err.to_string()))?;
    repo.refs
        .set_symbolic_ref("HEAD", &selection.local_ref)
        .map_err(|err| JsValue::from_str(&err.to_string()))?;
    repo.refs
        .set_symbolic_ref(
            "refs/remotes/origin/HEAD",
            &format!("refs/remotes/origin/{}", selection.branch),
        )
        .map_err(|err| JsValue::from_str(&err.to_string()))?;
    repo.set_promisor_remote("origin", repo_url, "blob:none");

    let out = Object::new();
    Reflect::set(
        &out,
        &JsValue::from_str("branch"),
        &JsValue::from_str(&selection.branch),
    )?;
    Reflect::set(
        &out,
        &JsValue::from_str("head"),
        &JsValue::from_str(&selection.local_ref),
    )?;
    Reflect::set(
        &out,
        &JsValue::from_str("oid"),
        &JsValue::from_str(&selection.oid.to_hex()),
    )?;
    Reflect::set(
        &out,
        &JsValue::from_str("packId"),
        &JsValue::from_str(&pack_id),
    )?;
    Ok(out)
}

#[cfg(any(target_arch = "wasm32", test))]
fn remote_tracking_ref_for_push(remote_ref: &str) -> Result<String> {
    let branch = remote_ref.strip_prefix("refs/heads/").ok_or_else(|| {
        Error::InvalidRef(format!("can only push branch refs for now: {remote_ref}"))
    })?;
    Ok(format!("refs/remotes/origin/{branch}"))
}

/// Push a local branch to a smart-HTTP remote using a non-delta pack.
#[cfg(target_arch = "wasm32")]
pub async fn push(
    repo: &mut crate::WasmRepository,
    repo_url: &str,
    local_ref: &str,
    remote_ref: &str,
) -> std::result::Result<js_sys::Object, wasm_bindgen::JsValue> {
    use grit_lib::smart_protocol::{
        build_receive_pack_request, parse_receive_pack_response, PushCommand,
        ReceivePackCapabilities,
    };
    use js_sys::{Array, Object, Reflect};
    use wasm_bindgen::JsValue;

    let local_oid = repo
        .refs
        .resolve_ref(local_ref)
        .map_err(|err| JsValue::from_str(&err.to_string()))?
        .ok_or_else(|| JsValue::from_str(&format!("local ref not found: {local_ref}")))?;
    let advertised = discover_receive_pack(repo_url).await?;
    let old_oid = advertised.advertised_oid(remote_ref);
    let pack = repo
        .write_pack_for_push(&local_oid.to_hex(), old_oid.map(|oid| oid.to_hex()))
        .map_err(|err| JsValue::from_str(&err.to_string()))?;
    let capabilities = ReceivePackCapabilities {
        advertised: advertised.capabilities,
        agent: Some(format!("grit-wasm/{}", crate::WasmRepository::version())),
        session_id: None,
    };
    let command = PushCommand {
        old_oid,
        new_oid: Some(local_oid),
        refname: remote_ref.to_string(),
    };
    let (request, use_sideband) =
        build_receive_pack_request(&capabilities, &[command], &[], &pack, false)
            .map_err(|err| JsValue::from_str(&err.to_string()))?;
    let rpc_url = service_url(repo_url, "git-receive-pack")
        .map_err(|err| JsValue::from_str(&err.to_string()))?;
    let response =
        crate::browser_http::fetch_git_rpc(&rpc_url, "git-receive-pack", &request, None).await?;
    let status = parse_receive_pack_response(&response, use_sideband)
        .map_err(|err| JsValue::from_str(&err.to_string()))?;
    let ref_ok = status
        .statuses
        .iter()
        .find(|entry| entry.refname == remote_ref)
        .map(|entry| entry.ok)
        .unwrap_or(status.statuses.is_empty() && status.unpack_ok);
    if !status.unpack_ok || !ref_ok {
        let message = status
            .statuses
            .iter()
            .find(|entry| entry.refname == remote_ref)
            .and_then(|entry| entry.message.clone())
            .unwrap_or(status.unpack_message.clone());
        return Err(JsValue::from_str(&format!("push rejected: {message}")));
    }

    let tracking_ref = remote_tracking_ref_for_push(remote_ref)
        .map_err(|err| JsValue::from_str(&err.to_string()))?;
    repo.refs
        .set_ref(&tracking_ref, local_oid)
        .map_err(|err| JsValue::from_str(&err.to_string()))?;

    let out = Object::new();
    Reflect::set(
        &out,
        &JsValue::from_str("remoteRef"),
        &JsValue::from_str(remote_ref),
    )?;
    Reflect::set(
        &out,
        &JsValue::from_str("trackingRef"),
        &JsValue::from_str(&tracking_ref),
    )?;
    Reflect::set(
        &out,
        &JsValue::from_str("oid"),
        &JsValue::from_str(&local_oid.to_hex()),
    )?;
    Reflect::set(
        &out,
        &JsValue::from_str("unpackOk"),
        &JsValue::from_bool(status.unpack_ok),
    )?;
    let status_array = Array::new();
    for entry in status.statuses {
        let obj = Object::new();
        Reflect::set(
            &obj,
            &JsValue::from_str("refname"),
            &JsValue::from_str(&entry.refname),
        )?;
        Reflect::set(
            &obj,
            &JsValue::from_str("ok"),
            &JsValue::from_bool(entry.ok),
        )?;
        if let Some(message) = entry.message {
            Reflect::set(
                &obj,
                &JsValue::from_str("message"),
                &JsValue::from_str(&message),
            )?;
        }
        status_array.push(&obj);
    }
    Reflect::set(&out, &JsValue::from_str("statuses"), &status_array)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_discovery_url_without_existing_query() {
        let url = info_refs_url("https://example.com/repo.git", "git-upload-pack").unwrap();

        assert_eq!(
            url,
            "https://example.com/repo.git/info/refs?service=git-upload-pack"
        );
    }

    #[test]
    fn builds_discovery_url_with_existing_query() {
        let url = info_refs_url("https://example.com/repo.git?x=1", "git-upload-pack").unwrap();

        assert_eq!(
            url,
            "https://example.com/repo.git?x=1/info/refs&service=git-upload-pack"
        );
    }

    #[test]
    fn builds_service_url() {
        let url = service_url("https://example.com/repo.git/", "git-upload-pack").unwrap();

        assert_eq!(url, "https://example.com/repo.git/git-upload-pack");
    }

    #[test]
    fn selects_head_symref_for_clone() {
        let oid = ObjectId::from_hex("67bf698f3ab735e92fb011a99cff3497c44d30c1").unwrap();
        let refs = vec![
            AdvertisedRef {
                name: "HEAD".to_string(),
                oid,
                symref_target: Some("refs/heads/main".to_string()),
            },
            AdvertisedRef {
                name: "refs/heads/main".to_string(),
                oid,
                symref_target: None,
            },
        ];

        let selected = select_clone_ref(&refs, None).unwrap();

        assert_eq!(selected.branch, "main");
        assert_eq!(selected.local_ref, "refs/heads/main");
        assert_eq!(selected.remote_ref, "refs/remotes/origin/main");
        assert_eq!(selected.oid, oid);
    }

    #[test]
    fn selects_requested_branch_for_clone() {
        let oid = ObjectId::from_hex("67bf698f3ab735e92fb011a99cff3497c44d30c1").unwrap();
        let refs = vec![AdvertisedRef {
            name: "refs/heads/topic".to_string(),
            oid,
            symref_target: None,
        }];

        let selected = select_clone_ref(&refs, Some("topic")).unwrap();

        assert_eq!(selected.branch, "topic");
        assert_eq!(selected.local_ref, "refs/heads/topic");
    }

    #[test]
    fn maps_branch_push_to_origin_tracking_ref() {
        assert_eq!(
            remote_tracking_ref_for_push("refs/heads/main").unwrap(),
            "refs/remotes/origin/main"
        );
        assert!(remote_tracking_ref_for_push("refs/tags/v1").is_err());
    }

    #[test]
    fn validates_path_components() {
        assert_eq!(
            path_components("/src/lib.rs").unwrap(),
            vec!["src".to_string(), "lib.rs".to_string()]
        );
        assert!(path_components("").is_err());
        assert!(path_components("src/../secret").is_err());
    }
}
