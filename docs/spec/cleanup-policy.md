# Cleanup Policy

The `xdg-thumbnail-prune` cleanup tool removes thumbnails that are no longer useful while avoiding deletion of thumbnails whose original files may be temporarily unavailable.

## Default Policy

- Stable local `file:` URI: delete the thumbnail when a direct local filesystem check confirms that the original path no longer exists.
- Stable local `file:` URI with matching original metadata: keep the thumbnail.
- Stable local `file:` URI with changed original metadata: report it as stale and invalid for lookup by default; when `--delete-stale-local` is passed, report it as a deletion candidate, and delete it only when `--delete` is also passed.
- Stable local `file:` URI whose original cannot be checked reliably because of permissions, transient I/O errors, unsupported authorities, or unsupported path conversion: report it as unverifiable and skip deletion.
- Remote URI such as `http:`, `https:`, `ftp:`, `sftp:`, `smb:`, or `dav:`: delete the thumbnail when it is older than the configured threshold under the selected age basis and `--delete` is passed.
- Archive or virtual URI such as `zip:`, `tar:`, `trash:`, `recent:`, `mtp:`, or KIO-style virtual schemes: delete the thumbnail when it is older than the configured threshold under the selected age basis and `--delete` is passed.
- Local file under a removable, portal, or desktop-fuse path: treat it like a remote or virtual URI and use age-based cleanup instead of a missing-file check.
- Personal-cache entries with unreadable PNG structure, metadata required for their personal-cache context missing, metadata with invalid syntax, or a standard filename that does not match the MD5 of the stored canonical `Thumb::URI`: delete the thumbnail when `--delete` is passed. Well-formed metadata that no longer matches an existing original is stale metadata, not invalid metadata syntax.
- Nonconforming successful-thumbnail PNGs that can still be parsed, such as entries without full alpha support, interlaced entries, or entries whose dimensions exceed the namespace limit: report them as invalid for application lookup, but do not delete them by default solely for format nonconformance.
- Failure entries: skip by default because they are application-specific retry state; when the user scans them with `--scope failures` or `--scope all`, apply the same URI classification, metadata checks, and age evaluation as successful thumbnails, except successful-thumbnail dimension checks do not apply. Failure entries may become deletion candidates only when `--allow-delete-failures` is passed, and actual deletion still requires `--delete`.
- Nonstandard filenames in thumbnail cache directories: skip by default; include them in reports when `--include-nonstandard-files` is passed; do not delete them in the initial prune CLI. A future nonstandard-file cleanup feature must define a narrower, reviewed deletion target before it is exposed.

The default age threshold is 30 days.

Actual deletion requires `--delete`; without it, the prune CLI reports the same decisions without mutating the cache.

The initial cleanup policy applies to the user's personal thumbnail cache. It does not define deletion behavior for shared thumbnail repositories. If shared-repository cleanup is added later, missing `Thumb::URI` or `Thumb::MTime` must be evaluated under a shared-specific policy because the Freedesktop shared-repository rules allow those keys to be absent when another freshness mechanism is used.

Standard thumbnail entry filenames are 32 lowercase hexadecimal MD5 characters followed by `.png`. The cleanup tool should not infer that other files under the cache are safe to delete. Nonstandard files, directories, and symlinks remain skipped unless a later reviewed design explicitly permits a narrower deletion policy.

## Removable And Desktop-Fuse Heuristics

The prune CLI should classify local `file:` paths under these prefixes as removable or desktop-managed by default:

- `/media`
- `/run/media/$UID`
- `/run/user/$UID/doc`
- `/run/user/$UID/gvfs`
- `/run/user/$UID/kio-fuse`

Users should be able to disable the `/media` default with `--ignore-fhs-media`. `/mnt` is not classified as removable by default because FHS defines its contents as a local administrative matter. Users who use `/mnt` for temporary or removable mounts can opt in with repeated `--removable-prefix` options.

Users should be able to add additional prefixes through repeated CLI options. The initial prune CLI has no persistent configuration file; if configuration is added later, its location, precedence, and merge behavior must be documented in `docs/spec/` before the feature is exposed.

## Age Basis

Age-based cleanup needs a timestamp for comparing each thumbnail against the configured threshold. Initial behavior defaults to thumbnail file access time, matching the Freedesktop deletion guidance for internet-related and removable-media thumbnails in terms of whether the thumbnail has been accessed recently.

Access-time cleanup is conservative because classifying entries may require inspecting thumbnail contents. Age decisions in access-time mode must be based on timestamp metadata captured before inspection, and a dry-run must not change later cleanup decisions by updating access times. Age-based deletion in access-time mode may run only for entries whose timestamp and required inspection can be completed without changing later age decisions. If access time is unavailable, cannot be preserved during inspection, or is identified as unreliable for the filesystem, the prune CLI reports the candidate as skipped rather than falling back silently to modification time. These skips are expected outcomes of the conservative access-time policy and do not by themselves make the command fail.

Users who want more portable and more aggressive age-based cleanup may explicitly choose `--age-basis mtime`. Modification-time cleanup uses thumbnail file modification time and can delete a thumbnail that was recently read but not recently rewritten. Human and JSONL summaries for age-based runs must expose the selected timestamp basis and counts for timestamp-unavailable, timestamp-unreliable, and timestamp-preservation-unavailable skips. Human summaries must also point users to the explicit `--age-basis mtime` option when access-time policy skips age-based candidates, include a concrete example such as `xdg-thumbnail-prune --older-than 30d --age-basis mtime`, and make clear that modification time is more portable and more aggressive.

The prune CLI should not try to prove complex access-time semantics for every filesystem. Initial timestamp bases are `atime`, `mtime`, and `unavailable`. CLI help and report output must make clear that access time is the default and that mtime cleanup is more aggressive than atime cleanup. When modification time is used for an age-based decision, verbose and report output must state that the decision is based on thumbnail file modification time rather than access time.

## Deletion Reasons

The prune CLI should return structured, stable reason identifiers for deletion candidates. Initial reason identifiers are `original-missing`, `stale-local-metadata`, `remote-older-than-threshold`, `virtual-older-than-threshold`, `removable-older-than-threshold`, `invalid-png-structure`, `missing-required-metadata`, `invalid-metadata-syntax`, and `uri-filename-mismatch`. `missing-required-metadata` applies to metadata required for the personal-cache scope being scanned. `uri-filename-mismatch` applies when a standard cache filename's MD5 stem does not match the stored canonical `Thumb::URI` for the same entry. Future shared-repository cleanup must define separate shared-specific reasons or acceptance policy. Well-formed metadata that is stale for an existing original must use `stale-local-metadata`, not a metadata-syntax reason. Report-only reasons should distinguish at least `nonconforming-format` and `nonconforming-dimensions`. Skip reasons should distinguish at least `original-unverifiable`, `nonstandard-filename`, `failure-deletion-not-enabled`, `timestamp-unreliable`, `timestamp-unavailable`, `timestamp-preservation-unavailable`, `resource-limit-exceeded`, `unreadable-entry`, and `out-of-scope`.

Reports should show deletion and skip reasons in report and verbose output. A reported reason must be stable for the same inspected entry, selected options, original availability, thumbnail metadata, timestamp information, cache namespace, and cache location.

## Non-Goals

- The cleanup tool should not contact remote servers to check whether remote originals still exist.
- The cleanup tool should not mount removable media to check whether originals still exist.
- The cleanup tool should not rewrite correct thumbnails just to normalize metadata.
- The cleanup tool should not create, update, regenerate, or save personal-cache thumbnails or failure entries.
- The cleanup tool should not create or update shared thumbnail repositories.
- The cleanup tool should not delete shared thumbnail repositories unless an explicit shared-repository option is added later.
- The cleanup tool should not create or request thumbnails whose original URI points inside the personal thumbnail cache or a shared `.sh_thumbnails` repository. This restriction protects against recursive thumbnail generation; it does not prevent deleting an inspected cache entry that is itself inside the resolved cache root and otherwise matches the cleanup policy.

## Resolved Defaults

- Stale local thumbnails with existing originals are reported by default and deleted only with explicit stale-local deletion enabled.
- The prune CLI does not persist its own last-seen timestamp database.
- Access time is the default basis for age-based cleanup. Modification time may be used only when the user explicitly selects it, and reports must expose the selected basis. Access-time skip summaries should tell users that modification-time cleanup is the portable, more aggressive fallback available through `--age-basis mtime`.
