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
--dry-run                     Report planned thumbnailer selection and target paths without running thumbnailers or writing cache entries.
--timeout <DURATION>          Maximum runtime for each thumbnailer invocation. Defaults to 30s.
--sandbox <MODE>              Thumbnailer sandbox mode: required or off. Defaults to required.
--format <FORMAT>             Output format: human or jsonl. Defaults to human.
--verbose                     Print discovery, MIME, command, validation, and cache-write details.
```

The option names above are the initial generate CLI contract. Behavior or option-name changes require a spec update.

## Platform And Sandbox Support

The initial sandboxed generate CLI supports Linux systems where `bubblewrap` (`bwrap`) is available and can create the required mount and network namespaces. On other Unix-like systems, or on Linux systems where `bwrap` is unavailable or cannot provide the requested isolation, `--sandbox required` fails before invoking a thumbnailer and reports the missing sandbox capability. The command must not fall back to unsandboxed execution unless the user explicitly passes `--sandbox off`.

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

Each candidate must contain a `[Thumbnailer Entry]` group with `Exec` and `MimeType` keys. `TryExec` is optional; when present, the entry is ignored unless the referenced executable exists and is executable using the same path lookup rules as desktop entries. Duplicate thumbnailer filenames are resolved by discovery precedence: the first candidate by data-directory precedence wins for that filename.

## Matching And Invocation

For each input, the generate CLI determines the MIME type using the platform shared MIME database. A thumbnailer matches when the detected MIME type is listed in the entry's semicolon-separated `MimeType` list, including canonical MIME aliases and subtype relationships exposed by the shared MIME database. When multiple thumbnailers match, selection is deterministic: higher-precedence discovery directories win, then lexical thumbnailer filename order within the same directory.

The `Exec` value uses thumbnailer command syntax: desktop-entry-style string unescaping and command-line tokenization, followed by thumbnailer-specific field-code expansion. The generate CLI must not apply Desktop Entry field-code meanings to `.thumbnailer` entries; in this format `%i`, `%u`, `%o`, and `%s` have the thumbnailer meanings documented below. The expanded command is executed directly as an argument vector inside a thumbnailer sandbox by default. The generate CLI must not invoke a shell implicitly. Shell behavior is used only when the selected thumbnailer explicitly names a shell in `Exec`.

The default `--sandbox required` mode runs thumbnailers with sandbox isolation that prevents ambient access to the user's home, personal thumbnail cache, user configuration directories, user data directories, and network. The sandbox must allow the thumbnailer to read the selected input, read required read-only system resources, execute the resolved thumbnailer program or interpreter, and write only to the CLI-provided temporary output location. Required system resources may include executable paths, interpreters, dynamic loader state, MIME data, codecs, font configuration, and other read-only files under system locations such as `/usr`, `/lib`, or `/etc`. User-controlled XDG data and configuration directories are not exposed unless a later compatibility mode explicitly documents that behavior. If the required sandbox cannot be created or cannot expose the selected executable and required runtime files read-only, generation fails with an actionable diagnostic before running the thumbnailer. The generate CLI must not silently retry without sandboxing. `--sandbox off` disables sandboxing explicitly for users who choose to trust the selected thumbnailer; reports must expose that the thumbnailer ran without sandbox isolation.

The initial field codes are:

- `%i`: sandbox-visible local input path for the original.
- `%u`: canonical original file URI.
- `%o`: sandbox-visible temporary output PNG path supplied by the generate CLI.
- `%s`: requested thumbnail size in pixels.
- `%%`: literal percent sign.

Unknown field codes are usage errors for that thumbnailer entry and must be reported without running the command. The temporary output path passed through `%o` must not be the final cache path, so failed or partial thumbnailer output is never visible as a cache entry.

## Cache Write Policy

For each requested size, the generate CLI computes the target cache filename from the canonical original URI. If a valid existing thumbnail already matches the readability-confirmed original identity and requested size, the generate CLI keeps it and does not run a thumbnailer unless `--force` is passed.

When a thumbnailer exits successfully, the generate CLI verifies that the temporary output exists, is readable, and can be handed to the library as rendered thumbnail input. The library performs final PNG conformance validation, namespace dimension validation, metadata writing, permission-controlled temporary-file creation, and atomic installation under the resolved personal thumbnail cache directory. The installed PNG must contain the personal-cache metadata required for validation, including at least `Thumb::URI` and `Thumb::MTime`, and including `Thumb::Size` and `Thumb::Mimetype` when those values are available.

Generated thumbnails must obey the same maximum dimensions as successful cache entries in the requested namespace. Non-PNG output, invalid PNG structure, dimension violations, missing temporary output, nonzero thumbnailer exit status, timeout, metadata-write failure, permission failure, or cache-installation failure are generation failures for that input and size.

## Report Output

The default human output should report generated entries, kept valid entries, skipped inputs, sandbox eligibility failures, thumbnailer failures, validation failures, and a final summary. Initial machine-readable output should be available through `--format jsonl`; the initial JSONL schema is unstable and may change before the project reaches a stable release. JSONL emits one record for each requested input and size so dry-run and write runs can be compared.

Each reported entry should include the input path, canonical original URI when it could be constructed, MIME type when known, selected thumbnailer when one was selected, sandbox mode and whether sandbox isolation was applied, target namespace, target cache path when it could be computed, decision, whether the decision was applied, and reason.

For reporting and exit-code purposes, the initial decisions are:

| Condition                                                                                                                                                                                                                                        | Decision                      | Exit contribution |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ----------------------------- | ----------------- |
| Existing thumbnail is valid and `--force` is not passed                                                                                                                                                                                          | `keep`                        | none              |
| Thumbnailer succeeds and cache installation succeeds                                                                                                                                                                                             | `generated`                   | none              |
| Input cannot be opened, original metadata cannot be read, MIME type cannot be determined, or no matching thumbnailer exists                                                                                                                      | `skip`                        | `4`               |
| Invalid thumbnailer entry is discovered but is not selected for a requested input-size pair                                                                                                                                                      | discovery or matching warning | `4`               |
| Selected thumbnailer entry is invalid, sandbox eligibility fails for the selected thumbnailer, thumbnailer execution fails, timeout occurs, output validation fails, metadata writing fails, permission setup fails, or cache installation fails | `failed`                      | `1`               |
| Command-line parsing fails                                                                                                                                                                                                                       | usage error                   | `2`               |
| Cache root resolution, thumbnailer discovery, or sandbox backend setup fails before reliable per-input reporting can be produced                                                                                                                 | abort                         | `3`               |

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
- `4`: generation completed but one or more requested input-size pairs were skipped or one or more nonfatal discovery, inspection, or matching errors occurred, such as unreadable inputs, unknown MIME types, no matching thumbnailer, or invalid thumbnailer entries that were not selected for a requested input-size pair.

## Safety Requirements

- `--dry-run` must not run thumbnailers and must not create, update, delete, or replace cache entries.
- The generate CLI must write only through temporary output files and atomic final installation under the resolved personal thumbnail cache directory.
- The generate CLI must not pass the final cache path as `%o`.
- The generate CLI must not generate thumbnails for files inside the personal thumbnail cache or a shared `.sh_thumbnails` repository.
- The generate CLI must not create or update shared thumbnail repositories.
- The generate CLI must not write failure entries unless a later spec explicitly defines failure-entry ownership and opt-in behavior.
- The generate CLI must not accept remote URI inputs, fetch remote originals as a source-acquisition step, or initiate mounts for remote or removable source filesystems on its own. Explicit local paths supplied by the user may still be backed by user-mounted FUSE, portal, removable, or network filesystems; the generate CLI treats them as local path inputs, while `--sandbox required` still prevents thumbnailer network access. Sandbox namespace setup and bind mounts used only to isolate a local thumbnailer invocation are allowed.
- With `--sandbox required`, the generate CLI must not run a thumbnailer unless the sandbox can restrict network access and ambient filesystem access as documented above.
- With `--sandbox off`, the generate CLI may run the selected thumbnailer without isolation only because the user explicitly requested that mode.
