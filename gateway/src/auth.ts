import type { Context, MiddlewareHandler } from "hono";
import type { Env } from "./env";
import { errorResponse } from "./errors";

/**
 * Constant-time string comparison.
 *
 * We avoid early-exit comparisons so token validation cannot leak timing
 * information about the secret.
 */
function timingSafeEqual(a: string, b: string): boolean {
  const aBytes = new TextEncoder().encode(a);
  const bBytes = new TextEncoder().encode(b);
  // Compare lengths in constant-time-ish fashion by always iterating over the
  // longer buffer, but still force a mismatch if lengths differ.
  const len = Math.max(aBytes.length, bBytes.length);
  let diff = aBytes.length ^ bBytes.length;
  for (let i = 0; i < len; i++) {
    const aByte = i < aBytes.length ? aBytes[i]! : 0;
    const bByte = i < bBytes.length ? bBytes[i]! : 0;
    diff |= aByte ^ bByte;
  }
  return diff === 0;
}

function extractBearerToken(header: string | undefined): string | null {
  if (!header) return null;
  const match = /^Bearer\s+(.+)$/i.exec(header.trim());
  if (!match) return null;
  const token = match[1]?.trim();
  return token && token.length > 0 ? token : null;
}

/**
 * Bearer-token middleware. Rejects requests whose Authorization header does
 * not match GATEWAY_TOKEN. `/health` should be mounted before this middleware.
 */
export function bearerAuth(): MiddlewareHandler<{ Bindings: Env }> {
  return async (c: Context<{ Bindings: Env }>, next) => {
    const expected = c.env.GATEWAY_TOKEN;
    if (!expected || expected.length === 0) {
      // Server is misconfigured — refuse to serve so a missing secret doesn't
      // silently disable auth.
      return errorResponse(c, 500, "gateway_not_configured", "Gateway secret is not configured");
    }

    const presented = extractBearerToken(c.req.header("Authorization"));
    if (presented === null) {
      return errorResponse(c, 401, "missing_auth", "Missing bearer token");
    }

    if (!timingSafeEqual(presented, expected)) {
      return errorResponse(c, 401, "invalid_auth", "Invalid bearer token");
    }

    await next();
    return;
  };
}
