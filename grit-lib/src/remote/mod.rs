//! Remotes, fetch/push, transport, and credential helpers.
mod core;

pub use core::*;

pub mod fetch {
    //! Canonical re-export of [`crate::fetch`].
    pub use crate::fetch::*;
}
pub mod push {
    //! Canonical re-export of [`crate::push`].
    pub use crate::push::*;
}
pub mod credentials {
    //! Canonical re-export of [`crate::credentials`].
    pub use crate::credentials::*;
}
pub mod url_rewrite {
    //! Canonical re-export of [`crate::url_rewrite`].
    pub use crate::url_rewrite::*;
}
pub mod transfer {
    //! Canonical re-export of [`crate::transfer`].
    pub use crate::transfer::*;
}
pub mod transport {
    //! Canonical re-export of [`crate::transport`].
    pub use crate::transport::*;
}
pub mod transport_path {
    //! Canonical re-export of [`crate::transport_path`].
    pub use crate::transport_path::*;
}
pub mod protocol {
    //! Canonical re-export of [`crate::protocol`].
    pub use crate::protocol::*;
}
pub mod protocol_v2 {
    //! Canonical re-export of [`crate::protocol_v2`].
    pub use crate::protocol_v2::*;
}
pub mod shallow {
    //! Canonical re-export of [`crate::shallow`].
    pub use crate::shallow::*;
}
pub mod fetch_head {
    //! Canonical re-export of [`crate::fetch_head`].
    pub use crate::fetch_head::*;
}
pub mod fetch_negotiator {
    //! Canonical re-export of [`crate::fetch_negotiator`].
    pub use crate::fetch_negotiator::*;
}
pub mod receive_pack {
    //! Canonical re-export of [`crate::receive_pack`].
    pub use crate::receive_pack::*;
}
pub mod push_cert {
    //! Canonical re-export of [`crate::push_cert`].
    pub use crate::push_cert::*;
}
pub mod push_report {
    //! Canonical re-export of [`crate::push_report`].
    pub use crate::push_report::*;
}
pub mod clone {
    //! Canonical re-export of [`crate::clone`].
    pub use crate::clone::*;
}
pub mod serve {
    //! Canonical re-export of [`crate::serve`].
    pub use crate::serve::receive_pack;
    pub use crate::serve::upload_pack;
    pub use crate::serve::*;
}
