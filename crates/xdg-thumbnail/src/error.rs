// SPDX-FileCopyrightText: 2026 KIM Hyunjae
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::path::PathBuf;

use thiserror::Error;

/// Errors returned by thumbnail cache identity and filesystem operations.
#[derive(Debug, Error)]
pub enum ThumbnailError {
    /// The requested operation depends on Unix path byte semantics.
    #[error("operation is unsupported on this platform")]
    UnsupportedPlatform,
    /// A URI or filename input is not valid for the requested thumbnail context.
    #[error("invalid thumbnail URI identity: {0}")]
    InvalidUriIdentity(&'static str),
    /// A cache namespace is not valid for filesystem use.
    #[error("invalid cache namespace: {0}")]
    InvalidNamespace(&'static str),
    /// The cache root could not be resolved from XDG environment variables.
    #[error("cache root could not be resolved: {0}")]
    CacheRootUnavailable(&'static str),
    /// An existing cache directory violates thumbnail cache privacy requirements.
    #[error("insecure cache directory: {0}")]
    InsecureCacheDirectory(PathBuf),
    /// Filesystem I/O failed.
    #[error("{context}: {source}")]
    Io {
        /// Operation that failed.
        context: &'static str,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// PNG data could not be decoded or encoded.
    #[error("png error: {0}")]
    Png(String),
    /// Thumbnail metadata is invalid.
    #[error("invalid thumbnail metadata: {0}")]
    InvalidMetadata(&'static str),
    /// Rendered thumbnail payload is unsupported.
    #[error("unsupported rendered thumbnail: {0}")]
    UnsupportedRenderedThumbnail(&'static str),
    /// Cache entry removal was refused by safety checks.
    #[error("refused to remove cache entry: {0}")]
    UnsafeRemoval(&'static str),
}

impl ThumbnailError {
    pub(crate) fn invalid_uri(reason: &'static str) -> Self {
        Self::InvalidUriIdentity(reason)
    }
}

/// Result type used by this crate.
pub type Result<T, E = ThumbnailError> = std::result::Result<T, E>;
