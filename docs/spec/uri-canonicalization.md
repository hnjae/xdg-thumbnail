# URI Canonicalization

Thumbnail path calculation and `Thumb::URI` metadata must use the same canonical thumbnail URI bytes. The implementation must not hash display paths, shell-expanded paths, lossy path conversions, IRIs, or a different URI string than the one stored in standard metadata.

Initial platform support targets Unix-like XDG desktop environments. Local filesystem URI construction depends on Unix path identity and path bytes; unsupported platforms must fail explicitly instead of approximating URI or path behavior that could produce incompatible thumbnail filenames.

The implementation should use well-maintained external libraries for commodity primitives such as MD5 and percent-encoding instead of hand-written implementations. External URI or IRI parsers may be used only for syntax validation or lossless helper views; parser reserialization must not become the canonical thumbnail URI unless the relevant constructor explicitly defines that normalization. The library owns the thumbnail URI identity string used for hashing, `Thumb::URI`, and metadata comparison.

## Personal Cache URIs

Personal-cache thumbnails use absolute canonical URIs. The MD5 input for the thumbnail filename is the exact byte sequence of the canonical URI string after URI percent-encoding. The URI used for hashing is a URI identity, not a user-facing display path.

For local filesystem paths, the absolute-path constructor should accept only absolute path bytes. It should emit a `file:///` URI with empty authority, percent-encode path bytes without lossy Unicode replacement, and never expand shell syntax such as `~`. The exact emitted URI bytes are the bytes used later for hashing and metadata validation.

A separate `file:` URI constructor may accept already textual local file URI input for callers that receive URI text rather than path bytes. In that constructor, `file://localhost/...` is normalized to `file:///...` for personal-cache path calculation. Other `file:` authorities are not directly checkable local paths; cache inspection may report them, but application lookup must not validate them by probing the local filesystem unless an explicit resolver is added.

For non-local personal-cache URIs, the library should not attempt to canonicalize every possible URI scheme. Callers that own a backend such as `http:`, `smb:`, `dav:`, `trash:`, or an application-specific virtual scheme may provide an already-canonical absolute URI string. The library should preserve that URI identity exactly for hashing and metadata comparison. Validation should be limited to rejecting values that cannot be absolute URI identities, including relative references, control characters, and unescaped non-ASCII IRI text. The library must not parse and reserialize caller-provided URI strings as a hidden normalization step.

Local path URI construction percent-encodes every byte that is not safe in the canonical URI path form and emits uppercase hexadecimal escape digits. For local path bytes, the safe bytes are ASCII letters, ASCII digits, `/` as the path separator, and these path characters: `-`, `.`, `_`, `~`, `!`, `$`, `&`, `'`, `(`, `)`, `*`, `+`, `,`, `;`, `=`, `:`, and `@`. All other bytes, including space, `%`, `#`, `?`, control bytes, backslash, and non-ASCII bytes, are percent-encoded. Existing literal percent bytes in the filesystem path are encoded as `%25`; callers that already have a non-local canonical URI string should use the caller-provided URI identity API instead of passing URI text through the local path constructor.

Relative `$XDG_CACHE_HOME` values are invalid under the XDG Base Directory rules and must be ignored. If `$XDG_CACHE_HOME` is unset, blank, or relative, the personal thumbnail root falls back to `$HOME/.cache/thumbnails`. If `$HOME` cannot be determined, cache root resolution must fail rather than invent a relative fallback.

The library should avoid filesystem canonicalization as a hidden URI normalization step. Resolving symlinks or changing path identity before hashing can make thumbnails incompatible with callers that use the visible path URI; callers that need a resolved path should resolve it before constructing the thumbnail URI.

## Shared Repository URIs

Shared thumbnail repositories are scoped to the directory that contains the original. A shared thumbnail URI must be `./` followed by one canonical, minimally percent-encoded path segment for the direct child filename. Shared URI segment encoding uses the same safe bytes as local path segments except that `/` is never allowed inside the filename segment.

Shared URI construction must reject empty filenames, `.` and `..`, slash path separators, parent segments, nested paths, encoded `/`, and any input that decodes to multiple path segments. The rejected forms must not be normalized into an accepted shared URI because doing so can alias different originals. On the initial Unix-like target platforms, backslash is a filename byte rather than a path separator and must be preserved through percent-encoding when needed.

When `Thumb::URI` is present in a shared thumbnail, it must match the shared relative URI used for filename hashing. Missing `Thumb::URI` or `Thumb::MTime` does not automatically invalidate a shared thumbnail because shared repositories may use external freshness mechanisms.

## Hashing

Thumbnail filenames are the lowercase hexadecimal MD5 digest of the canonical thumbnail URI string with `.png` appended. The hash is computed over the URI string, not over the original file contents.

The same canonical thumbnail URI string must be used for filename calculation, `Thumb::URI` metadata when that metadata is present, and validation comparisons. Divergence between those strings is a cache miss or invalid metadata, not an implementation detail to repair silently.

MD5 is a Freedesktop compatibility requirement for cache filenames, not a security boundary in this project. The implementation should delegate the digest algorithm to a maintained dependency, then format the resulting digest as 32 lowercase hexadecimal characters before appending `.png`.

APIs may expose lossless syntactic helpers such as scheme access, authority access, display formatting, or conversion to a parsed URL where that is lossless for the specific URI. Those helpers must not classify user-facing cleanup policy and must not replace the stored canonical string as the source of truth for hashing or metadata comparison.

## Compatibility Vectors

The URI bytes and MD5 values in these examples are part of the compatibility contract for hashing and metadata comparison:

| Input kind                                                                           | Input                                    | Canonical thumbnail URI or result                        | MD5 filename stem                  |
| ------------------------------------------------------------------------------------ | ---------------------------------------- | -------------------------------------------------------- | ---------------------------------- |
| local path                                                                           | `/home/alice/photo.png`                  | `file:///home/alice/photo.png`                           | `82346fd12242a0f50d9cf25786189951` |
| local path with space                                                                | `/home/alice/My Photo.png`               | `file:///home/alice/My%20Photo.png`                      | `a760eeee894f58795a5fb0ce8e4235f5` |
| local path with literal percent                                                      | `/home/alice/100%.png`                   | `file:///home/alice/100%25.png`                          | `c2084e2ae9571339fc37db20ca459ba0` |
| local path with shell-like text                                                      | `/home/alice/~literal.png`               | `file:///home/alice/~literal.png`                        | `32434a84374b6e67bb9b949250390257` |
| local path with non-UTF-8 bytes                                                      | path bytes `/tmp/\xFF.png`               | `file:///tmp/%FF.png`                                    | `432dc9e7c3ec5a69b2caad256c9ba799` |
| local path with fragment and query delimiter bytes                                   | `/home/alice/has#hash?query.png`         | `file:///home/alice/has%23hash%3Fquery.png`              | `4da0ebac210a741f1f016c22eb4c94ec` |
| local path with safe path punctuation                                                | `/home/alice/a+b=c@d.png`                | `file:///home/alice/a+b=c@d.png`                         | `5a57723b293ff32a8946faaf9de5f46a` |
| local path with lowercase percent-looking text                                       | `/home/alice/%ff.png`                    | `file:///home/alice/%25ff.png`                           | `f0f601f81374d3eb5daae240f77148a3` |
| local file URI with localhost authority input                                        | `file://localhost/home/alice/photo.png`  | `file:///home/alice/photo.png`                           | `82346fd12242a0f50d9cf25786189951` |
| non-local caller-provided URI                                                        | `smb://server/share/My%20Photo.png`      | preserved exactly as `smb://server/share/My%20Photo.png` | `9225e92d750e899fbcc3b764c3085162` |
| shared direct child                                                                  | `picture.png`                            | `./picture.png`                                          | `7fd0e41c1612f860427a76c4100745a3` |
| shared direct child with space                                                       | `My Photo.png`                           | `./My%20Photo.png`                                       | `2d307968e33baf350051fbae83b1ef47` |
| shared direct child with literal percent                                             | `100%.png`                               | `./100%25.png`                                           | `47d342b8e9d11c426b2a8fc828a38c81` |
| shared direct child with backslash on Unix-like targets                              | `name\part.png`                          | `./name%5Cpart.png`                                      | `d192df08f05de51d721ae04466e0d015` |
| shared filename containing literal slash or shared URI text containing encoded slash | `dir/picture.png` or `dir%2Fpicture.png` | rejected                                                 | rejected                           |
| shared parent or current segment                                                     | `.` or `..`                              | rejected                                                 | rejected                           |
