//! Gust library — core Git-compatible engine.
//!
//! # Architecture
//!
//! All Git-compatible logic lives here; the `grit` binary is a thin CLI shim
//! that parses arguments and delegates to types exposed from this crate.
//!
//! ## Modules
//!
//! - [`error`] — shared error types using `thiserror`
//! - [`objects`] — object ID, object kinds, and in-memory representations
//! - [`odb`] — loose object store (read/write zlib-compressed objects)
//! - [`repo`] — repository discovery and handle
//! - [`index`] — Git index (staging area) read/write
//! - [`ignore`] — ignore/exclude pattern matching for check-ignore
//! - [`refs`] — reference storage (files backend)

#[cfg(target_arch = "wasm32")]
pub mod commit;
#[cfg(target_arch = "wasm32")]
pub mod commit_encoding;
#[cfg(target_arch = "wasm32")]
pub mod error;
#[cfg(target_arch = "wasm32")]
pub mod objects;
#[cfg(target_arch = "wasm32")]
pub mod pack_write;
#[cfg(target_arch = "wasm32")]
pub mod pkt_line;
#[cfg(target_arch = "wasm32")]
pub mod smart_protocol;
#[cfg(target_arch = "wasm32")]
pub mod storage;
#[cfg(target_arch = "wasm32")]
pub mod unpack_objects;

#[cfg(not(target_arch = "wasm32"))]
pub mod attributes;
#[cfg(not(target_arch = "wasm32"))]
pub mod bloom;
#[cfg(not(target_arch = "wasm32"))]
pub mod check_ref_format;
#[cfg(not(target_arch = "wasm32"))]
pub mod combined_diff_patch;
#[cfg(not(target_arch = "wasm32"))]
pub mod combined_tree_diff;
#[cfg(not(target_arch = "wasm32"))]
pub mod commit;
#[cfg(not(target_arch = "wasm32"))]
pub mod commit_encoding;
#[cfg(not(target_arch = "wasm32"))]
pub mod commit_graph_file;
#[cfg(not(target_arch = "wasm32"))]
pub mod commit_graph_write;
#[cfg(not(target_arch = "wasm32"))]
pub mod commit_pretty;
#[cfg(not(target_arch = "wasm32"))]
pub mod commit_trailers;
#[cfg(not(target_arch = "wasm32"))]
pub mod config;
#[cfg(not(target_arch = "wasm32"))]
pub mod connectivity;
#[cfg(not(target_arch = "wasm32"))]
pub mod crlf;
#[cfg(not(target_arch = "wasm32"))]
pub mod delta_encode;
#[cfg(not(target_arch = "wasm32"))]
pub mod diff;
#[cfg(not(target_arch = "wasm32"))]
mod diff_indent_heuristic;
#[cfg(not(target_arch = "wasm32"))]
pub mod diffstat;
#[cfg(not(target_arch = "wasm32"))]
pub mod error;
#[cfg(not(target_arch = "wasm32"))]
mod ewah_bitmap;
#[cfg(not(target_arch = "wasm32"))]
pub mod fast_export;
#[cfg(not(target_arch = "wasm32"))]
pub mod fast_import;
#[cfg(not(target_arch = "wasm32"))]
pub mod fetch_head;
#[cfg(not(target_arch = "wasm32"))]
pub mod fetch_negotiator;
#[cfg(not(target_arch = "wasm32"))]
pub mod fetch_submodules;
#[cfg(not(target_arch = "wasm32"))]
pub mod filter_process;
#[cfg(not(target_arch = "wasm32"))]
pub mod fmt_merge_msg;
#[cfg(not(target_arch = "wasm32"))]
pub mod fsck_standalone;
#[cfg(not(target_arch = "wasm32"))]
pub mod git_date;
#[cfg(not(target_arch = "wasm32"))]
pub mod gitmodules;
#[cfg(not(target_arch = "wasm32"))]
pub mod hide_refs;
#[cfg(not(target_arch = "wasm32"))]
pub mod hooks;
#[cfg(not(target_arch = "wasm32"))]
pub mod ident;
#[cfg(not(target_arch = "wasm32"))]
pub mod ident_config;
#[cfg(not(target_arch = "wasm32"))]
pub mod ignore;
#[cfg(not(target_arch = "wasm32"))]
pub mod index;
#[cfg(not(target_arch = "wasm32"))]
pub mod index_name_hash_lazy;
#[cfg(not(target_arch = "wasm32"))]
pub mod interpret_trailers;
#[cfg(not(target_arch = "wasm32"))]
pub mod line_log;
#[cfg(not(target_arch = "wasm32"))]
pub mod ls_remote;
#[cfg(not(target_arch = "wasm32"))]
pub mod mailinfo;
#[cfg(not(target_arch = "wasm32"))]
pub mod mailmap;
#[cfg(not(target_arch = "wasm32"))]
pub mod merge_base;
#[cfg(not(target_arch = "wasm32"))]
pub mod merge_diff;
#[cfg(not(target_arch = "wasm32"))]
pub mod merge_file;
#[cfg(not(target_arch = "wasm32"))]
pub mod merge_tree_trivial;
#[cfg(not(target_arch = "wasm32"))]
pub mod merge_trees;
#[cfg(not(target_arch = "wasm32"))]
pub mod mergetool_vimdiff;
#[cfg(not(target_arch = "wasm32"))]
pub mod midx;
#[cfg(not(target_arch = "wasm32"))]
pub mod name_rev;
#[cfg(not(target_arch = "wasm32"))]
pub mod objects;
#[cfg(not(target_arch = "wasm32"))]
pub mod odb;
#[cfg(not(target_arch = "wasm32"))]
pub mod pack;
#[cfg(not(target_arch = "wasm32"))]
pub mod pack_geometry;
#[cfg(not(target_arch = "wasm32"))]
pub mod pack_name_hash;
#[cfg(not(target_arch = "wasm32"))]
pub mod pack_rev;
#[cfg(not(target_arch = "wasm32"))]
pub mod pack_write;
#[cfg(not(target_arch = "wasm32"))]
pub mod parse_options_test_tool;
#[cfg(not(target_arch = "wasm32"))]
pub mod patch_ids;
#[cfg(not(target_arch = "wasm32"))]
pub mod path_walk;
#[cfg(not(target_arch = "wasm32"))]
pub mod pathspec;
#[cfg(not(target_arch = "wasm32"))]
pub mod pkt_line;
#[cfg(not(target_arch = "wasm32"))]
pub mod precompose_config;
#[cfg(not(target_arch = "wasm32"))]
pub mod promisor;
#[cfg(not(target_arch = "wasm32"))]
pub mod prune_packed;
#[cfg(not(target_arch = "wasm32"))]
pub mod push_submodules;
#[cfg(not(target_arch = "wasm32"))]
pub mod quote_path;
#[cfg(not(target_arch = "wasm32"))]
pub mod ref_exclusions;
#[cfg(not(target_arch = "wasm32"))]
pub mod ref_namespace;
#[cfg(not(target_arch = "wasm32"))]
pub mod reflog;
#[cfg(not(target_arch = "wasm32"))]
pub mod refs;
#[cfg(not(target_arch = "wasm32"))]
pub mod refs_fsck;
#[cfg(not(target_arch = "wasm32"))]
pub mod reftable;
#[cfg(not(target_arch = "wasm32"))]
pub mod repo;
#[cfg(not(target_arch = "wasm32"))]
pub mod rerere;
#[cfg(not(target_arch = "wasm32"))]
pub mod resolve_undo;
#[cfg(not(target_arch = "wasm32"))]
pub mod rev_list;
#[cfg(not(target_arch = "wasm32"))]
pub mod rev_parse;
#[cfg(not(target_arch = "wasm32"))]
pub mod shallow;
#[cfg(not(target_arch = "wasm32"))]
pub mod shared_repo;
#[cfg(all(unix, not(target_arch = "wasm32")))]
pub mod simple_ipc;
#[cfg(not(target_arch = "wasm32"))]
pub mod smart_protocol;
#[cfg(not(target_arch = "wasm32"))]
pub mod sparse_checkout;
#[cfg(not(target_arch = "wasm32"))]
pub mod split_index;
#[cfg(not(target_arch = "wasm32"))]
pub mod storage;
#[cfg(not(target_arch = "wasm32"))]
pub mod unicode_normalization;
#[cfg(not(target_arch = "wasm32"))]
pub mod untracked_cache;
#[cfg(all(not(unix), not(target_arch = "wasm32")))]
pub mod simple_ipc {
    /// Whether simple IPC is supported on this platform.
    #[must_use]
    pub fn supports_simple_ipc() -> bool {
        false
    }

    /// Stub for non-Unix targets.
    pub fn run_simple_ipc_tool(_args: &[String]) -> i32 {
        eprintln!("simple IPC not available on this platform");
        1
    }
}
#[cfg(not(target_arch = "wasm32"))]
pub mod state;
#[cfg(not(target_arch = "wasm32"))]
pub mod stripspace;
#[cfg(not(target_arch = "wasm32"))]
pub mod submodule_active;
#[cfg(not(target_arch = "wasm32"))]
pub mod submodule_config;
#[cfg(not(target_arch = "wasm32"))]
pub mod submodule_config_cache;
#[cfg(not(target_arch = "wasm32"))]
pub mod submodule_gitdir;
#[cfg(not(target_arch = "wasm32"))]
pub mod tab_expand;
#[cfg(not(target_arch = "wasm32"))]
pub mod test_tool_progress;
#[cfg(not(target_arch = "wasm32"))]
pub mod textconv_cache;
#[cfg(not(target_arch = "wasm32"))]
pub mod tree_path_follow;
#[cfg(all(unix, not(target_arch = "wasm32")))]
pub mod unix_process;
#[cfg(not(target_arch = "wasm32"))]
pub mod unpack_objects;
#[cfg(not(target_arch = "wasm32"))]
pub mod userdiff;
#[cfg(not(target_arch = "wasm32"))]
pub mod whitespace_rule;
#[cfg(not(target_arch = "wasm32"))]
pub mod wildmatch;
#[cfg(not(target_arch = "wasm32"))]
pub mod worktree_cwd;
#[cfg(not(target_arch = "wasm32"))]
pub mod write_tree;
#[cfg(not(target_arch = "wasm32"))]
pub mod ws;
