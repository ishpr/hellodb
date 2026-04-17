import { defineWorkersConfig } from "@cloudflare/vitest-pool-workers/config";

export default defineWorkersConfig({
  test: {
    poolOptions: {
      workers: {
        singleWorker: true,
        wrangler: { configPath: "./wrangler.jsonc" },
        miniflare: {
          // Provide a known secret value for tests; overrides anything from wrangler.jsonc.
          bindings: {
            GATEWAY_TOKEN: "test-secret-token",
            GATEWAY_VERSION: "0.1.0-test",
          },
        },
      },
    },
  },
});
