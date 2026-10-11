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
