//! Copy missing blobs from the configured promisor remote into the local object store.
//!
//! Used by partial-clone hydration, `sparse-checkout` updates, and `backfill`.

use anyhow::{bail, Context, Result};
use grit_lib::config::ConfigSet;
use grit_lib::objects::{parse_commit, parse_tree, ObjectId, ObjectKind};
use grit_lib::promisor::{
    read_promisor_missing_oids, repo_treats_promisor_packs, write_promisor_marker,
};
use grit_lib::refs;
use grit_lib::repo::Repository;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Once;

use crate::commands::index_pack;
use crate::fetch_transport;

static LAZY_FETCH_DISABLED_WARN: Once = Once::new();

/// Match Git `git_env_bool("GIT_NO_LAZY_FETCH", 0)`: unset or empty → lazy fetch allowed; `0` /
/// `false` / `no` / `off` → allowed; truthy spellings → disabled. Invalid values error like Git.
pub(crate) fn git_no_lazy_fetch_env_disables_lazy() -> Result<bool> {
    let raw = match std::env::var("GIT_NO_LAZY_FETCH") {
        Err(_) => return Ok(false),
        Ok(s) if s.trim().is_empty() => return Ok(false),
        Ok(s) => s,
    };
    let t = raw.trim();
    let lower = t.to_ascii_lowercase();
    Ok(match lower.as_str() {
        "0" | "false" | "no" | "off" => false,
        "1" | "true" | "yes" | "on" => true,
        _ => bail!("bad boolean environment value '{t}' for 'GIT_NO_LAZY_FETCH'"),
    })
}

pub(crate) fn warn_lazy_fetch_disabled_once() {
    LAZY_FETCH_DISABLED_WARN.call_once(|| {
        eprintln!("warning: lazy fetching disabled; some objects may not be available");
    });
}

/// Whether this process may perform promisor lazy-fetch, matching the client side of Git's
/// `upload-pack` → `pack-objects` pairing: `upload-pack` defaults `GIT_NO_LAZY_FETCH=1` for the
/// pack-objects child, so an **unset** variable means lazy fetch is off unless explicitly set to
/// `0` / `false` / `no` / `off` (`t0411-clone-from-partial` test 6 vs 7).
pub(crate) fn promisor_lazy_fetch_allowed_for_client_process() -> Result<bool> {
    let raw = match std::env::var("GIT_NO_LAZY_FETCH") {
        Err(_) => return Ok(false),
        Ok(s) if s.trim().is_empty() => return Ok(false),
        Ok(s) => s,
    };
    let t = raw.trim();
    let lower = t.to_ascii_lowercase();
    Ok(match lower.as_str() {
        "0" | "false" | "no" | "off" => true,
        "1" | "true" | "yes" | "on" => false,
        _ => bail!("bad boolean environment value '{t}' for 'GIT_NO_LAZY_FETCH'"),
    })
}

/// Resolved promisor object source: local ODB path or HTTP remote (system `git fetch`).
pub(crate) enum PromisorSource {
    Local(grit_lib::odb::Odb),
    Http { remote: String },
}

/// Resolve `remote.<name>.url` into a [`PromisorSource`] (local ODB or HTTP).
fn open_promisor_remote_named(
    config: &ConfigSet,
    git_dir: &Path,
    name: &str,
) -> Result<Option<PromisorSource>> {
    let base = git_dir
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| git_dir.to_path_buf());
    let url_key = format!("remote.{name}.url");
    let Some(url) = config.get(&url_key) else {
        return Ok(None);
    };
    if url.starts_with("http://") || url.starts_with("https://") {
        return Ok(Some(PromisorSource::Http {
            remote: name.to_string(),
        }));
    }
    let path = resolve_remote_repo_path(&base, &url)?;
    let objects_dir = if path.join("objects").is_dir() {
        path.join("objects")
    } else if path.file_name().is_some_and(|n| n == ".git") || path.ends_with(".git") {
        path.join("objects")
    } else {
        path.join(".git").join("objects")
    };
    // Keep a promisor entry even when the source path was removed after clone (t0411): lazy fetch
    // uses `upload-pack` against the recorded git dir, not this ODB.
    Ok(Some(PromisorSource::Local(grit_lib::odb::Odb::new(
        &objects_dir,
    ))))
}

/// Ordered list of `(remote_name, source)` for promisor fetches: `extensions.partialclone` remote
/// first (when set), then each `remote.*.promisor=true` remote.
pub(crate) fn list_promisor_remotes(
    config: &ConfigSet,
    git_dir: &Path,
) -> Result<Vec<(String, PromisorSource)>> {
    let mut out: Vec<(String, PromisorSource)> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    if let Some(pc) = config.get("extensions.partialclone") {
        let name = pc.trim();
        if !name.is_empty() && seen.insert(name.to_string()) {
            if let Some(src) = open_promisor_remote_named(config, git_dir, name)? {
                out.push((name.to_string(), src));
            }
        }
    }

    for e in config.entries() {
        if !e.key.ends_with(".promisor") {
            continue;
        }
        if e.value.as_deref() != Some("true") {
            continue;
        }
        let Some(rest) = e.key.strip_prefix("remote.") else {
            continue;
        };
        let Some((name, _)) = rest.split_once('.') else {
            continue;
        };
        if !seen.insert(name.to_string()) {
            continue;
        }
        if let Some(src) = open_promisor_remote_named(config, git_dir, name)? {
            out.push((name.to_string(), src));
        }
    }

    Ok(out)
}

/// Locate the first promisor remote (same order as [`list_promisor_remotes`]) and open its object
/// store or HTTP name.
pub(crate) fn find_promisor_source(
    config: &ConfigSet,
    git_dir: &Path,
) -> Result<Option<PromisorSource>> {
    Ok(list_promisor_remotes(config, git_dir)?
        .into_iter()
        .next()
        .map(|(_, s)| s))
}

/// Try to lazy-fetch `oid` from a configured promisor remote into a new promisor pack.
///
/// Returns `Ok(())` when the object is present locally after the attempt. Matches Git's partial
/// clone behavior for `cat-file` / missing object reads.
pub(crate) fn try_lazy_fetch_promisor_object(repo: &Repository, oid: ObjectId) -> Result<()> {
    let config = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
    if !repo_treats_promisor_packs(&repo.git_dir, &config) {
        bail!("not a promisor repository");
    }
    if git_no_lazy_fetch_env_disables_lazy()? {
        warn_lazy_fetch_disabled_once();
        bail!("lazy fetching disabled");
    }
    if repo.odb.exists_local(&oid) {
        return Ok(());
    }

    for (remote_name, src) in list_promisor_remotes(&config, &repo.git_dir)? {
        match &src {
            PromisorSource::Local(odb) => {
                let remote_git_dir = odb
                    .objects_dir()
                    .parent()
                    .map(Path::to_path_buf)
                    .with_context(|| "promisor local objects_dir has no parent")?;
                let upload_pack = config
                    .get(&format!("remote.{remote_name}.uploadpack"))
                    .filter(|s| !s.is_empty());
                let pack = match fetch_transport::fetch_upload_pack_explicit_wants(
                    &repo.git_dir,
                    &remote_git_dir,
                    upload_pack.as_deref(),
                    &[oid],
                ) {
                    Ok(p) => p,
                    Err(_) => continue,
                };
                let pack_path = index_pack::ingest_pack_bytes(repo, &pack, true)
                    .with_context(|| format!("indexing promisor pack from {remote_name}"))?;
                let promisor_marker = pack_path.with_extension("promisor");
                let _ = std::fs::File::create(&promisor_marker);
                if repo.odb.read(&oid).is_ok() {
                    return Ok(());
                }
            }
            PromisorSource::Http { remote } => {
                if run_system_git_fetch_objects(repo, remote, &[oid]).is_ok()
                    && repo.odb.read(&oid).is_ok()
                {
                    return Ok(());
                }
            }
        }
    }

    bail!("could not fetch {} from promisor remote", oid.to_hex());
}

fn resolve_remote_repo_path(base: &Path, url: &str) -> Result<PathBuf> {
    let path_str = url.strip_prefix("file://").unwrap_or(url);
    let p = Path::new(path_str);
    let p = if p.is_absolute() {
        p.to_path_buf()
    } else {
        base.join(p)
    };
    // Prefer a canonical path when the directory exists; if the source was removed after clone
    // (t0411), keep the configured path so `upload-pack` can still be invoked the same way Git does.
    Ok(p.canonicalize().unwrap_or(p))
}

/// Drop promisor-marker entries for blobs already present locally so
/// `rev-list --missing=print` matches Git.
pub(crate) fn trim_promisor_marker_to_missing_local(dest: &Repository) -> Result<()> {
    let mut oids: HashSet<ObjectId> = read_promisor_missing_oids(&dest.git_dir)
        .into_iter()
        .collect();
    oids.retain(|oid| !dest.odb.exists_local(oid));
    write_promisor_marker(&dest.git_dir, &oids).map_err(|e| anyhow::anyhow!(e))
}

/// After `sparse-checkout set` / `add`, materialize tip blobs matching `patterns` from the
/// promisor remote when this is a partial clone (`grit-promisor-missing` is non-empty).
pub(crate) fn hydrate_sparse_patterns_after_sparse_checkout_update(
    repo: &Repository,
    patterns: &[String],
    cone_mode: bool,
) -> Result<()> {
    if read_promisor_missing_oids(&repo.git_dir).is_empty() {
        return Ok(());
    }
    let config = ConfigSet::load(Some(&repo.git_dir), true)?;
    let Some(promisor) = find_promisor_source(&config, &repo.git_dir)? else {
        return Ok(());
    };
    hydrate_sparse_tip_blobs_from_promisor(repo, &promisor, patterns, cone_mode)?;
    trim_promisor_marker_to_missing_local(repo)
}

/// Copy blobs under `HEAD` matching sparse-checkout `patterns` from the promisor remote.
pub(crate) fn hydrate_sparse_tip_blobs_from_promisor(
    dest: &Repository,
    promisor: &PromisorSource,
    patterns: &[String],
    cone_mode: bool,
) -> Result<()> {
    let head_oid = refs::resolve_ref(&dest.git_dir, "HEAD")?;
    let obj = dest
        .odb
        .read(&head_oid)
        .context("reading HEAD for sparse hydration")?;
    if obj.kind != ObjectKind::Commit {
        return Ok(());
    }
    let commit = parse_commit(&obj.data)?;
    let mut need = Vec::new();
    let mut seen_trees = HashSet::new();
    let mut seen_blobs = HashSet::new();
    collect_sparse_missing_blobs_from_tree(
        dest,
        promisor,
        commit.tree,
        "",
        patterns,
        cone_mode,
        &mut seen_trees,
        &mut seen_blobs,
        &mut need,
    )?;
    flush_promisor_blob_batches(dest, promisor, &mut need, 50_000)
}

/// Copy every blob under `tree_oid` that is not yet present in the local ODB (loose or pack).
pub(crate) fn hydrate_tree_blobs_from_promisor(
    dest: &Repository,
    promisor: &PromisorSource,
    tree_oid: ObjectId,
) -> Result<()> {
    let mut need = Vec::new();
    let mut seen_trees = HashSet::new();
    let mut seen_blobs = HashSet::new();
    collect_all_missing_blobs_from_tree(
        dest,
        tree_oid,
        &mut seen_trees,
        &mut seen_blobs,
        &mut need,
    )?;
    flush_promisor_blob_batches(dest, promisor, &mut need, 50_000)
}

/// Copy every blob reachable from `HEAD`'s tree from the promisor remote.
pub(crate) fn hydrate_head_tree_blobs_from_promisor(
    dest: &Repository,
    promisor: &PromisorSource,
) -> Result<()> {
    let head_oid = refs::resolve_ref(&dest.git_dir, "HEAD")?;
    let obj = dest
        .odb
        .read(&head_oid)
        .context("reading HEAD for hydration")?;
    if obj.kind != ObjectKind::Commit {
        return Ok(());
    }
    let commit = parse_commit(&obj.data)?;
    let mut need = Vec::new();
    let mut seen_trees = HashSet::new();
    let mut seen_blobs = HashSet::new();
    collect_all_missing_blobs_from_tree(
        dest,
        commit.tree,
        &mut seen_trees,
        &mut seen_blobs,
        &mut need,
    )?;
    flush_promisor_blob_batches(dest, promisor, &mut need, 50_000)
}

fn collect_sparse_missing_blobs_from_tree(
    dest: &Repository,
    promisor: &PromisorSource,
    tree_oid: ObjectId,
    prefix: &str,
    patterns: &[String],
    cone_mode: bool,
    seen_trees: &mut HashSet<ObjectId>,
    seen_blobs: &mut HashSet<ObjectId>,
    need: &mut Vec<ObjectId>,
) -> Result<()> {
    if !seen_trees.insert(tree_oid) {
        return Ok(());
    }
    let tree_obj = dest
        .odb
        .read(&tree_oid)
        .context("reading tree for sparse hydration")?;
    if tree_obj.kind != ObjectKind::Tree {
        return Ok(());
    }
    for entry in parse_tree(&tree_obj.data)? {
        if entry.mode == 0o160000 {
            continue;
        }
        let name = String::from_utf8_lossy(&entry.name).to_string();
        let rel = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        let is_dir = entry.mode == 0o040000;
        let pat_path = if is_dir {
            format!("{rel}/")
        } else {
            rel.clone()
        };
        let included = if is_dir && !cone_mode {
            true
        } else {
            super::sparse_checkout::path_matches_sparse_patterns(&pat_path, patterns, cone_mode)
        };
        if !included {
            continue;
        }
        if is_dir {
            collect_sparse_missing_blobs_from_tree(
                dest, promisor, entry.oid, &rel, patterns, cone_mode, seen_trees, seen_blobs, need,
            )?;
            continue;
        }
        if dest.odb.exists_local(&entry.oid) {
            continue;
        }
        if !seen_blobs.insert(entry.oid) {
            continue;
        }
        need.push(entry.oid);
    }
    Ok(())
}

fn collect_all_missing_blobs_from_tree(
    dest: &Repository,
    tree_oid: ObjectId,
    seen_trees: &mut HashSet<ObjectId>,
    seen_blobs: &mut HashSet<ObjectId>,
    need: &mut Vec<ObjectId>,
) -> Result<()> {
    if !seen_trees.insert(tree_oid) {
        return Ok(());
    }
    let tree_obj = dest
        .odb
        .read(&tree_oid)
        .context("reading tree for hydration")?;
    if tree_obj.kind != ObjectKind::Tree {
        return Ok(());
    }
    for entry in parse_tree(&tree_obj.data)? {
        if entry.mode == 0o160000 {
            continue;
        }
        if (entry.mode & 0o170000) == 0o040000 {
            collect_all_missing_blobs_from_tree(dest, entry.oid, seen_trees, seen_blobs, need)?;
            continue;
        }
        if dest.odb.exists_local(&entry.oid) {
            continue;
        }
        if !seen_blobs.insert(entry.oid) {
            continue;
        }
        need.push(entry.oid);
    }
    Ok(())
}

fn flush_promisor_blob_batches(
    repo: &Repository,
    promisor: &PromisorSource,
    need: &mut Vec<ObjectId>,
    min_batch: usize,
) -> Result<()> {
    let min_batch = min_batch.max(1);
    let mut batch: Vec<ObjectId> = Vec::new();
    for oid in need.drain(..) {
        batch.push(oid);
        if batch.len() >= min_batch {
            flush_promisor_blob_batch(repo, promisor, &mut batch)?;
        }
    }
    flush_promisor_blob_batch(repo, promisor, &mut batch)?;
    Ok(())
}

/// Copy up to `batch.len()` blobs from the promisor into `repo`, emit trace2 `promisor fetch_count`,
/// then clear `batch`.
pub(crate) fn flush_promisor_blob_batch(
    repo: &Repository,
    promisor: &PromisorSource,
    batch: &mut Vec<ObjectId>,
) -> Result<()> {
    if batch.is_empty() {
        return Ok(());
    }

    let count = batch.len();
    match promisor {
        PromisorSource::Local(odb) => {
            for oid in batch.drain(..) {
                let obj = odb
                    .read(&oid)
                    .with_context(|| format!("promisor remote missing object {}", oid.to_hex()))?;
                repo.odb
                    .write(obj.kind, &obj.data)
                    .with_context(|| format!("writing {}", oid.to_hex()))?;
            }
        }
        PromisorSource::Http { remote } => {
            let oids: Vec<ObjectId> = std::mem::take(batch);
            for oid in &oids {
                run_system_git_fetch_objects(repo, remote, &[*oid])?;
                let _ = repo
                    .odb
                    .read(oid)
                    .with_context(|| format!("object {} not present after fetch", oid.to_hex()))?;
            }
        }
    }

    if let Ok(p) = std::env::var("GIT_TRACE2_EVENT") {
        if !p.is_empty() {
            let _ = crate::trace2_write_json_data_line(
                &p,
                "promisor",
                "fetch_count",
                &count.to_string(),
            );
        }
    }

    Ok(())
}

fn system_git_binary() -> PathBuf {
    std::env::var("GIT_REAL_GIT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/usr/bin/git"))
}

fn run_system_git_fetch_objects(repo: &Repository, remote: &str, oids: &[ObjectId]) -> Result<()> {
    if oids.is_empty() {
        return Ok(());
    }
    let repo_root = repo.work_tree.as_deref().unwrap_or(repo.git_dir.as_path());
    let mut cmd = Command::new(system_git_binary());
    cmd.arg("-C").arg(repo_root);
    cmd.arg("fetch");
    cmd.arg(remote);
    for oid in oids {
        cmd.arg(oid.to_hex());
    }
    let status = cmd
        .status()
        .with_context(|| format!("failed to spawn {}", system_git_binary().display()))?;
    if !status.success() {
        bail!(
            "git fetch {} for {} objects failed with status {:?}",
            remote,
            oids.len(),
            status
        );
    }
    Ok(())
}
