//! Revision walking: rev-parse, rev-list, merge-base, and commit-graph acceleration.
pub mod handle;

pub use handle::RevWalk;

pub mod rev_list {
    //! rev_list API (re-exported at this path).
    pub use crate::rev_list::*;
}
pub mod rev_parse {
    //! rev_parse API (re-exported at this path).
    pub use crate::rev_parse::*;
}
pub mod merge_base {
    //! merge_base API (re-exported at this path).
    pub use crate::merge_base::*;
}
pub mod log_walk {
    //! Canonical re-export of [`crate::log_walk`].
    pub use crate::log_walk::*;
}
pub mod commit_graph {
    //! Commit-graph read and write helpers.
    pub use crate::commit_graph_file::*;
    pub use crate::commit_graph_write::*;
}
pub mod rev_list_error {
    //! rev_list_error API (re-exported at this path).
    pub use crate::rev_list_error::*;
}
pub mod rev_parse_error {
    //! rev_parse_error API (re-exported at this path).
    pub use crate::rev_parse_error::*;
}
