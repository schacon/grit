//! Compile-time checks that canonical domain paths resolve for workspace consumers.

use grit_lib::{
    config::ConfigSet,
    diff::DiffEntry,
    index::Index,
    objects::odb::store::MemoryStore,
    objects::{bitmap::BitmapIndex, hash::ObjectHasher, odb::Odb},
    parallel::ThreadPolicy,
    refs::{check_ref_format::RefNameOptions, reflog::ReflogEntry},
    remote::credentials::Credential,
    repo::Repository,
    revwalk::rev_list::RevListOptions,
    stash::StashEntry,
    worktree::ignore::IgnoreMatcher,
};

#[test]
fn canonical_paths_cover_external_uses() {
    let _ = std::any::type_name::<Repository>();
    let _ = std::any::type_name::<Odb>();
    let _ = std::any::type_name::<Index>();
    let _ = std::any::type_name::<DiffEntry>();
    let _ = std::any::type_name::<ConfigSet>();
    let _ = std::any::type_name::<BitmapIndex>();
    let _ = std::any::type_name::<ObjectHasher>();
    let _ = std::any::type_name::<ThreadPolicy>();
    let _ = std::any::type_name::<ReflogEntry>();
    let _ = std::any::type_name::<RefNameOptions>();
    let _ = std::any::type_name::<MemoryStore>();
    let _ = std::any::type_name::<Credential>();
    let _ = std::any::type_name::<RevListOptions>();
    let _ = std::any::type_name::<StashEntry>();
    let _ = std::any::type_name::<IgnoreMatcher>();

    let map = include_str!("../canonical-modules.toml");
    assert!(
        map.contains("[map]"),
        "canonical-modules.toml must define [map]"
    );
}

#[test]
fn canonical_layout_script_covers_workspace_imports() {
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root");
    let status = std::process::Command::new("python3")
        .arg("scripts/check_canonical_layout.py")
        .current_dir(workspace)
        .status()
        .expect("run check_canonical_layout.py");
    assert!(status.success(), "canonical layout map is incomplete");
}
