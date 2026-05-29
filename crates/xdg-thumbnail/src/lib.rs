// SPDX-FileCopyrightText: 2026 KIM Hyunjae
// SPDX-License-Identifier: MPL-2.0

//! Freedesktop thumbnail cache primitives.
//!
//! The library exposes typed helpers for resolving the personal thumbnail cache, constructing
//! canonical original identities, validating existing thumbnails, and atomically installing
//! caller-rendered thumbnail PNGs.
//!
//! ```no_run
//! use xdg_thumbnail::{
//!     PersonalCacheRoot, PersonalThumbnailLookup, ReadableOriginalIdentity, ThumbnailSize,
//! };
//!
//! fn main() -> xdg_thumbnail::Result<()> {
//!     let root = PersonalCacheRoot::resolve_from_env()?;
//!     let original = ReadableOriginalIdentity::from_local_path("/home/alice/Pictures/photo.png")?;
//!
//!     match root.lookup_thumbnail_png_bytes(&original, ThumbnailSize::Normal)? {
//!         PersonalThumbnailLookup::Valid(entry) => {
//!             let _png_bytes = entry.png_bytes();
//!         }
//!         PersonalThumbnailLookup::Missing | PersonalThumbnailLookup::Invalid(_) => {}
//!         _ => {}
//!     }
//!
//!     Ok(())
//! }
//! ```
//!
//! Owned request types make it straightforward to wrap blocking filesystem work in an async
//! runtime without adding a runtime dependency to this crate.
//!
//! ```no_run
//! use xdg_thumbnail::{
//!     PersonalCacheRoot, PersonalThumbnailInstallRequest, ReadableOriginalIdentity, ThumbnailSize,
//! };
//!
//! fn spawn_blocking<F, R>(operation: F) -> R
//! where
//!     F: FnOnce() -> R + Send + 'static,
//!     R: Send + 'static,
//! {
//!     operation()
//! }
//!
//! fn render_thumbnail_png() -> Vec<u8> {
//!     unimplemented!("return PNG bytes produced by the caller's renderer")
//! }
//!
//! fn main() -> xdg_thumbnail::Result<()> {
//!     let root = PersonalCacheRoot::resolve_from_env()?;
//!     let original = ReadableOriginalIdentity::from_local_path("/home/alice/Pictures/photo.png")?;
//!     let rendered_png = render_thumbnail_png();
//!     let request = PersonalThumbnailInstallRequest::new(
//!         root,
//!         original,
//!         ThumbnailSize::Normal,
//!         rendered_png,
//!     );
//!
//!     let installed = spawn_blocking(move || request.install_png_bytes())?;
//!     let _path = installed.path();
//!
//!     Ok(())
//! }
//! ```

#[cfg(not(unix))]
compile_error!(
    "xdg-thumbnail supports Unix-like targets only because thumbnail identity and cache safety depend on Unix path bytes, metadata, and permissions"
);

#[cfg(unix)]
mod cache;
#[cfg(unix)]
mod error;
#[cfg(unix)]
mod identity;
#[cfg(unix)]
mod inspection;
#[cfg(unix)]
mod namespace;
#[cfg(unix)]
mod png;
#[cfg(unix)]
mod uri;

#[cfg(unix)]
pub use cache::{
    FailureEntryWriteRequest, InstalledThumbnailPath, InstalledThumbnailPngBytes,
    PersonalCacheRoot, PersonalThumbnailInspectionRequest, PersonalThumbnailInstallRequest,
    PersonalThumbnailLookup, PersonalThumbnailLookupRequest, PersonalThumbnailRawInstallRequest,
    SharedCacheEntryInspection, SharedCacheEntryOutcome, SharedOriginalFacts,
    SharedOriginalMetadata, SharedThumbnailInspectionRequest, SharedThumbnailLookup,
    SharedThumbnailLookupRequest, SharedThumbnailMetadataPolicy, ThumbnailPathLookupEntry,
    ThumbnailPngBytesLookupEntry, ThumbnailRgba8LookupEntry,
};
#[cfg(unix)]
pub use error::{Result, ThumbnailError};
#[cfg(unix)]
pub use identity::{
    OriginalIdentity, ReadableOriginalIdentity, SharedRepositoryContext, UnixMTimeSeconds,
};
#[cfg(unix)]
pub use inspection::{
    AccessTimePreservation, CacheEntryHandle, CacheEntryInspection, CacheEntryInspectionOutcome,
    NonstandardEntryPolicy, OriginalUriIdentity, ThumbnailTimestamps,
};
#[cfg(unix)]
pub use namespace::{CacheNamespace, FailureNamespace, ThumbnailSize};
#[cfg(unix)]
pub use png::{
    CacheEntryProblem, OwnedRawThumbnailImage, ParsedThumbnailPng, PersonalValidationOutcome,
    RawThumbnailImage, RawThumbnailPixelFormat, SharedValidationOutcome, ThumbnailMetadata,
    ThumbnailPngBitDepth, ThumbnailPngColorType, validate_personal_failure_entry,
    validate_personal_thumbnail, validate_shared_thumbnail,
};
#[cfg(unix)]
pub use uri::{PersonalOriginalUri, SharedRelativeOriginalUri};

#[cfg(unix)]
pub(crate) use png::{
    decode_validated_thumbnail_png_to_rgba8, encode_rgba_png, normalized_personal_thumbnail_png,
    normalized_personal_thumbnail_raw_png, push_problem, thumbnail_metadata_pairs,
    validate_mime_type,
};
