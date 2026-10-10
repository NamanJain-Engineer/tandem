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
    const h = html(<ChangelogPage />);
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
    const latest = RELEASES[0];
    if (!latest) throw new Error("no release");
    const h = html(await VersionPage({ params: Promise.resolve({ version: latest.version }) }));
    expect(h).toContain(`<h1`);
    expect(h).toContain(latest.version);
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
