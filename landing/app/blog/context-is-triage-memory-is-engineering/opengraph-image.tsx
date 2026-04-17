/**
 * Post-specific OG image for the launch post. Two panels side-by-side
 * telling the argument in one glance.
 *
 * Design constraints for 1200x630:
 *   - headline must carry first; diagram supports, not dominates
 *   - diagram must fit in ~300px vertical — single window per panel
 *   - LinkedIn / Slack / iMessage truncate below 600px on mobile crops,
 *     so keep the tagline above the fold
 *
 * Left panel compresses the compact story into one frame: a post-compact
 * window with 5 visible drops (line-through). Right panel shows the
 * open stack with the same facts retained, 3 pulled into "next session."
 * Same 8 facts on both sides — identical input, different retention.
 */

import { ImageResponse } from "next/og";

export const size = { width: 1200, height: 630 };
export const contentType = "image/png";
export const alt =
  "Context is triage. Memory is engineering. — hellodb blog post";
export const dynamic = "force-static";

const BG = "#1a1815";
const BG_SUNKEN = "#15130f";
const BG_ELEVATED = "#221f1b";
const FG = "#f1efea";
const FG_MUTED = "#a29c8e";
const FG_SUBTLE = "#6d6a60";
const BORDER = "#38342e";
const BORDER_STRONG = "#4d4840";
const ACCENT = "#e0a96d";
const ACCENT_DIM = "#8a6540";

const FACTS = [
  "pnpm over npm",
  "oauth, not sessions",
  "tabs over spaces",
  "use OKLCH",
  "dark by default",
  "Rust workspaces",
] as const;
// Which indices the model keeps after /compact (left panel) AND which
// top-k pulls into next session (right panel). Same 3 on both sides,
// intentionally — the argument is about *retention*, not what was right.
const KEPT = new Set([0, 3, 5]);

export default function Image() {
  return new ImageResponse(
    (
      <div
        style={{
          width: "100%",
          height: "100%",
          background: BG,
          display: "flex",
          flexDirection: "column",
          padding: "48px 72px",
          fontFamily: "ui-serif, Georgia, serif",
          position: "relative",
        }}
      >
        {/* subtle amber wash top-right */}
        <div
          style={{
            position: "absolute",
            inset: 0,
            background:
              "radial-gradient(55% 45% at 88% -5%, rgba(224,169,109,0.13), transparent 65%)",
          }}
        />

        {/* wordmark + blog-post badge */}
        <div
          style={{
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
            fontSize: 22,
          }}
        >
          <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
            <span style={{ color: ACCENT }}>›</span>
            <span style={{ color: FG }}>hellodb</span>
          </div>
          <div
            style={{
              display: "flex",
              alignItems: "center",
              padding: "5px 14px",
              border: `1px solid ${BORDER_STRONG}`,
              borderRadius: 999,
              color: FG_SUBTLE,
              fontSize: 14,
              letterSpacing: 2.5,
            }}
          >
            BLOG POST
          </div>
        </div>

        {/* headline */}
        <div
          style={{
            display: "flex",
            flexDirection: "column",
            marginTop: 18,
            fontSize: 62,
            lineHeight: 1.02,
            color: FG,
            letterSpacing: -1,
          }}
        >
          <div style={{ display: "flex" }}>Context is triage.</div>
          <div style={{ display: "flex" }}>
            <span style={{ fontStyle: "italic", color: ACCENT }}>
              Memory is engineering.
            </span>
          </div>
        </div>

        {/* diagram — two panels, single window each */}
        <div
          style={{
            display: "flex",
            marginTop: 26,
            gap: 24,
            flex: 1,
          }}
        >
          <ContextMini />
          <div
            style={{
              width: 1,
              background: BORDER,
              alignSelf: "stretch",
            }}
          />
          <MemoryMini />
        </div>

        {/* footer — URL + caption */}
        <div
          style={{
            marginTop: 20,
            display: "flex",
            justifyContent: "space-between",
            alignItems: "center",
            fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
            fontSize: 15,
            color: FG_SUBTLE,
            letterSpacing: 1.5,
          }}
        >
          <div style={{ display: "flex" }}>hellodb.dev/blog</div>
          <div style={{ display: "flex", color: FG_MUTED }}>
            SAME INPUT · DIFFERENT RETENTION
          </div>
        </div>
      </div>
    ),
    { ...size },
  );
}

/* ───────────────────────── left: CONTEXT ───────────────────────── */

function ContextMini() {
  return (
    <div
      style={{
        flex: 1,
        display: "flex",
        flexDirection: "column",
        gap: 10,
      }}
    >
      <PanelTitle eyebrow="CONTEXT" accent="triage" eyebrowColor={FG_SUBTLE} />

      <div
        style={{
          display: "flex",
          flexDirection: "column",
          gap: 4,
          padding: "12px 14px",
          background: BG_SUNKEN,
          border: `1px solid ${BORDER_STRONG}`,
          borderRadius: 6,
        }}
      >
        <div
          style={{
            display: "flex",
            justifyContent: "space-between",
            fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
            fontSize: 11,
            color: FG_SUBTLE,
            letterSpacing: 1.6,
            marginBottom: 4,
          }}
        >
          <span>AFTER /COMPACT</span>
          <span style={{ color: FG_MUTED }}>3 / 6 KEPT</span>
        </div>
        {FACTS.map((f, i) => {
          const kept = KEPT.has(i);
          return (
            <div
              key={f}
              style={{
                display: "flex",
                padding: "3px 10px",
                background: kept ? BG_ELEVATED : "transparent",
                border: `1px ${kept ? "solid" : "dashed"} ${
                  kept ? BORDER : "#38342e80"
                }`,
                borderRadius: 3,
                fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
                fontSize: 14,
                color: kept ? FG_MUTED : "#6d6a6099",
                textDecoration: kept ? "none" : "line-through",
              }}
            >
              {f}
            </div>
          );
        })}
      </div>

      <div
        style={{
          display: "flex",
          fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
          fontSize: 12,
          color: FG_SUBTLE,
          letterSpacing: 0.5,
        }}
      >
        summary dropped 3 facts. next prompt misses them.
      </div>
    </div>
  );
}

/* ───────────────────────── right: MEMORY ───────────────────────── */

function MemoryMini() {
  const hashes = ["a7f2", "e4c8", "9d11", "f03a", "2b6e", "5c44"];
  return (
    <div
      style={{
        flex: 1,
        display: "flex",
        flexDirection: "column",
        gap: 10,
      }}
    >
      <PanelTitle
        eyebrow="MEMORY"
        accent="engineering"
        eyebrowColor={ACCENT}
      />

      {/* open stack — no outer border, signaling unboundedness */}
      <div style={{ display: "flex", flexDirection: "column" }}>
        {FACTS.map((f, i) => {
          const pulled = KEPT.has(i);
          return (
            <div
              key={f}
              style={{
                display: "flex",
                alignItems: "center",
                gap: 10,
                padding: "4px 0",
                borderBottom: `1px solid ${BORDER}80`,
                fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
                fontSize: 14,
                color: pulled ? FG : FG_MUTED,
              }}
            >
              <div
                style={{
                  width: 3,
                  height: 16,
                  background: pulled ? ACCENT : BORDER_STRONG,
                  borderRadius: 2,
                }}
              />
              <div style={{ display: "flex", color: FG_SUBTLE, width: 64 }}>
                b3:{hashes[i]}
              </div>
              <div style={{ display: "flex" }}>{f}</div>
            </div>
          );
        })}
      </div>

      <div
        style={{
          display: "flex",
          fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
          fontSize: 12,
          color: ACCENT,
          letterSpacing: 0.5,
        }}
      >
        → top-k pulls 3 back for next session. nothing lost.
      </div>
    </div>
  );
}

/* ───────────────────────── shared ───────────────────────── */

function PanelTitle({
  eyebrow,
  accent,
  eyebrowColor,
}: {
  eyebrow: string;
  accent: string;
  eyebrowColor: string;
}) {
  return (
    <div
      style={{
        display: "flex",
        alignItems: "baseline",
        gap: 10,
        fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
        fontSize: 13,
        letterSpacing: 2.2,
        color: eyebrowColor,
      }}
    >
      <span>{eyebrow}</span>
      <span style={{ color: FG_MUTED, fontSize: 14 }}>·</span>
      <span
        style={{
          fontFamily: "ui-serif, Georgia, serif",
          fontStyle: "italic",
          fontSize: 22,
          color: FG,
          letterSpacing: 0,
        }}
      >
        {accent}
      </span>
    </div>
  );
}
