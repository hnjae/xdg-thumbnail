// SPDX-FileCopyrightText: 2026 KIM Hyunjae
// SPDX-License-Identifier: MPL-2.0

use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};

use crate::inspection::{
    CacheEntryInspection, NonstandardEntryPolicy, read_thumbnail_for_inspection,
    thumbnail_timestamps, thumbnail_timestamps_from_metadata,
};
use crate::{
    AccessTimePreservation, CacheDirectoryProblem, CacheEntryHandle, CacheEntryProblem,
    CacheNamespace, CachePathProblem, CacheRootProblem, FailureNamespace, OwnedRawThumbnailImage,
    ParsedThumbnailPng, PersonalValidationOutcome, RawThumbnailImage,
    ReadablePersonalOriginalIdentity, Result, SharedRelativeOriginalUri, SharedRepositoryContext,
    SharedValidationOutcome, ThumbnailError, ThumbnailMetadata, ThumbnailSize, ThumbnailTimestamps,
    UnixMtimeSeconds, decode_validated_thumbnail_png_to_rgba8,
    downscaled_validated_thumbnail_png_to_rgba8, encode_rgba_png, metadata_problem,
    normalized_personal_thumbnail_from_cache_png, normalized_personal_thumbnail_png,
    normalized_personal_thumbnail_raw_png, push_problem, thumbnail_metadata_pairs,
    validate_personal_thumbnail, validate_shared_thumbnail,
};
use crate::{PersonalOriginalIdentity, PersonalOriginalUri};
use crate::{ThumbnailMetadataKey, ThumbnailMetadataProblemKind};

/// Root directory of the personal thumbnail cache, usually `$XDG_CACHE_HOME/thumbnails`.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct PersonalCacheRoot {
    path: PathBuf,
}

impl PersonalCacheRoot {
    /// Creates a cache root from an already resolved absolute thumbnail root path.
    ///
    /// # Errors
    ///
    /// Returns an error when `path` is not absolute.
    pub fn new(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if !path.is_absolute() {
            return Err(ThumbnailError::InvalidCacheRoot {
                path: path.to_owned(),
                problem: CacheRootProblem::NotAbsolute,
            });
        }
        Ok(Self {
            path: path.to_owned(),
        })
    }

    /// Resolves the personal thumbnail cache root from the process environment.
    ///
    /// # Errors
    ///
    /// Returns an error when neither an absolute `XDG_CACHE_HOME` nor an absolute `HOME` fallback
    /// is available.
    pub fn resolve_from_env() -> Result<Self> {
        let xdg_cache_home = std::env::var_os("XDG_CACHE_HOME");
        let home = std::env::var_os("HOME");
        Self::resolve_from_values(xdg_cache_home.as_deref(), home.as_deref())
    }

    /// Resolves the personal thumbnail cache root from supplied XDG values.
    ///
    /// Relative, unset, and blank `XDG_CACHE_HOME` values are ignored. `HOME`
    /// must be absolute when fallback is needed.
    ///
    /// # Errors
    ///
    /// Returns an error when the resolved thumbnail root would not be absolute.
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
            return Err(ThumbnailError::cache_root_unavailable(
                "HOME is required when XDG_CACHE_HOME is unset, blank, or relative",
            ));
        };
        if home.as_bytes().is_empty() {
            return Err(ThumbnailError::cache_root_unavailable(
                "HOME is required when XDG_CACHE_HOME is unset, blank, or relative",
            ));
        }
        let home = PathBuf::from(home);
        if !home.is_absolute() {
            return Err(ThumbnailError::cache_root_unavailable(
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
    pub fn cache_entry_path(
        &self,
        uri: &PersonalOriginalUri,
        namespace: &CacheNamespace,
    ) -> PathBuf {
        namespace.join_under(&self.path, &uri.thumbnail_file_name())
    }

    /// Returns a safe-removal handle for a computed personal-cache entry path.
    ///
    /// The handle uses the same pure path calculation as [`Self::cache_entry_path`] and does not
    /// check whether the entry currently exists. Removal still performs containment, no-follow, and
    /// regular-file checks at deletion time.
    #[must_use]
    pub fn cache_entry_handle(
        &self,
        uri: &PersonalOriginalUri,
        namespace: &CacheNamespace,
    ) -> CacheEntryHandle {
        let cache_dir = match namespace {
            CacheNamespace::Size(size) => self.path.join(size.directory_name()),
            CacheNamespace::Failure(namespace) => self.path.join("fail").join(namespace.as_str()),
        };
        CacheEntryHandle::new(cache_dir, self.cache_entry_path(uri, namespace))
    }

    /// Returns a validated personal-cache path for integrations that must pass a filename.
    ///
    /// The original identity must have already been confirmed readable. The candidate PNG is
    /// opened and validated before this method returns. Callers that reopen the returned path
    /// accept that another process may replace it after validation.
    ///
    /// # Errors
    ///
    /// Returns an error for unexpected filesystem I/O while reading the candidate or for PNG
    /// metadata parse failures after validation succeeds.
    pub fn lookup_thumbnail_path(
        &self,
        original: &ReadablePersonalOriginalIdentity,
        size: ThumbnailSize,
    ) -> Result<PersonalThumbnailLookup<ThumbnailPathLookupEntry>> {
        match self.lookup_thumbnail_entry(original, size)? {
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
    ///
    /// # Errors
    ///
    /// Returns an error for unexpected filesystem I/O while reading the candidate or for PNG
    /// metadata parse failures after validation succeeds.
    pub fn lookup_thumbnail_png_bytes(
        &self,
        original: &ReadablePersonalOriginalIdentity,
        size: ThumbnailSize,
    ) -> Result<PersonalThumbnailLookup<ThumbnailPngBytesLookupEntry>> {
        match self.lookup_thumbnail_entry(original, size)? {
            PersonalThumbnailLookup::Valid(entry) => Ok(PersonalThumbnailLookup::Valid(
                ThumbnailPngBytesLookupEntry {
                    path: entry.path,
                    bytes: entry.bytes,
                    metadata: entry.metadata,
                },
            )),
            PersonalThumbnailLookup::Missing => Ok(PersonalThumbnailLookup::Missing),
            PersonalThumbnailLookup::Invalid(problems) => {
                Ok(PersonalThumbnailLookup::Invalid(problems))
            }
        }
    }

    /// Returns decoded tightly packed RGBA8 pixels from the personal thumbnail cache.
    ///
    /// The original identity must have already been confirmed readable. The returned pixels are
    /// row-major `[red, green, blue, alpha]` bytes with straight alpha and `stride == width * 4`.
    ///
    /// # Errors
    ///
    /// Returns an error for unexpected filesystem I/O while reading the candidate or for PNG
    /// decoding failures after validation succeeds.
    pub fn lookup_thumbnail_rgba8(
        &self,
        original: &ReadablePersonalOriginalIdentity,
        size: ThumbnailSize,
    ) -> Result<PersonalThumbnailLookup<ThumbnailRgba8LookupEntry>> {
        match self.lookup_thumbnail_entry(original, size)? {
            PersonalThumbnailLookup::Valid(entry) => Ok(PersonalThumbnailLookup::Valid(
                rgba8_lookup_entry_from_parts(entry.path, &entry.bytes, entry.metadata)?,
            )),
            PersonalThumbnailLookup::Missing => Ok(PersonalThumbnailLookup::Missing),
            PersonalThumbnailLookup::Invalid(problems) => {
                Ok(PersonalThumbnailLookup::Invalid(problems))
            }
        }
    }

    /// Returns decoded RGBA8 display pixels, falling back to larger personal-cache namespaces.
    ///
    /// The exact namespace is checked first. If it is missing, larger namespaces are checked in
    /// ascending order. The first non-missing candidate controls the result.
    ///
    /// # Errors
    ///
    /// Returns an error for unexpected filesystem I/O while reading the candidate or for PNG
    /// decoding failures after validation succeeds.
    pub fn lookup_display_thumbnail_rgba8(
        &self,
        original: &ReadablePersonalOriginalIdentity,
        size: ThumbnailSize,
    ) -> Result<PersonalThumbnailLookup<DisplayThumbnailRgba8LookupEntry>> {
        for source_size in display_candidate_sizes(size) {
            match self.lookup_thumbnail_entry(original, source_size)? {
                PersonalThumbnailLookup::Valid(entry) => {
                    return Ok(PersonalThumbnailLookup::Valid(
                        display_rgba8_lookup_entry_from_parts(
                            entry.path,
                            &entry.bytes,
                            entry.metadata,
                            size,
                            source_size,
                        )?,
                    ));
                }
                PersonalThumbnailLookup::Missing => {}
                PersonalThumbnailLookup::Invalid(problems) => {
                    return Ok(PersonalThumbnailLookup::Invalid(problems));
                }
            }
        }
        Ok(PersonalThumbnailLookup::Missing)
    }

    /// Normalizes rendered PNG data, atomically installs a personal-cache thumbnail, and returns its path.
    ///
    /// # Errors
    ///
    /// Returns an error when rendered PNG normalization fails, final thumbnail validation fails,
    /// cache directories are unavailable or insecure, or atomic installation fails.
    pub fn install_thumbnail_returning_path(
        &self,
        original: &ReadablePersonalOriginalIdentity,
        size: ThumbnailSize,
        rendered_png: &[u8],
    ) -> Result<InstalledThumbnailPath> {
        let (path, _) = self.install_thumbnail_entry(original, size, rendered_png)?;
        Ok(InstalledThumbnailPath { path })
    }

    /// Normalizes rendered PNG data, atomically installs a personal-cache thumbnail, and returns final PNG bytes.
    ///
    /// # Errors
    ///
    /// Returns an error when rendered PNG normalization fails, final thumbnail validation fails,
    /// cache directories are unavailable or insecure, or atomic installation fails.
    pub fn install_thumbnail_returning_png_bytes(
        &self,
        original: &ReadablePersonalOriginalIdentity,
        size: ThumbnailSize,
        rendered_png: &[u8],
    ) -> Result<InstalledThumbnailPngBytes> {
        let (path, bytes) = self.install_thumbnail_entry(original, size, rendered_png)?;
        Ok(InstalledThumbnailPngBytes { path, bytes })
    }

    /// Normalizes raw pixel data, atomically installs a personal-cache thumbnail, and returns its path.
    ///
    /// # Errors
    ///
    /// Returns an error when raw conversion or normalization fails, final thumbnail validation
    /// fails, cache directories are unavailable or insecure, or atomic installation fails.
    pub fn install_raw_thumbnail_returning_path(
        &self,
        original: &ReadablePersonalOriginalIdentity,
        size: ThumbnailSize,
        image: RawThumbnailImage<'_>,
    ) -> Result<InstalledThumbnailPath> {
        let (path, _) = self.install_thumbnail_raw_entry(original, size, image)?;
        Ok(InstalledThumbnailPath { path })
    }

    /// Normalizes raw pixel data, atomically installs a personal-cache thumbnail, and returns final PNG bytes.
    ///
    /// # Errors
    ///
    /// Returns an error when raw conversion or normalization fails, final thumbnail validation
    /// fails, cache directories are unavailable or insecure, or atomic installation fails.
    pub fn install_raw_thumbnail_returning_png_bytes(
        &self,
        original: &ReadablePersonalOriginalIdentity,
        size: ThumbnailSize,
        image: RawThumbnailImage<'_>,
    ) -> Result<InstalledThumbnailPngBytes> {
        let (path, bytes) = self.install_thumbnail_raw_entry(original, size, image)?;
        Ok(InstalledThumbnailPngBytes { path, bytes })
    }

    /// Materializes the requested personal-cache namespace from an exact or larger personal entry.
    ///
    /// # Errors
    ///
    /// Returns an error when final PNG normalization, validation, or atomic installation fails.
    pub fn materialize_thumbnail_from_larger_cache_returning_path(
        &self,
        original: &ReadablePersonalOriginalIdentity,
        size: ThumbnailSize,
    ) -> Result<PersonalThumbnailLookup<MaterializedThumbnailPath>> {
        match self.materialize_thumbnail_from_larger_cache_entry(original, size)? {
            PersonalThumbnailLookup::Valid(entry) => {
                Ok(PersonalThumbnailLookup::Valid(MaterializedThumbnailPath {
                    target_path: entry.target_path,
                    source_path: entry.source_path,
                    requested_size: entry.requested_size,
                    source_size: entry.source_size,
                    written: entry.written,
                }))
            }
            PersonalThumbnailLookup::Missing => Ok(PersonalThumbnailLookup::Missing),
            PersonalThumbnailLookup::Invalid(problems) => {
                Ok(PersonalThumbnailLookup::Invalid(problems))
            }
        }
    }

    /// Materializes the requested personal-cache namespace and returns final target PNG bytes.
    ///
    /// # Errors
    ///
    /// Returns an error when final PNG normalization, validation, or atomic installation fails.
    pub fn materialize_thumbnail_from_larger_cache_returning_png_bytes(
        &self,
        original: &ReadablePersonalOriginalIdentity,
        size: ThumbnailSize,
    ) -> Result<PersonalThumbnailLookup<MaterializedThumbnailPngBytes>> {
        match self.materialize_thumbnail_from_larger_cache_entry(original, size)? {
            PersonalThumbnailLookup::Valid(entry) => Ok(PersonalThumbnailLookup::Valid(
                MaterializedThumbnailPngBytes {
                    target_path: entry.target_path,
                    source_path: entry.source_path,
                    requested_size: entry.requested_size,
                    source_size: entry.source_size,
                    written: entry.written,
                    bytes: entry.bytes,
                },
            )),
            PersonalThumbnailLookup::Missing => Ok(PersonalThumbnailLookup::Missing),
            PersonalThumbnailLookup::Invalid(problems) => {
                Ok(PersonalThumbnailLookup::Invalid(problems))
            }
        }
    }

    /// Materializes a shared thumbnail into this personal cache and returns the target path.
    ///
    /// Shared repositories are read-only; this method writes only to the receiver personal cache.
    ///
    /// # Errors
    ///
    /// Returns an error when shared facts cannot produce required personal metadata, final PNG
    /// normalization fails, or atomic installation fails.
    pub fn materialize_shared_thumbnail_returning_path(
        &self,
        shared: &SharedRepositoryContext,
        original_facts: SharedOriginalFacts,
        size: ThumbnailSize,
    ) -> Result<SharedThumbnailLookup<MaterializedThumbnailPath>> {
        match self.materialize_shared_thumbnail_entry(shared, original_facts, size)? {
            SharedThumbnailLookup::FullyVerified(entry) => Ok(
                SharedThumbnailLookup::FullyVerified(MaterializedThumbnailPath {
                    target_path: entry.target_path,
                    source_path: entry.source_path,
                    requested_size: entry.requested_size,
                    source_size: entry.source_size,
                    written: entry.written,
                }),
            ),
            SharedThumbnailLookup::MetadataIncomplete(entry) => Ok(
                SharedThumbnailLookup::MetadataIncomplete(MaterializedThumbnailPath {
                    target_path: entry.target_path,
                    source_path: entry.source_path,
                    requested_size: entry.requested_size,
                    source_size: entry.source_size,
                    written: entry.written,
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

    /// Materializes a shared thumbnail into this personal cache and returns final target PNG bytes.
    ///
    /// Shared repositories are read-only; this method writes only to the receiver personal cache.
    ///
    /// # Errors
    ///
    /// Returns an error when shared facts cannot produce required personal metadata, final PNG
    /// normalization fails, or atomic installation fails.
    pub fn materialize_shared_thumbnail_returning_png_bytes(
        &self,
        shared: &SharedRepositoryContext,
        original_facts: SharedOriginalFacts,
        size: ThumbnailSize,
    ) -> Result<SharedThumbnailLookup<MaterializedThumbnailPngBytes>> {
        match self.materialize_shared_thumbnail_entry(shared, original_facts, size)? {
            SharedThumbnailLookup::FullyVerified(entry) => Ok(
                SharedThumbnailLookup::FullyVerified(MaterializedThumbnailPngBytes {
                    target_path: entry.target_path,
                    source_path: entry.source_path,
                    requested_size: entry.requested_size,
                    source_size: entry.source_size,
                    written: entry.written,
                    bytes: entry.bytes,
                }),
            ),
            SharedThumbnailLookup::MetadataIncomplete(entry) => Ok(
                SharedThumbnailLookup::MetadataIncomplete(MaterializedThumbnailPngBytes {
                    target_path: entry.target_path,
                    source_path: entry.source_path,
                    requested_size: entry.requested_size,
                    source_size: entry.source_size,
                    written: entry.written,
                    bytes: entry.bytes,
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

    fn install_thumbnail_entry(
        &self,
        original: &ReadablePersonalOriginalIdentity,
        size: ThumbnailSize,
        rendered_png: &[u8],
    ) -> Result<(PathBuf, Vec<u8>)> {
        let namespace = CacheNamespace::Size(size);
        let path = self.cache_entry_path(original.identity().uri(), &namespace);
        let bytes = normalized_personal_thumbnail_png(rendered_png, original.identity(), size)?;
        self.write_personal_entry(&path, &namespace, &bytes)?;
        Ok((path, bytes))
    }

    fn install_thumbnail_raw_entry(
        &self,
        original: &ReadablePersonalOriginalIdentity,
        size: ThumbnailSize,
        image: RawThumbnailImage<'_>,
    ) -> Result<(PathBuf, Vec<u8>)> {
        let namespace = CacheNamespace::Size(size);
        let path = self.cache_entry_path(original.identity().uri(), &namespace);
        let bytes = normalized_personal_thumbnail_raw_png(image, original.identity(), size)?;
        self.write_personal_entry(&path, &namespace, &bytes)?;
        Ok((path, bytes))
    }

    fn materialize_thumbnail_from_larger_cache_entry(
        &self,
        original: &ReadablePersonalOriginalIdentity,
        size: ThumbnailSize,
    ) -> Result<PersonalThumbnailLookup<MaterializedPersonalEntry>> {
        let target_path =
            self.cache_entry_path(original.identity().uri(), &CacheNamespace::Size(size));
        for source_size in display_candidate_sizes(size) {
            match self.lookup_thumbnail_entry(original, source_size)? {
                PersonalThumbnailLookup::Valid(entry) if source_size == size => {
                    return Ok(PersonalThumbnailLookup::Valid(MaterializedPersonalEntry {
                        target_path,
                        source_path: entry.path,
                        requested_size: size,
                        source_size,
                        written: false,
                        bytes: entry.bytes,
                    }));
                }
                PersonalThumbnailLookup::Valid(entry) => {
                    let bytes = normalized_personal_thumbnail_from_cache_png(
                        &entry.bytes,
                        original.identity(),
                        size,
                    )?;
                    self.write_personal_entry(&target_path, &CacheNamespace::Size(size), &bytes)?;
                    return Ok(PersonalThumbnailLookup::Valid(MaterializedPersonalEntry {
                        target_path,
                        source_path: entry.path,
                        requested_size: size,
                        source_size,
                        written: true,
                        bytes,
                    }));
                }
                PersonalThumbnailLookup::Missing => {}
                PersonalThumbnailLookup::Invalid(problems) => {
                    return Ok(PersonalThumbnailLookup::Invalid(problems));
                }
            }
        }
        Ok(PersonalThumbnailLookup::Missing)
    }

    fn materialize_shared_thumbnail_entry(
        &self,
        shared: &SharedRepositoryContext,
        original_facts: SharedOriginalFacts,
        size: ThumbnailSize,
    ) -> Result<SharedThumbnailLookup<MaterializedPersonalEntry>> {
        let original = personal_identity_from_shared_facts(shared, original_facts)?;
        let target_path = self.cache_entry_path(original.uri(), &CacheNamespace::Size(size));
        for source_size in display_candidate_sizes(size) {
            match shared.lookup_thumbnail_entry(source_size, original_facts)? {
                SharedThumbnailLookup::FullyVerified(entry) => {
                    let bytes = normalized_personal_thumbnail_from_cache_png(
                        &entry.bytes,
                        &original,
                        size,
                    )?;
                    self.write_personal_entry(&target_path, &CacheNamespace::Size(size), &bytes)?;
                    return Ok(SharedThumbnailLookup::FullyVerified(
                        MaterializedPersonalEntry {
                            target_path,
                            source_path: entry.path,
                            requested_size: size,
                            source_size,
                            written: true,
                            bytes,
                        },
                    ));
                }
                SharedThumbnailLookup::MetadataIncomplete(entry) => {
                    let bytes = normalized_personal_thumbnail_from_cache_png(
                        &entry.bytes,
                        &original,
                        size,
                    )?;
                    self.write_personal_entry(&target_path, &CacheNamespace::Size(size), &bytes)?;
                    return Ok(SharedThumbnailLookup::MetadataIncomplete(
                        MaterializedPersonalEntry {
                            target_path,
                            source_path: entry.path,
                            requested_size: size,
                            source_size,
                            written: true,
                            bytes,
                        },
                    ));
                }
                SharedThumbnailLookup::Missing => {}
                SharedThumbnailLookup::Invalid(problems) => {
                    return Ok(SharedThumbnailLookup::Invalid(problems));
                }
                SharedThumbnailLookup::Unverifiable(problems) => {
                    return Ok(SharedThumbnailLookup::Unverifiable(problems));
                }
            }
        }
        Ok(SharedThumbnailLookup::Missing)
    }

    /// Writes a deterministic 1x1 transparent failure entry and returns its path.
    ///
    /// # Errors
    ///
    /// Returns an error when failure-entry PNG encoding fails, cache directories are unavailable or
    /// insecure, or atomic installation fails.
    pub fn write_failure_entry_returning_path(
        &self,
        original: &ReadablePersonalOriginalIdentity,
        namespace: &FailureNamespace,
    ) -> Result<InstalledThumbnailPath> {
        let (path, _) = self.write_failure_entry_returning_png_bytes_inner(original, namespace)?;
        Ok(InstalledThumbnailPath { path })
    }

    /// Writes a deterministic 1x1 transparent failure entry and returns final PNG bytes.
    ///
    /// # Errors
    ///
    /// Returns an error when failure-entry PNG encoding fails, cache directories are unavailable or
    /// insecure, or atomic installation fails.
    pub fn write_failure_entry_returning_png_bytes(
        &self,
        original: &ReadablePersonalOriginalIdentity,
        namespace: &FailureNamespace,
    ) -> Result<InstalledThumbnailPngBytes> {
        let (path, bytes) =
            self.write_failure_entry_returning_png_bytes_inner(original, namespace)?;
        Ok(InstalledThumbnailPngBytes { path, bytes })
    }

    fn write_failure_entry_returning_png_bytes_inner(
        &self,
        original: &ReadablePersonalOriginalIdentity,
        namespace: &FailureNamespace,
    ) -> Result<(PathBuf, Vec<u8>)> {
        let namespace = CacheNamespace::Failure(namespace.clone());
        let path = self.cache_entry_path(original.identity().uri(), &namespace);
        let bytes = encode_rgba_png(
            1,
            1,
            &[0, 0, 0, 0],
            &thumbnail_metadata_pairs(original.identity()),
        )?;
        self.write_personal_entry(&path, &namespace, &bytes)?;
        Ok((path, bytes))
    }

    fn lookup_thumbnail_entry(
        &self,
        original: &ReadablePersonalOriginalIdentity,
        size: ThumbnailSize,
    ) -> Result<PersonalThumbnailLookup<ValidatedPersonalEntry>> {
        let path = self.cache_entry_path(original.identity().uri(), &CacheNamespace::Size(size));
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
        let parent = path
            .parent()
            .ok_or_else(|| ThumbnailError::InvalidCachePath {
                path: path.to_owned(),
                problem: CachePathProblem::MissingParentDirectory,
            })?;
        let mut temp = tempfile::Builder::new()
            .prefix(".xdg-thumbnail-")
            .tempfile_in(parent)
            .map_err(|source| {
                ThumbnailError::io(
                    "create thumbnail temporary file",
                    Some(parent.to_owned()),
                    source,
                )
            })?;
        temp.as_file_mut().write_all(bytes).map_err(|source| {
            ThumbnailError::io(
                "write thumbnail temporary file",
                Some(temp.path().to_owned()),
                source,
            )
        })?;
        temp.as_file_mut()
            .set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|source| {
                ThumbnailError::io(
                    "set thumbnail temporary file permissions",
                    Some(temp.path().to_owned()),
                    source,
                )
            })?;
        temp.as_file_mut().sync_all().map_err(|source| {
            ThumbnailError::io(
                "sync thumbnail temporary file",
                Some(temp.path().to_owned()),
                source,
            )
        })?;
        fs::rename(temp.path(), path).map_err(|source| {
            ThumbnailError::io(
                "publish thumbnail cache entry",
                Some(path.to_owned()),
                source,
            )
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

impl AsRef<Path> for PersonalCacheRoot {
    fn as_ref(&self) -> &Path {
        self.as_path()
    }
}

impl SharedRepositoryContext {
    /// Returns a validated shared-repository path for integrations that must pass a filename.
    ///
    /// # Errors
    ///
    /// Returns an error for unexpected filesystem I/O while reading the candidate or for PNG
    /// metadata parse failures after validation succeeds.
    pub fn lookup_thumbnail_path(
        &self,
        original_facts: SharedOriginalFacts,
        size: ThumbnailSize,
    ) -> Result<SharedThumbnailLookup<ThumbnailPathLookupEntry>> {
        match self.lookup_thumbnail_entry(size, original_facts)? {
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
    ///
    /// # Errors
    ///
    /// Returns an error for unexpected filesystem I/O while reading the candidate or for PNG
    /// metadata parse failures after validation succeeds.
    pub fn lookup_thumbnail_png_bytes(
        &self,
        original_facts: SharedOriginalFacts,
        size: ThumbnailSize,
    ) -> Result<SharedThumbnailLookup<ThumbnailPngBytesLookupEntry>> {
        match self.lookup_thumbnail_entry(size, original_facts)? {
            SharedThumbnailLookup::FullyVerified(entry) => Ok(
                SharedThumbnailLookup::FullyVerified(ThumbnailPngBytesLookupEntry {
                    path: entry.path,
                    bytes: entry.bytes,
                    metadata: entry.metadata,
                }),
            ),
            SharedThumbnailLookup::MetadataIncomplete(entry) => Ok(
                SharedThumbnailLookup::MetadataIncomplete(ThumbnailPngBytesLookupEntry {
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

    /// Returns decoded tightly packed RGBA8 pixels from a shared thumbnail repository.
    ///
    /// The returned pixels are row-major `[red, green, blue, alpha]` bytes with straight alpha and
    /// `stride == width * 4`.
    ///
    /// # Errors
    ///
    /// Returns an error for unexpected filesystem I/O while reading the candidate or for PNG
    /// decoding failures after validation succeeds.
    pub fn lookup_thumbnail_rgba8(
        &self,
        original_facts: SharedOriginalFacts,
        size: ThumbnailSize,
    ) -> Result<SharedThumbnailLookup<ThumbnailRgba8LookupEntry>> {
        match self.lookup_thumbnail_entry(size, original_facts)? {
            SharedThumbnailLookup::FullyVerified(entry) => {
                Ok(SharedThumbnailLookup::FullyVerified(
                    rgba8_lookup_entry_from_parts(entry.path, &entry.bytes, entry.metadata)?,
                ))
            }
            SharedThumbnailLookup::MetadataIncomplete(entry) => {
                Ok(SharedThumbnailLookup::MetadataIncomplete(
                    rgba8_lookup_entry_from_parts(entry.path, &entry.bytes, entry.metadata)?,
                ))
            }
            SharedThumbnailLookup::Missing => Ok(SharedThumbnailLookup::Missing),
            SharedThumbnailLookup::Invalid(problems) => {
                Ok(SharedThumbnailLookup::Invalid(problems))
            }
            SharedThumbnailLookup::Unverifiable(problems) => {
                Ok(SharedThumbnailLookup::Unverifiable(problems))
            }
        }
    }

    /// Returns decoded RGBA8 display pixels, falling back to larger shared-repository namespaces.
    ///
    /// The exact namespace is checked first. If it is missing, larger namespaces are checked in
    /// ascending order. The first non-missing candidate controls the result.
    ///
    /// # Errors
    ///
    /// Returns an error for unexpected filesystem I/O while reading the candidate or for PNG
    /// decoding failures after validation succeeds.
    pub fn lookup_display_thumbnail_rgba8(
        &self,
        original_facts: SharedOriginalFacts,
        size: ThumbnailSize,
    ) -> Result<SharedThumbnailLookup<DisplayThumbnailRgba8LookupEntry>> {
        for source_size in display_candidate_sizes(size) {
            match self.lookup_thumbnail_entry(source_size, original_facts)? {
                SharedThumbnailLookup::FullyVerified(entry) => {
                    return Ok(SharedThumbnailLookup::FullyVerified(
                        display_rgba8_lookup_entry_from_parts(
                            entry.path,
                            &entry.bytes,
                            entry.metadata,
                            size,
                            source_size,
                        )?,
                    ));
                }
                SharedThumbnailLookup::MetadataIncomplete(entry) => {
                    return Ok(SharedThumbnailLookup::MetadataIncomplete(
                        display_rgba8_lookup_entry_from_parts(
                            entry.path,
                            &entry.bytes,
                            entry.metadata,
                            size,
                            source_size,
                        )?,
                    ));
                }
                SharedThumbnailLookup::Missing => {}
                SharedThumbnailLookup::Invalid(problems) => {
                    return Ok(SharedThumbnailLookup::Invalid(problems));
                }
                SharedThumbnailLookup::Unverifiable(problems) => {
                    return Ok(SharedThumbnailLookup::Unverifiable(problems));
                }
            }
        }
        Ok(SharedThumbnailLookup::Missing)
    }

    /// Inspects existing shared-repository thumbnails without exposing removal handles.
    ///
    /// # Errors
    ///
    /// Returns an error when a selected shared thumbnail cannot be inspected due to unexpected
    /// filesystem I/O.
    pub fn inspect_thumbnails(
        &self,
        sizes: &[ThumbnailSize],
        original: SharedOriginalMetadata,
    ) -> Result<Vec<SharedCacheEntryInspection>> {
        let mut inspections = Vec::new();
        for &size in sizes {
            if let Some(inspection) = self.inspect_thumbnail(size, original)? {
                inspections.push(inspection);
            }
        }
        Ok(inspections)
    }

    fn lookup_thumbnail_entry(
        &self,
        size: ThumbnailSize,
        original_facts: SharedOriginalFacts,
    ) -> Result<SharedThumbnailLookup<ValidatedSharedEntry>> {
        let path = self.cache_entry_path(size);
        let bytes = match read_cache_entry_no_follow(&path, "read shared thumbnail cache entry")? {
            CacheEntryRead::Bytes(bytes) => bytes,
            CacheEntryRead::Missing => return Ok(SharedThumbnailLookup::Missing),
            CacheEntryRead::Unreadable => {
                return Ok(SharedThumbnailLookup::Invalid(vec![
                    CacheEntryProblem::UnreadableEntry,
                ]));
            }
        };

        match validate_shared_thumbnail(&bytes, self, original_facts.metadata(), size) {
            SharedValidationOutcome::FullyVerified => {
                let parsed = ParsedThumbnailPng::parse(&bytes)?;
                Ok(SharedThumbnailLookup::FullyVerified(ValidatedSharedEntry {
                    path,
                    bytes,
                    metadata: parsed.into_metadata(),
                }))
            }
            SharedValidationOutcome::MetadataIncomplete => {
                if original_facts.metadata_policy()
                    == SharedThumbnailMetadataPolicy::RequireComplete
                {
                    let parsed = ParsedThumbnailPng::parse(&bytes)?;
                    Ok(SharedThumbnailLookup::Invalid(
                        missing_required_shared_metadata_problems(parsed.metadata()),
                    ))
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
        original: SharedOriginalMetadata,
    ) -> Result<Option<SharedCacheEntryInspection>> {
        let path = self.cache_entry_path(size);
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
        let outcome =
            shared_cache_entry_outcome(validate_shared_thumbnail(&bytes, self, original, size));
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

fn missing_required_shared_metadata_problems(
    metadata: &ThumbnailMetadata,
) -> Vec<CacheEntryProblem> {
    let mut problems = Vec::new();
    if metadata.thumb_uri().is_none() {
        push_problem(
            &mut problems,
            metadata_problem(
                ThumbnailMetadataKey::Uri,
                ThumbnailMetadataProblemKind::MissingRequired,
            ),
        );
    }
    if matches!(metadata.thumb_mtime_result(), Ok(None)) {
        push_problem(
            &mut problems,
            metadata_problem(
                ThumbnailMetadataKey::Mtime,
                ThumbnailMetadataProblemKind::MissingRequired,
            ),
        );
    }
    problems
}

/// Owned personal-cache lookup request for async or runtime-specific adapters.
///
/// Constructing this request does not perform filesystem I/O. Validation happens only when
/// [`Self::lookup_path`], [`Self::lookup_png_bytes`], or [`Self::lookup_rgba8`] is called.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PersonalThumbnailLookupRequest {
    root: PersonalCacheRoot,
    original: ReadablePersonalOriginalIdentity,
    size: ThumbnailSize,
}

impl PersonalThumbnailLookupRequest {
    /// Creates an owned personal-cache lookup request.
    #[must_use]
    pub fn new(
        root: PersonalCacheRoot,
        original: ReadablePersonalOriginalIdentity,
        size: ThumbnailSize,
    ) -> Self {
        Self {
            root,
            original,
            size,
        }
    }

    /// Returns a validated personal-cache path for the owned request.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`PersonalCacheRoot::lookup_thumbnail_path`].
    pub fn lookup_path(self) -> Result<PersonalThumbnailLookup<ThumbnailPathLookupEntry>> {
        let Self {
            root,
            original,
            size,
        } = self;
        root.lookup_thumbnail_path(&original, size)
    }

    /// Returns exact validated personal-cache PNG bytes for the owned request.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`PersonalCacheRoot::lookup_thumbnail_png_bytes`].
    pub fn lookup_png_bytes(self) -> Result<PersonalThumbnailLookup<ThumbnailPngBytesLookupEntry>> {
        let Self {
            root,
            original,
            size,
        } = self;
        root.lookup_thumbnail_png_bytes(&original, size)
    }

    /// Returns decoded tightly packed RGBA8 pixels for the owned request.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`PersonalCacheRoot::lookup_thumbnail_rgba8`].
    pub fn lookup_rgba8(self) -> Result<PersonalThumbnailLookup<ThumbnailRgba8LookupEntry>> {
        let Self {
            root,
            original,
            size,
        } = self;
        root.lookup_thumbnail_rgba8(&original, size)
    }

    /// Returns decoded display RGBA8 pixels for the owned request.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`PersonalCacheRoot::lookup_display_thumbnail_rgba8`].
    pub fn lookup_display_rgba8(
        self,
    ) -> Result<PersonalThumbnailLookup<DisplayThumbnailRgba8LookupEntry>> {
        let Self {
            root,
            original,
            size,
        } = self;
        root.lookup_display_thumbnail_rgba8(&original, size)
    }

    /// Splits this request into its owned parts.
    #[must_use]
    pub fn into_parts(self) -> PersonalThumbnailLookupRequestParts {
        PersonalThumbnailLookupRequestParts {
            root: self.root,
            original: self.original,
            size: self.size,
        }
    }
}

/// Owned parts of [`PersonalThumbnailLookupRequest`].
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct PersonalThumbnailLookupRequestParts {
    /// Personal thumbnail cache root.
    pub root: PersonalCacheRoot,
    /// Readability-confirmed original identity.
    pub original: ReadablePersonalOriginalIdentity,
    /// Requested thumbnail size.
    pub size: ThumbnailSize,
}

/// Owned personal-cache fallback materialization request for async or runtime-specific adapters.
///
/// Constructing this request does not perform filesystem I/O. Validation and materialization
/// happen only when [`Self::materialize_path`] or [`Self::materialize_png_bytes`] is called.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PersonalThumbnailMaterializationRequest {
    root: PersonalCacheRoot,
    original: ReadablePersonalOriginalIdentity,
    size: ThumbnailSize,
}

impl PersonalThumbnailMaterializationRequest {
    /// Creates an owned personal-cache materialization request.
    #[must_use]
    pub fn new(
        root: PersonalCacheRoot,
        original: ReadablePersonalOriginalIdentity,
        size: ThumbnailSize,
    ) -> Self {
        Self {
            root,
            original,
            size,
        }
    }

    /// Materializes the requested personal namespace and returns its path.
    ///
    /// # Errors
    ///
    /// Returns the same errors as
    /// [`PersonalCacheRoot::materialize_thumbnail_from_larger_cache_returning_path`].
    pub fn materialize_path(self) -> Result<PersonalThumbnailLookup<MaterializedThumbnailPath>> {
        let Self {
            root,
            original,
            size,
        } = self;
        root.materialize_thumbnail_from_larger_cache_returning_path(&original, size)
    }

    /// Materializes the requested personal namespace and returns final PNG bytes.
    ///
    /// # Errors
    ///
    /// Returns the same errors as
    /// [`PersonalCacheRoot::materialize_thumbnail_from_larger_cache_returning_png_bytes`].
    pub fn materialize_png_bytes(
        self,
    ) -> Result<PersonalThumbnailLookup<MaterializedThumbnailPngBytes>> {
        let Self {
            root,
            original,
            size,
        } = self;
        root.materialize_thumbnail_from_larger_cache_returning_png_bytes(&original, size)
    }

    /// Splits this request into its owned parts.
    #[must_use]
    pub fn into_parts(self) -> PersonalThumbnailMaterializationRequestParts {
        PersonalThumbnailMaterializationRequestParts {
            root: self.root,
            original: self.original,
            size: self.size,
        }
    }
}

/// Owned parts of [`PersonalThumbnailMaterializationRequest`].
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct PersonalThumbnailMaterializationRequestParts {
    /// Personal thumbnail cache root.
    pub root: PersonalCacheRoot,
    /// Readability-confirmed original identity.
    pub original: ReadablePersonalOriginalIdentity,
    /// Requested thumbnail size.
    pub size: ThumbnailSize,
}

/// Owned personal-cache install request for async or runtime-specific adapters.
///
/// Constructing this request does not perform filesystem I/O. Normalization and installation happen
/// only when [`Self::install_path`] or [`Self::install_png_bytes`] is called. Constructing a
/// [`ReadablePersonalOriginalIdentity`] from a local path performs blocking filesystem I/O, so async callers
/// should do that inside their runtime's blocking adapter too.
///
/// ```no_run
/// use xdg_thumbnail::{
///     PersonalCacheRoot, PersonalThumbnailInstallRequest, ReadablePersonalOriginalIdentity, ThumbnailSize,
/// };
///
/// fn spawn_blocking<F, R>(operation: F) -> R
/// where
///     F: FnOnce() -> R + Send + 'static,
///     R: Send + 'static,
/// {
///     operation()
/// }
///
/// fn render_thumbnail_png() -> Vec<u8> {
///     unimplemented!("return PNG bytes produced by the caller's renderer")
/// }
///
/// fn main() -> xdg_thumbnail::Result<()> {
///     let root = PersonalCacheRoot::resolve_from_env()?;
///     let rendered_png = render_thumbnail_png();
///
///     let installed = spawn_blocking(move || {
///         let original =
///             ReadablePersonalOriginalIdentity::from_local_path("/home/alice/Pictures/photo.png")?;
///         let request = PersonalThumbnailInstallRequest::new(
///             root,
///             original,
///             ThumbnailSize::Normal,
///             rendered_png,
///         );
///         request.install_png_bytes()
///     })?;
///     let _path = installed.path();
///     Ok(())
/// }
/// ```
#[derive(Debug, Eq, PartialEq)]
pub struct PersonalThumbnailInstallRequest {
    root: PersonalCacheRoot,
    original: ReadablePersonalOriginalIdentity,
    size: ThumbnailSize,
    rendered_png: Vec<u8>,
}

impl PersonalThumbnailInstallRequest {
    /// Creates an owned personal-cache install request.
    #[must_use]
    pub fn new(
        root: PersonalCacheRoot,
        original: ReadablePersonalOriginalIdentity,
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
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`PersonalCacheRoot::install_thumbnail_returning_path`].
    pub fn install_path(self) -> Result<InstalledThumbnailPath> {
        let Self {
            root,
            original,
            size,
            rendered_png,
        } = self;
        root.install_thumbnail_returning_path(&original, size, &rendered_png)
    }

    /// Normalizes rendered PNG data, installs a personal-cache thumbnail, and returns final PNG bytes.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`PersonalCacheRoot::install_thumbnail_returning_png_bytes`].
    pub fn install_png_bytes(self) -> Result<InstalledThumbnailPngBytes> {
        let Self {
            root,
            original,
            size,
            rendered_png,
        } = self;
        root.install_thumbnail_returning_png_bytes(&original, size, &rendered_png)
    }

    /// Splits this request into its owned parts.
    #[must_use]
    pub fn into_parts(self) -> PersonalThumbnailInstallRequestParts {
        PersonalThumbnailInstallRequestParts {
            root: self.root,
            original: self.original,
            size: self.size,
            rendered_png: self.rendered_png,
        }
    }
}

/// Owned parts of [`PersonalThumbnailInstallRequest`].
#[derive(Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct PersonalThumbnailInstallRequestParts {
    /// Personal thumbnail cache root.
    pub root: PersonalCacheRoot,
    /// Readability-confirmed original identity.
    pub original: ReadablePersonalOriginalIdentity,
    /// Requested thumbnail size.
    pub size: ThumbnailSize,
    /// Caller-rendered PNG bytes.
    pub rendered_png: Vec<u8>,
}

/// Owned personal-cache raw install request for async or runtime-specific adapters.
///
/// Constructing this request does not perform filesystem I/O. Raw conversion, normalization, and
/// installation happen only when [`Self::install_path`] or [`Self::install_png_bytes`] is called.
#[derive(Debug, Eq, PartialEq)]
pub struct PersonalThumbnailRawInstallRequest {
    root: PersonalCacheRoot,
    original: ReadablePersonalOriginalIdentity,
    size: ThumbnailSize,
    image: OwnedRawThumbnailImage,
}

impl PersonalThumbnailRawInstallRequest {
    /// Creates an owned personal-cache raw install request.
    #[must_use]
    pub fn new(
        root: PersonalCacheRoot,
        original: ReadablePersonalOriginalIdentity,
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
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`PersonalCacheRoot::install_raw_thumbnail_returning_path`].
    pub fn install_path(self) -> Result<InstalledThumbnailPath> {
        let Self {
            root,
            original,
            size,
            image,
        } = self;
        root.install_raw_thumbnail_returning_path(&original, size, image.as_borrowed())
    }

    /// Normalizes raw pixel data, installs a personal-cache thumbnail, and returns final PNG bytes.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`PersonalCacheRoot::install_raw_thumbnail_returning_png_bytes`].
    pub fn install_png_bytes(self) -> Result<InstalledThumbnailPngBytes> {
        let Self {
            root,
            original,
            size,
            image,
        } = self;
        root.install_raw_thumbnail_returning_png_bytes(&original, size, image.as_borrowed())
    }

    /// Splits this request into its owned parts.
    #[must_use]
    pub fn into_parts(self) -> PersonalThumbnailRawInstallRequestParts {
        PersonalThumbnailRawInstallRequestParts {
            root: self.root,
            original: self.original,
            size: self.size,
            image: self.image,
        }
    }
}

/// Owned parts of [`PersonalThumbnailRawInstallRequest`].
#[derive(Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct PersonalThumbnailRawInstallRequestParts {
    /// Personal thumbnail cache root.
    pub root: PersonalCacheRoot,
    /// Readability-confirmed original identity.
    pub original: ReadablePersonalOriginalIdentity,
    /// Requested thumbnail size.
    pub size: ThumbnailSize,
    /// Validated raw thumbnail image.
    pub image: OwnedRawThumbnailImage,
}

/// Owned failure-entry write request for async or runtime-specific adapters.
///
/// Constructing this request does not perform filesystem I/O. The failure entry is written only
/// when [`Self::write_path`] or [`Self::write_png_bytes`] is called.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FailureEntryWriteRequest {
    root: PersonalCacheRoot,
    original: ReadablePersonalOriginalIdentity,
    namespace: FailureNamespace,
}

impl FailureEntryWriteRequest {
    /// Creates an owned failure-entry write request.
    #[must_use]
    pub fn new(
        root: PersonalCacheRoot,
        original: ReadablePersonalOriginalIdentity,
        namespace: FailureNamespace,
    ) -> Self {
        Self {
            root,
            original,
            namespace,
        }
    }

    /// Writes a deterministic 1x1 transparent failure entry and returns its path.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`PersonalCacheRoot::write_failure_entry_returning_path`].
    pub fn write_path(self) -> Result<InstalledThumbnailPath> {
        let Self {
            root,
            original,
            namespace,
        } = self;
        root.write_failure_entry_returning_path(&original, &namespace)
    }

    /// Writes a deterministic 1x1 transparent failure entry and returns final PNG bytes.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`PersonalCacheRoot::write_failure_entry_returning_png_bytes`].
    pub fn write_png_bytes(self) -> Result<InstalledThumbnailPngBytes> {
        let Self {
            root,
            original,
            namespace,
        } = self;
        root.write_failure_entry_returning_png_bytes(&original, &namespace)
    }

    /// Splits this request into its owned parts.
    #[must_use]
    pub fn into_parts(self) -> FailureEntryWriteRequestParts {
        FailureEntryWriteRequestParts {
            root: self.root,
            original: self.original,
            namespace: self.namespace,
        }
    }
}

/// Owned parts of [`FailureEntryWriteRequest`].
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct FailureEntryWriteRequestParts {
    /// Personal thumbnail cache root.
    pub root: PersonalCacheRoot,
    /// Readability-confirmed original identity.
    pub original: ReadablePersonalOriginalIdentity,
    /// Failure-entry namespace.
    pub namespace: FailureNamespace,
}

/// Owned personal-cache inspection request for async or runtime-specific adapters.
///
/// Constructing this request does not perform filesystem I/O. Inspection happens only when
/// [`Self::inspect`] is called.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PersonalThumbnailInspectionRequest {
    root: PersonalCacheRoot,
    sizes: Vec<ThumbnailSize>,
    nonstandard_entry_policy: NonstandardEntryPolicy,
}

impl PersonalThumbnailInspectionRequest {
    /// Creates an owned personal-cache inspection request.
    #[must_use]
    pub fn new(
        root: PersonalCacheRoot,
        sizes: Vec<ThumbnailSize>,
        nonstandard_entry_policy: NonstandardEntryPolicy,
    ) -> Self {
        Self {
            root,
            sizes,
            nonstandard_entry_policy,
        }
    }

    /// Inspects standard successful thumbnail size directories.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`PersonalCacheRoot::inspect_thumbnails`].
    pub fn inspect(self) -> Result<Vec<CacheEntryInspection>> {
        let Self {
            root,
            sizes,
            nonstandard_entry_policy,
        } = self;
        root.inspect_thumbnails(&sizes, nonstandard_entry_policy)
    }

    /// Splits this request into its owned parts.
    #[must_use]
    pub fn into_parts(self) -> PersonalThumbnailInspectionRequestParts {
        PersonalThumbnailInspectionRequestParts {
            root: self.root,
            sizes: self.sizes,
            nonstandard_entry_policy: self.nonstandard_entry_policy,
        }
    }
}

/// Owned parts of [`PersonalThumbnailInspectionRequest`].
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct PersonalThumbnailInspectionRequestParts {
    /// Personal thumbnail cache root.
    pub root: PersonalCacheRoot,
    /// Successful thumbnail sizes to inspect.
    pub sizes: Vec<ThumbnailSize>,
    /// Policy for nonstandard cache directory entries.
    pub nonstandard_entry_policy: NonstandardEntryPolicy,
}

/// Owned failure-entry inspection request for async or runtime-specific adapters.
///
/// Constructing this request does not perform filesystem I/O. Inspection happens only when
/// [`Self::inspect`] is called.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FailureEntryInspectionRequest {
    root: PersonalCacheRoot,
    nonstandard_entry_policy: NonstandardEntryPolicy,
}

impl FailureEntryInspectionRequest {
    /// Creates an owned failure-entry inspection request.
    #[must_use]
    pub fn new(root: PersonalCacheRoot, nonstandard_entry_policy: NonstandardEntryPolicy) -> Self {
        Self {
            root,
            nonstandard_entry_policy,
        }
    }

    /// Inspects direct files in immediate real failure-entry namespaces.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`PersonalCacheRoot::inspect_failure_entries`].
    pub fn inspect(self) -> Result<Vec<CacheEntryInspection>> {
        let Self {
            root,
            nonstandard_entry_policy,
        } = self;
        root.inspect_failure_entries(nonstandard_entry_policy)
    }

    /// Splits this request into its owned parts.
    #[must_use]
    pub fn into_parts(self) -> FailureEntryInspectionRequestParts {
        FailureEntryInspectionRequestParts {
            root: self.root,
            nonstandard_entry_policy: self.nonstandard_entry_policy,
        }
    }
}

/// Owned parts of [`FailureEntryInspectionRequest`].
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct FailureEntryInspectionRequestParts {
    /// Personal thumbnail cache root.
    pub root: PersonalCacheRoot,
    /// Policy for nonstandard cache directory entries.
    pub nonstandard_entry_policy: NonstandardEntryPolicy,
}

/// Owned shared-repository lookup request for async or runtime-specific adapters.
///
/// Constructing this request does not perform filesystem I/O. Validation happens only when
/// [`Self::lookup_path`], [`Self::lookup_png_bytes`], or [`Self::lookup_rgba8`] is called.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SharedThumbnailLookupRequest {
    context: SharedRepositoryContext,
    original_facts: SharedOriginalFacts,
    size: ThumbnailSize,
}

impl SharedThumbnailLookupRequest {
    /// Creates an owned shared-repository lookup request.
    #[must_use]
    pub fn new(
        context: SharedRepositoryContext,
        original_facts: SharedOriginalFacts,
        size: ThumbnailSize,
    ) -> Self {
        Self {
            context,
            original_facts,
            size,
        }
    }

    /// Returns a validated shared-repository path for the owned request.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`SharedRepositoryContext::lookup_thumbnail_path`].
    pub fn lookup_path(self) -> Result<SharedThumbnailLookup<ThumbnailPathLookupEntry>> {
        let Self {
            context,
            original_facts,
            size,
        } = self;
        context.lookup_thumbnail_path(original_facts, size)
    }

    /// Returns exact validated shared-repository PNG bytes for the owned request.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`SharedRepositoryContext::lookup_thumbnail_png_bytes`].
    pub fn lookup_png_bytes(self) -> Result<SharedThumbnailLookup<ThumbnailPngBytesLookupEntry>> {
        let Self {
            context,
            original_facts,
            size,
        } = self;
        context.lookup_thumbnail_png_bytes(original_facts, size)
    }

    /// Returns decoded tightly packed RGBA8 pixels for the owned request.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`SharedRepositoryContext::lookup_thumbnail_rgba8`].
    pub fn lookup_rgba8(self) -> Result<SharedThumbnailLookup<ThumbnailRgba8LookupEntry>> {
        let Self {
            context,
            original_facts,
            size,
        } = self;
        context.lookup_thumbnail_rgba8(original_facts, size)
    }

    /// Returns decoded display RGBA8 pixels for the owned request.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`SharedRepositoryContext::lookup_display_thumbnail_rgba8`].
    pub fn lookup_display_rgba8(
        self,
    ) -> Result<SharedThumbnailLookup<DisplayThumbnailRgba8LookupEntry>> {
        let Self {
            context,
            original_facts,
            size,
        } = self;
        context.lookup_display_thumbnail_rgba8(original_facts, size)
    }

    /// Splits this request into its owned parts.
    #[must_use]
    pub fn into_parts(self) -> SharedThumbnailLookupRequestParts {
        SharedThumbnailLookupRequestParts {
            context: self.context,
            original_facts: self.original_facts,
            size: self.size,
        }
    }
}

/// Owned parts of [`SharedThumbnailLookupRequest`].
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct SharedThumbnailLookupRequestParts {
    /// Shared repository lookup context.
    pub context: SharedRepositoryContext,
    /// Shared original freshness facts and metadata policy.
    pub original_facts: SharedOriginalFacts,
    /// Requested thumbnail size.
    pub size: ThumbnailSize,
}

/// Owned shared-to-personal fallback materialization request for async adapters.
///
/// Constructing this request does not perform filesystem I/O. Validation and materialization
/// happen only when [`Self::materialize_path`] or [`Self::materialize_png_bytes`] is called.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SharedToPersonalThumbnailMaterializationRequest {
    personal_root: PersonalCacheRoot,
    shared_context: SharedRepositoryContext,
    original_facts: SharedOriginalFacts,
    size: ThumbnailSize,
}

impl SharedToPersonalThumbnailMaterializationRequest {
    /// Creates an owned shared-to-personal materialization request.
    #[must_use]
    pub fn new(
        personal_root: PersonalCacheRoot,
        shared_context: SharedRepositoryContext,
        original_facts: SharedOriginalFacts,
        size: ThumbnailSize,
    ) -> Self {
        Self {
            personal_root,
            shared_context,
            original_facts,
            size,
        }
    }

    /// Materializes the requested shared thumbnail into the personal cache and returns its path.
    ///
    /// # Errors
    ///
    /// Returns the same errors as
    /// [`PersonalCacheRoot::materialize_shared_thumbnail_returning_path`].
    pub fn materialize_path(self) -> Result<SharedThumbnailLookup<MaterializedThumbnailPath>> {
        let Self {
            personal_root,
            shared_context,
            original_facts,
            size,
        } = self;
        personal_root.materialize_shared_thumbnail_returning_path(
            &shared_context,
            original_facts,
            size,
        )
    }

    /// Materializes the requested shared thumbnail into the personal cache and returns final PNG bytes.
    ///
    /// # Errors
    ///
    /// Returns the same errors as
    /// [`PersonalCacheRoot::materialize_shared_thumbnail_returning_png_bytes`].
    pub fn materialize_png_bytes(
        self,
    ) -> Result<SharedThumbnailLookup<MaterializedThumbnailPngBytes>> {
        let Self {
            personal_root,
            shared_context,
            original_facts,
            size,
        } = self;
        personal_root.materialize_shared_thumbnail_returning_png_bytes(
            &shared_context,
            original_facts,
            size,
        )
    }

    /// Splits this request into its owned parts.
    #[must_use]
    pub fn into_parts(self) -> SharedToPersonalThumbnailMaterializationRequestParts {
        SharedToPersonalThumbnailMaterializationRequestParts {
            personal_root: self.personal_root,
            shared_context: self.shared_context,
            original_facts: self.original_facts,
            size: self.size,
        }
    }
}

/// Owned parts of [`SharedToPersonalThumbnailMaterializationRequest`].
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct SharedToPersonalThumbnailMaterializationRequestParts {
    /// Destination personal thumbnail cache root.
    pub personal_root: PersonalCacheRoot,
    /// Source shared repository lookup context.
    pub shared_context: SharedRepositoryContext,
    /// Shared original freshness facts and metadata policy.
    pub original_facts: SharedOriginalFacts,
    /// Requested thumbnail size.
    pub size: ThumbnailSize,
}

/// Owned shared-repository inspection request for async or runtime-specific adapters.
///
/// Constructing this request does not perform filesystem I/O. Inspection happens only when
/// [`Self::inspect`] is called.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SharedThumbnailInspectionRequest {
    context: SharedRepositoryContext,
    sizes: Vec<ThumbnailSize>,
    original: SharedOriginalMetadata,
}

impl SharedThumbnailInspectionRequest {
    /// Creates an owned shared-repository inspection request.
    #[must_use]
    pub fn new(
        context: SharedRepositoryContext,
        sizes: Vec<ThumbnailSize>,
        original: SharedOriginalMetadata,
    ) -> Self {
        Self {
            context,
            sizes,
            original,
        }
    }

    /// Inspects existing shared-repository thumbnails without exposing removal handles.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`SharedRepositoryContext::inspect_thumbnails`].
    pub fn inspect(self) -> Result<Vec<SharedCacheEntryInspection>> {
        let Self {
            context,
            sizes,
            original,
        } = self;
        context.inspect_thumbnails(&sizes, original)
    }

    /// Splits this request into its owned parts.
    #[must_use]
    pub fn into_parts(self) -> SharedThumbnailInspectionRequestParts {
        SharedThumbnailInspectionRequestParts {
            context: self.context,
            sizes: self.sizes,
            original: self.original,
        }
    }
}

/// Owned parts of [`SharedThumbnailInspectionRequest`].
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct SharedThumbnailInspectionRequestParts {
    /// Shared repository lookup context.
    pub context: SharedRepositoryContext,
    /// Successful thumbnail sizes to inspect.
    pub sizes: Vec<ThumbnailSize>,
    /// Policy-neutral shared original metadata facts.
    pub original: SharedOriginalMetadata,
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
        Err(source) => {
            return Err(ThumbnailError::io(context, Some(path.to_owned()), source));
        }
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
        Err(rustix::io::Errno::ACCESS | rustix::io::Errno::PERM) => {
            return Ok(CacheEntryRead::Unreadable);
        }
        Err(source) => {
            return Err(ThumbnailError::io(
                context,
                Some(path.to_owned()),
                std::io::Error::from(source),
            ));
        }
    };

    let stat = rustix::fs::fstat(&fd).map_err(|source| {
        ThumbnailError::io(context, Some(path.to_owned()), std::io::Error::from(source))
    })?;
    let file_type = rustix::fs::FileType::from_raw_mode(stat.st_mode);
    if !file_type.is_file() {
        return Ok(CacheEntryRead::Unreadable);
    }

    let mut file = File::from(fd);
    let mut bytes = Vec::new();
    if let Err(source) = file.read_to_end(&mut bytes) {
        if source.kind() == std::io::ErrorKind::PermissionDenied {
            return Ok(CacheEntryRead::Unreadable);
        }
        return Err(ThumbnailError::io(context, Some(path.to_owned()), source));
    }
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

struct MaterializedPersonalEntry {
    target_path: PathBuf,
    source_path: PathBuf,
    requested_size: ThumbnailSize,
    source_size: ThumbnailSize,
    written: bool,
    bytes: Vec<u8>,
}

fn display_candidate_sizes(requested_size: ThumbnailSize) -> impl Iterator<Item = ThumbnailSize> {
    ThumbnailSize::all()
        .iter()
        .copied()
        .skip_while(move |size| *size != requested_size)
}

fn personal_identity_from_shared_facts(
    shared: &SharedRepositoryContext,
    original_facts: SharedOriginalFacts,
) -> Result<PersonalOriginalIdentity> {
    let Some(mtime) = original_facts.mtime() else {
        return Err(ThumbnailError::invalid_metadata(
            "shared original mtime is required for personal materialization",
        ));
    };
    let original_path = shared.repository_root().join(shared.original_child_name());
    let uri = PersonalOriginalUri::from_absolute_path_bytes(original_path.as_os_str().as_bytes())?;
    let mut original = PersonalOriginalIdentity::new(uri, mtime);
    if let Some(size) = original_facts.original_byte_size() {
        original = original.with_original_byte_size(size);
    }
    Ok(original)
}

fn rgba8_lookup_entry_from_parts(
    path: PathBuf,
    bytes: &[u8],
    metadata: ThumbnailMetadata,
) -> Result<ThumbnailRgba8LookupEntry> {
    let decoded = decode_validated_thumbnail_png_to_rgba8(bytes)?;
    Ok(ThumbnailRgba8LookupEntry {
        path,
        width: decoded.width,
        height: decoded.height,
        stride: decoded.stride,
        pixels: decoded.pixels,
        metadata,
    })
}

fn display_rgba8_lookup_entry_from_parts(
    source_path: PathBuf,
    bytes: &[u8],
    source_metadata: ThumbnailMetadata,
    requested_size: ThumbnailSize,
    source_size: ThumbnailSize,
) -> Result<DisplayThumbnailRgba8LookupEntry> {
    let decoded = downscaled_validated_thumbnail_png_to_rgba8(bytes, requested_size)?;
    Ok(DisplayThumbnailRgba8LookupEntry {
        source_path,
        requested_size,
        source_size,
        width: decoded.width,
        height: decoded.height,
        stride: decoded.stride,
        pixels: decoded.pixels,
        source_metadata,
    })
}

/// Result of a validated personal thumbnail cache lookup.
#[derive(Clone, Debug, Eq, PartialEq)]
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

/// Policy-neutral original freshness facts for shared-repository validation and inspection.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct SharedOriginalMetadata {
    mtime: Option<UnixMtimeSeconds>,
    original_byte_size: Option<u64>,
}

impl SharedOriginalMetadata {
    /// Creates empty shared original metadata facts.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            mtime: None,
            original_byte_size: None,
        }
    }

    /// Adds a known original modification time.
    #[must_use]
    pub const fn with_mtime(mut self, mtime: UnixMtimeSeconds) -> Self {
        self.mtime = Some(mtime);
        self
    }

    /// Adds a known original byte size.
    #[must_use]
    pub const fn with_original_byte_size(mut self, original_byte_size: u64) -> Self {
        self.original_byte_size = Some(original_byte_size);
        self
    }

    /// Returns the original modification time when known.
    #[must_use]
    pub const fn mtime(&self) -> Option<UnixMtimeSeconds> {
        self.mtime
    }

    /// Returns the original byte size when known.
    #[must_use]
    pub const fn original_byte_size(&self) -> Option<u64> {
        self.original_byte_size
    }
}

/// Shared-repository lookup facts, including the metadata acceptance policy.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SharedOriginalFacts {
    metadata_policy: SharedThumbnailMetadataPolicy,
    metadata: SharedOriginalMetadata,
}

impl SharedOriginalFacts {
    /// Creates shared lookup facts from a metadata policy and policy-neutral metadata facts.
    #[must_use]
    pub const fn new(
        metadata_policy: SharedThumbnailMetadataPolicy,
        metadata: SharedOriginalMetadata,
    ) -> Self {
        Self {
            metadata_policy,
            metadata,
        }
    }

    /// Returns the shared lookup metadata acceptance policy.
    #[must_use]
    pub const fn metadata_policy(&self) -> SharedThumbnailMetadataPolicy {
        self.metadata_policy
    }

    /// Returns the original modification time when known.
    #[must_use]
    pub const fn mtime(&self) -> Option<UnixMtimeSeconds> {
        self.metadata.mtime()
    }

    /// Returns the original byte size when known.
    #[must_use]
    pub const fn original_byte_size(&self) -> Option<u64> {
        self.metadata.original_byte_size()
    }

    /// Returns policy-neutral shared original metadata facts.
    #[must_use]
    pub const fn metadata(&self) -> SharedOriginalMetadata {
        self.metadata
    }
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

    /// Splits this inspection into its owned facts.
    #[must_use]
    pub fn into_parts(self) -> SharedCacheEntryInspectionParts {
        SharedCacheEntryInspectionParts {
            outcome: self.outcome,
            shared_uri: self.shared_uri,
            timestamps: self.timestamps,
            size: self.size,
            path: self.path,
            metadata: self.metadata,
        }
    }
}

/// Owned parts of [`SharedCacheEntryInspection`].
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct SharedCacheEntryInspectionParts {
    /// Shared validation or inspection outcome.
    pub outcome: SharedCacheEntryOutcome,
    /// Shared relative URI used for hashing and metadata comparison.
    pub shared_uri: SharedRelativeOriginalUri,
    /// Timestamp facts.
    pub timestamps: ThumbnailTimestamps,
    /// Successful-thumbnail size namespace.
    pub size: ThumbnailSize,
    /// Inspected shared cache path.
    pub path: PathBuf,
    /// Parsed metadata when the entry was a readable PNG.
    pub metadata: Option<ThumbnailMetadata>,
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
    pub fn into_parts(self) -> ThumbnailPathLookupEntryParts {
        ThumbnailPathLookupEntryParts {
            path: self.path,
            metadata: self.metadata,
        }
    }
}

/// Owned parts of [`ThumbnailPathLookupEntry`].
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct ThumbnailPathLookupEntryParts {
    /// Path that was validated.
    pub path: PathBuf,
    /// Metadata parsed from the validated PNG.
    pub metadata: ThumbnailMetadata,
}

/// Exact validated PNG bytes and metadata facts.
#[derive(Debug, Eq, PartialEq)]
pub struct ThumbnailPngBytesLookupEntry {
    path: PathBuf,
    bytes: Vec<u8>,
    metadata: ThumbnailMetadata,
}

impl ThumbnailPngBytesLookupEntry {
    /// Returns the path from which the PNG bytes were validated.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the exact PNG bytes that passed validation.
    #[must_use]
    pub fn png_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Returns metadata parsed from the validated PNG.
    #[must_use]
    pub const fn metadata(&self) -> &ThumbnailMetadata {
        &self.metadata
    }

    /// Splits this result into its owned path, PNG bytes, and metadata.
    #[must_use]
    pub fn into_parts(self) -> ThumbnailPngBytesLookupEntryParts {
        ThumbnailPngBytesLookupEntryParts {
            path: self.path,
            png_bytes: self.bytes,
            metadata: self.metadata,
        }
    }
}

/// Owned parts of [`ThumbnailPngBytesLookupEntry`].
#[derive(Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct ThumbnailPngBytesLookupEntryParts {
    /// Path from which the PNG bytes were validated.
    pub path: PathBuf,
    /// Exact PNG bytes that passed validation.
    pub png_bytes: Vec<u8>,
    /// Metadata parsed from the validated PNG.
    pub metadata: ThumbnailMetadata,
}

/// Decoded tightly packed RGBA8 pixels and metadata facts from a validated cache PNG.
///
/// Pixels are row-major `[red, green, blue, alpha]` bytes with straight alpha and
/// `stride == width * 4`.
#[derive(Debug, Eq, PartialEq)]
pub struct ThumbnailRgba8LookupEntry {
    path: PathBuf,
    width: u32,
    height: u32,
    stride: usize,
    pixels: Vec<u8>,
    metadata: ThumbnailMetadata,
}

impl ThumbnailRgba8LookupEntry {
    /// Returns the path from which the PNG was validated and decoded.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the decoded image width in pixels.
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Returns the decoded image height in pixels.
    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// Returns the row stride in bytes.
    ///
    /// RGBA8 lookup results are tightly packed, so this is always `width * 4`.
    #[must_use]
    pub const fn stride(&self) -> usize {
        self.stride
    }

    /// Returns the decoded row-major RGBA8 pixel buffer.
    #[must_use]
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// Returns metadata parsed from the validated PNG.
    #[must_use]
    pub const fn metadata(&self) -> &ThumbnailMetadata {
        &self.metadata
    }

    /// Splits this result into its owned path, dimensions, stride, RGBA8 pixels, and metadata.
    #[must_use]
    pub fn into_parts(self) -> ThumbnailRgba8LookupEntryParts {
        ThumbnailRgba8LookupEntryParts {
            path: self.path,
            width: self.width,
            height: self.height,
            stride: self.stride,
            pixels: self.pixels,
            metadata: self.metadata,
        }
    }
}

/// Owned parts of [`ThumbnailRgba8LookupEntry`].
#[derive(Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct ThumbnailRgba8LookupEntryParts {
    /// Path from which the PNG was validated and decoded.
    pub path: PathBuf,
    /// Decoded image width in pixels.
    pub width: u32,
    /// Decoded image height in pixels.
    pub height: u32,
    /// Row stride in bytes.
    pub stride: usize,
    /// Decoded row-major RGBA8 pixel buffer.
    pub pixels: Vec<u8>,
    /// Metadata parsed from the validated PNG.
    pub metadata: ThumbnailMetadata,
}

/// Decoded display RGBA8 pixels from an exact or larger validated cache PNG.
///
/// Pixels are row-major `[red, green, blue, alpha]` bytes with straight alpha and
/// `stride == width * 4`. When `source_size()` differs from `requested_size()`, pixels were
/// derived from a larger source namespace and constrained to the requested namespace dimensions.
#[derive(Debug, Eq, PartialEq)]
pub struct DisplayThumbnailRgba8LookupEntry {
    source_path: PathBuf,
    requested_size: ThumbnailSize,
    source_size: ThumbnailSize,
    width: u32,
    height: u32,
    stride: usize,
    pixels: Vec<u8>,
    source_metadata: ThumbnailMetadata,
}

impl DisplayThumbnailRgba8LookupEntry {
    /// Returns the source cache path that was validated and decoded.
    #[must_use]
    pub fn source_path(&self) -> &Path {
        &self.source_path
    }

    /// Returns the requested display size namespace.
    #[must_use]
    pub const fn requested_size(&self) -> ThumbnailSize {
        self.requested_size
    }

    /// Returns the cache namespace that supplied the source PNG.
    #[must_use]
    pub const fn source_size(&self) -> ThumbnailSize {
        self.source_size
    }

    /// Returns whether the display pixels came from a larger namespace than requested.
    #[must_use]
    pub const fn is_derived(&self) -> bool {
        self.requested_size as u8 != self.source_size as u8
    }

    /// Returns the decoded display width in pixels.
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Returns the decoded display height in pixels.
    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// Returns the row stride in bytes.
    #[must_use]
    pub const fn stride(&self) -> usize {
        self.stride
    }

    /// Returns the decoded row-major RGBA8 display pixel buffer.
    #[must_use]
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// Returns metadata parsed from the source PNG.
    #[must_use]
    pub const fn source_metadata(&self) -> &ThumbnailMetadata {
        &self.source_metadata
    }

    /// Splits this result into its owned source path, size facts, pixels, and source metadata.
    #[must_use]
    pub fn into_parts(self) -> DisplayThumbnailRgba8LookupEntryParts {
        DisplayThumbnailRgba8LookupEntryParts {
            source_path: self.source_path,
            requested_size: self.requested_size,
            source_size: self.source_size,
            width: self.width,
            height: self.height,
            stride: self.stride,
            pixels: self.pixels,
            source_metadata: self.source_metadata,
        }
    }
}

/// Owned parts of [`DisplayThumbnailRgba8LookupEntry`].
#[derive(Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct DisplayThumbnailRgba8LookupEntryParts {
    /// Source cache path that was validated and decoded.
    pub source_path: PathBuf,
    /// Requested display size namespace.
    pub requested_size: ThumbnailSize,
    /// Cache namespace that supplied the source PNG.
    pub source_size: ThumbnailSize,
    /// Decoded display width in pixels.
    pub width: u32,
    /// Decoded display height in pixels.
    pub height: u32,
    /// Row stride in bytes.
    pub stride: usize,
    /// Decoded row-major RGBA8 display pixel buffer.
    pub pixels: Vec<u8>,
    /// Metadata parsed from the source PNG.
    pub source_metadata: ThumbnailMetadata,
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

impl AsRef<Path> for InstalledThumbnailPath {
    fn as_ref(&self) -> &Path {
        self.path()
    }
}

/// Path result of explicit fallback materialization into the personal cache.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MaterializedThumbnailPath {
    target_path: PathBuf,
    source_path: PathBuf,
    requested_size: ThumbnailSize,
    source_size: ThumbnailSize,
    written: bool,
}

impl MaterializedThumbnailPath {
    /// Returns the requested personal-cache target path.
    #[must_use]
    pub fn target_path(&self) -> &Path {
        &self.target_path
    }

    /// Returns the source cache path used for materialization.
    #[must_use]
    pub fn source_path(&self) -> &Path {
        &self.source_path
    }

    /// Returns the requested target size namespace.
    #[must_use]
    pub const fn requested_size(&self) -> ThumbnailSize {
        self.requested_size
    }

    /// Returns the source size namespace.
    #[must_use]
    pub const fn source_size(&self) -> ThumbnailSize {
        self.source_size
    }

    /// Returns whether this operation wrote a new target file.
    #[must_use]
    pub const fn written(&self) -> bool {
        self.written
    }

    /// Splits this result into its owned target path, source path, size facts, and write status.
    #[must_use]
    pub fn into_parts(self) -> MaterializedThumbnailPathParts {
        MaterializedThumbnailPathParts {
            target_path: self.target_path,
            source_path: self.source_path,
            requested_size: self.requested_size,
            source_size: self.source_size,
            written: self.written,
        }
    }
}

/// Owned parts of [`MaterializedThumbnailPath`].
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct MaterializedThumbnailPathParts {
    /// Requested personal-cache target path.
    pub target_path: PathBuf,
    /// Source cache path used for materialization.
    pub source_path: PathBuf,
    /// Requested target size namespace.
    pub requested_size: ThumbnailSize,
    /// Source size namespace.
    pub source_size: ThumbnailSize,
    /// Whether this operation wrote a new target file.
    pub written: bool,
}

/// PNG bytes result of explicit fallback materialization into the personal cache.
///
/// Returned bytes are the final target personal-cache PNG bytes, not the source cache bytes.
#[derive(Debug, Eq, PartialEq)]
pub struct MaterializedThumbnailPngBytes {
    target_path: PathBuf,
    source_path: PathBuf,
    requested_size: ThumbnailSize,
    source_size: ThumbnailSize,
    written: bool,
    bytes: Vec<u8>,
}

impl MaterializedThumbnailPngBytes {
    /// Returns the requested personal-cache target path.
    #[must_use]
    pub fn target_path(&self) -> &Path {
        &self.target_path
    }

    /// Returns the source cache path used for materialization.
    #[must_use]
    pub fn source_path(&self) -> &Path {
        &self.source_path
    }

    /// Returns the requested target size namespace.
    #[must_use]
    pub const fn requested_size(&self) -> ThumbnailSize {
        self.requested_size
    }

    /// Returns the source size namespace.
    #[must_use]
    pub const fn source_size(&self) -> ThumbnailSize {
        self.source_size
    }

    /// Returns whether this operation wrote a new target file.
    #[must_use]
    pub const fn written(&self) -> bool {
        self.written
    }

    /// Returns the final target personal-cache PNG bytes.
    #[must_use]
    pub fn png_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Splits this result into its owned target path, source path, size facts, write status, and PNG bytes.
    #[must_use]
    pub fn into_parts(self) -> MaterializedThumbnailPngBytesParts {
        MaterializedThumbnailPngBytesParts {
            target_path: self.target_path,
            source_path: self.source_path,
            requested_size: self.requested_size,
            source_size: self.source_size,
            written: self.written,
            png_bytes: self.bytes,
        }
    }
}

/// Owned parts of [`MaterializedThumbnailPngBytes`].
#[derive(Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct MaterializedThumbnailPngBytesParts {
    /// Requested personal-cache target path.
    pub target_path: PathBuf,
    /// Source cache path used for materialization.
    pub source_path: PathBuf,
    /// Requested target size namespace.
    pub requested_size: ThumbnailSize,
    /// Source size namespace.
    pub source_size: ThumbnailSize,
    /// Whether this operation wrote a new target file.
    pub written: bool,
    /// Final target personal-cache PNG bytes.
    pub png_bytes: Vec<u8>,
}

/// PNG bytes result of a successful personal-cache install or failure-entry write.
///
/// The returned bytes are the final PNG bytes published to the cache after metadata writing and
/// normalization. Installation metadata is determined from the supplied original facts; callers
/// that need to inspect the installed metadata can parse these bytes with [`ParsedThumbnailPng`].
#[derive(Debug, Eq, PartialEq)]
pub struct InstalledThumbnailPngBytes {
    path: PathBuf,
    bytes: Vec<u8>,
}

impl InstalledThumbnailPngBytes {
    /// Returns the installed cache path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the final normalized PNG bytes that were installed.
    #[must_use]
    pub fn png_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Splits this result into its owned path and final PNG bytes.
    #[must_use]
    pub fn into_parts(self) -> InstalledThumbnailPngBytesParts {
        InstalledThumbnailPngBytesParts {
            path: self.path,
            png_bytes: self.bytes,
        }
    }
}

/// Owned parts of [`InstalledThumbnailPngBytes`].
#[derive(Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct InstalledThumbnailPngBytesParts {
    /// Installed cache path.
    pub path: PathBuf,
    /// Final normalized PNG bytes that were installed.
    pub png_bytes: Vec<u8>,
}

fn ensure_private_directory(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            let problem = if metadata.file_type().is_symlink() {
                Some(CacheDirectoryProblem::Symlink)
            } else if !metadata.is_dir() {
                Some(CacheDirectoryProblem::NotDirectory)
            } else if metadata.uid() != rustix::process::getuid().as_raw() {
                Some(CacheDirectoryProblem::WrongOwner)
            } else if metadata.permissions().mode() & 0o077 != 0 {
                Some(CacheDirectoryProblem::GroupOrOtherAccessible)
            } else {
                None
            };
            if let Some(problem) = problem {
                return Err(ThumbnailError::InsecureCacheDirectory {
                    path: path.to_owned(),
                    problem,
                });
            }
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|source| {
                    ThumbnailError::io(
                        "create parent thumbnail cache directories",
                        Some(parent.to_owned()),
                        source,
                    )
                })?;
            }
            match fs::DirBuilder::new().mode(0o700).create(path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    return ensure_private_directory(path);
                }
                Err(source) => {
                    return Err(ThumbnailError::io(
                        "create thumbnail cache directory",
                        Some(path.to_owned()),
                        source,
                    ));
                }
            }
            fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|source| {
                ThumbnailError::io(
                    "set thumbnail cache directory permissions",
                    Some(path.to_owned()),
                    source,
                )
            })?;
            Ok(())
        }
        Err(source) => Err(ThumbnailError::io(
            "inspect thumbnail cache directory",
            Some(path.to_owned()),
            source,
        )),
    }
}
