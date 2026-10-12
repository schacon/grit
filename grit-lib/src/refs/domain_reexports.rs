//! Canonical nested paths for ref-adjacent modules (flat [`crate::reflog`] paths remain valid).

pub mod reflog {
    //! Canonical re-export of [`crate::reflog`].
    pub use crate::reflog::*;
}
pub mod refspec {
    //! Canonical re-export of [`crate::refspec`].
    pub use crate::refspec::*;
}
pub mod reftable {
    //! Canonical re-export of [`crate::reftable`].
    pub use crate::reftable::*;
}
pub mod check_ref_format {
    //! Canonical re-export of [`crate::check_ref_format`].
    pub use crate::check_ref_format::*;
}
pub mod ref_namespace {
    //! Canonical re-export of [`crate::ref_namespace`].
    pub use crate::ref_namespace::*;
}
pub mod hide_refs {
    //! Canonical re-export of [`crate::hide_refs`].
    pub use crate::hide_refs::*;
}
pub mod ref_exclusions {
    //! Canonical re-export of [`crate::ref_exclusions`].
    pub use crate::ref_exclusions::*;
}
pub mod ref_storage {
    //! Canonical re-export of [`crate::ref_storage`].
    pub use crate::ref_storage::*;
}
pub mod branch_tracking {
    //! Canonical re-export of [`crate::branch_tracking`].
    pub use crate::branch_tracking::*;
}
pub mod refs_fsck {
    //! Canonical re-export of [`crate::refs_fsck`].
    pub use crate::refs_fsck::*;
}
