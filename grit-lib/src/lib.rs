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
pub mod check_ref_format;
pub mod config;
pub mod crlf;
pub mod delta_encode;
pub mod diff;
pub mod error;
pub mod fmt_merge_msg;
pub mod fsck_standalone;
pub mod git_date;
pub mod hooks;
pub mod ignore;
pub mod index;
pub mod ls_remote;
pub mod merge_base;
pub mod merge_diff;
pub mod merge_file;
pub mod midx;
pub mod name_rev;
pub mod objects;
pub mod odb;
pub mod pack;
pub mod parse_options_test_tool;
pub mod patch_ids;
pub mod prune_packed;
pub mod reflog;
pub mod refs;
pub mod reftable;
pub mod repo;
pub mod rev_list;
pub mod rev_parse;
pub mod state;
pub mod stripspace;
pub mod tree_path_follow;
#[cfg(unix)]
pub mod unix_process;
pub mod unpack_objects;
pub mod userdiff;
pub mod wildmatch;
pub mod write_tree;
pub mod ws;
