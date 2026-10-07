import type { Metadata } from "next";
import type { ReactNode } from "react";
import { SITE } from "../lib/site";
import "./globals.css";

export const metadata: Metadata = {
  metadataBase: new URL(SITE.url),
  title: { default: `${SITE.product} · ${SITE.tagline}`, template: `%s · ${SITE.product}` },
  description:
    "Vibezeo runs Claude Code, Codex, Gemini CLI and other coding-agent CLIs side by side in real terminals, on your own accounts, with one Dashboard for every agent.",
  icons: { icon: "/favicon.svg" },
  openGraph: {
    title: SITE.product,
    description: SITE.tagline,
    url: SITE.url,
    siteName: SITE.product,
    type: "website",
  },
};

export default function RootLayout({ children }: { children: ReactNode }) {
  return (
    <html lang="en" data-theme="dark">
      <body>{children}</body>
    </html>
  );
}
