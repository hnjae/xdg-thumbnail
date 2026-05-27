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

The CLI crate is the policy runner. It should translate user input into cleanup policy, call the library to inspect cache entries, and perform filesystem mutations only after the library returns a decision.

## Library Responsibilities

- Resolve the thumbnail cache root from the XDG base directory rules.
- Represent thumbnail sizes: `normal`, `large`, `x-large`, and `xx-large`.
- Compute thumbnail filenames from canonical thumbnail URIs using MD5 and the `.png` suffix.
- Read and write standard PNG text metadata such as `Thumb::URI`, `Thumb::MTime`, `Thumb::Size`, and `Thumb::Mimetype`.
- Iterate cache entries from the global thumbnail cache and optional shared thumbnail repositories.
- Validate thumbnails by comparing stored metadata with the original file metadata where that comparison is meaningful.
- Save thumbnails atomically by writing a temporary PNG in the target directory and renaming it to the final path.
- Apply spec-compatible permissions for the personal cache: directories should be private to the user and thumbnail files should not be world-readable.
- Return structured inspection and cleanup decisions without directly applying CLI policy.

The library should avoid depending on CLI-only concerns such as terminal formatting, progress bars, command-line parsing, logging configuration, or user-specific cleanup defaults.

## CLI Responsibilities

- Parse command-line options such as `--older-than`, `--dry-run`, `--size`, `--include-failures`, `--verbose`, and custom removable path hints.
- Classify URI schemes and path prefixes according to user-facing cleanup policy.
- Apply age-based cleanup for remote, virtual, and removable-media-like entries.
- Delete files when requested and report what was removed, skipped, or left unchanged.
- Provide conservative defaults and clear dry-run output before destructive cleanup.
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

pub enum UriClass {
    LocalStableFile,
    LocalRemovableOrPortal,
    Remote,
    ArchiveOrVirtual,
    Unknown,
}

pub enum CacheEntryState {
    Valid,
    OriginalMissing,
    Outdated,
    UnreadableOriginal,
    RemoteOrUnknown,
    Malformed,
}

pub enum CleanupDecision {
    Keep,
    Delete(DeleteReason),
    Recreate,
    Skip(SkipReason),
}
```

The exact names can change during implementation, but the direction should remain: the library describes cache entries and candidate actions, while the CLI decides which policy to run and applies destructive changes.

## URI Classification Boundary

URI classification should be extensible because desktop environments and mounted filesystems vary. The library should provide a default classifier and allow callers to supply their own classifier.

```rust
pub trait UriClassifier {
    fn classify(&self, uri: &url::Url) -> UriClass;
}
```

The default classifier should handle stable, spec-derived behavior. The CLI should layer user-configurable path prefixes and desktop-specific heuristics on top, including GVfs, KIO FUSE, `/media`, `/run/media`, and `/mnt`.
