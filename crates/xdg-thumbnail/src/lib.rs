// SPDX-FileCopyrightText: 2026 KIM Hyunjae
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Freedesktop thumbnail cache primitives.

mod error;
mod identity;
mod inspection;
mod namespace;
mod png;
mod uri;

use std::ffi::OsStr;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, PermissionsExt};

pub use error::{Result, ThumbnailError};
pub use identity::{
    OriginalIdentity, ReadableOriginalIdentity, SharedRepositoryContext, UnixMTimeSeconds,
};
pub use inspection::{
    AccessTimePreservation, CacheEntryHandle, CacheEntryInspection, CacheEntryState,
    ThumbnailTimestamps, ThumbnailUriIdentity,
};
pub use namespace::{CacheNamespace, FailureNamespace, ThumbnailSize};
pub use png::{
    CacheEntryProblem, ParsedThumbnailPng, ThumbnailMetadata, ValidationOutcome,
    validate_personal_failure_entry, validate_personal_thumbnail, validate_shared_thumbnail,
};
pub use uri::{PersonalThumbnailUri, SharedRelativeThumbnailUri};

pub(crate) use png::{
    encode_rgba_png, normalized_personal_thumbnail_png, push_problem, thumbnail_metadata_pairs,
    validate_mime_type,
};

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
    /// The original identity must have already been confirmed readable. The candidate PNG is
    /// opened and validated before this method returns. Callers that reopen the returned path
    /// accept that another process may replace it after validation.
    pub fn validated_personal_path(
        &self,
        original: &ReadableOriginalIdentity,
        size: ThumbnailSize,
    ) -> Result<ThumbnailLookup<ValidatedThumbnailPath>> {
        let original = original.identity();
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
                    metadata: parsed.into_metadata(),
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
    ///
    /// The original identity must have already been confirmed readable.
    pub fn validated_personal_payload(
        &self,
        original: &ReadableOriginalIdentity,
        size: ThumbnailSize,
    ) -> Result<ThumbnailLookup<ValidatedThumbnailPayload>> {
        let original = original.identity();
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
                    metadata: parsed.into_metadata(),
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

#[cfg(unix)]
fn ensure_private_directory(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink()
                || !metadata.is_dir()
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
