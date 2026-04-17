import { Hono } from "hono";
import type { Env } from "./env";
import { errorResponse } from "./errors";

/** 50 MB hard cap per object. Rejects larger bodies with 413. */
const MAX_OBJECT_BYTES = 50 * 1024 * 1024;

/** Default and maximum number of keys returned by LIST. */
const DEFAULT_LIST_LIMIT = 100;
const MAX_LIST_LIMIT = 1000;

export const r2 = new Hono<{ Bindings: Env }>();

/**
 * Extract the raw object key from a path like `/r2/foo/bar`. We preserve the
 * remainder verbatim so callers can use `/` in keys (e.g. nested namespaces).
 */
function extractKey(path: string): string | null {
  // Strip leading `/r2/` (or `/r2` with no trailing slash — list endpoint).
  const match = /^\/r2\/(.+)$/.exec(path);
  if (!match) return null;
  const key = match[1];
  if (!key || key.length === 0) return null;
  // Reject obvious traversal or control chars. R2 itself allows most strings
  // but we keep this conservative.
  if (key.includes("\0")) return null;
  try {
    return decodeURIComponent(key);
  } catch {
    return null;
  }
}

/** GET /r2?prefix=...&cursor=...&limit=... — list keys. */
r2.get("/r2", async (c) => {
  const prefix = c.req.query("prefix") ?? undefined;
  const cursor = c.req.query("cursor") ?? undefined;
  const limitStr = c.req.query("limit");

  let limit = DEFAULT_LIST_LIMIT;
  if (limitStr !== undefined) {
    const parsed = Number.parseInt(limitStr, 10);
    if (!Number.isFinite(parsed) || parsed <= 0) {
      return errorResponse(c, 400, "invalid_limit", "`limit` must be a positive integer");
    }
    limit = Math.min(parsed, MAX_LIST_LIMIT);
  }

  const listed = await c.env.R2.list({
    prefix,
    cursor,
    limit,
  });

  return c.json({
    keys: listed.objects.map((o) => o.key),
    truncated: listed.truncated,
    next_cursor: listed.truncated ? listed.cursor ?? null : null,
  });
});

/** PUT /r2/:key* — store opaque bytes. */
r2.put("/r2/*", async (c) => {
  const key = extractKey(new URL(c.req.url).pathname);
  if (!key) {
    return errorResponse(c, 400, "invalid_key", "Invalid or missing object key");
  }

  // Enforce size cap via Content-Length when present; otherwise read the full
  // body and check length. R2 accepts ArrayBuffer directly, so we consume and
  // then hand off.
  const contentLengthHeader = c.req.header("Content-Length");
  if (contentLengthHeader !== undefined) {
    const declared = Number.parseInt(contentLengthHeader, 10);
    if (Number.isFinite(declared) && declared > MAX_OBJECT_BYTES) {
      return errorResponse(c, 413, "body_too_large", `Body exceeds ${MAX_OBJECT_BYTES} bytes`);
    }
  }

  const buffer = await c.req.arrayBuffer();
  if (buffer.byteLength > MAX_OBJECT_BYTES) {
    return errorResponse(c, 413, "body_too_large", `Body exceeds ${MAX_OBJECT_BYTES} bytes`);
  }

  await c.env.R2.put(key, buffer);
  return c.body(null, 204);
});

/** GET /r2/:key* — return stored bytes verbatim. */
r2.get("/r2/*", async (c) => {
  const key = extractKey(new URL(c.req.url).pathname);
  if (!key) {
    return errorResponse(c, 400, "invalid_key", "Invalid or missing object key");
  }
  const object = await c.env.R2.get(key);
  if (object === null) {
    return errorResponse(c, 404, "not_found", "Object not found");
  }
  // Stream directly. Content-Type is always octet-stream because bytes are
  // opaque (sealed-box encrypted on the Rust side).
  return new Response(object.body, {
    status: 200,
    headers: {
      "Content-Type": "application/octet-stream",
      "Content-Length": String(object.size),
      "ETag": object.httpEtag,
    },
  });
});

/** DELETE /r2/:key* — remove object; 404 if missing. */
r2.delete("/r2/*", async (c) => {
  const key = extractKey(new URL(c.req.url).pathname);
  if (!key) {
    return errorResponse(c, 400, "invalid_key", "Invalid or missing object key");
  }

  // R2.delete() is idempotent (doesn't error on missing keys), so we do a
  // head-check first to return 404 as specified by the contract.
  const head = await c.env.R2.head(key);
  if (head === null) {
    return errorResponse(c, 404, "not_found", "Object not found");
  }
  await c.env.R2.delete(key);
  return c.body(null, 204);
});
