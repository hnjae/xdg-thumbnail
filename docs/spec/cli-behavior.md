# CLI Behavior

The CLI manages thumbnails in the user's Freedesktop thumbnail cache. Its first command should be a pruning command that reports planned changes by default and deletes files only when explicitly requested.

## Command Shape

Initial command shape:

```text
xdg-thumbnail prune [OPTIONS]
```

Candidate options:

```text
--older-than <DURATION>       Age threshold for remote, virtual, and removable entries. Defaults to 30d.
--delete                      Apply deletion decisions. Without this option, prune only reports planned actions.
--delete-stale-local          Delete stale local thumbnails whose originals still exist but no longer match stored metadata.
--size <SIZE>                 Restrict successful thumbnail scan to one size namespace: normal, large, x-large, or xx-large.
--scope <SCOPE>               Restrict scan scope: thumbnails, failures, or all. Defaults to thumbnails.
--include-nonstandard-files   Include nonstandard filenames in reports and deletion candidates.
--removable-prefix <PATH>     Add a local path prefix that should use age-based cleanup. Can be passed multiple times.
--ignore-fhs-media            Do not treat /media as removable by default.
--format <FORMAT>             Output format: human or jsonl. Defaults to human.
--verbose                     Print classification and timestamp details.
```

The exact option names can change during implementation, but the CLI should preserve these capabilities.

## Default Scan Scope

By default, `prune --scope thumbnails` scans these personal cache directories:

- `$XDG_CACHE_HOME/thumbnails/normal`
- `$XDG_CACHE_HOME/thumbnails/large`
- `$XDG_CACHE_HOME/thumbnails/x-large`
- `$XDG_CACHE_HOME/thumbnails/xx-large`

`x-large` and `xx-large` are supported size classes because this project targets the Freedesktop Thumbnail Managing Standard `latest` text, including the December 2020 0.9.0 history entry that adds those sizes.

If `$XDG_CACHE_HOME` is unset, blank, or relative, the fallback is `$HOME/.cache/thumbnails`.

The command should not scan shared thumbnail repositories by default. Failure entries under `$XDG_CACHE_HOME/thumbnails/fail/<program-version>/` are separate failure namespaces and are scanned only with `--scope failures` or `--scope all`.

`--size` applies only to successful thumbnail size namespaces. Passing `--size` with `--scope failures` or `--scope all` is a usage error because failure entries are scoped by application identifier rather than thumbnail size, and `--scope all` intentionally includes both namespace families.

By default, deletion decisions apply only to standard thumbnail entry filenames: a 32-character lowercase hexadecimal MD5 digest followed by `.png`. Files with nonstandard names are reported as skipped when visible during scanning and are not deletion candidates unless `--include-nonstandard-files` is passed. Even with `--include-nonstandard-files`, actual deletion still requires `--delete`.

## Report Output

The default output should be readable for humans. Stable machine-readable output should be available through `--format jsonl`. Each candidate should include the thumbnail path, original URI if available, classification, decision, whether the decision was applied, reason, and the timestamp basis for age-based decisions.

Example:

```text
would-delete normal/abcdefabcdefabcdefabcdefabcdefab.png uri=http://example.test/a.jpg class=remote reason=older-than-threshold age=45d
keep normal/def456.png uri=file:///home/user/photo.jpg class=local-stable reason=valid
would-delete normal/0123456789abcdef0123456789abcdef.png class=unknown reason=malformed-metadata
skip normal/bad.png reason=nonstandard-filename
```

When `--delete` is passed and a deletion succeeds, the human output should use `deleted` rather than `would-delete`. JSONL output should keep the cleanup decision separate from an `applied` boolean so dry-run and destructive runs are easy to compare.

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
