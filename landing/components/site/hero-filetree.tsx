export function HeroFiletree() {
  const rows: { name: string; note: string; tone?: "accent" | "muted" }[] = [
    { name: "identity.key", note: "ed25519 · in OS keychain", tone: "accent" },
    { name: "db.sqlite", note: "SQLCipher · ChaCha20-Poly1305" },
    { name: "vec/code/hellodb/", note: "encrypted vector index, 384d" },
    { name: "branches/main/", note: "auto-merged facts (≥ 0.75)" },
    { name: "branches/digest-*/", note: "uncertain — awaiting review", tone: "accent" },
    { name: "brain.toml", note: "digest gates, decay tuning" },
    { name: "wal/", note: "experimental WAL utilities" },
  ];

  return (
    <div className="relative w-full overflow-hidden rounded-[var(--radius-card)] border border-border bg-bg-sunken/60 p-6">
      <div className="mb-4 flex items-center justify-between">
        <div className="font-mono text-[11px] uppercase tracking-[0.18em] text-fg-muted">
          ~/.hellodb
        </div>
        <div className="font-mono text-[11px] text-fg-muted">your machine</div>
      </div>

      <div className="overflow-hidden rounded-md border border-border bg-bg-elevated/40 font-mono text-[12.5px]">
        {rows.map((r, i) => (
          <div
            key={r.name}
            className={`flex flex-col gap-0.5 px-4 py-2 sm:flex-row sm:items-center sm:justify-between sm:gap-4 ${
              i < rows.length - 1 ? "border-b border-border/40" : ""
            }`}
          >
            <div className="flex items-center gap-2 text-fg">
              <span className="select-none text-fg-subtle">└</span>
              <span className={r.tone === "accent" ? "text-accent" : "text-fg"}>
                {r.name}
              </span>
            </div>
            <span className="ml-5 text-[11px] text-fg-subtle sm:ml-0 sm:shrink-0 sm:text-right">
              {r.note}
            </span>
          </div>
        ))}
      </div>

      <p className="mt-4 font-mono text-[11px] leading-relaxed text-fg-muted">
        Everything hellodb writes lives here. No telemetry, no shadow uploads.
        Delete the folder and it&apos;s gone — keys included.
      </p>
    </div>
  );
}
