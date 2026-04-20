#!/usr/bin/env bash
set -euo pipefail

if ! command -v hellodb >/dev/null 2>&1; then
  echo "hellodb not found on PATH" >&2
  exit 1
fi

hellodb status >/dev/null
echo "ok: hellodb is available and initialized"
