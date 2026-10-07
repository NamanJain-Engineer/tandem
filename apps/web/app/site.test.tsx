import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import type { ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { PLANS } from "../lib/pricing";
import { MILESTONES, PROGRESS } from "../lib/progress";
import { CHECKSUMS_URL, DOWNLOADS, RELEASE, RELEASES_REPO, assetUrl } from "../lib/release";
import {
  EARLY_ACCESS_HREF,
  ENGINES,
  OPERATOR,
  SITE,
  SITE_PAGES,
  robotsTxt,
  securityTxt,
  sitemapXml,
} from "../lib/site";
import AboutPage from "./about/page.site";
import ChangelogPage from "./changelog/page.site";
import DownloadPage from "./download/page.site";
import PricingPage from "./pricing/page.site";
import HomePage from "./page.site";
import NotFound from "./not-found.site";
import PrivacyPage from "./privacy/page.site";
import SecurityPage from "./security/page.site";
import TermsPage from "./terms/page.site";

const html = (el: ReactElement) => renderToStaticMarkup(el);
const PAGES = {
  home: HomePage,
  about: AboutPage,
  changelog: ChangelogPage,
  download: DownloadPage,
  pricing: PricingPage,
  privacy: PrivacyPage,
  terms: TermsPage,
  security: SecurityPage,
  notFound: NotFound,
};

describe("public site", () => {
  it("F-WB-01 home shows the tagline, the dev status, every engine and a way to get in touch", () => {
    const h = html(<HomePage />);
    expect(h).toContain(SITE.tagline);
    expect(h).toContain(`Early access · ${RELEASE.version}`);
    for (const e of ENGINES) expect(h).toContain(`>${e}<`);
    expect(h).toContain(`mailto:${SITE.contactEmail}`);
    for (const id of ["demo", "claude", "features", "engines", "roadmap", "pricing", "team", "faq"])
      expect(h).toContain(`id="${id}"`);
  });

  it("F-WB-01 the founder is named on the home and About pages, and Terms and Privacy name who runs the site", () => {
    for (const Page of [HomePage, AboutPage])
      expect(html(<Page />)).toContain(`${SITE.founder.role}, ${SITE.company}`);
    for (const Page of [TermsPage, PrivacyPage]) expect(html(<Page />)).toContain(OPERATOR);
    expect(html(<PrivacyPage />)).toContain(`Our host, ${SITE.host}`);
  });

  it("F-WB-01 About has the company details and a contact section, and every page links to it", () => {
    const h = html(<AboutPage />);
    for (const id of ["founder", "company", "contact"]) expect(h).toContain(`id="${id}"`);
    for (const fact of [SITE.founded, SITE.location, SITE.founder.bio, SITE.founder.link])
      expect(h).toContain(fact ?? "");
    expect(h).toContain(`mailto:${SITE.contactEmail}`);
    for (const [name, Page] of Object.entries(PAGES))
      expect(html(<Page />), name).toContain(`href="/about/"`);
  });

  it("F-WB-01 the screenshots are of the real app and ship with the site", () => {
    const h = html(<HomePage />);
    for (const name of ["code-mode", "launch-grid"]) {
      expect(h).toContain(`src="/screens/${name}.png"`);
      expect(existsSync(join(import.meta.dirname, `../public/screens/${name}.png`)), name).toBe(true);
    }
    expect(h).not.toContain("sample data");
  });

  it("F-WB-01 Built with Claude separates what's shipped from what's built but not live", () => {
    const h = html(<HomePage />);
    expect(h).toContain("Built with Claude");
    expect(h).toContain("--resume");
    expect(h).toContain("Built, live at launch");
    expect(h).toContain("go live with accounts at launch");
    expect(h).not.toContain("not built yet");
    // Dashboard approvals have no real source until Thread mode: never claim them.
    expect(h).not.toMatch(/approve or deny tool requests right on the card/i);
  });

  it("F-WB-01 the home page leads to the download, takes feedback by email and shows the real progress figures", () => {
    const h = html(<HomePage />);
    expect(h).toContain(`href="/download/"`);
    expect(h).toContain("Download early access");
    expect(EARLY_ACCESS_HREF.startsWith(`mailto:${SITE.contactEmail}?subject=`)).toBe(true);
    expect(h).toContain(PROGRESS.automatedTests.toLocaleString("en-US"));
    expect(h).toContain(`href="/changelog/"`);
  });

  it("F-WB-01 the demo video is a recording of the real app that ships with the site", () => {
    const h = html(<HomePage />);
    expect(h).toMatch(/<video[^>]*controls/);
    for (const file of ["demo.webm", "demo.mp4"]) {
      expect(h).toContain(`src="/screens/${file}"`);
      expect(existsSync(join(import.meta.dirname, `../public/screens/${file}`)), file).toBe(true);
    }
  });

  it("F-WB-01 download links only builds in the public release, with checksums and install steps; the rest say Coming soon", () => {
    const h = html(<DownloadPage />);
    expect(DOWNLOADS.map((d) => d.os)).toEqual(["macos", "windows", "linux"]);
    expect(DOWNLOADS.filter((d) => d.available).map((d) => d.os)).toEqual(["linux"]);
    for (const d of DOWNLOADS) {
      expect(h).toContain(`id="${d.os}"`);
      for (const a of d.assets) {
        expect(a.file, a.file).toContain(RELEASE.version);
        expect(assetUrl(a.file).startsWith(`${RELEASES_REPO}/releases/download/${RELEASE.tag}/`)).toBe(true);
        // No dead links: a build that isn't out yet isn't linked.
        if (d.available) expect(h).toContain(`href="${assetUrl(a.file)}"`);
        else expect(h).not.toContain(`href="${assetUrl(a.file)}"`);
      }
      expect(d.steps.length).toBeGreaterThan(0);
    }
    expect(h.match(/Coming soon/g)?.length).toBe(2);
    expect(h).toContain(`href="${CHECKSUMS_URL}"`);
    expect(h).toContain(`id="getting-started"`);
    // Honest about what this build is.
    expect(h).toContain("code-signed yet");
    expect(h).toContain("contact our servers");
    expect(SITE_PAGES).toContain("/download/");
  });

  it("F-WB-01 pricing shows the launch plans from the spec and says Free is available now, Pro and Team not yet", () => {
    const h = html(<PricingPage />);
    expect(PLANS.map((p) => [p.name, p.price])).toEqual([
      ["Free", "$0"],
      ["Pro", "$29"],
      ["Team", "$49"],
    ]);
    for (const p of PLANS) expect(h).toContain(`id="${p.id}"`);
    expect(h).toContain("Pricing at launch");
    expect(h).toContain("on sale yet");
    expect(h).toContain("Free is available now");
    expect(h).toContain("$290 a year");
    expect(h).toContain("Planned");
    expect(html(<HomePage />)).toContain(`href="/pricing/"`);
  });

  it("F-WB-01 the changelog lists every closed milestone", () => {
    const h = html(<ChangelogPage />);
    for (const m of MILESTONES) expect(h).toContain(`id="${m.id.toLowerCase()}"`);
    expect(MILESTONES.map((m) => m.id)).toEqual(["M5", "M4", "M3", "M2", "M1", "M0"]);
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

  it("F-WB-01 Vercel serves the export with headers that forbid framing and third-party origins", () => {
    type Rule = {
      source: string;
      headers: Array<{ key: string; value: string }>;
    };
    const config = JSON.parse(readFileSync(join(import.meta.dirname, "../vercel.json"), "utf8")) as {
      framework: string;
      buildCommand: string;
      outputDirectory?: string;
      headers: Rule[];
    };
    // Vercel's Next.js builder picks up the static export itself; an output
    // directory would point it away from `.next` and fail the build.
    expect(config.framework).toBe("nextjs");
    expect(config.buildCommand).toBe("pnpm build:site");
    expect(config.outputDirectory).toBeUndefined();
    const all = config.headers.find((r) => r.source === "/(.*)")?.headers ?? [];
    const header = (key: string) => all.find((h) => h.key === key)?.value ?? "";
    const csp = header("Content-Security-Policy");
    expect(csp).toContain("frame-ancestors 'none'");
    expect(csp).toContain("default-src 'self'");
    expect(csp).not.toMatch(/https?:/);
    expect(header("X-Content-Type-Options")).toBe("nosniff");
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
