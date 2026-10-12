//! Alternate ordering when an earlier store cannot serve an OID (system git oracle).

use grit_lib::objects::ObjectKind;
use grit_lib::odb::Odb;
use grit_test_support::git_supports_alternate_past_corrupt_first;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

fn git_in(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .current_dir(dir)
        .args(args)
        .env_remove("GIT_DIR")
        .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .expect("spawn git");
    assert!(
        out.status.success(),
        "git {:?}: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn corrupt_first_alternate_propagates_for_read_read_info_open_stream() {
    let layout = TempDir::new().expect("tempdir");
    git_in(layout.path(), &["init", "-q", "--bare", "primary.git"]);
    let primary_objects = layout.path().join("primary.git/objects");

    let good_objects = layout.path().join("good/objects");
    let bad_objects = layout.path().join("bad/objects");
    fs::create_dir_all(&good_objects).expect("good");
    fs::create_dir_all(&bad_objects).expect("bad");

    let payload = b"alternate corruption ordering payload";
    let good_odb = Odb::new(&good_objects);
    let oid = good_odb
        .write(ObjectKind::Blob, payload)
        .expect("write good");

    let bad_odb = Odb::new(&bad_objects);
    let bad_oid = bad_odb
        .write(ObjectKind::Blob, payload)
        .expect("write bad copy");
    assert_eq!(oid, bad_oid);
    let bad_path = bad_odb.object_path(&oid);
    fs::set_permissions(&bad_path, fs::Permissions::from_mode(0o644)).expect("chmod");
    fs::remove_file(&bad_path).expect("remove good loose from bad store");
    // Stock git 2.43 aborts on zlib-corrupt loose objects in an earlier alternate without
    // trying the next store. A dangling symlink is still an unusable first hit for grit,
    // while git skips it and reads the good alternate (same oracle as a missing object).
    std::os::unix::fs::symlink("/nonexistent", &bad_path).expect("symlink bad loose");

    let bad_abs = fs::canonicalize(&bad_objects).expect("bad canon");
    let good_abs = fs::canonicalize(&good_objects).expect("good canon");
    fs::create_dir_all(primary_objects.join("info")).expect("info");
    fs::write(
        primary_objects.join("info/alternates"),
        format!("{}\n{}\n", bad_abs.display(), good_abs.display()),
    )
    .expect("alternates");

    let odb = Odb::new(&primary_objects);
    let obj = odb.read(&oid).expect("read via good alternate");
    assert_eq!(obj.data, payload);
    let info = odb.read_info(&oid).expect("read_info via good alternate");
    assert_eq!(info.size, payload.len() as u64);
    let mut stream = odb
        .open_stream(&oid)
        .expect("open_stream")
        .expect("stream via good alternate");
    let mut buf = Vec::new();
    std::io::Read::read_to_end(&mut stream.reader, &mut buf).expect("read stream");
    assert_eq!(buf, payload);

    if !git_supports_alternate_past_corrupt_first() {
        eprintln!(
            "SKIP: system git does not read past a corrupt first alternate; grit fallback verified above"
        );
        return;
    }

    let git_dir = layout.path().join("primary.git");
    let cat = Command::new("git")
        .args(["cat-file", "blob", &oid.to_hex()])
        .env("GIT_DIR", &git_dir)
        .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .expect("git cat-file");
    assert!(
        cat.status.success(),
        "git must read via later alternate when earlier is unusable, stderr={}",
        String::from_utf8_lossy(&cat.stderr)
    );
    assert_eq!(cat.stdout, payload);
}
