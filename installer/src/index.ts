// hellodb-installer — lightweight public Worker at https://hellodb.dev.
//
// Routes:
//   GET /install   — fetch scripts/install.sh from raw.githubusercontent.com
//                    and return as text/plain so `curl | sh` works.
//   GET /install.sh — same as above, kept as an alias for `wget` users.
//   GET /          — redirect to the GitHub repo.
//   GET /docs      — redirect to the repo README.
//   GET /health    — { status: "ok", version, repo, install_url }.
//   everything else → 404 with a short JSON error.
//
// No authentication. No R2. No Workers AI. Just a dumb static-serving
// Worker that stays unconditionally public — the gateway Worker
// (authenticated) handles anything touching user data.

export interface Env {
  INSTALLER_VERSION: string;
  REPO: string;
  INSTALL_SCRIPT_RAW: string;
}

export default {
  async fetch(req: Request, env: Env, _ctx: ExecutionContext): Promise<Response> {
    const url = new URL(req.url);

    // Reject non-GET/HEAD up front — this is a read-only service.
    if (req.method !== "GET" && req.method !== "HEAD") {
      return jsonError(405, "method_not_allowed", "GET only");
    }

    switch (url.pathname) {
      case "/install":
      case "/install.sh":
        return serveInstallScript(env);

      case "/health":
        return Response.json({
          status: "ok",
          version: env.INSTALLER_VERSION,
          repo: env.REPO,
          install_url: `https://${url.host}/install`,
        });

      case "/":
        return Response.redirect(`https://github.com/${env.REPO}`, 302);

      case "/docs":
      case "/readme":
        return Response.redirect(`https://github.com/${env.REPO}#readme`, 302);

      case "/releases":
        return Response.redirect(`https://github.com/${env.REPO}/releases`, 302);

      default:
        return jsonError(404, "not_found", `no route for ${url.pathname}`);
    }
  },
};

async function serveInstallScript(env: Env): Promise<Response> {
  const upstream = await fetch(env.INSTALL_SCRIPT_RAW, {
    // Short cache — we want updates to land quickly but not thrash origin.
    cf: { cacheTtl: 60, cacheEverything: true },
  });
  if (!upstream.ok) {
    return jsonError(
      502,
      "upstream_unavailable",
      `could not fetch install.sh (status ${upstream.status})`,
    );
  }
  const body = await upstream.text();
  return new Response(body, {
    status: 200,
    headers: {
      "content-type": "text/x-shellscript; charset=utf-8",
      // Cache at the edge for 60s; clients (curl) shouldn't cache.
      "cache-control": "public, max-age=60",
      "x-content-type-options": "nosniff",
      // Advertise the source so users can audit before piping.
      "x-install-source": env.INSTALL_SCRIPT_RAW,
      "x-installer-version": env.INSTALLER_VERSION,
    },
  });
}

function jsonError(status: number, code: string, message: string): Response {
  return new Response(JSON.stringify({ error: message, code }), {
    status,
    headers: { "content-type": "application/json" },
  });
}
