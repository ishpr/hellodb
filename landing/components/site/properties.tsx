import { Section } from "./section";

export function Properties() {
  return (
    <Section
      eyebrow="three properties"
      title={
        <>
          Encrypted. Branchable.{" "}
          <span className="italic text-fg-muted">Semantic.</span>
        </>
      }
      lede="Three Rust crates do most of the work. The rest is plumbing."
    >
      <div className="grid gap-4 md:grid-cols-3">
        <PropertyCard
          name="Encrypted"
          crate="hellodb-crypto"
          body="Ed25519 signatures, ChaCha20-Poly1305 at rest, BLAKE3 content addresses, SQLCipher database. Identity keys live in your OS keychain — never on disk in plaintext."
          illustration={<EncryptedIllustration />}
          snippet={[
            ["$", "hellodb status"],
            [" ", "\"fingerprint\": \"ed25519:…\""],
            ["$", "ls ~/.hellodb"],
            [" ", "local.db · identity.key"],
          ]}
        />
        <PropertyCard
          name="Branchable"
          crate="hellodb-core + brain"
          body="The brain digests episodes into facts on a draft branch. Nothing lands on main until you approve. Memory works like git — branches, merges, history you can audit."
          illustration={<BranchableIllustration />}
          snippet={[
            ["$", "hellodb status"],
            [" ", "… digest-* drafts in namespace JSON"],
            ["$", "/hellodb:review"],
            [" ", "merge facts → main"],
          ]}
        />
        <PropertyCard
          name="Semantic"
          crate="hellodb-vector + embed"
          body="Per-namespace encrypted vector index. Cosine similarity with time-decay reinforcement at recall. Embeddings via Workers AI, OpenAI-compatible, or fully offline via fastembed-rs."
          illustration={<SemanticIllustration />}
          snippet={[
            ["$", "hellodb recall \"pnpm conventions\""],
            [" ", "→ 0.91  use pnpm not npm"],
            [" ", "→ 0.84  pnpm-lock.yaml in repo"],
            [" ", "→ 0.71  pnpm dlx for one-shots"],
          ]}
        />
      </div>
    </Section>
  );
}

function PropertyCard({
  name,
  crate,
  body,
  illustration,
  snippet,
}: {
  name: string;
  crate: string;
  body: string;
  illustration: React.ReactNode;
  snippet: [string, string][];
}) {
  return (
    <div className="group flex flex-col rounded-[var(--radius-card)] border border-border bg-bg-elevated/40 p-6 transition-colors hover:border-border-strong">
      <div className="mb-5 flex h-32 items-center justify-center rounded-lg border border-border bg-bg-sunken/60">
        {illustration}
      </div>
      <div className="mb-1 font-mono text-[11px] uppercase tracking-[0.16em] text-accent-muted">
        {crate}
      </div>
      <h3 className="font-display text-2xl text-fg">{name}</h3>
      <p className="mt-2 text-[15px] leading-relaxed text-fg-muted text-pretty">
        {body}
      </p>
      <div className="mt-5 overflow-hidden rounded-lg border border-border bg-bg-sunken/60 p-3 font-mono text-[12px]">
        {snippet.map(([prompt, line], i) => (
          <div key={i} className="flex gap-2">
            <span
              className={
                prompt === "$" ? "select-none text-accent" : "select-none text-transparent"
              }
            >
              {prompt}
            </span>
            <span className={prompt === "$" ? "text-fg" : "text-fg-muted"}>
              {line}
            </span>
          </div>
        ))}
      </div>
    </div>
  );
}

function EncryptedIllustration() {
  return (
    <svg viewBox="0 0 200 80" className="h-full w-full">
      <g fontFamily="var(--font-mono)" fontSize="11">
        {Array.from({ length: 6 }).map((_, i) => (
          <g key={i}>
            <text
              x={20 + i * 15}
              y={30}
              className="fill-fg-subtle"
              opacity={0.6 - i * 0.08}
            >
              {["p", "n", "p", "m", " ", "u"][i]}
            </text>
            <text
              x={20 + i * 15}
              y={55}
              className="fill-accent"
              opacity={0.4 + i * 0.1}
            >
              {["x9", "Kq", "f7", "Lm", "vP", "z3"][i]}
            </text>
          </g>
        ))}
        <line
          x1="14"
          x2="120"
          y1="40"
          y2="40"
          stroke="var(--color-border-strong)"
          strokeDasharray="2 3"
        />
        <text x="135" y="44" className="fill-fg-subtle">
          → ChaCha20
        </text>
      </g>
    </svg>
  );
}

function BranchableIllustration() {
  return (
    <svg viewBox="0 0 200 80" className="h-full w-full">
      {/* main line */}
      <line
        x1="20"
        x2="180"
        y1="50"
        y2="50"
        stroke="var(--color-fg-subtle)"
        strokeWidth={1.5}
      />
      {/* draft branch */}
      <path
        d="M 70 50 C 80 50, 85 25, 110 25 L 140 25"
        stroke="var(--color-accent)"
        strokeWidth={1.5}
        fill="none"
        strokeDasharray="3 3"
      />
      <path
        d="M 140 25 C 155 25, 158 50, 165 50"
        stroke="var(--color-accent)"
        strokeWidth={1.5}
        fill="none"
      />
      {/* nodes */}
      {[
        { x: 30, y: 50, c: "var(--color-fg-muted)" },
        { x: 70, y: 50, c: "var(--color-fg-muted)" },
        { x: 110, y: 25, c: "var(--color-accent)" },
        { x: 140, y: 25, c: "var(--color-accent)" },
        { x: 165, y: 50, c: "var(--color-fg)" },
      ].map((n, i) => (
        <circle key={i} cx={n.x} cy={n.y} r={4} fill={n.c} />
      ))}
      <text
        x="110"
        y="15"
        textAnchor="middle"
        fontSize="9"
        fontFamily="var(--font-mono)"
        className="fill-accent-muted"
      >
        draft
      </text>
      <text
        x="30"
        y="70"
        fontSize="9"
        fontFamily="var(--font-mono)"
        className="fill-fg-subtle"
      >
        main
      </text>
      <text
        x="172"
        y="70"
        fontSize="9"
        fontFamily="var(--font-mono)"
        className="fill-fg-subtle"
      >
        merged
      </text>
    </svg>
  );
}

function SemanticIllustration() {
  const points = [
    { x: 50, y: 28, r: 2 },
    { x: 75, y: 60, r: 2 },
    { x: 110, y: 35, r: 2 },
    { x: 130, y: 50, r: 2 },
    { x: 155, y: 25, r: 2 },
    { x: 165, y: 55, r: 2 },
    { x: 90, y: 45, r: 4, hit: true },
    { x: 115, y: 52, r: 3, hit: true },
    { x: 100, y: 30, r: 3, hit: true },
  ];
  return (
    <svg viewBox="0 0 200 80" className="h-full w-full">
      {/* query */}
      <circle
        cx={100}
        cy={42}
        r={28}
        fill="none"
        stroke="var(--color-accent)"
        strokeOpacity={0.4}
        strokeDasharray="2 3"
      />
      <circle cx={100} cy={42} r={3} fill="var(--color-accent)" />
      <text
        x={100}
        y={14}
        textAnchor="middle"
        fontSize="9"
        fontFamily="var(--font-mono)"
        className="fill-accent-muted"
      >
        query
      </text>
      {points.map((p, i) => (
        <circle
          key={i}
          cx={p.x}
          cy={p.y}
          r={p.r}
          fill={p.hit ? "var(--color-accent)" : "var(--color-fg-subtle)"}
          opacity={p.hit ? 0.95 : 0.5}
        />
      ))}
    </svg>
  );
}
