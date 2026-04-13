//! Submodule registration and activation (Git `submodule.c` parity for tooling).
//!
//! Used by `ls-files --recurse-submodules` to decide when to enter a gitlink.

use std::path::Path;

use crate::config::{ConfigFile, ConfigScope, ConfigSet};
use crate::index::Index;
use crate::odb::Odb;
use crate::pathspec::pathspec_matches;

/// Maps a submodule work tree path (as in the index / `.gitmodules`) to its submodule section name.
#[derive(Debug, Clone)]
pub struct SubmoduleRegistration {
    /// `submodule.<name>` section name from `.gitmodules`.
    pub name: String,
    /// Normalized `submodule.<name>.path` (forward slashes, no trailing slash).
    pub path: String,
}

/// Load `.gitmodules` mappings: path → submodule name.
///
/// Reads the work tree file when present; otherwise falls back to the `.gitmodules` blob at stage 0
/// in `index` (sparse checkout / absent file), using `odb` to load the blob.
pub fn load_submodule_registrations(
    work_tree: &Path,
    index: Option<&Index>,
    odb: Option<&Odb>,
) -> Vec<SubmoduleRegistration> {
    let gitmodules_path = work_tree.join(".gitmodules");
    let content = if gitmodules_path.exists() {
        let Some(s) = std::fs::read_to_string(&gitmodules_path).ok() else {
            return Vec::new();
        };
        s
    } else if let (Some(ix), Some(db)) = (index, odb) {
        let Some(ie) = ix.get(b".gitmodules", 0) else {
            return Vec::new();
        };
        let Ok(obj) = db.read(&ie.oid) else {
            return Vec::new();
        };
        if obj.kind != crate::objects::ObjectKind::Blob {
            return Vec::new();
        }
        let Some(s) = String::from_utf8(obj.data).ok() else {
            return Vec::new();
        };
        s
    } else {
        return Vec::new();
    };

    let Ok(config) = ConfigFile::parse(&gitmodules_path, &content, ConfigScope::Local) else {
        return Vec::new();
    };
    #[derive(Default)]
    struct Fields {
        path: Option<String>,
        url: Option<String>,
    }
    let mut by_name: std::collections::BTreeMap<String, Fields> = std::collections::BTreeMap::new();
    for entry in &config.entries {
        let key = &entry.key;
        if !key.starts_with("submodule.") {
            continue;
        }
        let rest = &key["submodule.".len()..];
        let Some(dot) = rest.rfind('.') else {
            continue;
        };
        let name = &rest[..dot];
        let var = &rest[dot + 1..];
        let slot = by_name.entry(name.to_string()).or_default();
        match var {
            "path" => slot.path = entry.value.clone(),
            "url" => slot.url = entry.value.clone(),
            _ => {}
        }
    }

    let mut out = Vec::new();
    for (name, f) in by_name {
        if let (Some(path), Some(_url)) = (f.path, f.url) {
            let path = path.replace('\\', "/");
            let path = path.trim_end_matches('/').to_string();
            if !path.is_empty() {
                out.push(SubmoduleRegistration { name, path });
            }
        }
    }
    out
}

/// Returns the `.gitmodules` submodule section name for a gitlink path, if registered.
pub fn submodule_name_for_path<'a>(
    registrations: &'a [SubmoduleRegistration],
    path: &str,
) -> Option<&'a str> {
    let path = path.replace('\\', "/");
    registrations
        .iter()
        .find(|r| r.path == path)
        .map(|r| r.name.as_str())
}

/// Git `is_submodule_active` / `is_tree_submodule_active` for `ls-files --recurse-submodules`.
///
/// Returns false when `module_name` is `None` (path not listed in `.gitmodules`).
#[must_use]
pub fn is_submodule_active(
    config: &ConfigSet,
    module_name: Option<&str>,
    submodule_path: &str,
) -> bool {
    let Some(name) = module_name else {
        return false;
    };

    let active_key = format!("submodule.{name}.active");
    if let Some(res) = config.get_bool(&active_key) {
        return res.unwrap_or(false);
    }

    let patterns = config.get_all("submodule.active");
    if !patterns.is_empty() {
        return patterns.iter().any(|p| pathspec_matches(p, submodule_path));
    }

    let url_key = format!("submodule.{name}.url");
    config.get(&url_key).is_some()
}
