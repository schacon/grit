//! Gate: every `pub mod` declared in `grit-lib/src/lib.rs` has a runnable `rust` doctest
//! in its root module docs (`//!` on the module file).

use std::fs;
use std::path::{Path, PathBuf};

fn grit_lib_src() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn module_root(src: &Path, name: &str) -> Option<PathBuf> {
    let as_file = src.join(format!("{name}.rs"));
    if as_file.is_file() {
        return Some(as_file);
    }
    let as_dir = src.join(name).join("mod.rs");
    if as_dir.is_file() {
        return Some(as_dir);
    }
    None
}

fn module_doc_lines(path: &Path) -> String {
    let text = fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let mut doc = String::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("//!") {
            doc.push_str(rest.trim_start());
            doc.push('\n');
            continue;
        }
        if line.starts_with("#!") {
            continue;
        }
        if line.trim().is_empty() && doc.is_empty() {
            continue;
        }
        break;
    }
    doc
}

fn pub_mods_from_lib_rs() -> Vec<String> {
    let lib = grit_lib_src().join("lib.rs");
    let text = fs::read_to_string(&lib).expect("lib.rs");
    let mut mods = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("#[") {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("pub mod ") {
            if rest.contains('{') {
                continue;
            }
            let name = rest.trim_end_matches(';').trim();
            if name == "prelude" {
                continue;
            }
            mods.push(name.to_string());
        }
    }
    mods.sort();
    mods.dedup();
    mods
}

#[test]
fn every_public_module_has_rust_doctest_in_module_docs() {
    let src = grit_lib_src();
    let missing: Vec<String> = pub_mods_from_lib_rs()
        .into_iter()
        .filter_map(|name| {
            if name == "ewah_bench_api" {
                return None;
            }
            let path = module_root(&src, &name)?;
            let doc = module_doc_lines(&path);
            if doc.contains("```rust") {
                None
            } else {
                Some(format!(
                    "{name} ({}): add a ```rust doctest to //! module docs",
                    path.display()
                ))
            }
        })
        .collect();

    assert!(
        missing.is_empty(),
        "public modules missing ```rust doctests in module docs:\n{}",
        missing.join("\n")
    );
}
