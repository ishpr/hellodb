import { env, SELF } from "cloudflare:test";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { setEmbedRunner, type EmbedRunner } from "../src/embed";

const TOKEN = "test-secret-token";
const authHeaders = { Authorization: `Bearer ${TOKEN}` };

describe("GET /health", () => {
  it("returns 200 without auth", async () => {
    const res = await SELF.fetch("http://gateway.test/health");
    expect(res.status).toBe(200);
    const body = (await res.json()) as { status: string; version: string; features: string[] };
    expect(body.status).toBe("ok");
    expect(body.features).toEqual(expect.arrayContaining(["embed", "r2"]));
    expect(typeof body.version).toBe("string");
  });

  it("ignores a wrong bearer on /health", async () => {
    const res = await SELF.fetch("http://gateway.test/health", {
      headers: { Authorization: "Bearer wrong" },
    });
    expect(res.status).toBe(200);
  });
});

describe("auth", () => {
  it("rejects /embed without Authorization", async () => {
    const res = await SELF.fetch("http://gateway.test/embed", {
      method: "POST",
      body: JSON.stringify({ text: "hi" }),
      headers: { "Content-Type": "application/json" },
    });
    expect(res.status).toBe(401);
    const body = (await res.json()) as { error: string; code: string };
    expect(body.code).toBe("missing_auth");
  });

  it("rejects /r2/ list without Authorization", async () => {
    const res = await SELF.fetch("http://gateway.test/r2?prefix=foo");
    expect(res.status).toBe(401);
  });

  it("rejects wrong bearer", async () => {
    const res = await SELF.fetch("http://gateway.test/embed", {
      method: "POST",
      body: JSON.stringify({ text: "hi" }),
      headers: { "Content-Type": "application/json", Authorization: "Bearer nope" },
    });
    expect(res.status).toBe(401);
    const body = (await res.json()) as { error: string; code: string };
    expect(body.code).toBe("invalid_auth");
  });

  it("rejects malformed Authorization header", async () => {
    const res = await SELF.fetch("http://gateway.test/embed", {
      method: "POST",
      body: JSON.stringify({ text: "hi" }),
      headers: { "Content-Type": "application/json", Authorization: "Basic abc" },
    });
    expect(res.status).toBe(401);
  });
});

describe("R2 roundtrip", () => {
  // Use a unique prefix per test run so the local R2 simulator's persistent
  // state doesn't leak between suites.
  const prefix = `test-${Date.now()}-${Math.floor(Math.random() * 1e9)}`;

  afterEach(async () => {
    // Best-effort cleanup to keep the local bucket tidy across runs.
    const list = await env.R2.list({ prefix });
    await Promise.all(list.objects.map((o) => env.R2.delete(o.key)));
  });

  it("PUT then GET round-trips bytes", async () => {
    const key = `${prefix}/roundtrip.bin`;
    const payload = new Uint8Array([1, 2, 3, 4, 5, 250, 251, 252]);

    const put = await SELF.fetch(`http://gateway.test/r2/${key}`, {
      method: "PUT",
      body: payload,
      headers: { ...authHeaders, "Content-Type": "application/octet-stream" },
    });
    expect(put.status).toBe(204);

    const get = await SELF.fetch(`http://gateway.test/r2/${key}`, { headers: authHeaders });
    expect(get.status).toBe(200);
    expect(get.headers.get("Content-Type")).toBe("application/octet-stream");
    const bytes = new Uint8Array(await get.arrayBuffer());
    expect(Array.from(bytes)).toEqual(Array.from(payload));
  });

  it("preserves slashes in keys", async () => {
    const key = `${prefix}/namespaces/claude.memory/deltas/1000.delta`;
    const payload = new TextEncoder().encode("opaque-ciphertext");

    const put = await SELF.fetch(`http://gateway.test/r2/${key}`, {
      method: "PUT",
      body: payload,
      headers: authHeaders,
    });
    expect(put.status).toBe(204);

    const get = await SELF.fetch(`http://gateway.test/r2/${key}`, { headers: authHeaders });
    expect(get.status).toBe(200);
    const text = new TextDecoder().decode(new Uint8Array(await get.arrayBuffer()));
    expect(text).toBe("opaque-ciphertext");
  });

  it("DELETE then GET returns 404", async () => {
    const key = `${prefix}/delete-me.bin`;

    await SELF.fetch(`http://gateway.test/r2/${key}`, {
      method: "PUT",
      body: new Uint8Array([9, 9, 9]),
      headers: authHeaders,
    });

    const del = await SELF.fetch(`http://gateway.test/r2/${key}`, {
      method: "DELETE",
      headers: authHeaders,
    });
    expect(del.status).toBe(204);

    const get = await SELF.fetch(`http://gateway.test/r2/${key}`, { headers: authHeaders });
    expect(get.status).toBe(404);
    const body = (await get.json()) as { error: string; code: string };
    expect(body.code).toBe("not_found");
  });

  it("DELETE on missing key returns 404", async () => {
    const res = await SELF.fetch(`http://gateway.test/r2/${prefix}/never-existed`, {
      method: "DELETE",
      headers: authHeaders,
    });
    expect(res.status).toBe(404);
  });

  it("GET on missing key returns 404", async () => {
    const res = await SELF.fetch(`http://gateway.test/r2/${prefix}/missing.bin`, {
      headers: authHeaders,
    });
    expect(res.status).toBe(404);
  });

  it("LIST with prefix returns expected keys", async () => {
    // Seed three objects under the prefix.
    const keys = [
      `${prefix}/list/a.delta`,
      `${prefix}/list/b.delta`,
      `${prefix}/list/nested/c.delta`,
    ];
    for (const k of keys) {
      const res = await SELF.fetch(`http://gateway.test/r2/${k}`, {
        method: "PUT",
        body: new Uint8Array([1]),
        headers: authHeaders,
      });
      expect(res.status).toBe(204);
    }

    const res = await SELF.fetch(
      `http://gateway.test/r2?prefix=${encodeURIComponent(`${prefix}/list/`)}`,
      { headers: authHeaders },
    );
    expect(res.status).toBe(200);
    const body = (await res.json()) as {
      keys: string[];
      truncated: boolean;
      next_cursor: string | null;
    };
    expect(body.keys.sort()).toEqual([...keys].sort());
    expect(body.truncated).toBe(false);
    expect(body.next_cursor).toBe(null);
  });

  it("rejects PUT larger than 50 MB via Content-Length", async () => {
    const key = `${prefix}/too-big.bin`;
    // Declare an oversized Content-Length without actually sending 50 MB.
    const res = await SELF.fetch(`http://gateway.test/r2/${key}`, {
      method: "PUT",
      body: new Uint8Array([0]),
      headers: {
        ...authHeaders,
        "Content-Length": String(60 * 1024 * 1024),
      },
    });
    expect(res.status).toBe(413);
    const body = (await res.json()) as { code: string };
    expect(body.code).toBe("body_too_large");
  });

  it("rejects invalid list limit", async () => {
    const res = await SELF.fetch("http://gateway.test/r2?limit=-5", { headers: authHeaders });
    expect(res.status).toBe(400);
  });
});

describe("POST /embed", () => {
  beforeEach(() => {
    // Default stub returning a 384-dim vector per input.
    const runner: EmbedRunner = async (_model, texts) => {
      const data = texts.map(() => Array.from({ length: 384 }, (_, i) => i / 384));
      return { shape: [texts.length, 384], data };
    };
    setEmbedRunner(runner);
  });

  afterEach(() => {
    setEmbedRunner(null);
  });

  it("returns {embedding, dim, model} for single text", async () => {
    const res = await SELF.fetch("http://gateway.test/embed", {
      method: "POST",
      headers: { ...authHeaders, "Content-Type": "application/json" },
      body: JSON.stringify({ text: "hello world" }),
    });
    expect(res.status).toBe(200);
    const body = (await res.json()) as {
      embedding: number[];
      dim: number;
      model: string;
    };
    expect(Array.isArray(body.embedding)).toBe(true);
    expect(body.embedding).toHaveLength(384);
    expect(body.dim).toBe(384);
    expect(body.model).toBe("@cf/baai/bge-small-en-v1.5");
  });

  it("returns {embeddings, dim, model} for batch", async () => {
    const res = await SELF.fetch("http://gateway.test/embed", {
      method: "POST",
      headers: { ...authHeaders, "Content-Type": "application/json" },
      body: JSON.stringify({
        texts: ["foo", "bar", "baz"],
        model: "@cf/baai/bge-base-en-v1.5",
      }),
    });
    expect(res.status).toBe(200);
    const body = (await res.json()) as {
      embeddings: number[][];
      dim: number;
      model: string;
    };
    expect(body.embeddings).toHaveLength(3);
    expect(body.embeddings[0]).toHaveLength(384);
    expect(body.dim).toBe(384);
    expect(body.model).toBe("@cf/baai/bge-base-en-v1.5");
  });

  it("uses default model when omitted", async () => {
    const res = await SELF.fetch("http://gateway.test/embed", {
      method: "POST",
      headers: { ...authHeaders, "Content-Type": "application/json" },
      body: JSON.stringify({ text: "default" }),
    });
    expect(res.status).toBe(200);
    const body = (await res.json()) as { model: string };
    expect(body.model).toBe("@cf/baai/bge-small-en-v1.5");
  });

  it("rejects disallowed model with 400", async () => {
    const res = await SELF.fetch("http://gateway.test/embed", {
      method: "POST",
      headers: { ...authHeaders, "Content-Type": "application/json" },
      body: JSON.stringify({ text: "hi", model: "@cf/meta/llama-3-8b-instruct" }),
    });
    expect(res.status).toBe(400);
    const body = (await res.json()) as { code: string };
    expect(body.code).toBe("model_not_allowed");
  });

  it("rejects empty body with 400", async () => {
    const res = await SELF.fetch("http://gateway.test/embed", {
      method: "POST",
      headers: { ...authHeaders, "Content-Type": "application/json" },
      body: JSON.stringify({}),
    });
    expect(res.status).toBe(400);
  });

  it("rejects both text and texts with 400", async () => {
    const res = await SELF.fetch("http://gateway.test/embed", {
      method: "POST",
      headers: { ...authHeaders, "Content-Type": "application/json" },
      body: JSON.stringify({ text: "a", texts: ["b"] }),
    });
    expect(res.status).toBe(400);
  });

  it("rejects invalid JSON with 400", async () => {
    const res = await SELF.fetch("http://gateway.test/embed", {
      method: "POST",
      headers: { ...authHeaders, "Content-Type": "application/json" },
      body: "{not json",
    });
    expect(res.status).toBe(400);
  });

  it("returns 502 when AI binding throws", async () => {
    const failingRunner: EmbedRunner = async () => {
      throw new Error("boom");
    };
    setEmbedRunner(failingRunner);
    const res = await SELF.fetch("http://gateway.test/embed", {
      method: "POST",
      headers: { ...authHeaders, "Content-Type": "application/json" },
      body: JSON.stringify({ text: "hi" }),
    });
    expect(res.status).toBe(502);
    const body = (await res.json()) as { code: string };
    expect(body.code).toBe("embed_failed");
  });
});

describe("unknown routes", () => {
  it("returns 404 for unknown authenticated route", async () => {
    const res = await SELF.fetch("http://gateway.test/nope", { headers: authHeaders });
    expect(res.status).toBe(404);
  });

  it("returns 401 for unknown route without auth", async () => {
    // Unauthenticated requests hit auth middleware first, so 401 is correct.
    const res = await SELF.fetch("http://gateway.test/nope");
    expect(res.status).toBe(401);
  });
});

