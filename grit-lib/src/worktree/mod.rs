//! Working tree, ignore/attributes, and checkout path helpers.
mod core;

pub use core::*;

pub mod attributes {
    //! attributes API (re-exported at this path).
    pub use crate::attributes::*;
}
pub mod crlf {
    //! crlf API (re-exported at this path).
    pub use crate::crlf::*;
}
pub mod ignore {
    //! ignore API (re-exported at this path).
    pub use crate::ignore::*;
}
pub mod worktree_cwd {
    //! worktree_cwd API (re-exported at this path).
    pub use crate::worktree_cwd::*;
}
pub mod worktree_ref {
    //! worktree_ref API (re-exported at this path).
    pub use crate::worktree_ref::*;
}
pub mod worktree_rules {
    //! worktree_rules API (re-exported at this path).
    pub use crate::worktree_rules::*;
}
pub mod pathspec {
    //! pathspec API (re-exported at this path).
    pub use crate::pathspec::*;
}
pub mod wildmatch {
    //! wildmatch API (re-exported at this path).
    pub use crate::wildmatch::*;
}
