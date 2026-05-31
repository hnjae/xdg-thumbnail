# xdg-thumbnail

`xdg-thumbnail` is a Rust workspace for Freedesktop thumbnail cache tooling.

It contains a reusable library crate plus small command-line tools for generating and pruning personal thumbnail cache entries. The project currently targets Unix-like XDG desktop environments, with the default generator path focused on Linux sandboxed thumbnailer execution.

> [!IMPORTANT]
> This project was developed with AI assistance. Please review the code carefully before relying on it in production.

## Workspace

- `crates/xdg-thumbnail`: library primitives for URI identity, cache paths, validation, inspection, and installation.
- `crates/xdg-thumbnail-generate`: CLI for running installed thumbnailers and writing cache entries.
- `crates/xdg-thumbnail-prune`: CLI for reporting and optionally deleting stale or invalid cache entries.
- `docs/spec/`: external behavior contracts.
- `docs/architecture/`: internal design notes.

## Development

```sh
devenv shell
devenv tasks run ci:check
```

## Links

- Freedesktop Thumbnail Managing Standard: <https://specifications.freedesktop.org/thumbnail/latest/>
- Rust crate README: `crates/xdg-thumbnail/README.md`
- Documentation index: `docs/README.md`
