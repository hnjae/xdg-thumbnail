// SPDX-FileCopyrightText: 2026 KIM Hyunjae
// SPDX-License-Identifier: MPL-2.0

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use std::os::unix::ffi::OsStrExt;

use crate::{
    CacheRootProblem, PersonalOriginalUri, Result, SharedRelativeOriginalUri, ThumbnailError,
    ThumbnailSize, validate_mime_type,
};

/// Whole Unix epoch seconds used by `Thumb::MTime`.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct UnixMtimeSeconds {
    seconds: u64,
}

impl UnixMtimeSeconds {
    /// Creates a timestamp from non-negative whole Unix epoch seconds.
    #[must_use]
    pub const fn new(seconds: u64) -> Self {
        Self { seconds }
    }

    /// Creates a timestamp from signed whole Unix epoch seconds.
    ///
    /// # Errors
    ///
    /// Returns an error when `seconds` is negative.
    pub const fn try_from_i64(seconds: i64) -> Result<Self> {
        if seconds < 0 {
            return Err(ThumbnailError::invalid_metadata(
                "mtime is before the Unix epoch",
            ));
        }
        Ok(Self {
            seconds: seconds as u64,
        })
    }

    /// Converts a [`SystemTime`] to whole non-negative Unix epoch seconds.
    ///
    /// # Errors
    ///
    /// Returns an error when `time` is before the Unix epoch.
    pub fn from_system_time(time: SystemTime) -> Result<Self> {
        let duration = time
            .duration_since(UNIX_EPOCH)
            .map_err(|_| ThumbnailError::invalid_metadata("mtime is before the Unix epoch"))?;
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

impl TryFrom<i64> for UnixMtimeSeconds {
    type Error = ThumbnailError;

    fn try_from(seconds: i64) -> Result<Self> {
        Self::try_from_i64(seconds)
    }
}

impl fmt::Display for UnixMtimeSeconds {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.seconds)
    }
}

/// Canonical personal URI plus original freshness and write facts needed for validation and writes.
///
/// This is not a URI-only identity. It carries the `Thumb::URI` value together with the
/// modification time required for freshness checks and optional original size and MIME facts used
/// when writing thumbnail metadata.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PersonalOriginalIdentity {
    uri: PersonalOriginalUri,
    mtime: UnixMtimeSeconds,
    original_byte_size: Option<u64>,
    mime_type: Option<String>,
}

impl PersonalOriginalIdentity {
    /// Creates an original identity from caller-confirmed facts without a MIME type.
    #[must_use]
    pub fn new(uri: PersonalOriginalUri, mtime: UnixMtimeSeconds) -> Self {
        Self {
            uri,
            mtime,
            original_byte_size: None,
            mime_type: None,
        }
    }

    /// Adds the original byte size.
    #[must_use]
    pub fn with_original_byte_size(mut self, size: u64) -> Self {
        self.original_byte_size = Some(size);
        self
    }

    /// Adds a MIME type.
    ///
    /// # Errors
    ///
    /// Returns an error when `mime_type` is not valid thumbnail metadata.
    pub fn with_mime_type(mut self, mime_type: impl Into<String>) -> Result<Self> {
        let mime_type = mime_type.into();
        validate_mime_type(&mime_type)?;
        self.mime_type = Some(mime_type);
        Ok(self)
    }

    /// Returns the canonical personal-cache URI.
    #[must_use]
    pub fn uri(&self) -> &PersonalOriginalUri {
        &self.uri
    }

    /// Returns the original modification time.
    #[must_use]
    pub const fn mtime(&self) -> UnixMtimeSeconds {
        self.mtime
    }

    /// Returns the original byte size when known.
    #[must_use]
    pub const fn original_byte_size(&self) -> Option<u64> {
        self.original_byte_size
    }

    /// Returns the original MIME type when known.
    #[must_use]
    pub fn mime_type(&self) -> Option<&str> {
        self.mime_type.as_deref()
    }
}

/// An original identity whose source has been confirmed readable by the caller.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReadablePersonalOriginalIdentity {
    identity: PersonalOriginalIdentity,
}

impl ReadablePersonalOriginalIdentity {
    /// Records caller- or backend-confirmed original identity facts as readable.
    ///
    /// This performs no I/O. Callers use it to attest that the supplied identity describes an
    /// original that was readable through their chosen storage backend.
    #[must_use]
    pub fn assume_readable(identity: PersonalOriginalIdentity) -> Self {
        Self { identity }
    }

    /// Opens a local original for reading and derives its identity facts.
    ///
    /// This performs blocking filesystem I/O. Async applications should call it from a blocking
    /// adapter rather than directly on an async executor worker.
    ///
    /// The cache URI identity is built from the caller-supplied absolute path bytes without
    /// resolving symlinks. Readability, modification time, and size are taken from opening and
    /// statting the referenced file target.
    ///
    /// # Errors
    ///
    /// Returns an error when the path is not absolute, the original cannot be opened for reading,
    /// metadata or modification time cannot be read, or the path cannot form a canonical local URI.
    pub fn from_local_path(path: impl AsRef<Path>) -> Result<Self> {
        Self::from_local_path_inner(path.as_ref(), None)
    }

    /// Opens a local original for reading and derives its identity facts with a MIME type.
    ///
    /// This performs blocking filesystem I/O. Async applications should call it from a blocking
    /// adapter rather than directly on an async executor worker.
    ///
    /// The cache URI identity is built from the caller-supplied absolute path bytes without
    /// resolving symlinks. Readability, modification time, and size are taken from opening and
    /// statting the referenced file target.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`Self::from_local_path`] and also rejects invalid MIME type
    /// metadata.
    pub fn from_local_path_with_mime_type(
        path: impl AsRef<Path>,
        mime_type: impl Into<String>,
    ) -> Result<Self> {
        Self::from_local_path_inner(path.as_ref(), Some(mime_type.into()))
    }

    fn from_local_path_inner(path: &Path, mime_type: Option<String>) -> Result<Self> {
        if !path.is_absolute() {
            return Err(ThumbnailError::invalid_uri("local path must be absolute"));
        }
        let file = File::open(path).map_err(|source| {
            ThumbnailError::io("open original for reading", Some(path.to_owned()), source)
        })?;
        let metadata = file.metadata().map_err(|source| {
            ThumbnailError::io("read original metadata", Some(path.to_owned()), source)
        })?;
        let uri = PersonalOriginalUri::from_absolute_path_bytes(path.as_os_str().as_bytes())?;
        let mtime = UnixMtimeSeconds::from_system_time(metadata.modified().map_err(|source| {
            ThumbnailError::io(
                "read original modification time",
                Some(path.to_owned()),
                source,
            )
        })?)?;
        let identity =
            PersonalOriginalIdentity::new(uri, mtime).with_original_byte_size(metadata.len());
        let identity = if let Some(mime_type) = mime_type {
            identity.with_mime_type(mime_type)?
        } else {
            identity
        };
        Ok(Self { identity })
    }

    /// Returns the readable identity facts.
    #[must_use]
    pub const fn identity(&self) -> &PersonalOriginalIdentity {
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
    ///
    /// # Errors
    ///
    /// Returns an error when `repository_root` is not absolute or `original_child_name` cannot be
    /// represented as a shared direct-child URI identity.
    pub fn new(
        repository_root: impl AsRef<Path>,
        original_child_name: impl AsRef<OsStr>,
    ) -> Result<Self> {
        let repository_root = repository_root.as_ref();
        let original_child_name = original_child_name.as_ref();
        if !repository_root.is_absolute() {
            return Err(ThumbnailError::InvalidCacheRoot {
                path: repository_root.to_owned(),
                problem: CacheRootProblem::NotAbsolute,
            });
        }
        let shared_uri =
            SharedRelativeOriginalUri::from_raw_child_name(original_child_name.as_bytes())?;
        Ok(Self {
            repository_root: repository_root.to_owned(),
            original_child_name: original_child_name.to_owned(),
            shared_uri,
        })
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

    /// Computes the shared repository cache entry path for a successful thumbnail size.
    #[must_use]
    pub fn cache_entry_path(&self, size: ThumbnailSize) -> PathBuf {
        self.repository_root
            .join(".sh_thumbnails")
            .join(size.directory_name())
            .join(self.shared_uri.thumbnail_file_name())
    }
}
