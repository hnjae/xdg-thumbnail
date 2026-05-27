# Cleanup Policy

The cleanup tool removes thumbnails that are no longer useful while avoiding deletion of thumbnails whose original files may be temporarily unavailable.

## Default Policy

- Stable local `file:` URI: delete the thumbnail when a direct local filesystem check confirms that the original path no longer exists.
- Stable local `file:` URI with matching original metadata: keep the thumbnail.
- Stable local `file:` URI with changed original metadata: report it as stale and invalid for lookup; the CLI leaves it for applications to recreate by default and deletes it only when `--delete-stale-local` and `--delete` are both passed.
- Stable local `file:` URI whose original cannot be checked reliably because of permissions, transient I/O errors, unsupported authorities, or unsupported path conversion: report it as unverifiable and skip deletion.
- Remote URI such as `http:`, `https:`, `ftp:`, `sftp:`, `smb:`, or `dav:`: delete the thumbnail when it has not been accessed within the configured age threshold and `--delete` is passed.
- Archive or virtual URI such as `zip:`, `tar:`, `trash:`, `recent:`, `mtp:`, or KIO-style virtual schemes: delete the thumbnail when it has not been accessed within the configured age threshold and `--delete` is passed.
- Local file under a removable, portal, or desktop-fuse path: treat it like a remote or virtual URI and use age-based cleanup instead of a missing-file check.
- Malformed thumbnail PNG, missing required identity metadata, or metadata with invalid syntax: delete the thumbnail when `--delete` is passed. Well-formed metadata that no longer matches an existing original is stale metadata, not malformed metadata.
- Nonconforming successful-thumbnail PNGs that can still be parsed, such as entries without full alpha support, interlaced entries, or entries whose dimensions exceed the namespace limit: report them as invalid for application lookup, but do not delete them by default solely for format nonconformance.
- Failure entries: skip by default because they are application-specific retry state; when the user scans them with `--scope failures` or `--scope all`, apply the same URI classification, metadata checks, and age evaluation as successful thumbnails, except successful-thumbnail dimension checks do not apply. Failure entries may become deletion candidates only when `--delete-failures` is passed, and actual deletion still requires `--delete`.
- Nonstandard filenames in thumbnail cache directories: skip by default; include them in reports when `--include-nonstandard-files` is passed; do not delete them in the initial CLI. A future nonstandard-file cleanup feature must define a narrower, reviewed deletion target before it is exposed.

The default age threshold is 30 days.

Actual deletion requires `--delete`; without it, the CLI reports the same decisions without mutating the cache.

Standard thumbnail entry filenames are 32 lowercase hexadecimal MD5 characters followed by `.png`. The cleanup tool should not infer that other files under the cache are safe to delete. Nonstandard files, directories, and symlinks remain skipped unless a later reviewed design explicitly permits a narrower deletion policy.

## Removable And Desktop-Fuse Heuristics

The CLI should classify local `file:` paths under these prefixes as removable or desktop-managed by default:

- `/media`
- `/run/media/$UID`
- `/run/user/$UID/doc`
- `/run/user/$UID/gvfs`
- `/run/user/$UID/kio-fuse`

Users should be able to disable the `/media` default with `--ignore-fhs-media`. `/mnt` is not classified as removable by default because FHS defines its contents as a local administrative matter. Users who use `/mnt` for temporary or removable mounts can opt in with repeated `--removable-prefix` options.

Users should be able to add additional prefixes through repeated CLI options. The initial CLI has no persistent configuration file; if configuration is added later, its location, precedence, and merge behavior must be documented in `docs/spec/` before the feature is exposed.

## Access Time

Age-based cleanup needs a timestamp for "not accessed within the configured period." Initial behavior uses the thumbnail file access time when filesystem metadata exposes it. The CLI must capture the thumbnail file metadata before reading PNG metadata, because opening the thumbnail may update access time. If the platform supports an access-time-preserving open such as `O_NOATIME`, using it is preferred but not required.

The CLI should not try to prove complex access-time semantics for every filesystem. Initial timestamp bases are `access-time`, `modification-time`, and `unavailable`. Age-based deletion may run for `access-time` by default. If access time is unavailable, the CLI reports the candidate as skipped rather than falling back silently to modification time. If the CLI identifies a mount or filesystem as not maintaining useful access times, it may report the timestamp as unreliable and skip age-based deletion for affected entries; otherwise it reports the captured access time as the decision basis.

Users who prefer aggressive cleanup should be able to explicitly choose thumbnail file modification time as the age basis. When modification time is used for an age-based decision, verbose and report output must state that the decision is based on thumbnail file modification time rather than access time because many Linux systems use `relatime` or `noatime`.

## Deletion Reasons

The CLI should return structured, stable reason identifiers for deletion candidates. Initial reason identifiers are `original-missing`, `stale-local-metadata`, `remote-older-than-threshold`, `virtual-older-than-threshold`, `removable-older-than-threshold`, and `malformed`. The `malformed` reason covers unreadable PNG structure, missing required identity metadata, and metadata values that cannot be parsed; it does not cover metadata that is well-formed but stale for an existing original. Report-only reasons should distinguish at least `nonconforming-format` and `nonconforming-dimensions`. Skip reasons should distinguish at least `original-unverifiable`, `nonstandard-filename`, `failure-deletion-not-enabled`, `timestamp-unreliable`, `timestamp-unavailable`, `unreadable-entry`, and `out-of-scope`.

Reports should derive deletion and skip reasons from policy-neutral inspection facts such as metadata validity, original availability, unsupported URI for local validation, thumbnail timestamps, cache namespace, and cache location. The CLI should show deletion and skip reasons in report and verbose output.

## Non-Goals

- The cleanup tool should not contact remote servers to check whether remote originals still exist.
- The cleanup tool should not mount removable media to check whether originals still exist.
- The cleanup tool should not rewrite correct thumbnails just to normalize metadata.
- The cleanup tool should not delete shared thumbnail repositories unless an explicit shared-repository option is added later.
- The cleanup tool should not create, rewrite, or delete thumbnails whose original URI points inside the personal thumbnail cache or a shared `.sh_thumbnails` repository.

## Resolved Defaults

- Stale local thumbnails with existing originals are reported by default and deleted only with explicit stale-local deletion enabled.
- The CLI does not persist its own last-seen timestamp database.
- Access time is the default basis for age-based cleanup. Modification time may be used only when the user explicitly selects it, and reports must expose that basis.
