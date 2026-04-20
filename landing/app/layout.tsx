import type { Metadata, Viewport } from "next";
import { Instrument_Serif, Inter, JetBrains_Mono } from "next/font/google";
import "./globals.css";

const inter = Inter({
  variable: "--font-inter",
  subsets: ["latin"],
  display: "swap",
});

const instrumentSerif = Instrument_Serif({
  variable: "--font-instrument-serif",
  subsets: ["latin"],
  weight: "400",
  display: "swap",
});

const jetbrainsMono = JetBrains_Mono({
  variable: "--font-jetbrains-mono",
  subsets: ["latin"],
  display: "swap",
});

const TITLE = "hellodb — sovereign memory for Claude Code";
const DESCRIPTION =
  "Local-first, end-to-end encrypted, branchable memory for Claude Code. Plugin-driven digest pipelines auto-merge confident facts and hold uncertain ones for review. You own the keys, the data, and the bill.";
const SITE = "https://hellodb.dev";

export const metadata: Metadata = {
  metadataBase: new URL(SITE),
  title: {
    default: TITLE,
    template: "%s · hellodb",
  },
  description: DESCRIPTION,
  applicationName: "hellodb",
  authors: [{ name: "Ish Prasad" }],
  keywords: [
    "Claude Code",
    "MCP server",
    "agent memory",
    "local-first",
    "sovereign",
    "end-to-end encrypted",
    "SQLCipher",
    "vector search",
    "branchable memory",
    "Rust",
    "Cloudflare Workers AI",
    "R2",
    "Ed25519",
    "self-hosted memory",
    "memory-digest",
    "AI agent",
  ],
  category: "Developer Tools",
  alternates: {
    canonical: SITE,
  },
  openGraph: {
    title: TITLE,
    description: DESCRIPTION,
    type: "website",
    url: SITE,
    siteName: "hellodb",
    locale: "en_US",
  },
  twitter: {
    card: "summary_large_image",
    title: TITLE,
    description: DESCRIPTION,
  },
  robots: {
    index: true,
    follow: true,
    googleBot: {
      index: true,
      follow: true,
      "max-image-preview": "large",
      "max-snippet": -1,
      "max-video-preview": -1,
    },
  },
  formatDetection: {
    telephone: false,
    email: false,
    address: false,
  },
};

export const viewport: Viewport = {
  themeColor: [
    { media: "(prefers-color-scheme: dark)", color: "#1a1815" },
    { media: "(prefers-color-scheme: light)", color: "#1a1815" },
  ],
  colorScheme: "dark",
  width: "device-width",
  initialScale: 1,
};

const JSON_LD = {
  "@context": "https://schema.org",
  "@graph": [
    {
      "@type": "SoftwareApplication",
      "@id": `${SITE}/#software`,
      name: "hellodb",
      alternateName: "hellodb — sovereign memory for Claude Code",
      description: DESCRIPTION,
      url: SITE,
      applicationCategory: "DeveloperApplication",
      applicationSubCategory: "Agent Memory",
      operatingSystem: "macOS, Linux, Windows",
      softwareVersion: "0.1.0",
      programmingLanguage: "Rust",
      license: "https://opensource.org/license/mit",
      downloadUrl: `${SITE}/install`,
      codeRepository: "https://github.com/eprasad7/hellodb",
      offers: {
        "@type": "Offer",
        price: "0",
        priceCurrency: "USD",
      },
      featureList: [
        "Local-first",
        "End-to-end encrypted (Ed25519 + ChaCha20-Poly1305, SQLCipher)",
        "Branchable memory (git-like)",
        "Semantic recall with time-decay reinforcement",
        "Memory plugin agents (memory-digest, memory-consolidate)",
        "22 MCP tools",
        "Cloudflare Workers AI + R2 via your own account",
        "Claude Code native memory interop (CLAUDE.md import)",
      ],
    },
    {
      "@type": "WebSite",
      "@id": `${SITE}/#website`,
      url: SITE,
      name: "hellodb",
      description: DESCRIPTION,
      inLanguage: "en-US",
      publisher: { "@id": `${SITE}/#org` },
    },
    {
      "@type": "Organization",
      "@id": `${SITE}/#org`,
      name: "hellodb",
      url: SITE,
      logo: `${SITE}/icon`,
    },
  ],
};

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html
      lang="en"
      className={`${inter.variable} ${instrumentSerif.variable} ${jetbrainsMono.variable}`}
    >
      <head>
        <script
          type="application/ld+json"
          dangerouslySetInnerHTML={{ __html: JSON.stringify(JSON_LD) }}
        />
      </head>
      <body className="min-h-screen bg-bg text-fg antialiased">
        {children}
      </body>
    </html>
  );
}
