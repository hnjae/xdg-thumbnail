# Library API Specification

The `xdg-thumbnail` library provides reusable primitives for applications that need to read, validate, create, or inspect Freedesktop thumbnails.

## Public API Goals

- Make spec-compatible thumbnail path calculation straightforward.
- Make thumbnail URI canonicalization explicit and shared by path calculation, metadata writing, and validation.
- Use the same canonical thumbnail URI bytes for hashing, metadata writing, and validation.
- Provide canonical URI constructors for local filesystem paths and shared-repository child filenames, while allowing callers to provide already-canonical absolute URI identities for non-local backends without changing the identity used for hashing.
- Allow applications to validate cached thumbnails without duplicating PNG metadata parsing.
- Allow applications to create thumbnails atomically after they render image data.
- Allow management tools to inspect cache entries without embedding CLI cleanup policy into the library.
- Allow management tools to request safe removal of inspected cache entries without reimplementing thumbnail-cache path containment and symlink checks.

## Core Capabilities

The library should expose APIs for:

- Resolving the personal thumbnail cache root.
- Constructing canonical personal-cache `file:` URI strings from absolute local paths without lossy path conversion.
- Constructing canonical shared-repository URI strings only for direct child originals represented by `./`-prefixed relative URIs.
- Accepting caller-provided canonical absolute URI identities for non-local backends and preserving the supplied identity bytes for hashing and metadata comparison.
- Computing the cache path for a canonical original URI and requested thumbnail namespace.
- Representing cache namespaces separately for successful thumbnail sizes and program-version failure entries.
- Parsing thumbnail PNG metadata.
- Building standard metadata for a newly generated thumbnail.
- Validating that saved successful thumbnails are 8-bit non-interlaced PNG files with full alpha support and dimensions that fit the requested size class.
- Checking whether a personal-cache thumbnail is valid for a given original by verifying `Thumb::URI`, `Thumb::MTime`, and `Thumb::Size` according to the Freedesktop standard.
- Checking shared-repository thumbnails with a separate validation context where present `Thumb::URI`, `Thumb::MTime`, and `Thumb::Size` values are verified, but missing `Thumb::URI` or `Thumb::MTime` does not automatically make the entry invalid.
- Iterating cache entries from known thumbnail directories.
- Returning policy-neutral inspection facts for cache management tools.
- Returning cache entry handles that identify entries discovered by library iteration and can remove those entries safely when the caller has already made a deletion decision.
- Saving a completed PNG to the cache through an atomic temporary file flow.
- Refusing to create thumbnails for originals located inside thumbnail cache directories.
- Reading shared thumbnail repositories without modifying them unless the caller explicitly requests shared-repository creation.

## Image Generation Boundary

The base library API does not decode images, render documents, or extract video frames. Instead, it accepts already-rendered thumbnail image data plus metadata and saves it according to the standard. The caller is responsible for preserving the original aspect ratio and applying source interpretation metadata such as Exif orientation while rendering; the library should still reject successful thumbnails whose PNG encoding or pixel dimensions do not satisfy the requested size class.

For personal-cache writes, the metadata builder must require `Thumb::URI` and `Thumb::MTime`. `Thumb::MTime` must be stored and compared as whole Unix epoch seconds. `Thumb::Size` should be included when original file size is available. The save helper should not write a personal-cache thumbnail when the original modification time cannot be obtained. Shared-repository writes are only available through an explicit shared creation mode and must use the shared relative URI rules in `docs/spec/uri-canonicalization.md`.

For personal-cache validation, missing `Thumb::URI`, a `Thumb::URI` value that differs from the canonical original URI, missing `Thumb::MTime`, or a `Thumb::MTime` value that differs from the original modification time in whole seconds makes the thumbnail invalid for display. `Thumb::Size` should be compared when present. Management tools should distinguish missing or malformed required metadata from metadata that is well-formed but stale for an existing original.

For shared-repository validation, `Thumb::URI`, `Thumb::MTime`, and `Thumb::Size` should be compared when present. Missing `Thumb::URI` or `Thumb::MTime` should produce a validation result that is usable by callers that accept shared repository freshness policy, but it must not be reported as equivalent to a fully metadata-validated personal-cache thumbnail. The public result type should distinguish at least fully verified entries, shared entries accepted by caller policy despite missing freshness metadata, unchecked inspection results, and invalid entries.

Application lookup APIs must not return an existing personal-cache thumbnail as display-valid, write a new personal-cache thumbnail, or write a failure entry when the caller cannot confirm that the original file is currently readable. For non-local backends, callers may provide an explicit original identity object containing the canonical thumbnail URI, current modification time, optional size, and proof that the original was readable through that backend. Separate cache-inspection APIs for management tools may still parse thumbnail files and metadata without opening the original, but they must report policy-neutral facts and must not present the entry as a validated thumbnail for display.

Failure entries are separate from successful thumbnail size namespaces. They are PNG metadata carriers stored under `fail/<program-version>/`; successful-thumbnail dimension limits do not apply to them. A failure writer should create a valid PNG metadata carrier, not a zero-byte file, and must require an explicit program-version namespace plus `Thumb::URI` and `Thumb::MTime` for readable originals.

The library should not apply user-facing cleanup policy. It may report facts such as missing originals, unreadable originals, unsupported original URI for local validation, mismatched metadata, malformed PNGs, nonconforming PNG format, thumbnail timestamps, and cache location, but age thresholds, removable path heuristics, URI class names, and deletion decisions belong to the caller.

The library removal API must operate only on cache entry handles returned by library iteration or explicit cache-path resolution. It must verify that the target is still inside the resolved thumbnail cache directory, must not follow symlinks, and must report deletion failures without retrying outside the cache root.
