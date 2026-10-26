//! Large blob display must preserve raw bytes and the structured output contracts.

use std::process::Command;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use grit_lib::objects::ObjectKind;
use grit_lib::repo::{InitOptions, Repository};

#[test]
fn large_cat_matches_git_with_replace_refs_in_every_output_mode() {
    let dir = tempfile::tempdir().expect("tempdir");
    let repo = Repository::init(dir.path(), &InitOptions::default()).expect("init");
    let original = repo
        .odb()
        .write(ObjectKind::Blob, b"original")
        .expect("blob");
    let payload: Vec<u8> = (0..1024 * 1024 + 79).map(|i| (i % 251) as u8).collect();
    let replacement = repo.odb().write(ObjectKind::Blob, &payload).expect("blob");
    let replacements = repo.git_dir().join("refs/replace");
    std::fs::create_dir_all(&replacements).expect("replace directory");
    std::fs::write(
        replacements.join(original.to_hex()),
        format!("{}\n", replacement.to_hex()),
    )
    .expect("replace ref");
    let expected = Command::new("git")
        .current_dir(dir.path())
        .args(["cat-file", "blob", &original.to_hex()])
        .output()
        .expect("git cat-file");
    assert!(expected.status.success());
    assert_eq!(expected.stdout, payload);

    let cat = |mode: &[&str], no_replace: bool| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_grit"));
        command
            .current_dir(dir.path())
            .args(["cat", &original.to_hex()])
            .args(mode)
            .env_remove("GIT_NO_REPLACE_OBJECTS");
        if no_replace {
            command.env("GIT_NO_REPLACE_OBJECTS", "1");
        }
        let output = command.output().expect("cat");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        output.stdout
    };
    assert_eq!(cat(&[], false), expected.stdout);
    assert_eq!(cat(&[], true), b"original");

    let json: serde_json::Value = serde_json::from_slice(&cat(&["--json"], false)).expect("json");
    assert_eq!(json["size"], payload.len());
    assert_eq!(json["content"]["kind"], "blob");
    assert_eq!(json["content"]["encoding"], "base64");
    assert_eq!(json["content"]["data_base64"], STANDARD.encode(&payload));
    let markdown = String::from_utf8(cat(&["--markdown"], false)).expect("markdown");
    assert!(markdown.contains("binary blob"));
    assert!(markdown.contains(&format!("- **size:** {}", payload.len())));
}
