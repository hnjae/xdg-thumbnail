# Cleanup Policy

The cleanup tool removes thumbnails that are no longer useful while avoiding deletion of thumbnails whose original files may be temporarily unavailable.

## Default Policy

- Stable local `file:` URI: delete the thumbnail when the original path no longer exists.
- Stable local `file:` URI with matching original metadata: keep the thumbnail.
- Stable local `file:` URI with changed original metadata: report it as stale; the CLI may leave it for applications to recreate instead of deleting it by default.
- Remote URI such as `http:`, `https:`, `ftp:`, `sftp:`, `smb:`, or `dav:`: delete the thumbnail when it has not been accessed within the configured age threshold.
- Archive or virtual URI such as `zip:`, `tar:`, `trash:`, `recent:`, `mtp:`, or KIO-style virtual schemes: delete the thumbnail when it has not been accessed within the configured age threshold.
- Local file under a removable, portal, or desktop-fuse path: treat it like a remote or virtual URI and use age-based cleanup instead of a missing-file check.
- Malformed thumbnail PNG or thumbnail metadata: skip by default and report it; a future `--delete-malformed` option may delete these entries explicitly.

The default age threshold is 30 days.

## Removable And Desktop-Fuse Heuristics

The CLI should classify local `file:` paths under these prefixes as removable or desktop-managed by default:

- `/media`
- `/mnt`
- `/run/media`
- `/run/user/$UID/gvfs`
- `/run/user/$UID/kio-fuse`

Users should be able to add or replace these prefixes through CLI options or configuration. The library should not hard-code this policy as universal truth; it should expose the classification hook used by the CLI.

## Access Time

Age-based cleanup needs a timestamp for "not accessed within the configured period." Initial behavior should use the thumbnail file access time when the filesystem exposes it. If access time is unavailable or clearly unsupported, the CLI should fall back to the thumbnail file modification time and report the fallback in verbose output.

This fallback is intentionally conservative and implementation-defined because many Linux systems use `relatime` or `noatime`.

## Deletion Reasons

The library should return structured reasons for deletion candidates.

```rust
pub enum DeleteReason {
    OriginalMissing,
    RemoteOlderThanThreshold,
    VirtualOlderThanThreshold,
    RemovableOlderThanThreshold,
}
```

The CLI should show these reasons in dry-run and verbose output.

## Non-Goals

- The cleanup tool should not contact remote servers to check whether remote originals still exist.
- The cleanup tool should not mount removable media to check whether originals still exist.
- The cleanup tool should not rewrite correct thumbnails just to normalize metadata.
- The cleanup tool should not delete shared thumbnail repositories unless an explicit shared-repository option is added later.

## Open Decisions

- Whether stale local thumbnails with existing originals should be deleted by default or only reported. Initial policy: report only.
- Whether malformed PNGs should have an explicit cleanup option in the first release. Initial policy: skip and report.
- Whether the CLI should persist its own last-seen timestamp database instead of relying on filesystem access time. Initial policy: no separate database.
