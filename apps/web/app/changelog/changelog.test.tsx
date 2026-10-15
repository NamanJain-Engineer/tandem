import type { ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { RELEASES, versionPath } from "../../lib/changelog";
import { RELEASE } from "../../lib/release";
import { sitemapXml } from "../../lib/site";
import ChangelogPage from "./page.site";
import VersionPage, { generateStaticParams } from "./[version]/page.site";

const html = (el: ReactElement) => renderToStaticMarkup(el);

describe("the website changelog from CHANGELOG.md (F-WB-03)", () => {
  it("F-WB-03 the index lists every version newest first, linking its page", () => {
    // From the Releases heading on: the "New in" tiles above it link their
    // own release's notes first (D-1841).
    const page = html(<ChangelogPage />);
    const h = page.slice(page.indexOf('id="releases-title"'));
    expect(RELEASES[0]?.version).toBe(RELEASE.version);
    let at = -1;
    for (const r of RELEASES) {
      const i = h.indexOf(`href="${versionPath(r.version)}"`);
      expect(i, r.version).toBeGreaterThan(at);
      at = i;
    }
  });

  it("F-WB-03 each version has a static page with its summary, categories and platforms", async () => {
    expect(generateStaticParams().map((p) => p.version)).toEqual(RELEASES.map((r) => r.version));
    // 0.3.0's notes have the Markdown this checks; every version renders the same way.
    const release = RELEASES.find((r) => r.version === "0.3.0");
    if (!release) throw new Error("no 0.3.0 release");
    const h = html(await VersionPage({ params: Promise.resolve({ version: release.version }) }));
    expect(h).toContain(`<h1`);
    expect(h).toContain(release.version);
    expect(h).toContain("<h2");
    expect(h).toContain("Added");
    // Markdown in bullets is rendered, and the platform reads in words.
    expect(h).toContain("<strong>Files.</strong>");
    expect(h).toContain("<code>.gitignore</code>");
    expect(h).toContain("Linux");
    expect(h).not.toContain("[lin]");
  });

  it("F-WB-03 the sitemap lists the version pages", () => {
    const map = sitemapXml();
    for (const r of RELEASES) expect(map).toContain(`/changelog/${r.version}/</loc>`);
  });
});
