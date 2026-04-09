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

pub mod attributes;
pub mod bloom;
pub mod check_ref_format;
pub mod commit_encoding;
pub mod commit_graph_file;
pub mod commit_graph_write;
pub mod commit_pretty;
pub mod commit_trailers;
pub mod config;
pub mod connectivity;
pub mod crlf;
pub mod delta_encode;
pub mod diff;
pub mod diffstat;
pub mod error;
pub mod fast_export;
pub mod fast_import;
pub mod fetch_negotiator;
pub mod filter_process;
pub mod fmt_merge_msg;
pub mod fsck_standalone;
pub mod git_date;
pub mod gitmodules;
pub mod hooks;
pub mod ident;
pub mod ignore;
pub mod index;
pub mod index_name_hash_lazy;
pub mod interpret_trailers;
pub mod line_log;
pub mod ls_remote;
pub mod mailmap;
pub mod merge_base;
pub mod merge_diff;
pub mod merge_file;
pub mod merge_trees;
pub mod mergetool_vimdiff;
pub mod midx;
pub mod name_rev;
pub mod objects;
pub mod odb;
pub mod pack;
pub mod pack_geometry;
pub mod pack_name_hash;
pub mod parse_options_test_tool;
pub mod patch_ids;
pub mod pathspec;
pub mod promisor;
pub mod prune_packed;
pub mod quote_path;
pub mod reflog;
pub mod refs;
pub mod reftable;
pub mod repo;
pub mod rerere;
pub mod rev_list;
pub mod rev_parse;
#[cfg(unix)]
pub mod simple_ipc;
pub mod sparse_checkout;
#[cfg(not(unix))]
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
pub mod state;
pub mod stripspace;
pub mod submodule_gitdir;
pub mod textconv_cache;
pub mod tree_path_follow;
#[cfg(unix)]
pub mod unix_process;
pub mod unpack_objects;
pub mod userdiff;
pub mod whitespace_rule;
pub mod wildmatch;
pub mod write_tree;
pub mod ws;
