#!/usr/bin/env bash
# Restore plugin/bin/ to dev-mode symlinks into target/release/. Use this
# after `make bundle` if you want to go back to dev iteration (symlinks
# auto-update when you `cargo build` — no re-bundling needed).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PLUGIN_BIN="$ROOT/plugin/bin"
mkdir -p "$PLUGIN_BIN"

for bin in hellodb hellodb-mcp hellodb-brain; do
  rm -f "$PLUGIN_BIN/$bin"
  ln -sf "../../target/release/$bin" "$PLUGIN_BIN/$bin"
done

echo "symlinked plugin/bin/ to target/release/:"
ls -la "$PLUGIN_BIN"
