// SPDX-FileCopyrightText: 2026 KIM Hyunjae
// SPDX-License-Identifier: MPL-2.0

//! Freedesktop thumbnail cache primitives.

mod cache;
mod error;
mod identity;
mod inspection;
mod namespace;
mod png;
mod uri;

pub use cache::{
    CacheRoot, FailureEntryWriteRequest, InstalledThumbnailPath, InstalledThumbnailPayload,
    PersonalThumbnailInspectionRequest, PersonalThumbnailInstallRequest,
    PersonalThumbnailLookupRequest, PersonalThumbnailRawInstallRequest, SharedCacheEntryInspection,
    SharedCacheEntryOutcome, SharedThumbnailInspectionRequest, SharedThumbnailLookup,
    SharedThumbnailLookupRequest, ThumbnailLookup, ValidatedThumbnailPath,
    ValidatedThumbnailPayload,
};
pub use error::{Result, ThumbnailError};
pub use identity::{
    OriginalIdentity, ReadableOriginalIdentity, SharedRepositoryContext, UnixMTimeSeconds,
};
pub use inspection::{
    AccessTimePreservation, CacheEntryHandle, CacheEntryInspection, CacheEntryInspectionOutcome,
    OriginalUriIdentity, ThumbnailTimestamps,
};
pub use namespace::{CacheNamespace, FailureNamespace, ThumbnailSize};
pub use png::{
    CacheEntryProblem, OwnedRawThumbnailImage, ParsedThumbnailPng, PersonalValidationOutcome,
    RawThumbnailImage, RawThumbnailPixelFormat, SharedValidationOutcome, ThumbnailMetadata,
    ThumbnailPngBitDepth, ThumbnailPngColorType, validate_personal_failure_entry,
    validate_personal_thumbnail, validate_shared_thumbnail,
};
pub use uri::{PersonalOriginalUri, SharedRelativeOriginalUri};

pub(crate) use png::{
    encode_rgba_png, normalized_personal_thumbnail_png, normalized_personal_thumbnail_raw_png,
    push_problem, thumbnail_metadata_pairs, validate_mime_type,
};
