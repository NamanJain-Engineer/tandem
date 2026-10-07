import type { Metadata } from "next";
import type { ReactNode } from "react";
import { SITE } from "../lib/site";
import "./globals.css";

/** The preview image for shared links: a screenshot of the real app. */
const PREVIEW = "/screens/code-mode.png";
/** The founder's X handle, from their profile link. */
const creator = SITE.founder.link ? `@${SITE.founder.link.split("/").pop()}` : undefined;

export const metadata: Metadata = {
  metadataBase: new URL(SITE.url),
  title: { default: `${SITE.product} · ${SITE.tagline}`, template: `%s · ${SITE.product}` },
  description:
    "Vibezeo runs Claude Code, Codex, Gemini CLI and other coding-agent CLIs side by side in real terminals, on your own accounts, with one Dashboard for every agent.",
  icons: { icon: "/favicon.svg" },
  authors: [{ name: SITE.founder.name, url: SITE.founder.link }],
  openGraph: {
    title: SITE.product,
    description: SITE.tagline,
    url: SITE.url,
    siteName: SITE.product,
    type: "website",
    images: [{ url: PREVIEW, width: 1440, height: 900, alt: `The ${SITE.product} desktop app` }],
  },
  twitter: {
    card: "summary_large_image",
    title: SITE.product,
    description: SITE.tagline,
    creator,
    images: [PREVIEW],
  },
};

export default function RootLayout({ children }: { children: ReactNode }) {
  return (
    <html lang="en" data-theme="dark">
      <body>{children}</body>
    </html>
  );
}
