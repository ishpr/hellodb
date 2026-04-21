"use client";

import { useEffect } from "react";

export function ConsoleSig() {
  useEffect(() => {
    if (typeof window === "undefined") return;
    if ((window as { __hellodbSig?: boolean }).__hellodbSig) return;
    (window as { __hellodbSig?: boolean }).__hellodbSig = true;
    const accent = "color:#e0a96d;font-weight:600;font-family:ui-monospace,Menlo,monospace";
    const muted = "color:#8b8779;font-family:ui-monospace,Menlo,monospace";
    const fg = "color:#f1efea;font-family:ui-monospace,Menlo,monospace";
    /* eslint-disable no-console */
    console.log("%c› hellodb", accent);
    console.log("%csovereign memory for agents · MCP · Claude Code plugin", fg);
    console.log("%c—", muted);
    console.log(
      "%cthis page was built in one session, in claude code,\nwith the same memory layer it markets.",
      muted,
    );
    console.log("%c$ curl -fsSL hellodb.dev/install | sh", accent);
    console.log("%c—", muted);
    console.log("%cgithub: https://github.com/ishpr/hellodb", muted);
    /* eslint-enable no-console */
  }, []);
  return null;
}
