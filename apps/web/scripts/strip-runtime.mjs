// Removes Next's client runtime from the exported marketing pages (D-1813,
// docs/WEBSITE_BRIEF.md §10.3). They have no client components: links are
// plain <a>, the menu is a popover, the FAQ is <details>. What Next adds
// anyway (its runtime chunks, their preloads and the inline RSC payload,
// about half of each page's HTML) runs for nothing and delays the page.
//
// Kept: the docs (Fumadocs is a client app) and the demo app. Afterwards
// the only scripts left must be F-WB-01 AC2's two: /demo/embed.js on the
// home page and /download/detect.js on /download/. Anything else fails the
// build, so a client component added to a marketing page can't silently
// stop working.
import { readFileSync, readdirSync, statSync, writeFileSync } from "node:fs";
import { join, relative, sep } from "node:path";
import process from "node:process";

const OUT = join(import.meta.dirname, "..", "out");
const SKIP = ["docs", join("demo", "app")];
const ALLOWED = {
  "index.html": ['<script src="/demo/embed.js" defer="">'],
  [join("download", "index.html")]: ['<script src="/download/detect.js" defer="">'],
};

function* pages(dir) {
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    const rel = relative(OUT, path);
    if (SKIP.some((s) => rel === s || rel.startsWith(s + sep))) continue;
    if (statSync(path).isDirectory()) yield* pages(path);
    else if (name.endsWith(".html")) yield path;
  }
}

const RUNTIME = [
  // <script src="/_next/static/chunks/…" …></script>
  /<script[^>]*\ssrc="\/_next\/static\/chunks\/[^"]+"[^>]*><\/script>/g,
  // <link rel="preload" as="script" … href="/_next/static/chunks/…">
  /<link(?=[^>]*\sas="script")(?=[^>]*\shref="\/_next\/static\/chunks\/)[^>]*\/?>/g,
  // the inline RSC payload: <script>self.__next_f…</script>
  /<script>\(?self\.__next_f[\s\S]*?<\/script>/g,
];

let saved = 0;
const problems = [];
for (const path of pages(OUT)) {
  const before = readFileSync(path, "utf8");
  let html = before;
  for (const re of RUNTIME) html = html.replace(re, "");
  const rel = relative(OUT, path);
  const scripts = [...html.matchAll(/<script[^>]*>/g)].map((m) => m[0]);
  const allowed = ALLOWED[rel] ?? [];
  if (JSON.stringify(scripts) !== JSON.stringify(allowed))
    problems.push(`${rel}: ${JSON.stringify(scripts)} (allowed ${JSON.stringify(allowed)})`);
  saved += before.length - html.length;
  writeFileSync(path, html);
}
if (problems.length) {
  process.stderr.write(`strip-runtime: scripts beyond F-WB-01 AC2's allowlist:\n${problems.join("\n")}\n`);
  process.exit(1);
}
process.stdout.write(`strip-runtime: removed Next's unused runtime from the marketing pages (${Math.round(saved / 1000)} KB of HTML)\n`);
