# Agent Guidelines

- If `typos` reports a false positive for a valid term, update the `typos` configuration in `.typos.toml` instead of rewriting correct documentation or code just to satisfy the hook.

## Compatibility Policy

`xdg-thumbnail` is currently in pre-release. Backward compatibility for configurations, APIs, and internal formats should not be maintained unless explicitly requested.

## Documentation

- Keep architecture documents in `docs/architecture/`.
- Keep user-facing product and behavior specifications in `docs/spec/`, not internal implementation details.
- Write all documentation in a concise, technical style.
- Do not hard-wrap prose in Markdown files. Keep ordinary paragraphs and list items on a single source line unless Markdown syntax or structured blocks require line breaks.
- Use Mermaid for diagrams when diagrams are needed.

## Spec-Driven Workflow

- Follow Spec-driven development: document product behavior and externally visible behavior decisions in `docs/spec/`.
- Commit documentation updates before starting code changes for the same behavior change.

## Commits Guidelines

- Use Conventional Commits for all commit messages.
- Use a Conventional Commit scope when a clear scope exists.
