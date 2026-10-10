import localFont from "next/font/local";

// Self-hosted through next/font (D-1802): the files ship from /_next/static,
// Inter is preloaded, and each face gets a metric-matched fallback so the
// swap moves nothing. Latin subsets only.

/** Inter with its optical-size axis (14–32 opsz, 100–900 wght): display
 * sizes draw in the tighter Inter Display cut automatically. */
export const inter = localFont({
  src: "../node_modules/@fontsource-variable/inter/files/inter-latin-opsz-normal.woff2",
  variable: "--font-inter",
  weight: "100 900",
  display: "swap",
  preload: true,
  adjustFontFallback: "Arial",
});

/** JetBrains Mono for labels, versions, keycaps, commands and hashes. */
export const mono = localFont({
  src: "../node_modules/@fontsource-variable/jetbrains-mono/files/jetbrains-mono-latin-wght-normal.woff2",
  variable: "--font-jbm",
  weight: "100 800",
  display: "swap",
  preload: false,
  adjustFontFallback: false,
});
