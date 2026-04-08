//! Copy missing blobs from the configured promisor remote into the local object store.
//!
//! Used by partial-clone hydration, `sparse-checkout` updates, and `backfill`.

use anyhow::{bail, Context, Result};
use grit_lib::config::ConfigSet;
use grit_lib::objects::{parse_commit, parse_tree, ObjectId, ObjectKind};
use grit_lib::promisor::{read_promisor_missing_oids, write_promisor_marker};
use grit_lib::refs;
use grit_lib::repo::Repository;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Resolved promisor object source: local ODB path or HTTP remote (system `git fetch`).
pub(crate) enum PromisorSource {
    Local(grit_lib::odb::Odb),
    Http { remote: String },
}

/// Locate the first `remote.*.promisor=true` remote and open its object store or HTTP name.
pub(crate) fn find_promisor_source(
    config: &ConfigSet,
    git_dir: &Path,
) -> Result<Option<PromisorSource>> {
    let base = git_dir
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| git_dir.to_path_buf());

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
        let url_key = format!("remote.{name}.url");
        let Some(url) = config.get(&url_key) else {
            continue;
        };
        if url.starts_with("http://") || url.starts_with("https://") {
            return Ok(Some(PromisorSource::Http {
                remote: name.to_string(),
            }));
        }
        let path = match resolve_remote_repo_path(&base, &url) {
            Ok(p) => p,
            Err(_) => continue,
        };
        let objects_dir = if path.join("objects").is_dir() {
            path.join("objects")
        } else if path.file_name().is_some_and(|n| n == ".git") || path.ends_with(".git") {
            path.join("objects")
        } else {
            path.join(".git").join("objects")
        };
        if objects_dir.is_dir() {
            return Ok(Some(PromisorSource::Local(grit_lib::odb::Odb::new(
                &objects_dir,
            ))));
        }
    }
    Ok(None)
}

fn resolve_remote_repo_path(base: &Path, url: &str) -> Result<PathBuf> {
    let path_str = url.strip_prefix("file://").unwrap_or(url);
    let p = Path::new(path_str);
    let p = if p.is_absolute() {
        p.to_path_buf()
    } else {
        base.join(p)
    };
    p.canonicalize()
        .with_context(|| format!("resolving promisor remote path {}", p.display()))
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
                    .with_context(|| format!("promisor remote missing blob {}", oid.to_hex()))?;
                if obj.kind != ObjectKind::Blob {
                    bail!("promisor object {} is not a blob", oid.to_hex());
                }
                repo.odb
                    .write(ObjectKind::Blob, &obj.data)
                    .with_context(|| format!("writing blob {}", oid.to_hex()))?;
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
