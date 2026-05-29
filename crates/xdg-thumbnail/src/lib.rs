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
    CacheRoot, InstalledThumbnailPath, InstalledThumbnailPayload, SharedCacheEntryInspection,
    SharedCacheEntryOutcome, SharedThumbnailLookup, ThumbnailLookup, ValidatedThumbnailPath,
    ValidatedThumbnailPayload,
};
pub use error::{Result, ThumbnailError};
pub use identity::{
    OriginalIdentity, ReadableOriginalIdentity, SharedRepositoryContext, UnixMTimeSeconds,
};
pub use inspection::{
    AccessTimePreservation, CacheEntryHandle, CacheEntryInspection, ThumbnailTimestamps,
    ThumbnailUriIdentity,
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
