# Remote MCP Bridge

## What It Does

Provides bridge patterns for tools that only support local stdio MCP configs
but need to connect to a remote HTTP MCP endpoint.

## Bridge Options

- `supergateway` (recommended for streamable HTTP MCP)
- `mcp-remote` (works, may require longer startup timeout)

## Example (supergateway)

```json
{
  "mcpServers": {
    "hellodb-remote": {
      "command": "npx",
      "args": [
        "-y",
        "supergateway",
        "--streamableHttp",
        "https://example.com/hellodb-mcp"
      ]
    }
  }
}
```

## Example (mcp-remote with header)

```json
{
  "mcpServers": {
    "hellodb-remote": {
      "command": "npx",
      "args": [
        "-y",
        "mcp-remote",
        "https://example.com/hellodb-mcp",
        "--header",
        "x-hellodb-key:${HELLODB_MCP_KEY}"
      ],
      "env": {
        "HELLODB_MCP_KEY": "replace-me"
      }
    }
  }
}
```

## Verification

- MCP client starts without timeout.
- `tools/list` returns `hellodb_*` tools from the remote endpoint.
