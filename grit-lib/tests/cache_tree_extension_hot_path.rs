//! Cache-tree extension validity on large synthetic indexes.

use grit_lib::index::write_tree::cache_tree_extension_valid;
use grit_lib::index::Index;

#[test]
fn flat_100k_git_index_cache_tree_extension_valid() {
    let repo = std::path::Path::new("/tmp/grit-bench-100k-spread");
    if !repo.join(".git/index").is_file() {
        eprintln!("SKIP: /tmp/grit-bench-100k-spread not present");
        return;
    }
    let index = Index::load(&repo.join(".git/index")).expect("load index");
    assert!(
        cache_tree_extension_valid(index.cache_tree.as_ref()),
        "git-written cache-tree on 100k fixture must be structurally valid"
    );
}
