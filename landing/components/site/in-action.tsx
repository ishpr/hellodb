import { Section } from "./section";

export function InAction() {
  return (
    <Section
      eyebrow="in action"
      title={
        <>
          One line in chat. <span className="italic text-fg-muted">A fact for next session.</span>
        </>
      }
      lede="The /hellodb:memorize skill is loaded into every Claude Code session by the plugin. You don't think about it; the agent recognizes durable facts and writes them. The digest backend takes it from there."
    >
      <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
        <div className="min-w-0 rounded-[var(--radius-card)] border border-border bg-bg-sunken/60 p-6">
          <div className="mb-4 flex items-center justify-between">
            <div className="font-mono text-[11px] uppercase tracking-[0.16em] text-fg-subtle">
              session · capture
            </div>
            <div className="font-mono text-[10px] text-fg-subtle">t = 0ms</div>
          </div>
          <Bubble who="you">
            this project always uses pnpm, never npm or yarn. lockfile is
            committed.
          </Bubble>
          <ToolCall name="hellodb_remember">
{`{
  "namespace": "code/hellodb",
  "kind": "convention",
  "text": "use pnpm for this project (npm/yarn forbidden); pnpm-lock.yaml is committed",
  "evidence": "user-stated, 2026-04-16"
}`}
          </ToolCall>
          <Bubble who="claude">
            Got it — saved as a project convention. I&apos;ll use pnpm going
            forward and won&apos;t suggest npm scripts.
          </Bubble>
          <div className="mt-3 flex items-center gap-2 font-mono text-[11px] text-fg-subtle">
            <span className="inline-flex items-center gap-1.5 rounded-full border border-success/40 bg-success/10 px-2 py-0.5 text-success">
              <span className="h-1.5 w-1.5 rounded-full bg-success" />
              confidence 0.92 · auto-merged
            </span>
          </div>
          <p className="mt-4 font-mono text-[11px] leading-relaxed text-fg-muted">
            Write returns instantly. On the next Stop hook, memory-digest
            scores the fact and (because confidence ≥ 0.75) lands it straight
            on main. No review queue to clear.
          </p>
        </div>

        <div className="min-w-0 rounded-[var(--radius-card)] border border-border bg-bg-sunken/60 p-6">
          <div className="mb-4 flex items-center justify-between">
            <div className="font-mono text-[11px] uppercase tracking-[0.16em] text-accent-muted">
              new session · recall
            </div>
            <div className="font-mono text-[10px] text-fg-subtle">t = days later</div>
          </div>
          <Bubble who="you">add a script to install dependencies for the new contributor doc</Bubble>
          <ToolCall name="hellodb_recall">
{`{
  "namespace": "code/hellodb",
  "query": "package manager + install command",
  "top_k": 3
}`}
          </ToolCall>
          <ToolResult>
            {`→ 0.91  use pnpm for this project (npm/yarn forbidden)`}
            {"\n"}
            {`→ 0.78  pnpm-lock.yaml is committed`}
            {"\n"}
            {`→ 0.62  prefer pnpm dlx for one-shot tools`}
          </ToolResult>
          <Bubble who="claude">
            Adding it to the contributor doc. I&apos;ll use{" "}
            <code className="font-mono text-accent">pnpm install</code> per the
            project convention.
          </Bubble>
          <p className="mt-4 font-mono text-[11px] leading-relaxed text-fg-muted">
            Cosine similarity ranks hits; time-decay reinforcement boosts
            recently-touched facts. No round-trip to a cloud service.
          </p>
        </div>
      </div>
    </Section>
  );
}

function Bubble({
  who,
  children,
}: {
  who: "you" | "claude";
  children: React.ReactNode;
}) {
  const isYou = who === "you";
  return (
    <div
      className={`mb-3 flex items-start gap-2 ${
        isYou ? "flex-row-reverse" : "flex-row"
      }`}
    >
      <div
        aria-hidden="true"
        className={`mt-1 flex h-6 w-6 shrink-0 items-center justify-center rounded-full font-mono text-[10px] ${
          isYou
            ? "bg-accent/15 text-accent ring-1 ring-accent/30"
            : "bg-bg-elevated text-fg-muted ring-1 ring-border"
        }`}
      >
        {isYou ? "›" : "C"}
      </div>
      <div
        className={`max-w-[85%] rounded-2xl px-4 py-2.5 text-[13.5px] leading-relaxed ${
          isYou
            ? "rounded-tr-sm bg-accent/[0.08] text-fg"
            : "rounded-tl-sm bg-bg-elevated/70 text-fg-muted"
        }`}
      >
        <div
          className={`mb-1 font-mono text-[10px] uppercase tracking-[0.14em] ${
            isYou ? "text-accent-muted" : "text-fg-subtle"
          }`}
        >
          {who}
        </div>
        {children}
      </div>
    </div>
  );
}

function ToolCall({
  name,
  children,
}: {
  name: string;
  children: React.ReactNode;
}) {
  return (
    <div className="my-3 overflow-hidden rounded-lg border border-border bg-bg-elevated/40">
      <div className="flex items-center justify-between border-b border-border/60 px-3 py-1.5 font-mono text-[10px] uppercase tracking-[0.14em]">
        <span className="text-accent-muted">tool call</span>
        <span className="text-fg-subtle">{name}</span>
      </div>
      <pre className="overflow-x-auto px-3 py-2 font-mono text-[11.5px] leading-relaxed text-fg-muted">
        {children}
      </pre>
    </div>
  );
}

function ToolResult({ children }: { children: React.ReactNode }) {
  return (
    <div className="my-3 overflow-hidden rounded-lg border border-accent/20 bg-accent/[0.04]">
      <div className="border-b border-accent/15 px-3 py-1.5 font-mono text-[10px] uppercase tracking-[0.14em] text-accent-muted">
        tool result
      </div>
      <pre className="overflow-x-auto px-3 py-2 font-mono text-[11.5px] leading-relaxed text-fg">
        {children}
      </pre>
    </div>
  );
}
