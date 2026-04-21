import { Section } from "./section";

export function GatewayDiagram() {
  return (
    <Section
      id="diagram"
      eyebrow="architecture"
      title={
        <>
          Your machine. Your Cloudflare. <br />
          <span className="italic text-fg-muted">No middleman.</span>
        </>
      }
      lede={
        <>
          hellodb never talks to Cloudflare APIs directly — only through{" "}
          <span className="text-fg">your own</span> gateway Worker. Every
          primitive lives in your account. One token to rotate. The free tier
          covers solo use forever.
        </>
      }
    >
      {/* Mobile fallback: stacked cards (SVG is illegible <md) */}
      <div className="grid gap-4 md:hidden">
        <MobilePanel
          title="your machine"
          sub="~/.hellodb · always local"
          rows={[
            { label: "hellodb-mcp", hint: "stdio JSON-RPC 2.0" },
            { label: "hellodb-brain", hint: "digests on Stop hook" },
            { label: "SQLCipher", hint: "encrypted at rest" },
            { label: "vector index", hint: "per-namespace, encrypted" },
            { label: "Ed25519 keys", hint: "OS keychain" },
          ]}
        />
        <div className="py-1 text-center font-mono text-[11px] tracking-[0.16em] text-fg-muted">
          <span className="text-accent">▼</span>{" "}
          <span>HTTPS · bearer in OS keychain</span>{" "}
          <span className="text-accent">▼</span>
        </div>
        <MobilePanel
          title="your gateway Worker"
          sub="your Cloudflare · ~$0"
          accent
          rows={[
            { label: "/health", hint: "JSON status · unauthenticated" },
            { label: "/embed", hint: "→ Workers AI · bge-small (384d)" },
            { label: "/r2/*", hint: "→ R2 · encrypted blobs" },
            { label: "Bearer", hint: "Authorization · GATEWAY_TOKEN" },
            { label: "deploy", hint: "wrangler login · your account" },
          ]}
        />
      </div>

      <div className="hidden rounded-[var(--radius-card)] border border-border bg-bg-sunken/60 p-6 ring-amber md:block md:p-10">
        <svg
          viewBox="0 0 1000 480"
          className="h-auto w-full"
          role="img"
          aria-label="hellodb on your laptop talks over HTTPS to your gateway Worker, which proxies to Workers AI for embeddings and R2 for encrypted blobs; authenticated routes use a bearer token"
        >
          <defs>
            <linearGradient id="conn-grad" x1="0" x2="1" y1="0" y2="0">
              <stop offset="0" stopColor="var(--color-accent)" stopOpacity="0.1" />
              <stop offset="0.5" stopColor="var(--color-accent)" stopOpacity="0.55" />
              <stop offset="1" stopColor="var(--color-accent)" stopOpacity="0.1" />
            </linearGradient>
            <radialGradient id="packet-glow">
              <stop offset="0" stopColor="var(--color-accent)" stopOpacity="0.95" />
              <stop offset="1" stopColor="var(--color-accent)" stopOpacity="0" />
            </radialGradient>
          </defs>

          {/* LEFT: local machine */}
          <g>
            <rect
              x="40"
              y="60"
              width="320"
              height="360"
              rx="14"
              fill="var(--color-bg-elevated)"
              stroke="var(--color-border)"
            />
            <text
              x="60"
              y="92"
              className="fill-fg font-mono"
              style={{ fontSize: 14, fontWeight: 500 }}
            >
              your machine
            </text>
            <text
              x="60"
              y="112"
              className="fill-fg-subtle font-mono"
              style={{ fontSize: 11 }}
            >
              ~/.hellodb · always local
            </text>

            <line
              x1="60"
              x2="340"
              y1="130"
              y2="130"
              stroke="var(--color-border)"
            />

            {[
              { y: 158, label: "hellodb-mcp", desc: "stdio JSON-RPC 2.0" },
              { y: 208, label: "hellodb-brain", desc: "digests on Stop hook" },
              { y: 258, label: "SQLCipher", desc: "encrypted at rest" },
              { y: 308, label: "vector index", desc: "per-namespace, encrypted" },
              { y: 358, label: "Ed25519 keys", desc: "OS keychain" },
            ].map((row) => (
              <g key={row.label}>
                <circle
                  cx="76"
                  cy={row.y - 5}
                  r={3}
                  fill="var(--color-accent)"
                  opacity={0.7}
                />
                <text
                  x="92"
                  y={row.y}
                  className="fill-fg font-mono"
                  style={{ fontSize: 13 }}
                >
                  {row.label}
                </text>
                <text
                  x="92"
                  y={row.y + 16}
                  className="fill-fg-subtle font-mono"
                  style={{ fontSize: 10 }}
                >
                  {row.desc}
                </text>
              </g>
            ))}
          </g>

          {/* CONNECTION */}
          <g>
            <text
              x="500"
              y="180"
              textAnchor="middle"
              className="fill-fg-subtle font-mono"
              style={{ fontSize: 10, letterSpacing: 1.5 }}
            >
              HTTPS · bearer in OS keychain
            </text>

            <path
              d="M 360 200 L 640 200"
              stroke="var(--color-border-strong)"
              strokeWidth={1}
              strokeDasharray="4 6"
            />
            <path
              d="M 360 200 L 640 200"
              stroke="url(#conn-grad)"
              strokeWidth={2}
            />
            <text
              x="630"
              y="195"
              textAnchor="end"
              className="fill-accent font-mono"
              style={{ fontSize: 14 }}
            >
              ▶
            </text>

            <path
              d="M 640 250 L 360 250"
              stroke="var(--color-border-strong)"
              strokeWidth={1}
              strokeDasharray="4 6"
            />
            <path
              d="M 640 250 L 360 250"
              stroke="url(#conn-grad)"
              strokeWidth={2}
            />
            <text
              x="370"
              y="245"
              textAnchor="start"
              className="fill-accent font-mono"
              style={{ fontSize: 14 }}
            >
              ◀
            </text>

            {/* animated packets */}
            <circle r="11" fill="url(#packet-glow)" cy="200">
              <animate
                attributeName="cx"
                values="370;630;630"
                keyTimes="0;0.6;1"
                dur="3.2s"
                repeatCount="indefinite"
              />
              <animate
                attributeName="opacity"
                values="0;1;1;0"
                keyTimes="0;0.05;0.55;0.6"
                dur="3.2s"
                repeatCount="indefinite"
              />
            </circle>
            <circle r="3" fill="var(--color-accent)" cy="200">
              <animate
                attributeName="cx"
                values="370;630;630"
                keyTimes="0;0.6;1"
                dur="3.2s"
                repeatCount="indefinite"
              />
              <animate
                attributeName="opacity"
                values="0;1;1;0"
                keyTimes="0;0.05;0.55;0.6"
                dur="3.2s"
                repeatCount="indefinite"
              />
            </circle>
            <circle r="9" fill="url(#packet-glow)" cy="250">
              <animate
                attributeName="cx"
                values="630;630;370"
                keyTimes="0;0.4;1"
                dur="3.2s"
                repeatCount="indefinite"
              />
              <animate
                attributeName="opacity"
                values="0;0;1;1;0"
                keyTimes="0;0.4;0.45;0.95;1"
                dur="3.2s"
                repeatCount="indefinite"
              />
            </circle>

            <text
              x="500"
              y="290"
              textAnchor="middle"
              className="fill-fg-subtle font-mono"
              style={{ fontSize: 10 }}
            >
              encrypted deltas
            </text>
          </g>

          {/* RIGHT: gateway worker + CF primitives */}
          <g>
            <rect
              x="640"
              y="60"
              width="320"
              height="360"
              rx="14"
              fill="var(--color-bg-elevated)"
              stroke="var(--color-accent)"
              strokeOpacity={0.35}
            />
            <text
              x="660"
              y="92"
              className="fill-fg font-mono"
              style={{ fontSize: 14, fontWeight: 500 }}
            >
              your gateway Worker
            </text>
            <text
              x="660"
              y="112"
              className="fill-fg-subtle font-mono"
              style={{ fontSize: 11 }}
            >
              your Cloudflare account · ~$0
            </text>

            <line
              x1="660"
              x2="940"
              y1="130"
              y2="130"
              stroke="var(--color-border)"
            />

            {[
              { y: 158, route: "/health", target: "JSON status · unauthenticated" },
              { y: 208, route: "/embed", target: "Workers AI · bge-small (384d)" },
              { y: 258, route: "/r2/*", target: "R2 bucket · encrypted blobs" },
              { y: 308, route: "Bearer", target: "Authorization · GATEWAY_TOKEN" },
              { y: 358, route: "deploy", target: "wrangler login · your account" },
            ].map((row) => (
              <g key={row.route}>
                <text
                  x="676"
                  y={row.y}
                  className="fill-accent font-mono"
                  style={{ fontSize: 13 }}
                >
                  {row.route}
                </text>
                <text
                  x="676"
                  y={row.y + 16}
                  className="fill-fg-subtle font-mono"
                  style={{ fontSize: 10 }}
                >
                  → {row.target}
                </text>
              </g>
            ))}
          </g>

          {/* footer note */}
          <text
            x="500"
            y="455"
            textAnchor="middle"
            className="fill-fg-muted font-mono"
            style={{ fontSize: 11 }}
          >
            every box on the right lives in YOUR Cloudflare. one wrangler deploy. ~$0 free tier.
          </text>
        </svg>
      </div>

      <div className="mt-8 grid gap-4 sm:grid-cols-3">
        {[
          {
            kicker: "no shared infra",
            body: "We don't run a service. There's nothing to outage, breach, or shut down.",
          },
          {
            kicker: "no API token",
            body: "wrangler login (browser OAuth) provisions everything. No long-lived CF API token to leak or rotate.",
          },
          {
            kicker: "free tier covers solo",
            body: "10k Workers AI neurons/day. 10 GB R2. 100k Worker requests/day.",
          },
        ].map((c) => (
          <div
            key={c.kicker}
            className="rounded-xl border border-border bg-bg-elevated/40 p-4"
          >
            <div className="mb-1 font-mono text-[11px] uppercase tracking-[0.16em] text-accent-muted">
              {c.kicker}
            </div>
            <div className="text-sm leading-relaxed text-fg-muted">{c.body}</div>
          </div>
        ))}
      </div>
    </Section>
  );
}

function MobilePanel({
  title,
  sub,
  rows,
  accent = false,
}: {
  title: string;
  sub: string;
  rows: { label: string; hint: string }[];
  accent?: boolean;
}) {
  return (
    <div
      className={`rounded-[var(--radius-card)] border bg-bg-elevated/40 p-4 ${
        accent ? "border-accent/35" : "border-border"
      }`}
    >
      <div className="mb-1 font-mono text-[13px] text-fg">{title}</div>
      <div className="mb-3 font-mono text-[11px] text-fg-muted">{sub}</div>
      <div className="border-t border-border/60 pt-3">
        {rows.map((r, i) => (
          <div
            key={r.label}
            className={`flex items-center justify-between gap-3 py-1.5 font-mono text-[12px] ${
              i < rows.length - 1 ? "border-b border-border/30" : ""
            }`}
          >
            <span className={accent ? "text-accent" : "text-fg"}>{r.label}</span>
            <span className="text-right text-[11px] text-fg-muted">{r.hint}</span>
          </div>
        ))}
      </div>
    </div>
  );
}
