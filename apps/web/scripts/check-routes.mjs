// Every exported page answers at its own URL, with the trailing slash
// (trailingSlash: true), and never redirects (D-1815). Vercel takes its
// routes from .next/routes-manifest.json and vercel.json: Next's own
// trailing-slash rule stripped the slash from /changelog/0.6.0/ because
// "0.6.0" looks like a file, so next.config turns those rules off and
// vercel.json adds back only "no slash → slash". This fails the build if
// any page's URL matches a redirect, or a slash-less URL isn't sent home.
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative, sep } from "node:path";
import process from "node:process";
import { pathToFileURL } from "node:url";

const WEB = join(import.meta.dirname, "..");
const OUT = join(WEB, "out");

/** Next and Vercel write sources as path-to-regexp; ours use only
 * `:name(regex)` groups, which become plain capture groups. */
export const sourceRegex = (source) => new RegExp(`^${source.replace(/:\w+\(/g, "(").replace(/\/$/, "/")}$`);

function* pageUrls(dir) {
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) {
      if (name === "_next" || relative(OUT, path).startsWith(join("demo", "app"))) continue;
      yield* pageUrls(path);
    } else if (name === "index.html") {
      const rel = relative(OUT, dir).split(sep).join("/");
      yield rel ? `/${rel}/` : "/";
    }
  }
}

function main() {
  const manifest = JSON.parse(readFileSync(join(WEB, ".next", "routes-manifest.json"), "utf8"));
  const vercel = JSON.parse(readFileSync(join(WEB, "vercel.json"), "utf8"));
  const rules = [
    ...(manifest.redirects ?? []).map((r) => ({ from: "routes-manifest", re: new RegExp(r.regex) })),
    ...(vercel.redirects ?? []).map((r) => ({ from: "vercel.json", re: sourceRegex(r.source) })),
  ];

  const problems = [];
  let pages = 0;
  for (const url of pageUrls(OUT)) {
    pages++;
    for (const r of rules) if (r.re.test(url)) problems.push(`${url} redirects (${r.from}: ${r.re})`);
    if (url !== "/" && !rules.some((r) => r.re.test(url.slice(0, -1))))
      problems.push(`${url.slice(0, -1)} doesn't redirect to ${url}`);
  }
  if (problems.length) {
    process.stderr.write(`check-routes:\n${problems.join("\n")}\n`);
    process.exit(1);
  }
  process.stdout.write(`check-routes: ${pages} pages answer at their trailing-slash URL with no redirect\n`);
}

// The check reads the export (`out/` and its manifest), so it runs only as
// `node scripts/check-routes.mjs` after `build:site`. Importing the module, as
// routes.test.ts does for sourceRegex, reads nothing and can't exit the run.
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) main();
