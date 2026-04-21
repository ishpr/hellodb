import { ImageResponse } from "next/og";

export const size = { width: 1200, height: 630 };
export const contentType = "image/png";
export const alt = "hellodb — sovereign memory for agents (MCP · Claude Code)";
export const dynamic = "force-static";

const BG = "#1a1815";
const FG = "#f1efea";
const FG_MUTED = "#a29c8e";
const FG_SUBTLE = "#8b8779";
const ACCENT = "#e0a96d";

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
          padding: "64px 80px",
          fontFamily: "ui-serif, Georgia, serif",
          position: "relative",
        }}
      >
        <div
          style={{
            position: "absolute",
            inset: 0,
            background:
              "radial-gradient(60% 50% at 80% 0%, rgba(224,169,109,0.18), transparent 70%)",
          }}
        />

        {/* Top — wordmark row */}
        <div
          style={{
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
            fontSize: 26,
          }}
        >
          <div
            style={{
              display: "flex",
              alignItems: "center",
              gap: 14,
              color: FG,
            }}
          >
            <span style={{ color: ACCENT }}>›</span>
            <span>hellodb</span>
            <span style={{ color: FG_SUBTLE, fontSize: 20, marginLeft: 16 }}>
              v0.1.1
            </span>
          </div>
          <div style={{ display: "flex", color: FG_SUBTLE, fontSize: 20 }}>
            MCP · Claude Code plugin · stdio hellodb-mcp
          </div>
        </div>

        {/* Middle — headline */}
        <div
          style={{
            display: "flex",
            flexDirection: "column",
            marginTop: 72,
            fontSize: 112,
            lineHeight: 0.98,
            color: FG,
            letterSpacing: -1.5,
          }}
        >
          <div style={{ display: "flex" }}>Sovereign memory</div>
          <div style={{ display: "flex", gap: 28 }}>
            <span>for</span>
            <span style={{ fontStyle: "italic", color: ACCENT }}>agents.</span>
          </div>
        </div>

        {/* Tagline — single line */}
        <div
          style={{
            display: "flex",
            marginTop: 36,
            fontSize: 28,
            color: FG_MUTED,
          }}
        >
          Local-first. Encrypted. You review only the uncertain.
        </div>

        {/* Bottom — install pill + tech chips */}
        <div
          style={{
            marginTop: "auto",
            display: "flex",
            justifyContent: "space-between",
            alignItems: "center",
            fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
            fontSize: 22,
          }}
        >
          <div
            style={{
              display: "flex",
              alignItems: "center",
              gap: 14,
              padding: "14px 22px",
              border: `1px solid ${ACCENT}`,
              borderRadius: 999,
              color: ACCENT,
            }}
          >
            <span>$</span>
            <span>curl -fsSL hellodb.dev/install | sh</span>
          </div>
          <div
            style={{
              display: "flex",
              gap: 18,
              color: FG_SUBTLE,
              fontSize: 18,
            }}
          >
            <span>MIT</span>
            <span>·</span>
            <span>Rust</span>
            <span>·</span>
            <span>MCP</span>
            <span>·</span>
            <span>~$0/mo</span>
          </div>
        </div>
      </div>
    ),
    { ...size },
  );
}
