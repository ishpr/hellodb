#!/usr/bin/env bash
# Copy freshly-built release binaries into plugin/bin/, replacing the
# dev-mode symlinks with real files. Run after `cargo build --release`.
#
# Usage:
#   ./scripts/bundle-plugin.sh                      # uses target/release/
#   ./scripts/bundle-plugin.sh aarch64-apple-darwin # uses target/<triple>/release/
set -euo pipefail

TRIPLE="${1:-}"
if [[ -n "$TRIPLE" ]]; then
  SRC="target/${TRIPLE}/release"
else
  SRC="target/release"
fi

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PLUGIN_BIN="$ROOT/plugin/bin"

mkdir -p "$PLUGIN_BIN"

BINS=(hellodb hellodb-mcp hellodb-brain)
missing=()
for bin in "${BINS[@]}"; do
  if [[ ! -f "$ROOT/$SRC/$bin" ]]; then
    missing+=("$bin")
  fi
done
if [[ ${#missing[@]} -gt 0 ]]; then
  echo "error: missing release binaries in $SRC/: ${missing[*]}" >&2
  echo "       run: cargo build --release${TRIPLE:+ --target $TRIPLE}" >&2
  exit 1
fi

for bin in "${BINS[@]}"; do
  # Remove stale file OR symlink (dev mode uses symlinks into target/release/).
  rm -f "$PLUGIN_BIN/$bin"
  cp "$ROOT/$SRC/$bin" "$PLUGIN_BIN/$bin"
  chmod +x "$PLUGIN_BIN/$bin"
done

echo "bundled binaries from $SRC/ into plugin/bin/:"
ls -la "$PLUGIN_BIN"
