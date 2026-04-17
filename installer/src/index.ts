// hellodb-installer — lightweight public Worker at https://hellodb.dev.
//
// Routes:
//   GET /install       — POSIX shell installer (text/x-shellscript).
//                        On /install, if the User-Agent looks like PowerShell
//                        (iwr / Invoke-WebRequest), we return the .ps1 version
//                        so `iwr hellodb.dev/install | iex` Just Works.
//   GET /install.sh    — explicit POSIX path.
//   GET /install.ps1   — explicit PowerShell path.
//   GET /              — 302 to the GitHub repo.
//   GET /docs          — 302 to README.
//   GET /releases      — 302 to Releases tab.
//   GET /health        — { status, version, repo, install_urls }.
//   everything else    — 404 JSON.
//
// No auth. No user data. Just a static-serving public shim so the
// one-liner install URL is the same domain the user types into their
// browser.

export interface Env {
  INSTALLER_VERSION: string;
  REPO: string;
  INSTALL_SCRIPT_SH_RAW: string;
  INSTALL_SCRIPT_PS1_RAW: string;
}

export default {
  async fetch(req: Request, env: Env, _ctx: ExecutionContext): Promise<Response> {
    const url = new URL(req.url);

    if (req.method !== "GET" && req.method !== "HEAD") {
      return jsonError(405, "method_not_allowed", "GET only");
    }

    switch (url.pathname) {
      case "/install":
        // User-agent sniff so `iwr hellodb.dev/install | iex` on Windows
        // gets the .ps1 without the user having to remember the extension.
        return serveInstall(env, isPowershellUa(req) ? "ps1" : "sh");
      case "/install.sh":
        return serveInstall(env, "sh");
      case "/install.ps1":
        return serveInstall(env, "ps1");

      case "/health":
        return Response.json({
          status: "ok",
          version: env.INSTALLER_VERSION,
          repo: env.REPO,
          install_urls: {
            sh:  `https://${url.host}/install.sh`,
            ps1: `https://${url.host}/install.ps1`,
            auto: `https://${url.host}/install`,
          },
        });

      // NOTE: no "/" handler. The Worker is bound to hellodb.dev via
      // route patterns that only claim /install*, /health, /docs,
      // /readme, /releases. Everything else (including "/") falls
      // through to the Cloudflare Pages deployment of `landing/`.

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

async function serveInstall(env: Env, flavor: "sh" | "ps1"): Promise<Response> {
  const upstream = flavor === "ps1" ? env.INSTALL_SCRIPT_PS1_RAW : env.INSTALL_SCRIPT_SH_RAW;
  const resp = await fetch(upstream, {
    cf: { cacheTtl: 60, cacheEverything: true },
  });
  if (!resp.ok) {
    return jsonError(
      502,
      "upstream_unavailable",
      `could not fetch install.${flavor} (status ${resp.status})`,
    );
  }
  const body = await resp.text();
  const contentType =
    flavor === "ps1" ? "text/plain; charset=utf-8" : "text/x-shellscript; charset=utf-8";
  return new Response(body, {
    status: 200,
    headers: {
      "content-type": contentType,
      "cache-control": "public, max-age=60",
      "x-content-type-options": "nosniff",
      "x-install-source": upstream,
      "x-install-flavor": flavor,
      "x-installer-version": env.INSTALLER_VERSION,
    },
  });
}

function isPowershellUa(req: Request): boolean {
  const ua = (req.headers.get("user-agent") || "").toLowerCase();
  return (
    ua.includes("powershell") ||
    ua.includes("windowspowershell") ||
    ua.includes("invoke-webrequest") ||
    ua.includes("windowsnt")
  );
}

function jsonError(status: number, code: string, message: string): Response {
  return new Response(JSON.stringify({ error: message, code }), {
    status,
    headers: { "content-type": "application/json" },
  });
}
