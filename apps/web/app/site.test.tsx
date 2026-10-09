import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import type { ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { PLANS } from "../lib/pricing";
import { MILESTONES, PROGRESS } from "../lib/progress";
import { fileURLToPath } from "node:url";
import { CHECKSUMS_URL, DOWNLOADS, DOWNLOADS_PATH, RELEASE, assetUrl } from "../lib/release";
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
import * as about from "./about/page.site";
import AboutPage from "./about/page.site";
import * as changelog from "./changelog/page.site";
import * as demo from "./demo/page.site";
import DemoPage, { DEMO_APP } from "./demo/page.site";
import * as download from "./download/page.site";
import { metadata as rootMetadata } from "./layout.site";
import * as home from "./page.site";
import * as pricing from "./pricing/page.site";
import * as privacy from "./privacy/page.site";
import * as security from "./security/page.site";
import * as terms from "./terms/page.site";
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
  demo: DemoPage,
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
    expect(h).toContain(`Released · ${RELEASE.version} for Linux`);
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

  it("F-WB-01 the founder's name links to their X profile on About and the home page", () => {
    expect(SITE.founder.link).toBe("https://x.com/jus_eng_njainam");
    for (const Page of [AboutPage, HomePage])
      expect(html(<Page />)).toMatch(/<a href="https:\/\/x\.com\/jus_eng_njainam"[^>]*>Naman Jain<\/a>/);
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
    expect(h).toContain("Built, live when accounts open");
    expect(h).toContain("go live when accounts open");
    expect(h).not.toContain("not built yet");
    // Dashboard approvals have no real source until Thread mode: never claim them.
    expect(h).not.toMatch(/approve or deny tool requests right on the card/i);
  });

  it("F-WB-01 the home page leads to the download, takes feedback by email and shows the real progress figures", () => {
    const h = html(<HomePage />);
    expect(h).toContain(`href="/download/"`);
    expect(h).toContain("Download for Linux");
    expect(EARLY_ACCESS_HREF.startsWith(`mailto:${SITE.contactEmail}?subject=`)).toBe(true);
    expect(h).toContain(PROGRESS.automatedTests.toLocaleString("en-US"));
    expect(h).toContain(`href="/changelog/"`);
  });

  it("F-WB-01 the demo video is a recording of the real app that ships with the site", () => {
    const h = html(<HomePage />);
    expect(h).toMatch(/<video[^>]*controls/);
    expect(h).toContain("Watch it run (1 min)");
    expect(h).toContain(`poster="/screens/product-poster.jpg"`);
    expect(existsSync(join(import.meta.dirname, "../public/screens/product-poster.jpg"))).toBe(true);
    // It has sound, so it doesn't start muted, and it says what's sped up or illustrated.
    expect(h).not.toMatch(/<video[^>]*muted/);
    expect(h).toContain("2.3× speed");
    expect(h).toContain("illustration");
    for (const file of ["product.webm", "product.mp4"]) {
      expect(h).toContain(`src="/screens/${file}"`);
      expect(existsSync(join(import.meta.dirname, `../public/screens/${file}`)), file).toBe(true);
    }
  });

  it("F-WB-01 download offers only the builds that exist, with checksums and install steps", () => {
    const h = html(<DownloadPage />);
    expect(DOWNLOADS.map((d) => d.os)).toEqual(["macos", "windows", "linux"]);
    expect(DOWNLOADS.filter((d) => d.available).map((d) => d.os)).toEqual(["linux"]);
    for (const d of DOWNLOADS) {
      if (d.available) expect(h).toContain(`id="${d.os}"`);
      else expect(h).not.toContain(`id="${d.os}"`);
      for (const a of d.assets) {
        expect(a.file, a.file).toContain(RELEASE.version);
        expect(assetUrl(a.file).startsWith(`${DOWNLOADS_PATH}/`)).toBe(true);
        // No dead links: a build that isn't out yet isn't linked.
        if (d.available) expect(h).toContain(`href="${assetUrl(a.file)}"`);
        else expect(h).not.toContain(`href="${assetUrl(a.file)}"`);
      }
      expect(d.steps.length).toBeGreaterThan(0);
    }
    expect(h).not.toContain("Coming soon");
    expect(h).toContain(`href="${CHECKSUMS_URL}"`);
    expect(h).toContain(`id="getting-started"`);
    // Honest about what this build is.
    expect(h).toContain("isn&#x27;t code-signed, so the checksum is how you confirm");
    expect(h).toContain("contact our servers");
    expect(SITE_PAGES).toContain("/download/");
  });

  it("F-WB-01 says the product is released for Linux and never claims a build that isn't out", () => {
    const pages = [HomePage, DownloadPage, PricingPage].map((P) => html(<P />)).join("\n");
    expect(pages).not.toMatch(/early.access/i);
    expect(pages).not.toMatch(/at launch|before launch|with launch/i);
    // macOS and Windows are only ever "next" or "coming soon", never available.
    expect(pages).not.toMatch(/(for|on) macOS, Windows and Linux/);
    expect(pages).not.toContain(".dmg");
    expect(pages).not.toContain(".msi");
    for (const unfinished of [
      "are next",
      "Coming soon",
      "on sale yet",
      "Planned",
      "isn't code-signed yet",
      "testing early builds",
    ])
      expect(pages, unfinished).not.toContain(unfinished);
  });

  it("F-WB-01 the site is Vibezeo's, serves its own downloads and links no code repository", () => {
    const pages = Object.values(PAGES)
      .map((P) => html(<P />))
      .join("\n");
    expect(pages).not.toMatch(/\bTandem\b/);
    expect(pages).not.toContain("github.com/NamanJain-Engineer/tandem");
    for (const d of DOWNLOADS.filter((x) => x.available))
      for (const a of d.assets)
        expect(
          existsSync(fileURLToPath(new URL(`../public${assetUrl(a.file)}`, import.meta.url))),
          a.file,
        ).toBe(true);
    expect(existsSync(fileURLToPath(new URL(`../public${CHECKSUMS_URL}`, import.meta.url)))).toBe(true);
  });

  it("F-WB-01 every build is named Vibezeo, and same-day platforms are a goal, not a claim", () => {
    for (const d of DOWNLOADS) for (const a of d.assets) expect(a.file, a.file).toMatch(/^Vibezeo_/);
    const h = html(<HomePage />);
    expect(h).toContain("The goal is that Linux, macOS and Windows get the same features on the same day");
    expect(h).toContain(`milestones in the ${RELEASE.version} release`);
  });

  it("F-WB-01 search engines get a canonical URL for every page, a share image and PNG icons", () => {
    const canon = { "/": home, "/about/": about, "/changelog/": changelog, "/download/": download, "/demo/": demo, "/pricing/": pricing, "/privacy/": privacy, "/security/": security, "/terms/": terms };
    expect(Object.keys(canon).sort()).toEqual([...SITE_PAGES].sort());
    for (const [path, mod] of Object.entries(canon)) expect(mod.metadata.alternates?.canonical, path).toBe(path);
    expect(JSON.stringify(rootMetadata.openGraph)).toContain("/og.png");
    expect(robotsTxt()).toContain("Allow: /");
    for (const file of ["og.png", "icon.png", "apple-touch-icon.png"])
      expect(existsSync(join(import.meta.dirname, `../public/${file}`)), file).toBe(true);
  });

  it("F-WB-01 no page shows a year", () => {
    for (const [name, Page] of Object.entries(PAGES))
      expect(html(<Page />), name).not.toMatch(/\b(19|20)\d{2}\b/);
  });

  it("F-WB-01 pricing shows the launch plans from the spec and says Free is available now, Pro and Team not yet", () => {
    const h = html(<PricingPage />);
    expect(PLANS.map((p) => [p.name, p.price])).toEqual([
      ["Free", "$0"],
      ["Pro", "$29"],
      ["Team", "$49"],
    ]);
    for (const p of PLANS) expect(h).toContain(`id="${p.id}"`);
    expect(h).toContain("download Vibezeo");
    expect(h).toContain("start with");
    expect(h).toContain("Free is available now");
    expect(h).toContain("$290 a year");
    expect(h).not.toContain("Planned");
    expect(html(<HomePage />)).toContain(`href="/pricing/"`);
  });

  it("F-WB-01 the changelog lists every closed milestone", () => {
    const h = html(<ChangelogPage />);
    for (const m of MILESTONES) expect(h).toContain(`id="${m.id.toLowerCase()}"`);
    expect(MILESTONES.map((m) => m.id)).toEqual(["M7", "M6", "M5", "M4", "M3", "M2", "M1", "M0"]);
  });

  it("F-WB-01 every page has one h1, the legal links and the contact address", () => {
    for (const [name, Page] of Object.entries(PAGES)) {
      const h = html(<Page />);
      expect(h.match(/<h1[\s>]/g)?.length, name).toBe(1);
      for (const href of ["/privacy/", "/terms/", "/security/"]) expect(h, name).toContain(`href="${href}"`);
      expect(h, name).toContain(SITE.contactEmail);
    }
  });

  it("F-WB-01 the live demo says its agents are simulated and opens the browser build", () => {
    const h = html(<DemoPage />);
    expect(h).toContain(`href="${DEMO_APP}"`);
    expect(h).toContain("simulated");
    expect(h).toContain("nothing you type leaves your browser");
    expect(html(<HomePage />)).toContain(`href="/demo/"`);
    // The demo app is a separate build; the pages themselves stay script-free.
    expect(DEMO_APP).toBe("/demo/app/");
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
