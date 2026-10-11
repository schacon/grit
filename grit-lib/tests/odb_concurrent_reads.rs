//! Concurrent [`Odb::read`] from worker threads (pack store context and byte identity).

use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Barrier};

use grit_lib::midx::{write_multi_pack_index_with_options, WriteMultiPackIndexOptions};
use grit_lib::objects::{ObjectId, ObjectKind};
use grit_lib::odb::Odb;
use grit_lib::pack::{clear_pack_cache, read_pack_index};
use grit_lib::pack_store::test_hooks;
use grit_lib::repo::init_repository;
use grit_lib::transfer::{build_pack, PackBuildOptions};
use grit_lib::RefStorageFormat;
use tempfile::TempDir;

fn assert_send_sync<T: Send + Sync>() {}

#[test]
fn odb_repository_and_pack_store_are_send_sync() {
    assert_send_sync::<Odb>();
    assert_send_sync::<grit_lib::pack_store::PackStore>();
    assert_send_sync::<grit_lib::repo::Repository>();
}

fn install_pack(objects_dir: &Path, stem: &str, pack: &[u8]) -> Vec<ObjectId> {
    let scratch = tempfile::tempdir().expect("index-pack scratch");
    let pack_path = scratch.path().join(format!("{stem}.pack"));
    fs::write(&pack_path, pack).expect("write pack");
    let out = std::process::Command::new("git")
        .current_dir(scratch.path())
        .args(["index-pack", &format!("{stem}.pack")])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .expect("spawn index-pack");
    assert!(
        out.status.success(),
        "index-pack: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let dest_dir = objects_dir.join("pack");
    fs::create_dir_all(&dest_dir).expect("pack dir");
    fs::copy(
        scratch.path().join(format!("{stem}.pack")),
        dest_dir.join(format!("{stem}.pack")),
    )
    .expect("copy pack");
    fs::copy(
        scratch.path().join(format!("{stem}.idx")),
        dest_dir.join(format!("{stem}.idx")),
    )
    .expect("copy idx");
    clear_pack_cache();
    let idx = read_pack_index(&dest_dir.join(format!("{stem}.idx"))).expect("read idx");
    idx.iter()
        .filter_map(|e| ObjectId::from_bytes(e.oid()).ok())
        .collect()
}

#[test]
fn odb_concurrent_reads() {
    clear_pack_cache();
    test_hooks::reset();

    let root = TempDir::new().expect("tempdir");
    let repo = init_repository(root.path(), false, "main", None, RefStorageFormat::Files)
        .expect("init repo");
    let primary_objects = repo.git_dir.join("objects");

    let mut packed_blob_oids = Vec::new();
    for batch in 0..4 {
        let mut batch_oids = Vec::new();
        for i in 0..16 {
            let n = batch * 16 + i;
            let oid = repo
                .odb
                .write(ObjectKind::Blob, format!("packed blob {n}\n").as_bytes())
                .expect("write blob for pack");
            batch_oids.push(oid);
        }
        let pack = build_pack(
            &repo.odb,
            &batch_oids,
            &[],
            &PackBuildOptions {
                delta: false,
                max_depth: 32,
                use_ofs_delta: true,
                ..PackBuildOptions::default()
            },
        )
        .expect("build pack");
        install_pack(&primary_objects, &format!("p{batch}"), &pack);
        packed_blob_oids.extend(batch_oids);
    }

    let loose_oid = repo
        .odb
        .write(ObjectKind::Blob, b"loose-only concurrent read\n")
        .expect("loose write");

    let alt_root = TempDir::new().expect("alt tempdir");
    let alt_objects = alt_root.path().join("objects");
    fs::create_dir_all(&alt_objects).expect("alt objects");
    let alt_odb = Odb::new(&alt_objects);
    let alt_blob = alt_odb
        .write(ObjectKind::Blob, b"from-alternate-store\n")
        .expect("alt blob");
    let alt_pack =
        build_pack(&alt_odb, &[alt_blob], &[], &PackBuildOptions::default()).expect("alt pack");
    install_pack(&alt_objects, "alt", &alt_pack);

    fs::create_dir_all(primary_objects.join("info")).expect("info");
    fs::write(
        primary_objects.join("info/alternates"),
        format!("{}\n", alt_objects.display()),
    )
    .expect("alternates");

    let pack_dir = primary_objects.join("pack");
    write_multi_pack_index_with_options(&pack_dir, &WriteMultiPackIndexOptions::default())
        .expect("write midx");
    let probe = packed_blob_oids[0];
    grit_lib::midx::try_read_object_via_midx(&primary_objects, &probe)
        .expect("midx read probe")
        .expect("midx must list probe oid");

    let odb = Odb::with_work_tree(&primary_objects, root.path())
        .with_config_git_dir(repo.git_dir.clone());

    let mut oids = packed_blob_oids;
    oids.push(loose_oid);
    oids.push(alt_blob);
    assert!(oids.len() >= 10, "need a mixed object sample");

    let serial: Vec<Vec<u8>> = oids
        .iter()
        .map(|oid| {
            odb.read(oid)
                .unwrap_or_else(|e| panic!("serial read {}: {e}", oid.to_hex()))
                .data
        })
        .collect();

    let stop = Arc::new(AtomicBool::new(false));
    let odb_inv = odb.clone();
    let stop_inv = Arc::clone(&stop);
    let invalidator = std::thread::spawn(move || {
        while !stop_inv.load(Ordering::Acquire) {
            odb_inv.invalidate_packs();
            std::thread::yield_now();
        }
    });

    let threads = 8;
    let barrier = Arc::new(Barrier::new(threads));
    let mut handles = Vec::new();
    for _ in 0..threads {
        let b = Arc::clone(&barrier);
        let odb_t = odb.clone();
        let sample = oids.clone();
        let expect = serial.clone();
        handles.push(std::thread::spawn(move || {
            b.wait();
            for (oid, want) in sample.iter().zip(expect.iter()) {
                let got = odb_t.read(oid).expect("parallel read").data;
                assert_eq!(&got, want, "byte mismatch for {}", oid.to_hex());
            }
        }));
    }
    for h in handles {
        h.join().expect("reader thread");
    }

    stop.store(true, Ordering::Release);
    invalidator.join().expect("invalidator");

    assert_eq!(
        test_hooks::legacy_stores_created(),
        0,
        "Odb reads must not create legacy thread-local pack stores"
    );
    assert_eq!(
        test_hooks::implicit_stores_created(),
        0,
        "Odb reads must not create implicit pack stores"
    );

    std::thread::scope(|scope| {
        for _ in 0..8 {
            let odb_s = odb.clone();
            let oid = oids[0];
            scope.spawn(move || {
                odb_s.read(&oid).expect("scoped read");
            });
        }
    });

    assert_eq!(test_hooks::legacy_stores_created(), 0);
    assert_eq!(test_hooks::implicit_stores_created(), 0);
}
