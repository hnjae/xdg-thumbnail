# Agent Guidelines

## Tooling

- Run the full CI check with `devenv tasks run ci:check`.
- Format the repository with `devenv shell -- treefmt`.
- Run all pre-commit hooks with `devenv tasks run ci:git-hooks`.

## Compatibility Policy

`xdg-thumbnail` is currently in pre-release. Backward compatibility for configurations, APIs, and internal formats should not be maintained unless explicitly requested.

## Commits Guidelines

- Use a Conventional Commit scope when a clear scope exists.
