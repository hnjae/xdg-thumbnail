# Prune CLI Behavior

The `xdg-thumbnail-prune` CLI manages stale or invalid entries in the user's Freedesktop thumbnail cache. It reports planned changes by default and deletes files only when explicitly requested.

## Command Shape

Initial command shape:

```text
xdg-thumbnail-prune [OPTIONS]
```

Options:

```text
--older-than <DURATION>       Age threshold for remote, virtual, and removable entries under the selected age basis. Defaults to 30d. See Duration Syntax.
--delete                      Apply deletion decisions. Without this option, prune only reports planned actions.
--delete-stale-local          Include stale local thumbnails whose originals still exist but no longer match stored metadata as deletion candidates. Actual deletion still requires --delete.
--allow-delete-failures       Allow failure entries scanned by --scope failures or --scope all to become deletion candidates. Actual deletion still requires --delete.
--size <SIZE>                 Restrict successful thumbnail scan to one size namespace: normal, large, x-large, or xx-large. Can be passed multiple times.
--scope <SCOPE>               Restrict scan scope: thumbnails, failures, or all. Defaults to thumbnails.
--include-nonstandard-files   Include nonstandard filenames in reports as skipped entries.
--removable-prefix <PATH>     Add a local path prefix that should use age-based cleanup. Can be passed multiple times.
--ignore-fhs-media            Do not treat /media as removable by default.
--age-basis <BASIS>           Timestamp basis for age-based cleanup: access-time or modification-time. Defaults to access-time. modification-time is a more portable and more aggressive explicit mode.
--format <FORMAT>             Output format: human or jsonl. Defaults to human.
--verbose                     Print classification and timestamp details.
```

The option names above are the initial prune CLI contract. Behavior or option-name changes require a spec update.

## Duration Syntax

`<DURATION>` values are positive base-10 integers followed by a unit suffix with no whitespace. Supported units are `s` for seconds, `m` for minutes, `h` for hours, and `d` for 24-hour days. `0`, negative values, fractional values, missing units, unknown units, and values too large to represent safely are usage errors.

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

When failure entries are scanned, the prune CLI applies the same inspection and classification policy used for successful thumbnails: classify the stored original URI, validate available metadata, and use the configured age basis for remote, virtual, and removable entries. Failure entries are application-specific retry state, so they may become deletion candidates only when `--allow-delete-failures` is passed. Actual deletion still requires `--delete`. Passing `--allow-delete-failures` without `--scope failures` or `--scope all` is a usage error, and the diagnostic must tell the user to add one of those scan scopes. Failure entries do not use successful-thumbnail size validation.

Failure entry scanning is limited to one namespace level below `$XDG_CACHE_HOME/thumbnails/fail/`. Each immediate real directory is treated as one program-version namespace, and only files directly contained in that namespace directory are inspected as failure entries. The prune CLI must not follow symlinked failure namespace directories, must not recurse into nested directories, and must report visible skipped entries when reporting is requested. A missing `fail` directory is not an error.

By default, deletion decisions and reports include only standard thumbnail entry filenames: a 32-character lowercase hexadecimal MD5 digest followed by `.png`. Files with nonstandard names are not visible in default reports and are not deletion candidates. `--include-nonstandard-files` makes them visible as skipped entries in reports. Directories and symlinks remain skipped unless a later design explicitly permits them.

For `file:` originals classified as stable local files, deletion for a missing original requires a reliable local check that distinguishes confirmed absence from permission errors, transient I/O errors, unsupported authorities, and unsupported path conversion. Unverifiable originals are reported and skipped rather than treated as missing.

## Age Basis

Age-based cleanup defaults to thumbnail file access time, matching the Freedesktop deletion guidance for internet-related and removable-media thumbnails in terms of whether the thumbnail has been accessed recently. Because the prune command may need to read thumbnail contents to classify entries, access-time cleanup is conservative: timestamp metadata must be captured before content reads, and the command must avoid reading thumbnail contents in a way that changes later age decisions.

Entries that cannot be inspected without potentially changing access time are reported as skipped rather than treated as age-based deletion candidates. These skips are normal conservative access-time policy outcomes and do not by themselves make the command fail. Users who want more portable and more aggressive age-based cleanup may pass `--age-basis modification-time`; that mode uses thumbnail file modification time and can delete a thumbnail that was recently read but not recently rewritten.

## Report Output

The default human output should report deletion candidates, applied deletions, skipped entries that require user attention, operational errors, and a final summary. Entries kept without issues should be omitted from human output unless `--verbose` is passed. Initial machine-readable output should be available through `--format jsonl`. JSONL emits one record for each visible inspected entry so dry-run and destructive runs can be compared exactly, plus summary records when needed. Because the project is pre-release, additive JSONL fields may be added without a compatibility promise, but removing or renaming the v0 fields below requires a spec update.

Each JSONL entry record must include at least `schema_version: 0`, `event: "entry"`, `thumbnail_path_display`, `thumbnail_path_bytes_b64`, `uri`, `namespace`, `classification`, `decision`, `applied`, `reason`, `age_basis`, `timestamp`, `access_time_preservation`, and `error`. The `thumbnail_path_display` field is a human-oriented UTF-8 string suitable for logs and may use escaping or replacement for non-UTF-8 path bytes. The `thumbnail_path_bytes_b64` field uses unpadded RFC 4648 standard base64 over the exact Unix path bytes and is the lossless machine-readable representation; it is `null` only when the path could not be computed. Nullable fields are represented as `null` rather than omitted when the value could not be computed. `error` is either `null` or an object with stable `kind` and human-oriented `message` fields. Summary records use `event: "summary"` with counters for scanned, kept, would_delete, deleted, skipped, errors, and the selected age basis.

Each reported human entry should include the thumbnail path, original URI if available, namespace, classification, decision, whether the decision was applied, reason, and the timestamp basis for age-based decisions. When access time is the selected age basis, reports should also expose whether access time was preserved during metadata inspection or why age evaluation was skipped. Verbose human output should include kept entries and classification details.

When `--age-basis access-time` is active and one or more age-based candidates are skipped because access time is unavailable, unreliable, or cannot be preserved during inspection, the human summary must include a short hint that `--age-basis modification-time` is more portable and more aggressive, including an example such as `xdg-thumbnail-prune --older-than 30d --age-basis modification-time`. JSONL summaries expose this through the timestamp skip counters and selected age basis rather than through prose hints.

Example with `--include-nonstandard-files` enabled:

```text
would-delete normal/abcdefabcdefabcdefabcdefabcdefab.png uri=http://example.test/a.jpg class=remote reason=remote-older-than-threshold age=45d basis=access-time
would-delete normal/0123456789abcdef0123456789abcdef.png class=unknown reason=missing-required-metadata
skip normal/bad.png reason=nonstandard-filename
summary scanned=421 kept=398 would-delete=2 skipped=21 errors=0 basis=access-time timestamp-unavailable=0 timestamp-unreliable=0 timestamp-preservation-unavailable=0
```

When `--delete` is passed and a deletion succeeds, the human output should use `deleted` rather than `would-delete`. JSONL output should keep the cleanup decision separate from an `applied` boolean so dry-run and destructive runs are easy to compare.

When `--age-basis modification-time` is used, human and JSONL output must expose that modification time was the basis for age-based decisions.

## Exit Codes

- `0`: scan completed and no operational errors occurred.
- `1`: one or more deletions failed.
- `2`: command-line usage error.
- `3`: cache scan failed before producing reliable results.
- `4`: scan completed but one or more nonfatal inspection errors occurred, such as unreadable entries or per-entry I/O errors that did not invalidate the whole scan. Entries skipped only because access time is unavailable, unreliable, or cannot be preserved are normal access-time policy outcomes and do not by themselves contribute to exit code `4`.

Deletion candidates found during a non-delete report are not errors.

## Safety Requirements

- The command must not delete or rewrite files unless `--delete` is passed.
- When `--age-basis access-time` is active, the command should not read thumbnail contents in a way that can update thumbnail access times. Entries that cannot be inspected without potentially changing access time are reported as skipped rather than treated as age-based deletion candidates.
- Deletion should only target files located under the resolved thumbnail cache directories.
- The prune CLI should never follow thumbnail path symlinks for deletion without an explicit, reviewed design.
- The prune CLI should never follow symlinked failure namespace directories.
- The prune CLI should never create, update, regenerate, or request thumbnails.
- The prune CLI should never create or request thumbnails for files located under the personal thumbnail cache or a shared `.sh_thumbnails` repository.
- The prune CLI should never create or update shared thumbnail repositories.
- Missing size directories are not errors.
- Unreadable entries should be reported and skipped.
- Nonstandard filename deletion is not part of the initial prune CLI contract.
- Failure entry deletion must require `--delete`, `--allow-delete-failures`, and a scan scope that includes failure entries.
