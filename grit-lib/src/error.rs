//! Shared error types for the Gust library.
//!
//! Library code uses [`Error`] (a `thiserror` enum) so callers can match on
//! specific failure modes. The binary wraps these with `anyhow` for human-
//! readable top-level reporting.

use thiserror::Error;

/// The top-level error type for all Gust library operations.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    /// A repository could not be found or is structurally invalid.
    #[error("not a git repository (or any of the parent directories): {0}")]
    NotARepository(String),

    /// A bare repository was found but access is forbidden by safe.bareRepository.
    #[error("cannot use bare repository '{0}' (safe.bareRepository is 'explicit')")]
    ForbiddenBareRepository(String),

    /// The repository is owned by a different user (safe.directory).
    #[error("detected dubious ownership in repository at '{0}'")]
    DubiousOwnership(String),

    /// Repository format version is not supported by this implementation.
    #[error("unsupported repository format version '{0}'")]
    UnsupportedRepositoryFormatVersion(u32),

    /// Repository declares an unsupported extension.
    #[error("unknown repository extension '{0}'")]
    UnsupportedRepositoryExtension(String),

    /// A supplied object ID string was not valid hex or the wrong length.
    #[error("invalid object id '{0}'")]
    InvalidObjectId(String),

    /// The requested object does not exist in the object store.
    #[error("object not found: {0}")]
    ObjectNotFound(String),

    /// An object's stored data is corrupt or malformed.
    #[error("corrupt object: {0}")]
    CorruptObject(String),

    /// An unsupported or unknown object type was encountered.
    #[error("unknown object type '{0}'")]
    UnknownObjectType(String),

    /// Loose object header type field exceeds Git's 32-byte limit.
    #[error("header for {oid} too long, exceeds 32 bytes")]
    ObjectHeaderTooLong { oid: String },

    /// An I/O error from the underlying filesystem.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// A zlib compression or decompression failure.
    #[error("zlib error: {0}")]
    Zlib(String),

    /// The index file is missing, truncated, or has a bad header.
    #[error("index error: {0}")]
    IndexError(String),

    /// A reference name or value is invalid.
    #[error("invalid ref: {0}")]
    InvalidRef(String),

    /// A general path-related error (invalid UTF-8, out-of-bounds, etc.).
    #[error("path error: {0}")]
    PathError(String),

    /// A configuration file parsing or access error.
    #[error("config error: {0}")]
    ConfigError(String),
}

/// Convenience alias for `Result<T, Error>`.
pub type Result<T> = std::result::Result<T, Error>;
