# CLI Behavior

The CLI manages thumbnails in the user's Freedesktop thumbnail cache. Its first command should be a pruning command that can run in dry-run mode before deleting files.

## Command Shape

Initial command shape:

```text
xdg-thumbnail prune [OPTIONS]
```

Candidate options:

```text
--older-than <DURATION>       Age threshold for remote, virtual, and removable entries. Defaults to 30d.
--dry-run                     Report planned deletions without deleting files.
--size <SIZE>                 Restrict scan to one size directory: normal, large, x-large, or xx-large.
--all-sizes                   Scan all size directories. This should be the default.
--include-failures            Include thumbnails/fail entries.
--removable-prefix <PATH>     Add a path prefix that should use age-based cleanup.
--no-default-removable        Disable built-in removable path prefixes.
--verbose                     Print classification and timestamp details.
```

The exact option names can change during implementation, but the CLI should preserve these capabilities.

## Default Scan Scope

By default, `prune` scans these personal cache directories:

- `$XDG_CACHE_HOME/thumbnails/normal`
- `$XDG_CACHE_HOME/thumbnails/large`
- `$XDG_CACHE_HOME/thumbnails/x-large`
- `$XDG_CACHE_HOME/thumbnails/xx-large`

If `$XDG_CACHE_HOME` is unset or blank, the fallback is `$HOME/.cache/thumbnails`.

The command should not scan shared thumbnail repositories by default.

## Dry-Run Output

Dry-run output should be machine-readable enough to test but readable enough for humans. Each candidate should include the thumbnail path, original URI if available, classification, decision, and reason.

Example:

```text
delete normal/abc123.png uri=http://example.test/a.jpg class=remote reason=older-than-threshold age=45d
keep normal/def456.png uri=file:///home/user/photo.jpg class=local-stable reason=valid
skip normal/bad.png class=unknown reason=malformed-metadata
```

## Exit Codes

- `0`: scan completed and no operational errors occurred.
- `1`: one or more deletions failed.
- `2`: command-line usage error.
- `3`: cache scan failed before producing reliable results.

Deletion candidates found during dry-run are not errors.

## Safety Requirements

- `--dry-run` must not delete or rewrite files.
- Deletion should only target files located under the resolved thumbnail cache directories.
- The CLI should never follow thumbnail path symlinks for deletion without an explicit, reviewed design.
- Missing size directories are not errors.
- Unreadable entries should be reported and skipped.

