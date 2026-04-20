import Link from "next/link";
import { Terminal, Prompt } from "./code-block";
import { HeroPipeline } from "./hero-pipeline";
import { HeroFiletree } from "./hero-filetree";

export function Hero() {
  return (
    <section className="relative w-full overflow-hidden px-6 pb-12 pt-16 md:px-10 md:pb-16 md:pt-24">
      <div className="pointer-events-none absolute inset-x-0 top-0 -z-10 h-[600px] [background:radial-gradient(60%_60%_at_50%_0%,var(--color-accent-glow),transparent_70%)]" />

      <div className="mx-auto grid max-w-6xl grid-cols-1 items-start gap-14 lg:grid-cols-[1.1fr_1fr] lg:gap-16">
        <div className="flex min-w-0 flex-col">
          <div className="mb-6 inline-flex w-fit items-center gap-2 rounded-full border border-border bg-bg-elevated/60 px-3 py-1 font-mono text-[11px] tracking-tight text-fg-muted">
            <span className="h-1.5 w-1.5 animate-[pulse-dot_2.5s_ease-in-out_infinite] rounded-full bg-accent" />
            v0.1.0 — phase 1 shipped
          </div>

          <h1 className="font-display text-[44px] leading-[1] tracking-tight text-fg text-balance sm:text-[64px] sm:leading-[0.95] lg:text-[80px]">
            Sovereign memory<br />
            for <span className="italic text-accent">Claude Code.</span>
          </h1>

          <p className="mt-6 max-w-xl text-lg leading-relaxed text-fg-muted text-pretty">
            Local-first. End-to-end encrypted. Branchable. Two plugin agents
            distill your sessions into facts — high-confidence ones merge
            auto, uncertain ones wait for one-click review. You own the
            keys, the data, and the bill.
          </p>

          <div className="mt-8 w-full max-w-md">
            <Terminal label="install">
              <div className="flex flex-col gap-1.5">
                <Prompt comment="downloads · inits · registers Claude plugin">
                  curl -fsSL hellodb.dev/install | sh
                </Prompt>
                <div className="ml-5 text-fg-subtle">
                  {"# done. Memory lives in ~/.hellodb on your next session."}
                </div>
              </div>
            </Terminal>
          </div>

          <div className="mt-7 flex flex-wrap items-center gap-3">
            <Link
              href="#install"
              className="inline-flex h-11 items-center gap-2 rounded-full bg-accent px-5 font-mono text-[13px] font-medium text-bg transition-colors hover:bg-accent-muted"
            >
              install →
            </Link>
            <a
              href="https://github.com/eprasad7/hellodb"
              target="_blank"
              rel="noopener noreferrer"
              className="inline-flex h-11 items-center gap-2 rounded-full border border-border bg-bg-elevated px-5 font-mono text-[13px] text-fg transition-colors hover:border-border-strong"
            >
              github ↗
            </a>
          </div>
          <div className="mt-4 font-mono text-[12px] text-fg-muted">
            MIT · Rust · MCP-native · macOS · Linux · Windows
          </div>
        </div>

        <div className="flex min-w-0 flex-col gap-5">
          <HeroPipeline />
          <HeroFiletree />
        </div>
      </div>

      <div className="mx-auto mt-10 max-w-6xl md:mt-14">
        <div className="grid grid-cols-2 gap-4 rounded-[var(--radius-card)] border border-border bg-bg-elevated/30 p-4 sm:grid-cols-4 sm:gap-3 md:p-6">
          {[
            { value: "0ms", label: "write path" },
            { value: "384d", label: "semantic recall" },
            { value: "~$0", label: "monthly cost" },
            { value: "0", label: "cloud lock-in" },
          ].map((s) => (
            <div
              key={s.label}
              className="flex flex-col items-start gap-1 px-1 py-2 sm:px-3"
            >
              <div className="font-display text-[28px] leading-none text-accent sm:text-3xl md:text-4xl">
                {s.value}
              </div>
              <div className="font-mono text-[10px] uppercase tracking-[0.16em] text-fg-muted sm:text-[11px]">
                {s.label}
              </div>
            </div>
          ))}
        </div>
      </div>
    </section>
  );
}
