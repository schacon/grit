//! Bundle files and bundle-over-remote transport.
mod core;

pub use core::*;

pub mod bundle_remote {
    //! bundle_remote API (re-exported at this path).
    pub use crate::bundle_remote::*;
}
