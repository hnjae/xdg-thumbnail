# Crate Boundaries

The repository should be organized as a Cargo workspace with one reusable library crate and one CLI crate.

Initial implementation scope is Unix-like XDG desktop environments. Platform-specific APIs should make Unix path bytes, permissions, file metadata, and XDG cache behavior explicit, and unsupported platforms should fail with clear errors rather than silently approximating incompatible cache identities.

```text
xdg-thumbnail/
  Cargo.toml
  crates/
    xdg-thumbnail/
      Cargo.toml
      src/lib.rs
    xdg-thumbnail-cli/
      Cargo.toml
      src/main.rs
```

The library crate is the spec-oriented core. It should implement stable cache inspection and validation concepts from the Freedesktop Thumbnail Managing Standard and expose APIs that can be reused by cleanup tools and thumbnail consumers.

The CLI crate is the policy runner. It should translate user input into cleanup policy, call the library to inspect cache entries, and perform filesystem mutations only after the CLI has made an explicit deletion decision.

## Library Responsibilities

- Resolve the thumbnail cache root from the XDG base directory rules, including ignoring relative `$XDG_CACHE_HOME` values.
- Represent canonical thumbnail URIs as library-owned string newtypes that preserve the exact MD5 and `Thumb::URI` input.
- Construct canonical thumbnail URIs for local filesystem paths and shared-repository child filenames; for other schemes, preserve caller-provided canonical absolute URI strings without parser reserialization.
- Represent thumbnail sizes: `normal`, `large`, `x-large`, and `xx-large`.
- Represent cache namespaces separately for successful thumbnail sizes and program-version failure entries.
- Compute thumbnail filenames from canonical thumbnail URIs using MD5 and the `.png` suffix, including absolute canonical URIs for the personal cache and `./`-prefixed relative URIs for shared repositories, as specified in `docs/spec/uri-canonicalization.md`.
- Reject shared-repository relative URIs that are not a single direct child filename, including parent segments, path separators, or encoded path separators.
- Read standard PNG text metadata such as `Thumb::URI`, `Thumb::MTime`, `Thumb::Size`, and `Thumb::Mimetype`.
- Require `Thumb::URI` and `Thumb::MTime` for personal-cache validation and compare `Thumb::MTime` as whole Unix epoch seconds.
- Iterate cache entries from the personal thumbnail cache and optional shared thumbnail repositories.
- Validate personal and shared thumbnails with separate contexts because shared repositories may omit `Thumb::URI` or `Thumb::MTime` when they use other freshness mechanisms.
- Inspect successful thumbnail PNGs against the required image format and maximum dimensions for the selected size class.
- Treat failure entries as metadata-carrying PNG files in program-version failure namespaces, not as successful thumbnail size entries.
- Treat shared thumbnail repositories as read-only during initial lookup and cleanup; shared-repository writes are outside the initial library and CLI responsibilities.
- Return structured, policy-neutral inspection facts without applying CLI cleanup policy. Invalid PNG structure, missing required metadata, invalid metadata syntax, stale metadata, nonconforming PNG encoding, and dimension violations must remain distinguishable facts.
- Provide cache entry handles for entries discovered by library iteration or explicit cache-path resolution, and implement safe removal on those handles with containment checks and no symlink following. The removal design should prefer directory-relative APIs such as `openat` and `unlinkat`, or a capability-style equivalent, so containment and symlink checks are not only string-prefix checks. Any fallback that cannot provide that strength must be documented and reported as best-effort.
- Expose thumbnail file timestamp facts, including modification time, access time when available, and whether metadata inspection preserved access time, without deciding age-based cleanup policy.
- Avoid exposing user-facing URI classification, age thresholds, deletion reasons, or cleanup decisions from the library API.

The library should avoid depending on CLI-only concerns such as terminal formatting, progress bars, command-line parsing, logging configuration, user-specific cleanup defaults, or user-facing report vocabulary. Image rendering, thumbnail generation, metadata writing, and thumbnail save helpers are outside the base crate scope.

The `x-large` and `xx-large` size classes are treated as supported documented behavior from the Freedesktop Thumbnail Managing Standard `latest` text, including the December 2020 0.9.0 history entry.

## CLI Responsibilities

- Parse command-line options such as `--older-than`, `--delete`, `--delete-stale-local`, `--allow-delete-failures`, `--size`, `--scope`, `--age-basis`, `--include-nonstandard-files`, `--format`, `--ignore-fhs-media`, `--verbose`, and repeated custom removable path hints.
- Classify URI schemes and path prefixes according to user-facing cleanup policy.
- Apply age-based cleanup for remote, virtual, and removable-media-like entries.
- Inspect and classify failure entries when the user includes failure namespaces in the scan scope, while requiring an extra failure-deletion opt-in before treating them as deletion candidates.
- Own cleanup policy types such as URI classes, deletion reasons, skip reasons, and cleanup decisions.
- Request removal through library cache entry handles only after the CLI has made an explicit deletion decision, and only when the relevant destructive flags are present.
- Delete successful thumbnail entries only when `--delete` is passed, failure entries only when both `--delete` and `--allow-delete-failures` are passed, and report what was removed, skipped, or left unchanged.
- Skip nonstandard cache filenames by default and expose them only as reportable skipped entries when the user passes `--include-nonstandard-files`.
- Provide conservative defaults and clear report output before destructive cleanup.
- Convert library errors into actionable CLI diagnostics and exit codes.

## Shared Types

The library should expose policy-neutral types that the CLI can combine into user-facing behavior.

```rust
pub enum ThumbnailSize {
    Normal,
    Large,
    XLarge,
    XxLarge,
}

pub enum CacheNamespace {
    Size(ThumbnailSize),
    Failure(FailureNamespace),
}

pub struct FailureNamespace {
    program_version: String,
}

pub struct CanonicalThumbnailUri {
    value: String,
}

pub enum CacheEntryProblem {
    OriginalMissing,
    StaleMetadata,
    UnreadableOriginal,
    UnverifiableOriginal,
    MissingRequiredMetadata,
    InvalidMetadataSyntax,
    InvalidPngStructure,
    NonconformingPngFormat,
    DimensionsExceedNamespace,
}

pub enum ValidationOutcome {
    FullyVerified,
    SharedAcceptedByPolicy,
    UncheckedInspection,
    Invalid(Vec<CacheEntryProblem>),
}

pub struct CacheEntryInspection {
    outcome: ValidationOutcome,
    original_uri: Option<CanonicalThumbnailUri>,
    thumbnail_timestamps: ThumbnailTimestamps,
    namespace: CacheNamespace,
    cache_location: CacheLocation,
    handle: CacheEntryHandle,
}

pub struct ThumbnailTimestamps {
    accessed_at: Option<std::time::SystemTime>,
    modified_at: Option<std::time::SystemTime>,
    access_time_preserved_during_inspection: AccessTimePreservation,
}

pub enum AccessTimePreservation {
    Preserved,
    NotPreserved,
    NotNeeded,
    Unsupported,
}

pub struct CacheEntryHandle {
    cache_root: CacheRoot,
    path: std::path::PathBuf,
}
```

The exact names can change during implementation, but the direction should remain: the library describes cache entries and filesystem facts with enough precision to avoid accidental cleanup policy, while the CLI classifies entries, decides which cleanup policy to run, and requests destructive changes only through library-provided cache entry handles.

## URI Classification Boundary

URI classification should be extensible because desktop environments and mounted filesystems vary. User-facing cleanup classification belongs to the CLI layer. The library may report that an original URI cannot be validated as a directly checkable local file, but it must not label the URI as remote, virtual, removable, or safe to delete by age. The CLI may parse a canonical thumbnail URI for scheme and authority classification, but the parsed form must not replace the library-owned canonical string used for hashing and metadata comparison.

CLI-side classification can use types shaped like this:

```rust
pub enum UriClass {
    LocalStableFile,
    LocalRemovableOrPortal,
    Remote,
    ArchiveOrVirtual,
    Unknown,
}

pub trait CleanupClassifier {
    fn classify(&self, uri: &CanonicalThumbnailUri) -> UriClass;
}
```

The CLI default classifier should handle stable URI scheme categories and user-configurable path prefixes. It should treat `/media`, `/run/media/$UID`, `/run/user/$UID/doc`, GVfs, and KIO FUSE paths as removable, portal, or desktop-managed by default; `/media` can be disabled with `--ignore-fhs-media`; `/mnt` is excluded by default and can be added with repeated `--removable-prefix` options.

For `file:` URIs, the default classifier should only treat empty authority and `localhost` authority as directly checkable local paths. Other authorities should be classified conservatively as remote or unknown unless an implementation-specific resolver is added. Direct local checks must distinguish confirmed absence from permission errors, transient I/O errors, and unsupported path conversion so cleanup policy can skip unverifiable originals instead of deleting them as missing.
