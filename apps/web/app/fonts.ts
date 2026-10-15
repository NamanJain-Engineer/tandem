import localFont from "next/font/local";

// Self-hosted through next/font (D-1802): the files ship from /_next/static,
// Inter is preloaded, and each face gets a metric-matched fallback so the
// swap moves nothing. Latin subsets only, trimmed to the weights the site
// draws (D-1844, scripts/trim-fonts.py; OFL, licences beside them).

/** Inter with its optical-size axis (14–32 opsz) and weights 400–650:
 * display sizes draw in the tighter Inter Display cut automatically. */
export const inter = localFont({
  src: "./fonts/inter-site.woff2",
  variable: "--font-inter",
  weight: "400 650",
  display: "swap",
  preload: true,
  adjustFontFallback: "Arial",
});

/** JetBrains Mono, 400–500, for labels, versions, keycaps, commands and hashes. */
export const mono = localFont({
  src: "./fonts/mono-site.woff2",
  variable: "--font-jbm",
  weight: "400 500",
  display: "swap",
  preload: false,
  adjustFontFallback: false,
});
