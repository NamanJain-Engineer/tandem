import type { Metadata, Viewport } from "next";
import type { ReactNode } from "react";
import { SITE } from "../lib/site";
import { THEME_COLOR, THEME_SCRIPT } from "../lib/theme";
import { inter, mono } from "./fonts";
import "./globals.css";

export const metadata: Metadata = {
  metadataBase: new URL(SITE.url),
  title: { default: `${SITE.product} · ${SITE.tagline}`, template: `%s · ${SITE.product}` },
  description:
    "Vibezeo runs Claude Code, Codex, Gemini CLI and other coding-agent CLIs side by side in real terminals, on your own accounts, with one Dashboard for every agent.",
  applicationName: SITE.product,
  icons: {
    icon: [
      { url: "/favicon.svg", type: "image/svg+xml" },
      { url: "/icon.png", type: "image/png", sizes: "512x512" },
    ],
    apple: { url: "/apple-touch-icon.png", sizes: "180x180" },
  },
  openGraph: {
    title: SITE.product,
    description: SITE.tagline,
    url: SITE.url,
    siteName: SITE.product,
    type: "website",
    images: [{ url: "/og.png", width: 1200, height: 630, alt: `${SITE.product}: ${SITE.tagline}` }],
  },
  twitter: { card: "summary_large_image", title: SITE.product, description: SITE.tagline, images: ["/og.png"] },
};

/** System follows the visitor's OS in CSS (D-1801); the theme script sets
 * these colours itself when the visitor picks Light or Dark (D-1870). */
export const viewport: Viewport = {
  colorScheme: "dark light",
  themeColor: [
    { media: "(prefers-color-scheme: dark)", color: THEME_COLOR.dark },
    { media: "(prefers-color-scheme: light)", color: THEME_COLOR.light },
  ],
};

/** The theme script is the first thing in `<head>`, so the choice is on
 * `<html>` before anything paints (F-WB-01 AC4, D-1871). It changes `<html>`'s
 * attributes before React hydrates the portal's pages, hence
 * `suppressHydrationWarning` there and nowhere else. */
export default function RootLayout({ children }: { children: ReactNode }) {
  return (
    <html lang="en" className={`${inter.variable} ${mono.variable}`} suppressHydrationWarning>
      <head>
        <script data-theme-boot="" dangerouslySetInnerHTML={{ __html: THEME_SCRIPT }} />
      </head>
      <body className="site">{children}</body>
    </html>
  );
}
