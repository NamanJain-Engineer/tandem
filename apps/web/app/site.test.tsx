import { readFileSync } from "node:fs";
import { join } from "node:path";
import type { ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { ENGINES, OPERATOR, SITE, SITE_PAGES, robotsTxt, securityTxt, sitemapXml } from "../lib/site";
import HomePage from "./page.site";
import NotFound from "./not-found.site";
import PrivacyPage from "./privacy/page.site";
import SecurityPage from "./security/page.site";
import TermsPage from "./terms/page.site";

const html = (el: ReactElement) => renderToStaticMarkup(el);
const PAGES = {
  home: HomePage,
  privacy: PrivacyPage,
  terms: TermsPage,
  security: SecurityPage,
  notFound: NotFound,
};

describe("public site", () => {
  it("F-WB-01 home shows the tagline, the dev status, every engine and a way to get in touch", () => {
    const h = html(<HomePage />);
    expect(h).toContain(SITE.tagline);
    expect(h).toContain("In active development");
    for (const e of ENGINES) expect(h).toContain(`>${e}<`);
    expect(h).toContain(`mailto:${SITE.contactEmail}`);
    for (const id of ["features", "engines", "roadmap", "team", "faq"]) expect(h).toContain(`id="${id}"`);
  });

  it("F-WB-01 the founder is named on the home page, and Terms and Privacy name who runs the site", () => {
    expect(html(<HomePage />)).toContain(`${SITE.founder.role}, ${SITE.company}`);
    for (const Page of [TermsPage, PrivacyPage]) expect(html(<Page />)).toContain(OPERATOR);
  });

  it("F-WB-01 the illustration is labelled as sample data", () => {
    expect(html(<HomePage />)).toContain("Illustration · sample data");
  });

  it("F-WB-01 every page has one h1, the legal links and the contact address", () => {
    for (const [name, Page] of Object.entries(PAGES)) {
      const h = html(<Page />);
      expect(h.match(/<h1[\s>]/g)?.length, name).toBe(1);
      for (const href of ["/privacy/", "/terms/", "/security/"]) expect(h, name).toContain(`href="${href}"`);
      expect(h, name).toContain(SITE.contactEmail);
    }
  });

  it("F-WB-01 AC2 no page loads a script, iframe or third-party resource", () => {
    for (const [name, Page] of Object.entries(PAGES)) {
      const h = html(<Page />);
      expect(h, name).not.toMatch(/<(script|iframe)[\s>]/);
      const external = [...h.matchAll(/src="((?:https?:)?\/\/[^"]+)"/g)].map((m) => m[1]);
      expect(external, name).toEqual([]);
    }
  });

  it("F-WB-01 headers forbid framing and third-party origins", () => {
    const headers = readFileSync(join(import.meta.dirname, "../public/_headers"), "utf8");
    const csp = /Content-Security-Policy: (.*)/.exec(headers)?.[1] ?? "";
    expect(csp).toContain("frame-ancestors 'none'");
    expect(csp).toContain("default-src 'self'");
    expect(csp).not.toMatch(/https?:/);
    expect(headers).toContain("X-Content-Type-Options: nosniff");
  });

  it("F-WB-01 robots, sitemap and security.txt point at the site", () => {
    expect(robotsTxt()).toContain(`Sitemap: ${SITE.url}/sitemap.xml`);
    const map = sitemapXml();
    for (const p of SITE_PAGES) expect(map).toContain(`<loc>${SITE.url}${p}</loc>`);
    const now = new Date("2026-10-07T00:00:00Z");
    const txt = securityTxt(now);
    expect(txt).toContain(`Contact: mailto:${SITE.contactEmail}`);
    const expires = new Date(/Expires: (.*)/.exec(txt)?.[1] ?? "");
    const days = (expires.getTime() - now.getTime()) / 86_400_000;
    expect(days).toBeGreaterThan(300);
    expect(days).toBeLessThan(365);
  });
});
