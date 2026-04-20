# Integrations

Integrations are runnable connection guides between `hellodb` and external
systems (chat channels, remote MCP bridges, hosted runtimes).

## Required Layout

Each integration directory must include:

- `README.md`
- `metadata.json`

Optional files:

- deployment manifests
- helper scripts
- example configs

## Current Integrations

- `slack-capture` — Slack channel to hellodb ingestion path.
- `remote-mcp-bridge` — expose hellodb MCP over remote HTTP bridges for tools
  that only support local stdio MCP declarations.
