#!/usr/bin/env bash
# Background-digest fire-and-forget wrapper for the Stop hook.
#
# What this does:
#   1. Bail immediately if we're already inside a digest-triggered session
#      (HELLODB_DIGEST_HOOK=1). Prevents infinite loop: the session spawned
#      to digest also fires its own Stop hook, which would try to spawn
#      another session, ad infinitum.
#   2. Fire the brain daemon synchronously (it's fast — just tails + writes
#      cursor state; no LLM). Gates handle cooldown logic.
#   3. Spawn a background `claude -p /hellodb:digest-now` with the guard
#      env var set, only if claude is on PATH and it's been long enough
#      since the last agent-digest run.
#
# Fire-and-forget: we nohup + `&` + `disown` the claude invocation so the
# user's shell doesn't block waiting on the LLM call. Its output lands in
# ~/.hellodb/digest.log for debugging; stderr merged in.
#
# This script must NEVER exit non-zero on a normal path — Claude Code hooks
# surface failures visibly to the user, and a broken digest shouldn't turn
# every session end into an error message. We `|| true` every external call
# and only exit 0.
set -u

HELLODB_HOME="${HELLODB_HOME:-$HOME/.hellodb}"
DIGEST_LOG="$HELLODB_HOME/digest.log"
LAST_RUN_FILE="$HELLODB_HOME/last-agent-digest.ts"
# Minimum gap between agent-digest runs. Default 30 min — don't hammer the
# API on every tiny session. Override via HELLODB_DIGEST_MIN_GAP_SEC.
MIN_GAP_SEC="${HELLODB_DIGEST_MIN_GAP_SEC:-1800}"

# Resolve the brain binary. Prefer the plugin's bundled copy (set by Claude
# Code when the plugin fires the hook); fall back to $PATH so this script
# still works when invoked outside the plugin lifecycle (e.g. manual testing).
if [ -n "${CLAUDE_PLUGIN_ROOT:-}" ] && [ -x "${CLAUDE_PLUGIN_ROOT}/bin/hellodb-brain" ]; then
  BRAIN="${CLAUDE_PLUGIN_ROOT}/bin/hellodb-brain"
else
  BRAIN="$(command -v hellodb-brain 2>/dev/null || true)"
fi

mkdir -p "$HELLODB_HOME" 2>/dev/null || true

# ----- recursion guard ---------------------------------------------------

if [ "${HELLODB_DIGEST_HOOK:-0}" = "1" ]; then
  # We are the spawned digest session. Don't re-spawn another one. Still run
  # brain if we can find it (cursor/state bookkeeping is cheap and useful),
  # but be fully silent on any failure — this path fires from inside a
  # non-interactive claude -p that's about to exit.
  if [ -n "$BRAIN" ] && [ -x "$BRAIN" ]; then
    "$BRAIN" >>"$DIGEST_LOG" 2>&1 || true
  fi
  exit 0
fi

# ----- run the brain daemon first (always) ------------------------------
# This is the deterministic fallback pipeline: tails episodes, updates
# cursor, and (if gates pass) writes a MockBackend digest. Fast and free.
if [ -n "$BRAIN" ] && [ -x "$BRAIN" ]; then
  "$BRAIN" >>"$DIGEST_LOG" 2>&1 || true
else
  # No brain binary found — log and continue; we still try the agent-digest
  # spawn below, which is independent of brain.
  printf '[%s] run-digest-async: brain binary not found; skipping mock pass\n' \
    "$(date -u +%Y-%m-%dT%H:%M:%SZ)" >>"$DIGEST_LOG" 2>&1 || true
fi

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

printf '%s' "$now" > "$LAST_RUN_FILE" 2>/dev/null || true

# Fire and forget. `HELLODB_DIGEST_HOOK=1` is the recursion guard — the
# spawned session's Stop hook will see it and bail out of this branch
# instead of spawning another `claude -p`.
#
# We use `--dangerously-skip-permissions` because the spawned session needs
# to call the hellodb MCP tools without a human approving each one (it's
# running headless). The skill itself is the enforcement boundary — its
# prompt restricts which tools to call and how. A tighter `--allowedTools`
# list would be preferable but the full set for digest spans ~8 tools
# across tailing, querying, branching, remembering, and archiving; a
# permission allowlist would need to stay in sync with the skill's
# behavior, which is brittle. Revisit when Claude Code ships per-skill
# permission scoping.
nohup env HELLODB_DIGEST_HOOK=1 \
  claude -p "/hellodb:digest-now" \
    --dangerously-skip-permissions \
  >>"$DIGEST_LOG" 2>&1 </dev/null &

# Detach fully so the Stop hook returns immediately and the user's shell
# is not held waiting on the background claude process.
disown 2>/dev/null || true
exit 0
