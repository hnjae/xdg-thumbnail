# URI Canonicalization

Thumbnail path calculation and `Thumb::URI` metadata must use the same canonical thumbnail URI bytes. The implementation must not hash display paths, shell-expanded paths, lossy path conversions, IRIs, or a different URI string than the one stored in standard metadata.

Initial platform support targets Unix-like XDG desktop environments. Local filesystem URI construction depends on Unix path identity and path bytes; unsupported platforms must fail explicitly instead of approximating URI or path behavior that could produce incompatible thumbnail filenames.

## Personal Cache URIs

Personal-cache thumbnails use absolute canonical URIs. The MD5 input for the thumbnail filename is the exact byte sequence of the canonical URI string after URI percent-encoding. The URI used for hashing is a URI identity, not a user-facing display path.

For local filesystem paths, the public constructor should accept only absolute paths. It should emit a `file:///` URI with empty authority, percent-encode path bytes without lossy Unicode replacement, and never expand shell syntax such as `~`. The exact emitted URI bytes are the bytes used later for hashing and metadata validation.

`file://localhost/...` is normalized to `file:///...` when constructing a local filesystem URI for personal-cache path calculation. Other `file:` authorities are not directly checkable local paths; cache inspection may report them, but application lookup must not validate them by probing the local filesystem unless an explicit resolver is added.

For non-local personal-cache URIs, the library should not attempt to canonicalize every possible URI scheme. Callers that own a backend such as `http:`, `smb:`, `dav:`, `trash:`, or an application-specific virtual scheme may provide an already-canonical absolute URI string. The library should preserve that URI identity exactly for hashing and metadata comparison. Validation should be limited to rejecting values that cannot be absolute URI identities, including relative references, control characters, and unescaped non-ASCII IRI text. The library must not parse and reserialize caller-provided URI strings as a hidden normalization step.

Local path URI construction percent-encodes every byte that is not safe in the canonical URI path form and emits uppercase hexadecimal escape digits. Existing literal percent bytes in the filesystem path are encoded as `%25`; callers that already have a non-local canonical URI string should use the caller-provided URI identity API instead of passing URI text through the local path constructor.

Relative `$XDG_CACHE_HOME` values are invalid under the XDG Base Directory rules and must be ignored. If `$XDG_CACHE_HOME` is unset, blank, or relative, the personal thumbnail root falls back to `$HOME/.cache/thumbnails`. If `$HOME` cannot be determined, cache root resolution must fail rather than invent a relative fallback.

The library should avoid filesystem canonicalization as a hidden URI normalization step. Resolving symlinks or changing path identity before hashing can make thumbnails incompatible with callers that use the visible path URI; callers that need a resolved path should resolve it before constructing the thumbnail URI.

## Shared Repository URIs

Shared thumbnail repositories are scoped to the directory that contains the original. A shared thumbnail URI must be `./` followed by one canonical, minimally percent-encoded path segment for the direct child filename.

Shared URI construction must reject empty filenames, `.` and `..`, slash path separators, parent segments, nested paths, encoded `/`, and any input that decodes to multiple path segments. The rejected forms must not be normalized into an accepted shared URI because doing so can alias different originals. On the initial Unix-like target platforms, backslash is a filename byte rather than a path separator and must be preserved through percent-encoding when needed.

When `Thumb::URI` is present in a shared thumbnail, it must match the shared relative URI used for filename hashing. Missing `Thumb::URI` or `Thumb::MTime` does not automatically invalidate a shared thumbnail because shared repositories may use external freshness mechanisms.

## Hashing

Thumbnail filenames are the lowercase hexadecimal MD5 digest of the canonical thumbnail URI string with `.png` appended. The hash is computed over the URI string, not over the original file contents.

The same canonical thumbnail URI string must be used for filename calculation, `Thumb::URI` metadata when that metadata is present, and validation comparisons. Divergence between those strings is a cache miss or invalid metadata, not an implementation detail to repair silently.

APIs may expose lossless syntactic helpers such as scheme access, authority access, display formatting, or conversion to a parsed URL where that is lossless for the specific URI. Those helpers must not classify user-facing cleanup policy and must not replace the stored canonical string as the source of truth for hashing or metadata comparison.

## Compatibility Vectors

The URI bytes in these examples are part of the compatibility contract for hashing and metadata comparison:

| Input kind                                                | Input                                    | Canonical thumbnail URI or result                        |
| --------------------------------------------------------- | ---------------------------------------- | -------------------------------------------------------- |
| local path                                                | `/home/alice/photo.png`                  | `file:///home/alice/photo.png`                           |
| local path with space                                     | `/home/alice/My Photo.png`               | `file:///home/alice/My%20Photo.png`                      |
| local path with literal percent                           | `/home/alice/100%.png`                   | `file:///home/alice/100%25.png`                          |
| local path with shell-like text                           | `/home/alice/~literal.png`               | `file:///home/alice/~literal.png`                        |
| local path with non-UTF-8 bytes                           | path bytes `/tmp/\xFF.png`               | `file:///tmp/%FF.png`                                    |
| local path with localhost authority input                 | `file://localhost/home/alice/photo.png`  | `file:///home/alice/photo.png`                           |
| non-local caller-provided URI                             | `smb://server/share/My%20Photo.png`      | preserved exactly as `smb://server/share/My%20Photo.png` |
| shared direct child                                       | `picture.png`                            | `./picture.png`                                          |
| shared direct child with space                            | `My Photo.png`                           | `./My%20Photo.png`                                       |
| shared direct child with backslash on Unix-like targets   | `name\part.png`                          | `./name%5Cpart.png`                                      |
| shared filename containing literal slash or encoded slash | `dir/picture.png` or `dir%2Fpicture.png` | rejected                                                 |
| shared parent or current segment                          | `.` or `..`                              | rejected                                                 |
