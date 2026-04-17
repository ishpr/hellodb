# hellodb-gateway

A tiny Cloudflare Worker that fronts two Cloudflare services for a local hellodb installation:

1. **Workers AI** — cheap, quota-friendly text embeddings (`@cf/baai/bge-*`).
2. **R2** — opaque object store for encrypted delta-sync blobs.

**Local hellodb never talks to Cloudflare directly.** It talks to *your* deployed Worker, authenticated with one shared-secret bearer token. That gives you:

- One token to rotate instead of CF API keys + R2 access keys.
- Sovereign billing — every user owns their own CF account and R2 bucket.
- A single chokepoint you can revoke at the edge.
- A natural place to drop Cloudflare Access, mTLS, or per-device OAuth later.

---

## Deploy (5 steps)

```bash
cd gateway
npm install
npx wrangler login

# 1. Create the R2 bucket that will hold your encrypted deltas.
npx wrangler r2 bucket create hellodb

# 2. Generate and set a strong bearer token. Copy the token — you'll paste it
#    into ~/.hellodb/cloudflare.toml in the next step.
TOKEN=$(openssl rand -hex 32)
echo "$TOKEN" | npx wrangler secret put GATEWAY_TOKEN
echo "Token: $TOKEN"

# 3. Deploy.
npx wrangler deploy
```

`wrangler deploy` prints a URL like `https://hellodb-gateway.<you>.workers.dev`. Copy both the URL and the token into `~/.hellodb/cloudflare.toml`:

```toml
[gateway]
url   = "https://hellodb-gateway.<you>.workers.dev"
token = "<the token you just generated>"
```

Workers AI is enabled automatically the first time the Worker calls `env.AI.run(...)` — no extra setup, but note the [free-tier limits](https://developers.cloudflare.com/workers-ai/platform/pricing/) (roughly 10k neurons/day, ~90k bge-small calls).

---

## HTTP API (contract)

Every endpoint except `/health` requires `Authorization: Bearer <GATEWAY_TOKEN>`.

All non-2xx responses use a fixed envelope:

```json
{ "error": "human message", "code": "machine_code" }
```

### `GET /health`

```json
200 OK
{ "status": "ok", "version": "0.1.0", "features": ["embed", "r2"] }
```

### `POST /embed`

Single:

```jsonc
// Request
{ "text": "find my notes about pnpm", "model": "@cf/baai/bge-small-en-v1.5" }

// Response
{
  "embedding": [0.01, -0.03, ...],
  "dim": 384,
  "model": "@cf/baai/bge-small-en-v1.5"
}
```

Batch:

```jsonc
// Request
{ "texts": ["foo", "bar"], "model": "@cf/baai/bge-base-en-v1.5" }

// Response
{
  "embeddings": [[...], [...]],
  "dim": 384,
  "model": "@cf/baai/bge-base-en-v1.5"
}
```

- Default model: `@cf/baai/bge-small-en-v1.5` (384-d).
- Allowlisted models: `@cf/baai/bge-small-en-v1.5`, `@cf/baai/bge-base-en-v1.5`, `@cf/baai/bge-large-en-v1.5`. Anything else returns 400 `model_not_allowed`.
- Max 100 texts per batch, 8000 chars per input.

### `PUT /r2/:key*`

Stores opaque bytes. The key may contain `/` — e.g. `PUT /r2/namespaces/claude.memory/deltas/1000.delta`. Bodies > 50 MB return 413.

```
204 No Content
```

### `GET /r2/:key*`

```
200 OK                 (Content-Type: application/octet-stream)
404 Not Found          (code: not_found)
```

### `DELETE /r2/:key*`

```
204 No Content
404 Not Found          (code: not_found — the key never existed)
```

### `GET /r2?prefix=…&cursor=…&limit=…`

```json
200 OK
{
  "keys": ["namespaces/claude.memory/deltas/1000.delta", "..."],
  "truncated": false,
  "next_cursor": null
}
```

Default `limit=100`, max `1000`. When `truncated` is `true`, pass `next_cursor` as `cursor` on the next request.

---

## Security notes

- **Single shared secret.** Today the gateway authenticates with one bearer token compared constant-time against `GATEWAY_TOKEN`. Rotate with `wrangler secret put GATEWAY_TOKEN` — the old token stops working the moment the new deploy reaches the edge.
- **Opaque bytes.** The gateway does not inspect, re-encrypt, or decompress R2 bodies. The Rust sync layer is responsible for end-to-end encryption (sealed-box) before `PUT`.
- **No rate limiting yet.** A leaked bearer can burn through your Workers-AI free tier until you rotate. Treat the token like a credit-card number.
- **No arbitrary model access.** The `/embed` endpoint rejects any model outside the bge-* allowlist, so a leaked token can't be used to invoke Llama / Whisper / image-gen models on your account.
- **Future slice: Cloudflare Access.** To get OAuth-per-device, front this Worker with a Cloudflare Access application and swap the bearer check for a service-token or JWT verification. The API shape does not need to change.

---

## Developing

```bash
npm install
npm test                 # vitest-pool-workers — runs against miniflare locally
npx wrangler types       # regenerate worker-configuration.d.ts after config changes
npx wrangler dev         # local dev server on http://localhost:8787
```

`npm test` uses a stubbed Workers-AI binding, so it runs fully offline. The R2 binding uses miniflare's local simulation.

---

## File layout

```
gateway/
├── package.json
├── tsconfig.json
├── wrangler.jsonc
├── vitest.config.ts
├── src/
│   ├── index.ts     — router + error handler
│   ├── auth.ts      — constant-time bearer check
│   ├── embed.ts     — POST /embed → Workers AI
│   ├── r2.ts        — PUT/GET/DELETE/LIST /r2/*
│   ├── health.ts    — GET /health
│   ├── env.ts       — typed bindings
│   └── errors.ts    — error envelope helper
└── test/
    └── index.test.ts
```

---

## Auth choices

The gateway supports three auth modes, layered:

### 1. Bearer only (default, simplest)

The Worker checks `Authorization: Bearer $GATEWAY_TOKEN` against a secret you generate and upload with `wrangler secret put GATEWAY_TOKEN`. Good for single-user or small-team setups. That's what `scripts/setup-cloudflare.sh` configures automatically.

### 2. Cloudflare Access in front of the Worker (recommended for teams)

If you enable [Cloudflare Access](https://developers.cloudflare.com/cloudflare-one/applications/) on the Worker URL, Cloudflare will challenge every request at the edge — the Worker never sees unauthorized traffic. You configure identity providers (GitHub, Google, email-OTP, SAML, OIDC) in the Cloudflare dashboard.

**Enable it (dashboard flow, ~5 clicks):**
1. Cloudflare dashboard → Zero Trust → Access → Applications → Add an application.
2. Type: **Self-hosted**. Application domain: your Worker URL.
3. Add a policy: *"Allow users whose email matches …"* (or whatever scope).
4. Select identity providers — GitHub is fastest. Save.

From that moment on, browser traffic to the Worker is gated behind login. Headless tools (like the local hellodb binary) then need an **Access Service Token**:

1. Same Access app → *Service auth* → Create a service token.
2. Copy the `Client ID` + `Client Secret`.
3. Set `HELLODB_EMBED_CF_ACCESS_CLIENT_ID` and `HELLODB_EMBED_CF_ACCESS_CLIENT_SECRET` in your shell.

The hellodb embed client automatically adds the `CF-Access-Client-Id` and `CF-Access-Client-Secret` headers, Access validates them at the edge, and your existing `GATEWAY_TOKEN` bearer still provides a second layer.

### 3. Cloudflare Access only (no shared bearer)

Same as #2 but you can disable the Worker-side bearer check. Not yet a supported config; file an issue if you want this wired as an env-var flag.

### Which should I pick?

- **Just me, one machine.** Mode 1.
- **Just me, multiple devices, want to revoke without rotating the bearer.** Mode 2 — Access with one service token per device.
- **Small team sharing one hellodb instance.** Mode 2.
- **Enterprise SSO / SAML integration.** Mode 2 with your corporate IdP.

---

## No "Sign in with Cloudflare" for hellodb itself

Cloudflare does not run a public OAuth provider that arbitrary third-party apps can register against — you can't build a consumer "Sign in with Cloudflare" button the way GitHub/Google offer. What you *can* do is have the install flow use `wrangler login` (Cloudflare's own OAuth, pre-registered for wrangler) to avoid making the user paste an API token. That's exactly what `scripts/setup-cloudflare.sh` does:

```
make setup-cloudflare    # opens a browser for OAuth, deploys everything, prints env
```

The resulting API token stays in the OS keychain under wrangler's control; hellodb never reads it.
