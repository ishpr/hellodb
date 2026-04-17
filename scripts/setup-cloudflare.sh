#!/usr/bin/env bash
# Zero-token Cloudflare setup for hellodb.
#
# What this does, in order:
#   1. Ensure wrangler is available (installs via npm if needed).
#   2. Run `wrangler login` — opens a browser, you authorize hellodb's
#      Cloudflare integration. The token is stored in your OS keychain
#      by wrangler; hellodb never sees it.
#   3. Create the R2 bucket (idempotent).
#   4. Generate a fresh bearer (GATEWAY_TOKEN) and set it as a Worker secret.
#   5. Deploy the gateway Worker.
#   6. Print your local env lines to copy into ~/.zshrc (or a .env file).
#
# You can re-run this. Steps 3–5 are idempotent. Step 4 generates a NEW
# bearer if you pass --rotate, which invalidates all prior hellodb clients.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
GATEWAY_DIR="$ROOT/gateway"
BUCKET="${HELLODB_R2_BUCKET:-hellodb}"
WORKER_NAME="${HELLODB_WORKER_NAME:-hellodb-gateway}"

ROTATE=0
for arg in "$@"; do
  case "$arg" in
    --rotate) ROTATE=1 ;;
    -h|--help)
      sed -n '2,20p' "$0"
      exit 0
      ;;
  esac
done

info()  { printf "\033[1;34m==>\033[0m %s\n" "$*"; }
ok()    { printf "\033[1;32m✓\033[0m %s\n" "$*"; }
warn()  { printf "\033[1;33m!\033[0m %s\n" "$*" >&2; }
err()   { printf "\033[1;31m✗\033[0m %s\n" "$*" >&2; }

# ----- 1. wrangler availability -------------------------------------------

if ! command -v npx >/dev/null 2>&1; then
  err "npx not found. Install Node.js (18+) first: https://nodejs.org/"
  exit 1
fi

if [[ ! -d "$GATEWAY_DIR/node_modules" ]]; then
  info "installing gateway npm deps..."
  (cd "$GATEWAY_DIR" && npm ci --prefer-offline --no-audit --no-fund)
fi
WRANGLER="npx --prefix $GATEWAY_DIR wrangler"

# ----- 2. wrangler login (OAuth) ------------------------------------------
#
# Skip if CLOUDFLARE_API_TOKEN is already set — that means you've opted for
# token-based auth explicitly (CI, a shared service account, etc.).

if [[ -n "${CLOUDFLARE_API_TOKEN:-}" ]]; then
  info "CLOUDFLARE_API_TOKEN is set; skipping wrangler login."
else
  info "running wrangler login (a browser will open)..."
  info "  grant access to Workers, R2, and Workers AI when prompted."
  (cd "$GATEWAY_DIR" && $WRANGLER login) || {
    err "wrangler login failed or was cancelled."
    exit 1
  }
  ok "wrangler login complete."
fi

# Best-effort identity check. On --rotate we also need this to verify the
# current wrangler session owns the bucket/worker we're about to touch.
info "verifying account access..."
ACCOUNT_LINE=$( (cd "$GATEWAY_DIR" && $WRANGLER whoami 2>&1) | grep -E "Account ID" -A 2 | tail -n 1 || true )
ok "whoami: $ACCOUNT_LINE"

# ----- 3. R2 bucket -------------------------------------------------------

info "creating R2 bucket '$BUCKET' (idempotent)..."
if (cd "$GATEWAY_DIR" && echo y | $WRANGLER r2 bucket create "$BUCKET") 2>&1 | grep -qi "already exists"; then
  ok "R2 bucket '$BUCKET' already exists."
else
  ok "R2 bucket '$BUCKET' ready."
fi

# ----- 4. GATEWAY_TOKEN ---------------------------------------------------

GEN_TOKEN=""
if [[ "$ROTATE" -eq 1 ]]; then
  info "rotating GATEWAY_TOKEN (old clients will stop working)..."
  GEN_TOKEN=1
else
  # Heuristic: if the Worker doesn't exist yet, we need a fresh token.
  # wrangler will create the worker implicitly on `secret put`, so we can't
  # really check beforehand. We generate a fresh token only on first run by
  # looking for a stashed fingerprint locally.
  FINGERPRINT_FILE="$HOME/.hellodb/gateway-token-fingerprint"
  if [[ ! -f "$FINGERPRINT_FILE" ]]; then
    info "no existing gateway token fingerprint; generating one..."
    GEN_TOKEN=1
  else
    ok "existing gateway token detected; reusing. (pass --rotate to replace.)"
  fi
fi

if [[ "$GEN_TOKEN" == "1" ]]; then
  TOKEN=$(openssl rand -hex 32)
  info "setting GATEWAY_TOKEN secret on Worker '$WORKER_NAME'..."
  (cd "$GATEWAY_DIR" && printf '%s' "$TOKEN" | $WRANGLER secret put GATEWAY_TOKEN --name "$WORKER_NAME") >/dev/null
  mkdir -p "$(dirname "$HOME/.hellodb/gateway-token-fingerprint")"
  printf '%s\n' "$TOKEN" | shasum -a 256 | awk '{print $1}' > "$HOME/.hellodb/gateway-token-fingerprint"

  # Store the full token in the OS keychain on macOS; on Linux, write to a
  # 0600 file under $HELLODB_HOME. We never put secrets in the repo.
  if [[ "$(uname -s)" == "Darwin" ]]; then
    security add-generic-password -U -a "$USER" -s "hellodb-gateway-token" -w "$TOKEN" 2>/dev/null \
      && ok "GATEWAY_TOKEN saved to macOS Keychain (service: hellodb-gateway-token)"
  else
    TOKEN_FILE="$HOME/.hellodb/gateway.token"
    umask 077
    printf '%s' "$TOKEN" > "$TOKEN_FILE"
    ok "GATEWAY_TOKEN saved to $TOKEN_FILE (mode 0600)"
  fi
  ok "new bearer generated and uploaded."
  echo ""
  echo "Your new GATEWAY_TOKEN (copy before scrolling past):"
  echo "$TOKEN"
  echo ""
fi

# ----- 5. deploy the worker -----------------------------------------------

info "deploying gateway Worker..."
DEPLOY_OUT=$( (cd "$GATEWAY_DIR" && $WRANGLER deploy --name "$WORKER_NAME" 2>&1) ) || {
  err "deploy failed:"
  printf '%s\n' "$DEPLOY_OUT" >&2
  exit 1
}

WORKER_URL=$(printf '%s\n' "$DEPLOY_OUT" | grep -oE 'https://[^ ]+\.workers\.dev' | head -1 || true)
if [[ -z "$WORKER_URL" ]]; then
  warn "couldn't parse Worker URL from deploy output; inspect manually:"
  printf '%s\n' "$DEPLOY_OUT" | tail -20
else
  ok "deployed: $WORKER_URL"
  # Stash the URL so `onboard.sh` (or anything else) can pick it up without
  # having to re-parse `wrangler deploy` output. Gitignored via $HOME scope.
  mkdir -p "$HOME/.hellodb"
  printf '%s\n' "$WORKER_URL" > "$HOME/.hellodb/cloudflare.gateway.url"
fi

# ----- 6. print config lines ---------------------------------------------

echo ""
echo "─────────────────────────────────────────────────────────────────"
echo "Add these to your shell (e.g. ~/.zshrc) or a .env you source:"
echo "─────────────────────────────────────────────────────────────────"
echo "  export HELLODB_EMBED_BACKEND=cloudflare"
if [[ -n "$WORKER_URL" ]]; then
  echo "  export HELLODB_EMBED_GATEWAY_URL=$WORKER_URL"
fi
if [[ "$(uname -s)" == "Darwin" ]]; then
  echo '  export HELLODB_EMBED_GATEWAY_TOKEN=$(security find-generic-password -a "$USER" -s hellodb-gateway-token -w)'
else
  echo '  export HELLODB_EMBED_GATEWAY_TOKEN=$(cat ~/.hellodb/gateway.token)'
fi
echo ""
echo "Optional — if you enable Cloudflare Access on the Worker:"
echo "  export HELLODB_EMBED_CF_ACCESS_CLIENT_ID=<your-service-token-client-id>"
echo "  export HELLODB_EMBED_CF_ACCESS_CLIENT_SECRET=<your-service-token-client-secret>"
echo "─────────────────────────────────────────────────────────────────"
echo ""
ok "done. try: hellodb status   (should list your identity + namespaces)"
