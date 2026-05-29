// SPDX-FileCopyrightText: 2026 KIM Hyunjae
// SPDX-License-Identifier: MPL-2.0

use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use std::os::unix::fs::MetadataExt;

use crate::{
    CacheEntryProblem, CacheNamespace, FailureNamespace, ParsedThumbnailPng, PersonalCacheRoot,
    PersonalOriginalUri, Result, SharedRelativeOriginalUri, ThumbnailError, ThumbnailMetadata,
    ThumbnailSize, push_problem, validate_mime_type,
};

/// Original URI identity parsed from a cache entry.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum OriginalUriIdentity {
    /// Absolute personal-cache URI identity.
    Personal(PersonalOriginalUri),
    /// Shared repository relative URI identity.
    Shared(SharedRelativeOriginalUri),
}

/// Validation confidence and validity for policy-neutral cache inspection.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum CacheEntryInspectionOutcome {
    /// Inspection parsed the entry but did not validate it against an original.
    Unchecked,
    /// The entry is invalid for inspection or cache-management use.
    Invalid(Vec<CacheEntryProblem>),
}

/// Whether access time was preserved while inspecting an entry.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
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
    outcome: CacheEntryInspectionOutcome,
    original_uri: Option<OriginalUriIdentity>,
    timestamps: ThumbnailTimestamps,
    namespace: CacheNamespace,
    path: PathBuf,
    handle: CacheEntryHandle,
}

impl CacheEntryInspection {
    /// Returns the validation or inspection outcome.
    #[must_use]
    pub const fn outcome(&self) -> &CacheEntryInspectionOutcome {
        &self.outcome
    }

    /// Returns the original URI parsed from metadata when present and valid.
    #[must_use]
    pub const fn original_uri(&self) -> Option<&OriginalUriIdentity> {
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
        remove_cache_entry_handle(self)
    }

    /// Returns the handled path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl PersonalCacheRoot {
    /// Inspects standard successful thumbnail size directories.
    pub fn inspect_thumbnails(
        &self,
        sizes: &[ThumbnailSize],
        include_nonstandard: bool,
    ) -> Result<Vec<CacheEntryInspection>> {
        let mut inspections = Vec::new();
        for &size in sizes {
            let namespace = CacheNamespace::Size(size);
            let dir = self.as_path().join(size.directory_name());
            inspect_namespace_dir(
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
        let fail_root = self.as_path().join("fail");
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
            inspect_namespace_dir(
                &entry.path(),
                CacheNamespace::Failure(namespace),
                include_nonstandard,
                None,
                &mut inspections,
            )?;
        }
        Ok(inspections)
    }
}

fn inspect_namespace_dir(
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
                outcome: CacheEntryInspectionOutcome::Invalid(vec![
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
                outcome: CacheEntryInspectionOutcome::Invalid(vec![
                    CacheEntryProblem::UnreadableEntry,
                ]),
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
            outcome: CacheEntryInspectionOutcome::Invalid(vec![CacheEntryProblem::UnreadableEntry]),
            original_uri: None,
            timestamps,
            namespace,
            path,
            handle,
        };
    }

    let (read_result, preservation) = read_thumbnail_for_inspection(&path);
    timestamps = thumbnail_timestamps_from_metadata(&metadata, preservation);
    let bytes = match read_result {
        Ok(bytes) => bytes,
        Err(_) => {
            return CacheEntryInspection {
                outcome: CacheEntryInspectionOutcome::Invalid(vec![
                    CacheEntryProblem::UnreadableEntry,
                ]),
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
        Err(ThumbnailError::ResourceLimitExceeded(_)) => {
            return CacheEntryInspection {
                outcome: CacheEntryInspectionOutcome::Invalid(vec![
                    CacheEntryProblem::ResourceLimitExceeded,
                ]),
                original_uri: None,
                timestamps,
                namespace,
                path,
                handle,
            };
        }
        Err(_) => {
            return CacheEntryInspection {
                outcome: CacheEntryInspectionOutcome::Invalid(vec![
                    CacheEntryProblem::InvalidPngStructure,
                ]),
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
    let original_uri = inspect_required_metadata(&mut problems, parsed.metadata());
    if let Some(OriginalUriIdentity::Personal(uri)) = &original_uri {
        inspect_filename_uri_match(&mut problems, &path, uri);
    }
    let outcome = if problems.is_empty() {
        CacheEntryInspectionOutcome::Unchecked
    } else {
        CacheEntryInspectionOutcome::Invalid(problems)
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

fn inspect_required_metadata(
    problems: &mut Vec<CacheEntryProblem>,
    metadata: &ThumbnailMetadata,
) -> Option<OriginalUriIdentity> {
    let original_uri = match metadata.thumb_uri() {
        Some(uri) => match PersonalOriginalUri::from_validated_absolute_uri(uri) {
            Ok(uri) => Some(OriginalUriIdentity::Personal(uri)),
            Err(_) => {
                push_problem(problems, CacheEntryProblem::InvalidMetadataSyntax);
                None
            }
        },
        None => {
            push_problem(problems, CacheEntryProblem::MissingRequiredMetadata);
            None
        }
    };
    match metadata.thumb_mtime_result() {
        Ok(Some(_)) => {}
        Ok(None) => push_problem(problems, CacheEntryProblem::MissingRequiredMetadata),
        Err(_) => push_problem(problems, CacheEntryProblem::InvalidMetadataSyntax),
    }
    if metadata.thumb_size_result().is_err() {
        push_problem(problems, CacheEntryProblem::InvalidMetadataSyntax);
    }
    if let Some(mime_type) = metadata.thumb_mimetype() {
        if validate_mime_type(mime_type).is_err() {
            push_problem(problems, CacheEntryProblem::InvalidMetadataSyntax);
        }
    }
    original_uri
}

fn inspect_filename_uri_match(
    problems: &mut Vec<CacheEntryProblem>,
    path: &Path,
    uri: &PersonalOriginalUri,
) {
    let Some(filename) = path.file_name().and_then(OsStr::to_str) else {
        push_problem(problems, CacheEntryProblem::UriFilenameMismatch);
        return;
    };
    if filename != uri.thumbnail_filename() {
        push_problem(problems, CacheEntryProblem::UriFilenameMismatch);
    }
}

pub(crate) fn read_thumbnail_for_inspection(
    path: &Path,
) -> (std::io::Result<Vec<u8>>, AccessTimePreservation) {
    read_thumbnail_for_inspection_unix(path)
}

fn read_thumbnail_for_inspection_unix(
    path: &Path,
) -> (std::io::Result<Vec<u8>>, AccessTimePreservation) {
    #[cfg(any(target_os = "linux", target_os = "fuchsia"))]
    {
        let flags = rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::CLOEXEC
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::NOATIME;
        if let Ok(bytes) = read_thumbnail_with_flags(path, flags) {
            return (Ok(bytes), AccessTimePreservation::Preserved);
        }
    }

    read_thumbnail_and_restore_timestamps(path)
}

fn read_thumbnail_with_flags(path: &Path, flags: rustix::fs::OFlags) -> std::io::Result<Vec<u8>> {
    let fd =
        rustix::fs::open(path, flags, rustix::fs::Mode::empty()).map_err(std::io::Error::from)?;
    let mut file = File::from(fd);
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn read_thumbnail_and_restore_timestamps(
    path: &Path,
) -> (std::io::Result<Vec<u8>>, AccessTimePreservation) {
    let flags =
        rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::CLOEXEC | rustix::fs::OFlags::NOFOLLOW;
    let fd = match rustix::fs::open(path, flags, rustix::fs::Mode::empty()) {
        Ok(fd) => fd,
        Err(error) => {
            return (
                Err(std::io::Error::from(error)),
                AccessTimePreservation::Unsupported,
            );
        }
    };
    let mut file = File::from(fd);
    let timestamps = match file.metadata() {
        Ok(metadata) => timestamps_from_unix_metadata(&metadata),
        Err(error) => return (Err(error), AccessTimePreservation::Unsupported),
    };
    let mut bytes = Vec::new();
    if let Err(error) = file.read_to_end(&mut bytes) {
        return (Err(error), AccessTimePreservation::Unsupported);
    }

    let preservation = if rustix::fs::futimens(&file, &timestamps).is_ok() {
        AccessTimePreservation::Preserved
    } else {
        AccessTimePreservation::NotPreserved
    };
    (Ok(bytes), preservation)
}

fn timestamps_from_unix_metadata(metadata: &fs::Metadata) -> rustix::fs::Timestamps {
    rustix::fs::Timestamps {
        last_access: rustix::fs::Timespec {
            tv_sec: metadata.atime(),
            tv_nsec: metadata.atime_nsec() as _,
        },
        last_modification: rustix::fs::Timespec {
            tv_sec: metadata.mtime(),
            tv_nsec: metadata.mtime_nsec() as _,
        },
    }
}

pub(crate) fn thumbnail_timestamps(
    path: &Path,
    preservation: AccessTimePreservation,
) -> ThumbnailTimestamps {
    let (accessed_at, modified_at) = fs::symlink_metadata(path)
        .map_or((None, None), |metadata| timestamps_from_metadata(&metadata));
    ThumbnailTimestamps {
        accessed_at,
        modified_at,
        access_time_preserved_during_inspection: preservation,
    }
}

pub(crate) fn thumbnail_timestamps_from_metadata(
    metadata: &fs::Metadata,
    preservation: AccessTimePreservation,
) -> ThumbnailTimestamps {
    let (accessed_at, modified_at) = timestamps_from_metadata(metadata);
    ThumbnailTimestamps {
        accessed_at,
        modified_at,
        access_time_preserved_during_inspection: preservation,
    }
}

fn timestamps_from_metadata(metadata: &fs::Metadata) -> (Option<SystemTime>, Option<SystemTime>) {
    (metadata.accessed().ok(), metadata.modified().ok())
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

fn remove_cache_entry_handle(handle: &CacheEntryHandle) -> Result<()> {
    let filename = handle
        .path
        .file_name()
        .ok_or(ThumbnailError::UnsafeRemoval("entry has no filename"))?;
    let filename_path = Path::new(filename);
    if filename_path.components().count() != 1
        || filename == OsStr::new(".")
        || filename == OsStr::new("..")
        || handle.path.parent() != Some(handle.cache_dir.as_path())
    {
        return Err(ThumbnailError::UnsafeRemoval(
            "entry is not a direct child of its cache directory",
        ));
    }

    let dir = rustix::fs::open(
        &handle.cache_dir,
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::CLOEXEC
            | rustix::fs::OFlags::DIRECTORY
            | rustix::fs::OFlags::NOFOLLOW,
        rustix::fs::Mode::empty(),
    )
    .map_err(|source| ThumbnailError::Io {
        context: "open cache directory before removal",
        source: std::io::Error::from(source),
    })?;

    let stat = rustix::fs::statat(&dir, filename, rustix::fs::AtFlags::SYMLINK_NOFOLLOW).map_err(
        |source| ThumbnailError::Io {
            context: "inspect cache entry before removal",
            source: std::io::Error::from(source),
        },
    )?;
    let file_type = rustix::fs::FileType::from_raw_mode(stat.st_mode);
    if file_type.is_symlink() {
        return Err(ThumbnailError::UnsafeRemoval("entry is a symlink"));
    }
    if !file_type.is_file() {
        return Err(ThumbnailError::UnsafeRemoval("entry is not a regular file"));
    }

    rustix::fs::unlinkat(&dir, filename, rustix::fs::AtFlags::empty()).map_err(|source| {
        ThumbnailError::Io {
            context: "remove cache entry",
            source: std::io::Error::from(source),
        }
    })
}
