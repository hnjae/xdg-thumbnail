# Crate Boundaries

The target repository shape is a Cargo workspace with one reusable library crate and separate CLI crates for pruning and generation. The current skeleton may omit planned crates until their implementation work starts; the target layout below documents intended ownership boundaries rather than a guarantee that every crate exists in the initial skeleton.

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
- Represent personal absolute thumbnail URIs and shared-repository relative thumbnail URIs as separate library-owned string newtypes that preserve the exact MD5 and `Thumb::URI` input for their own context.
- Construct canonical personal thumbnail URIs for local filesystem paths and textual local `file:` URI inputs, construct canonical shared-repository relative thumbnail URIs from raw direct-child filenames, and validate and preserve caller-selected stable absolute personal URI identities for other schemes without parser reserialization or scheme-specific normalization.
- Use maintained dependencies for commodity primitives such as MD5 digest calculation, byte percent-encoding, and optional URI syntax validation, while keeping canonical thumbnail URI identity and cache filename policy in the library instead of exposing parser-specific URL objects or handwritten digest code.
- Represent thumbnail sizes: `normal`, `large`, `x-large`, and `xx-large`.
- Represent cache namespaces separately for successful thumbnail sizes and program-version failure entries.
- Compute thumbnail filenames from canonical thumbnail URIs using MD5 and the `.png` suffix, including absolute canonical URIs for the personal cache and `./`-prefixed relative URIs for shared repositories, as specified in `docs/spec/uri-canonicalization.md`.
- Expose pure computed-path APIs separately from validation APIs so callers cannot mistake an MD5-derived cache path for a display-valid thumbnail.
- Compute shared-repository cache paths only from an explicit shared repository context that includes the repository root, direct child original filename, and shared relative URI, so personal-cache absolute URI identity is never reused as a shared-repository lookup key.
- Keep raw shared-repository filename construction separate from textual shared-URI parsing. Raw filenames with literal percent-looking text such as `dir%2Fpicture.png` are valid direct child names and are percent-encoded as `./dir%252Fpicture.png`; textual shared URIs that decode to multiple path segments, such as `./dir%2Fpicture.png`, are rejected. On Unix-like targets, backslash remains a filename byte and is preserved through percent-encoding when needed.
- Read standard PNG text metadata such as `Thumb::URI`, `Thumb::MTime`, `Thumb::Size`, and `Thumb::Mimetype`.
- Write standard PNG text metadata such as required `Thumb::URI` and `Thumb::MTime`, plus optional `Thumb::Size`, `Thumb::Mimetype`, and media-specific keys when the caller supplies them.
- Require `Thumb::URI` and `Thumb::MTime` for personal-cache validation and compare `Thumb::MTime` as whole Unix epoch seconds.
- Expose validated path lookup for callers that need a toolkit-consumable path while documenting the post-validation replacement race when the caller reopens that path.
- Expose validated payload lookup as the preferred display surface for thumbnail views, returning the exact cache PNG bytes that passed validation together with path and metadata facts when the caller requests payload output. Opened handles may be added later as optimized variants.
- Make path-versus-bytes output selection explicit in lookup and install APIs instead of returning an ambiguous path-or-payload union.
- Reject personal-cache successful thumbnail and failure-entry writes when the caller cannot provide a readability-confirmed original identity with an original modification time, because such entries violate the Freedesktop write preconditions or cannot satisfy global-cache freshness checks.
- Iterate cache entries from the personal thumbnail cache and optional shared thumbnail repositories.
- Validate personal and shared thumbnails with separate contexts because shared repositories use direct child relative URIs and may omit `Thumb::URI` or `Thumb::MTime` when they use other freshness mechanisms.
- Inspect successful thumbnail PNGs against the required image format and maximum dimensions for the selected size class.
- Normalize supported caller-provided rendered PNG payloads into Freedesktop-conforming 8-bit non-interlaced RGBA PNG output before successful personal-cache installation, downscale rendered output when needed to fit the requested cache namespace, optionally encode explicitly described raw pixel buffers through the same final PNG path, and return the installed path plus optional final normalized PNG bytes when requested.
- Install successful personal-cache thumbnails atomically from caller-provided rendered thumbnail payloads and readability-confirmed original identities, after normalizing the final PNG encoding and cache-size dimensions for the requested size namespace.
- Create missing personal thumbnail cache directories with mode `0700` and final thumbnail files with mode `0600`; reject existing standard personal-cache directories that are not owned by the current user or that grant group/other access, while reporting explicit permission errors instead of silently rewriting existing directory permissions.
- Treat failure entries as metadata-carrying PNG files in program-version failure namespaces, not as successful thumbnail size entries.
- Provide opt-in failure-entry writing when the caller supplies an explicit validated program-version namespace and a readability-confirmed original identity, without deciding application retry policy. The initial writer should generate a deterministic minimal 1x1 transparent RGBA PNG instead of accepting caller-rendered failure payloads.
- Treat shared thumbnail repositories as read-only during initial lookup, cleanup, and application generation; shared-repository writes are outside the initial library and CLI responsibilities.
- Return structured, policy-neutral inspection facts without applying CLI cleanup policy. Invalid PNG structure, missing required metadata, invalid metadata syntax, stale metadata, nonconforming PNG encoding, and dimension violations must remain distinguishable facts.
- Provide cache entry handles for entries discovered by library iteration or explicit cache-path resolution, and implement safe removal on those handles with containment checks and no symlink following. The removal design should prefer directory-relative APIs such as `openat` and `unlinkat`, or a capability-style equivalent, so containment and symlink checks are not only string-prefix checks. Any fallback that cannot provide that strength must be documented and reported as best-effort.
- Expose thumbnail file timestamp facts, including modification time, access time when available, and whether metadata inspection preserved access time, without deciding age-based cleanup policy.
- Avoid exposing user-facing URI classification, age thresholds, deletion reasons, or cleanup decisions from the library API.

The library boundary for non-local and virtual originals is validation and preservation, not global URL canonicalization. Applications that own KIO-style, remote, archive-entry, resolved-playback, temporary-extraction, or application-specific sources choose the stable original URI identity for those sources; the library rejects values that cannot be absolute thumbnail URI identities and then uses the accepted bytes unchanged for MD5 filenames, `Thumb::URI`, and metadata comparison. The library must not fetch those sources, mount them, inspect archive members, follow playback resolver paths, or parse and reserialize the supplied URI as a hidden normalization step.

The library should avoid depending on CLI-only concerns such as terminal formatting, progress bars, command-line parsing, logging configuration, user-specific cleanup defaults, or user-facing report vocabulary. Image rendering, thumbnailer execution, renderer temporary-file management, source metadata extraction, source-aware aspect-ratio decisions, animation frame selection, aspect-ratio repair, and user-facing failure policy are outside the base crate scope; rendered PNG normalization to the Freedesktop final cache format, cache-size downscaling of rendered output, optional raw-pixel PNG encoding, Freedesktop metadata writing, and atomic personal-cache installation from rendered payloads are library responsibilities.

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
- Preserve non-UTF-8 thumbnail path bytes in JSONL reports through explicit lossless byte fields rather than relying on human-oriented display strings.
- Convert library errors into actionable CLI diagnostics and exit codes.

## Generate CLI Responsibilities

- Parse command-line options such as `--size`, `--force`, `--dry-run`, `--timeout`, `--sandbox`, `--format`, and `--verbose`.
- Resolve relative local input paths against the current working directory into absolute paths, apply generate CLI input policy such as recursive-cache rejection, and call the library to construct canonical personal-cache thumbnail URIs without hidden symlink normalization.
- Reject inputs located under the personal thumbnail cache or a shared `.sh_thumbnails` repository.
- Confirm local input readability and obtain original metadata before keeping an existing thumbnail as valid or asking the library to install a generated thumbnail.
- Discover `.thumbnailer` files from `$XDG_DATA_HOME/thumbnailers` and `$XDG_DATA_DIRS` thumbnailer directories, preserving each candidate's discovery origin so sandbox eligibility and diagnostics can distinguish user-provided and system-provided thumbnailers.
- Parse thumbnailer `Exec`, `TryExec`, and `MimeType` keys using key-file parsing, desktop-entry-style command tokenization, and thumbnailer-specific field-code expansion.
- Determine input MIME types through the platform shared MIME database, including canonical aliases and subtype relationships exposed by that database, and select a matching thumbnailer deterministically.
- Run selected thumbnailer commands directly as argument vectors with thumbnailer-specific `%i`, `%u`, `%o`, `%s`, and `%%` field-code expansion inside the configured sandbox after non-mutating sandbox eligibility checks confirm that the selected entry fits the documented sandbox profile, or after the user explicitly chooses `--sandbox off`. The generate CLI must keep the canonical cache identity URI separate from the sandbox-visible `%u` value passed to the thumbnailer.
- Use temporary output paths for thumbnailer execution and never expose partial output as a cache entry.
- Verify that thumbnailer output exists and is readable before handing it to the library.
- Ask the library to normalize supported rendered PNG output to the final cache PNG format, downscale rendered output when needed to fit successful-thumbnail namespace requirements, write required personal-cache metadata such as `Thumb::URI` and `Thumb::MTime`, write optional metadata when available, and install generated thumbnails atomically under the resolved personal thumbnail cache root. The selected thumbnailer or renderer owns source-aware aspect-ratio behavior; the generate CLI does not decode every source format to repair aspect-ratio mistakes.
- Skip valid existing thumbnails unless `--force` is passed.
- Report generated, kept, skipped, and failed input-size pairs in human and JSONL formats.
- Preserve non-UTF-8 input and cache path bytes in JSONL reports through explicit lossless byte fields rather than relying on human-oriented display strings.
- Avoid writing shared thumbnail repositories or failure entries in the initial generate CLI.

## Thumbnailer Sandbox

The initial generate CLI sandbox backend is `bubblewrap` (`bwrap`) on Linux, and the default generate command uses `--sandbox required`. Sandbox setup belongs to the generate CLI crate because it is tied to external thumbnailer execution, command expansion, temporary renderer output, and user-facing `--sandbox` policy rather than Freedesktop cache inspection or installation.

In `--sandbox required` mode, the generate CLI must fail before executing a thumbnailer when `bwrap` is unavailable, when the host is not Linux, when the requested isolation cannot be applied, or when the selected thumbnailer does not fit the documented sandbox profile. CLI help, diagnostics, dry-run output, and summaries for such failures must make the Linux `bubblewrap` default requirement explicit. In `--dry-run` mode these same checks are reported as planned failures without executing thumbnailers or mutating the cache, and they should be represented per input-size pair when enough information is available. There is no implicit unsandboxed fallback. `--sandbox off` is an explicit user opt-out and should be reflected in human and JSONL reports.

The sandbox should create a private mount namespace and unshare networking. The thumbnailer should receive read access to the selected input, read access to documented read-only system runtime resources, and write access only to a private temporary output directory owned by the generate CLI. The initial profile may expose read-only system locations such as `/usr`, `/bin`, `/sbin`, `/lib`, `/lib64`, and `/etc` when needed for ordinary system thumbnailers to start. User-controlled locations such as the user's home, personal thumbnail cache, `$XDG_CONFIG_HOME`, `$XDG_DATA_HOME`, user entries from `$XDG_CONFIG_DIRS` or `$XDG_DATA_DIRS`, and arbitrary writable host paths must not be exposed wholesale unless a later spec explicitly defines a compatibility mode. The generate CLI is not required to infer arbitrary runtime dependencies for user-provided thumbnailers, plugins, codecs, configuration, or helper programs outside the documented profile. A selected user-provided thumbnailer may run in `--sandbox required` only when the resolved command and literal host paths fit the same documented sandbox profile as system entries. Shell-based thumbnailer entries are not eligible in the initial required sandbox because shell command strings cannot be bounded to the documented path exposure model without broader command analysis. Otherwise, the generate CLI reports `sandbox-ineligible` and does not run that thumbnailer unsandboxed.

`%i` and `%o` expand to sandbox-visible paths. `%u` expands to a thumbnailer input URI that the external process can open; under `--sandbox required`, this may be a `file:` URI for the sandbox-visible input path rather than the host path. The canonical original URI used for cache identity, hashing, `Thumb::URI`, and validation remains a separate value derived from the host input path. The generate CLI owns the mapping between host input and sandbox input, the private temporary output directory, and cleanup of temporary files after it has read the generated PNG into memory.

Executable and `TryExec` lookup should happen before entering the sandbox using desktop-entry-compatible path lookup rules. Sandbox eligibility should then check the resolved executable, resolved script interpreter if any, and literal host paths in the command template against the documented sandbox profile. If a thumbnailer explicitly names a shell in `Exec`, or resolves to a shell through an interpreter wrapper, the initial `--sandbox required` mode reports `sandbox-ineligible`; shell command support requires `--sandbox off` or a later documented compatibility mode.

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

pub struct PersonalThumbnailUri {
    value: String,
}

pub struct SharedRelativeThumbnailUri {
    value: String,
}

pub struct UnixMTimeSeconds {
    seconds: i64,
}

pub struct OriginalIdentity {
    uri: PersonalThumbnailUri,
    mtime: UnixMTimeSeconds,
    size: Option<u64>,
    mime_type: Option<String>,
}

pub struct ReadableOriginalIdentity {
    identity: OriginalIdentity,
}

pub struct SharedRepositoryContext {
    repository_root: std::path::PathBuf,
    original_child_name: std::ffi::OsString,
    shared_uri: SharedRelativeThumbnailUri,
}

pub enum ThumbnailUriIdentity {
    Personal(PersonalThumbnailUri),
    Shared(SharedRelativeThumbnailUri),
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
    SharedMetadataIncomplete,
    UncheckedInspection,
    Invalid(Vec<CacheEntryProblem>),
}

pub struct CacheEntryInspection {
    outcome: ValidationOutcome,
    original_uri: Option<ThumbnailUriIdentity>,
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

`UnixMTimeSeconds` stores the whole Unix epoch seconds used in `Thumb::MTime`; constructors from filesystem metadata must define truncation, pre-epoch, and overflow handling before validation or writes depend on the value. `PersonalThumbnailUri` and `SharedRelativeThumbnailUri` remain separate types so an absolute personal-cache URI cannot accidentally be reused as a shared-repository lookup key. `FailureNamespace` values must be validated direct directory names before use. The initial accepted character set is ASCII letters, digits, `.`, `_`, `+`, and `-`; empty values, `.`, `..`, path separators, NUL, and control characters are rejected.

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
    fn classify(&self, uri: &PersonalThumbnailUri) -> UriClass;
}
```

The prune CLI default classifier should handle stable URI scheme categories and user-configurable path prefixes. It should treat `/media`, `/run/media/$UID`, `/run/user/$UID/doc`, GVfs, and KIO FUSE paths as removable, portal, or desktop-managed by default; `/media` can be disabled with `--ignore-fhs-media`; `/mnt` is excluded by default and can be added with repeated `--removable-prefix` options.

For `file:` URIs, the default classifier should only treat empty authority and `localhost` authority as directly checkable local paths. Other authorities should be classified conservatively as remote or unknown unless an implementation-specific resolver is added. Direct local checks must distinguish confirmed absence from permission errors, transient I/O errors, and unsupported path conversion so cleanup policy can skip unverifiable originals instead of deleting them as missing.
