// The scripts each exported marketing page may keep after strip-runtime
// (F-WB-01 AC2, D-1813): the home page's demo loader, the download page's OS
// note, and on a blog post page its JSON-LD, which is data for search
// engines, not code (F-WB-06, D-1852), and /blog/blog.js, which adds the
// code blocks' copy buttons (D-1860). Anything else fails the build.
import { sep } from "node:path";

/** The `<script …>` opening tags, in order, allowed in `rel` (a path under
 * out/, e.g. `blog/my-post/index.html`). */
export function allowedScripts(rel) {
  const path = rel.split(sep).join("/");
  if (path === "index.html") return ['<script src="/demo/embed.js" defer="">'];
  if (path === "download/index.html") return ['<script src="/download/detect.js" defer="">'];
  // /blog/<slug>/ only: not the index, the tag pages or the feed.
  if (/^blog\/(?!tags\/)[a-z0-9-]+\/index\.html$/.test(path)) return ['<script type="application/ld+json">', '<script src="/blog/blog.js" defer="">'];
  return [];
}
