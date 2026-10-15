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

/** The real-app captures on the home page (D-1838): taken at 2× from the
 * build named, served as AVIF and WebP at 1× and 2× with a PNG fallback. A
 * test checks the build is a release in CHANGELOG.md and the files ship. */
export const SCREENS = {
  codeMode: {
    src: "/screens/code-mode.png",
    width: 1440,
    height: 900,
    formats: ["avif", "webp"],
    retina: true,
    build: "0.7.2",
    alt: "The Vibezeo desktop app in Code mode with four terminal panes over its own repository: the git history, htop, vim open on the Claude Code hook handler, and a cargo test run with 256 tests passing.",
  },
  launchGrid: {
    src: "/screens/launch-grid.png",
    width: 544,
    height: 610,
    formats: ["avif", "webp"],
    retina: true,
    build: "0.7.2",
    alt: "Vibezeo's launch grid in a new tab: Claude Code 2.1.300 detected, a terminal, and ten more engines with Install buttons.",
  },
} as const;
