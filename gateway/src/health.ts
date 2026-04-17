import { Hono } from "hono";
import type { Env } from "./env";

export const health = new Hono<{ Bindings: Env }>();

health.get("/health", (c) => {
  return c.json({
    status: "ok",
    version: c.env.GATEWAY_VERSION ?? "unknown",
    features: ["embed", "r2"],
  });
});
