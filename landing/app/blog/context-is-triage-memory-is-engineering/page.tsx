import type { Metadata } from "next";
import Link from "next/link";
import { Nav } from "@/components/site/nav";
import { Footer } from "@/components/site/footer";
import { ContextVsMemory } from "@/components/site/context-vs-memory";

const TITLE = "Context is triage. Memory is engineering.";
const DESCRIPTION =
  "Anthropic just published a clear-eyed post on Claude Code session management. It names three problems — context rot, lossy /compact, hand-written /clear briefs. hellodb was built to make each of those problems go away.";
const DATE = "2026-04-17";
const CANONICAL =
  "https://hellodb.dev/blog/context-is-triage-memory-is-engineering";

export const metadata: Metadata = {
  title: TITLE,
  description: DESCRIPTION,
  alternates: { canonical: CANONICAL },
  openGraph: {
    title: TITLE,
    description: DESCRIPTION,
    type: "article",
    url: CANONICAL,
    publishedTime: `${DATE}T00:00:00Z`,
    authors: ["Ish Prasad"],
  },
  twitter: {
    card: "summary_large_image",
    title: TITLE,
    description: DESCRIPTION,
  },
};

export default function Post() {
  return (
    <>
      <Nav />
      <main className="flex flex-col">
        <article className="relative mx-auto w-full max-w-3xl px-6 pt-16 pb-24 md:px-8 md:pt-24 md:pb-32">
          {/* header */}
          <header className="border-b border-border pb-10">
            <div className="flex items-center gap-3 font-mono text-[12px] text-fg-subtle">
              <Link
                href="/blog"
                className="text-fg-muted transition-colors hover:text-accent"
              >
                ← blog
              </Link>
              <span aria-hidden="true">·</span>
              <time dateTime={DATE}>{formatDate(DATE)}</time>
              <span aria-hidden="true">·</span>
              <span>8 min read</span>
            </div>
            <h1 className="mt-5 font-display text-[36px] leading-[1.05] tracking-tight text-balance text-fg md:text-[52px]">
              Context is triage.{" "}
              <span className="italic text-accent">Memory is engineering.</span>
            </h1>
            <p className="mt-6 text-[17px] leading-relaxed text-fg-muted text-pretty md:text-[18px]">
              Anthropic just published{" "}
              <a
                href="https://claude.com/blog/using-claude-code-session-management-and-1m-context"
                target="_blank"
                rel="noopener noreferrer"
                className="text-accent underline decoration-accent/40 underline-offset-4 transition-colors hover:decoration-accent"
              >
                a clear-eyed post on Claude Code session management
              </a>
              . It&rsquo;s an honest acknowledgment that{" "}
              <em>session management, not raw context size, is what
              determines output quality</em>. It names three problems. hellodb
              was built to make each of them go away.
            </p>
          </header>

          {/* body */}
          <div className="prose-custom mt-10 flex flex-col gap-8 text-[16px] leading-[1.75] text-fg-muted md:text-[17px]">
            <Section>
              <p>
                Read the post. It&rsquo;s worth reading in full &mdash; less for
                the announcement (the new <code>/usage</code> command is
                useful but modest) and more for the quiet concession underneath:
                the team building Claude Code is openly admitting what their
                heaviest users have been quietly patching around for months.
              </p>
              <p>
                Here&rsquo;s the thesis of the post, compressed to one sentence:
              </p>
              <blockquote>
                Context rot, lossy compaction, and manual session briefs are
                real; here are the primitives we give you to survive them.
              </blockquote>
              <p>
                Every word of that is true. None of those primitives are the
                solution.
              </p>
            </Section>

            <ContextVsMemory />

            <Section>
              <H2>Problem 1 &mdash; context rot</H2>
              <p>
                Direct quote:
              </p>
              <blockquote>
                &ldquo;Context rot is the observation that model performance
                degrades as context grows, because attention gets spread across
                more tokens and older, irrelevant content starts to
                distract.&rdquo;
              </blockquote>
              <p>
                This is a fundamental property of transformer attention. No
                amount of context-window expansion fixes it &mdash; 1M tokens
                rots more than 200K tokens, just later. The post&rsquo;s own
                advice follows: keep the window lean; start a new session when
                you start a new task.
              </p>
              <p>
                Good advice. But it pushes a cost onto the user: every new
                session means re-loading the state you need. Your stack. Your
                conventions. The decision you made three sessions ago about
                why you&rsquo;re not using Redux. The reason you picked{" "}
                <code>pnpm</code> over <code>npm</code>. Claude forgets all of
                it the moment you <code>/clear</code>.
              </p>
              <p>
                <strong>hellodb&rsquo;s answer:</strong> durable facts
                don&rsquo;t live in your context window. They live in{" "}
                <code>~/.hellodb/local.db</code> &mdash; SQLCipher-encrypted,
                content-addressed, Ed25519-signed. At session start, Claude
                calls <code>hellodb_find_relevant_memories</code> and gets the
                top-<em>k</em> memories your current task actually needs,
                ranked by semantic similarity times reinforcement decay. The
                window stays lean because the store is the source of truth.
              </p>
            </Section>

            <Section>
              <H2>Problem 2 &mdash; <code>/compact</code> is lossy</H2>
              <p>
                From the post:
              </p>
              <blockquote>
                &ldquo;Autocompact fires after a long debugging session and
                summarizes the investigation, and your next message is &lsquo;now
                fix that other warning we saw in bar.ts&rsquo; &mdash; the
                other warning might have been dropped from the summary.&rdquo;
              </blockquote>
              <p>
                The team also concedes a harder constraint: <em>the model is
                at its least intelligent point when compacting</em>. At the
                moment you most need a careful summary, the model has the least
                capacity to produce one.
              </p>
              <p>
                This is a structural problem with context-as-memory. A summary
                is a compression of a summary is a compression of a summary.
                Information entropy only moves in one direction.
              </p>
              <p>
                <strong>hellodb&rsquo;s answer:</strong> facts are{" "}
                <em>immutable and content-addressed</em>. A compact pass
                can&rsquo;t drop a fact because the fact was never in the
                context &mdash; it&rsquo;s in the store, under a BLAKE3 hash
                that will never change. Compact as aggressively as you want.
                Nothing load-bearing lives in the window long enough to lose.
              </p>
              <p>
                The digest runs <em>out of band</em>: after your session ends,
                a digest backend reads the raw episode tail and writes
                consolidated facts to a draft branch (<code>claude.facts/
                digest-&lt;ts&gt;</code>). High-confidence facts auto-merge to{" "}
                <code>main</code>. Low-confidence or contradictory ones stay on
                the draft for you to review. Your primary session never sees
                the messy intermediate step.
              </p>
            </Section>

            <Section>
              <H2>Problem 3 &mdash; <code>/clear</code> demands a brief</H2>
              <p>
                The post&rsquo;s recommendation for <code>/clear</code>:
              </p>
              <blockquote>
                &ldquo;Start fresh session with user-written brief.&rdquo;
              </blockquote>
              <p>
                Three words. Enormous amount of cognitive load hidden inside
                &ldquo;user-written brief.&rdquo; Every time you{" "}
                <code>/clear</code>, <em>you</em> are the compaction function.
                You are sitting there typing out &ldquo;we&rsquo;re working on
                the auth refactor, remember we&rsquo;re using oauth not sessions,
                and I need you to&nbsp;...&rdquo; That&rsquo;s the state your
                last session spent an hour getting right.
              </p>
              <p>
                <strong>hellodb&rsquo;s answer:</strong> the brief already
                exists. Across past sessions, durable facts were captured
                automatically (via the <code>memorize</code> skill) or harvested
                from your existing <code>CLAUDE.md</code> files (via{" "}
                <code>hellodb ingest --from-claudemd</code>). You{" "}
                <code>/clear</code>; Claude invokes{" "}
                <code>hellodb_find_relevant_memories</code>; the top-8 relevant
                facts are re-seeded. No brief to write.
              </p>
              <p>
                The retrieval tool mirrors Claude Code&rsquo;s own memory-manifest
                shape &mdash; <code>type</code> (user / feedback / project /
                reference), <code>description</code>, <code>source_path</code>
                , <code>decayed_score</code>. It falls back gracefully: if you
                haven&rsquo;t configured an embedding backend, it ranks by
                keyword overlap + reinforcement decay instead. Always returns
                something; never errors for missing config.
              </p>
            </Section>

            <Section>
              <H2>The 1M context tell</H2>
              <p>
                Near the end of the post:
              </p>
              <blockquote>
                &ldquo;With one million context, you have more time to{" "}
                <code>/compact</code> proactively with a description.&rdquo;
              </blockquote>
              <p>
                Read that carefully. 1M context doesn&rsquo;t solve the
                compaction problem. It gives you <em>more time</em> before you
                hit it. The compaction is still coming. The summary is still
                lossy. The primitives are still triage.
              </p>
              <p>
                Context is triage. Memory is engineering.
              </p>
              <p>
                A context window is what fits in the model&rsquo;s attention
                right now. Memory is what survives every session you&rsquo;ve
                ever had, queryable, branchable, auditable, reinforceable. They
                are <em>different categories of thing</em>. Anthropic&rsquo;s
                post is about surviving the first one; hellodb is about owning
                the second.
              </p>
            </Section>

            <Section>
              <H2>What we built, in one screen</H2>
              <p>
                hellodb ships as a Claude Code plugin plus a local MCP server.
                You install it with one command:
              </p>
              <pre>
                <code>curl -fsSL hellodb.dev/install | sh</code>
              </pre>
              <p>
                The installer drops three binaries into{" "}
                <code>/usr/local/bin</code> (<code>hellodb</code>,{" "}
                <code>hellodb-mcp</code>, <code>hellodb-brain</code>),
                generates an Ed25519 identity key, opens an encrypted SQLite
                database at <code>~/.hellodb/local.db</code>, and registers the
                plugin with Claude Code if <code>claude</code> is on your PATH.
              </p>
              <p>
                On first run:
              </p>
              <pre>
                <code>hellodb ingest --from-claudemd</code>
              </pre>
              <p>
                ...scans <code>~/.claude/projects/*/memory/*.md</code> and
                imports every memory file into a per-project namespace (
                <code>claude.memory.&lt;project-slug&gt;</code>). Each project
                is hard-isolated &mdash; memories from your auth refactor repo
                never leak into your side-project repo. Content-addressing
                dedupes, so re-running is a no-op on unchanged files.
              </p>
              <p>
                From there, every session:
              </p>
              <ol>
                <li>
                  <strong>Primary agent</strong> writes episodes as they happen
                  (via the <code>memorize</code> skill or any{" "}
                  <code>hellodb_note</code> / <code>hellodb_remember</code>{" "}
                  call).
                </li>
                <li>
                  <strong>Stop hook</strong> fires when the session ends,
                  idempotent + cool-down-gated.
                </li>
                <li>
                  <strong>Brain daemon</strong> tails the episode namespace and
                  digests new material via the configured digest backend.
                  High-confidence facts auto-merge; low-confidence or
                  contradictory ones stay on a draft branch for review.
                </li>
                <li>
                  <strong>Next session</strong>, on any topic, in any repo,
                  calls <code>hellodb_find_relevant_memories</code> and pulls
                  the top-<em>k</em> curated facts back into context &mdash;
                  no window bloat, no <code>/compact</code> roulette.
                </li>
              </ol>
              <p>
                That&rsquo;s the loop. That&rsquo;s the whole pitch.
              </p>
            </Section>

            <Section>
              <H2>On owning it</H2>
              <p>
                One more thing worth saying explicitly.
              </p>
              <p>
                Everything above is open source, MIT-licensed, local-first. The
                DB lives on your machine, encrypted with a key derived from an
                identity you own. The signing key never leaves disk. Optional
                semantic search runs on <em>your</em> Cloudflare account via
                Workers AI &mdash; no shared service, no affiliate middleman,
                no API key you don&rsquo;t control.
              </p>
              <p>
                The reason I care about this: if I spend all day pair-programming
                with Claude, my memory of that work should live somewhere I own.
                Not in a context window that&rsquo;s rented and then garbage-
                collected. Not in a cloud service that might deprecate the
                retention policy. On my disk, under my key, in my format.
              </p>
              <p>
                Anthropic&rsquo;s post is honest about the limits of context.
                hellodb is what you build when you take that honesty seriously.
              </p>
            </Section>

            <footer className="mt-6 rounded-[var(--radius-card)] border border-accent/30 bg-accent/5 p-6 md:p-8">
              <div className="font-mono text-[11px] uppercase tracking-[0.16em] text-accent">
                try it
              </div>
              <p className="mt-3 text-[15px] leading-relaxed text-fg md:text-[16px]">
                One-line install. Apple Silicon, Linux x64/arm64, Windows x64.
                Auto-registers as a Claude Code plugin.
              </p>
              <pre className="mt-4 overflow-x-auto rounded-[10px] border border-border bg-bg-sunken px-4 py-3 font-mono text-[13px] text-fg">
                <code>curl -fsSL hellodb.dev/install | sh</code>
              </pre>
              <div className="mt-5 flex flex-wrap gap-3 font-mono text-[13px]">
                <a
                  href="https://github.com/ishpr/hellodb"
                  target="_blank"
                  rel="noopener noreferrer"
                  className="inline-flex h-10 items-center rounded-full border border-border px-4 text-fg-muted transition-colors hover:border-accent hover:text-accent"
                >
                  github →
                </a>
                <Link
                  href="/"
                  className="inline-flex h-10 items-center rounded-full border border-accent/40 bg-accent/10 px-4 text-accent transition-colors hover:border-accent hover:bg-accent/15"
                >
                  how it works →
                </Link>
              </div>
            </footer>
          </div>
        </article>
      </main>
      <Footer />
    </>
  );
}

/* ---------- tiny helpers so the content stays readable ------------------ */

function Section({ children }: { children: React.ReactNode }) {
  return <section className="flex flex-col gap-5">{children}</section>;
}

function H2({ children }: { children: React.ReactNode }) {
  return (
    <h2 className="mt-4 font-display text-[26px] leading-tight tracking-tight text-fg md:text-[32px]">
      {children}
    </h2>
  );
}

function formatDate(iso: string): string {
  const d = new Date(iso + "T00:00:00Z");
  return d.toLocaleDateString("en-US", {
    year: "numeric",
    month: "short",
    day: "numeric",
    timeZone: "UTC",
  });
}
