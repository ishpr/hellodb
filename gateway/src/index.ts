import { Hono } from "hono";
import type { Env } from "./env";
import { bearerAuth } from "./auth";
import { health } from "./health";
import { embed } from "./embed";
import { r2 } from "./r2";
import { errorResponse } from "./errors";

const app = new Hono<{ Bindings: Env }>();

// /health is the ONLY unauthenticated route. Mount before the bearer middleware.
app.route("/", health);

// Everything else requires a valid bearer token.
app.use("*", bearerAuth());

app.route("/", embed);
app.route("/", r2);

// 404 fallback in the authenticated namespace — only reachable after auth
// succeeds, so it can't be used to probe the token.
app.notFound((c) => errorResponse(c, 404, "not_found", "Route not found"));

// Final safety net — never leak Cloudflare internals or stack traces.
app.onError((err, c) => {
  console.error("unhandled", { err: String(err) });
  return errorResponse(c, 500, "internal_error", "Internal server error");
});

export default app;
