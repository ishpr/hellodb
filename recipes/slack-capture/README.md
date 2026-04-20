# Slack Capture

## What It Does

Adds a simple Slack capture path for `hellodb`: messages posted to a capture
channel are transformed into memory records through an MCP/CLI ingestion step.

## Prerequisites

- `hellodb` installed and initialized.
- A Slack app with bot token and event subscription for one channel.
- A small relay endpoint or worker that forwards message text to your local or
  remote `hellodb` MCP/CLI process.

## Steps

1. Create a private Slack channel (for example `#capture`) and note its
   channel ID.

2. Configure your relay with environment variables:

   ```sh
   export HELLODB_NAMESPACE=claude.episodes
   export HELLODB_SCHEMA=claude.episode
   export SLACK_CAPTURE_CHANNEL=C1234567890
   ```

3. On each Slack message event, call one of:

   - CLI path:

     ```sh
     hellodb note \
       --namespace "$HELLODB_NAMESPACE" \
       --topic "slack-capture" \
       --text "$MESSAGE_TEXT"
     ```

   - MCP path: invoke `hellodb_ingest_text` with source metadata.

4. Run digest to convert captured episodes into curated facts:

   ```sh
   hellodb brain --force
   ```

## Verification

- New records appear in `claude.episodes/main` after posting to Slack.
- `hellodb recall --top 5` includes facts derived from captured messages after
  a digest pass.

## Troubleshooting

- If messages are missing, confirm the channel ID matches your relay filter.
- If ingest fails, run `hellodb doctor` and check relay logs for payload shape.
