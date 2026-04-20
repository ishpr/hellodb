# Slack Capture Integration

## What It Does

Defines a production integration shape for routing Slack channel events into
`hellodb` episode ingestion.

This integration is implementation-oriented. For step-by-step operator setup,
see `recipes/slack-capture`.

## Architecture

1. Slack Events API receives messages for the configured channel.
2. Relay validates source and channel ID.
3. Relay forwards normalized text + metadata to `hellodb` (`hellodb note` or
   MCP `hellodb_ingest_text`).
4. `hellodb-brain` digests episodes into curated facts.

## Minimal Event Payload Contract

```json
{
  "source": "slack",
  "channel_id": "C1234567890",
  "user_id": "U1234567890",
  "text": "Captured message text",
  "timestamp": "1713371000.000200"
}
```

## Security Notes

- Store Slack bot tokens in secret managers, not in repo files.
- Apply channel allow-list filters before ingestion.
- Redact obvious secrets before writing captured text to memory.
