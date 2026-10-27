//! Gate guard for issue #2144: `grit push` of a whole repository over smart
//! HTTP must stay within twice the peak memory of `git push` to the same stock
//! `git http-backend` server, and within twice the time in optimized builds.
//! The pushed repository must pass `git fsck`. A cargo-shaped history covers the
//! object walk and delta reuse; a pack of large incompressible blobs catches
//! anything that holds the pack in memory.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use grit_utils::network_http::GitHttpBackendServer;
use grit_utils::repack_fixture::{build_cargo_shape_repo, CargoShape};
use tempfile::TempDir;

const TIME_SLACK: Duration = Duration::from_millis(500);
const RSS_SLACK: u64 = 8 * 1024 * 1024;

fn resolve_git() -> PathBuf {
    grit_utils::binary::resolve_binary("git", None).expect("git")
}

/// Build the `grit` binary with the same profile as this test.
fn build_grit() -> PathBuf {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let release = !cfg!(debug_assertions);
    let mut args = vec!["build", "-p", "grit-cli", "--bin", "grit", "-q"];
    if release {
        args.push("--release");
    }
    let status = Command::new(env!("CARGO"))
        .args(&args)
        .current_dir(&workspace)
        .status()
        .expect("cargo build grit");
    assert!(status.success());
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace.join("target"));
    target
        .join(if release { "release" } else { "debug" })
        .join(format!("grit{}", std::env::consts::EXE_SUFFIX))
}

/// Peak RSS and wall time of `command`, run in a fresh `grit-bench measure-rss` helper.
fn measure(cwd: &Path, command: &str) -> (u64, Duration) {
    let start = Instant::now();
    let out = Command::new(env!("CARGO_BIN_EXE_grit-bench"))
        .arg("measure-rss")
        .arg("--cwd")
        .arg(cwd)
        .env("GRIT_BENCH_MEASURE_CMD", command)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .expect("measure-rss");
    let elapsed = start.elapsed();
    assert!(
        out.status.success(),
        "{command}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let rss = String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse()
        .expect("rss");
    (rss, elapsed)
}

fn git(git: &Path, dir: &Path, args: &[&str]) -> String {
    let out = Command::new(git)
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .expect("git");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

fn quote(path: &Path) -> String {
    grit_utils::shell::shell_quote(path.to_str().expect("utf-8 path"))
}

/// Push `main` of `fixture` with git and with grit to empty bare repositories on
/// one `git http-backend` server, and check grit against git's budgets: peak
/// memory above the baseline within `rss_percent` of git's, and time within 2x.
fn check_push_budgets(
    label: &str,
    git_bin: &Path,
    grit: &Path,
    tmp: &Path,
    fixture: &Path,
    rss_percent: u64,
) {
    let tip = git(git_bin, fixture, &["rev-parse", "main"]);
    let root = tmp.join(format!("srv-{label}"));
    std::fs::create_dir_all(&root).expect("mkdir srv");
    for name in ["git.git", "grit.git"] {
        git(git_bin, &root, &["init", "-q", "--bare", name]);
    }
    let server = GitHttpBackendServer::start(git_bin, root.clone()).expect("http-backend");
    git(
        git_bin,
        fixture,
        &["remote", "add", "tgit", &server.url("git.git")],
    );
    git(
        git_bin,
        fixture,
        &["remote", "add", "tgrit", &server.url("grit.git")],
    );

    // Code pages and start-up allocations do not scale with the repository (and
    // are much larger for a debug build), so memory is compared above each
    // tool's own baseline: reading one commit in the same repository.
    let (grit_base, _) = measure(fixture, &format!("{} log -n 1", quote(grit)));
    let (git_base, _) = measure(fixture, &format!("{} log -1", quote(git_bin)));
    let (git_rss, git_time) = measure(fixture, &format!("{} push -q tgit main", quote(git_bin)));
    let (grit_rss, grit_time) = measure(fixture, &format!("{} push tgrit main", quote(grit)));

    let pushed = root.join("grit.git");
    assert_eq!(git(git_bin, &pushed, &["rev-parse", "main"]), tip);
    git(git_bin, &pushed, &["fsck", "--strict", "--no-dangling"]);
    eprintln!(
        "{label}: git {git_time:?} {git_rss} B rss (base {git_base} B); \
         grit {grit_time:?} {grit_rss} B rss (base {grit_base} B)"
    );

    let grit_excess = grit_rss.saturating_sub(grit_base);
    let git_excess = git_rss.saturating_sub(git_base);
    assert!(
        grit_excess * 100 <= git_excess * rss_percent + RSS_SLACK * 100,
        "{label}: grit push peak RSS {grit_rss} B ({grit_excess} B over `log -n 1`) \
         vs git {git_rss} B ({git_excess} B over `log -1`)"
    );
    if !cfg!(debug_assertions) {
        assert!(
            grit_time <= 2 * git_time + TIME_SLACK,
            "{label}: grit push took {grit_time:?} vs git {git_time:?}"
        );
    }
}

/// Deterministic incompressible bytes.
fn noise(len: usize, seed: u64) -> Vec<u8> {
    let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    let mut out = Vec::with_capacity(len + 8);
    while out.len() < len {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        out.extend_from_slice(&state.to_le_bytes());
    }
    out.truncate(len);
    out
}

#[test]
fn push_over_http_stays_within_git_budgets_on_cargo_shape() {
    if !cfg!(target_os = "linux") {
        return;
    }
    let git_bin = resolve_git();
    let grit = build_grit();
    let tmp = TempDir::new().expect("tempdir");
    let fixture = tmp.path().join("fixture");
    build_cargo_shape_repo(&git_bin, &fixture, CargoShape::SMOKE).expect("fixture");
    check_push_budgets("cargo shape", &git_bin, &grit, tmp.path(), &fixture, 200);
}

#[test]
fn push_over_http_of_large_blobs_does_not_hold_the_pack() {
    if !cfg!(target_os = "linux") {
        return;
    }
    let git_bin = resolve_git();
    let grit = build_grit();
    let tmp = TempDir::new().expect("tempdir");
    let fixture = tmp.path().join("blobs");
    std::fs::create_dir_all(&fixture).expect("mkdir");
    git(&git_bin, &fixture, &["init", "-q", "-b", "main"]);
    for i in 0..4u64 {
        let name = format!("blob{i}.bin");
        std::fs::write(fixture.join(&name), noise(16 << 20, i + 1)).expect("write blob");
        git(&git_bin, &fixture, &["add", &name]);
        git(
            &git_bin,
            &fixture,
            &[
                "-c",
                "user.name=T",
                "-c",
                "user.email=t@example.com",
                "commit",
                "-q",
                "-m",
                &name,
            ],
        );
    }
    git(&git_bin, &fixture, &["repack", "-adq"]);
    // Both tools copy these blobs from the mapped pack, so nothing but the
    // mapping should grow with the pack; the bound is tighter than 2x.
    check_push_budgets("large blobs", &git_bin, &grit, tmp.path(), &fixture, 150);
}
