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

pub mod config;
pub mod diff;
pub mod error;
pub mod ignore;
pub mod index;
pub mod merge_base;
pub mod objects;
pub mod odb;
pub mod pack;
pub mod patch_ids;
pub mod refs;
pub mod repo;
pub mod rev_list;
pub mod rev_parse;
pub mod state;
pub mod write_tree;
