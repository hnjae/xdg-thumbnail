# Generate CLI Behavior

The `xdg-thumbnail-generate` CLI creates personal Freedesktop thumbnail cache entries for explicit local filesystem inputs by selecting and running installed `.thumbnailer` helpers. It is a generation tool, not a cleanup tool, and it must not delete existing cache entries except for replacing a target thumbnail through an atomic successful regeneration.

## Command Shape

Initial command shape:

```text
xdg-thumbnail-generate [OPTIONS] <PATH>...
```

Options:

```text
--size <SIZE>                 Generate one size namespace: normal, large, x-large, or xx-large. Defaults to normal. Can be passed multiple times.
--force                       Regenerate even when a valid target thumbnail already exists.
--dry-run                     Report planned thumbnailer selection, sandbox eligibility, and target paths without running thumbnailers or writing cache entries.
--timeout <DURATION>          Maximum runtime for each thumbnailer invocation. Defaults to 30s. See Duration Syntax.
--sandbox <MODE>              Thumbnailer sandbox mode: required or off. Defaults to required, which requires Linux with bubblewrap and never falls back to unsandboxed execution.
--format <FORMAT>             Output format: human or jsonl. Defaults to human.
--verbose                     Print discovery, MIME, command, validation, and cache-write details.
```

The option names above are the initial generate CLI contract. Behavior or option-name changes require a spec update.

## Duration Syntax

`<DURATION>` values are positive base-10 integers followed by a unit suffix with no whitespace. Supported units are `s` for seconds, `m` for minutes, `h` for hours, and `d` for 24-hour days. `0`, negative values, fractional values, missing units, unknown units, and values too large to represent safely are usage errors.

## Default Platform And Sandbox Requirement

The default generate CLI behavior is intentionally sandbox-first. `--sandbox required` is the default, and it requires Linux with `bubblewrap` (`bwrap`) available and capable of creating the required mount and network namespaces. On non-Linux systems, or on Linux systems where `bwrap` is unavailable or cannot provide the requested isolation, the default command fails before invoking a thumbnailer and reports the missing sandbox capability. CLI help, startup diagnostics, dry-run records, and summaries for sandbox-related failures must state plainly that the default mode requires Linux `bubblewrap` support, that no unsandboxed fallback is attempted, and that users who intentionally trust the selected thumbnailer may rerun with `--sandbox off`.

In `--dry-run` mode, sandbox capability and selected-thumbnailer eligibility are checked without executing thumbnailers; missing sandbox capability should be reported as a per-input-size planned failure when MIME matching and target-path calculation can still produce reliable records, and should abort only when reliable per-input reporting cannot be produced. The command must not fall back to unsandboxed execution unless the user explicitly passes `--sandbox off`.

The initial sandbox eligibility model is intentionally narrow. The generate CLI is not required to infer arbitrary runtime dependencies for thumbnailers, user plugins, user configuration, or helper programs outside the documented sandbox profile. A selected thumbnailer is eligible for `--sandbox required` only when its resolved executable, resolved script interpreter if any, and literal host paths required by the command template can be exposed read-only without exposing user-controlled directories wholesale. User-provided `.thumbnailer` files may be discovered, but they are not automatically sandbox-eligible merely because they are discoverable; entries that depend on user-controlled executables, scripts, plugins, codecs, configuration, or data trees must fail with `sandbox-ineligible` under `--sandbox required` unless a later compatibility mode documents broader exposure. Entries whose `Exec` command resolves to a shell such as `sh` or `bash`, including shell commands reached through an interpreter wrapper, are not sandbox-eligible in the initial `--sandbox required` mode because the command's filesystem behavior cannot be described by the documented profile. Users can still run such entries with `--sandbox off`.

## Input Scope

The initial generate CLI accepts local filesystem paths only. Relative input paths are resolved against the current working directory into absolute paths before URI construction, but the generate CLI must not perform hidden symlink canonicalization as a URI normalization step. The resulting absolute path bytes are encoded into the canonical personal-cache `file:` URI described in `docs/spec/uri-canonicalization.md`.

Inputs located inside the resolved personal thumbnail cache or a shared `.sh_thumbnails` repository are rejected. This prevents recursive thumbnail generation and keeps generated cache entries tied to original user content rather than cache artifacts.

Before keeping an existing thumbnail as valid or installing a newly generated thumbnail, the generate CLI must confirm that the local input can be opened for reading and that the original modification time can be obtained. If this cannot be confirmed, the input-size pair is skipped with a nonfatal report reason, contributes to exit code `4` unless a higher-priority fatal condition also occurs, and the generate CLI must not treat an existing cache entry as display-valid or write a successful or failure cache entry for that input.

The generate CLI writes only to the user's personal thumbnail cache under `$XDG_CACHE_HOME/thumbnails/<size>/`, with the same `$XDG_CACHE_HOME` fallback behavior documented for the library and prune CLI. The initial generate CLI must not create or update shared thumbnail repositories and must not write failure entries under `thumbnails/fail/`.

## Thumbnailer Discovery

The generate CLI discovers `.thumbnailer` files from XDG data directories, in precedence order:

- `$XDG_DATA_HOME/thumbnailers`, falling back to `$HOME/.local/share/thumbnailers` when `$XDG_DATA_HOME` is unset, blank, or relative.
- Each absolute directory in `$XDG_DATA_DIRS`, falling back to `/usr/local/share:/usr/share` when `$XDG_DATA_DIRS` is unset or blank, with `/thumbnailers` appended to each data directory.

Only files whose names end in `.thumbnailer` are candidates. Missing discovery directories are ignored. Relative XDG data directories are ignored because they cannot define stable system or user thumbnailer locations.

Each candidate must contain a `[Thumbnailer Entry]` group with `Exec` and `MimeType` keys. `TryExec` is optional; when present, the entry is ignored unless the referenced executable exists and is executable using the same path lookup rules as desktop entries. Duplicate thumbnailer filenames are resolved by discovery precedence: the first candidate by data-directory precedence wins for that filename. Discovery records must retain whether the candidate came from the user data directory or a system data directory so sandbox eligibility diagnostics can explain why a selected user-provided thumbnailer could or could not be isolated.

## Matching And Invocation

For each input, the generate CLI determines the MIME type using the platform shared MIME database. A thumbnailer matches when the detected MIME type is listed in the entry's semicolon-separated `MimeType` list, including canonical MIME aliases and subtype relationships exposed by the shared MIME database. When multiple thumbnailers match, selection is deterministic: higher-precedence discovery directories win, then lexical thumbnailer filename order within the same directory. The selected thumbnailer owns source decoding and source interpretation, including metadata such as Exif orientation, and source-aware scaling decisions such as preserving the original aspect ratio. The generate CLI validates the rendered output for cache conformance but does not inspect the original source format to repair renderer mistakes.

The `Exec` value uses thumbnailer command syntax: desktop-entry-style string unescaping and command-line tokenization, followed by thumbnailer-specific field-code expansion. The generate CLI must not apply Desktop Entry field-code meanings to `.thumbnailer` entries; in this format `%i`, `%u`, `%o`, and `%s` have the thumbnailer meanings documented below. The expanded command is executed directly as an argument vector inside a thumbnailer sandbox by default. The generate CLI must not invoke a shell implicitly. Shell behavior is used only when the selected thumbnailer explicitly names a shell in `Exec`; such entries are `sandbox-ineligible` under the initial `--sandbox required` mode and may run only when the user explicitly chooses `--sandbox off` or a later spec defines a broader compatibility mode.

The default `--sandbox required` mode runs thumbnailers with sandbox isolation that prevents ambient access to the user's home, personal thumbnail cache, user configuration directories, user data directories, and network. The sandbox must allow the thumbnailer to read the selected input through sandbox-visible input path and URI values, read documented read-only runtime resources, execute the resolved thumbnailer program or resolved script interpreter, and write only to the CLI-provided temporary output location. The initial profile may expose read-only system locations such as `/usr`, `/bin`, `/sbin`, `/lib`, `/lib64`, and `/etc` when needed for ordinary system thumbnailers to start, plus a private writable temporary output directory. User-controlled XDG data and configuration directories, arbitrary home paths, the personal thumbnail cache, and arbitrary writable host paths are not exposed wholesale unless a later compatibility mode explicitly documents that behavior. If the required sandbox cannot be created or the selected thumbnailer does not fit the initial sandbox profile, generation fails with an actionable diagnostic before running the thumbnailer. The diagnostic must repeat that the default requires Linux `bubblewrap`; this is expected behavior on unsupported systems rather than an implicit request to run unsandboxed. The generate CLI must not silently retry without sandboxing. `--sandbox off` disables sandboxing explicitly for users who choose to trust the selected thumbnailer; reports must expose that the thumbnailer ran without sandbox isolation.

The generate CLI maintains two URI identities when sandbox path mapping is used. The cache identity URI is the canonical original URI derived from the host input path and is used for the Freedesktop MD5 filename, `Thumb::URI` metadata, validation, and reports. The thumbnailer input URI is the URI passed through `%u` so the external thumbnailer can open the sandbox-visible input. These values may differ under `--sandbox required`; the thumbnailer input URI must never be used for cache filename calculation or installed `Thumb::URI` metadata.

The initial field codes are:

- `%i`: sandbox-visible local input path for the original.
- `%u`: sandbox-visible original file URI for the thumbnailer process. With `--sandbox off`, this is normally the same local file URI as the cache identity URI. With `--sandbox required`, this may use the sandbox-visible input path.
- `%o`: sandbox-visible temporary output PNG path supplied by the generate CLI.
- `%s`: requested thumbnail size in pixels.
- `%%`: literal percent sign.

Unknown field codes are usage errors for that thumbnailer entry and must be reported without running the command. The temporary output path passed through `%o` must not be the final cache path, so failed or partial thumbnailer output is never visible as a cache entry.

## Cache Write Policy

For each requested size, the generate CLI computes the target cache filename from the canonical cache identity URI, not from any sandbox-visible thumbnailer input URI. If a valid existing thumbnail already matches the readability-confirmed original identity and requested size, the generate CLI keeps it and does not run a thumbnailer unless `--force` is passed.

When a thumbnailer exits successfully, the generate CLI verifies that the temporary output exists, is readable, and can be handed to the library as rendered thumbnail input. The library performs final PNG conformance validation, namespace dimension validation, metadata writing, permission-controlled temporary-file creation, and atomic installation under the resolved personal thumbnail cache directory. The installed PNG must contain the personal-cache metadata required for validation, including at least `Thumb::URI` set to the cache identity URI and `Thumb::MTime`, and including `Thumb::Size` and `Thumb::Mimetype` when those values are available.

Generated thumbnails must obey the same maximum dimensions as successful cache entries in the requested namespace and should preserve the original aspect ratio. The initial generate CLI treats aspect-ratio correctness as the renderer or thumbnailer responsibility because it does not decode every source format or compare rendered dimensions against source dimensions. Non-PNG output, invalid PNG structure, nonconforming PNG encoding, dimension violations, missing temporary output, nonzero thumbnailer exit status, timeout, metadata-write failure, permission failure, or cache-installation failure are generation failures for that input and size.

## Report Output

The default human output should report generated entries, kept valid entries, skipped inputs, sandbox eligibility failures, thumbnailer failures, validation failures, and a final summary. Initial machine-readable output should be available through `--format jsonl`. JSONL emits one record for each requested input and size so dry-run and write runs can be compared, plus summary and warning records when needed. Because the project is pre-release, additive JSONL fields may be added without a compatibility promise, but removing or renaming the v0 fields below requires a spec update.

Each JSONL entry record must include at least `schema_version: 0`, `event: "entry"`, `input_path_display`, `input_path_bytes_b64`, `uri`, `thumbnailer_uri`, `mime_type`, `thumbnailer`, `sandbox_mode`, `sandbox_applied`, `sandbox_eligibility`, `namespace`, `cache_path_display`, `cache_path_bytes_b64`, `decision`, `applied`, `reason`, and `error`. The `uri` field is the canonical cache identity URI used for hashing and installed metadata. The `thumbnailer_uri` field is the `%u` value passed to the thumbnailer and is `null` when no thumbnailer command is planned or run. The `*_display` fields are human-oriented UTF-8 strings suitable for logs and may use escaping or replacement for non-UTF-8 path bytes. The `*_bytes_b64` fields use unpadded RFC 4648 standard base64 over the exact Unix path bytes and are the lossless machine-readable representation; they are `null` only when the corresponding path could not be computed. Nullable fields are represented as `null` rather than omitted when the value could not be computed. `error` is either `null` or an object with stable `kind` and human-oriented `message` fields. Summary records use `event: "summary"` with counters for requested, generated, kept, skipped, failed, warnings, and `exit_code`. Warning records use `event: "warning"` and do not replace per-entry records.

The initial stable `reason` identifiers are `already-valid`, `created`, `dry-run`, `uri-construction-failed`, `input-unreadable`, `original-metadata-unavailable`, `mime-unknown`, `no-matching-thumbnailer`, `thumbnailer-entry-invalid`, `sandbox-unavailable`, `sandbox-ineligible`, `thumbnailer-exit`, `thumbnailer-timeout`, `thumbnailer-output-missing`, `thumbnailer-output-unreadable`, `output-invalid-png`, `output-nonconforming-png`, `output-dimensions-exceed-namespace`, `metadata-write-failed`, `permission-setup-failed`, and `cache-install-failed`. Additional reason identifiers require a spec update when they affect documented behavior or output compatibility.

Each reported human entry should include the input path, canonical cache identity URI when it could be constructed, thumbnailer input URI when it differs from the cache identity URI, MIME type when known, selected thumbnailer when one was selected, sandbox mode and whether sandbox isolation was applied, target namespace, target cache path when it could be computed, decision, whether the decision was applied, and reason.

For reporting and exit-code purposes, the initial decisions are:

| Condition                                                                                                                                                                                                                                        | Decision                       | Exit contribution                                                          |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------ | -------------------------------------------------------------------------- |
| Existing thumbnail is valid and `--force` is not passed                                                                                                                                                                                          | `keep`                         | none                                                                       |
| Thumbnailer succeeds and cache installation succeeds                                                                                                                                                                                             | `generated`                    | none                                                                       |
| Input cannot be opened, original metadata cannot be read, MIME type cannot be determined, or no matching thumbnailer exists                                                                                                                      | `skip`                         | `4`                                                                        |
| Invalid thumbnailer entry is discovered but is unrelated to the requested input-size pairs                                                                                                                                                       | discovery warning              | none                                                                       |
| Invalid thumbnailer entry would otherwise match a requested input-size pair but cannot be selected safely                                                                                                                                        | `skip` or `failed` as reported | `4` for non-selected matching skips, `1` for selected-thumbnailer failures |
| Selected thumbnailer entry is invalid, sandbox eligibility fails for the selected thumbnailer, thumbnailer execution fails, timeout occurs, output validation fails, metadata writing fails, permission setup fails, or cache installation fails | `failed`                       | `1`                                                                        |
| Command-line parsing fails                                                                                                                                                                                                                       | usage error                    | `2`                                                                        |
| Cache root resolution, thumbnailer discovery, or sandbox backend setup fails before reliable per-input reporting can be produced                                                                                                                 | abort                          | `3`                                                                        |

When multiple categories occur in one run, the process exit code is selected by category priority rather than by numeric maximum: command-line usage error `2`, pre-report abort `3`, selected input-size failure `1`, skip-only or nonfatal matching error `4`, then success `0`. Summary records expose the selected `exit_code` using this priority.

Example:

```text
generated normal/abcdefabcdefabcdefabcdefabcdefab.png input=/home/alice/photo.jpg mime=image/jpeg thumbnailer=gdk-pixbuf-thumbnailer reason=created
keep normal/0123456789abcdef0123456789abcdef.png input=/home/alice/existing.png reason=already-valid
skip /home/alice/archive.foo reason=no-matching-thumbnailer
summary inputs=3 requested=3 generated=1 kept=1 skipped=1 failed=0
```

## Exit Codes

- `0`: generation completed, every requested input-size pair was generated or kept, and no nonfatal discovery, inspection, or matching errors occurred.
- `1`: one or more requested input-size pairs failed during selected-thumbnailer validation, sandbox eligibility checks for the selected thumbnailer, thumbnailer execution, output validation, metadata writing, permission setup, or cache installation.
- `2`: command-line usage error.
- `3`: thumbnailer discovery, sandbox backend setup, or cache root resolution failed before producing reliable results.
- `4`: generation completed but one or more requested input-size pairs were skipped or one or more nonfatal matching errors occurred, such as unreadable inputs, unknown MIME types, no matching thumbnailer, or invalid thumbnailer entries that would otherwise match a requested input-size pair but were not selected. Invalid thumbnailer entries unrelated to the requested inputs are warnings and do not contribute to the exit code.

## Safety Requirements

- `--dry-run` must not run thumbnailers and must not create, update, delete, or replace cache entries. It may perform non-mutating sandbox capability and selected-thumbnailer eligibility checks so reports can show whether a later write run would fail before execution.
- The generate CLI must write only through temporary output files and atomic final installation under the resolved personal thumbnail cache directory.
- The generate CLI must not pass the final cache path as `%o`.
- The generate CLI must not generate thumbnails for files inside the personal thumbnail cache or a shared `.sh_thumbnails` repository.
- The generate CLI must not create or update shared thumbnail repositories.
- The generate CLI must not write failure entries unless a later spec explicitly defines failure-entry ownership and opt-in behavior.
- The generate CLI must not accept remote URI inputs, fetch remote originals as a source-acquisition step, or initiate mounts for remote or removable source filesystems on its own. Explicit local paths supplied by the user may still be backed by user-mounted FUSE, portal, removable, or network filesystems; the generate CLI treats them as local path inputs, while `--sandbox required` still prevents thumbnailer network access. Sandbox namespace setup and bind mounts used only to isolate a local thumbnailer invocation are allowed.
- With `--sandbox required`, the generate CLI must not run a thumbnailer unless the sandbox can restrict network access and ambient filesystem access as documented above.
- With `--sandbox off`, the generate CLI may run the selected thumbnailer without isolation only because the user explicitly requested that mode.
