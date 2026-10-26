/** The site's media and the build each one shows (§7). A caption names the
 * build it was recorded with, never the current release unless they match
 * (a site test checks). */
export const VIDEO = {
  /** The product video was recorded with the first Linux release. */
  recordedWith: "0.1.0",
  poster: "/screens/product-poster.jpg",
  sources: [
    // VP9 first: open-source Chromium builds can't play H.264.
    { src: "/screens/product.webm", type: "video/webm" },
    { src: "/screens/product.mp4", type: "video/mp4" },
  ],
} as const;

/** The real-app captures on the site (D-1838; Code mode's is on /demo/):
 * taken at 2× from the build named, served as AVIF and WebP at 1× and 2×
 * with a PNG fallback. A test checks the build is a release in CHANGELOG.md
 * and the files ship. */
export const SCREENS = {
  codeMode: {
    src: "/screens/code-mode.png",
    width: 1440,
    height: 900,
    formats: ["avif", "webp"],
    retina: true,
    build: "0.12.0",
    alt: "The Vibezeo desktop app in Code mode with four shell panes over its own repository: the git history, htop, vim open on the Claude Code hook handler, and a cargo test run with 271 tests passing.",
  },
} as const;
