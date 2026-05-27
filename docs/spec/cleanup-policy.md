# Cleanup Policy

The cleanup tool removes thumbnails that are no longer useful while avoiding deletion of thumbnails whose original files may be temporarily unavailable.

## Default Policy

- Stable local `file:` URI: delete the thumbnail when the original path no longer exists.
- Stable local `file:` URI with matching original metadata: keep the thumbnail.
- Stable local `file:` URI with changed original metadata: report it as stale and invalid for lookup; the CLI leaves it for applications to recreate by default and deletes it only when `--delete-stale-local` and `--delete` are both passed.
- Remote URI such as `http:`, `https:`, `ftp:`, `sftp:`, `smb:`, or `dav:`: delete the thumbnail when it has not been accessed within the configured age threshold and `--delete` is passed.
- Archive or virtual URI such as `zip:`, `tar:`, `trash:`, `recent:`, `mtp:`, or KIO-style virtual schemes: delete the thumbnail when it has not been accessed within the configured age threshold and `--delete` is passed.
- Local file under a removable, portal, or desktop-fuse path: treat it like a remote or virtual URI and use age-based cleanup instead of a missing-file check.
- Malformed thumbnail PNG or thumbnail metadata: delete the thumbnail when `--delete` is passed.
- Nonstandard filenames in thumbnail cache directories: skip by default; include them in reports only when `--include-nonstandard-files` is passed; allow deletion decisions only when `--delete-nonstandard-files` and `--delete` are both passed.

The default age threshold is 30 days.

Actual deletion requires `--delete`; without it, the CLI reports the same decisions without mutating the cache.

Standard thumbnail entry filenames are 32 lowercase hexadecimal MD5 characters followed by `.png`. The cleanup tool should not infer that other files under the cache are safe to delete unless the user explicitly opts into nonstandard-file deletion. Nonstandard directories and symlinks remain skipped unless a later reviewed design explicitly permits them.

## Removable And Desktop-Fuse Heuristics

The CLI should classify local `file:` paths under these prefixes as removable or desktop-managed by default:

- `/media`
- `/run/media/$UID`
- `/run/user/$UID/gvfs`
- `/run/user/$UID/kio-fuse`

Users should be able to disable the `/media` default with `--ignore-fhs-media`. `/mnt` is not classified as removable by default because FHS defines its contents as a local administrative matter. Users who use `/mnt` for temporary or removable mounts can opt in with repeated `--removable-prefix` options.

Users should be able to add additional prefixes through CLI options or configuration. The library should not hard-code this policy as universal truth; removable and desktop-fuse heuristics belong to the CLI policy layer.

## Access Time

Age-based cleanup needs a timestamp for "not accessed within the configured period." Initial behavior should use the thumbnail file access time when the filesystem exposes it. The CLI should capture the thumbnail file access time before reading PNG metadata, because opening the thumbnail may update access time. If the platform supports an access-time-preserving open such as `O_NOATIME`, using it is preferred but not required.

The CLI should classify the timestamp basis per entry instead of trying to globally prove filesystem access-time semantics. Initial timestamp bases are `access-time`, `access-time-untrusted`, `modification-time`, and `unavailable`. Age-based deletion may run only for `access-time` by default. If access time is unavailable or untrusted, the CLI should report the candidate as skipped rather than delete it because modification time is not evidence that the thumbnail was recently accessed.

An explicit future option may allow modification-time fallback for users who prefer aggressive cleanup. If such a fallback is added, verbose and report output must state that the decision is based on thumbnail file modification time rather than access time because many Linux systems use `relatime` or `noatime`.

## Deletion Reasons

The CLI should return structured reasons for deletion candidates. The library should return policy-neutral inspection facts such as metadata validity, original availability, unsupported URI for local validation, thumbnail timestamps, cache namespace, and cache location.

```rust
pub enum DeleteReason {
    OriginalMissing,
    StaleLocalMetadata,
    RemoteOlderThanThreshold,
    VirtualOlderThanThreshold,
    RemovableOlderThanThreshold,
    Malformed,
    NonstandardFile,
}
```

The CLI should show these reasons in report and verbose output.

## Non-Goals

- The cleanup tool should not contact remote servers to check whether remote originals still exist.
- The cleanup tool should not mount removable media to check whether originals still exist.
- The cleanup tool should not rewrite correct thumbnails just to normalize metadata.
- The cleanup tool should not delete shared thumbnail repositories unless an explicit shared-repository option is added later.
- The cleanup tool should not create, rewrite, or delete thumbnails for files located inside thumbnail cache directories.

## Open Decisions

- Whether stale local thumbnails with existing originals should be deleted by default or only reported. Initial policy: report only, with an explicit `--delete-stale-local` option reserved for users who want destructive stale-local cleanup.
- Whether the CLI should persist its own last-seen timestamp database instead of relying on filesystem access time. Initial policy: no separate database.
- Whether an explicit modification-time fallback option should be added for age-based cleanup when access time is unavailable. Initial policy: do not delete those candidates by default.
