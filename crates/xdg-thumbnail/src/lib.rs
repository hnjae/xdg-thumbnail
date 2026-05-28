// SPDX-FileCopyrightText: 2026 KIM Hyunjae
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Freedesktop thumbnail cache primitives.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs::{self, File};
use std::io::Cursor;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, PermissionsExt};

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
    fn invalid_uri(reason: &'static str) -> Self {
        Self::InvalidUriIdentity(reason)
    }
}

/// Result type used by this crate.
pub type Result<T, E = ThumbnailError> = std::result::Result<T, E>;

/// Root directory of the personal thumbnail cache, usually `$XDG_CACHE_HOME/thumbnails`.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct CacheRoot {
    path: PathBuf,
}

impl CacheRoot {
    /// Creates a cache root from an already resolved absolute thumbnail root path.
    pub fn new(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if !path.is_absolute() {
            return Err(ThumbnailError::CacheRootUnavailable(
                "thumbnail cache root must be absolute",
            ));
        }
        Ok(Self {
            path: path.to_owned(),
        })
    }

    /// Resolves the personal thumbnail cache root from the process environment.
    pub fn resolve_from_env() -> Result<Self> {
        let xdg_cache_home = std::env::var_os("XDG_CACHE_HOME");
        let home = std::env::var_os("HOME");
        Self::resolve_from_values(xdg_cache_home.as_deref(), home.as_deref())
    }

    /// Resolves the personal thumbnail cache root from supplied XDG values.
    ///
    /// Relative, unset, and blank `XDG_CACHE_HOME` values are ignored. `HOME`
    /// must be absolute when fallback is needed.
    #[cfg(unix)]
    pub fn resolve_from_values(
        xdg_cache_home: Option<&OsStr>,
        home: Option<&OsStr>,
    ) -> Result<Self> {
        if let Some(cache_home) = xdg_cache_home {
            if !cache_home.as_bytes().is_empty() {
                let path = PathBuf::from(cache_home);
                if path.is_absolute() {
                    return Self::new(path.join("thumbnails"));
                }
            }
        }

        let Some(home) = home else {
            return Err(ThumbnailError::CacheRootUnavailable(
                "HOME is required when XDG_CACHE_HOME is unset, blank, or relative",
            ));
        };
        if home.as_bytes().is_empty() {
            return Err(ThumbnailError::CacheRootUnavailable(
                "HOME is required when XDG_CACHE_HOME is unset, blank, or relative",
            ));
        }
        let home = PathBuf::from(home);
        if !home.is_absolute() {
            return Err(ThumbnailError::CacheRootUnavailable(
                "HOME must be absolute",
            ));
        }
        Self::new(home.join(".cache").join("thumbnails"))
    }

    /// Resolves the personal thumbnail cache root from supplied XDG values.
    #[cfg(not(unix))]
    pub fn resolve_from_values(
        _xdg_cache_home: Option<&OsStr>,
        _home: Option<&OsStr>,
    ) -> Result<Self> {
        Err(ThumbnailError::UnsupportedPlatform)
    }

    /// Returns the resolved thumbnail root path.
    #[must_use]
    pub fn as_path(&self) -> &Path {
        &self.path
    }

    /// Computes the personal-cache path for an accepted URI and namespace without reading it.
    #[must_use]
    pub fn personal_path(&self, uri: &PersonalThumbnailUri, namespace: &CacheNamespace) -> PathBuf {
        namespace.join_under(&self.path, &uri.thumbnail_filename())
    }

    /// Returns a validated personal-cache path for integrations that must pass a filename.
    ///
    /// The candidate PNG is opened and validated before this method returns. Callers that reopen
    /// the returned path accept that another process may replace it after validation.
    pub fn validated_personal_path(
        &self,
        original: &OriginalIdentity,
        size: ThumbnailSize,
    ) -> Result<ThumbnailLookup<ValidatedThumbnailPath>> {
        let path = self.personal_path(original.uri(), &CacheNamespace::Size(size));
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(ThumbnailLookup::Missing);
            }
            Err(source) => {
                return Err(ThumbnailError::Io {
                    context: "read thumbnail cache entry",
                    source,
                });
            }
        };

        match validate_personal_thumbnail(&bytes, original, size) {
            ValidationOutcome::FullyVerified => {
                let parsed = ParsedThumbnailPng::parse(&bytes)?;
                Ok(ThumbnailLookup::Valid(ValidatedThumbnailPath {
                    path,
                    metadata: parsed.metadata,
                }))
            }
            ValidationOutcome::Invalid(problems) => Ok(ThumbnailLookup::Invalid(problems)),
            ValidationOutcome::SharedMetadataIncomplete
            | ValidationOutcome::UncheckedInspection => Ok(ThumbnailLookup::Invalid(vec![
                CacheEntryProblem::UnverifiableOriginal,
            ])),
        }
    }

    /// Returns exact validated PNG bytes from the personal thumbnail cache.
    pub fn validated_personal_payload(
        &self,
        original: &OriginalIdentity,
        size: ThumbnailSize,
    ) -> Result<ThumbnailLookup<ValidatedThumbnailPayload>> {
        let path = self.personal_path(original.uri(), &CacheNamespace::Size(size));
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(ThumbnailLookup::Missing);
            }
            Err(source) => {
                return Err(ThumbnailError::Io {
                    context: "read thumbnail cache entry",
                    source,
                });
            }
        };

        match validate_personal_thumbnail(&bytes, original, size) {
            ValidationOutcome::FullyVerified => {
                let parsed = ParsedThumbnailPng::parse(&bytes)?;
                Ok(ThumbnailLookup::Valid(ValidatedThumbnailPayload {
                    path,
                    bytes,
                    metadata: parsed.metadata,
                }))
            }
            ValidationOutcome::Invalid(problems) => Ok(ThumbnailLookup::Invalid(problems)),
            ValidationOutcome::SharedMetadataIncomplete
            | ValidationOutcome::UncheckedInspection => Ok(ThumbnailLookup::Invalid(vec![
                CacheEntryProblem::UnverifiableOriginal,
            ])),
        }
    }

    /// Normalizes rendered PNG data and atomically installs a personal-cache thumbnail.
    pub fn install_personal_thumbnail(
        &self,
        original: &ReadableOriginalIdentity,
        size: ThumbnailSize,
        rendered_png: &[u8],
    ) -> Result<InstalledThumbnail> {
        let namespace = CacheNamespace::Size(size);
        let path = self.personal_path(original.identity().uri(), &namespace);
        let bytes = normalized_personal_thumbnail_png(rendered_png, original.identity(), size)?;
        self.write_personal_entry(&path, &namespace, &bytes)?;
        Ok(InstalledThumbnail { path, bytes })
    }

    /// Writes a deterministic 1x1 transparent failure entry in an explicit namespace.
    pub fn write_failure_entry(
        &self,
        namespace: &FailureNamespace,
        original: &ReadableOriginalIdentity,
    ) -> Result<InstalledThumbnail> {
        let namespace = CacheNamespace::Failure(namespace.clone());
        let path = self.personal_path(original.identity().uri(), &namespace);
        let bytes = encode_rgba_png(
            1,
            1,
            &[0, 0, 0, 0],
            &thumbnail_metadata_pairs(original.identity()),
        )?;
        self.write_personal_entry(&path, &namespace, &bytes)?;
        Ok(InstalledThumbnail { path, bytes })
    }

    /// Inspects standard successful thumbnail size directories.
    pub fn inspect_thumbnails(
        &self,
        sizes: &[ThumbnailSize],
        include_nonstandard: bool,
    ) -> Result<Vec<CacheEntryInspection>> {
        let mut inspections = Vec::new();
        for &size in sizes {
            let namespace = CacheNamespace::Size(size);
            let dir = self.path.join(size.directory_name());
            self.inspect_namespace_dir(
                &dir,
                namespace,
                include_nonstandard,
                Some(size),
                &mut inspections,
            )?;
        }
        Ok(inspections)
    }

    /// Inspects direct files in immediate real failure-entry namespaces.
    pub fn inspect_failure_entries(
        &self,
        include_nonstandard: bool,
    ) -> Result<Vec<CacheEntryInspection>> {
        let fail_root = self.path.join("fail");
        let mut inspections = Vec::new();
        let entries = match fs::read_dir(&fail_root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(inspections),
            Err(source) => {
                return Err(ThumbnailError::Io {
                    context: "read failure thumbnail directory",
                    source,
                });
            }
        };

        for entry in entries {
            let entry = entry.map_err(|source| ThumbnailError::Io {
                context: "read failure namespace directory entry",
                source,
            })?;
            let file_type = entry.file_type().map_err(|source| ThumbnailError::Io {
                context: "read failure namespace file type",
                source,
            })?;
            if file_type.is_symlink() || !file_type.is_dir() {
                continue;
            }
            let Some(namespace_name) = entry.file_name().to_str().map(ToOwned::to_owned) else {
                continue;
            };
            let Ok(namespace) = FailureNamespace::new(namespace_name) else {
                continue;
            };
            self.inspect_namespace_dir(
                &entry.path(),
                CacheNamespace::Failure(namespace),
                include_nonstandard,
                None,
                &mut inspections,
            )?;
        }
        Ok(inspections)
    }

    fn inspect_namespace_dir(
        &self,
        dir: &Path,
        namespace: CacheNamespace,
        include_nonstandard: bool,
        successful_size: Option<ThumbnailSize>,
        inspections: &mut Vec<CacheEntryInspection>,
    ) -> Result<()> {
        let metadata = match fs::symlink_metadata(dir) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(source) => {
                return Err(ThumbnailError::Io {
                    context: "inspect thumbnail namespace directory",
                    source,
                });
            }
        };
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Ok(());
        }

        let entries = match fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(source) => {
                return Err(ThumbnailError::Io {
                    context: "read thumbnail namespace directory",
                    source,
                });
            }
        };

        for entry in entries {
            let entry = entry.map_err(|source| ThumbnailError::Io {
                context: "read thumbnail directory entry",
                source,
            })?;
            let path = entry.path();
            let filename = entry.file_name();
            let standard = filename
                .to_str()
                .is_some_and(is_standard_thumbnail_filename);
            if !standard && !include_nonstandard {
                continue;
            }

            let handle = CacheEntryHandle {
                cache_dir: dir.to_owned(),
                path: path.clone(),
            };
            if standard {
                inspections.push(inspect_cache_entry(
                    path,
                    namespace.clone(),
                    handle,
                    successful_size,
                ));
            } else {
                let timestamps = thumbnail_timestamps(&path, AccessTimePreservation::NotNeeded);
                inspections.push(CacheEntryInspection {
                    outcome: ValidationOutcome::Invalid(vec![
                        CacheEntryProblem::NonstandardFilename,
                    ]),
                    original_uri: None,
                    timestamps,
                    namespace: namespace.clone(),
                    path,
                    handle,
                });
            }
        }
        Ok(())
    }

    fn write_personal_entry(
        &self,
        path: &Path,
        namespace: &CacheNamespace,
        bytes: &[u8],
    ) -> Result<()> {
        self.ensure_namespace_dir(namespace)?;
        let parent = path.parent().ok_or(ThumbnailError::CacheRootUnavailable(
            "cache path has no parent directory",
        ))?;
        let mut temp = tempfile::Builder::new()
            .prefix(".xdg-thumbnail-")
            .tempfile_in(parent)
            .map_err(|source| ThumbnailError::Io {
                context: "create thumbnail temporary file",
                source,
            })?;
        temp.as_file_mut()
            .write_all(bytes)
            .map_err(|source| ThumbnailError::Io {
                context: "write thumbnail temporary file",
                source,
            })?;
        temp.as_file_mut()
            .set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|source| ThumbnailError::Io {
                context: "set thumbnail temporary file permissions",
                source,
            })?;
        temp.as_file_mut()
            .sync_all()
            .map_err(|source| ThumbnailError::Io {
                context: "sync thumbnail temporary file",
                source,
            })?;
        fs::rename(temp.path(), path).map_err(|source| ThumbnailError::Io {
            context: "publish thumbnail cache entry",
            source,
        })?;
        Ok(())
    }

    fn ensure_namespace_dir(&self, namespace: &CacheNamespace) -> Result<()> {
        ensure_private_directory(&self.path)?;
        match namespace {
            CacheNamespace::Size(size) => {
                ensure_private_directory(&self.path.join(size.directory_name()))
            }
            CacheNamespace::Failure(namespace) => {
                let fail = self.path.join("fail");
                ensure_private_directory(&fail)?;
                ensure_private_directory(&fail.join(namespace.as_str()))
            }
        }
    }
}

/// A canonical absolute URI identity for entries in the personal thumbnail cache.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PersonalThumbnailUri {
    value: String,
}

impl PersonalThumbnailUri {
    /// Constructs a canonical `file:///` URI from absolute Unix path bytes.
    ///
    /// This constructor performs byte-level percent-encoding and never expands
    /// shell syntax or resolves symlinks.
    #[cfg(unix)]
    pub fn from_absolute_path_bytes(path: &[u8]) -> Result<Self> {
        if !path.starts_with(b"/") {
            return Err(ThumbnailError::invalid_uri("local path must be absolute"));
        }
        if path.contains(&0) {
            return Err(ThumbnailError::invalid_uri(
                "local path must not contain NUL",
            ));
        }

        Ok(Self {
            value: format!("file://{}", encode_uri_path_bytes(path, true)),
        })
    }

    /// Constructs a canonical `file:///` URI from absolute Unix path bytes.
    #[cfg(not(unix))]
    pub fn from_absolute_path_bytes(_path: &[u8]) -> Result<Self> {
        Err(ThumbnailError::UnsupportedPlatform)
    }

    /// Accepts textual local `file:` URI input and normalizes `localhost`.
    pub fn from_local_file_uri(uri: &str) -> Result<Self> {
        validate_ascii_uri_identity(uri)?;
        let rest = uri
            .strip_prefix("file:")
            .ok_or_else(|| ThumbnailError::invalid_uri("local URI must use the file scheme"))?;

        let normalized = if let Some(path) = rest.strip_prefix("//localhost/") {
            format!("file:///{path}")
        } else if rest.starts_with("///") {
            uri.to_owned()
        } else if rest.starts_with("//") {
            return Err(ThumbnailError::invalid_uri(
                "file URI authority is not directly local",
            ));
        } else if rest.starts_with('/') {
            format!("file://{rest}")
        } else {
            return Err(ThumbnailError::invalid_uri(
                "file URI path must be absolute",
            ));
        };

        let path = normalized
            .strip_prefix("file://")
            .ok_or_else(|| ThumbnailError::invalid_uri("local URI must use the file scheme"))?;
        if !path.starts_with('/') {
            return Err(ThumbnailError::invalid_uri(
                "file URI path must be absolute",
            ));
        }
        validate_percent_escapes(path.as_bytes())?;

        Ok(Self { value: normalized })
    }

    /// Accepts a caller-selected absolute thumbnail URI identity and preserves it exactly.
    pub fn from_absolute_uri(uri: &str) -> Result<Self> {
        validate_ascii_uri_identity(uri)?;
        let scheme_end = uri
            .find(':')
            .ok_or_else(|| ThumbnailError::invalid_uri("URI must be absolute"))?;
        validate_scheme(&uri[..scheme_end])?;
        validate_percent_escapes(uri.as_bytes())?;

        Ok(Self {
            value: uri.to_owned(),
        })
    }

    /// Returns the canonical URI identity string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.value
    }

    /// Returns the Freedesktop MD5 filename stem for this URI identity.
    #[must_use]
    pub fn md5_stem(&self) -> String {
        md5_stem(self.value.as_bytes())
    }

    /// Returns the Freedesktop thumbnail filename for this URI identity.
    #[must_use]
    pub fn thumbnail_filename(&self) -> String {
        format!("{}.png", self.md5_stem())
    }
}

impl fmt::Display for PersonalThumbnailUri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.value)
    }
}

/// A canonical `./`-prefixed URI identity for direct children in shared repositories.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SharedRelativeThumbnailUri {
    value: String,
}

impl SharedRelativeThumbnailUri {
    /// Constructs a shared URI from one raw direct child filename.
    pub fn from_raw_child_name(name: &[u8]) -> Result<Self> {
        validate_raw_shared_child_name(name)?;

        Ok(Self {
            value: format!("./{}", encode_uri_path_bytes(name, false)),
        })
    }

    /// Parses textual `./` shared URI input without allowing encoded path separators.
    pub fn parse(uri: &str) -> Result<Self> {
        validate_ascii_uri_identity(uri)?;
        let encoded = uri
            .strip_prefix("./")
            .ok_or_else(|| ThumbnailError::invalid_uri("shared URI must start with ./"))?;
        if encoded.is_empty() {
            return Err(ThumbnailError::invalid_uri(
                "shared URI child name must not be empty",
            ));
        }
        if encoded.contains('/') {
            return Err(ThumbnailError::invalid_uri(
                "shared URI must name one direct child",
            ));
        }
        validate_percent_escapes(encoded.as_bytes())?;
        let decoded = percent_decode_bytes(encoded.as_bytes())?;
        validate_raw_shared_child_name(&decoded)?;

        Ok(Self {
            value: uri.to_owned(),
        })
    }

    /// Returns the canonical shared relative URI identity string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.value
    }

    /// Returns the Freedesktop MD5 filename stem for this URI identity.
    #[must_use]
    pub fn md5_stem(&self) -> String {
        md5_stem(self.value.as_bytes())
    }

    /// Returns the Freedesktop thumbnail filename for this URI identity.
    #[must_use]
    pub fn thumbnail_filename(&self) -> String {
        format!("{}.png", self.md5_stem())
    }
}

/// A successful-thumbnail size namespace or a program failure namespace.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum CacheNamespace {
    /// A successful thumbnail size directory.
    Size(ThumbnailSize),
    /// A failure-entry namespace under `fail/`.
    Failure(FailureNamespace),
}

impl CacheNamespace {
    fn join_under(&self, root: &Path, filename: &str) -> PathBuf {
        match self {
            Self::Size(size) => root.join(size.directory_name()).join(filename),
            Self::Failure(namespace) => root.join("fail").join(namespace.as_str()).join(filename),
        }
    }

    /// Returns a display-oriented namespace name.
    #[must_use]
    pub fn name(&self) -> String {
        match self {
            Self::Size(size) => size.directory_name().to_owned(),
            Self::Failure(namespace) => format!("fail/{}", namespace.as_str()),
        }
    }
}

/// A validated direct directory name for failure entries.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct FailureNamespace {
    value: String,
}

impl FailureNamespace {
    /// Creates a failure namespace from an ASCII direct directory name.
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        if value.is_empty() || value == "." || value == ".." {
            return Err(ThumbnailError::InvalidNamespace(
                "failure namespace must be a non-empty direct name",
            ));
        }
        if !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'+' | b'-'))
        {
            return Err(ThumbnailError::InvalidNamespace(
                "failure namespace contains an invalid character",
            ));
        }
        Ok(Self { value })
    }

    /// Returns the namespace directory name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.value
    }
}

impl fmt::Display for FailureNamespace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.value)
    }
}

/// Whole Unix epoch seconds used by `Thumb::MTime`.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct UnixMTimeSeconds {
    seconds: i64,
}

impl UnixMTimeSeconds {
    /// Creates a timestamp from whole Unix epoch seconds.
    #[must_use]
    pub const fn new(seconds: i64) -> Self {
        Self { seconds }
    }

    /// Converts a [`SystemTime`] to whole non-negative Unix epoch seconds.
    pub fn from_system_time(time: SystemTime) -> Result<Self> {
        let duration = time
            .duration_since(UNIX_EPOCH)
            .map_err(|_| ThumbnailError::InvalidMetadata("mtime is before the Unix epoch"))?;
        let seconds = i64::try_from(duration.as_secs())
            .map_err(|_| ThumbnailError::InvalidMetadata("mtime overflows i64 seconds"))?;
        Ok(Self { seconds })
    }

    /// Returns whole Unix epoch seconds.
    #[must_use]
    pub const fn as_i64(self) -> i64 {
        self.seconds
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
    uri: PersonalThumbnailUri,
    mtime: UnixMTimeSeconds,
    size: Option<u64>,
    mime_type: Option<String>,
}

impl OriginalIdentity {
    /// Creates an original identity from caller-confirmed facts.
    pub fn new(
        uri: PersonalThumbnailUri,
        mtime: UnixMTimeSeconds,
        size: Option<u64>,
        mime_type: Option<impl Into<String>>,
    ) -> Result<Self> {
        let mime_type = mime_type.map(Into::into);
        if let Some(mime_type) = mime_type.as_deref() {
            validate_mime_type(mime_type)?;
        }
        Ok(Self {
            uri,
            mtime,
            size,
            mime_type,
        })
    }

    /// Returns the canonical personal-cache URI.
    #[must_use]
    pub fn uri(&self) -> &PersonalThumbnailUri {
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
    pub const fn new(identity: OriginalIdentity) -> Self {
        Self { identity }
    }

    /// Opens a local original for reading and derives its identity facts.
    #[cfg(unix)]
    pub fn from_local_path(
        path: impl AsRef<Path>,
        mime_type: Option<impl Into<String>>,
    ) -> Result<Self> {
        let path = path.as_ref();
        let file = File::open(path).map_err(|source| ThumbnailError::Io {
            context: "open original for reading",
            source,
        })?;
        let metadata = file.metadata().map_err(|source| ThumbnailError::Io {
            context: "read original metadata",
            source,
        })?;
        let uri = PersonalThumbnailUri::from_absolute_path_bytes(path.as_os_str().as_bytes())?;
        let mtime = UnixMTimeSeconds::from_system_time(metadata.modified().map_err(|source| {
            ThumbnailError::Io {
                context: "read original modification time",
                source,
            }
        })?)?;
        let identity = OriginalIdentity::new(uri, mtime, Some(metadata.len()), mime_type)?;
        Ok(Self { identity })
    }

    /// Opens a local original for reading and derives its identity facts.
    #[cfg(not(unix))]
    pub fn from_local_path(
        _path: impl AsRef<Path>,
        _mime_type: Option<impl Into<String>>,
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
    shared_uri: SharedRelativeThumbnailUri,
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
            SharedRelativeThumbnailUri::from_raw_child_name(original_child_name.as_bytes())?;
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
    pub const fn shared_uri(&self) -> &SharedRelativeThumbnailUri {
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

impl fmt::Display for SharedRelativeThumbnailUri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.value)
    }
}

fn md5_stem(input: &[u8]) -> String {
    format!("{:x}", md5::compute(input))
}

fn validate_raw_shared_child_name(name: &[u8]) -> Result<()> {
    if name.is_empty() {
        return Err(ThumbnailError::invalid_uri(
            "shared child name must not be empty",
        ));
    }
    if name == b"." || name == b".." {
        return Err(ThumbnailError::invalid_uri(
            "shared child name must not be . or ..",
        ));
    }
    if name.contains(&b'/') || name.contains(&0) {
        return Err(ThumbnailError::invalid_uri(
            "shared child name must be one path segment",
        ));
    }
    Ok(())
}

fn validate_scheme(scheme: &str) -> Result<()> {
    let mut bytes = scheme.bytes();
    let Some(first) = bytes.next() else {
        return Err(ThumbnailError::invalid_uri("URI scheme must not be empty"));
    };
    if !first.is_ascii_alphabetic() {
        return Err(ThumbnailError::invalid_uri(
            "URI scheme must start with an ASCII letter",
        ));
    }
    if !bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.')) {
        return Err(ThumbnailError::invalid_uri(
            "URI scheme contains an invalid character",
        ));
    }
    Ok(())
}

fn validate_ascii_uri_identity(uri: &str) -> Result<()> {
    if uri.is_empty() {
        return Err(ThumbnailError::invalid_uri("URI must not be empty"));
    }
    if !uri.is_ascii() {
        return Err(ThumbnailError::invalid_uri(
            "URI identity must be ASCII and percent-encoded",
        ));
    }
    if uri.bytes().any(|byte| byte.is_ascii_control()) {
        return Err(ThumbnailError::invalid_uri(
            "URI identity must not contain control characters",
        ));
    }
    Ok(())
}

fn validate_percent_escapes(input: &[u8]) -> Result<()> {
    let mut i = 0;
    while i < input.len() {
        if input[i] == b'%' {
            if i + 2 >= input.len()
                || !input[i + 1].is_ascii_hexdigit()
                || !input[i + 2].is_ascii_hexdigit()
            {
                return Err(ThumbnailError::invalid_uri(
                    "URI contains an invalid percent escape",
                ));
            }
            i += 3;
        } else {
            i += 1;
        }
    }
    Ok(())
}

fn percent_decode_bytes(input: &[u8]) -> Result<Vec<u8>> {
    validate_percent_escapes(input)?;
    let mut output = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        if input[i] == b'%' {
            let high = hex_value(input[i + 1]).ok_or_else(|| {
                ThumbnailError::invalid_uri("URI contains an invalid percent escape")
            })?;
            let low = hex_value(input[i + 2]).ok_or_else(|| {
                ThumbnailError::invalid_uri("URI contains an invalid percent escape")
            })?;
            output.push(high << 4 | low);
            i += 3;
        } else {
            output.push(input[i]);
            i += 1;
        }
    }
    Ok(output)
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn encode_uri_path_bytes(bytes: &[u8], allow_slash: bool) -> String {
    let mut encoded = String::with_capacity(bytes.len());
    for &byte in bytes {
        if is_safe_path_byte(byte, allow_slash) {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

fn is_safe_path_byte(byte: u8, allow_slash: bool) -> bool {
    byte.is_ascii_alphanumeric()
        || (allow_slash && byte == b'/')
        || matches!(
            byte,
            b'-' | b'.'
                | b'_'
                | b'~'
                | b'!'
                | b'$'
                | b'&'
                | b'\''
                | b'('
                | b')'
                | b'*'
                | b'+'
                | b','
                | b';'
                | b'='
                | b':'
                | b'@'
        )
}

/// Policy-neutral problem found while validating or inspecting a cache entry.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum CacheEntryProblem {
    /// The original local file is missing.
    OriginalMissing,
    /// Metadata is well-formed but no longer matches the original.
    StaleMetadata,
    /// The original could not be read well enough to validate the entry.
    UnreadableOriginal,
    /// The original cannot be verified in this validation context.
    UnverifiableOriginal,
    /// Required metadata is missing for this validation context.
    MissingRequiredMetadata,
    /// Present metadata has invalid syntax.
    InvalidMetadataSyntax,
    /// PNG structure could not be decoded.
    InvalidPngStructure,
    /// PNG encoding does not conform to the successful-thumbnail requirements.
    NonconformingPngFormat,
    /// PNG dimensions exceed the requested namespace.
    DimensionsExceedNamespace,
    /// The cache directory entry is not a standard thumbnail filename.
    NonstandardFilename,
}

/// Validation confidence and validity for a cache entry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ValidationOutcome {
    /// Required metadata and PNG constraints are fully verified.
    FullyVerified,
    /// Shared thumbnail metadata is standard-allowed but incomplete.
    SharedMetadataIncomplete,
    /// Inspection did not validate the entry against an original.
    UncheckedInspection,
    /// The entry is invalid for the requested validation context.
    Invalid(Vec<CacheEntryProblem>),
}

/// Result of a validated thumbnail cache lookup.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ThumbnailLookup<T> {
    /// The cache entry exists and passed validation.
    Valid(T),
    /// The computed cache path does not exist.
    Missing,
    /// The cache entry exists but is invalid for the requested context.
    Invalid(Vec<CacheEntryProblem>),
    /// The original could not be verified, so no existing cache entry is display-valid.
    Unverifiable(Vec<CacheEntryProblem>),
}

/// A validated cache path and metadata facts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedThumbnailPath {
    path: PathBuf,
    metadata: ThumbnailMetadata,
}

impl ValidatedThumbnailPath {
    /// Returns the path that was validated.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns metadata parsed from the validated PNG.
    #[must_use]
    pub const fn metadata(&self) -> &ThumbnailMetadata {
        &self.metadata
    }
}

/// Exact validated PNG bytes and metadata facts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedThumbnailPayload {
    path: PathBuf,
    bytes: Vec<u8>,
    metadata: ThumbnailMetadata,
}

/// Result of a successful personal-cache install.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstalledThumbnail {
    path: PathBuf,
    bytes: Vec<u8>,
}

/// Original URI identity parsed from a cache entry.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum ThumbnailUriIdentity {
    /// Absolute personal-cache URI identity.
    Personal(PersonalThumbnailUri),
    /// Shared repository relative URI identity.
    Shared(SharedRelativeThumbnailUri),
}

/// Whether access time was preserved while inspecting an entry.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum AccessTimePreservation {
    /// Inspection preserved access time.
    Preserved,
    /// Inspection may have updated access time.
    NotPreserved,
    /// No content read was needed.
    NotNeeded,
    /// Access-time preservation is unsupported.
    Unsupported,
}

/// Timestamp facts captured for a thumbnail cache entry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThumbnailTimestamps {
    accessed_at: Option<SystemTime>,
    modified_at: Option<SystemTime>,
    access_time_preserved_during_inspection: AccessTimePreservation,
}

impl ThumbnailTimestamps {
    /// Returns the thumbnail file access time when available.
    #[must_use]
    pub const fn accessed_at(&self) -> Option<SystemTime> {
        self.accessed_at
    }

    /// Returns the thumbnail file modification time when available.
    #[must_use]
    pub const fn modified_at(&self) -> Option<SystemTime> {
        self.modified_at
    }

    /// Returns whether metadata inspection preserved access time.
    #[must_use]
    pub const fn access_time_preserved_during_inspection(&self) -> AccessTimePreservation {
        self.access_time_preserved_during_inspection
    }
}

/// Policy-neutral inspection facts for a cache entry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CacheEntryInspection {
    outcome: ValidationOutcome,
    original_uri: Option<ThumbnailUriIdentity>,
    timestamps: ThumbnailTimestamps,
    namespace: CacheNamespace,
    path: PathBuf,
    handle: CacheEntryHandle,
}

impl CacheEntryInspection {
    /// Returns the validation or inspection outcome.
    #[must_use]
    pub const fn outcome(&self) -> &ValidationOutcome {
        &self.outcome
    }

    /// Returns the original URI parsed from metadata when present and valid.
    #[must_use]
    pub const fn original_uri(&self) -> Option<&ThumbnailUriIdentity> {
        self.original_uri.as_ref()
    }

    /// Returns timestamp facts.
    #[must_use]
    pub const fn timestamps(&self) -> &ThumbnailTimestamps {
        &self.timestamps
    }

    /// Returns the cache namespace.
    #[must_use]
    pub const fn namespace(&self) -> &CacheNamespace {
        &self.namespace
    }

    /// Returns the inspected cache path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns a handle that can safely remove this cache entry.
    #[must_use]
    pub const fn handle(&self) -> &CacheEntryHandle {
        &self.handle
    }
}

/// A handle for a discovered cache entry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CacheEntryHandle {
    cache_dir: PathBuf,
    path: PathBuf,
}

impl CacheEntryHandle {
    /// Removes the handled entry after containment and symlink checks.
    pub fn remove(&self) -> Result<()> {
        if self.path.parent() != Some(self.cache_dir.as_path()) {
            return Err(ThumbnailError::UnsafeRemoval(
                "entry is not a direct child of its cache directory",
            ));
        }
        let cache_dir_metadata =
            fs::symlink_metadata(&self.cache_dir).map_err(|source| ThumbnailError::Io {
                context: "inspect cache directory before removal",
                source,
            })?;
        if cache_dir_metadata.file_type().is_symlink() || !cache_dir_metadata.is_dir() {
            return Err(ThumbnailError::UnsafeRemoval(
                "cache directory is not a real directory",
            ));
        }
        let metadata = fs::symlink_metadata(&self.path).map_err(|source| ThumbnailError::Io {
            context: "inspect cache entry before removal",
            source,
        })?;
        if metadata.file_type().is_symlink() {
            return Err(ThumbnailError::UnsafeRemoval("entry is a symlink"));
        }
        if !metadata.is_file() {
            return Err(ThumbnailError::UnsafeRemoval("entry is not a regular file"));
        }
        fs::remove_file(&self.path).map_err(|source| ThumbnailError::Io {
            context: "remove cache entry",
            source,
        })
    }

    /// Returns the handled path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl InstalledThumbnail {
    /// Returns the installed cache path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the final normalized PNG bytes that were installed.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

impl ValidatedThumbnailPayload {
    /// Returns the path from which the payload was validated.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the exact PNG bytes that passed validation.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Returns metadata parsed from the validated PNG.
    #[must_use]
    pub const fn metadata(&self) -> &ThumbnailMetadata {
        &self.metadata
    }
}

/// Freedesktop thumbnail PNG text metadata.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ThumbnailMetadata {
    values: BTreeMap<String, String>,
}

impl ThumbnailMetadata {
    /// Returns a raw metadata value by key.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }

    /// Returns `Thumb::URI` when present.
    #[must_use]
    pub fn thumb_uri(&self) -> Option<&str> {
        self.get("Thumb::URI")
    }

    /// Returns parsed `Thumb::MTime` when present and syntactically valid.
    #[must_use]
    pub fn thumb_mtime(&self) -> Option<i64> {
        self.thumb_mtime_result().ok().flatten()
    }

    fn thumb_mtime_result(&self) -> std::result::Result<Option<i64>, std::num::ParseIntError> {
        self.get("Thumb::MTime").map(str::parse::<i64>).transpose()
    }

    /// Returns parsed `Thumb::Size` when present and syntactically valid.
    #[must_use]
    pub fn thumb_size(&self) -> Option<u64> {
        self.thumb_size_result().ok().flatten()
    }

    fn thumb_size_result(&self) -> std::result::Result<Option<u64>, std::num::ParseIntError> {
        self.get("Thumb::Size").map(str::parse::<u64>).transpose()
    }

    /// Returns `Thumb::Mimetype` when present.
    #[must_use]
    pub fn thumb_mimetype(&self) -> Option<&str> {
        self.get("Thumb::Mimetype")
    }
}

/// Decoded facts from a thumbnail PNG.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParsedThumbnailPng {
    width: u32,
    height: u32,
    bit_depth: png::BitDepth,
    color_type: png::ColorType,
    interlaced: bool,
    metadata: ThumbnailMetadata,
}

impl ParsedThumbnailPng {
    /// Parses PNG structure and Freedesktop text metadata.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let decoder = png::Decoder::new(Cursor::new(bytes));
        let mut reader = decoder
            .read_info()
            .map_err(|err| ThumbnailError::Png(err.to_string()))?;
        let Some(output_buffer_size) = reader.output_buffer_size() else {
            return Err(ThumbnailError::Png(
                "png output buffer size is unavailable".to_owned(),
            ));
        };
        let mut buffer = vec![0; output_buffer_size];
        reader
            .next_frame(&mut buffer)
            .map_err(|err| ThumbnailError::Png(err.to_string()))?;

        let info = reader.info();
        let mut values = BTreeMap::new();
        for chunk in &info.uncompressed_latin1_text {
            values.insert(chunk.keyword.clone(), chunk.text.clone());
        }
        for chunk in &info.compressed_latin1_text {
            let text = chunk
                .get_text()
                .map_err(|err| ThumbnailError::Png(err.to_string()))?;
            values.insert(chunk.keyword.clone(), text);
        }
        for chunk in &info.utf8_text {
            let text = chunk
                .get_text()
                .map_err(|err| ThumbnailError::Png(err.to_string()))?;
            values.insert(chunk.keyword.clone(), text);
        }

        Ok(Self {
            width: info.width,
            height: info.height,
            bit_depth: info.bit_depth,
            color_type: info.color_type,
            interlaced: info.interlaced,
            metadata: ThumbnailMetadata { values },
        })
    }

    /// Returns the image width in pixels.
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Returns the image height in pixels.
    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// Returns the image bit depth.
    #[must_use]
    pub const fn bit_depth(&self) -> png::BitDepth {
        self.bit_depth
    }

    /// Returns the image color type.
    #[must_use]
    pub const fn color_type(&self) -> png::ColorType {
        self.color_type
    }

    /// Returns whether the PNG is interlaced.
    #[must_use]
    pub const fn interlaced(&self) -> bool {
        self.interlaced
    }

    /// Returns parsed Freedesktop thumbnail metadata.
    #[must_use]
    pub const fn metadata(&self) -> &ThumbnailMetadata {
        &self.metadata
    }

    fn conformance_problems(&self, size: ThumbnailSize) -> Vec<CacheEntryProblem> {
        let mut problems = Vec::new();
        if self.bit_depth != png::BitDepth::Eight
            || self.interlaced
            || !matches!(
                self.color_type,
                png::ColorType::Rgba | png::ColorType::GrayscaleAlpha
            )
        {
            push_problem(&mut problems, CacheEntryProblem::NonconformingPngFormat);
        }
        if self.width > size.max_dimension() || self.height > size.max_dimension() {
            push_problem(&mut problems, CacheEntryProblem::DimensionsExceedNamespace);
        }
        problems
    }
}

/// Validates a personal-cache successful thumbnail PNG against a readable original identity.
#[must_use]
pub fn validate_personal_thumbnail(
    bytes: &[u8],
    original: &OriginalIdentity,
    size: ThumbnailSize,
) -> ValidationOutcome {
    let parsed = match ParsedThumbnailPng::parse(bytes) {
        Ok(parsed) => parsed,
        Err(_) => {
            return ValidationOutcome::Invalid(vec![CacheEntryProblem::InvalidPngStructure]);
        }
    };

    let mut problems = parsed.conformance_problems(size);
    let metadata = parsed.metadata();

    match metadata.thumb_uri() {
        Some(uri) if uri == original.uri().as_str() => {}
        Some(_) => push_problem(&mut problems, CacheEntryProblem::StaleMetadata),
        None => push_problem(&mut problems, CacheEntryProblem::MissingRequiredMetadata),
    }

    match metadata.thumb_mtime_result() {
        Ok(Some(mtime)) if mtime == original.mtime().as_i64() => {}
        Ok(Some(_)) => push_problem(&mut problems, CacheEntryProblem::StaleMetadata),
        Ok(None) => push_problem(&mut problems, CacheEntryProblem::MissingRequiredMetadata),
        Err(_) => push_problem(&mut problems, CacheEntryProblem::InvalidMetadataSyntax),
    }

    compare_optional_size(&mut problems, metadata, original.size());
    compare_optional_mimetype(&mut problems, metadata, original.mime_type());

    if problems.is_empty() {
        ValidationOutcome::FullyVerified
    } else {
        ValidationOutcome::Invalid(problems)
    }
}

/// Validates a shared-repository successful thumbnail PNG against explicit shared context.
#[must_use]
pub fn validate_shared_thumbnail(
    bytes: &[u8],
    context: &SharedRepositoryContext,
    mtime: Option<UnixMTimeSeconds>,
    original_size: Option<u64>,
    size: ThumbnailSize,
) -> ValidationOutcome {
    let parsed = match ParsedThumbnailPng::parse(bytes) {
        Ok(parsed) => parsed,
        Err(_) => {
            return ValidationOutcome::Invalid(vec![CacheEntryProblem::InvalidPngStructure]);
        }
    };

    let mut problems = parsed.conformance_problems(size);
    let mut incomplete = false;
    let metadata = parsed.metadata();

    match metadata.thumb_uri() {
        Some(uri) if uri == context.shared_uri().as_str() => {}
        Some(_) => push_problem(&mut problems, CacheEntryProblem::StaleMetadata),
        None => incomplete = true,
    }

    match (metadata.thumb_mtime_result(), mtime) {
        (Ok(Some(stored)), Some(expected)) if stored == expected.as_i64() => {}
        (Ok(Some(_)), Some(_)) => push_problem(&mut problems, CacheEntryProblem::StaleMetadata),
        (Ok(Some(_)), None) => push_problem(&mut problems, CacheEntryProblem::UnverifiableOriginal),
        (Ok(None), _) => incomplete = true,
        (Err(_), _) => push_problem(&mut problems, CacheEntryProblem::InvalidMetadataSyntax),
    }

    compare_optional_size(&mut problems, metadata, original_size);

    if !problems.is_empty() {
        ValidationOutcome::Invalid(problems)
    } else if incomplete {
        ValidationOutcome::SharedMetadataIncomplete
    } else {
        ValidationOutcome::FullyVerified
    }
}

fn compare_optional_size(
    problems: &mut Vec<CacheEntryProblem>,
    metadata: &ThumbnailMetadata,
    expected: Option<u64>,
) {
    match (metadata.thumb_size_result(), expected) {
        (Ok(Some(stored)), Some(expected)) if stored == expected => {}
        (Ok(Some(_)), Some(_)) => push_problem(problems, CacheEntryProblem::StaleMetadata),
        (Ok(_), _) => {}
        (Err(_), _) => push_problem(problems, CacheEntryProblem::InvalidMetadataSyntax),
    }
}

fn compare_optional_mimetype(
    problems: &mut Vec<CacheEntryProblem>,
    metadata: &ThumbnailMetadata,
    expected: Option<&str>,
) {
    if let (Some(stored), Some(expected)) = (metadata.thumb_mimetype(), expected) {
        if stored != expected {
            push_problem(problems, CacheEntryProblem::StaleMetadata);
        }
    }
}

fn push_problem(problems: &mut Vec<CacheEntryProblem>, problem: CacheEntryProblem) {
    if !problems.contains(&problem) {
        problems.push(problem);
    }
}

fn inspect_cache_entry(
    path: PathBuf,
    namespace: CacheNamespace,
    handle: CacheEntryHandle,
    successful_size: Option<ThumbnailSize>,
) -> CacheEntryInspection {
    let mut timestamps = thumbnail_timestamps(&path, AccessTimePreservation::NotNeeded);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(_) => {
            return CacheEntryInspection {
                outcome: ValidationOutcome::Invalid(vec![CacheEntryProblem::UnreadableOriginal]),
                original_uri: None,
                timestamps,
                namespace,
                path,
                handle,
            };
        }
    };

    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return CacheEntryInspection {
            outcome: ValidationOutcome::Invalid(vec![CacheEntryProblem::UnreadableOriginal]),
            original_uri: None,
            timestamps,
            namespace,
            path,
            handle,
        };
    }

    timestamps = thumbnail_timestamps(&path, AccessTimePreservation::NotPreserved);
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(_) => {
            return CacheEntryInspection {
                outcome: ValidationOutcome::Invalid(vec![CacheEntryProblem::UnreadableOriginal]),
                original_uri: None,
                timestamps,
                namespace,
                path,
                handle,
            };
        }
    };

    let parsed = match ParsedThumbnailPng::parse(&bytes) {
        Ok(parsed) => parsed,
        Err(_) => {
            return CacheEntryInspection {
                outcome: ValidationOutcome::Invalid(vec![CacheEntryProblem::InvalidPngStructure]),
                original_uri: None,
                timestamps,
                namespace,
                path,
                handle,
            };
        }
    };

    let mut problems =
        successful_size.map_or_else(Vec::new, |size| parsed.conformance_problems(size));
    inspect_required_metadata(&mut problems, parsed.metadata());
    let original_uri = parsed.metadata().thumb_uri().and_then(parse_thumbnail_uri);
    let outcome = if problems.is_empty() {
        ValidationOutcome::UncheckedInspection
    } else {
        ValidationOutcome::Invalid(problems)
    };

    CacheEntryInspection {
        outcome,
        original_uri,
        timestamps,
        namespace,
        path,
        handle,
    }
}

fn inspect_required_metadata(problems: &mut Vec<CacheEntryProblem>, metadata: &ThumbnailMetadata) {
    if metadata.thumb_uri().is_none() {
        push_problem(problems, CacheEntryProblem::MissingRequiredMetadata);
    }
    match metadata.thumb_mtime_result() {
        Ok(Some(_)) => {}
        Ok(None) => push_problem(problems, CacheEntryProblem::MissingRequiredMetadata),
        Err(_) => push_problem(problems, CacheEntryProblem::InvalidMetadataSyntax),
    }
    if metadata.thumb_size_result().is_err() {
        push_problem(problems, CacheEntryProblem::InvalidMetadataSyntax);
    }
}

fn parse_thumbnail_uri(uri: &str) -> Option<ThumbnailUriIdentity> {
    if uri.starts_with("./") {
        SharedRelativeThumbnailUri::parse(uri)
            .ok()
            .map(ThumbnailUriIdentity::Shared)
    } else {
        PersonalThumbnailUri::from_absolute_uri(uri)
            .ok()
            .map(ThumbnailUriIdentity::Personal)
    }
}

fn thumbnail_timestamps(path: &Path, preservation: AccessTimePreservation) -> ThumbnailTimestamps {
    let (accessed_at, modified_at) = fs::symlink_metadata(path).map_or((None, None), |metadata| {
        (metadata.accessed().ok(), metadata.modified().ok())
    });
    ThumbnailTimestamps {
        accessed_at,
        modified_at,
        access_time_preserved_during_inspection: preservation,
    }
}

fn is_standard_thumbnail_filename(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".png") else {
        return false;
    };
    stem.len() == 32
        && stem
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

struct RgbaImage {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

fn normalized_personal_thumbnail_png(
    rendered_png: &[u8],
    original: &OriginalIdentity,
    size: ThumbnailSize,
) -> Result<Vec<u8>> {
    let image = decode_rendered_png_to_rgba8(rendered_png)?;
    let image = downscale_to_namespace(image, size)?;
    let metadata = thumbnail_metadata_pairs(original);
    let png = encode_rgba_png(image.width, image.height, &image.pixels, &metadata)?;
    match validate_personal_thumbnail(&png, original, size) {
        ValidationOutcome::FullyVerified => Ok(png),
        ValidationOutcome::Invalid(problems) => Err(ThumbnailError::UnsupportedRenderedThumbnail(
            rendered_validation_error(problems.as_slice()),
        )),
        ValidationOutcome::SharedMetadataIncomplete | ValidationOutcome::UncheckedInspection => {
            Err(ThumbnailError::UnsupportedRenderedThumbnail(
                "normalized thumbnail could not be verified",
            ))
        }
    }
}

fn rendered_validation_error(problems: &[CacheEntryProblem]) -> &'static str {
    if problems.contains(&CacheEntryProblem::DimensionsExceedNamespace) {
        "dimensions exceed namespace"
    } else if problems.contains(&CacheEntryProblem::NonconformingPngFormat) {
        "nonconforming final PNG"
    } else if problems.contains(&CacheEntryProblem::InvalidPngStructure) {
        "invalid final PNG structure"
    } else {
        "metadata validation failed"
    }
}

fn thumbnail_metadata_pairs(original: &OriginalIdentity) -> Vec<(String, String)> {
    let mut metadata = vec![
        ("Thumb::URI".to_owned(), original.uri().as_str().to_owned()),
        ("Thumb::MTime".to_owned(), original.mtime().to_string()),
    ];
    if let Some(size) = original.size() {
        metadata.push(("Thumb::Size".to_owned(), size.to_string()));
    }
    if let Some(mime_type) = original.mime_type() {
        metadata.push(("Thumb::Mimetype".to_owned(), mime_type.to_owned()));
    }
    metadata
}

fn decode_rendered_png_to_rgba8(bytes: &[u8]) -> Result<RgbaImage> {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(
        png::Transformations::EXPAND | png::Transformations::STRIP_16 | png::Transformations::ALPHA,
    );
    let mut reader = decoder
        .read_info()
        .map_err(|err| ThumbnailError::Png(err.to_string()))?;
    let Some(output_buffer_size) = reader.output_buffer_size() else {
        return Err(ThumbnailError::Png(
            "png output buffer size is unavailable".to_owned(),
        ));
    };
    let mut buffer = vec![0; output_buffer_size];
    let output = reader
        .next_frame(&mut buffer)
        .map_err(|err| ThumbnailError::Png(err.to_string()))?;
    let frame = &buffer[..output.buffer_size()];
    if output.bit_depth != png::BitDepth::Eight {
        return Err(ThumbnailError::UnsupportedRenderedThumbnail(
            "decoded PNG did not normalize to 8-bit samples",
        ));
    }

    let pixels = match output.color_type {
        png::ColorType::Rgba => frame.to_vec(),
        png::ColorType::Rgb => {
            let mut out = Vec::with_capacity(output.width as usize * output.height as usize * 4);
            for pixel in frame.chunks_exact(3) {
                out.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]);
            }
            out
        }
        png::ColorType::GrayscaleAlpha => {
            let mut out = Vec::with_capacity(output.width as usize * output.height as usize * 4);
            for pixel in frame.chunks_exact(2) {
                out.extend_from_slice(&[pixel[0], pixel[0], pixel[0], pixel[1]]);
            }
            out
        }
        png::ColorType::Grayscale | png::ColorType::Indexed => {
            let mut out = Vec::with_capacity(output.width as usize * output.height as usize * 4);
            for &gray in frame {
                out.extend_from_slice(&[gray, gray, gray, 255]);
            }
            out
        }
    };

    Ok(RgbaImage {
        width: output.width,
        height: output.height,
        pixels,
    })
}

fn downscale_to_namespace(image: RgbaImage, size: ThumbnailSize) -> Result<RgbaImage> {
    let max = size.max_dimension();
    if image.width <= max && image.height <= max {
        return Ok(image);
    }
    let (width, height) = constrain_dimensions(image.width, image.height, max);
    let source = image
        .pixels
        .chunks_exact(4)
        .map(|pixel| resize::px::RGBA::new(pixel[0], pixel[1], pixel[2], pixel[3]))
        .collect::<Vec<_>>();
    let mut dest = vec![resize::px::RGBA::new(0, 0, 0, 0); width as usize * height as usize];
    let mut resizer = resize::new(
        image.width as usize,
        image.height as usize,
        width as usize,
        height as usize,
        resize::Pixel::RGBA8P,
        resize::Type::Lanczos3,
    )
    .map_err(|_| ThumbnailError::UnsupportedRenderedThumbnail("resize setup failed"))?;
    resizer
        .resize(&source, &mut dest)
        .map_err(|_| ThumbnailError::UnsupportedRenderedThumbnail("resize failed"))?;
    let mut pixels = Vec::with_capacity(dest.len() * 4);
    for pixel in dest {
        pixels.extend_from_slice(&[pixel.r, pixel.g, pixel.b, pixel.a]);
    }
    Ok(RgbaImage {
        width,
        height,
        pixels,
    })
}

fn constrain_dimensions(width: u32, height: u32, max: u32) -> (u32, u32) {
    if width >= height {
        let scaled_height = (u64::from(height) * u64::from(max) / u64::from(width)).max(1) as u32;
        (max, scaled_height)
    } else {
        let scaled_width = (u64::from(width) * u64::from(max) / u64::from(height)).max(1) as u32;
        (scaled_width, max)
    }
}

fn encode_rgba_png(
    width: u32,
    height: u32,
    pixels: &[u8],
    metadata: &[(String, String)],
) -> Result<Vec<u8>> {
    let expected_len = width as usize * height as usize * 4;
    if pixels.len() != expected_len {
        return Err(ThumbnailError::UnsupportedRenderedThumbnail(
            "RGBA buffer length does not match dimensions",
        ));
    }
    let mut output = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut output, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        for (key, value) in metadata {
            encoder
                .add_text_chunk(key.clone(), value.clone())
                .map_err(|err| ThumbnailError::Png(err.to_string()))?;
        }
        let mut writer = encoder
            .write_header()
            .map_err(|err| ThumbnailError::Png(err.to_string()))?;
        writer
            .write_image_data(pixels)
            .map_err(|err| ThumbnailError::Png(err.to_string()))?;
    }
    Ok(output)
}

#[cfg(unix)]
fn ensure_private_directory(path: &Path) -> Result<()> {
    match fs::metadata(path) {
        Ok(metadata) => {
            if !metadata.is_dir()
                || metadata.uid() != rustix::process::getuid().as_raw()
                || metadata.permissions().mode() & 0o077 != 0
            {
                return Err(ThumbnailError::InsecureCacheDirectory(path.to_owned()));
            }
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|source| ThumbnailError::Io {
                    context: "create parent thumbnail cache directories",
                    source,
                })?;
            }
            fs::create_dir(path).map_err(|source| ThumbnailError::Io {
                context: "create thumbnail cache directory",
                source,
            })?;
            fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|source| {
                ThumbnailError::Io {
                    context: "set thumbnail cache directory permissions",
                    source,
                }
            })?;
            Ok(())
        }
        Err(source) => Err(ThumbnailError::Io {
            context: "inspect thumbnail cache directory",
            source,
        }),
    }
}

#[cfg(not(unix))]
fn ensure_private_directory(_path: &Path) -> Result<()> {
    Err(ThumbnailError::UnsupportedPlatform)
}

/// A standard Freedesktop thumbnail size directory.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ThumbnailSize {
    /// 128px thumbnail cache directory.
    Normal,
    /// 256px thumbnail cache directory.
    Large,
    /// 512px thumbnail cache directory.
    XLarge,
    /// 1024px thumbnail cache directory.
    XxLarge,
}

impl ThumbnailSize {
    /// Returns the standard cache directory name for this size.
    #[must_use]
    pub const fn directory_name(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Large => "large",
            Self::XLarge => "x-large",
            Self::XxLarge => "xx-large",
        }
    }

    /// Returns the maximum width and height for this namespace in pixels.
    #[must_use]
    pub const fn max_dimension(self) -> u32 {
        match self {
            Self::Normal => 128,
            Self::Large => 256,
            Self::XLarge => 512,
            Self::XxLarge => 1024,
        }
    }

    /// Returns all standard thumbnail sizes in cache scan order.
    #[must_use]
    pub const fn all() -> [Self; 4] {
        [Self::Normal, Self::Large, Self::XLarge, Self::XxLarge]
    }
}

fn validate_mime_type(mime_type: &str) -> Result<()> {
    if mime_type.is_empty()
        || !mime_type.is_ascii()
        || mime_type.bytes().any(|byte| byte.is_ascii_control())
        || !mime_type.contains('/')
    {
        return Err(ThumbnailError::InvalidMetadata("invalid MIME type"));
    }
    Ok(())
}

/// Validation state for a thumbnail cache entry.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum CacheEntryState {
    /// The thumbnail metadata still matches the original.
    Valid,
    /// The original local file no longer exists.
    OriginalMissing,
    /// The original exists but metadata no longer matches.
    Outdated,
    /// The original could not be read well enough to validate the entry.
    UnreadableOriginal,
    /// The original cannot be verified through the available validation context.
    UnverifiableOriginal,
    /// The thumbnail file or metadata is malformed.
    Malformed,
}

#[cfg(test)]
mod tests {
    use super::ThumbnailSize;

    #[test]
    fn thumbnail_size_directory_names_match_standard() {
        assert_eq!(ThumbnailSize::Normal.directory_name(), "normal");
        assert_eq!(ThumbnailSize::Large.directory_name(), "large");
        assert_eq!(ThumbnailSize::XLarge.directory_name(), "x-large");
        assert_eq!(ThumbnailSize::XxLarge.directory_name(), "xx-large");
    }

    #[test]
    fn all_thumbnail_sizes_are_in_scan_order() {
        let names = ThumbnailSize::all().map(ThumbnailSize::directory_name);

        assert_eq!(names, ["normal", "large", "x-large", "xx-large"]);
    }
}
