# Recipes

Recipes are reproducible, copy/paste automation playbooks for `hellodb`.
Each recipe must be runnable from a clean machine and include metadata for
search/indexing in docs and CI.

## Required Layout

Each recipe directory must include:

- `README.md` — operator guide with prerequisites and exact commands.
- `metadata.json` — structured metadata validated in CI.

Optional files:

- `migration.sql` — schema/data migration snippet.
- `verify.sh` — smoke-check script run after setup.

## Metadata Contract

`metadata.json` must contain:

- `id` (string, kebab-case)
- `title` (string)
- `summary` (string, <= 220 chars recommended)
- `category` (string, e.g. `import`, `capture`, `workflow`)
- `owner` (string)
- `last_updated` (ISO date, `YYYY-MM-DD`)
- `entrypoints` (array of strings, at least one)
- `requires` (array of strings, optional)

## Template

Start from `recipes/_template/`.

## Current Recipes

- `claude-memory-import` — normalize and ingest Claude memory markdown.
- `slack-capture` — capture Slack messages into hellodb via MCP/CLI bridge.
