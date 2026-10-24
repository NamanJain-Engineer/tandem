import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { sourceRegex } from "../scripts/check-routes.mjs";
import { FEED_PATH, POSTS, blogPages, ogImagePath } from "../lib/blog/posts";
import { RELEASES, versionPath } from "../lib/changelog";
import { DOCS_PAGES, docsPath } from "../lib/docs/pages";
import { SITE_PAGES } from "../lib/site";

// D-1815: every page answers at its trailing-slash URL with no redirect.
// On Vercel, /changelog/0.6.0/ answered 308 to /changelog/0.6.0: Next's own
// trailing-slash rule reads "0.6.0" as a file name.

const web = join(import.meta.dirname, "..");
const vercel = JSON.parse(readFileSync(join(web, "vercel.json"), "utf8")) as {
  redirects: Array<{ source: string; destination: string; permanent: boolean }>;
};
const toRegex = sourceRegex as (source: string) => RegExp;
const rules = vercel.redirects.map((r) => ({ ...r, re: toRegex(r.source) }));
const target = (url: string) => {
  const r = rules.find((x) => x.re.test(url));
  if (!r) return null;
  const m = r.re.exec(url) ?? [];
  return r.destination.replace(/:\w+/, m[1] ?? "");
};

describe("routes (D-1815)", () => {
  const pages = [...SITE_PAGES, ...DOCS_PAGES.map((p) => docsPath(p.slug)), ...RELEASES.map((r) => versionPath(r.version)), ...blogPages()];

  it("F-WB-03 version pages serve at their trailing-slash URL, with no redirect", () => {
    expect(RELEASES.length).toBeGreaterThan(0);
    for (const r of RELEASES) {
      const url = versionPath(r.version);
      expect(url).toBe(`/changelog/${r.version}/`);
      expect(target(url), url).toBeNull();
      expect(target(url.slice(0, -1)), url).toBe(url);
    }
  });

  it("F-WB-01 every page serves at its trailing-slash URL; the slash-less URL redirects there", () => {
    for (const url of pages) {
      expect(target(url), url).toBeNull();
      if (url !== "/") expect(target(url.slice(0, -1)), url).toBe(url);
    }
    // Files are left alone, and site-info keeps answering at its slash form (D-135).
    for (const file of ["/llms.txt", "/openapi.json", "/.well-known/security.txt", "/downloads/SHA256SUMS.txt", FEED_PATH])
      expect(target(file), file).toBeNull();
    expect(target("/api/v1/site-info")).toBe("/api/v1/site-info/");
    expect(rules.every((r) => r.permanent)).toBe(true);
  });

  it("F-WB-01 the old pricing page and anything under it go to the download page (D-869)", () => {
    for (const url of ["/pricing", "/pricing/", "/pricing/compare/"]) expect(target(url), url).toBe("/download/");
    expect(pages).not.toContain("/pricing/");
  });

  it("F-WB-06 AC7 every blog page serves at its trailing-slash URL, and the feed as a file", () => {
    expect(blogPages().length).toBeGreaterThan(2);
    for (const url of blogPages()) {
      expect(target(url), url).toBeNull();
      expect(target(url.slice(0, -1)), url).toBe(url);
    }
    expect(target(FEED_PATH)).toBeNull();
    for (const p of POSTS) expect(target(ogImagePath(p.slug)), p.slug).toBeNull();
  });

  it("F-WB-01 Next's own trailing-slash redirects are off for the export, so they can't strip a slash", () => {
    const config = readFileSync(join(web, "next.config.ts"), "utf8");
    expect(config).toContain("skipTrailingSlashRedirect: true");
    const pkg = JSON.parse(readFileSync(join(web, "package.json"), "utf8")) as { scripts: Record<string, string> };
    expect(pkg.scripts["build:site"]).toContain("node scripts/check-routes.mjs");
  });
});
