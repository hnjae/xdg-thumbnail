# Library API Specification

The `xdg-thumbnail` library provides reusable primitives for applications that need to read, validate, create, or inspect Freedesktop thumbnails.

## Public API Goals

- Make spec-compatible thumbnail path calculation straightforward.
- Make thumbnail URI canonicalization explicit and shared by path calculation, metadata writing, and validation.
- Represent the canonical thumbnail URI as an owned library type rather than exposing a general URL type as the identity used for hashing.
- Allow applications to validate cached thumbnails without duplicating PNG metadata parsing.
- Allow applications to create thumbnails atomically after they render image data.
- Allow management tools to inspect cache entries without embedding CLI cleanup policy into the library.

## Core Capabilities

The library should expose APIs for:

- Resolving the personal thumbnail cache root.
- Constructing canonical personal-cache and shared-repository thumbnail URI strings without lossy path conversion or hidden URL reserialization.
- Computing the cache path for a canonical original URI and requested thumbnail namespace.
- Computing shared-repository paths only for direct child originals represented by `./`-prefixed relative URIs.
- Representing cache namespaces separately for successful thumbnail sizes and application-specific failure entries.
- Parsing thumbnail PNG metadata.
- Building standard metadata for a newly generated thumbnail.
- Validating that saved successful thumbnails are 8-bit non-interlaced PNG files with full alpha support and dimensions that fit the requested size class.
- Checking whether a personal-cache thumbnail is valid for a given original by verifying `Thumb::URI`, `Thumb::MTime`, and `Thumb::Size` according to the Freedesktop standard.
- Checking shared-repository thumbnails with a separate validation context where present `Thumb::URI`, `Thumb::MTime`, and `Thumb::Size` values are verified, but missing `Thumb::URI` or `Thumb::MTime` does not automatically make the entry invalid.
- Iterating cache entries from known thumbnail directories.
- Returning policy-neutral inspection facts for cache management tools.
- Saving a completed PNG to the cache through an atomic temporary file flow.
- Refusing to create thumbnails for originals located inside thumbnail cache directories.
- Reading shared thumbnail repositories without modifying them unless the caller explicitly requests shared-repository creation.

## Image Generation Boundary

The core library should not initially decode images, render documents, or extract video frames. Instead, it should accept already-rendered thumbnail image data plus metadata and save it according to the standard. The caller is responsible for preserving the original aspect ratio and applying source interpretation metadata such as Exif orientation while rendering; the library should still reject successful thumbnails whose PNG encoding or pixel dimensions do not satisfy the requested size class.

For personal-cache writes, the metadata builder must require `Thumb::URI` and `Thumb::MTime`. `Thumb::MTime` must be stored and compared as whole Unix epoch seconds. `Thumb::Size` should be included when original file size is available. The save helper should not write a personal-cache thumbnail when the original modification time cannot be obtained. Shared-repository writes are only available through an explicit shared creation mode and must use the shared relative URI rules in `docs/spec/uri-canonicalization.md`.

For personal-cache validation, missing `Thumb::URI`, a `Thumb::URI` value that differs from the canonical original URI, missing `Thumb::MTime`, or a `Thumb::MTime` value that differs from the original modification time in whole seconds makes the thumbnail invalid. `Thumb::Size` should be compared when present.

For shared-repository validation, `Thumb::URI`, `Thumb::MTime`, and `Thumb::Size` should be compared when present. Missing `Thumb::URI` or `Thumb::MTime` should produce a validation result that is usable by callers that accept shared repository freshness policy, but it must not be reported as equivalent to a fully metadata-validated personal-cache thumbnail. The public result type should distinguish at least fully verified entries, shared entries accepted by caller policy despite missing freshness metadata, unchecked inspection results, and invalid entries.

Application lookup APIs must not return an existing personal-cache thumbnail as display-valid, write a new personal-cache thumbnail, or write a failure entry when the caller cannot confirm that the original file is currently readable. For non-local backends, callers may provide an explicit original identity object containing the canonical thumbnail URI, current modification time, optional size, and proof that the original was readable through that backend. Separate cache-inspection APIs for management tools may still parse thumbnail files and metadata without opening the original, but they must report policy-neutral facts and must not present the entry as a validated thumbnail for display.

Failure entries are separate from successful thumbnail size namespaces. They are valid PNG metadata carriers stored under `fail/<application-id>/`; successful-thumbnail dimension limits do not apply to them. A failure writer should create a minimal valid PNG rather than a zero-byte file and must require an explicit application identifier plus `Thumb::URI` and `Thumb::MTime` for readable originals.

The library should not apply user-facing cleanup policy. It may report facts such as missing originals, unreadable originals, unsupported original URI for local validation, mismatched metadata, malformed PNGs, thumbnail access time, and cache location, but age thresholds, removable path heuristics, URI class names, and deletion decisions belong to the CLI.

A later optional feature may provide image-specific helpers, but the base crate should remain small enough for both CLI tools and GUI applications.

## Kiriview Integration Target

Kiriview should be able to use the library through a flow equivalent to:

```rust
let uri = ThumbnailUri::from_file_path(path)?;
let original = OriginalFile::open_readable(path)?;
let identity = OriginalIdentity::from_readable_file(&uri, &original)?;
let cache_path = cache.thumbnail_path(&uri, CacheNamespace::Size(ThumbnailSize::Normal));

if cache.is_valid(&cache_path, &identity)?.is_fully_verified() {
    return Ok(cache_path);
}

let rendered_png = render_thumbnail(path)?;
cache.save_png_atomic(CacheNamespace::Size(ThumbnailSize::Normal), rendered_png, metadata, &identity)?;
```

The exact API should be shaped by implementation, but Kiriview should not need to know the hash filename algorithm, cache directory layout, or PNG metadata keys directly.
