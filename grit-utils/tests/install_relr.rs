//! Exercise the shipped installers with isolated platform probes and downloads.
#![cfg(target_os = "linux")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

fn executable(path: &Path, body: &str) {
    std::fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn install(script: &str, arch: &str, getconf: &str, ldd: &str, missing: bool) -> Vec<String> {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let bin = root.join("bin");
    let payload = root.join("payload");
    std::fs::create_dir(&bin).unwrap();
    std::fs::create_dir(&payload).unwrap();
    executable(&payload.join("grit"), "echo 'grit installer-test'");
    assert!(Command::new("tar")
        .args(["czf"])
        .arg(root.join("payload.tar.gz"))
        .arg("-C")
        .arg(&payload)
        .arg("grit")
        .status()
        .unwrap()
        .success());
    executable(
        &bin.join("uname"),
        "case \"$1\" in -s) echo Linux;; -m) echo \"$TEST_ARCH\";; esac",
    );
    executable(
        &bin.join("getconf"),
        "printf '%s\\n' \"$TEST_GETCONF\"; [ -n \"$TEST_GETCONF\" ]",
    );
    executable(&bin.join("ldd"), "printf '%s\\n' \"$TEST_LDD\"");
    // Keep host libc detection out of this isolated platform fixture.
    executable(&bin.join("ls"), "exit 1");
    executable(
        &bin.join("grep"),
        "case \"$*\" in */etc/os-release*) exit 1;; esac\nexec /usr/bin/grep \"$@\"",
    );
    executable(
        &bin.join("curl"),
        r#"
echo "$2" >> "$TEST_ROOT/urls"
case "$2" in *-relr.tar.gz) [ "$TEST_MISSING" = 0 ] || exit 22;; esac
cp "$TEST_ROOT/payload.tar.gz" "$4"
"#,
    );
    let output = Command::new("/bin/sh")
        .arg(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../site/public")
                .join(script),
        )
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("INSTALL_DIR", root.join("installed"))
        .env("TEST_ROOT", root)
        .env("TEST_ARCH", arch)
        .env("TEST_GETCONF", getconf)
        .env("TEST_LDD", ldd)
        .env("TEST_MISSING", if missing { "1" } else { "0" })
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("grit installer-test"));
    std::fs::read_to_string(root.join("urls"))
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect()
}

#[test]
fn installers_select_relr_only_for_known_supported_x86_64_glibc() {
    for script in ["install", "install-nightly"] {
        for (getconf, ldd, relr) in [
            ("glibc 2.34", "ldd (GNU libc) 2.39", false),
            ("glibc 2.35", "", false),
            ("glibc 2.36", "", true),
            ("glibc 2.39", "", true),
            ("glibc 3.0", "", true),
            ("", "ldd (GNU libc) 2.36", true),
            ("", "ldd (GNU C Library) 2.36", true),
            ("invalid", "ldd (Debian GLIBC 2.36-9+deb12u10) 2.36", true),
            ("", "ldd (Ubuntu GLIBC 2.35-0ubuntu3) 2.35", false),
            ("glibc 2.36-extra", "", false),
            ("glibc 2.36\njunk", "", false),
            ("glibc 2.36\nglibc 2.39", "", false),
            ("glibc 2.x", "ldd 2.39", false),
            ("", "ldd (unknown libc) 2.39", false),
            ("", "", false),
        ] {
            let urls = install(script, "x86_64", getconf, ldd, false);
            assert_eq!(urls.len(), 1);
            assert_eq!(
                urls[0].ends_with("-gnu-relr.tar.gz"),
                relr,
                "{script}: {getconf:?} / {ldd:?}"
            );
            assert!(urls[0].contains(if script == "install" {
                "/latest/download/"
            } else {
                "/download/nightly/"
            }));
        }
        let urls = install(script, "aarch64", "glibc 2.39", "", false);
        assert!(urls[0].ends_with("aarch64-unknown-linux-gnu.tar.gz"));
        let urls = install(
            script,
            "x86_64",
            "",
            "musl libc (x86_64)\nVersion 1.2.5",
            false,
        );
        assert!(urls[0].ends_with("x86_64-unknown-linux-musl.tar.gz"));
    }
}

#[test]
fn missing_relr_asset_falls_back_to_existing_gnu_asset() {
    for script in ["install", "install-nightly"] {
        let urls = install(script, "x86_64", "glibc 2.36", "", true);
        assert_eq!(urls.len(), 2);
        assert!(urls[0].ends_with("-gnu-relr.tar.gz"));
        assert!(urls[1].ends_with("-gnu.tar.gz"));
    }
}
