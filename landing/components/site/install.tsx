import { Section } from "./section";
import { Terminal, Prompt } from "./code-block";

export function Install() {
  return (
    <Section
      id="install"
      eyebrow="install"
      title={
        <>
          One command. <span className="italic text-fg-muted">Then forget it&apos;s there.</span>
        </>
      }
      lede="The local install is encrypted, branchable memory in 30 seconds. Cloudflare gateway is opt-in for cross-device sync and remote embeddings — same memory, more devices, no API token to babysit."
    >
      <div className="grid grid-cols-1 gap-6 lg:grid-cols-2">
        <div className="min-w-0">
          <div className="mb-3 flex items-center justify-between">
            <h3 className="font-mono text-[13px] uppercase tracking-[0.16em] text-fg-subtle">
              local · always works offline
            </h3>
            <span className="font-mono text-[10px] text-fg-subtle">
              ~30 seconds
            </span>
          </div>
          <Terminal label="curl | sh">
            <div className="flex flex-col gap-1.5">
              <Prompt comment="downloads · inits · registers Claude plugin">
                curl -fsSL hellodb.dev/install | sh
              </Prompt>
              <div className="ml-5 text-fg-subtle">
                {"# Windows: iwr hellodb.dev/install | iex"}
              </div>
              <div className="mt-2 ml-5 text-fg-subtle">
                {"# done. Memory lives in ~/.hellodb on your next Claude session."}
              </div>
            </div>
          </Terminal>
          <p className="mt-3 font-mono text-[12px] leading-relaxed text-fg-muted">
            One script. Detects your platform, fetches the right release,
            generates an Ed25519 identity. Installs{" "}
            <span className="text-accent">5 skills</span>,{" "}
            <span className="text-accent">2 plugin agents</span>{" "}
            (memory-digest + memory-consolidate), a Stop hook, and{" "}
            <span className="text-accent">22 MCP tools</span> covering
            namespaces, schemas, branches, vector upsert/recall, embed, and
            Claude Code memory interop.
          </p>
        </div>

        <div className="min-w-0">
          <div className="mb-3 flex items-center justify-between">
            <h3 className="font-mono text-[13px] uppercase tracking-[0.16em] text-fg-subtle">
              cloudflare · cross-device + semantic
            </h3>
            <span className="font-mono text-[10px] text-fg-subtle">
              ~3 minutes · OAuth, no token
            </span>
          </div>
          <Terminal label="make setup-cloudflare">
            <div className="flex flex-col gap-1.5">
              <Prompt comment="opens browser → wrangler login (CF OAuth)">
                make setup-cloudflare
              </Prompt>
              <div className="ml-5 text-fg-subtle">
                {"  ↳ creates R2 bucket            (idempotent)"}
              </div>
              <div className="ml-5 text-fg-subtle">
                {"  ↳ generates GATEWAY_TOKEN      (Worker secret)"}
              </div>
              <div className="ml-5 text-fg-subtle">
                {"  ↳ wrangler deploy ./gateway    (your account)"}
              </div>
              <div className="ml-5 text-fg-subtle">
                {"  ↳ writes ~/.hellodb/env.sh + sources from rc"}
              </div>
              <div className="mt-2 ml-5 text-fg-subtle">
                {"# done. Your worker. Your bucket. ~$0 free tier."}
              </div>
            </div>
          </Terminal>
          <p className="mt-3 font-mono text-[12px] leading-relaxed text-fg-muted">
            wrangler stores the OAuth token in your OS keychain — hellodb never
            sees it. Rotate the gateway bearer anytime with{" "}
            <span className="text-accent">make rotate-gateway-token</span>.
          </p>
        </div>
      </div>

      <div className="mt-10 grid gap-4 md:grid-cols-2">
        <div className="rounded-xl border border-border bg-bg-elevated/30 p-5">
          <div className="mb-2 font-mono text-[11px] uppercase tracking-[0.16em] text-accent-muted">
            alternative · build from source
          </div>
          <div className="font-mono text-[13px] text-fg-muted [overflow-wrap:anywhere]">
            <span className="select-none text-accent">$</span> git clone
            https://github.com/eprasad7/hellodb &amp;&amp; cd hellodb &amp;&amp;
            make onboard
          </div>
          <p className="mt-2 font-mono text-[11px] leading-relaxed text-fg-muted">
            Detects Rust, builds release, bundles the plugin, registers it with
            Claude Code, runs <span className="text-accent">hellodb init</span>,
            and offers (y/N) the Cloudflare setup. One prompt, one install.
          </p>
        </div>

        <div className="rounded-xl border border-border bg-bg-elevated/30 p-5">
          <div className="mb-2 font-mono text-[11px] uppercase tracking-[0.16em] text-accent-muted">
            bonus · import existing Claude Code memory
          </div>
          <div className="font-mono text-[13px] text-fg-muted [overflow-wrap:anywhere]">
            <span className="select-none text-accent">$</span> hellodb ingest
            --from-claudemd
          </div>
          <p className="mt-2 font-mono text-[11px] leading-relaxed text-fg-muted">
            Walks <span className="text-accent">~/.claude/projects/*/memory/*.md</span>,
            writes one signed record per file, dedupes on re-run. Query back
            via <span className="text-accent">hellodb_find_relevant_memories</span>{" "}
            from any MCP client — hybrid ranking, decay-aware.
          </p>
        </div>
      </div>
    </Section>
  );
}
