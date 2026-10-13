//! Tree and content diffs, rename detection, merge diffs, and related helpers.
mod core;
pub mod handle;

pub use core::*;
pub use handle::Diff;

pub mod bloom {
    //! bloom API (re-exported at this path).
    pub use crate::bloom::*;
}
pub mod combined_diff_patch {
    //! combined_diff_patch API (re-exported at this path).
    pub use crate::combined_diff_patch::*;
}
pub mod combined_tree_diff {
    //! combined_tree_diff API (re-exported at this path).
    pub use crate::combined_tree_diff::*;
}
pub mod diffstat {
    //! diffstat API (re-exported at this path).
    pub use crate::diffstat::*;
}
pub mod patch_ids {
    //! patch_ids API (re-exported at this path).
    pub use crate::patch_ids::*;
}
pub mod userdiff {
    //! userdiff API (re-exported at this path).
    pub use crate::userdiff::*;
}
pub mod merge_diff {
    //! merge_diff API (re-exported at this path).
    pub use crate::merge_diff::*;
}
pub mod merge_file {
    //! merge_file API (re-exported at this path).
    pub use crate::merge_file::*;
}
pub mod merge_trees {
    //! merge_trees API (re-exported at this path).
    pub use crate::merge_trees::*;
}
pub mod rerere {
    //! rerere API (re-exported at this path).
    pub use crate::rerere::*;
}
pub mod fmt_merge_msg {
    //! fmt_merge_msg API (re-exported at this path).
    pub use crate::fmt_merge_msg::*;
}
pub mod ws {
    //! Whitespace helpers for diff and apply.
    pub use crate::ws::*;
}
