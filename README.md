# hellodb

**Sovereign, encrypted, branchable memory for agents.**

**Current release:** `v0.1.1` (see [GitHub Releases](https://github.com/ishpr/hellodb/releases)).

A local-first database for agent memory that you own: SQLCipher-encrypted at
rest, content-addressed + signed records, git-style branches, and a passive
digest pipeline that turns raw session episodes into curated, decay-ranked
facts. Optional Cloudflare backing (Workers AI embeddings, R2 sync) through
your own account — zero shared service, zero affiliate middleman.

**`hellodb-mcp`** is a **stdio MCP server** (newline-delimited JSON-RPC, MCP
`2024-11-05`). Any agent host or IDE that supports **MCP** and can spawn a
local process can attach to the same tools—not only Claude. The Claude Code
plugin below is the fastest path if you already use Claude; everyone else
points their MCP config at `hellodb-mcp` (see [Connector snippets](#connector-snippets))
or at a remote URL plus a small bridge.

Ships as a Claude Code plugin with:
- **5 skills** that Claude triggers automatically (`/hellodb:memorize`,
  `/hellodb:recall`, `/hellodb:review`, `/hellodb:digest-now`,
  `/hellodb:consolidate-now`)
- **2 plugin agents** (`memory-digest`, `memory-consolidate`) with prompts
  tuned for fast, low-cost digestion/consolidation (digest backend is
  pluggable: `mock` locally, `openrouter` when configured)
- **Stop hook** that fires the digest pipeline in the background after
  every session, idempotent, cool-down-gated
- **22 MCP tools** exposing every primitive (namespaces, schemas,
  records, branches, merge, tail cursor, reinforcement metadata, vector
  upsert/recall, embed, ingest, and `hellodb_find_relevant_memories` —
  Claude Code-shaped retrieval with graceful fallback from semantic to
  keyword ranking). Current MCP server auth model is local single-identity
  execution; multi-principal consent/delegation flows remain library-level
  primitives.

---

## Quick start

**macOS / Linux:**

```sh
curl -fsSL hellodb.dev/install | sh
```

**Windows (PowerShell 5.1+ or PowerShell Core):**

```powershell
iwr -useb hellodb.dev/install.ps1 | iex
```

> **Intel Mac (`x86_64-apple-darwin`) note:** prebuilt tarballs are
> Apple Silicon only — Apple has shipped 100% of new Macs on ARM since
> late 2020 and the Intel matrix slot was blocking releases on scarce
> runners. Intel users can either **build from source** (`git clone … &&
> make build`) or run the install under Rosetta 2 from an `aarch64`
> shell. The installer prints these options if it detects `x86_64`.

That's it. The installer:

1. Detects your platform and downloads the matching release tarball
2. Verifies SHA256
3. Installs `hellodb`, `hellodb-mcp`, and `hellodb-brain` into `/usr/local/bin`
   (or `~/.local/bin` / `%USERPROFILE%\.hellodb\bin` as a fallback) and
   adds it to PATH
4. Runs `hellodb init` to generate your identity key + encrypted DB at
   `~/.hellodb/` (mode 0600 on the key)
5. Registers the plugin with Claude Code if `claude` is on your PATH

Restart Claude Code once after install. The plugin's skills become visible
to the session automatically.

### First session

Confirm everything works and pull in any existing Claude Code memory:

```sh
hellodb status                      # identity fingerprint + data_dir + namespace list
hellodb ingest --from-claudemd      # import ~/.claude/projects/*/memory/*.md into hellodb
hellodb recall --top 8              # show top-ranked facts (decay-adjusted)
```

`ingest --from-claudemd` is idempotent — re-running on unchanged files is a
no-op (content-addressing dedupes). Each Claude Code project lands in its own
namespace (`claude.memory.<project-slug>`), so cross-project memory never
leaks between repos.

Inside Claude Code, the plugin's skills (`/hellodb:memorize`,
`/hellodb:recall`, `/hellodb:review`) and the MCP tool
`hellodb_find_relevant_memories` wire up automatically once the session
restarts.

### Optional: Cloudflare-backed semantic search

Semantic recall (`/hellodb:recall`, `hellodb_embed_and_search`) needs an
embedding backend. Cloudflare's Workers AI has a generous free tier
(~90K embeddings/day on `bge-small-en-v1.5`) and R2 for encrypted delta
sync (10 GB + zero egress).

```sh
git clone https://github.com/ishpr/hellodb
cd hellodb
make setup-cloudflare   # wrangler OAuth, deploys a gateway Worker to your account
```

This writes the env vars to `~/.hellodb/env.sh` and (with confirmation) a
source line to your `~/.zshrc`. One OAuth click; your CF account owns
everything — we never see your key.

Without this step, hellodb still works as a local encrypted store with
filter-based (non-semantic) recall.

---

## Dual-path onboarding

Pick one path per machine:

### Path A: local-first (default)

1. Install:

```sh
curl -fsSL hellodb.dev/install | sh
```

2. Initialize and verify:

```sh
hellodb status
hellodb ingest --from-claudemd
hellodb recall --top 8
```

### Path B: hosted-optional add-ons

Use this when you want managed embeddings/sync in your own cloud account.

1. Deploy Cloudflare gateway:

```sh
make setup-cloudflare
```

2. (Optional) OpenRouter-backed digestion — set an API key, then optional
   model and base URL:

```sh
export HELLODB_BRAIN_OPENROUTER_API_KEY=...
# optional:
export HELLODB_BRAIN_OPENROUTER_MODEL=openai/gpt-4o-mini
export HELLODB_BRAIN_OPENROUTER_BASE_URL=https://openrouter.ai/api/v1
export HELLODB_BRAIN_OPENROUTER_FALLBACK_TO_MOCK=1
```

3. Point `brain.toml` at the digest backend:

```toml
[digest]
backend = "openrouter"
```

### Credential tracker (recommended)

Store these once so setup can be repeated consistently:

- `HELLODB_HOME`
- `HELLODB_EMBED_GATEWAY_URL`
- `HELLODB_EMBED_GATEWAY_TOKEN` (or keychain reference)
- `HELLODB_BRAIN_OPENROUTER_API_KEY` (if using openrouter digest)
- Cloudflare worker name + R2 bucket name

### Connector snippets

**Links to share**

| What | URL |
|------|-----|
| **MCP setup (all hosts)** — stdio paths, Claude, Cursor, Codex, remote URL | `https://github.com/ishpr/hellodb#connector-snippets` |
| **Install script** (downloads `hellodb`, `hellodb-mcp`, `hellodb-brain`) | `https://hellodb.dev/install` |
| **OpenAI Codex — MCP** (config format, `codex mcp add`, HTTP MCP) | [`developers.openai.com/codex/mcp`](https://developers.openai.com/codex/mcp) |

There is **no special “MCP URL” for local Codex**: you install the binaries,
then point Codex at the **`hellodb-mcp` executable** (stdio). A `https://…`
URL only applies if **you** host a remote MCP endpoint (gateway / bridge) and
configure Codex for **streamable HTTP** per OpenAI’s docs.

These examples are **Claude- and Cursor-flavored** in places because their MCP
UIs are well documented; the underlying contract is **generic MCP**—any
compliant client can use the same binary or URL.

- **Any MCP host (local stdio):** configure your client to run the installed
  binary as the MCP server command (no args required):

```sh
command -v hellodb-mcp
```

  Use the absolute path your client expects; after `curl … | sh` it is usually
  under `/usr/local/bin` or `~/.local/bin`.

- **Claude Desktop (remote connector):**

  Add a custom connector URL to your exposed MCP endpoint, for example:

```text
https://YOUR_HOST/hellodb-mcp?key=YOUR_ACCESS_KEY
```

- **Claude Code (local stdio):** use the MCP binary (same as the bundled
  plugin — `hellodb-mcp`, not the `hellodb` CLI):

```sh
claude mcp add hellodb "$(command -v hellodb-mcp)"
```

- **OpenAI Codex (local stdio):** install first (`curl -fsSL hellodb.dev/install | sh`),
  then register the server (writes `~/.codex/config.toml` by default). See
  [Codex MCP](https://developers.openai.com/codex/mcp).

```sh
codex mcp add hellodb -- "$(command -v hellodb-mcp)"
```

  If Codex can’t resolve your PATH when it spawns the process, use the absolute
  path from `command -v hellodb-mcp` (often `/usr/local/bin/hellodb-mcp`). For
  **remote** MCP, use Codex’s HTTP/streamable options with a URL you control
  (same shape as Claude Desktop above), not a hellodb-owned public endpoint.

- **Cursor / other hosts (remote via bridge):** e.g. `supergateway` wrapping
  your HTTPS MCP endpoint:

```json
{
  "mcpServers": {
    "hellodb-remote": {
      "command": "npx",
      "args": [
        "-y",
        "supergateway",
        "--streamableHttp",
        "https://YOUR_HOST/hellodb-mcp?key=YOUR_ACCESS_KEY"
      ]
    }
  }
}
```

See `integrations/remote-mcp-bridge/README.md` for additional bridge options.

---

## The memory loop

```text
┌──────────────────┐   /hellodb:memorize    ┌──────────────────┐
│  Claude session  │ ────────────────────► │ claude.episodes  │
│  (coding agent)  │   agent writes raw    │  (namespaced)    │
└────────┬─────────┘                       └────────┬─────────┘
         │                                            │ tail cursor
         │                                            ▼
         │                                  ┌──────────────────┐
         │                                  │ hellodb-brain    │
         │                                  │ Stop hook each   │
         │                                  │ session + digest │
         │                                  │ (plugin agents)  │
         │                                  └────────┬─────────┘
         │                                            │
         │                                            ▼
         │                                  ┌──────────────────┐
         │                                  │ Draft branch     │
         │                                  │ claude.facts/    │
         │                                  │ digest-*         │
         │                                  └────────┬─────────┘
         │                                            │
         │   /hellodb:review (merge approved facts) ◄┘
         │
         ▼
┌──────────────────┐
│ /hellodb:recall  │◄── MCP: hellodb_embed_and_search, hellodb_find_relevant_memories
│ + decay ranking  │    (semantic when embedder configured; keyword fallback otherwise)
└──────────────────┘
```

**Privacy model:** records are content-addressed and signed by a local
Ed25519 identity. The SQLite file is encrypted with a SQLCipher key
derived from the identity seed via BLAKE3. Vector indices are sealed
at rest per namespace using ChaCha20-Poly1305 (see `hellodb-crypto`).

**Sidecar pattern:** digestion runs in `hellodb-brain` (Stop hook + plugin
agents), not inside the main agent turn. The coding agent writes episodes;
the brain tails the log and proposes facts on draft branches for you to
review. Backends: `mock` for deterministic local runs, or `openrouter` when
configured for LLM-backed extraction. See **Path B** under
[Dual-path onboarding](#dual-path-onboarding) for `brain.toml` and
environment variables.

---

## Architecture

```
crates/
├── hellodb-crypto     Ed25519, X25519, ChaCha20-Poly1305, BLAKE3 primitives
├── hellodb-core       Record model, namespaces, schemas, branches, merge
├── hellodb-storage    SQLCipher + MemoryEngine + tail-cursor + metadata sidecar
├── hellodb-auth       Consent/delegation/access primitives (library layer)
├── hellodb-query      Filter / sort / cursor-paginate query engine
├── hellodb-sync       Encrypted-delta sync: FileSystem, Memory, GatewayBackend
├── hellodb-vector     Per-namespace encrypted ANN index with POSIX flock
├── hellodb-embed      Pluggable Embedder: Cloudflare gateway, OpenAI-compat,
│                      mock (deterministic, for tests), fastembed (local ONNX, opt-in feature)
├── hellodb-brain      Passive digest daemon — CLI + Stop-hook orchestration
├── hellodb-mcp        stdio JSON-RPC MCP server exposing all primitives
└── hellodb-cli        Unified `hellodb` front door: init / status / recall / ingest / doctor / mcp / brain

plugin/                 Claude Code plugin bundle (manifest, skills, agents, hooks)
gateway/                TypeScript Cloudflare Worker: Workers AI + R2 proxy
installer/              TypeScript Cloudflare Worker: serves install.sh / install.ps1
scripts/                install.sh + install.ps1 + onboard.sh + setup-cloudflare.sh
recipes/                Community playbooks (metadata + README contract)
integrations/           Reference architectures (Slack capture, remote MCP bridge, …)
landing/                Next.js landing page (served at hellodb.dev)
```

---

## Developing from source

```sh
git clone https://github.com/ishpr/hellodb
cd hellodb

# Build + run + install plugin locally in one command
make onboard          # prompts for Rust install if missing, then build + bundle + init + optional Cloudflare

# Or, step by step:
make build            # cargo build --release
make test             # cargo test --workspace
make bundle           # copy binaries into plugin/bin/
make install          # register with Claude Code (user scope)
make setup-cloudflare # zero-token gateway deploy via wrangler login
```

See the Makefile for the full target list.

---

## CLI reference

```
hellodb init                 first-time setup: data dir, identity, brain.toml
hellodb status               identity + namespaces + record counts + brain state
hellodb recall [--top N]     top facts ranked by decayed reinforcement score
                             flags: --top, --namespace, --format md|json,
                                    --half-life-days, --verbose
hellodb ingest               import Claude Code auto-memory markdown files
                             flags: --from-claudemd (scan ~/.claude/projects/*/memory/*.md),
                                    --source PATH (explicit dir), --dry-run
hellodb mcp                  run the MCP server (stdio; for Claude Code)
hellodb brain [--status]     run one passive-memory digest pass
                             flags: --dry-run, --force, --status, --init-config
hellodb doctor               diagnose config / permission / DB-open issues
```

Configure via `HELLODB_HOME` (default `~/.hellodb`). All binaries read the
same encrypted DB from that location.

---

## License

MIT — see [LICENSE](./LICENSE).

---

## Security

If you find a vulnerability, please open a private security advisory at
https://github.com/ishpr/hellodb/security/advisories/new rather than a
public issue. No bug bounty, but we'll respond quickly and credit you in
the fix.

---

## Acknowledgments

Built on [SQLCipher](https://www.zetetic.net/sqlcipher/),
[rusqlite](https://github.com/rusqlite/rusqlite),
[BLAKE3](https://github.com/BLAKE3-team/BLAKE3), and
[ed25519-dalek](https://github.com/dalek-cryptography/curve25519-dalek).
Runs optionally on [Cloudflare Workers AI](https://developers.cloudflare.com/workers-ai/)
and [R2](https://developers.cloudflare.com/r2/) via your own account.
