// Starts the docs' Next chunks after the first paint (D-1819).
//
// The docs are static HTML that Next then hydrates. Next loads its chunks as
// `async` scripts, so on a fast machine they finish before the first paint,
// and Lighthouse's simulation (Lantern) counts every request that finished
// before the observed largest paint toward it: about 170 KB of gzipped
// JavaScript in front of a paragraph of text. Here each docs page's chunk
// tags and their preloads move into one first-party loader, /docs-boot.js,
// which adds them back once a frame has painted. The inline RSC payload
// stays where Next put it. Run after `next build` (build:site).
import { readFileSync, readdirSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import process from "node:process";

const CHUNK = /<script src="(\/_next\/static\/chunks\/[^"]+)"([^>]*)><\/script>/g;
const PRELOAD = /<link(?=[^>]*\sas="script")(?=[^>]*\shref="\/_next\/static\/chunks\/)[^>]*\/?>/g;

/** One page's HTML with its chunks behind the loader. The legacy-browser
 * polyfills (`noModule`) are dropped: every browser the site supports runs
 * modules. Idempotent. */
export function deferChunks(html) {
  const chunks = [];
  let out = html.replace(CHUNK, (_, src, attrs) => {
    if (/\snoModule/i.test(attrs)) return "";
    const id = /\sid="([^"]+)"/.exec(attrs)?.[1];
    chunks.push(id ? `${src}#${id}` : src);
    return "";
  });
  if (chunks.length === 0) return html;
  out = out.replace(PRELOAD, "");
  return out.replace("</body>", `<script src="/docs-boot.js" data-chunks="${chunks.join(" ")}" defer=""></script></body>`);
}

function* pages(dir) {
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) yield* pages(path);
    else if (name.endsWith(".html")) yield path;
  }
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const docs = join(import.meta.dirname, "..", "out", "docs");
  let n = 0;
  for (const path of pages(docs)) {
    const html = readFileSync(path, "utf8");
    const out = deferChunks(html);
    if (!out.includes('src="/docs-boot.js"')) {
      process.stderr.write(`defer-docs: ${path} has no Next chunks to defer\n`);
      process.exit(1);
    }
    writeFileSync(path, out);
    n++;
  }
  process.stdout.write(`defer-docs: ${n} docs pages start their scripts after the first paint\n`);
}
