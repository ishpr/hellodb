# hellodb — build + plugin packaging
#
# Common workflow:
#     make                    # cargo build --release (all binaries)
#     make test               # cargo test --workspace
#     make bundle             # copy release binaries into plugin/bin/
#     make install            # bundle + install plugin into Claude Code
#     make dev                # symlink plugin/bin/ → target/release/ (dev mode)
#     make uninstall          # remove plugin + marketplace from Claude Code
#     make doctor             # run `hellodb doctor`
#     make clean              # cargo clean

CARGO           := cargo
RELEASE_DIR     := target/release
PLUGIN_DIR      := plugin
REPO_ROOT       := $(shell pwd)

.PHONY: help all build test fmt lint check bundle dev install uninstall doctor status clean setup-cloudflare rotate-gateway-token

help:
	@echo "hellodb make targets:"
	@echo "  build               cargo build --release (all binaries)"
	@echo "  test                cargo test --workspace"
	@echo "  fmt                 cargo fmt --all"
	@echo "  lint                cargo clippy --workspace -- -D warnings"
	@echo "  check               fmt-check + clippy + test (same as CI)"
	@echo "  bundle              copy release binaries into plugin/bin/"
	@echo "  dev                 symlink plugin/bin/ → target/release/ (iteration mode)"
	@echo "  install             bundle + register plugin with Claude Code (user scope)"
	@echo "  uninstall           remove plugin + marketplace from Claude Code"
	@echo "  doctor              run ./target/release/hellodb doctor"
	@echo "  status              run ./target/release/hellodb status"
	@echo ""
	@echo "  setup-cloudflare    one-command Cloudflare setup via wrangler OAuth"
	@echo "                       (browser login, creates R2 bucket, deploys Worker)"
	@echo "  rotate-gateway-token invalidate current bearer and issue a new one"
	@echo ""
	@echo "  clean               cargo clean"

all: build

build:
	$(CARGO) build --release

test:
	$(CARGO) test --workspace

fmt:
	$(CARGO) fmt --all

lint:
	$(CARGO) clippy --workspace --all-targets -- -D warnings

check:
	$(CARGO) fmt --all --check
	$(CARGO) clippy --workspace --all-targets -- -D warnings
	$(CARGO) test --workspace

bundle: build
	@./scripts/bundle-plugin.sh

dev: build
	@./scripts/dev-symlink-plugin.sh

install: bundle
	@claude plugin marketplace list 2>/dev/null | grep -q "hellodb" \
	  || claude plugin marketplace add $(REPO_ROOT)
	@claude plugin list 2>/dev/null | grep -q "hellodb@hellodb" \
	  || claude plugin install hellodb@hellodb

uninstall:
	@claude plugin uninstall hellodb@hellodb 2>/dev/null || true
	@claude plugin marketplace remove hellodb 2>/dev/null || true

doctor: build
	./$(RELEASE_DIR)/hellodb doctor

status: build
	./$(RELEASE_DIR)/hellodb status

setup-cloudflare:
	@./scripts/setup-cloudflare.sh

rotate-gateway-token:
	@./scripts/setup-cloudflare.sh --rotate

clean:
	$(CARGO) clean
