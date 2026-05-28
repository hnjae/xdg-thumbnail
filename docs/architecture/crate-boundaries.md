# Crate Boundaries

The repository should be organized as a Cargo workspace with one reusable library crate and separate CLI crates for pruning and generation.

Initial implementation scope is Unix-like XDG desktop environments. Platform-specific APIs should make Unix path bytes, permissions, file metadata, and XDG cache behavior explicit, and unsupported platforms should fail with clear errors rather than silently approximating incompatible cache identities.

```text
xdg-thumbnail/
  Cargo.toml
  crates/
    xdg-thumbnail/
      Cargo.toml
      src/lib.rs
    xdg-thumbnail-prune/
      Cargo.toml
      src/main.rs
    xdg-thumbnail-generate/
      Cargo.toml
      src/main.rs
```

The library crate is the spec-oriented core. It should implement stable cache inspection, validation, and personal-cache installation concepts from the Freedesktop Thumbnail Managing Standard and expose APIs that can be reused by cleanup tools, thumbnail consumers, and applications that provide their own thumbnail rendering.

The CLI crates are policy runners. `xdg-thumbnail-prune` should translate user input into cleanup policy, call the library to inspect cache entries, and perform filesystem mutations only after the prune CLI has made an explicit deletion decision. `xdg-thumbnail-generate` should discover and execute installed thumbnailer helpers, then call library installation APIs only after generation succeeds.

## Library Responsibilities

- Resolve the thumbnail cache root from the XDG base directory rules, including ignoring relative `$XDG_CACHE_HOME` values.
- Represent canonical thumbnail URIs as library-owned string newtypes that preserve the exact MD5 and `Thumb::URI` input.
- Construct canonical thumbnail URIs for local filesystem paths and shared-repository child filenames; for other schemes, preserve caller-provided canonical absolute URI strings without parser reserialization.
- Represent thumbnail sizes: `normal`, `large`, `x-large`, and `xx-large`.
- Represent cache namespaces separately for successful thumbnail sizes and program-version failure entries.
- Compute thumbnail filenames from canonical thumbnail URIs using MD5 and the `.png` suffix, including absolute canonical URIs for the personal cache and `./`-prefixed relative URIs for shared repositories, as specified in `docs/spec/uri-canonicalization.md`.
- Reject shared-repository relative URIs that are not a single direct child filename, including parent segments, slash path separators, or encoded `/`. On Unix-like targets, backslash remains a filename byte and is preserved through percent-encoding when needed.
- Read standard PNG text metadata such as `Thumb::URI`, `Thumb::MTime`, `Thumb::Size`, and `Thumb::Mimetype`.
- Write standard PNG text metadata such as required `Thumb::URI` and `Thumb::MTime`, plus optional `Thumb::Size`, `Thumb::Mimetype`, and media-specific keys when the caller supplies them.
- Require `Thumb::URI` and `Thumb::MTime` for personal-cache validation and compare `Thumb::MTime` as whole Unix epoch seconds.
- Reject personal-cache successful thumbnail and failure-entry writes when the caller cannot provide an original modification time, because such entries cannot satisfy global-cache freshness checks.
- Iterate cache entries from the personal thumbnail cache and optional shared thumbnail repositories.
- Validate personal and shared thumbnails with separate contexts because shared repositories may omit `Thumb::URI` or `Thumb::MTime` when they use other freshness mechanisms.
- Inspect successful thumbnail PNGs against the required image format and maximum dimensions for the selected size class.
- Normalize caller-provided rendered thumbnail payloads to 8-bit non-interlaced RGBA PNG output before successful personal-cache installation.
- Install successful personal-cache thumbnails atomically from caller-provided in-memory thumbnail payloads, after validating the normalized final PNG encoding and dimensions against the requested size namespace.
- Create personal thumbnail cache directories and final thumbnail files with the private permissions required by the Freedesktop standard.
- Treat failure entries as metadata-carrying PNG files in program-version failure namespaces, not as successful thumbnail size entries.
- Provide opt-in failure-entry writing when the caller supplies an explicit validated program-version namespace and original identity, without deciding application retry policy.
- Treat shared thumbnail repositories as read-only during initial lookup, cleanup, and application generation; shared-repository writes are outside the initial library and CLI responsibilities.
- Return structured, policy-neutral inspection facts without applying CLI cleanup policy. Invalid PNG structure, missing required metadata, invalid metadata syntax, stale metadata, nonconforming PNG encoding, and dimension violations must remain distinguishable facts.
- Provide cache entry handles for entries discovered by library iteration or explicit cache-path resolution, and implement safe removal on those handles with containment checks and no symlink following. The removal design should prefer directory-relative APIs such as `openat` and `unlinkat`, or a capability-style equivalent, so containment and symlink checks are not only string-prefix checks. Any fallback that cannot provide that strength must be documented and reported as best-effort.
- Expose thumbnail file timestamp facts, including modification time, access time when available, and whether metadata inspection preserved access time, without deciding age-based cleanup policy.
- Avoid exposing user-facing URI classification, age thresholds, deletion reasons, or cleanup decisions from the library API.

The library should avoid depending on CLI-only concerns such as terminal formatting, progress bars, command-line parsing, logging configuration, user-specific cleanup defaults, or user-facing report vocabulary. Image rendering, thumbnailer execution, renderer temporary-file management, source metadata extraction, scaling decisions, and user-facing failure policy are outside the base crate scope; rendered-thumbnail payload normalization, Freedesktop metadata writing, and atomic personal-cache installation from in-memory payloads are library responsibilities.

The `x-large` and `xx-large` size classes are treated as supported documented behavior from the Freedesktop Thumbnail Managing Standard `latest` text, including the December 2020 0.9.0 history entry.

## Prune CLI Responsibilities

- Parse command-line options such as `--older-than`, `--delete`, `--delete-stale-local`, `--allow-delete-failures`, `--size`, `--scope`, `--age-basis`, `--include-nonstandard-files`, `--format`, `--ignore-fhs-media`, `--verbose`, and repeated custom removable path hints.
- Classify URI schemes and path prefixes according to user-facing cleanup policy.
- Apply age-based cleanup for remote, virtual, and removable-media-like entries.
- Inspect and classify failure entries when the user includes failure namespaces in the scan scope, while requiring an extra failure-deletion opt-in before treating them as deletion candidates.
- Own cleanup policy types such as URI classes, deletion reasons, skip reasons, and cleanup decisions.
- Request removal through library cache entry handles only after the prune CLI has made an explicit deletion decision, and only when the relevant destructive flags are present.
- Delete successful thumbnail entries only when `--delete` is passed, failure entries only when both `--delete` and `--allow-delete-failures` are passed, and report what was removed, skipped, or left unchanged.
- Skip nonstandard cache filenames by default and expose them only as reportable skipped entries when the user passes `--include-nonstandard-files`.
- Provide conservative defaults and clear report output before destructive cleanup.
- Convert library errors into actionable CLI diagnostics and exit codes.

## Generate CLI Responsibilities

- Parse command-line options such as `--size`, `--force`, `--dry-run`, `--timeout`, `--sandbox`, `--format`, and `--verbose`.
- Resolve relative local input paths against the current working directory into absolute paths, apply generate CLI input policy such as recursive-cache rejection, and call the library to construct canonical personal-cache thumbnail URIs without hidden symlink normalization.
- Reject inputs located under the personal thumbnail cache or a shared `.sh_thumbnails` repository.
- Discover `.thumbnailer` files from `$XDG_DATA_HOME/thumbnailers` and `$XDG_DATA_DIRS` thumbnailer directories.
- Parse thumbnailer `Exec`, `TryExec`, and `MimeType` keys using key-file parsing, desktop-entry-style command tokenization, and thumbnailer-specific field-code expansion.
- Determine input MIME types through the platform shared MIME database, including canonical aliases and subtype relationships exposed by that database, and select a matching thumbnailer deterministically.
- Run selected thumbnailer commands directly as argument vectors with thumbnailer-specific `%i`, `%u`, `%o`, `%s`, and `%%` field-code expansion inside the configured sandbox.
- Use temporary output paths for thumbnailer execution and never expose partial output as a cache entry.
- Validate generated PNG output against successful-thumbnail namespace requirements before installation.
- Ask the library to write required personal-cache metadata such as `Thumb::URI` and `Thumb::MTime`, plus optional metadata when available.
- Ask the library to install generated thumbnails atomically under the resolved personal thumbnail cache root.
- Skip valid existing thumbnails unless `--force` is passed.
- Report generated, kept, skipped, and failed input-size pairs in human and JSONL formats.
- Avoid writing shared thumbnail repositories or failure entries in the initial generate CLI.

## Thumbnailer Sandbox

The initial generate CLI sandbox backend is `bubblewrap` (`bwrap`). Sandbox setup belongs to the generate CLI crate because it is tied to external thumbnailer execution, command expansion, temporary renderer output, and user-facing `--sandbox` policy rather than Freedesktop cache inspection or installation.

In `--sandbox required` mode, the generate CLI must fail before executing a thumbnailer when `bwrap` is unavailable or when the requested isolation cannot be applied. There is no implicit unsandboxed fallback. `--sandbox off` is an explicit user opt-out and should be reflected in human and JSONL reports.

The sandbox should create a private mount namespace and unshare networking. The thumbnailer should receive read access to the selected input, read access to required system resources such as executable paths, interpreters, dynamic loader state, MIME data, codecs, and font configuration, and write access only to a private temporary output directory owned by the generate CLI. The sandbox should not expose the user's home, personal thumbnail cache, XDG configuration directories, XDG data directories, or arbitrary writable host paths unless a later spec explicitly defines a compatibility mode. If the selected executable, interpreter, or required runtime files cannot be exposed read-only under `--sandbox required`, the generate CLI reports a sandbox eligibility failure and does not run that thumbnailer unsandboxed.

`%i` and `%o` expand to sandbox-visible paths. `%u` remains the canonical original URI used for cache identity, hashing, and metadata, even when the sandbox-visible input path differs from the host path. The generate CLI owns the mapping between host input and sandbox input, the private temporary output directory, and cleanup of temporary files after it has read the generated PNG into memory.

Executable and `TryExec` lookup should happen before entering the sandbox using desktop-entry-compatible path lookup rules. The sandbox must then expose enough read-only system paths for the resolved executable and its runtime dependencies to start. If a thumbnailer explicitly names a shell in `Exec`, that shell still runs inside the sandbox.

Landlock may be considered later as an additional hardening layer or fallback for specific filesystem restrictions, but it is not the initial backend because the generate CLI requires a network namespace and predictable path exposure for external thumbnailer processes.

## Shared Types

The library should expose policy-neutral types that CLI crates can combine into user-facing behavior.

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

`FailureNamespace` values must be validated direct directory names before use. The initial accepted character set is ASCII letters, digits, `.`, `_`, `+`, and `-`; empty values, `.`, `..`, path separators, NUL, and control characters are rejected.

The exact names can change during implementation, but the direction should remain: the library describes cache entries and filesystem facts with enough precision to avoid accidental cleanup policy, while the prune CLI classifies entries, decides which cleanup policy to run, and requests destructive changes only through library-provided cache entry handles.

## URI Classification Boundary

URI classification should be extensible because desktop environments and mounted filesystems vary. User-facing cleanup classification belongs to the prune CLI layer. The library may report that an original URI cannot be validated as a directly checkable local file, but it must not label the URI as remote, virtual, removable, or safe to delete by age. The prune CLI may parse a canonical thumbnail URI for lossless syntactic facts such as scheme and authority before applying cleanup classification, but the parsed form must not replace the library-owned canonical string used for hashing and metadata comparison.

Prune CLI-side classification can use types shaped like this:

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

The prune CLI default classifier should handle stable URI scheme categories and user-configurable path prefixes. It should treat `/media`, `/run/media/$UID`, `/run/user/$UID/doc`, GVfs, and KIO FUSE paths as removable, portal, or desktop-managed by default; `/media` can be disabled with `--ignore-fhs-media`; `/mnt` is excluded by default and can be added with repeated `--removable-prefix` options.

For `file:` URIs, the default classifier should only treat empty authority and `localhost` authority as directly checkable local paths. Other authorities should be classified conservatively as remote or unknown unless an implementation-specific resolver is added. Direct local checks must distinguish confirmed absence from permission errors, transient I/O errors, and unsupported path conversion so cleanup policy can skip unverifiable originals instead of deleting them as missing.
