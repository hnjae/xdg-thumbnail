# xdg-thumbnail Documentation

This directory contains project-level documentation for the `xdg-thumbnail` workspace.

Initial implementation scope is Unix-like XDG desktop environments. The initial sandboxed `xdg-thumbnail-generate` design specifically requires Linux with `bubblewrap`; other platforms may still support cache inspection and pruning APIs where their filesystem semantics are compatible.

- `architecture/`: internal design, crate boundaries, data flow, and implementation constraints.
- `research/`: dated investigation notes for upstream behavior, references, and implementation decisions.
- `spec/`: externally visible behavior, CLI contract, cleanup policy, and public library API contract.

Primary references:

- Freedesktop Thumbnail Managing Standard latest text: <https://specifications.freedesktop.org/thumbnail/latest/>
- Thumbnail directory layout: <https://specifications.freedesktop.org/thumbnail/latest/directory.html>
- Thumbnail metadata keys: <https://specifications.freedesktop.org/thumbnail/latest/creation.html>
- Thumbnail URI canonicalization: `spec/uri-canonicalization.md`
- Prune CLI behavior: `spec/prune-cli-behavior.md`
- Generate CLI behavior: `spec/generate-cli-behavior.md`
- Thumbnail filename hashing: <https://specifications.freedesktop.org/thumbnail/latest/thumbsave.html>
- Modification checks: <https://specifications.freedesktop.org/thumbnail/latest/modifications.html>
- Deletion guidance: <https://specifications.freedesktop.org/thumbnail/latest/delete.html>
- Shared thumbnail repositories: <https://specifications.freedesktop.org/thumbnail/latest/shared.html>

This project targets the `latest` text, including the December 2020 0.9.0 history entry that adds `x-large` and `xx-large`, even though the freedesktop specification index still lists only the 0.8.0 versioned link.
