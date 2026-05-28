---
date: 2026-05-28
---

# GNOME Thumbnailer Documentation Research

This note records the documentation found for implementing `xdg-thumbnail-generate` against GNOME-style thumbnailers.

## Summary

GNOME has official thumbnailer documentation, but it is not a standalone product specification named "GNOME thumbnailer specification." The practical reference is the `GnomeDesktopThumbnailFactory` API documentation in `gnome-desktop`, plus the Freedesktop Thumbnail Managing Standard for cache storage, filenames, metadata, and validation rules.

For `xdg-thumbnail-generate`, the GNOME API documentation is the best source for `.thumbnailer` discovery and invocation behavior. The Freedesktop standard remains the normative source for cache directory layout, PNG cache entries, URI-derived MD5 filenames, thumbnail metadata, size namespaces, atomic save behavior, and failure-entry shape.

## Primary Sources

- GNOME `GnomeDesktopThumbnailFactory` API reference: <https://gnome.pages.gitlab.gnome.org/gnome-desktop/html/gnome-desktop3/gnome-desktop3-GnomeDesktopThumbnailFactory.html>
- Freedesktop Thumbnail Managing Standard: <https://specifications.freedesktop.org/thumbnail/latest-single/>
- Freedesktop thumbnails specification index: <https://www.freedesktop.org/wiki/Specifications/thumbnails/>
- `gnome-desktop` repository README and source mirror: <https://github.com/GNOME/gnome-desktop>

## GNOME Thumbnailer Model

`GnomeDesktopThumbnailFactory` generates and looks up thumbnails by selecting external thumbnailer programs for content types. The documentation says thumbnailers are described by `.thumbnailer` files installed under `share/thumbnailers`, using a key-file format with a `[Thumbnailer Entry]` group.

The documented required keys are `Exec` and `MimeType`. `Exec` is the command template to run. `MimeType` is a semicolon-separated list of MIME types supported by that thumbnailer.

The documented `Exec` field codes are `%u` for the original file URI, `%i` for the original local path, `%o` for the output image path to write, `%s` for the requested maximum thumbnail size in pixels, and `%%` for a literal percent sign.

The thumbnailer contract is process-based: on success, the thumbnailer writes the output image before exiting; on failure, it should not write an image and should return a nonzero exit status. GNOME then may fall back to `gdk-pixbuf` where possible.

## Implementation Details Observed In GNOME Source

The upstream `gnome-desktop` source complements the generated API documentation. Current source loads thumbnailers from `g_get_user_data_dir()/thumbnailers` followed by each `g_get_system_data_dirs()` entry with `/thumbnailers` appended, which corresponds to `$XDG_DATA_HOME/thumbnailers` and `$XDG_DATA_DIRS/*/thumbnailers` after GLib's XDG resolution.

The source accepts an optional `TryExec` key. When present, GNOME skips the thumbnailer if the referenced program is not found in `PATH`.

The source reads GNOME settings from `org.gnome.desktop.thumbnailers`, including `disable-all` and `disable`, to disable all external thumbnailers or selected MIME types.

The source maps cache size namespaces to `normal`, `large`, `x-large`, and `xx-large`, with maximum sizes of 128, 256, 512, and 1024 pixels respectively. The generated API page still describes normal and large in the prose, but the enum includes `GNOME_DESKTOP_THUMBNAIL_SIZE_XLARGE` and `GNOME_DESKTOP_THUMBNAIL_SIZE_XXLARGE`.

## Temporary Output And Final Cache Save

GNOME separates thumbnailer execution from final cache installation. `gnome_desktop_thumbnail_factory_generate_thumbnail()` selects the thumbnailer command for the MIME type and calls `gnome_desktop_thumbnail_script_exec()` to run it.

`gnome_desktop_thumbnail_script_exec()` creates an execution context with an input path and a temporary output path. In the `bwrap` case, the temporary output directory is bind-mounted as `/tmp` inside the sandbox, the source file is read-only bound into the sandbox, and the expanded thumbnailer command receives the sandbox-visible input and output paths through `%i`, `%u`, and `%o`.

The external thumbnailer writes a PNG to the `%o` temporary output path. After the thumbnailer exits successfully, GNOME reads that temporary output file into memory and converts it into a `GdkPixbuf`. The temporary file and temporary directory are cleanup artifacts, not the final Freedesktop cache entry.

Final cache installation is performed later by `gnome_desktop_thumbnail_factory_save_thumbnail()`. That function computes the URI-derived target path, writes a new PNG via `gdk_pixbuf_save()`, adds GNOME-owned metadata such as `tEXt::Thumb::URI`, `tEXt::Thumb::MTime`, and `tEXt::Software`, sets private permissions, and atomically renames the temporary save file into the final cache path.

The practical boundary is that `.thumbnailer` helpers render image pixels to the output path they are given, while `libgnome-desktop` owns cache naming, final PNG writing, metadata insertion, permissions, and atomic publication. A standalone `xdg-thumbnail-generate` implementation that does not call `libgnome-desktop` must implement the same final-save responsibilities itself after running a `.thumbnailer` helper.

## Cache Storage Rules

The Freedesktop Thumbnail Managing Standard defines the user thumbnail cache under `$XDG_CACHE_HOME/thumbnails`, falling back to `$HOME/.cache/thumbnails` via the XDG Base Directory rules.

The standard size directories are `normal`, `large`, `x-large`, and `xx-large`, and the failure area is `fail`. The maximum dimensions are 128, 256, 512, and 1024 pixels respectively. Thumbnails must preserve aspect ratio rather than being stretched to a square.

The thumbnail filename is the MD5 hash of the canonical original URI with `.png` appended. The hash is over the URI string, not the file contents. For example, a `file:///...` URI maps to `$XDG_CACHE_HOME/thumbnails/<size>/<md5>.png`.

Successful cache entries are PNG files. Relevant PNG text metadata includes `Thumb::URI`, `Thumb::MTime`, `Thumb::Size`, and `Thumb::Mimetype`. For personal-cache validation, `Thumb::URI` and `Thumb::MTime` are required identity and freshness checks; `Thumb::Size` should be checked when present.

Concurrent writers should write to a temporary file in the target directory and then atomically rename it to the final thumbnail filename. Cache directories should be private to the user, and thumbnail files should not expose private source content through broader permissions.

## Failure Entries

The Freedesktop standard defines failure entries as per-program records under `$XDG_CACHE_HOME/thumbnails/fail/<program-version>/`. They are PNG metadata carriers named using the same URI-derived hash procedure as successful thumbnails.

GNOME's `GnomeDesktopThumbnailFactory` uses `$XDG_CACHE_HOME/thumbnails/fail/gnome-thumbnail-factory/` for its own failed-thumbnail records. This is GNOME-specific ownership, not a general instruction that other programs should write into that namespace.

For `xdg-thumbnail-generate`, the existing project spec that avoids writing failure entries by default remains consistent with the standard because failure entries are application-specific retry state.

## API Stability And Security Notes

The `GnomeDesktopThumbnailFactory` API reference marks the API stability level as unstable. Depending on `libgnome-desktop` as a public stable API would therefore carry compatibility risk.

The `gnome-desktop` README says thumbnailing uses sandboxing and notes runtime dependencies such as `bwrap` on supported platforms. The security model limits thumbnailers to the source file, public system files, and the thumbnail output path, and prevents network access and arbitrary file writes. A standalone `xdg-thumbnail-generate` implementation that executes `.thumbnailer` commands directly should treat sandboxing as a separate design decision rather than assuming GNOME's safety properties automatically apply.

## Implications For `xdg-thumbnail-generate`

Use the GNOME documentation as the compatibility target for `.thumbnailer` file format and command field-code expansion.

Use the Freedesktop standard as the compatibility target for output cache layout, target filename calculation, PNG metadata, size validation, permissions, and atomic installation.

Discover thumbnailers through XDG data directories rather than only `$PREFIX/share/thumbnailers`, because GNOME source resolves user and system XDG data locations.

Support `TryExec` for GNOME compatibility, even though the generated API prose only lists the three primary `.thumbnailer` keys.

Do not assume `GnomeDesktopThumbnailFactory` is a stable library dependency. Reimplementing the small discovery and invocation subset may be preferable for `xdg-thumbnail-generate`, especially because this project is pre-release and already has its own CLI behavior spec.

Treat direct execution of external thumbnailers as security-sensitive. The project architecture uses `bubblewrap` as the initial sandbox backend for `xdg-thumbnail-generate`; any explicit unsandboxed mode should be documented as a user opt-out and must keep `%o` pointed at an isolated temporary output path, never the final cache path.
