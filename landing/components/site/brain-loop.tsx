import { Section } from "./section";

export function BrainLoop() {
  const steps = [
    { label: "write", time: "0ms", body: "hellodb_note returns instantly. The agent never waits." },
    { label: "digest", time: "pluggable backend", body: "memory-digest extracts facts from episodes and scores each with a confidence." },
    { label: "score", time: "≥ 0.75 auto", body: "High confidence → merges straight to main. Low or superseding → held for review." },
    { label: "review", time: "edge cases only", body: "/hellodb:review shows only the uncertain ones. Empty most days." },
    { label: "recall", time: "semantic", body: "Future sessions hit merged facts via vector recall + decay reinforcement." },
  ];

  return (
    <Section
      eyebrow="the loop"
      title={
        <>
          Install it. <span className="italic text-accent">Forget it&apos;s there.</span>
          <br />
          You only review what&apos;s uncertain.
        </>
      }
      lede="Two plugin agents do the work. memory-digest scores every fact; high-confidence merges auto-land on main, low-confidence and superseding edits wait for your review. Default threshold 0.75, tunable in brain.toml. Backend can stay local/deterministic or use a configured remote model."
    >
      <div className="rounded-[var(--radius-card)] border border-border bg-bg-sunken/60 p-6 sm:p-8">
        {/* Desktop: horizontal track */}
        <div className="hidden md:block">
          <svg viewBox="0 0 1000 90" className="h-auto w-full">
            <line
              x1="60"
              x2="940"
              y1="45"
              y2="45"
              stroke="var(--color-border-strong)"
              strokeWidth={1}
              strokeDasharray="4 6"
            />
            <line
              x1="60"
              x2="940"
              y1="45"
              y2="45"
              stroke="var(--color-accent)"
              strokeWidth={2}
              strokeDasharray="32 16"
              opacity={0.7}
              style={{ animation: "dash-march 2.4s linear infinite" }}
            />
            {steps.map((s, i) => {
              const x = 60 + i * (880 / (steps.length - 1));
              return (
                <g key={s.label}>
                  <circle
                    cx={x}
                    cy={45}
                    r={9}
                    fill="var(--color-bg-sunken)"
                    stroke="var(--color-accent)"
                    strokeWidth={1.5}
                  />
                  <circle cx={x} cy={45} r={3} fill="var(--color-accent)" />
                  <text
                    x={x}
                    y={20}
                    textAnchor="middle"
                    className="fill-fg font-mono"
                    style={{ fontSize: 13 }}
                  >
                    {s.label}
                  </text>
                  <text
                    x={x}
                    y={78}
                    textAnchor="middle"
                    className="fill-fg-subtle font-mono"
                    style={{ fontSize: 10 }}
                  >
                    {s.time}
                  </text>
                </g>
              );
            })}
          </svg>
          <div className="mt-6 grid grid-cols-5 gap-3">
            {steps.map((s) => (
              <div key={s.label} className="text-[13px] leading-relaxed text-fg-muted">
                {s.body}
              </div>
            ))}
          </div>
        </div>

        {/* Mobile: vertical stepper */}
        <ol className="flex flex-col gap-5 md:hidden">
          {steps.map((s, i) => (
            <li key={s.label} className="flex gap-4">
              <div className="flex flex-col items-center">
                <div className="flex h-7 w-7 items-center justify-center rounded-full border border-accent bg-bg-sunken font-mono text-[11px] text-accent">
                  {i + 1}
                </div>
                {i < steps.length - 1 && (
                  <div className="mt-1 h-full w-px bg-border" />
                )}
              </div>
              <div>
                <div className="flex items-baseline gap-3">
                  <span className="font-mono text-sm text-fg">{s.label}</span>
                  <span className="font-mono text-[10px] text-fg-subtle">
                    {s.time}
                  </span>
                </div>
                <div className="mt-1 text-[13px] leading-relaxed text-fg-muted">
                  {s.body}
                </div>
              </div>
            </li>
          ))}
        </ol>
      </div>
    </Section>
  );
}
