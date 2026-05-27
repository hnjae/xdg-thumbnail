# CLI Behavior

The CLI manages thumbnails in the user's Freedesktop thumbnail cache. The initial CLI provides a pruning command that reports planned changes by default and deletes files only when explicitly requested.

## Command Shape

Initial command shape:

```text
xdg-thumbnail prune [OPTIONS]
```

Options:

```text
--older-than <DURATION>       Age threshold for remote, virtual, and removable entries. Defaults to 30d.
--delete                      Apply deletion decisions. Without this option, prune only reports planned actions.
--delete-stale-local          Delete stale local thumbnails whose originals still exist but no longer match stored metadata.
--delete-failures             Allow failure entries scanned by --scope failures or --scope all to become deletion candidates. Actual deletion still requires --delete.
--size <SIZE>                 Restrict successful thumbnail scan to one size namespace: normal, large, x-large, or xx-large. Can be passed multiple times.
--scope <SCOPE>               Restrict scan scope: thumbnails, failures, or all. Defaults to thumbnails.
--include-nonstandard-files   Include nonstandard filenames in reports.
--delete-nonstandard-files    Treat nonstandard filenames as deletion candidates. Implies report visibility for those entries; actual deletion still requires --delete.
--removable-prefix <PATH>     Add a local path prefix that should use age-based cleanup. Can be passed multiple times.
--ignore-fhs-media            Do not treat /media as removable by default.
--age-basis <BASIS>           Timestamp basis for age-based cleanup: access-time or modification-time. Defaults to access-time.
--format <FORMAT>             Output format: human or jsonl. Defaults to human.
--verbose                     Print classification and timestamp details.
```

The option names above are the initial CLI contract. Behavior or option-name changes require a spec update.

## Default Scan Scope

By default, `prune --scope thumbnails` scans these personal cache directories:

- `$XDG_CACHE_HOME/thumbnails/normal`
- `$XDG_CACHE_HOME/thumbnails/large`
- `$XDG_CACHE_HOME/thumbnails/x-large`
- `$XDG_CACHE_HOME/thumbnails/xx-large`

`x-large` and `xx-large` are supported size classes because this project targets the Freedesktop Thumbnail Managing Standard `latest` text, including the December 2020 0.9.0 history entry that adds those sizes.

If `$XDG_CACHE_HOME` is unset, blank, or relative, the fallback is `$HOME/.cache/thumbnails`. If `$HOME` cannot be determined, cache root resolution fails with an actionable diagnostic instead of guessing a relative path.

The command should not scan shared thumbnail repositories by default. Failure entries under `$XDG_CACHE_HOME/thumbnails/fail/<program-version>/` are separate failure namespaces and are scanned only with `--scope failures` or `--scope all`.

`--size` applies only to successful thumbnail size namespaces. With `--scope all`, successful thumbnail entries are restricted to the requested sizes while failure entries are still scanned. Passing `--size` with `--scope failures` is a usage error because no successful thumbnail namespace is being scanned.

When failure entries are scanned, the CLI applies the same inspection and classification policy used for successful thumbnails: classify the stored original URI, validate available metadata, and use the configured age basis for remote, virtual, and removable entries. Failure entries are application-specific retry state, so they may become deletion candidates only when `--delete-failures` is passed. Actual deletion still requires `--delete`. Passing `--delete-failures` without scanning failure entries is a usage error. Failure entries do not use successful-thumbnail size validation.

By default, deletion decisions apply only to standard thumbnail entry filenames: a 32-character lowercase hexadecimal MD5 digest followed by `.png`. Files with nonstandard names are reported as skipped when visible during scanning and are not deletion candidates. `--include-nonstandard-files` makes them visible in reports. `--delete-nonstandard-files` also makes them visible and is required before any nonstandard filename may become a deletion candidate. Even then, actual deletion still requires `--delete`, and directories and symlinks remain skipped unless a later design explicitly permits them.

For local `file:` originals, deletion for a missing original requires a reliable local check that distinguishes confirmed absence from permission errors, transient I/O errors, unsupported authorities, and unsupported path conversion. Unverifiable originals are reported and skipped rather than treated as missing.

## Report Output

The default human output should report deletion candidates, applied deletions, skipped entries that require user attention, operational errors, and a final summary. Entries kept without issues should be omitted from human output unless `--verbose` is passed. Stable machine-readable output should be available through `--format jsonl`; JSONL emits one record for each visible inspected entry so dry-run and destructive runs can be compared exactly.

Each reported entry should include the thumbnail path, original URI if available, namespace, classification, decision, whether the decision was applied, reason, and the timestamp basis for age-based decisions. Verbose human output should include kept entries and classification details.

Example:

```text
would-delete normal/abcdefabcdefabcdefabcdefabcdefab.png uri=http://example.test/a.jpg class=remote reason=remote-older-than-threshold age=45d basis=access-time
would-delete normal/0123456789abcdef0123456789abcdef.png class=unknown reason=malformed
skip normal/bad.png reason=nonstandard-filename
summary scanned=421 kept=398 would-delete=2 skipped=21 errors=0
```

When `--delete` is passed and a deletion succeeds, the human output should use `deleted` rather than `would-delete`. JSONL output should keep the cleanup decision separate from an `applied` boolean so dry-run and destructive runs are easy to compare.

When `--age-basis modification-time` is used, human and JSONL output must expose that modification time was the basis for age-based decisions.

## Exit Codes

- `0`: scan completed and no operational errors occurred.
- `1`: one or more deletions failed.
- `2`: command-line usage error.
- `3`: cache scan failed before producing reliable results.

Deletion candidates found during a non-delete report are not errors.

## Safety Requirements

- The command must not delete or rewrite files unless `--delete` is passed.
- Deletion should only target files located under the resolved thumbnail cache directories.
- The CLI should never follow thumbnail path symlinks for deletion without an explicit, reviewed design.
- The CLI should never create or request thumbnails for files located under the personal thumbnail cache or a shared `.sh_thumbnails` repository.
- Missing size directories are not errors.
- Unreadable entries should be reported and skipped.
- Nonstandard filename deletion must require both `--delete` and `--delete-nonstandard-files`.
- Failure entry deletion must require `--delete`, `--delete-failures`, and a scan scope that includes failure entries.
