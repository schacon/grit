//! Git objects: ids, kinds, in-memory representations, and the object database stack.
//!
//! Canonical layout groups loose/packed storage, hashing, deltas, and reachability
//! bitmaps under this module. Flat [`crate::odb`] paths remain valid.

mod core;
pub mod handle;

pub use core::*;
pub use handle::Objects;

pub mod odb {
    pub use crate::odb::builder;
    pub use crate::odb::store;
    pub use crate::odb::*;
}

pub mod pack {
    pub use crate::pack::*;
}

pub mod midx {
    pub use crate::midx::*;
}

pub mod hash {
    pub use crate::hash::*;
}

pub mod commit_graph {
    pub use crate::commit_graph_file::*;
    pub use crate::commit_graph_write::*;
}

pub mod bitmap {
    pub use crate::bitmap_walk::*;
    pub use crate::pack_bitmap::{
        BitmapError, BitmapIndex, BitmapIndexCache, CommitReachabilityBitmap, MidxBitmapWriter,
        PackBitmapWriteError, PackBitmapWriteOptions, PackBitmapWriter, TypeBitmap,
    };
}

pub mod delta_encode {
    pub use crate::delta_encode::*;
}
pub mod delta_islands {
    pub use crate::delta_islands::*;
}
pub mod index_pack {
    pub use crate::index_pack::*;
}
pub mod pack_name_hash {
    pub use crate::pack_name_hash::*;
}
pub mod pack_rev {
    pub use crate::pack_rev::*;
}
pub mod pack_store {
    pub use crate::pack_store::*;
}
pub mod unpack_objects {
    pub use crate::unpack_objects::*;
}
pub mod prune_packed {
    pub use crate::prune_packed::*;
}
pub mod promisor {
    //! Canonical re-export of [`crate::promisor`].
    pub use crate::promisor::*;
}
pub mod promisor_remote {
    pub use crate::promisor_remote::*;
}
pub mod pack_objects {
    pub use crate::pack_objects::*;
}
pub mod pack_object_select {
    pub use crate::pack_object_select::*;
}
pub mod pack_receive {
    pub use crate::pack_receive::*;
}
