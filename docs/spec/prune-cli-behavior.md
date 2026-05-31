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
--allow-stale-local-deletion  Allow stale local thumbnails to become deletion candidates. Actual deletion still requires --delete.
--allow-failure-deletion      Allow scanned failure entries to become deletion candidates. Requires --scope failures or --scope all. Actual deletion still requires --delete.
--size <SIZE>                 Restrict successful thumbnail scan to one size namespace: normal, large, x-large, or xx-large. Defaults to all successful thumbnail size namespaces. Can be passed multiple times; duplicate values are ignored after their first occurrence.
--scope <SCOPE>               Restrict scan scope: thumbnails, failures, or all. Defaults to thumbnails.
--include-nonstandard-files   Include nonstandard filenames in reports as skipped entries.
--removable-prefix <PATH>     Add an absolute local path prefix that should use age-based cleanup. Can be passed multiple times.
--ignore-media-prefix         Do not treat /media as removable by default.
--age-basis <BASIS>           Timestamp basis for age-based cleanup: atime or mtime. Defaults to atime. mtime is a more portable and more aggressive explicit mode.
--format <FORMAT>             Output format: human or jsonl. Defaults to human.
--verbose                     Include kept entries and timestamp details in human output.
--generate-completion <SHELL> Generate a shell completion script to stdout and exit. Supported shells are defined by clap_complete.
--generate-manpage            Generate a man page to stdout and exit.
```

The option names above are the initial prune CLI contract. Behavior or option-name changes require a spec update.

`--generate-completion` and `--generate-manpage` are public metadata-generation modes derived from the clap command definition for packagers and users. They do not scan cache state, do not read or delete thumbnails, bypass scan and deletion option validation, and exit successfully after writing the requested artifact to stdout. The generated man page and installed shell completions must describe the same command shape as the executable. Nix package outputs must include generated bash, fish, and zsh completions plus section 1 man pages for installed CLI binaries.

`--help` and `--version` are metadata-only modes provided by the command parser. They do not scan cache state, bypass deletion-option validation, and must exit successfully with code `0`. CLI help, generated man pages, and generated completions must include option descriptions matching the public behavior documented here.

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

The command should not scan shared thumbnail repositories by default, and the initial prune CLI does not define shared-repository deletion behavior. Failure entries under `$XDG_CACHE_HOME/thumbnails/fail/<program-version>/` are separate failure namespaces and are scanned only with `--scope failures` or `--scope all`.

`--size` applies only to successful thumbnail size namespaces. When omitted, all successful thumbnail size namespaces are scanned. With `--scope all`, successful thumbnail entries are restricted to the requested sizes while failure entries are still scanned. Passing `--size` with `--scope failures` is a usage error because no successful thumbnail namespace is being scanned.

`--removable-prefix` accepts absolute local path prefixes only. Relative, blank, or otherwise non-absolute prefix values are usage errors with exit code `2`. The prune CLI uses accepted prefixes as lexical path prefixes for classification and must not canonicalize them, require them to exist, resolve symlinks, or rewrite user input.

When failure entries are scanned, the prune CLI applies the same inspection and classification policy used for successful thumbnails: classify the stored original URI, validate available metadata, and use the configured age basis for remote, virtual, and removable entries. Failure entries are application-specific retry state, so they may become deletion candidates only when `--allow-failure-deletion` is passed. Actual deletion still requires `--delete`. Passing `--allow-failure-deletion` without `--scope failures` or `--scope all` is a usage error, and the diagnostic must tell the user to add one of those scan scopes. Failure entries do not use successful-thumbnail size validation.

Failure entry scanning is limited to one namespace level below `$XDG_CACHE_HOME/thumbnails/fail/`. Each immediate real directory is treated as one program-version namespace, and only files directly contained in that namespace directory are inspected as failure entries. The prune CLI must not follow symlinked failure namespace directories, must not recurse into nested directories, and must report visible skipped entries when reporting is requested. A missing `fail` directory is not an error.

By default, deletion decisions and reports include only standard thumbnail entry filenames: a 32-character lowercase hexadecimal MD5 digest followed by `.png`. Files with nonstandard names are not visible in default reports and are not deletion candidates. `--include-nonstandard-files` makes them visible as skipped entries in reports. Directories and symlinks remain skipped unless a later design explicitly permits them.

For `file:` originals classified as stable local files, deletion for a missing original requires a reliable local check that distinguishes confirmed absence from permission errors, transient I/O errors, unsupported authorities, and unsupported path conversion. Unverifiable originals are reported and skipped rather than treated as missing. Standard cache entries whose filename MD5 stem does not match the stored canonical `Thumb::URI` are reported with reason `uri-filename-mismatch` and may become deletion candidates because the entry cannot be a valid cache hit for the stored original identity.

## Age Basis

Age-based cleanup defaults to thumbnail file access time, matching the Freedesktop deletion guidance for internet-related and removable-media thumbnails in terms of whether the thumbnail has been accessed recently. Because the prune command may need to read thumbnail contents to classify entries, access-time cleanup is conservative: timestamp metadata must be captured before content reads, and the command must avoid reading thumbnail contents in a way that changes later age decisions.

Entries that cannot be inspected without potentially changing access time are reported as skipped rather than treated as age-based deletion candidates. These skips are normal conservative access-time policy outcomes and do not by themselves make the command fail. Users who want more portable and more aggressive age-based cleanup may pass `--age-basis mtime`; that mode uses thumbnail file modification time and can delete a thumbnail that was recently read but not recently rewritten.

## Report Output

The default human output should report deletion candidates, applied deletions, skipped entries that require user attention, operational errors, and a final summary. Entries kept without issues should be omitted from human output unless `--verbose` is passed. Initial machine-readable output should be available through `--format jsonl`. JSONL emits one record for each visible inspected entry so dry-run and destructive runs can be compared exactly, plus summary records when needed. Because the project is pre-release, additive JSONL fields may be added without a compatibility promise, but removing or renaming the v0 fields below requires a spec update.

Each JSONL entry record must include at least `schema_version: 0`, `event: "entry"`, `thumbnail_path_display`, `thumbnail_path_bytes_b64`, `uri`, `namespace`, `classification`, `decision`, `applied`, `reason`, `age_basis`, `timestamp`, `access_time_preservation`, and `error`. The `thumbnail_path_display` field is a human-oriented UTF-8 string suitable for logs and may use escaping or replacement for non-UTF-8 path bytes. The `thumbnail_path_bytes_b64` field uses unpadded RFC 4648 standard base64 over the exact Unix path bytes and is the lossless machine-readable representation; it is `null` only when the path could not be computed. Nullable fields are represented as `null` rather than omitted when the value could not be computed. `error` is either `null` or an object with stable `kind` and human-oriented `message` fields. Summary records use `event: "summary"` with counters for scanned, kept, would_delete, deleted, skipped, errors, and the selected age basis.

Initial JSONL `decision` values are `keep`, `delete`, `stale`, and `skip`. `keep` means the entry is left unchanged because it is currently useful or outside the selected deletion policy. `delete` means the entry is a deletion candidate; `applied` distinguishes report-only candidates from deletions actually performed with `--delete`. `stale` means a stable local original still exists but the thumbnail metadata no longer matches it, so the entry is invalid for lookup and should be recreated by applications; the prune CLI reports this state without deleting the file unless `--allow-stale-local-deletion` is passed. With `--allow-stale-local-deletion`, the decision is `delete` with reason `stale-local-metadata`; the file remains in place with `applied: false` unless `--delete` is also passed, and is deleted with `applied: true` only when both options are present. `skip` means the entry is not acted on because it is unverifiable, out of scope, nonstandard, unsafe to delete under the selected options, or failed inspection in a way that is not a deletion candidate.

Each reported human entry should include the thumbnail path, original URI if available, namespace, classification, decision, whether the decision was applied, reason, and the timestamp basis for age-based decisions. When access time is the selected age basis, reports should also expose whether access time was preserved during metadata inspection or why age evaluation was skipped. Verbose human output should include kept entries and add per-entry `timestamp=<unix-seconds-or-none>` and `access-time-preservation=<value>` details.

When `--age-basis atime` is active and one or more age-based candidates are skipped because access time is unavailable, unreliable, or cannot be preserved during inspection, the human summary must include a short hint that `--age-basis mtime` is more portable and more aggressive, including an example such as `xdg-thumbnail-prune --older-than 30d --age-basis mtime`. JSONL summaries expose this through the timestamp skip counters and selected age basis rather than through prose hints.

Example with `--include-nonstandard-files` enabled:

```text
would-delete normal/abcdefabcdefabcdefabcdefabcdefab.png uri=http://example.test/a.jpg class=remote reason=remote-older-than-threshold age=45d basis=atime
would-delete normal/0123456789abcdef0123456789abcdef.png class=unknown reason=missing-required-metadata
skip normal/bad.png reason=nonstandard-filename
summary scanned=421 kept=398 would-delete=2 skipped=21 errors=0 basis=atime timestamp-unavailable=0 timestamp-unreliable=0 timestamp-preservation-unavailable=0
```

When `--delete` is passed and a deletion succeeds, the human output should use `deleted` rather than `would-delete`. JSONL output should keep the cleanup decision separate from an `applied` boolean so dry-run and destructive runs are easy to compare.

When `--age-basis mtime` is used, human and JSONL output must expose that thumbnail file modification time was the basis for age-based decisions.

## Exit Codes

- `0`: scan completed and no operational errors occurred.
- `1`: one or more deletions failed.
- `2`: command-line usage error.
- `3`: cache scan failed before producing reliable results.
- `4`: scan completed but one or more nonfatal inspection errors occurred, such as unreadable entries or per-entry I/O errors that did not invalidate the whole scan. Entries skipped only because access time is unavailable, unreliable, or cannot be preserved are normal access-time policy outcomes and do not by themselves contribute to exit code `4`.

Deletion candidates found during a non-delete report are not errors.

## Safety Requirements

- The command must not delete or rewrite files unless `--delete` is passed.
- When `--age-basis atime` is active, the command should not read thumbnail contents in a way that can update thumbnail access times. Entries that cannot be inspected without potentially changing access time are reported as skipped rather than treated as age-based deletion candidates.
- Deletion should only target files located under the resolved thumbnail cache directories.
- The prune CLI should never follow thumbnail path symlinks for deletion without an explicit, reviewed design.
- The prune CLI should never follow symlinked failure namespace directories.
- The prune CLI should never create, update, regenerate, or request thumbnails.
- The prune CLI should never create or request thumbnails for files located under the personal thumbnail cache or a shared `.sh_thumbnails` repository.
- The prune CLI should never create or update shared thumbnail repositories.
- Missing size directories are not errors.
- Unreadable entries should be reported and skipped.
- Nonstandard filename deletion is not part of the initial prune CLI contract.
- Failure entry deletion must require `--delete`, `--allow-failure-deletion`, and a scan scope that includes failure entries.
