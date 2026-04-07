//! Regenerate `git <cmd> -h` synopsis snippets from vendored `git/Documentation/*.adoc` for t0450.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::path::PathBuf;
use std::process::Command;

fn main() {
    let manifest_dir =
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let script = manifest_dir.join("../scripts/generate-upstream-help-synopsis.py");
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let dst = out_dir.join("upstream_help_synopsis.rs");

    let out_file = std::fs::File::create(&dst).unwrap_or_else(|e| {
        panic!("create {}: {e}", dst.display());
    });

    let status = Command::new("python3")
        .arg(&script)
        .stdout(out_file)
        .status()
        .unwrap_or_else(|e| panic!("spawn python3 for {}: {e}", script.display()));

    if !status.success() {
        panic!("generate-upstream-help-synopsis.py failed with {status}");
    }

    let docs_dir = manifest_dir.join("../git/Documentation");
    println!("cargo:rerun-if-changed={}", script.display());
    println!("cargo:rerun-if-changed={}", docs_dir.display());
}
