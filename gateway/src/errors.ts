import type { Context } from "hono";
import type { ContentfulStatusCode } from "hono/utils/http-status";

/** Standard error envelope — never leak Cloudflare internals or stack traces. */
export interface ErrorBody {
  error: string;
  code: string;
}

export function errorResponse(
  c: Context,
  status: ContentfulStatusCode,
  code: string,
  message: string,
): Response {
  const body: ErrorBody = { error: message, code };
  return c.json(body, status);
}
