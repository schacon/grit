//! Helpers for partial-clone lazy object fetching.
//!
//! These helpers currently support local/file remotes only. When a command
//! needs objects that are missing locally, we fetch them from the configured
//! promisor remote and optionally append packet-trace lines when
//! `GIT_TRACE_PACKET` is set.

use anyhow::{bail, Context, Result};
use grit_lib::config::ConfigSet;
use grit_lib::objects::ObjectId;
use grit_lib::repo::Repository;
use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Fetch any requested objects that are missing from the local ODB.
///
/// If the repository is not configured as a partial clone (no promisor
/// remote), this is a no-op.
pub fn maybe_fetch_missing_objects(repo: &Repository, requested: &[ObjectId]) -> Result<()> {
    let mut seen = HashSet::new();
    let mut missing = Vec::new();
    for oid in requested {
        if !seen.insert(*oid) {
            continue;
        }
        if !repo.odb.exists(oid) {
            missing.push(*oid);
        }
    }

    if missing.is_empty() {
        return Ok(());
    }

    let Some(remote_url) = promisor_remote_url(repo)? else {
        return Ok(());
    };

    let remote_repo = open_remote_repo(&remote_url)
        .with_context(|| format!("opening promisor remote '{}'", remote_url))?;

    for oid in &missing {
        let obj = remote_repo
            .odb
            .read(oid)
            .with_context(|| format!("missing object {oid} on promisor remote"))?;
        let written = repo.odb.write(obj.kind, &obj.data)?;
        if written != *oid {
            bail!("promisor object hash mismatch for {oid}");
        }
    }

    write_trace_packet_lines(&missing);
    Ok(())
}

fn promisor_remote_url(repo: &Repository) -> Result<Option<String>> {
    let config = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
    let remote_name = promisor_remote_name(&config);
    let Some(name) = remote_name else {
        return Ok(None);
    };
    Ok(config.get(&format!("remote.{name}.url")))
}

fn promisor_remote_name(config: &ConfigSet) -> Option<String> {
    if let Some(name) = config.get("extensions.partialclone") {
        if !name.is_empty() {
            return Some(name);
        }
    }

    for entry in config.entries() {
        if !entry.key.starts_with("remote.") || !entry.key.ends_with(".promisor") {
            continue;
        }
        let Some(value) = entry.value.as_deref() else {
            continue;
        };
        if matches!(
            value.to_ascii_lowercase().as_str(),
            "true" | "yes" | "on" | "1"
        ) && entry.key.len() > "remote..promisor".len()
        {
            let name = entry
                .key
                .strip_prefix("remote.")
                .and_then(|s| s.strip_suffix(".promisor"))
                .unwrap_or_default()
                .to_string();
            if !name.is_empty() {
                return Some(name);
            }
        }
    }

    if matches!(
        config
            .get("remote.origin.promisor")
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str(),
        "true" | "yes" | "on" | "1"
    ) {
        return Some("origin".to_string());
    }
    None
}

fn open_remote_repo(url: &str) -> Result<Repository> {
    if url.contains("://") && !url.starts_with("file://") {
        bail!("unsupported promisor remote URL: {url}");
    }
    let remote_path = if let Some(stripped) = url.strip_prefix("file://") {
        PathBuf::from(stripped)
    } else {
        PathBuf::from(url)
    };

    if let Ok(repo) = Repository::open(&remote_path, None) {
        return Ok(repo);
    }
    Repository::open(&remote_path.join(".git"), Some(Path::new(&remote_path))).map_err(Into::into)
}

fn write_trace_packet_lines(wanted: &[ObjectId]) {
    let Ok(dest) = std::env::var("GIT_TRACE_PACKET") else {
        return;
    };
    if dest.is_empty() || dest == "0" || dest.eq_ignore_ascii_case("false") {
        return;
    }

    let target = if dest == "1" {
        "/dev/stderr".to_owned()
    } else {
        dest
    };

    let Ok(mut out) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(target)
    else {
        return;
    };

    for oid in wanted {
        let _ = writeln!(out, "want {}", oid.to_hex());
    }
    let _ = writeln!(out, "fetch> done");
}
