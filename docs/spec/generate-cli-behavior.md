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

## Input Scope

The initial generate CLI accepts local filesystem paths only. Relative input paths are resolved against the current working directory into absolute paths before URI construction, but the generate CLI must not perform hidden symlink canonicalization as a URI normalization step. The generate CLI owns CLI input policy such as recursive-cache rejection, while the `xdg-thumbnail` library owns encoding the resulting absolute path bytes into the canonical personal-cache `file:` URI described in `docs/spec/uri-canonicalization.md`.

Inputs located inside the resolved personal thumbnail cache or a shared `.sh_thumbnails` repository are rejected. This prevents recursive thumbnail generation and keeps generated cache entries tied to original user content rather than cache artifacts.

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

The default `--sandbox required` mode runs thumbnailers with sandbox isolation that prevents ambient access to the user's home, personal thumbnail cache, configuration directories, and network. The sandbox must allow the thumbnailer to read the selected input, read required system resources, execute the resolved thumbnailer program or interpreter, and write only to the CLI-provided temporary output location. If the required sandbox cannot be created or cannot expose the selected executable and required runtime files read-only, generation fails with an actionable diagnostic before running the thumbnailer. The generate CLI must not silently retry without sandboxing. `--sandbox off` disables sandboxing explicitly for users who choose to trust the selected thumbnailer; reports must expose that the thumbnailer ran without sandbox isolation.

The initial field codes are:

- `%i`: sandbox-visible local input path for the original.
- `%u`: canonical original file URI.
- `%o`: sandbox-visible temporary output PNG path supplied by the generate CLI.
- `%s`: requested thumbnail size in pixels.
- `%%`: literal percent sign.

Unknown field codes are usage errors for that thumbnailer entry and must be reported without running the command. The temporary output path passed through `%o` must not be the final cache path, so failed or partial thumbnailer output is never visible as a cache entry.

## Cache Write Policy

For each requested size, the generate CLI computes the target cache filename from the canonical original URI. If a valid existing thumbnail already matches the original identity and requested size, the generate CLI keeps it and does not run a thumbnailer unless `--force` is passed.

When a thumbnailer runs successfully, the generate CLI validates the temporary output as a PNG suitable for the requested size namespace. It then writes or rewrites the final PNG metadata required for personal-cache validation, including at least `Thumb::URI` and `Thumb::MTime`, and including `Thumb::Size` and `Thumb::Mimetype` when those values are available. The final cache entry is installed atomically under the resolved personal thumbnail cache directory.

Generated thumbnails must obey the same maximum dimensions as successful cache entries in the requested namespace. Non-PNG output, invalid PNG structure, dimension violations, missing temporary output, nonzero thumbnailer exit status, timeout, or metadata-write failure are generation failures for that input and size.

## Report Output

The default human output should report generated entries, kept valid entries, skipped inputs, sandbox eligibility failures, thumbnailer failures, validation failures, and a final summary. Initial machine-readable output should be available through `--format jsonl`; the initial JSONL schema is unstable and may change before the project reaches a stable release. JSONL emits one record for each requested input and size so dry-run and write runs can be compared.

Each reported entry should include the input path, canonical original URI, MIME type when known, selected thumbnailer when one was selected, sandbox mode and whether sandbox isolation was applied, target namespace, target cache path, decision, whether the decision was applied, and reason.

Example:

```text
generated normal/abcdefabcdefabcdefabcdefabcdefab.png input=/home/alice/photo.jpg mime=image/jpeg thumbnailer=gdk-pixbuf-thumbnailer reason=created
keep normal/0123456789abcdef0123456789abcdef.png input=/home/alice/existing.png reason=already-valid
skip /home/alice/archive.foo reason=no-matching-thumbnailer
summary inputs=3 requested=3 generated=1 kept=1 skipped=1 failed=0
```

## Exit Codes

- `0`: generation completed and no requested input-size pair failed.
- `1`: one or more requested input-size pairs failed during sandbox eligibility checks for the selected thumbnailer, thumbnailer execution, output validation, metadata writing, or cache installation.
- `2`: command-line usage error.
- `3`: thumbnailer discovery, global sandbox backend setup, or cache root resolution failed before producing reliable results.
- `4`: generation completed but one or more nonfatal inspection or matching errors occurred, such as unreadable inputs or invalid thumbnailer entries that did not prevent other requested work.

## Safety Requirements

- `--dry-run` must not run thumbnailers and must not create, update, delete, or replace cache entries.
- The generate CLI must write only through temporary output files and atomic final installation under the resolved personal thumbnail cache directory.
- The generate CLI must not pass the final cache path as `%o`.
- The generate CLI must not generate thumbnails for files inside the personal thumbnail cache or a shared `.sh_thumbnails` repository.
- The generate CLI must not create or update shared thumbnail repositories.
- The generate CLI must not write failure entries unless a later spec explicitly defines failure-entry ownership and opt-in behavior.
- The generate CLI must not contact remote servers or mount remote or removable source filesystems on its own. Sandbox namespace setup and bind mounts used only to isolate a local thumbnailer invocation are allowed.
- With `--sandbox required`, the generate CLI must not run a thumbnailer unless the sandbox can restrict network access and ambient filesystem access as documented above.
- With `--sandbox off`, the generate CLI may run the selected thumbnailer without isolation only because the user explicitly requested that mode.
