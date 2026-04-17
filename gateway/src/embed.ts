import { Hono } from "hono";
import type { Env } from "./env";
import { errorResponse } from "./errors";

/**
 * Allowlisted embedding models. Keeping this tight prevents the gateway from
 * being used as a free proxy for arbitrary Workers AI traffic — callers can
 * only request bge-* embedding models.
 */
const ALLOWED_MODELS = [
  "@cf/baai/bge-small-en-v1.5",
  "@cf/baai/bge-base-en-v1.5",
  "@cf/baai/bge-large-en-v1.5",
] as const;

type AllowedModel = (typeof ALLOWED_MODELS)[number];

const DEFAULT_MODEL: AllowedModel = "@cf/baai/bge-small-en-v1.5";

/** Max individual input length (characters). Keeps payloads sane. */
const MAX_TEXT_CHARS = 8_000;

/** Max number of texts per batch request. */
const MAX_BATCH_SIZE = 100;

function isAllowedModel(model: string): model is AllowedModel {
  return (ALLOWED_MODELS as readonly string[]).includes(model);
}

interface EmbedRequestBody {
  text?: unknown;
  texts?: unknown;
  model?: unknown;
}

interface WorkersAiEmbedResponse {
  shape: number[];
  data: number[][];
}

/**
 * Validates the incoming JSON body into one of two shapes:
 * - single: { text: string }
 * - batch:  { texts: string[] }
 */
type ValidatedInput =
  | { kind: "single"; text: string; model: AllowedModel }
  | { kind: "batch"; texts: string[]; model: AllowedModel };

function validate(body: unknown): { ok: true; input: ValidatedInput } | { ok: false; code: string; message: string } {
  if (typeof body !== "object" || body === null) {
    return { ok: false, code: "invalid_body", message: "Request body must be a JSON object" };
  }
  const { text, texts, model: rawModel } = body as EmbedRequestBody;

  let model: AllowedModel;
  if (rawModel === undefined || rawModel === null) {
    model = DEFAULT_MODEL;
  } else if (typeof rawModel !== "string") {
    return { ok: false, code: "invalid_model", message: "`model` must be a string" };
  } else if (!isAllowedModel(rawModel)) {
    return {
      ok: false,
      code: "model_not_allowed",
      message: `Model not allowed. Allowed: ${ALLOWED_MODELS.join(", ")}`,
    };
  } else {
    model = rawModel;
  }

  const hasText = text !== undefined;
  const hasTexts = texts !== undefined;
  if (hasText && hasTexts) {
    return { ok: false, code: "invalid_body", message: "Provide either `text` or `texts`, not both" };
  }
  if (!hasText && !hasTexts) {
    return { ok: false, code: "invalid_body", message: "Provide `text` (string) or `texts` (string[])" };
  }

  if (hasText) {
    if (typeof text !== "string" || text.length === 0) {
      return { ok: false, code: "invalid_text", message: "`text` must be a non-empty string" };
    }
    if (text.length > MAX_TEXT_CHARS) {
      return { ok: false, code: "text_too_large", message: `\`text\` exceeds ${MAX_TEXT_CHARS} characters` };
    }
    return { ok: true, input: { kind: "single", text, model } };
  }

  if (!Array.isArray(texts)) {
    return { ok: false, code: "invalid_texts", message: "`texts` must be an array of strings" };
  }
  if (texts.length === 0) {
    return { ok: false, code: "invalid_texts", message: "`texts` must not be empty" };
  }
  if (texts.length > MAX_BATCH_SIZE) {
    return {
      ok: false,
      code: "batch_too_large",
      message: `\`texts\` exceeds ${MAX_BATCH_SIZE} items`,
    };
  }
  const validated: string[] = [];
  for (let i = 0; i < texts.length; i++) {
    const item = texts[i];
    if (typeof item !== "string" || item.length === 0) {
      return { ok: false, code: "invalid_texts", message: `\`texts[${i}]\` must be a non-empty string` };
    }
    if (item.length > MAX_TEXT_CHARS) {
      return { ok: false, code: "text_too_large", message: `\`texts[${i}]\` exceeds ${MAX_TEXT_CHARS} characters` };
    }
    validated.push(item);
  }
  return { ok: true, input: { kind: "batch", texts: validated, model } };
}

/**
 * Indirection for the Workers AI call. Production wires this straight to
 * `env.AI.run(...)`. Tests replace it via `setEmbedRunner(...)` so they can
 * run fully offline without burning Workers-AI quota.
 */
export type EmbedRunner = (
  model: AllowedModel,
  texts: string[],
  env: Env,
) => Promise<WorkersAiEmbedResponse>;

const defaultRunner: EmbedRunner = async (model, texts, env) => {
  return (await env.AI.run(model, { text: texts })) as WorkersAiEmbedResponse;
};

let currentRunner: EmbedRunner = defaultRunner;

/** Test-only: swap the embed backend. */
export function setEmbedRunner(runner: EmbedRunner | null): void {
  currentRunner = runner ?? defaultRunner;
}

export const embed = new Hono<{ Bindings: Env }>();

embed.post("/embed", async (c) => {
  let body: unknown;
  try {
    body = await c.req.json();
  } catch {
    return errorResponse(c, 400, "invalid_json", "Request body is not valid JSON");
  }

  const result = validate(body);
  if (!result.ok) {
    return errorResponse(c, 400, result.code, result.message);
  }
  const { input } = result;

  // Workers AI accepts { text: string | string[] } for bge-* models. Passing
  // an array always returns `{ shape: [N, D], data: [[...], ...] }`.
  const texts = input.kind === "single" ? [input.text] : input.texts;

  let response: WorkersAiEmbedResponse;
  try {
    response = await currentRunner(input.model, texts, c.env);
  } catch (err) {
    // Never leak internals; log to observability instead.
    console.error("workers-ai embed failed", { model: input.model, err: String(err) });
    return errorResponse(c, 502, "embed_failed", "Embedding provider failed");
  }

  if (!response || !Array.isArray(response.data) || response.data.length === 0) {
    return errorResponse(c, 502, "embed_failed", "Embedding provider returned no data");
  }

  const first = response.data[0];
  if (!Array.isArray(first)) {
    return errorResponse(c, 502, "embed_failed", "Embedding provider returned malformed data");
  }
  const dim = first.length;

  if (input.kind === "single") {
    return c.json({ embedding: first, dim, model: input.model });
  }
  return c.json({ embeddings: response.data, dim, model: input.model });
});
