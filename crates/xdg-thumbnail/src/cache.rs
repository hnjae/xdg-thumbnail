// SPDX-FileCopyrightText: 2026 KIM Hyunjae
// SPDX-License-Identifier: MPL-2.0

use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, PermissionsExt};

use crate::PersonalOriginalUri;
use crate::inspection::{
    CacheEntryInspection, read_thumbnail_for_inspection, thumbnail_timestamps,
    thumbnail_timestamps_from_metadata,
};
use crate::{
    AccessTimePreservation, CacheEntryProblem, CacheNamespace, FailureNamespace,
    OwnedRawThumbnailImage, ParsedThumbnailPng, PersonalValidationOutcome, RawThumbnailImage,
    ReadableOriginalIdentity, Result, SharedRelativeOriginalUri, SharedRepositoryContext,
    SharedValidationOutcome, ThumbnailError, ThumbnailMetadata, ThumbnailSize, ThumbnailTimestamps,
    UnixMTimeSeconds, encode_rgba_png, normalized_personal_thumbnail_png,
    normalized_personal_thumbnail_raw_png, thumbnail_metadata_pairs, validate_personal_thumbnail,
    validate_shared_thumbnail,
};

/// Root directory of the personal thumbnail cache, usually `$XDG_CACHE_HOME/thumbnails`.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct PersonalCacheRoot {
    path: PathBuf,
}

impl PersonalCacheRoot {
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

    /// Returns the resolved thumbnail root path.
    #[must_use]
    pub fn as_path(&self) -> &Path {
        &self.path
    }

    /// Computes the personal-cache path for an accepted URI and namespace without reading it.
    #[must_use]
    pub fn personal_path(&self, uri: &PersonalOriginalUri, namespace: &CacheNamespace) -> PathBuf {
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
    ) -> Result<PersonalThumbnailLookup<ThumbnailPathLookupEntry>> {
        match self.validated_personal_entry(original, size)? {
            PersonalThumbnailLookup::Valid(entry) => {
                Ok(PersonalThumbnailLookup::Valid(ThumbnailPathLookupEntry {
                    path: entry.path,
                    metadata: entry.metadata,
                }))
            }
            PersonalThumbnailLookup::Missing => Ok(PersonalThumbnailLookup::Missing),
            PersonalThumbnailLookup::Invalid(problems) => {
                Ok(PersonalThumbnailLookup::Invalid(problems))
            }
        }
    }

    /// Returns exact validated PNG bytes from the personal thumbnail cache.
    ///
    /// The original identity must have already been confirmed readable.
    pub fn validated_personal_bytes(
        &self,
        original: &ReadableOriginalIdentity,
        size: ThumbnailSize,
    ) -> Result<PersonalThumbnailLookup<ThumbnailBytesLookupEntry>> {
        match self.validated_personal_entry(original, size)? {
            PersonalThumbnailLookup::Valid(entry) => {
                Ok(PersonalThumbnailLookup::Valid(ThumbnailBytesLookupEntry {
                    path: entry.path,
                    bytes: entry.bytes,
                    metadata: entry.metadata,
                }))
            }
            PersonalThumbnailLookup::Missing => Ok(PersonalThumbnailLookup::Missing),
            PersonalThumbnailLookup::Invalid(problems) => {
                Ok(PersonalThumbnailLookup::Invalid(problems))
            }
        }
    }

    /// Normalizes rendered PNG data, atomically installs a personal-cache thumbnail, and returns its path.
    pub fn install_personal_thumbnail_path(
        &self,
        original: &ReadableOriginalIdentity,
        size: ThumbnailSize,
        rendered_png: &[u8],
    ) -> Result<InstalledThumbnailPath> {
        let (path, _) = self.install_personal_thumbnail_entry(original, size, rendered_png)?;
        Ok(InstalledThumbnailPath { path })
    }

    /// Normalizes rendered PNG data, atomically installs a personal-cache thumbnail, and returns final bytes.
    pub fn install_personal_thumbnail_bytes(
        &self,
        original: &ReadableOriginalIdentity,
        size: ThumbnailSize,
        rendered_png: &[u8],
    ) -> Result<InstalledThumbnailBytes> {
        let (path, bytes) = self.install_personal_thumbnail_entry(original, size, rendered_png)?;
        Ok(InstalledThumbnailBytes { path, bytes })
    }

    /// Normalizes raw pixel data, atomically installs a personal-cache thumbnail, and returns its path.
    pub fn install_personal_thumbnail_raw_path(
        &self,
        original: &ReadableOriginalIdentity,
        size: ThumbnailSize,
        image: RawThumbnailImage<'_>,
    ) -> Result<InstalledThumbnailPath> {
        let (path, _) = self.install_personal_thumbnail_raw_entry(original, size, image)?;
        Ok(InstalledThumbnailPath { path })
    }

    /// Normalizes raw pixel data, atomically installs a personal-cache thumbnail, and returns final bytes.
    pub fn install_personal_thumbnail_raw_bytes(
        &self,
        original: &ReadableOriginalIdentity,
        size: ThumbnailSize,
        image: RawThumbnailImage<'_>,
    ) -> Result<InstalledThumbnailBytes> {
        let (path, bytes) = self.install_personal_thumbnail_raw_entry(original, size, image)?;
        Ok(InstalledThumbnailBytes { path, bytes })
    }

    fn install_personal_thumbnail_entry(
        &self,
        original: &ReadableOriginalIdentity,
        size: ThumbnailSize,
        rendered_png: &[u8],
    ) -> Result<(PathBuf, Vec<u8>)> {
        let namespace = CacheNamespace::Size(size);
        let path = self.personal_path(original.identity().uri(), &namespace);
        let bytes = normalized_personal_thumbnail_png(rendered_png, original.identity(), size)?;
        self.write_personal_entry(&path, &namespace, &bytes)?;
        Ok((path, bytes))
    }

    fn install_personal_thumbnail_raw_entry(
        &self,
        original: &ReadableOriginalIdentity,
        size: ThumbnailSize,
        image: RawThumbnailImage<'_>,
    ) -> Result<(PathBuf, Vec<u8>)> {
        let namespace = CacheNamespace::Size(size);
        let path = self.personal_path(original.identity().uri(), &namespace);
        let bytes = normalized_personal_thumbnail_raw_png(image, original.identity(), size)?;
        self.write_personal_entry(&path, &namespace, &bytes)?;
        Ok((path, bytes))
    }

    /// Writes a deterministic 1x1 transparent failure entry and returns its path.
    pub fn write_failure_entry_path(
        &self,
        namespace: &FailureNamespace,
        original: &ReadableOriginalIdentity,
    ) -> Result<InstalledThumbnailPath> {
        let (path, _) = self.write_failure_entry_bytes_inner(namespace, original)?;
        Ok(InstalledThumbnailPath { path })
    }

    /// Writes a deterministic 1x1 transparent failure entry and returns final bytes.
    pub fn write_failure_entry_bytes(
        &self,
        namespace: &FailureNamespace,
        original: &ReadableOriginalIdentity,
    ) -> Result<InstalledThumbnailBytes> {
        let (path, bytes) = self.write_failure_entry_bytes_inner(namespace, original)?;
        Ok(InstalledThumbnailBytes { path, bytes })
    }

    fn write_failure_entry_bytes_inner(
        &self,
        namespace: &FailureNamespace,
        original: &ReadableOriginalIdentity,
    ) -> Result<(PathBuf, Vec<u8>)> {
        let namespace = CacheNamespace::Failure(namespace.clone());
        let path = self.personal_path(original.identity().uri(), &namespace);
        let bytes = encode_rgba_png(
            1,
            1,
            &[0, 0, 0, 0],
            &thumbnail_metadata_pairs(original.identity()),
        )?;
        self.write_personal_entry(&path, &namespace, &bytes)?;
        Ok((path, bytes))
    }

    fn validated_personal_entry(
        &self,
        original: &ReadableOriginalIdentity,
        size: ThumbnailSize,
    ) -> Result<PersonalThumbnailLookup<ValidatedPersonalEntry>> {
        let path = self.personal_path(original.identity().uri(), &CacheNamespace::Size(size));
        let bytes = match read_cache_entry_no_follow(&path, "read thumbnail cache entry")? {
            CacheEntryRead::Bytes(bytes) => bytes,
            CacheEntryRead::Missing => return Ok(PersonalThumbnailLookup::Missing),
            CacheEntryRead::Unreadable => {
                return Ok(PersonalThumbnailLookup::Invalid(vec![
                    CacheEntryProblem::UnreadableEntry,
                ]));
            }
        };

        match validate_personal_thumbnail(&bytes, original, size) {
            PersonalValidationOutcome::FullyVerified => {
                let parsed = ParsedThumbnailPng::parse(&bytes)?;
                Ok(PersonalThumbnailLookup::Valid(ValidatedPersonalEntry {
                    path,
                    bytes,
                    metadata: parsed.into_metadata(),
                }))
            }
            PersonalValidationOutcome::Invalid(problems) => {
                Ok(PersonalThumbnailLookup::Invalid(problems))
            }
        }
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

impl SharedRepositoryContext {
    /// Returns a validated shared-repository path for integrations that must pass a filename.
    pub fn lookup_thumbnail_path(
        &self,
        size: ThumbnailSize,
        metadata_policy: SharedThumbnailMetadataPolicy,
        mtime: Option<UnixMTimeSeconds>,
        original_byte_size: Option<u64>,
    ) -> Result<SharedThumbnailLookup<ThumbnailPathLookupEntry>> {
        match self.lookup_thumbnail_entry(size, metadata_policy, mtime, original_byte_size)? {
            SharedThumbnailLookup::FullyVerified(entry) => Ok(
                SharedThumbnailLookup::FullyVerified(ThumbnailPathLookupEntry {
                    path: entry.path,
                    metadata: entry.metadata,
                }),
            ),
            SharedThumbnailLookup::MetadataIncomplete(entry) => Ok(
                SharedThumbnailLookup::MetadataIncomplete(ThumbnailPathLookupEntry {
                    path: entry.path,
                    metadata: entry.metadata,
                }),
            ),
            SharedThumbnailLookup::Missing => Ok(SharedThumbnailLookup::Missing),
            SharedThumbnailLookup::Invalid(problems) => {
                Ok(SharedThumbnailLookup::Invalid(problems))
            }
            SharedThumbnailLookup::Unverifiable(problems) => {
                Ok(SharedThumbnailLookup::Unverifiable(problems))
            }
        }
    }

    /// Returns exact validated PNG bytes from a shared thumbnail repository.
    pub fn lookup_thumbnail_bytes(
        &self,
        size: ThumbnailSize,
        metadata_policy: SharedThumbnailMetadataPolicy,
        mtime: Option<UnixMTimeSeconds>,
        original_byte_size: Option<u64>,
    ) -> Result<SharedThumbnailLookup<ThumbnailBytesLookupEntry>> {
        match self.lookup_thumbnail_entry(size, metadata_policy, mtime, original_byte_size)? {
            SharedThumbnailLookup::FullyVerified(entry) => Ok(
                SharedThumbnailLookup::FullyVerified(ThumbnailBytesLookupEntry {
                    path: entry.path,
                    bytes: entry.bytes,
                    metadata: entry.metadata,
                }),
            ),
            SharedThumbnailLookup::MetadataIncomplete(entry) => Ok(
                SharedThumbnailLookup::MetadataIncomplete(ThumbnailBytesLookupEntry {
                    path: entry.path,
                    bytes: entry.bytes,
                    metadata: entry.metadata,
                }),
            ),
            SharedThumbnailLookup::Missing => Ok(SharedThumbnailLookup::Missing),
            SharedThumbnailLookup::Invalid(problems) => {
                Ok(SharedThumbnailLookup::Invalid(problems))
            }
            SharedThumbnailLookup::Unverifiable(problems) => {
                Ok(SharedThumbnailLookup::Unverifiable(problems))
            }
        }
    }

    /// Inspects existing shared-repository thumbnails without exposing removal handles.
    pub fn inspect_thumbnails(
        &self,
        sizes: &[ThumbnailSize],
        mtime: Option<UnixMTimeSeconds>,
        original_byte_size: Option<u64>,
    ) -> Result<Vec<SharedCacheEntryInspection>> {
        let mut inspections = Vec::new();
        for &size in sizes {
            if let Some(inspection) = self.inspect_thumbnail(size, mtime, original_byte_size)? {
                inspections.push(inspection);
            }
        }
        Ok(inspections)
    }

    fn lookup_thumbnail_entry(
        &self,
        size: ThumbnailSize,
        metadata_policy: SharedThumbnailMetadataPolicy,
        mtime: Option<UnixMTimeSeconds>,
        original_byte_size: Option<u64>,
    ) -> Result<SharedThumbnailLookup<ValidatedSharedEntry>> {
        let path = self.thumbnail_path(size);
        let bytes = match read_cache_entry_no_follow(&path, "read shared thumbnail cache entry")? {
            CacheEntryRead::Bytes(bytes) => bytes,
            CacheEntryRead::Missing => return Ok(SharedThumbnailLookup::Missing),
            CacheEntryRead::Unreadable => {
                return Ok(SharedThumbnailLookup::Invalid(vec![
                    CacheEntryProblem::UnreadableEntry,
                ]));
            }
        };

        match validate_shared_thumbnail(&bytes, self, mtime, original_byte_size, size) {
            SharedValidationOutcome::FullyVerified => {
                let parsed = ParsedThumbnailPng::parse(&bytes)?;
                Ok(SharedThumbnailLookup::FullyVerified(ValidatedSharedEntry {
                    path,
                    bytes,
                    metadata: parsed.into_metadata(),
                }))
            }
            SharedValidationOutcome::MetadataIncomplete => {
                if metadata_policy == SharedThumbnailMetadataPolicy::RequireComplete {
                    Ok(SharedThumbnailLookup::Invalid(vec![
                        CacheEntryProblem::MissingRequiredMetadata,
                    ]))
                } else {
                    let parsed = ParsedThumbnailPng::parse(&bytes)?;
                    Ok(SharedThumbnailLookup::MetadataIncomplete(
                        ValidatedSharedEntry {
                            path,
                            bytes,
                            metadata: parsed.into_metadata(),
                        },
                    ))
                }
            }
            SharedValidationOutcome::Invalid(problems) if only_unverifiable_original(&problems) => {
                Ok(SharedThumbnailLookup::Unverifiable(problems))
            }
            SharedValidationOutcome::Invalid(problems) => {
                Ok(SharedThumbnailLookup::Invalid(problems))
            }
        }
    }

    fn inspect_thumbnail(
        &self,
        size: ThumbnailSize,
        mtime: Option<UnixMTimeSeconds>,
        original_byte_size: Option<u64>,
    ) -> Result<Option<SharedCacheEntryInspection>> {
        let path = self.thumbnail_path(size);
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => {
                return Ok(Some(SharedCacheEntryInspection {
                    outcome: SharedCacheEntryOutcome::Invalid(vec![
                        CacheEntryProblem::UnreadableEntry,
                    ]),
                    shared_uri: self.shared_uri().clone(),
                    timestamps: thumbnail_timestamps(&path, AccessTimePreservation::NotNeeded),
                    size,
                    path,
                    metadata: None,
                }));
            }
        };

        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Ok(Some(SharedCacheEntryInspection {
                outcome: SharedCacheEntryOutcome::Invalid(vec![CacheEntryProblem::UnreadableEntry]),
                shared_uri: self.shared_uri().clone(),
                timestamps: thumbnail_timestamps_from_metadata(
                    &metadata,
                    AccessTimePreservation::NotNeeded,
                ),
                size,
                path,
                metadata: None,
            }));
        }

        let (read_result, preservation) = read_thumbnail_for_inspection(&path);
        let timestamps = thumbnail_timestamps_from_metadata(&metadata, preservation);
        let bytes = match read_result {
            Ok(bytes) => bytes,
            Err(_) => {
                return Ok(Some(SharedCacheEntryInspection {
                    outcome: SharedCacheEntryOutcome::Invalid(vec![
                        CacheEntryProblem::UnreadableEntry,
                    ]),
                    shared_uri: self.shared_uri().clone(),
                    timestamps,
                    size,
                    path,
                    metadata: None,
                }));
            }
        };

        let parsed = ParsedThumbnailPng::parse(&bytes).ok();
        let outcome = shared_cache_entry_outcome(validate_shared_thumbnail(
            &bytes,
            self,
            mtime,
            original_byte_size,
            size,
        ));
        Ok(Some(SharedCacheEntryInspection {
            outcome,
            shared_uri: self.shared_uri().clone(),
            timestamps,
            size,
            path,
            metadata: parsed.map(ParsedThumbnailPng::into_metadata),
        }))
    }
}

/// Owned personal-cache lookup request for async or runtime-specific adapters.
///
/// Constructing this request does not perform filesystem I/O. Validation happens only when
/// [`Self::validated_path`] or [`Self::validated_bytes`] is called.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PersonalThumbnailLookupRequest {
    root: PersonalCacheRoot,
    original: ReadableOriginalIdentity,
    size: ThumbnailSize,
}

impl PersonalThumbnailLookupRequest {
    /// Creates an owned personal-cache lookup request.
    #[must_use]
    pub const fn new(
        root: PersonalCacheRoot,
        original: ReadableOriginalIdentity,
        size: ThumbnailSize,
    ) -> Self {
        Self {
            root,
            original,
            size,
        }
    }

    /// Returns a validated personal-cache path for the owned request.
    pub fn validated_path(self) -> Result<PersonalThumbnailLookup<ThumbnailPathLookupEntry>> {
        let Self {
            root,
            original,
            size,
        } = self;
        root.validated_personal_path(&original, size)
    }

    /// Returns exact validated personal-cache PNG bytes for the owned request.
    pub fn validated_bytes(self) -> Result<PersonalThumbnailLookup<ThumbnailBytesLookupEntry>> {
        let Self {
            root,
            original,
            size,
        } = self;
        root.validated_personal_bytes(&original, size)
    }

    /// Splits this request into its owned parts.
    #[must_use]
    pub fn into_parts(self) -> (PersonalCacheRoot, ReadableOriginalIdentity, ThumbnailSize) {
        (self.root, self.original, self.size)
    }
}

/// Owned personal-cache install request for async or runtime-specific adapters.
///
/// Constructing this request does not perform filesystem I/O. Normalization and installation happen
/// only when [`Self::install_path`] or [`Self::install_bytes`] is called.
///
/// ```ignore
/// let request = PersonalThumbnailInstallRequest::new(root, original, size, rendered_png);
///
/// let installed = tokio::task::spawn_blocking(move || request.install_bytes())
///     .await
///     .expect("blocking thumbnail task panicked")?;
/// ```
#[derive(Debug, Eq, PartialEq)]
pub struct PersonalThumbnailInstallRequest {
    root: PersonalCacheRoot,
    original: ReadableOriginalIdentity,
    size: ThumbnailSize,
    rendered_png: Vec<u8>,
}

impl PersonalThumbnailInstallRequest {
    /// Creates an owned personal-cache install request.
    #[must_use]
    pub fn new(
        root: PersonalCacheRoot,
        original: ReadableOriginalIdentity,
        size: ThumbnailSize,
        rendered_png: Vec<u8>,
    ) -> Self {
        Self {
            root,
            original,
            size,
            rendered_png,
        }
    }

    /// Normalizes rendered PNG data, installs a personal-cache thumbnail, and returns its path.
    pub fn install_path(self) -> Result<InstalledThumbnailPath> {
        let Self {
            root,
            original,
            size,
            rendered_png,
        } = self;
        root.install_personal_thumbnail_path(&original, size, &rendered_png)
    }

    /// Normalizes rendered PNG data, installs a personal-cache thumbnail, and returns final bytes.
    pub fn install_bytes(self) -> Result<InstalledThumbnailBytes> {
        let Self {
            root,
            original,
            size,
            rendered_png,
        } = self;
        root.install_personal_thumbnail_bytes(&original, size, &rendered_png)
    }

    /// Splits this request into its owned parts.
    #[must_use]
    pub fn into_parts(
        self,
    ) -> (
        PersonalCacheRoot,
        ReadableOriginalIdentity,
        ThumbnailSize,
        Vec<u8>,
    ) {
        (self.root, self.original, self.size, self.rendered_png)
    }
}

/// Owned personal-cache raw install request for async or runtime-specific adapters.
///
/// Constructing this request does not perform filesystem I/O. Raw conversion, normalization, and
/// installation happen only when [`Self::install_path`] or [`Self::install_bytes`] is called.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PersonalThumbnailRawInstallRequest {
    root: PersonalCacheRoot,
    original: ReadableOriginalIdentity,
    size: ThumbnailSize,
    image: OwnedRawThumbnailImage,
}

impl PersonalThumbnailRawInstallRequest {
    /// Creates an owned personal-cache raw install request.
    #[must_use]
    pub fn new(
        root: PersonalCacheRoot,
        original: ReadableOriginalIdentity,
        size: ThumbnailSize,
        image: OwnedRawThumbnailImage,
    ) -> Self {
        Self {
            root,
            original,
            size,
            image,
        }
    }

    /// Normalizes raw pixel data, installs a personal-cache thumbnail, and returns its path.
    pub fn install_path(self) -> Result<InstalledThumbnailPath> {
        let Self {
            root,
            original,
            size,
            image,
        } = self;
        root.install_personal_thumbnail_raw_path(&original, size, image.as_borrowed())
    }

    /// Normalizes raw pixel data, installs a personal-cache thumbnail, and returns final bytes.
    pub fn install_bytes(self) -> Result<InstalledThumbnailBytes> {
        let Self {
            root,
            original,
            size,
            image,
        } = self;
        root.install_personal_thumbnail_raw_bytes(&original, size, image.as_borrowed())
    }

    /// Splits this request into its owned parts.
    #[must_use]
    pub fn into_parts(
        self,
    ) -> (
        PersonalCacheRoot,
        ReadableOriginalIdentity,
        ThumbnailSize,
        OwnedRawThumbnailImage,
    ) {
        (self.root, self.original, self.size, self.image)
    }
}

/// Owned failure-entry write request for async or runtime-specific adapters.
///
/// Constructing this request does not perform filesystem I/O. The failure entry is written only
/// when [`Self::write_path`] or [`Self::write_bytes`] is called.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FailureEntryWriteRequest {
    root: PersonalCacheRoot,
    namespace: FailureNamespace,
    original: ReadableOriginalIdentity,
}

impl FailureEntryWriteRequest {
    /// Creates an owned failure-entry write request.
    #[must_use]
    pub const fn new(
        root: PersonalCacheRoot,
        namespace: FailureNamespace,
        original: ReadableOriginalIdentity,
    ) -> Self {
        Self {
            root,
            namespace,
            original,
        }
    }

    /// Writes a deterministic 1x1 transparent failure entry and returns its path.
    pub fn write_path(self) -> Result<InstalledThumbnailPath> {
        let Self {
            root,
            namespace,
            original,
        } = self;
        root.write_failure_entry_path(&namespace, &original)
    }

    /// Writes a deterministic 1x1 transparent failure entry and returns final bytes.
    pub fn write_bytes(self) -> Result<InstalledThumbnailBytes> {
        let Self {
            root,
            namespace,
            original,
        } = self;
        root.write_failure_entry_bytes(&namespace, &original)
    }

    /// Splits this request into its owned parts.
    #[must_use]
    pub fn into_parts(
        self,
    ) -> (
        PersonalCacheRoot,
        FailureNamespace,
        ReadableOriginalIdentity,
    ) {
        (self.root, self.namespace, self.original)
    }
}

/// Owned personal-cache inspection request for async or runtime-specific adapters.
///
/// Constructing this request does not perform filesystem I/O. Inspection happens only when
/// [`Self::inspect`] is called.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PersonalThumbnailInspectionRequest {
    root: PersonalCacheRoot,
    sizes: Vec<ThumbnailSize>,
    include_nonstandard: bool,
}

impl PersonalThumbnailInspectionRequest {
    /// Creates an owned personal-cache inspection request.
    #[must_use]
    pub const fn new(
        root: PersonalCacheRoot,
        sizes: Vec<ThumbnailSize>,
        include_nonstandard: bool,
    ) -> Self {
        Self {
            root,
            sizes,
            include_nonstandard,
        }
    }

    /// Inspects standard successful thumbnail size directories.
    pub fn inspect(self) -> Result<Vec<CacheEntryInspection>> {
        let Self {
            root,
            sizes,
            include_nonstandard,
        } = self;
        root.inspect_thumbnails(&sizes, include_nonstandard)
    }

    /// Splits this request into its owned parts.
    #[must_use]
    pub fn into_parts(self) -> (PersonalCacheRoot, Vec<ThumbnailSize>, bool) {
        (self.root, self.sizes, self.include_nonstandard)
    }
}

/// Owned shared-repository lookup request for async or runtime-specific adapters.
///
/// Constructing this request does not perform filesystem I/O. Validation happens only when
/// [`Self::lookup_path`] or [`Self::lookup_bytes`] is called.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SharedThumbnailLookupRequest {
    context: SharedRepositoryContext,
    size: ThumbnailSize,
    metadata_policy: SharedThumbnailMetadataPolicy,
    mtime: Option<UnixMTimeSeconds>,
    original_byte_size: Option<u64>,
}

impl SharedThumbnailLookupRequest {
    /// Creates an owned shared-repository lookup request.
    #[must_use]
    pub const fn new(
        context: SharedRepositoryContext,
        size: ThumbnailSize,
        metadata_policy: SharedThumbnailMetadataPolicy,
        mtime: Option<UnixMTimeSeconds>,
        original_byte_size: Option<u64>,
    ) -> Self {
        Self {
            context,
            size,
            metadata_policy,
            mtime,
            original_byte_size,
        }
    }

    /// Returns a validated shared-repository path for the owned request.
    pub fn lookup_path(self) -> Result<SharedThumbnailLookup<ThumbnailPathLookupEntry>> {
        let Self {
            context,
            size,
            metadata_policy,
            mtime,
            original_byte_size,
        } = self;
        context.lookup_thumbnail_path(size, metadata_policy, mtime, original_byte_size)
    }

    /// Returns exact validated shared-repository PNG bytes for the owned request.
    pub fn lookup_bytes(self) -> Result<SharedThumbnailLookup<ThumbnailBytesLookupEntry>> {
        let Self {
            context,
            size,
            metadata_policy,
            mtime,
            original_byte_size,
        } = self;
        context.lookup_thumbnail_bytes(size, metadata_policy, mtime, original_byte_size)
    }

    /// Splits this request into its owned parts.
    #[must_use]
    pub fn into_parts(
        self,
    ) -> (
        SharedRepositoryContext,
        ThumbnailSize,
        SharedThumbnailMetadataPolicy,
        Option<UnixMTimeSeconds>,
        Option<u64>,
    ) {
        (
            self.context,
            self.size,
            self.metadata_policy,
            self.mtime,
            self.original_byte_size,
        )
    }
}

/// Owned shared-repository inspection request for async or runtime-specific adapters.
///
/// Constructing this request does not perform filesystem I/O. Inspection happens only when
/// [`Self::inspect`] is called.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SharedThumbnailInspectionRequest {
    context: SharedRepositoryContext,
    sizes: Vec<ThumbnailSize>,
    mtime: Option<UnixMTimeSeconds>,
    original_byte_size: Option<u64>,
}

impl SharedThumbnailInspectionRequest {
    /// Creates an owned shared-repository inspection request.
    #[must_use]
    pub const fn new(
        context: SharedRepositoryContext,
        sizes: Vec<ThumbnailSize>,
        mtime: Option<UnixMTimeSeconds>,
        original_byte_size: Option<u64>,
    ) -> Self {
        Self {
            context,
            sizes,
            mtime,
            original_byte_size,
        }
    }

    /// Inspects existing shared-repository thumbnails without exposing removal handles.
    pub fn inspect(self) -> Result<Vec<SharedCacheEntryInspection>> {
        let Self {
            context,
            sizes,
            mtime,
            original_byte_size,
        } = self;
        context.inspect_thumbnails(&sizes, mtime, original_byte_size)
    }

    /// Splits this request into its owned parts.
    #[must_use]
    pub fn into_parts(
        self,
    ) -> (
        SharedRepositoryContext,
        Vec<ThumbnailSize>,
        Option<UnixMTimeSeconds>,
        Option<u64>,
    ) {
        (
            self.context,
            self.sizes,
            self.mtime,
            self.original_byte_size,
        )
    }
}

fn only_unverifiable_original(problems: &[CacheEntryProblem]) -> bool {
    !problems.is_empty()
        && problems
            .iter()
            .all(|problem| *problem == CacheEntryProblem::UnverifiableOriginal)
}

enum CacheEntryRead {
    Missing,
    Unreadable,
    Bytes(Vec<u8>),
}

fn read_cache_entry_no_follow(path: &Path, context: &'static str) -> Result<CacheEntryRead> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(CacheEntryRead::Missing);
        }
        Err(source) => return Err(ThumbnailError::Io { context, source }),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Ok(CacheEntryRead::Unreadable);
    }

    let flags = rustix::fs::OFlags::RDONLY
        | rustix::fs::OFlags::CLOEXEC
        | rustix::fs::OFlags::NOFOLLOW
        | rustix::fs::OFlags::NONBLOCK;
    let fd = match rustix::fs::open(path, flags, rustix::fs::Mode::empty()) {
        Ok(fd) => fd,
        Err(rustix::io::Errno::NOENT) => return Ok(CacheEntryRead::Missing),
        Err(rustix::io::Errno::LOOP | rustix::io::Errno::ISDIR | rustix::io::Errno::NOTDIR) => {
            return Ok(CacheEntryRead::Unreadable);
        }
        Err(source) => {
            return Err(ThumbnailError::Io {
                context,
                source: std::io::Error::from(source),
            });
        }
    };

    let stat = rustix::fs::fstat(&fd).map_err(|source| ThumbnailError::Io {
        context,
        source: std::io::Error::from(source),
    })?;
    let file_type = rustix::fs::FileType::from_raw_mode(stat.st_mode);
    if !file_type.is_file() {
        return Ok(CacheEntryRead::Unreadable);
    }

    let mut file = File::from(fd);
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|source| ThumbnailError::Io { context, source })?;
    Ok(CacheEntryRead::Bytes(bytes))
}

fn shared_cache_entry_outcome(outcome: SharedValidationOutcome) -> SharedCacheEntryOutcome {
    match outcome {
        SharedValidationOutcome::FullyVerified => SharedCacheEntryOutcome::FullyVerified,
        SharedValidationOutcome::MetadataIncomplete => SharedCacheEntryOutcome::MetadataIncomplete,
        SharedValidationOutcome::Invalid(problems) if only_unverifiable_original(&problems) => {
            SharedCacheEntryOutcome::Unverifiable(problems)
        }
        SharedValidationOutcome::Invalid(problems) => SharedCacheEntryOutcome::Invalid(problems),
    }
}

struct ValidatedPersonalEntry {
    path: PathBuf,
    bytes: Vec<u8>,
    metadata: ThumbnailMetadata,
}

struct ValidatedSharedEntry {
    path: PathBuf,
    bytes: Vec<u8>,
    metadata: ThumbnailMetadata,
}

/// Result of a validated personal thumbnail cache lookup.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum PersonalThumbnailLookup<T> {
    /// The cache entry exists and passed validation.
    Valid(T),
    /// The computed cache path does not exist.
    Missing,
    /// The cache entry exists but is invalid for the requested context.
    Invalid(Vec<CacheEntryProblem>),
}

/// Result of a validated shared thumbnail repository lookup.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum SharedThumbnailLookup<T> {
    /// The cache entry exists and required metadata and PNG constraints are fully verified.
    FullyVerified(T),
    /// The cache entry is otherwise usable but lacks standard-optional shared freshness metadata.
    MetadataIncomplete(T),
    /// The computed shared cache path does not exist.
    Missing,
    /// The cache entry exists but is invalid for the requested context.
    Invalid(Vec<CacheEntryProblem>),
    /// Caller-supplied shared original facts are insufficient to verify the entry.
    Unverifiable(Vec<CacheEntryProblem>),
}

/// Metadata acceptance policy for shared-repository thumbnail lookups.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum SharedThumbnailMetadataPolicy {
    /// Require `Thumb::URI` and `Thumb::MTime` to be present and verified.
    RequireComplete,
    /// Accept standard-allowed missing `Thumb::URI` or `Thumb::MTime` as metadata-incomplete.
    AllowIncomplete,
}

/// Validation state for a shared cache entry inspection.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum SharedCacheEntryOutcome {
    /// Required metadata and PNG constraints are fully verified.
    FullyVerified,
    /// Standard-optional shared freshness metadata is absent.
    MetadataIncomplete,
    /// The entry is invalid for the requested shared context.
    Invalid(Vec<CacheEntryProblem>),
    /// Caller-supplied shared original facts are insufficient to verify the entry.
    Unverifiable(Vec<CacheEntryProblem>),
}

/// Read-only inspection facts for an existing shared thumbnail repository entry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SharedCacheEntryInspection {
    outcome: SharedCacheEntryOutcome,
    shared_uri: SharedRelativeOriginalUri,
    timestamps: ThumbnailTimestamps,
    size: ThumbnailSize,
    path: PathBuf,
    metadata: Option<ThumbnailMetadata>,
}

impl SharedCacheEntryInspection {
    /// Returns the shared validation or inspection outcome.
    #[must_use]
    pub const fn outcome(&self) -> &SharedCacheEntryOutcome {
        &self.outcome
    }

    /// Returns the shared relative URI used for hashing and metadata comparison.
    #[must_use]
    pub const fn shared_uri(&self) -> &SharedRelativeOriginalUri {
        &self.shared_uri
    }

    /// Returns timestamp facts.
    #[must_use]
    pub const fn timestamps(&self) -> &ThumbnailTimestamps {
        &self.timestamps
    }

    /// Returns the successful-thumbnail size namespace.
    #[must_use]
    pub const fn size(&self) -> ThumbnailSize {
        self.size
    }

    /// Returns the inspected shared cache path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns parsed metadata when the entry was a readable PNG.
    #[must_use]
    pub const fn metadata(&self) -> Option<&ThumbnailMetadata> {
        self.metadata.as_ref()
    }
}

/// A validated cache path and metadata facts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThumbnailPathLookupEntry {
    path: PathBuf,
    metadata: ThumbnailMetadata,
}

impl ThumbnailPathLookupEntry {
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

    /// Splits this result into its owned path and metadata.
    #[must_use]
    pub fn into_parts(self) -> (PathBuf, ThumbnailMetadata) {
        (self.path, self.metadata)
    }
}

/// Exact validated PNG bytes and metadata facts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThumbnailBytesLookupEntry {
    path: PathBuf,
    bytes: Vec<u8>,
    metadata: ThumbnailMetadata,
}

impl ThumbnailBytesLookupEntry {
    /// Returns the path from which the bytes were validated.
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

    /// Splits this result into its owned path, bytes, and metadata.
    #[must_use]
    pub fn into_parts(self) -> (PathBuf, Vec<u8>, ThumbnailMetadata) {
        (self.path, self.bytes, self.metadata)
    }
}

/// Path result of a successful personal-cache install or failure-entry write.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstalledThumbnailPath {
    path: PathBuf,
}

impl InstalledThumbnailPath {
    /// Returns the installed cache path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the installed path as an owned [`PathBuf`].
    #[must_use]
    pub fn into_path_buf(self) -> PathBuf {
        self.path
    }
}

/// Bytes result of a successful personal-cache install or failure-entry write.
///
/// The returned bytes are the final PNG bytes published to the cache after metadata writing and
/// normalization. Installation metadata is determined from the supplied original facts; callers
/// that need to inspect the installed metadata can parse these bytes with [`ParsedThumbnailPng`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstalledThumbnailBytes {
    path: PathBuf,
    bytes: Vec<u8>,
}

impl InstalledThumbnailBytes {
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

    /// Splits this result into its owned path and final PNG bytes.
    #[must_use]
    pub fn into_parts(self) -> (PathBuf, Vec<u8>) {
        (self.path, self.bytes)
    }
}

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
