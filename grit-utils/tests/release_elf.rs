//! Exercise the release guard against actual linker outputs, including insecure probes.
#![allow(clippy::unwrap_used, clippy::expect_used)]
#![cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]

use std::path::{Path, PathBuf};
use std::process::Command;

fn probe(directory: &Path, flags: &[&str]) -> PathBuf {
    let source = directory.join("probe.c");
    let binary = directory.join("probe");
    std::fs::write(&source, "#include <stdio.h>\nstatic const char * volatile name = \"probe\";\nint main(void) { puts(name); return 0; }\n").unwrap();
    let output = Command::new("cc")
        .arg(&source)
        .arg("-o")
        .arg(&binary)
        .args(flags)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    binary
}

fn check(binary: &Path, relr: &str, baseline: &str) -> std::process::Output {
    Command::new("python3")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../scripts/check-linux-release.py"))
        .arg(binary)
        .args(["--relr", relr, "--max-glibc", baseline])
        .output()
        .unwrap()
}

#[test]
fn relr_guard_reproduces_unpacked_startup_metadata_and_checks_candidate() {
    let directory = tempfile::tempdir().unwrap();
    let binary = probe(directory.path(), &["-fPIE", "-pie", "-Wl,-z,relro,-z,now"]);
    let stock = check(&binary, "required", "2.39");
    assert!(!stock.status.success());
    assert!(String::from_utf8_lossy(&stock.stderr).contains("RELR must be required"));
    assert!(check(&binary, "forbidden", "2.34").status.success());
    let binary = probe(
        directory.path(),
        &[
            "-fPIE",
            "-pie",
            "-Wl,-z,relro,-z,now,-z,pack-relative-relocs",
        ],
    );
    let candidate = check(&binary, "required", "2.36");
    assert!(
        candidate.status.success(),
        "{}",
        String::from_utf8_lossy(&candidate.stderr)
    );
    assert!(!check(&binary, "required", "2.34").status.success());
    assert!(!check(&binary, "forbidden", "2.39").status.success());
}

#[test]
fn guard_rejects_non_pie_lazy_binding_and_newer_loader_requirements() {
    let directory = tempfile::tempdir().unwrap();
    let binary = probe(directory.path(), &["-no-pie", "-Wl,-z,now"]);
    assert!(!check(&binary, "forbidden", "2.39").status.success());
    let binary = probe(directory.path(), &["-fPIE", "-pie", "-Wl,-z,lazy"]);
    assert!(!check(&binary, "forbidden", "2.39").status.success());
    let binary = probe(directory.path(), &["-fPIE", "-pie", "-Wl,-z,now"]);
    assert!(!check(&binary, "forbidden", "2.2.5").status.success());
}

#[test]
#[ignore = "set GRIT_RELEASE_ELF to the candidate release artifact; checks its actual loader contract"]
fn candidate_release_elf_contract() {
    let binary = PathBuf::from(std::env::var_os("GRIT_RELEASE_ELF").expect("GRIT_RELEASE_ELF"));
    let output = check(&binary, "required", "2.36");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
