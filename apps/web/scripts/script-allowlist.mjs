// The scripts each exported marketing page may keep after strip-runtime
// (F-WB-01 AC2, D-1813): the home page's demo loader (also the scene player
// on 0.9.0's changelog page, D-1885), the download page's OS
// note, and on a blog post page its JSON-LD, which is data for search
// engines, not code (F-WB-06, D-1852), and /blog/blog.js, which adds the
// code blocks' copy buttons (D-1860). Before them, on every page, the
// inline theme script (D-1871). Anything else fails the build.
import { sep } from "node:path";

/** The `<script …>` opening tags, in order, allowed in `rel` (a path under
 * out/, e.g. `blog/my-post/index.html`). */
export function allowedScripts(rel) {
  const path = rel.split(sep).join("/");
  return [THEME_BOOT, ...pageScripts(path)];
}

/** Every page opens with the theme script, inline in `<head>` so the
 * visitor's choice is applied before the first paint (F-WB-01 AC4, D-1871). */
export const THEME_BOOT = '<script data-theme-boot="">';

function pageScripts(path) {
  if (path === "index.html" || path === "changelog/0.9.0/index.html") return ['<script src="/demo/embed.js" defer="">'];
  if (path === "download/index.html") return ['<script src="/download/detect.js" defer="">'];
  // /blog/<slug>/ only: not the index, the tag pages or the feed.
  if (/^blog\/(?!tags\/)[a-z0-9-]+\/index\.html$/.test(path)) return ['<script type="application/ld+json">', '<script src="/blog/blog.js" defer="">'];
  return [];
}
