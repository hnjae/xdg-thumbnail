# Crate Boundaries

The repository should be organized as a Cargo workspace with one reusable library crate and one CLI crate.

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

The library crate is the spec-oriented core. It should implement stable concepts from the Freedesktop Thumbnail Managing Standard and expose APIs that can be reused by GUI applications such as Kiriview.

The CLI crate is the policy runner. It should translate user input into cleanup policy, call the library to inspect cache entries, and perform filesystem mutations only after the CLI has made an explicit deletion decision.

## Library Responsibilities

- Resolve the thumbnail cache root from the XDG base directory rules, including ignoring relative `$XDG_CACHE_HOME` values.
- Represent canonical thumbnail URIs as library-owned string newtypes that preserve the exact MD5 and `Thumb::URI` input.
- Represent thumbnail sizes: `normal`, `large`, `x-large`, and `xx-large`.
- Represent cache namespaces separately for successful thumbnail sizes and application-specific failure entries.
- Compute thumbnail filenames from canonical thumbnail URIs using MD5 and the `.png` suffix, including absolute canonical URIs for the personal cache and `./`-prefixed relative URIs for shared repositories, as specified in `docs/spec/uri-canonicalization.md`.
- Reject shared-repository relative URIs that are not a single direct child filename, including parent segments, path separators, or encoded path separators.
- Read and write standard PNG text metadata such as `Thumb::URI`, `Thumb::MTime`, `Thumb::Size`, and `Thumb::Mimetype`.
- Require `Thumb::URI` and `Thumb::MTime` for personal-cache thumbnails, store and compare `Thumb::MTime` as whole Unix epoch seconds, and reject personal-cache writes when the original modification time cannot be obtained.
- Iterate cache entries from the personal thumbnail cache and optional shared thumbnail repositories.
- Validate personal and shared thumbnails with separate contexts because shared repositories may omit `Thumb::URI` or `Thumb::MTime` when they use other freshness mechanisms.
- Validate saved successful thumbnail PNGs against the required image format and maximum dimensions for the selected size class.
- Treat failure entries as metadata-carrying PNG files in application-specific failure namespaces, not as successful thumbnail size entries.
- Save thumbnails atomically by writing a temporary PNG in the target directory and renaming it to the final path.
- Reject thumbnail creation requests for originals located inside the personal thumbnail cache or a shared `.sh_thumbnails` repository.
- Treat shared thumbnail repositories as read-only during normal lookup and cleanup; shared-repository writes require an explicit creation mode.
- Apply spec-compatible permissions for the personal cache: cache directories must be created with mode `700`, and thumbnail files must be created with mode `600`.
- Apply shared-repository permissions only in explicit shared creation mode, using permissions consistent with the original files rather than personal-cache-only privacy permissions.
- Return structured, policy-neutral inspection facts without applying CLI cleanup policy.
- Avoid exposing user-facing URI classification, age thresholds, deletion reasons, or cleanup decisions from the library API.

The library should avoid depending on CLI-only concerns such as terminal formatting, progress bars, command-line parsing, logging configuration, or user-specific cleanup defaults.

The `x-large` and `xx-large` size classes are treated as supported documented behavior from the Freedesktop Thumbnail Managing Standard `latest` text, including the December 2020 0.9.0 history entry.

## CLI Responsibilities

- Parse command-line options such as `--older-than`, `--delete`, `--size`, `--scope`, `--include-nonstandard-files`, `--format`, `--ignore-fhs-media`, `--verbose`, and repeated custom removable path hints.
- Classify URI schemes and path prefixes according to user-facing cleanup policy.
- Apply age-based cleanup for remote, virtual, and removable-media-like entries.
- Own cleanup policy types such as URI classes, deletion reasons, skip reasons, and cleanup decisions.
- Delete files only when `--delete` is passed and report what was removed, skipped, or left unchanged.
- Skip nonstandard cache filenames by default and include them in deletion policy only when the user passes `--include-nonstandard-files`.
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
    Failure(ApplicationId),
}

pub struct CanonicalThumbnailUri {
    value: String,
}

pub enum CacheEntryState {
    OriginalMissing,
    Outdated,
    UnreadableOriginal,
    UnverifiableOriginal,
    MissingRequiredMetadata,
    Malformed,
}

pub enum ValidationOutcome {
    FullyVerified,
    SharedAcceptedByPolicy,
    UncheckedInspection,
    Invalid(CacheEntryState),
}

pub struct CacheEntryInspection {
    outcome: ValidationOutcome,
    original_uri: Option<CanonicalThumbnailUri>,
    thumbnail_accessed_at: Option<std::time::SystemTime>,
    namespace: CacheNamespace,
    cache_location: CacheLocation,
}
```

The exact names can change during implementation, but the direction should remain: the library describes cache entries and filesystem facts, while the CLI classifies entries, decides which cleanup policy to run, and applies destructive changes.

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

The CLI default classifier should handle stable URI scheme categories and user-configurable path prefixes. It should treat `/media`, `/run/media/$UID`, GVfs, and KIO FUSE paths as removable or desktop-managed by default; `/media` can be disabled with `--ignore-fhs-media`; `/mnt` is excluded by default and can be added with repeated `--removable-prefix` options.

For `file:` URIs, the default classifier should only treat empty authority and `localhost` authority as directly checkable local paths. Other authorities should be classified conservatively as remote or unknown unless an implementation-specific resolver is added.
