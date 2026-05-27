# URI Canonicalization

Thumbnail path calculation and `Thumb::URI` metadata must use the same canonical thumbnail URI string. The library must not hash display paths, shell-expanded paths, lossy path conversions, or a different URI string than the one written into standard metadata. The canonical thumbnail URI should be represented by a library-owned string newtype so general-purpose URL parsers cannot silently reserialize or normalize the identity used for MD5 hashing.

## Personal Cache URIs

Personal-cache thumbnails use absolute canonical URIs. The MD5 input for the thumbnail filename is the exact UTF-8 byte sequence of that URI string.

For local filesystem paths, the public constructor should accept only absolute paths. It should emit a `file:///` URI with empty authority, percent-encode path bytes without lossy Unicode replacement, and never expand shell syntax such as `~`. The constructor should preserve the exact canonical string it returns for later hashing and metadata writing.

`file://localhost/...` is normalized to `file:///...` when constructing a local filesystem URI for personal-cache path calculation. Other `file:` authorities are not directly checkable local paths; cache inspection may report them, but application lookup must not validate them by probing the local filesystem unless an explicit resolver is added.

Relative `$XDG_CACHE_HOME` values are invalid under the XDG Base Directory rules and must be ignored. If `$XDG_CACHE_HOME` is unset, blank, or relative, the personal thumbnail root falls back to `$HOME/.cache/thumbnails`. If `$HOME` cannot be determined, cache root resolution must fail rather than invent a relative fallback.

The library should avoid filesystem canonicalization as a hidden URI normalization step. Resolving symlinks or changing path identity before hashing can make thumbnails incompatible with callers that use the visible path URI; callers that need a resolved path should resolve it before constructing the thumbnail URI.

## Shared Repository URIs

Shared thumbnail repositories are scoped to the directory that contains the original. A shared thumbnail URI must be `./` followed by one canonical, minimally percent-encoded path segment for the direct child filename.

Shared URI construction must reject empty filenames, `.` and `..`, path separators, parent segments, nested paths, encoded `/`, encoded `\`, and any input that decodes to multiple path segments. The rejected forms must not be normalized into an accepted shared URI because doing so can alias different originals.

When `Thumb::URI` is present in a shared thumbnail, it must match the shared relative URI used for filename hashing. Missing `Thumb::URI` or `Thumb::MTime` does not automatically invalidate a shared thumbnail because shared repositories may use external freshness mechanisms.

## Hashing

Thumbnail filenames are the lowercase hexadecimal MD5 digest of the canonical thumbnail URI string with `.png` appended. The hash is computed over the URI string, not over the original file contents.

The same canonical thumbnail URI string must be used for filename calculation, `Thumb::URI` metadata when that metadata is present, and validation comparisons. Divergence between those strings is a cache miss or invalid metadata, not an implementation detail to repair silently.

APIs may expose helper methods for scheme classification, display formatting, or conversion to a parsed URL where that is lossless for the specific URI. Those helpers must not replace the stored canonical string as the source of truth for hashing or metadata comparison.
