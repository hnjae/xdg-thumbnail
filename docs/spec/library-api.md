# Library API Specification

The `xdg-thumbnail` library provides reusable primitives for applications that need to read, validate, inspect, or safely remove Freedesktop thumbnail cache entries.

Initial platform support targets Unix-like XDG desktop environments. APIs that depend on Unix path bytes, Unix file metadata, or XDG cache semantics should return an explicit unsupported-platform error on platforms where those semantics cannot be represented reliably.

## Public API Goals

- Make spec-compatible thumbnail path calculation straightforward.
- Make thumbnail URI canonicalization explicit and shared by path calculation, metadata comparison, and validation.
- Use the same canonical thumbnail URI bytes for hashing, metadata comparison, and validation.
- Provide canonical URI constructors for local filesystem paths and shared-repository child filenames, while allowing callers to provide already-canonical absolute URI identities for non-local backends without changing the identity used for hashing.
- Allow applications to validate cached thumbnails without duplicating PNG metadata parsing.
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
- Inspecting whether successful thumbnails are 8-bit non-interlaced PNG files with full alpha support and dimensions that fit the requested size class.
- Checking whether a personal-cache thumbnail is valid for a given original by verifying `Thumb::URI`, `Thumb::MTime`, and `Thumb::Size` according to the Freedesktop standard.
- Checking shared-repository thumbnails with a separate validation context where present `Thumb::URI`, `Thumb::MTime`, and `Thumb::Size` values are verified, but missing `Thumb::URI` or `Thumb::MTime` does not automatically make the entry invalid.
- Iterating cache entries from known thumbnail directories.
- Returning policy-neutral inspection facts for cache management tools, including thumbnail timestamps and whether metadata inspection preserved access time.
- Distinguishing inspection facts precisely enough that callers do not have to infer cleanup policy from a coarse invalid state.
- Returning cache entry handles that identify entries discovered by library iteration and can remove those entries safely when the caller has already made a deletion decision.
- Reading shared thumbnail repositories without modifying them.

## Generation Boundary

The base library API does not decode images, render documents, extract video frames, create thumbnail PNGs, write PNG metadata, write failure entries, or update thumbnail repositories. It may inspect existing PNG entries and report whether their metadata, encoding, and dimensions satisfy the Freedesktop standard for lookup and cleanup decisions.

For personal-cache validation, missing `Thumb::URI`, a `Thumb::URI` value that differs from the canonical original URI, missing `Thumb::MTime`, or a `Thumb::MTime` value that differs from the original modification time in whole seconds makes the thumbnail invalid for display. `Thumb::Size` should be compared when present. Management tools should distinguish missing required metadata and invalid metadata syntax from metadata that is well-formed but stale for an existing original.

For shared-repository validation, `Thumb::URI`, `Thumb::MTime`, and `Thumb::Size` should be compared when present. Missing `Thumb::URI` or `Thumb::MTime` should produce a validation result that is usable by callers that accept shared repository freshness policy, but it must not be reported as equivalent to a fully metadata-validated personal-cache thumbnail. The public result type should distinguish at least fully verified entries, shared entries accepted by caller policy despite missing freshness metadata, unchecked inspection results, and invalid entries.

Application validation APIs must not return an existing personal-cache thumbnail as display-valid when the caller cannot confirm that the original file is currently readable. For non-local backends, callers may provide an explicit original identity object containing the canonical thumbnail URI, current modification time, optional size, and proof that the original was readable through that backend. Separate cache-inspection APIs for management tools may still parse thumbnail files and metadata without opening the original, but they must report policy-neutral facts and must not present the entry as a validated thumbnail for display.

Failure entries are separate from successful thumbnail size namespaces. They are PNG metadata carriers stored under `fail/<program-version>/`; successful-thumbnail dimension limits do not apply to them. The initial library API can locate, parse, and inspect failure entries, but it does not write failure entries.

The library should not apply user-facing cleanup policy. It may report facts such as missing originals, unreadable originals, unsupported original URI for local validation, missing required metadata, invalid metadata syntax, well-formed metadata mismatches, unreadable PNG structure, nonconforming PNG encoding, successful-thumbnail dimension violations, thumbnail timestamps, whether access time was preserved during metadata inspection, and cache location, but age thresholds, removable path heuristics, URI class names, reason vocabulary, and deletion decisions belong to the caller.

The library must not collapse all invalid entries into a single generic error state. A PNG that can be parsed but lacks full alpha support, uses interlacing, or exceeds the selected successful-thumbnail namespace dimensions is nonconforming for lookup, not equivalent to an unreadable PNG or invalid identity metadata.

The library removal API must operate only on cache entry handles returned by library iteration or explicit cache-path resolution. It must verify that the target is still inside the resolved thumbnail cache directory, must not follow symlinks, and must report deletion failures without retrying outside the cache root.

## Non-Goals

- Creating, updating, rewriting, or saving successful personal-cache thumbnails is not a base library goal; generation belongs in `xdg-thumbnail-generate` or other separate applications and crates that own thumbnailer execution or image, document, and video decoding stacks.
- Writing failure entries is not a base library goal.
- The initial library API does not create or update shared thumbnail repositories. Shared repositories are read-only inputs for lookup and inspection.
- A future explicit shared-repository creation mode may be added only after its externally visible behavior is documented in `docs/spec/`. That future mode must remain opt-in, must use the shared relative URI rules in `docs/spec/uri-canonicalization.md`, and must document a permission model that preserves the original file's intended visibility while explicitly calling out any security-motivated deviation from the Freedesktop shared-repository text.
