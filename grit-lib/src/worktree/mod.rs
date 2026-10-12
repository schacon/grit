//! Working tree, ignore/attributes, and checkout path helpers.
mod core;

pub use core::*;

pub mod attributes {
    //! Canonical re-export of [`crate::attributes`].
    pub use crate::attributes::*;
}
pub mod crlf {
    //! Canonical re-export of [`crate::crlf`].
    pub use crate::crlf::*;
}
pub mod ignore {
    //! Canonical re-export of [`crate::ignore`].
    pub use crate::ignore::*;
}
pub mod worktree_cwd {
    //! Canonical re-export of [`crate::worktree_cwd`].
    pub use crate::worktree_cwd::*;
}
pub mod worktree_ref {
    //! Canonical re-export of [`crate::worktree_ref`].
    pub use crate::worktree_ref::*;
}
pub mod worktree_rules {
    //! Canonical re-export of [`crate::worktree_rules`].
    pub use crate::worktree_rules::*;
}
pub mod pathspec {
    //! Canonical re-export of [`crate::pathspec`].
    pub use crate::pathspec::*;
}
pub mod wildmatch {
    //! Canonical re-export of [`crate::wildmatch`].
    pub use crate::wildmatch::*;
}
