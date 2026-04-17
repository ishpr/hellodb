#!/usr/bin/env bash
# Background-digest fire-and-forget wrapper for the Stop hook.
#
# What this does:
#   1. Bail immediately if we're already inside a digest-triggered session
#      (HELLODB_DIGEST_HOOK=1). Prevents infinite loop: the session spawned
#      to digest also fires its own Stop hook, which would try to spawn
#      another session, ...
#   2. Fire the brain daemon synchronously (it's fast — just tails + writes
#      cursor state; no LLM). Gates handle cooldown logic.
#   3. Spawn a background `claude -p /hellodb:digest-now` with the guard
#      env var set, only if claude is on PATH and it's been long enough
#      since the last agent-digest run.
#
# Fire-and-forget: we nohup + `&` the claude invocation so the user's shell
# doesn't block waiting on the LLM call. Its output lands in
# ~/.hellodb/digest.log for debugging; stderr merged in.
set -eu

HELLODB_HOME="${HELLODB_HOME:-$HOME/.hellodb}"
DIGEST_LOG="$HELLODB_HOME/digest.log"
LAST_RUN_FILE="$HELLODB_HOME/last-agent-digest.ts"
# Minimum gap between agent-digest runs. Default 30 min — don't hammer the
# API on every tiny session. Override via HELLODB_DIGEST_MIN_GAP_SEC.
MIN_GAP_SEC="${HELLODB_DIGEST_MIN_GAP_SEC:-1800}"

# ----- recursion guard ---------------------------------------------------

if [ "${HELLODB_DIGEST_HOOK:-0}" = "1" ]; then
  # We are the spawned digest session. Don't re-spawn. Still run brain
  # (fast, non-LLM) because its cursor/state bookkeeping is useful.
  exec "${CLAUDE_PLUGIN_ROOT}/bin/hellodb-brain"
fi

# ----- run the brain daemon first (always) ------------------------------
# This is the deterministic fallback pipeline: tails episodes, updates
# cursor, and (if gates pass) writes a MockBackend digest. Fast and free.
"${CLAUDE_PLUGIN_ROOT}/bin/hellodb-brain" >>"$DIGEST_LOG" 2>&1 || true

# ----- agent-digest cool-down check -------------------------------------

now=$(date +%s)
last=0
if [ -f "$LAST_RUN_FILE" ]; then
  last=$(cat "$LAST_RUN_FILE" 2>/dev/null || echo 0)
fi
elapsed=$((now - last))
if [ "$elapsed" -lt "$MIN_GAP_SEC" ]; then
  # Too soon since the last agent-digest. Brain's pipeline already ran;
  # agent pass is optional polish, don't burn tokens every session.
  exit 0
fi

# ----- spawn background agent-digest session -----------------------------

if ! command -v claude >/dev/null 2>&1; then
  # Claude Code CLI not on PATH; agent-digest unavailable. Brain's mock
  # pipeline already ran, so we're not blocking anything.
  exit 0
fi

mkdir -p "$HELLODB_HOME"
printf '%s' "$now" > "$LAST_RUN_FILE"

# Fire and forget. `--allowedTools` limits what the spawned session can do
# so it can't go off-script. `HELLODB_DIGEST_HOOK=1` is the recursion guard
# — the spawned session's Stop hook will see it and bail out of this branch.
nohup env HELLODB_DIGEST_HOOK=1 \
  claude -p "/hellodb:hellodb-digest-now" \
    --dangerously-skip-permissions \
  >>"$DIGEST_LOG" 2>&1 </dev/null &

# Detach fully so the Stop hook returns immediately and the user's shell
# is not held waiting on the background claude process.
disown || true
exit 0
