//! Bundle files and bundle-over-remote transport.
mod core;

pub use core::*;

pub mod bundle_remote {
    //! Canonical re-export of [`crate::bundle_remote`].
    pub use crate::bundle_remote::*;
}
