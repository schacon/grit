//! Storage traits for Git object access.
//!
//! These traits describe the object database operations needed by portable
//! library code without requiring a filesystem-backed [`crate::odb::Odb`].

use sha1::{Digest, Sha1};

use crate::error::{Error, Result};
use crate::objects::{Object, ObjectId, ObjectKind};

/// Identifier for a stored pack.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PackId(String);

impl PackId {
    /// Construct a pack identifier from a display string.
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Return the pack identifier as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Summary metadata for a stored pack.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackDescriptor {
    /// Stable pack identifier inside this store.
    pub id: PackId,
    /// Raw pack byte length.
    pub byte_len: usize,
    /// Whether this pack came from a promisor remote.
    pub promisor: bool,
}

/// A stored Git reference target.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RefTarget {
    /// Direct reference target, such as a branch pointing at a commit.
    Direct(ObjectId),
    /// Symbolic reference target, such as `HEAD` pointing at `refs/heads/main`.
    Symbolic(String),
}

/// Compute the Git object ID for an object kind and raw object body.
///
/// The hash is over the canonical object storage bytes:
/// `"<kind> <size>\0<body>"`.
#[must_use]
pub fn object_id_for(kind: ObjectKind, data: &[u8]) -> ObjectId {
    let header = format!("{} {}\0", kind.as_str(), data.len());
    let mut hasher = Sha1::new();
    hasher.update(header.as_bytes());
    hasher.update(data);
    let digest = hasher.finalize();
    let bytes: [u8; 20] = digest.into();
    ObjectId::from_bytes(&bytes).unwrap_or_else(|_| ObjectId::zero())
}

/// Read-only object access for a Git object store.
pub trait ObjectReader {
    /// Read the object named by `oid`.
    ///
    /// Returns an error when the object is missing or corrupt.
    fn read_object(&self, oid: &ObjectId) -> Result<Object>;

    /// Return whether the object named by `oid` is available.
    ///
    /// Implementations may answer from indexes or metadata without reading the
    /// full object body.
    fn has_object(&self, oid: &ObjectId) -> Result<bool> {
        match self.read_object(oid) {
            Ok(_) => Ok(true),
            Err(Error::ObjectNotFound(_)) => Ok(false),
            Err(err) => Err(err),
        }
    }
}

/// Mutable object access for a Git object store.
pub trait ObjectWriter: ObjectReader {
    /// Write an object body under `kind` and return its object ID.
    ///
    /// Implementations must hash the canonical Git object storage bytes, not
    /// just the raw body.
    fn write_object(&mut self, kind: ObjectKind, data: &[u8]) -> Result<ObjectId>;
}

/// Full object store access.
pub trait ObjectStore: ObjectReader + ObjectWriter {}

impl<T> ObjectStore for T where T: ObjectReader + ObjectWriter {}

/// Read-only access to a Git reference store.
pub trait RefReader {
    /// Resolve `name` to an object ID.
    ///
    /// Symbolic refs are followed to their direct target. Missing refs return
    /// `Ok(None)`.
    fn resolve_ref(&self, name: &str) -> Result<Option<ObjectId>>;

    /// Read the symbolic target of `name`.
    ///
    /// Direct or missing refs return `Ok(None)`.
    fn read_symbolic_ref(&self, name: &str) -> Result<Option<String>>;

    /// List direct object IDs for refs matching `prefix`.
    ///
    /// Symbolic refs may be included when they resolve to an object ID.
    fn list_refs(&self, prefix: &str) -> Result<Vec<(String, ObjectId)>>;
}

/// Mutable access to a Git reference store.
pub trait RefWriter: RefReader {
    /// Set `name` to the direct object ID `oid`.
    fn set_ref(&mut self, name: &str, oid: ObjectId) -> Result<()>;

    /// Set `name` to the symbolic reference target `target`.
    fn set_symbolic_ref(&mut self, name: &str, target: &str) -> Result<()>;

    /// Delete `name` from the reference store.
    fn delete_ref(&mut self, name: &str) -> Result<()>;
}

/// Full reference store access.
pub trait RefStore: RefReader + RefWriter {}

impl<T> RefStore for T where T: RefReader + RefWriter {}

/// Read-only access to raw pack storage.
pub trait PackReader {
    /// List packs known to this store.
    fn list_packs(&self) -> Result<Vec<PackDescriptor>>;

    /// Read raw pack bytes by `id`.
    ///
    /// Missing packs return `Ok(None)`.
    fn read_pack(&self, id: &PackId) -> Result<Option<Vec<u8>>>;
}

/// Mutable access to raw pack storage.
pub trait PackWriter: PackReader {
    /// Store raw pack bytes and return the assigned pack ID.
    ///
    /// The `promisor` flag records whether missing objects reachable from this
    /// pack may be fetched later from the remote.
    fn add_pack(&mut self, bytes: Vec<u8>, promisor: bool) -> Result<PackId>;

    /// Delete a stored pack by `id`.
    fn delete_pack(&mut self, id: &PackId) -> Result<()>;
}

/// Full raw pack store access.
pub trait PackStore: PackReader + PackWriter {}

impl<T> PackStore for T where T: PackReader + PackWriter {}

/// Compute a stable identifier for raw pack bytes.
///
/// This is storage metadata, not a Git object ID. The identifier is the SHA-1
/// of the complete pack byte stream.
#[must_use]
pub fn pack_id_for(bytes: &[u8]) -> PackId {
    let mut hasher = Sha1::new();
    hasher.update(bytes);
    PackId(hex::encode(hasher.finalize()))
}

#[cfg(not(target_arch = "wasm32"))]
impl ObjectReader for crate::odb::Odb {
    fn read_object(&self, oid: &ObjectId) -> Result<Object> {
        self.read(oid)
    }

    fn has_object(&self, oid: &ObjectId) -> Result<bool> {
        Ok(self.exists(oid))
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl ObjectWriter for crate::odb::Odb {
    fn write_object(&mut self, kind: ObjectKind, data: &[u8]) -> Result<ObjectId> {
        self.write(kind, data)
    }
}

/// Native file-backed reference store adapter.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Debug)]
pub struct FileRefStore {
    git_dir: std::path::PathBuf,
}

#[cfg(not(target_arch = "wasm32"))]
impl FileRefStore {
    /// Create a reference store rooted at `git_dir`.
    ///
    /// The path should point at the repository's `.git` directory, or at a bare
    /// repository directory.
    #[must_use]
    pub fn new(git_dir: impl Into<std::path::PathBuf>) -> Self {
        Self {
            git_dir: git_dir.into(),
        }
    }

    /// Return the git directory used by this store.
    #[must_use]
    pub fn git_dir(&self) -> &std::path::Path {
        &self.git_dir
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl RefReader for FileRefStore {
    fn resolve_ref(&self, name: &str) -> Result<Option<ObjectId>> {
        match crate::refs::read_raw_ref(&self.git_dir, name)? {
            crate::refs::RawRefLookup::NotFound | crate::refs::RawRefLookup::IsDirectory => {
                Ok(None)
            }
            crate::refs::RawRefLookup::Exists => {
                crate::refs::resolve_ref(&self.git_dir, name).map(Some)
            }
        }
    }

    fn read_symbolic_ref(&self, name: &str) -> Result<Option<String>> {
        crate::refs::read_symbolic_ref(&self.git_dir, name)
    }

    fn list_refs(&self, prefix: &str) -> Result<Vec<(String, ObjectId)>> {
        crate::refs::list_refs(&self.git_dir, prefix)
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl RefWriter for FileRefStore {
    fn set_ref(&mut self, name: &str, oid: ObjectId) -> Result<()> {
        crate::refs::write_ref(&self.git_dir, name, &oid)
    }

    fn set_symbolic_ref(&mut self, name: &str, target: &str) -> Result<()> {
        crate::refs::write_symbolic_ref(&self.git_dir, name, target)
    }

    fn delete_ref(&mut self, name: &str) -> Result<()> {
        crate::refs::delete_ref(&self.git_dir, name)
    }
}
