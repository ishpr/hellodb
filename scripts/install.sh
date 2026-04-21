#!/usr/bin/env sh
# hellodb installer — fetches the latest release tarball for your platform,
# installs the binaries, bootstraps the encrypted DB, and (if Claude Code
# is on your PATH) registers the plugin automatically.
#
# One-liner:
#     curl -fsSL hellodb.dev/install | sh
#
# Respectful of /etc/profile.d/ and ~/.zshrc conventions: prefers
# /usr/local/bin if writable, falls back to ~/.local/bin and adds a
# line to your shell rc so the new command is on PATH immediately.
#
# Environment overrides:
#     HELLODB_VERSION       Pin to a tag (default: latest release)
#     HELLODB_INSTALL_DIR   Binary install dir (default: /usr/local/bin or ~/.local/bin)
#     HELLODB_REPO          Source repo (default: ishpr/hellodb)
#     HELLODB_HOME          Data dir (default: ~/.hellodb, passed through to hellodb)
#     HELLODB_SKIP_INIT     Set to 1 to skip `hellodb init` (no DB bootstrap)
#     HELLODB_SKIP_PLUGIN   Set to 1 to skip Claude Code plugin registration
#
# POSIX sh on purpose — works on macOS default /bin/sh (bash 3.2) and
# on minimal Alpine-style busybox sh. No bash-isms.

set -eu

REPO="${HELLODB_REPO:-ishpr/hellodb}"
VERSION="${HELLODB_VERSION:-latest}"

# Pretty printing
color() { printf "\033[%sm%s\033[0m" "$1" "$2"; }
info()  { printf "%s %s\n" "$(color '1;34' '==>')"  "$*"; }
ok()    { printf "%s %s\n" "$(color '1;32' '✓')"   "$*"; }
warn()  { printf "%s %s\n" "$(color '1;33' '!')"   "$*" >&2; }
err()   { printf "%s %s\n" "$(color '1;31' '✗')"   "$*" >&2; exit 1; }

# ----- platform detection -------------------------------------------------

OS="$(uname -s)"
ARCH="$(uname -m)"
case "$OS-$ARCH" in
  Darwin-arm64|Darwin-aarch64)   TARGET="aarch64-apple-darwin" ;;
  Darwin-x86_64)
    err "Intel Mac (x86_64) not supported by prebuilt tarballs — Apple Silicon only.
    build from source instead:
      git clone https://github.com/$REPO && cd hellodb && make build
    or run under Rosetta 2 if you have an aarch64 shell."
    ;;
  Linux-x86_64)                  TARGET="x86_64-unknown-linux-gnu" ;;
  Linux-aarch64|Linux-arm64)     TARGET="aarch64-unknown-linux-gnu" ;;
  *) err "unsupported platform: $OS-$ARCH. open an issue at https://github.com/$REPO/issues." ;;
esac
info "detected platform: $TARGET"

# ----- resolve version ----------------------------------------------------

API="https://api.github.com/repos/$REPO"
if [ "$VERSION" = "latest" ]; then
  info "resolving latest release from $API..."
  TAG=$(
    curl -fsSL "$API/releases/latest" \
      | grep -E '"tag_name"' \
      | head -1 \
      | sed -E 's/.*"tag_name": *"([^"]+)".*/\1/'
  )
  [ -n "$TAG" ] || err "couldn't resolve latest release tag. try again or set HELLODB_VERSION."
else
  TAG="$VERSION"
fi
ok "installing $TAG"

# ----- download + verify --------------------------------------------------

TARBALL="hellodb-plugin-$TARGET.tar.gz"
URL="https://github.com/$REPO/releases/download/$TAG/$TARBALL"
SHA_URL="${URL}.sha256"

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

info "downloading $TARBALL..."
curl -fsSL -o "$TMP/$TARBALL" "$URL" \
  || err "download failed. check $URL manually."

info "verifying checksum..."
if curl -fsSL -o "$TMP/$TARBALL.sha256" "$SHA_URL" 2>/dev/null; then
  (cd "$TMP" && shasum -a 256 -c "$TARBALL.sha256" >/dev/null 2>&1) \
    || err "SHA256 mismatch — tarball may be corrupt or tampered with. aborting."
  ok "checksum verified"
else
  warn "checksum file missing — continuing without verify (not ideal, please report)."
fi

# ----- extract ------------------------------------------------------------

info "extracting to $TMP/out/..."
mkdir -p "$TMP/out"
tar -xzf "$TMP/$TARBALL" -C "$TMP/out"

BIN_SRC="$TMP/out/plugin/bin"
[ -x "$BIN_SRC/hellodb" ] || err "tarball layout unexpected: no plugin/bin/hellodb"

# ----- install binaries ---------------------------------------------------

if [ -n "${HELLODB_INSTALL_DIR:-}" ]; then
  INSTALL_DIR="$HELLODB_INSTALL_DIR"
elif [ -w /usr/local/bin ] 2>/dev/null; then
  INSTALL_DIR="/usr/local/bin"
else
  INSTALL_DIR="$HOME/.local/bin"
fi
mkdir -p "$INSTALL_DIR"

info "installing binaries to $INSTALL_DIR/..."
for bin in hellodb hellodb-mcp hellodb-brain; do
  cp "$BIN_SRC/$bin" "$INSTALL_DIR/$bin"
  chmod +x "$INSTALL_DIR/$bin"
done
ok "installed: hellodb, hellodb-mcp, hellodb-brain"

# ----- install plugin payload ---------------------------------------------

# The plugin bundle ships alongside the binaries — copy it to ~/.hellodb/plugin
# so `claude plugin marketplace add` can point at it.
# The plugin bundle + the marketplace manifest both ship in the tarball.
# `claude plugin marketplace add <dir>` expects a directory containing
# `.claude-plugin/marketplace.json` at its root, so we lay out:
#
#     $INSTALL_ROOT/
#       plugin/             (the plugin itself)
#       .claude-plugin/
#         marketplace.json  (the manifest `claude plugin marketplace add` looks for)
INSTALL_ROOT="${HELLODB_HOME:-$HOME/.hellodb}"
PLUGIN_DEST="$INSTALL_ROOT/plugin"
MARKETPLACE_DEST="$INSTALL_ROOT/.claude-plugin"
mkdir -p "$INSTALL_ROOT"

if [ -d "$TMP/out/plugin" ]; then
  rm -rf "$PLUGIN_DEST"
  cp -R "$TMP/out/plugin" "$PLUGIN_DEST"
  # Bundle's plugin/bin/ needs the .exe/unix binaries the consumer will run.
  mkdir -p "$PLUGIN_DEST/bin"
  cp "$BIN_SRC/hellodb" "$BIN_SRC/hellodb-mcp" "$BIN_SRC/hellodb-brain" "$PLUGIN_DEST/bin/"
  chmod +x "$PLUGIN_DEST/bin/"*
fi

# Copy marketplace.json so `claude plugin marketplace add $INSTALL_ROOT` works.
# Without this, every fresh install fails at plugin registration.
if [ -d "$TMP/out/.claude-plugin" ]; then
  rm -rf "$MARKETPLACE_DEST"
  cp -R "$TMP/out/.claude-plugin" "$MARKETPLACE_DEST"
fi

# ----- PATH setup ---------------------------------------------------------

case ":$PATH:" in
  *":$INSTALL_DIR:"*) ok "$INSTALL_DIR already on PATH" ;;
  *)
    # Append a line to the user's shell rc. Choose the one matching $SHELL.
    RC=""
    SHELL_KIND=""
    case "${SHELL:-}" in
      */zsh)  RC="$HOME/.zshrc"                  ; SHELL_KIND="posix" ;;
      */bash) RC="$HOME/.bashrc"                 ; SHELL_KIND="posix" ;;
      */fish) RC="$HOME/.config/fish/config.fish"; SHELL_KIND="fish"  ;;
    esac
    if [ -n "$RC" ]; then
      # Anchor the idempotency check to the literal export line, not just the
      # install dir — a user may have `$INSTALL_DIR` mentioned in the rc for
      # an unrelated reason (history line, comment, another tool) without
      # actually exporting it onto PATH.
      MARK="# hellodb installer PATH"
      if ! grep -Fq "$MARK" "$RC" 2>/dev/null; then
        mkdir -p "$(dirname "$RC")"
        {
          echo ""
          echo "$MARK (added $(date +%Y-%m-%d))"
          if [ "$SHELL_KIND" = "fish" ]; then
            echo "set -gx PATH $INSTALL_DIR \$PATH"
          else
            echo "export PATH=\"$INSTALL_DIR:\$PATH\""
          fi
        } >> "$RC"
        ok "added $INSTALL_DIR to PATH via $RC (restart your shell or run: source $RC)"
      fi
    else
      warn "couldn't detect your shell rc; add this to it manually:"
      printf "    export PATH=\"%s:\$PATH\"\n" "$INSTALL_DIR" >&2
    fi
    ;;
esac

# ----- hellodb init -------------------------------------------------------

if [ "${HELLODB_SKIP_INIT:-0}" = "1" ]; then
  info "skipping hellodb init (HELLODB_SKIP_INIT=1)"
else
  info "bootstrapping hellodb..."
  "$INSTALL_DIR/hellodb" init >/dev/null
  ok "identity + encrypted DB + brain.toml written to ${HELLODB_HOME:-$HOME/.hellodb}/"
fi

# ----- Claude Code plugin registration ------------------------------------

if [ "${HELLODB_SKIP_PLUGIN:-0}" = "1" ]; then
  info "skipping Claude Code plugin registration (HELLODB_SKIP_PLUGIN=1)"
elif command -v claude >/dev/null 2>&1; then
  info "registering plugin with Claude Code..."
  # Tight match: marketplace list lines look like `  ❯ <name>\n    Source: ...`.
  # We want to match the exact marketplace name, not any substring that
  # happens to contain "hellodb" (e.g. a marketplace URL mentioning it).
  if claude plugin marketplace list 2>/dev/null | grep -Eq '^[[:space:]]*❯[[:space:]]+hellodb[[:space:]]*$'; then
    ok "marketplace 'hellodb' already registered"
  else
    claude plugin marketplace add "$INSTALL_ROOT" >/dev/null 2>&1 \
      && ok "marketplace added from $INSTALL_ROOT" \
      || warn "marketplace add failed — run manually: claude plugin marketplace add $INSTALL_ROOT"
  fi
  # Plugin list lines look like `  ❯ hellodb@hellodb\n    Version: ...`.
  if claude plugin list 2>/dev/null | grep -Eq '^[[:space:]]*❯[[:space:]]+hellodb@hellodb([[:space:]]|$)'; then
    ok "plugin already installed"
  else
    claude plugin install hellodb@hellodb >/dev/null 2>&1 \
      && ok "plugin installed" \
      || warn "plugin install failed — run manually: claude plugin install hellodb@hellodb"
  fi
else
  warn "Claude Code CLI not found; skipping plugin registration."
  warn "install Claude Code, then run: claude plugin install hellodb@hellodb"
fi

# ----- done ---------------------------------------------------------------

printf "\n"
printf "%s %s\n" "$(color '1;32' '✓')" "done."
printf "\n"
printf "next:\n"
printf "  1. open a new terminal (or: source your shell rc) so hellodb is on PATH\n"
printf "  2. restart Claude Code to pick up the plugin\n"
printf "  3. optional: enable Cloudflare embeddings + R2 sync\n"
printf "         hellodb                    # see subcommands\n"
printf "         git clone https://github.com/%s && cd hellodb && make setup-cloudflare\n" "$REPO"
printf "\n"
printf "docs: https://github.com/%s\n" "$REPO"
