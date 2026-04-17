#!/usr/bin/env bash
# One-shot onboard for hellodb.
#
# What it automates, in order:
#   1. Rust toolchain — detects, prompts to install via rustup if missing.
#   2. cargo build --release (all binaries).
#   3. Bundle binaries into plugin/bin/.
#   4. Register the plugin with Claude Code (marketplace add + install).
#   5. hellodb init — generate identity.key, bootstrap SQLCipher DB, write brain.toml.
#   6. OPTIONAL: Cloudflare setup (wrangler OAuth browser flow) — prompts y/N.
#   7. Persist env vars to ~/.hellodb/env.sh and add a source line to the
#      user's shell rc (zsh or bash).
#
# What it cannot automate:
#   - The Cloudflare OAuth browser flow itself (step 6 — user must click "Authorize").
#   - Closing and re-opening Claude Code to pick up the new plugin/tools.
#
# Flags:
#   --yes             Assume "yes" for every prompt (e.g. install Rust, enable CF).
#   --no-cloudflare   Skip the Cloudflare setup step entirely (local-only install).
#   --no-claude       Skip the plugin-register step (build + init only).
#   --no-shellrc      Don't touch ~/.zshrc / ~/.bashrc; just write ~/.hellodb/env.sh.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

ASSUME_YES=0
SKIP_CF=0
SKIP_CLAUDE=0
SKIP_SHELLRC=0
for arg in "$@"; do
  case "$arg" in
    --yes|-y)      ASSUME_YES=1 ;;
    --no-cloudflare) SKIP_CF=1 ;;
    --no-claude)     SKIP_CLAUDE=1 ;;
    --no-shellrc)    SKIP_SHELLRC=1 ;;
    -h|--help)
      sed -n '2,27p' "$0"
      exit 0
      ;;
  esac
done

info()  { printf "\033[1;34m==>\033[0m %s\n" "$*"; }
ok()    { printf "\033[1;32m✓\033[0m %s\n" "$*"; }
warn()  { printf "\033[1;33m!\033[0m %s\n" "$*" >&2; }
err()   { printf "\033[1;31m✗\033[0m %s\n" "$*" >&2; }
ask() {
  # ask "prompt text" default(y|n); returns 0 for yes, 1 for no
  local prompt="$1" default="${2:-n}"
  if [[ "$ASSUME_YES" == "1" ]]; then return 0; fi
  local hint; [[ "$default" == "y" ]] && hint="[Y/n]" || hint="[y/N]"
  read -rp "$(printf '\033[1;36m?\033[0m %s %s ' "$prompt" "$hint")" ans
  ans="${ans:-$default}"
  [[ "$ans" =~ ^[Yy]$ ]]
}

# ----- 1. Rust ------------------------------------------------------------

if ! command -v cargo >/dev/null 2>&1; then
  if [[ -f "$HOME/.cargo/env" ]]; then
    # shellcheck disable=SC1091
    source "$HOME/.cargo/env"
  fi
fi
if ! command -v cargo >/dev/null 2>&1; then
  warn "Rust toolchain (cargo) not found."
  if ask "Install Rust now via rustup? (one-time, writes to ~/.cargo and ~/.rustup)" y; then
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable --profile minimal
    # shellcheck disable=SC1091
    source "$HOME/.cargo/env"
    ok "Rust installed."
  else
    err "Rust is required. Install from https://rustup.rs and re-run."
    exit 1
  fi
else
  ok "Rust found: $(cargo --version)"
fi

# ----- 2. cargo build --release ------------------------------------------

info "building release binaries (cargo build --release)..."
cargo build --release
ok "built hellodb / hellodb-mcp / hellodb-brain."

# ----- 3. bundle plugin binaries -----------------------------------------

info "bundling binaries into plugin/bin/..."
./scripts/bundle-plugin.sh >/dev/null
ok "plugin bundled."

# ----- 4. register plugin with Claude Code -------------------------------

if [[ "$SKIP_CLAUDE" == "1" ]]; then
  info "skipping Claude Code plugin registration (--no-claude)"
elif ! command -v claude >/dev/null 2>&1; then
  warn "claude CLI not found on PATH — skipping plugin registration."
  warn "install Claude Code, then run:   make install"
else
  info "registering plugin with Claude Code (user scope)..."
  # Anchor presence checks to the exact list-line shape so we don't
  # false-positive on any substring containing "hellodb".
  claude plugin marketplace list 2>/dev/null | grep -Eq '^[[:space:]]*❯[[:space:]]+hellodb[[:space:]]*$' \
    || claude plugin marketplace add "$ROOT" >/dev/null 2>&1 \
    || warn "marketplace add failed (may already exist)"
  claude plugin list 2>/dev/null | grep -Eq '^[[:space:]]*❯[[:space:]]+hellodb@hellodb([[:space:]]|$)' \
    || claude plugin install hellodb@hellodb >/dev/null 2>&1 \
    || warn "plugin install failed (may already be installed)"
  ok "plugin registered."
fi

# ----- 5. hellodb init ---------------------------------------------------

info "bootstrapping hellodb data dir (~/.hellodb)..."
./target/release/hellodb init >/dev/null
ok "identity + brain.toml + encrypted DB ready at \$HELLODB_HOME (default ~/.hellodb)."

# ----- 6. Cloudflare setup (optional) ------------------------------------

CF_ENABLED=0
if [[ "$SKIP_CF" == "1" ]]; then
  info "skipping Cloudflare setup (--no-cloudflare). Run 'make setup-cloudflare' later to enable."
elif ask "Enable Cloudflare (Workers AI embeddings + R2 sync, free tier)?" n; then
  ./scripts/setup-cloudflare.sh
  CF_ENABLED=1
  ok "Cloudflare gateway deployed."
else
  info "skipping Cloudflare for now. Run 'make setup-cloudflare' anytime to enable."
fi

# ----- 7. persist env vars -----------------------------------------------

ENV_FILE="$HOME/.hellodb/env.sh"
mkdir -p "$(dirname "$ENV_FILE")"
info "writing $ENV_FILE..."
{
  echo "# hellodb shell env — sourced on shell startup."
  echo "# Regenerate by re-running scripts/onboard.sh."
  echo ""
  if [[ "$CF_ENABLED" == "1" ]]; then
    # `setup-cloudflare.sh` prints a "Add these to your shell" block with the
    # concrete worker URL, and also writes a marker file we can read here.
    WORKER_URL=""
    if [[ -f "$HOME/.hellodb/cloudflare.gateway.url" ]]; then
      WORKER_URL=$(cat "$HOME/.hellodb/cloudflare.gateway.url" 2>/dev/null || true)
    fi
    # If the user exported it in their current shell session, that wins.
    WORKER_URL="${HELLODB_EMBED_GATEWAY_URL:-$WORKER_URL}"

    echo "export HELLODB_EMBED_BACKEND=cloudflare"
    if [[ -n "$WORKER_URL" ]]; then
      echo "export HELLODB_EMBED_GATEWAY_URL=$WORKER_URL"
    else
      echo "# export HELLODB_EMBED_GATEWAY_URL=<your worker URL>"
      echo "# (scroll up to the 'make setup-cloudflare' output — the worker URL"
      echo "#  was printed there; drop it in above and re-source this file)"
    fi
    if [[ "$(uname -s)" == "Darwin" ]]; then
      echo 'export HELLODB_EMBED_GATEWAY_TOKEN=$(security find-generic-password -a "$USER" -s hellodb-gateway-token -w 2>/dev/null)'
    else
      echo 'export HELLODB_EMBED_GATEWAY_TOKEN=$(cat ~/.hellodb/gateway.token 2>/dev/null)'
    fi
  else
    echo "# (Cloudflare not enabled — run 'make setup-cloudflare' to add these later)"
    echo "# export HELLODB_EMBED_BACKEND=cloudflare"
    echo "# export HELLODB_EMBED_GATEWAY_URL=https://your-worker.workers.dev"
    echo "# export HELLODB_EMBED_GATEWAY_TOKEN=..."
  fi
} > "$ENV_FILE"
chmod 600 "$ENV_FILE"
ok "wrote $ENV_FILE (mode 0600)"

if [[ "$SKIP_SHELLRC" == "0" ]]; then
  # Pick the shell rc file — prefer zsh since it's macOS default.
  RC_FILE=""
  if [[ -n "${ZSH_VERSION:-}" ]] || [[ "${SHELL:-}" == *"zsh"* ]]; then
    RC_FILE="$HOME/.zshrc"
  elif [[ -n "${BASH_VERSION:-}" ]] || [[ "${SHELL:-}" == *"bash"* ]]; then
    RC_FILE="$HOME/.bashrc"
  fi
  if [[ -n "$RC_FILE" ]] && ! grep -q "source.*\.hellodb/env\.sh" "$RC_FILE" 2>/dev/null; then
    if ask "Add 'source ~/.hellodb/env.sh' to $RC_FILE?" y; then
      {
        echo ""
        echo "# hellodb — added by scripts/onboard.sh"
        echo "[ -f \"\$HOME/.hellodb/env.sh\" ] && source \"\$HOME/.hellodb/env.sh\""
      } >> "$RC_FILE"
      ok "appended source line to $RC_FILE"
    fi
  fi
fi

# ----- done --------------------------------------------------------------

echo ""
echo "─────────────────────────────────────────────────────────────────"
echo " done."
echo "─────────────────────────────────────────────────────────────────"
echo ""
echo "next:"
echo "  - open a NEW shell (or 'source ~/.hellodb/env.sh')"
echo "  - restart Claude Code so it picks up the plugin"
echo "  - try: claude → type '/hellodb:review' or just have a conversation"
echo ""
if [[ "$CF_ENABLED" != "1" ]]; then
  echo "  (Cloudflare not enabled yet — 'make setup-cloudflare' whenever you want semantic recall)"
  echo ""
fi
