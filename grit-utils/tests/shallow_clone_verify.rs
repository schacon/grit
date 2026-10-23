//! Shallow depth-1 clone semantics must reject full clones (daily benchmark regression).

use std::path::PathBuf;
use std::process::Command;

use grit_utils::fixture_registry::FixtureKind;
use grit_utils::network_verify::verify_shallow_depth_one_clone;

fn resolve_git() -> PathBuf {
    grit_utils::binary::resolve_binary("git", None).expect("git")
}

#[test]
fn full_clone_fails_shallow_depth_one_verify() {
    let git = resolve_git();
    let cache = tempfile::TempDir::new().expect("cache");
    std::env::set_var("GRIT_BENCH_CACHE", cache.path());
    let (bare, _record) =
        grit_utils::fixture_registry::ensure_cached(&git, FixtureKind::Synth10kSmoke, false)
            .expect("synth-10k bare");
    let bare_count = Command::new(&git)
        .args(["rev-list", "--count", "HEAD"])
        .current_dir(&bare)
        .output()
        .expect("rev-list bare");
    assert!(bare_count.status.success());
    let bare_n: u64 = String::from_utf8_lossy(&bare_count.stdout)
        .trim()
        .parse()
        .expect("bare count");
    assert!(
        bare_n > 1,
        "fixture must have deep history for shallow test (got {bare_n})"
    );

    let dest = cache.path().join("full-clone");
    let dest_name = dest.file_name().unwrap().to_str().unwrap();
    let parent = dest.parent().unwrap();
    let status = Command::new(&git)
        .args(["clone", "-q", bare.to_str().unwrap(), dest_name])
        .current_dir(parent)
        .status()
        .expect("git clone");
    assert!(status.success(), "git clone failed");

    let err = verify_shallow_depth_one_clone(&git, &bare, &dest, "main")
        .expect_err("full clone must not pass shallow-depth1 verify");
    let msg = err.to_string();
    assert!(
        msg.contains("shallow") || msg.contains("rev-list --count HEAD expected 1"),
        "unexpected error: {msg}"
    );
}
