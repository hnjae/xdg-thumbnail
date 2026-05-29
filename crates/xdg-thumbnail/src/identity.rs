// SPDX-FileCopyrightText: 2026 KIM Hyunjae
// SPDX-License-Identifier: MPL-2.0

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;

use crate::{
    PersonalOriginalUri, Result, SharedRelativeOriginalUri, ThumbnailError, ThumbnailSize,
    validate_mime_type,
};

/// Whole Unix epoch seconds used by `Thumb::MTime`.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct UnixMTimeSeconds {
    seconds: u64,
}

impl UnixMTimeSeconds {
    /// Creates a timestamp from non-negative whole Unix epoch seconds.
    #[must_use]
    pub const fn new(seconds: u64) -> Self {
        Self { seconds }
    }

    /// Creates a timestamp from signed whole Unix epoch seconds.
    pub const fn try_from_i64(seconds: i64) -> Result<Self> {
        if seconds < 0 {
            return Err(ThumbnailError::InvalidMetadata(
                "mtime is before the Unix epoch",
            ));
        }
        Ok(Self {
            seconds: seconds as u64,
        })
    }

    /// Converts a [`SystemTime`] to whole non-negative Unix epoch seconds.
    pub fn from_system_time(time: SystemTime) -> Result<Self> {
        let duration = time
            .duration_since(UNIX_EPOCH)
            .map_err(|_| ThumbnailError::InvalidMetadata("mtime is before the Unix epoch"))?;
        Ok(Self {
            seconds: duration.as_secs(),
        })
    }

    /// Returns whole Unix epoch seconds.
    #[must_use]
    pub const fn as_u64(self) -> u64 {
        self.seconds
    }
}

impl TryFrom<i64> for UnixMTimeSeconds {
    type Error = ThumbnailError;

    fn try_from(seconds: i64) -> Result<Self> {
        Self::try_from_i64(seconds)
    }
}

impl fmt::Display for UnixMTimeSeconds {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.seconds)
    }
}

/// Original identity and freshness facts needed for validation and writes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OriginalIdentity {
    uri: PersonalOriginalUri,
    mtime: UnixMTimeSeconds,
    size: Option<u64>,
    mime_type: Option<String>,
}

impl OriginalIdentity {
    /// Creates an original identity from caller-confirmed facts without a MIME type.
    #[must_use]
    pub fn new(uri: PersonalOriginalUri, mtime: UnixMTimeSeconds, size: Option<u64>) -> Self {
        Self {
            uri,
            mtime,
            size,
            mime_type: None,
        }
    }

    /// Creates an original identity from caller-confirmed facts with a MIME type.
    pub fn with_mime_type(
        uri: PersonalOriginalUri,
        mtime: UnixMTimeSeconds,
        size: Option<u64>,
        mime_type: impl Into<String>,
    ) -> Result<Self> {
        let mime_type = mime_type.into();
        validate_mime_type(&mime_type)?;
        Ok(Self {
            uri,
            mtime,
            size,
            mime_type: Some(mime_type),
        })
    }

    /// Returns the canonical personal-cache URI.
    #[must_use]
    pub fn uri(&self) -> &PersonalOriginalUri {
        &self.uri
    }

    /// Returns the original modification time.
    #[must_use]
    pub const fn mtime(&self) -> UnixMTimeSeconds {
        self.mtime
    }

    /// Returns the original byte size when known.
    #[must_use]
    pub const fn size(&self) -> Option<u64> {
        self.size
    }

    /// Returns the original MIME type when known.
    #[must_use]
    pub fn mime_type(&self) -> Option<&str> {
        self.mime_type.as_deref()
    }
}

/// An original identity whose source has been confirmed readable by the caller.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReadableOriginalIdentity {
    identity: OriginalIdentity,
}

impl ReadableOriginalIdentity {
    /// Marks caller-confirmed original identity facts as readable.
    #[must_use]
    pub const fn from_confirmed_readable_identity(identity: OriginalIdentity) -> Self {
        Self { identity }
    }

    /// Opens a local original for reading and derives its identity facts.
    ///
    /// This performs blocking filesystem I/O. Async applications should call it from a blocking
    /// adapter rather than directly on an async executor worker.
    #[cfg(unix)]
    pub fn from_local_path(path: impl AsRef<Path>) -> Result<Self> {
        Self::from_local_path_inner(path.as_ref(), None)
    }

    /// Opens a local original for reading and derives its identity facts with a MIME type.
    ///
    /// This performs blocking filesystem I/O. Async applications should call it from a blocking
    /// adapter rather than directly on an async executor worker.
    #[cfg(unix)]
    pub fn from_local_path_with_mime_type(
        path: impl AsRef<Path>,
        mime_type: impl Into<String>,
    ) -> Result<Self> {
        Self::from_local_path_inner(path.as_ref(), Some(mime_type.into()))
    }

    #[cfg(unix)]
    fn from_local_path_inner(path: &Path, mime_type: Option<String>) -> Result<Self> {
        if !path.is_absolute() {
            return Err(ThumbnailError::invalid_uri("local path must be absolute"));
        }
        let file = File::open(path).map_err(|source| ThumbnailError::Io {
            context: "open original for reading",
            source,
        })?;
        let metadata = file.metadata().map_err(|source| ThumbnailError::Io {
            context: "read original metadata",
            source,
        })?;
        let uri = PersonalOriginalUri::from_absolute_path_bytes(path.as_os_str().as_bytes())?;
        let mtime = UnixMTimeSeconds::from_system_time(metadata.modified().map_err(|source| {
            ThumbnailError::Io {
                context: "read original modification time",
                source,
            }
        })?)?;
        let identity = if let Some(mime_type) = mime_type {
            OriginalIdentity::with_mime_type(uri, mtime, Some(metadata.len()), mime_type)?
        } else {
            OriginalIdentity::new(uri, mtime, Some(metadata.len()))
        };
        Ok(Self { identity })
    }

    /// Opens a local original for reading and derives its identity facts.
    #[cfg(not(unix))]
    pub fn from_local_path(_path: impl AsRef<Path>) -> Result<Self> {
        Err(ThumbnailError::UnsupportedPlatform)
    }

    /// Opens a local original for reading and derives its identity facts with a MIME type.
    #[cfg(not(unix))]
    pub fn from_local_path_with_mime_type(
        _path: impl AsRef<Path>,
        _mime_type: impl Into<String>,
    ) -> Result<Self> {
        Err(ThumbnailError::UnsupportedPlatform)
    }

    /// Returns the readable identity facts.
    #[must_use]
    pub const fn identity(&self) -> &OriginalIdentity {
        &self.identity
    }
}

/// Explicit context for read-only shared repository lookup.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SharedRepositoryContext {
    repository_root: PathBuf,
    original_child_name: OsString,
    shared_uri: SharedRelativeOriginalUri,
}

impl SharedRepositoryContext {
    /// Creates a shared repository context for one direct child of `repository_root`.
    #[cfg(unix)]
    pub fn new(repository_root: impl AsRef<Path>, original_child_name: &OsStr) -> Result<Self> {
        let repository_root = repository_root.as_ref();
        if !repository_root.is_absolute() {
            return Err(ThumbnailError::CacheRootUnavailable(
                "shared repository root must be absolute",
            ));
        }
        let shared_uri =
            SharedRelativeOriginalUri::from_raw_child_name(original_child_name.as_bytes())?;
        Ok(Self {
            repository_root: repository_root.to_owned(),
            original_child_name: original_child_name.to_owned(),
            shared_uri,
        })
    }

    /// Creates a shared repository context for one direct child of `repository_root`.
    #[cfg(not(unix))]
    pub fn new(_repository_root: impl AsRef<Path>, _original_child_name: &OsStr) -> Result<Self> {
        Err(ThumbnailError::UnsupportedPlatform)
    }

    /// Returns the shared repository root directory.
    #[must_use]
    pub fn repository_root(&self) -> &Path {
        &self.repository_root
    }

    /// Returns the direct child original filename.
    #[must_use]
    pub fn original_child_name(&self) -> &OsStr {
        &self.original_child_name
    }

    /// Returns the shared URI used for hashing and optional metadata comparison.
    #[must_use]
    pub const fn shared_uri(&self) -> &SharedRelativeOriginalUri {
        &self.shared_uri
    }

    /// Computes the shared repository path for a successful thumbnail size.
    #[must_use]
    pub fn thumbnail_path(&self, size: ThumbnailSize) -> PathBuf {
        self.repository_root
            .join(".sh_thumbnails")
            .join(size.directory_name())
            .join(self.shared_uri.thumbnail_filename())
    }
}
