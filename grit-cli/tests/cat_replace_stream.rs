//! Large-blob cat output must honor replacement objects in every output mode.

use std::path::Path;
use std::process::{Command, Output};

fn git(root: &Path, args: &[&str]) -> Vec<u8> {
    let out = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .expect("git");
    assert!(out.status.success(), "{args:?}: {:?}", out.stderr);
    out.stdout
}

fn cat(root: &Path, spec: &str, flags: &[&str], no_replace: bool) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_grit"));
    command.current_dir(root).args(["cat", spec]).args(flags);
    if no_replace {
        command.env("GIT_NO_REPLACE_OBJECTS", "1");
    }
    let out = command.output().expect("grit cat");
    assert!(out.status.success(), "{:?}", out.stderr);
    out
}

#[test]
fn cat_streams_replacement_payload_and_dispatches_on_replacement_kind() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    git(root, &["init", "-q"]);
    git(
        root,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "--allow-empty",
            "-qm",
            "initial",
        ],
    );
    std::fs::write(root.join("original"), b"original").expect("original");
    let original = git(root, &["hash-object", "-w", "original"]);
    let original = std::str::from_utf8(&original).expect("oid").trim();
    let payload = vec![0u8; 2 * 1024 * 1024];
    std::fs::write(root.join("replacement"), &payload).expect("replacement");
    let replacement = git(root, &["hash-object", "-w", "replacement"]);
    let replacement = std::str::from_utf8(&replacement).expect("oid").trim();
    let head = git(root, &["rev-parse", "HEAD"]);
    let head = std::str::from_utf8(&head).expect("oid").trim();

    for spec in [original, head] {
        git(
            root,
            &["update-ref", &format!("refs/replace/{spec}"), replacement],
        );
        let expected = git(root, &["cat-file", "-p", spec]);
        assert_eq!(expected, payload);
        assert_eq!(cat(root, spec, &[], false).stdout, expected);

        let out = cat(root, spec, &["--json"], false);
        let value: serde_json::Value = serde_json::from_slice(&out.stdout).expect("JSON");
        assert_eq!(value["kind"], "blob");
        assert_eq!(value["size"], payload.len());
        assert_eq!(value["content"]["omitted_inline"], true);
        assert!(value["content"].get("data_base64").is_none());

        let out = cat(root, spec, &["--markdown"], false);
        assert!(std::str::from_utf8(&out.stdout)
            .expect("Markdown")
            .contains("omitted"));
    }

    assert_eq!(cat(root, original, &[], true).stdout, b"original");
    let out = Command::new("git")
        .current_dir(root)
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .args(["cat-file", "-p", head])
        .output()
        .expect("git without replacements");
    assert!(out.status.success());
    assert_eq!(cat(root, head, &[], true).stdout, out.stdout);
}
