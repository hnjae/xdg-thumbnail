# Library API Specification

The `xdg-thumbnail` library provides reusable primitives for applications that need to read, validate, inspect, install, record failure entries for, or safely remove Freedesktop thumbnail cache entries.

Initial platform support targets Unix-like XDG desktop environments. APIs that depend on Unix path bytes, Unix file metadata, or XDG cache semantics should return an explicit unsupported-platform error on platforms where those semantics cannot be represented reliably.

## Public API Goals

- Make spec-compatible thumbnail path calculation straightforward.
- Make thumbnail URI canonicalization explicit and shared by path calculation, metadata comparison, and validation.
- Use the same canonical thumbnail URI bytes for hashing, metadata comparison, and validation.
- Provide canonical URI constructors for local filesystem paths and shared-repository child filenames, while allowing callers to provide already-canonical absolute URI identities for non-local backends without changing the identity used for hashing.
- Allow applications to validate cached thumbnails without duplicating PNG metadata parsing.
- Allow applications that own their own rendering or decoding stack to install spec-compatible personal-cache thumbnails without duplicating Freedesktop PNG metadata, permission, and atomic-save rules.
- Allow management tools to inspect cache entries without embedding CLI cleanup policy into the library.
- Allow management tools to request safe removal of inspected cache entries without reimplementing thumbnail-cache path containment and symlink checks.

## Core Capabilities

The library should expose APIs for:

- Resolving the personal thumbnail cache root.
- Constructing canonical personal-cache `file:` URI strings from absolute local path bytes without lossy path conversion. Callers may resolve relative user input before calling this constructor, but the library owns the canonical URI identity used for hashing, `Thumb::URI`, metadata comparison, and validation.
- Constructing canonical personal-cache `file:` URI strings from textual local file URI input as a separate path from the absolute-path constructor, including normalizing `file://localhost/...` to `file:///...` while rejecting or reporting non-local `file:` authorities as not directly checkable unless an explicit resolver is added.
- Constructing canonical shared-repository URI strings only for direct child originals represented by `./`-prefixed relative URIs.
- Accepting caller-provided canonical absolute URI identities for non-local backends and preserving the supplied identity bytes for hashing and metadata comparison.
- Computing the personal-cache path for a canonical absolute original URI and requested thumbnail namespace.
- Computing shared-repository cache paths only from an explicit shared repository context that includes the repository root, the direct child original filename, and the shared `./`-prefixed URI used for hashing and metadata comparison.
- Representing cache namespaces separately for successful thumbnail sizes and program-version failure entries.
- Parsing thumbnail PNG metadata.
- Writing successful personal-cache thumbnail PNG metadata from a caller-provided readability-confirmed original identity and caller-provided in-memory thumbnail payload.
- Installing successful personal-cache thumbnails atomically under the resolved personal cache root from a readability-confirmed original identity and rendered thumbnail payload after validating the target namespace and payload dimensions.
- Inspecting whether successful thumbnails are 8-bit non-interlaced PNG files with full alpha support and dimensions that fit the requested size class.
- Checking whether a personal-cache thumbnail is valid for a given original by verifying required `Thumb::URI` and `Thumb::MTime` metadata and verifying `Thumb::Size` when present.
- Checking shared-repository thumbnails with a separate validation context where the shared repository root and direct child original filename are explicit, present `Thumb::URI`, `Thumb::MTime`, and `Thumb::Size` values are verified, but missing `Thumb::URI` or `Thumb::MTime` does not automatically make the entry invalid.
- Iterating cache entries from known thumbnail directories.
- Returning policy-neutral inspection facts for cache management tools, including thumbnail timestamps and whether metadata inspection preserved access time.
- Distinguishing inspection facts precisely enough that callers do not have to infer cleanup policy from a coarse invalid state.
- Returning cache entry handles that identify entries discovered by library iteration and can remove those entries safely when the caller has already made a deletion decision.
- Reading shared thumbnail repositories without modifying them.
- Writing opt-in failure entries under `fail/<program-version>/` when the caller explicitly supplies the failure namespace and a readability-confirmed original identity.

## Generation Boundary

The base library API does not decode original source images, render documents, extract video frames, select thumbnailer helpers, scale source content, apply source orientation metadata, manage renderer temporary files, or decide when generation should be attempted. It may decode and normalize an already rendered in-memory thumbnail payload, write Freedesktop thumbnail PNG metadata, and atomically install personal-cache entries when the caller supplies that payload with a readability-confirmed original identity.

Personal-cache successful-thumbnail and failure-entry write APIs require a readability-confirmed original identity with a canonical thumbnail URI and a modification time in whole Unix epoch seconds. The library is not expected to prove readability for every possible URI scheme. Instead, write APIs should make the readability requirement explicit in the type or constructor used to create the write identity. For local `file:` originals, the library should provide a convenience constructor that opens the original for reading and derives the canonical URI, modification time, and optional size from that readable file handle. For non-local or virtual backends, callers that own the backend may provide an explicitly backend-confirmed readable identity containing the canonical URI, current modification time, optional size, and optional MIME type. If the caller cannot confirm original readability or cannot obtain the original modification time, the library must reject a global personal-cache successful thumbnail or failure-entry write instead of creating an entry that violates the Freedesktop write preconditions or cannot be validated later. `Thumb::Size` and `Thumb::Mimetype` should be written when the caller supplies them.

Successful-thumbnail write APIs should initially accept caller-provided PNG bytes as the primary rendered payload. Callers that render to temporary files, including `.thumbnailer` `%o` outputs, must read the rendered thumbnail into memory before calling the library. The initial public write contract does not accept filesystem paths as rendered-thumbnail payloads.

Successful-thumbnail write APIs must normalize accepted rendered payloads to a Freedesktop-conforming final PNG before installation. Regardless of whether the caller supplies PNG bytes or a supported raw pixel buffer, the installed successful thumbnail must be an 8-bit non-interlaced RGBA PNG. PNG inputs without alpha are expanded with opaque alpha, grayscale inputs are expanded to RGB channels, grayscale-alpha inputs are expanded to RGBA, and indexed-color inputs are expanded before writing. The initial contract should reject PNG inputs whose bit depth, color interpretation, animation, or ancillary data cannot be converted deterministically without adding source-rendering policy.

The library may also expose a narrow convenience API for caller-provided raw pixel buffers, such as explicit `RGBA8` or `RGB8` with dimensions and stride, when this does not expand the library into source decoding or rendering. Raw pixel APIs must define pixel format, alpha handling, color assumptions, row layout, and dimension validation explicitly. Raw pixel inputs are normalized through the same final RGBA8 PNG write path and remain secondary to the PNG-bytes install API.

Write APIs must create missing standard personal-cache directories with mode `0700`, install final thumbnail files with mode `0600`, write temporary files in the destination directory, and publish final entries with an atomic rename. Existing cache directories whose permissions differ are not silently rewritten by the initial library API; the write either proceeds when the directory can be used safely by the current user or fails with an explicit permission diagnostic. A write must not expose a partial thumbnail at the final cache path. The library must validate the normalized final PNG against the requested namespace before installation, regardless of whether the input was PNG bytes or a supported raw pixel buffer.

For personal-cache validation, missing `Thumb::URI`, a `Thumb::URI` value that differs from the canonical original URI, missing `Thumb::MTime`, or a `Thumb::MTime` value that differs from the original modification time in whole seconds makes the thumbnail invalid for display. `Thumb::Size` should be compared when present. Management tools should distinguish missing required metadata and invalid metadata syntax from metadata that is well-formed but stale for an existing original.

For shared-repository validation, `Thumb::URI`, `Thumb::MTime`, and `Thumb::Size` should be compared when present. Missing `Thumb::URI` or `Thumb::MTime` should produce a validation result that is usable by callers that accept shared repository freshness policy, but it must not be reported as equivalent to a fully metadata-validated personal-cache thumbnail. The public result type should distinguish at least fully verified entries, shared entries accepted by caller policy despite missing freshness metadata, unchecked inspection results, and invalid entries.

Application validation APIs must not return an existing personal-cache thumbnail as display-valid when the caller cannot confirm that the original file is currently readable. For non-local backends, callers may provide an explicit original identity object containing the canonical thumbnail URI, current modification time, optional size, and proof that the original was readable through that backend. The same readability-confirmed identity model should be used by personal-cache write APIs, while separate cache-inspection APIs for management tools may still parse thumbnail files and metadata without opening the original. Inspection APIs must report policy-neutral facts and must not present the entry as a validated thumbnail for display.

Failure entries are separate from successful thumbnail size namespaces. They are PNG metadata carriers stored under `fail/<program-version>/`; successful-thumbnail dimension limits do not apply to them. The initial failure-entry write API should not accept caller-rendered image payloads. Instead, it should create a deterministic minimal 1x1 transparent 8-bit non-interlaced RGBA PNG containing required `Thumb::URI` and `Thumb::MTime` metadata and optional original metadata supplied by the caller. The library can locate, parse, inspect, and explicitly write failure entries, but it must not decide when an application should record a failure or suppress future generation attempts. Failure namespace values must be non-empty direct directory names: no `/`, NUL, control characters, `.`, or `..`. The initial public constructor should accept only ASCII letters, digits, `.`, `_`, `+`, and `-` so failure-entry writes cannot create nested paths or ambiguous namespace directories.

The library should not apply user-facing cleanup policy. It may report facts such as missing originals, unreadable originals, unsupported original URI for local validation, missing required metadata, invalid metadata syntax, well-formed metadata mismatches, unreadable PNG structure, nonconforming PNG encoding, successful-thumbnail dimension violations, thumbnail timestamps, whether access time was preserved during metadata inspection, and cache location, but age thresholds, removable path heuristics, URI class names, reason vocabulary, and deletion decisions belong to the caller.

The library must not collapse all invalid entries into a single generic error state. A PNG that can be parsed but lacks full alpha support, uses interlacing, or exceeds the selected successful-thumbnail namespace dimensions is nonconforming for lookup, not equivalent to an unreadable PNG or invalid identity metadata.

The library removal API must operate only on cache entry handles returned by library iteration or explicit cache-path resolution. It must verify that the target is still inside the resolved thumbnail cache directory, must not follow symlinks, and must report deletion failures without retrying outside the cache root.

## Non-Goals

- Decoding source images, rendering documents, extracting video frames, selecting thumbnailer helpers, or scaling source content is not a base library goal; generation orchestration belongs in `xdg-thumbnail-generate` or other separate applications and crates that own thumbnailer execution or image, document, and video decoding stacks.
- Automatically deciding retry suppression, creating failure entries after arbitrary errors, or applying application-specific failure policy is not a base library goal.
- The initial library API does not create or update shared thumbnail repositories. Shared repositories are read-only inputs for lookup and inspection.
- A future explicit shared-repository creation mode may be added only after its externally visible behavior is documented in `docs/spec/`. That future mode must remain opt-in, must use the shared relative URI rules in `docs/spec/uri-canonicalization.md`, and must document a permission model that preserves the original file's intended visibility while explicitly calling out any security-motivated deviation from the Freedesktop shared-repository text.
