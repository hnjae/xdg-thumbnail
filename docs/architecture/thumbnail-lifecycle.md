# Thumbnail Lifecycle

This document describes the intended data flow for reading, validating, inspecting, installing, and pruning thumbnails.

## Lookup Flow

```mermaid
flowchart TD
    A[Original identity input] --> B[Accepted thumbnail URI identity]
    B --> C{Original readable?}
    C -- no --> L[Not display-valid]
    C -- yes --> D[MD5 filename]
    D --> E[Personal cache path]
    E --> F{Personal PNG exists?}
    F -- no --> S{Shared lookup enabled?}
    F -- yes --> H[Read PNG metadata]
    H --> I{Metadata valid for context?}
    I -- yes --> J[Return validated path or PNG bytes]
    I -- no --> S
    S -- yes --> T{Acceptable shared thumbnail?}
    T -- yes --> U[Return validated shared path or PNG bytes]
    T -- no --> N[Cache miss]
    S -- no --> N
```

The personal thumbnail repository has priority over shared thumbnail repositories. If a personal thumbnail exists but is outdated or corrupt and shared lookup is enabled, the caller should check the shared repository before reporting a cache miss. If caller policy chooses a shared entry, cleanup of the stale personal entry is a separate caller policy decision. Normal lookup must treat shared repositories as read-only; personal-cache writes use the explicit install lifecycle.

Shared thumbnail repositories are scoped to the directory that contains the original file. Shared lookup therefore requires a shared repository context, not only the personal-cache absolute URI: the context must identify the repository root, the direct child original filename, and the `./`-prefixed relative URI used for shared hashing and optional `Thumb::URI` comparison. Shared-repository thumbnail URIs are not recursive paths and must reject parent segments, slash path separators, and textual shared URI inputs containing encoded `/`. Raw direct-child filenames that contain percent-looking text remain valid filenames and must be percent-encoded rather than treated as URI text. On Unix-like targets, backslash is a filename byte and must be preserved through percent-encoding when needed.

## Validation Model

Validation must verify stored metadata against the original whenever the standard requires or permits it. For personal-cache thumbnails, missing `Thumb::URI`, a `Thumb::URI` mismatch against the canonical original URI, missing `Thumb::MTime`, or a `Thumb::MTime` mismatch is invalid because the standard requires these keys for identity and freshness checks. `Thumb::MTime` must be stored and compared as whole Unix epoch seconds. `Thumb::Size` should be checked when present.

Shared-repository validation is a separate context. When `Thumb::URI`, `Thumb::MTime`, or `Thumb::Size` is present, the library should compare it with the explicit shared relative URI or original metadata from the shared repository context. Missing `Thumb::URI` or `Thumb::MTime` is not automatically invalid for shared thumbnails because shared repositories may use other freshness mechanisms, so the library should report incomplete freshness metadata and callers must decide whether their use case can use that entry. Shared lookup APIs should make the acceptance policy visible: a safety-oriented policy may require present and matching shared freshness metadata, while a caller that wants to use standard-allowed shared thumbnails with incomplete metadata must choose an explicit acceptance policy.

The validation result should carry confidence separately from validity. A personal thumbnail with matching required metadata is fully verified. A shared thumbnail with missing `Thumb::URI` or `Thumb::MTime` is a policy-neutral incomplete-metadata result that callers may accept or reject under their own shared-repository policy; the library must not report it as equivalent to a fully verified personal thumbnail. A management-tool inspection that did not read the original is an unchecked inspection result, not a display-valid thumbnail.

The library should compare modification times for equality, not only check whether the original is newer. A replacement file can have an older modification time than the thumbnail metadata, and that still means the thumbnail no longer represents the original.

## Generation And Install Boundary

Thumbnail source creation is outside the base library and prune CLI scope. The generate CLI or embedding application owns generation orchestration: it discovers or selects the renderer, runs or calls it, and decides whether generation should be attempted. The selected renderer or thumbnailer owns source-format decoding, source interpretation, source metadata handling such as Exif orientation, and source-aware aspect-ratio decisions for inputs such as PNG, WebP, documents, and video. The library may downscale already rendered PNG output to fit a cache namespace, but the generate CLI does not inspect the original source format to repair renderer mistakes.

When the generate CLI runs external `.thumbnailer` helpers, sandbox setup is part of CLI execution policy rather than the base library because it belongs to external process execution, command expansion, and temporary renderer output. The generate CLI keeps the host canonical URI for cache identity, hashing, metadata, and validation separate from the sandbox-visible URI passed to thumbnailers through `%u`. The user-visible sandbox modes, default behavior, failure reporting, path exposure model, and thumbnailer eligibility rules are specified in `docs/spec/generate-cli-behavior.md`.

The library owns the Freedesktop installation mechanics for personal-cache entries once a caller supplies a readability-confirmed original identity and already rendered thumbnail bytes. The caller owns renderer temporary files, including external thumbnailer `%o` outputs, until rendered output is handed to the library through a supported input form. The install path normalizes supported rendered PNG bytes to 8-bit non-interlaced RGBA PNG output with full alpha support, downscales rendered output when needed to fit the target namespace, writes standard metadata, creates missing cache directories with mode `0700`, rejects existing standard cache directories that are not current-user-owned and private, writes temporary files in the target directory, installs final files with mode `0600`, and publishes the result with an atomic rename. Supported normalization may include adding opaque alpha to RGB output, expanding grayscale or indexed-color PNG output, converting supported PNG bit depths to 8-bit output, and rewriting the PNG to add or replace standard thumbnail metadata. Source decoding, source metadata interpretation, animation frame selection, and repairing aspect-ratio mistakes remain outside the base library.

Shared-repository writes remain outside the initial lifecycle. Failure entry writing is an explicit library primitive because it uses the same Freedesktop filename, metadata, permission, and atomic-install rules, but callers own the policy for when a failure should be recorded. The initial failure writer creates a deterministic minimal 1x1 transparent RGBA PNG with required failure metadata instead of accepting renderer-provided bytes.

Application lookup must not use existing thumbnails when the original is not currently readable. Separate management-tool inspection may still parse thumbnail files and metadata without opening the original, but such inspection must report facts rather than validate the thumbnail for display.

## Lookup Result Surfaces

The library exposes cache reuse through layered result surfaces. A computed path is only the MD5-derived cache location for an accepted URI identity and namespace; it is useful for reports, dry-run output, and install targeting, but it does not mean a thumbnail exists or is valid. A validated path means the library opened the cache PNG and verified it against the original identity and namespace before returning the path; it is a convenience for toolkits that require a filename, but callers that reopen the path accept that the file can be replaced after validation. A validated PNG bytes result means the library returns the exact PNG bytes that passed validation, plus the cache path and metadata facts; this is the preferred surface for application thumbnail views. Output selection is caller-driven: lookup should expose either separate calls or an explicit mode for path versus PNG bytes. Opened handles or mmap-backed byte buffers can be added later as optimized variants, but the initial caller-selectable outputs are path and PNG bytes.

Lookup results should distinguish valid, missing, invalid, and unverifiable originals. Missing and invalid results let an application decide whether to render and install a new thumbnail. Unverifiable results mean the caller did not or could not provide readability and modification-time proof for the original, so the library must not present an existing cache entry as display-valid.

## Personal Install Flow

```mermaid
flowchart TD
    A[Caller rendered thumbnail bytes] --> B{Readable original identity has URI and mtime?}
    B -- no --> C[Reject write]
    B -- yes --> D[Compute personal cache path]
    D --> E[Normalize and downscale PNG for namespace]
    E --> F[Write standard metadata]
    F --> G[Write temp file in target directory]
    G --> H[Set 0600 file permissions]
    H --> I[Atomic rename to final path]
    I --> J[Return installed path and optional final PNG bytes]
```

Applications that embed their own renderer should use this install flow instead of reimplementing the Freedesktop cache filename, metadata, permission, and atomic-save rules. The library should not know whether the renderer input bytes came from Qt image decoding, a Rust image decoder, a document renderer, a video frame extractor, or an external thumbnailer. Install results should at least report the installed cache path. When the caller requests PNG bytes output, the library should return the final normalized PNG bytes for the installed entry; those bytes represent the cache entry after cache-size downscaling, metadata writing, and normalization, not the original renderer input. Install should follow the same explicit path-versus-PNG-bytes output selection model as lookup.

## Pruning Model

The library owns cache entry discovery, policy-neutral inspection facts, and low-level cache path safety. The prune CLI owns user-facing cleanup policy, report vocabulary, and destructive intent. A pruning run should therefore enumerate cache entries through the library, classify and decide in the prune CLI, then request deletion through a library cache entry handle only for entries that still pass containment checks and are not symlinks. Cache entry deletion should be implemented with directory-relative removal primitives such as `openat` and `unlinkat`, or a capability-style equivalent, where available; weaker fallbacks must be treated as best-effort rather than race-free.

Age-based cleanup policy belongs to the prune CLI, while the library reports timestamp facts and whether metadata inspection preserved access time. The prune CLI spec defines the default age basis, skip behavior when access-time preservation is unavailable, and the explicit modification-time mode.

## Existing Thumbnail Lookup Target

A thumbnail consumer that only wants to reuse existing thumbnails should be able to use the library through a flow equivalent to:

```rust
let identity = ReadablePersonalOriginalIdentity::from_local_path(path)?;
let personal_uri: &PersonalOriginalUri = identity.identity().uri();
let computed_path = cache.cache_entry_path(personal_uri, &CacheNamespace::Size(ThumbnailSize::Normal));

match cache.lookup_thumbnail_png_bytes(&identity, ThumbnailSize::Normal)? {
    PersonalThumbnailLookup::Valid(thumbnail) => return Ok(Some(thumbnail)),
    PersonalThumbnailLookup::Missing | PersonalThumbnailLookup::Invalid(_) => {}
    _ => {}
}

let repository_root = path.parent().expect("absolute original path has a parent");
let original_child_name = path.file_name().expect("original path has a filename");
let shared = SharedRepositoryContext::new(repository_root, original_child_name)?;
let shared_metadata = SharedOriginalMetadata::new().with_mtime(identity.identity().mtime());
let shared_metadata = if let Some(size) = identity.identity().original_byte_size() {
    shared_metadata.with_original_byte_size(size)
} else {
    shared_metadata
};
let shared_original = SharedOriginalFacts::new(SharedThumbnailMetadataPolicy::RequireComplete, shared_metadata);
match shared.lookup_thumbnail_png_bytes(shared_original, ThumbnailSize::Normal)? {
    SharedThumbnailLookup::FullyVerified(thumbnail) => return Ok(Some(thumbnail)),
    SharedThumbnailLookup::MetadataIncomplete(_) => {}
    SharedThumbnailLookup::Missing | SharedThumbnailLookup::Invalid(_) => {}
    SharedThumbnailLookup::Unverifiable(_) => return Ok(None),
    _ => {}
}

Ok(None)
```

The exact API should be shaped by implementation, but applications should not need to know the hash filename algorithm, cache directory layout, or PNG metadata keys directly. `computed_path` is available for diagnostics or reports, while validated PNG bytes lookup can return display-grade validated PNG bytes rather than asking the caller to reopen a path and reparse metadata. A cache miss is returned to the caller; lookup does not perform rendering, and callers that render a thumbnail use the separate install flow. The example uses a safety-oriented shared policy that can decline standard-allowed shared thumbnails with incomplete freshness metadata. Applications that intentionally accept shared thumbnails with incomplete freshness metadata should pass an explicit shared policy tied to their own trust or freshness model.

## Failure Entries

The standard supports per-program-version failure entries under `thumbnails/fail/<program-version>/`. The library should model these as failure namespaces, not as thumbnail sizes. It should be able to locate, parse, inspect, and explicitly write failure entries. Failure-entry writes require the caller to provide a validated direct directory namespace and a readability-confirmed original identity; the library must not decide when a failed render should suppress future attempts. Failure entries are PNG metadata carriers named with the same URI-derived filename procedure as successful thumbnails, and successful-thumbnail size validation does not apply to them. The initial writer should generate a minimal 1x1 transparent RGBA PNG with `Thumb::URI` and `Thumb::MTime`, plus optional caller-supplied original metadata.

Initial behavior: the prune CLI scans successful thumbnail entries by default and scans failure entries only with `--scope failures` or `--scope all`. This avoids touching application-specific retry state without explicit user intent. Once failure entries are in scope, the prune CLI should inspect and classify them like successful thumbnails because the user is asking to manage cache entries for the same original URI identity; the special cases are that successful-thumbnail dimension validation does not apply and deletion requires `--allow-delete-failures` in addition to `--delete`.

Failure iteration should treat only immediate real directories below `fail/` as program-version namespaces and should inspect only direct child files inside each namespace. Symlinked namespace directories and nested directories are skipped so failure scanning cannot escape the intended cache shape.
