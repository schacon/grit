//! Command implementations for the `grit` binary.
//!
//! Each submodule corresponds to one plumbing subcommand.

pub mod add;
pub mod branch;
pub mod cat_file;
pub mod check_ignore;
pub mod checkout_index;
pub mod commit;
pub mod commit_tree;
pub mod config;
pub mod count_objects;
pub mod diff_index;
pub mod for_each_ref;
pub mod gc;
pub mod git_passthrough;
pub mod hash_object;
pub mod init;
pub mod log;
pub mod ls_files;
pub mod ls_tree;
pub mod merge_base;
pub mod read_tree;
pub mod repack;
pub mod restore;
pub mod rev_list;
pub mod rev_parse;
pub mod show;
pub mod show_ref;
pub mod status;
pub mod symbolic_ref;
pub mod tag;
pub mod update_index;
pub mod update_ref;
pub mod verify_pack;
pub mod write_tree;
