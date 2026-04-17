/**
 * Worker bindings.
 *
 * `wrangler types` generates `worker-configuration.d.ts` with a global
 * `Env` derived from `wrangler.jsonc`. Secrets aren't part of that config
 * (they live in the secret store), so we extend it here to add
 * `GATEWAY_TOKEN`.
 */

/// <reference path="../worker-configuration.d.ts" />

export interface Env extends Cloudflare.Env {
  /** Shared secret; set via `wrangler secret put GATEWAY_TOKEN`. */
  GATEWAY_TOKEN: string;
}
