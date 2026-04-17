/**
 * Tell `@cloudflare/vitest-pool-workers` what shape our test env has so
 * `import { env } from "cloudflare:test"` is fully typed against our bindings
 * (including the GATEWAY_TOKEN secret that wrangler.jsonc doesn't know about).
 */
import type { Env } from "../src/env";

declare module "cloudflare:test" {
  interface ProvidedEnv extends Env {}
}
