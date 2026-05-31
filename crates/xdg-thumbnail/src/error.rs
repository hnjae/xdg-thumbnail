// SPDX-FileCopyrightText: 2026 KIM Hyunjae
// SPDX-License-Identifier: MPL-2.0

use std::path::PathBuf;

use thiserror::Error;

/// Problem found in an explicitly supplied cache root path.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum CacheRootProblem {
    /// The supplied cache root is not absolute.
    NotAbsolute,
}

/// Problem found in an explicitly supplied cache entry path.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum CachePathProblem {
    /// The supplied cache entry path has no parent directory.
    MissingParentDirectory,
}

/// Privacy or type problem found in an existing personal-cache directory.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum CacheDirectoryProblem {
    /// The directory path is a symbolic link.
    Symlink,
    /// The path exists but is not a directory.
    NotDirectory,
    /// The directory is not owned by the current user.
    WrongOwner,
    /// The directory grants group or other access.
    GroupOrOtherAccessible,
}

/// Errors returned by thumbnail cache identity and filesystem operations.
///
/// Reason strings carried by variants are diagnostic text for humans and logs. They are not stable
/// machine-matchable API values; callers should use structured variants and typed validation
/// problems for control flow.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ThumbnailError {
    /// A URI or filename input is not valid for the requested thumbnail context.
    #[error("invalid thumbnail URI identity: {reason}")]
    #[non_exhaustive]
    InvalidUriIdentity {
        /// Diagnostic reason.
        reason: &'static str,
    },
    /// A cache namespace is not valid for filesystem use.
    #[error("invalid cache namespace: {reason}")]
    #[non_exhaustive]
    InvalidNamespace {
        /// Diagnostic reason.
        reason: &'static str,
    },
    /// The cache root could not be resolved from XDG environment variables.
    #[error("cache root could not be resolved: {reason}")]
    #[non_exhaustive]
    CacheRootUnavailable {
        /// Diagnostic reason.
        reason: &'static str,
    },
    /// An explicitly supplied cache root path is invalid.
    #[error("invalid cache root {path:?}: {problem:?}")]
    #[non_exhaustive]
    InvalidCacheRoot {
        /// Invalid cache root path.
        path: PathBuf,
        /// Typed root problem.
        problem: CacheRootProblem,
    },
    /// An explicitly computed cache entry path is invalid.
    #[error("invalid cache path {path:?}: {problem:?}")]
    #[non_exhaustive]
    InvalidCachePath {
        /// Invalid cache entry path.
        path: PathBuf,
        /// Typed path problem.
        problem: CachePathProblem,
    },
    /// An existing cache directory violates thumbnail cache privacy requirements.
    #[error("insecure cache directory {path:?}: {problem:?}")]
    #[non_exhaustive]
    InsecureCacheDirectory {
        /// Existing directory path.
        path: PathBuf,
        /// Typed directory problem.
        problem: CacheDirectoryProblem,
    },
    /// Filesystem I/O failed.
    #[error("{context}: {source}")]
    #[non_exhaustive]
    Io {
        /// Operation that failed.
        context: &'static str,
        /// Path involved in the operation, when available.
        path: Option<PathBuf>,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// PNG data could not be decoded or encoded.
    #[error("png error: {message}")]
    #[non_exhaustive]
    Png {
        /// PNG diagnostic message.
        message: String,
    },
    /// Thumbnail metadata is invalid.
    #[error("invalid thumbnail metadata: {reason}")]
    #[non_exhaustive]
    InvalidMetadata {
        /// Diagnostic reason.
        reason: &'static str,
    },
    /// Rendered thumbnail bytes are unsupported.
    #[error("unsupported rendered thumbnail: {reason}")]
    #[non_exhaustive]
    UnsupportedRenderedThumbnail {
        /// Diagnostic reason.
        reason: &'static str,
    },
    /// PNG parsing or thumbnail normalization would exceed configured resource limits.
    #[error("resource limit exceeded: {reason}")]
    #[non_exhaustive]
    ResourceLimitExceeded {
        /// Diagnostic reason.
        reason: &'static str,
    },
    /// Cache entry removal was refused by safety checks.
    #[error("refused to remove cache entry: {reason}")]
    #[non_exhaustive]
    UnsafeRemoval {
        /// Diagnostic reason.
        reason: &'static str,
    },
}

impl ThumbnailError {
    pub(crate) const fn invalid_uri(reason: &'static str) -> Self {
        Self::InvalidUriIdentity { reason }
    }

    pub(crate) const fn invalid_namespace(reason: &'static str) -> Self {
        Self::InvalidNamespace { reason }
    }

    pub(crate) const fn cache_root_unavailable(reason: &'static str) -> Self {
        Self::CacheRootUnavailable { reason }
    }

    pub(crate) const fn invalid_metadata(reason: &'static str) -> Self {
        Self::InvalidMetadata { reason }
    }

    pub(crate) const fn unsupported_rendered_thumbnail(reason: &'static str) -> Self {
        Self::UnsupportedRenderedThumbnail { reason }
    }

    pub(crate) const fn resource_limit_exceeded(reason: &'static str) -> Self {
        Self::ResourceLimitExceeded { reason }
    }

    pub(crate) const fn unsafe_removal(reason: &'static str) -> Self {
        Self::UnsafeRemoval { reason }
    }

    pub(crate) fn png(message: impl Into<String>) -> Self {
        Self::Png {
            message: message.into(),
        }
    }

    pub(crate) fn io(
        context: &'static str,
        path: impl Into<Option<PathBuf>>,
        source: std::io::Error,
    ) -> Self {
        Self::Io {
            context,
            path: path.into(),
            source,
        }
    }
}

/// Result type used by this crate.
pub type Result<T, E = ThumbnailError> = std::result::Result<T, E>;
