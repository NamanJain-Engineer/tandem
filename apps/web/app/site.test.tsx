import { FEATURES } from "../lib/home";
import { spawnSync } from "node:child_process";
import { existsSync, readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import type { ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { HERO, OWNERSHIP, PRINCIPLES } from "../lib/home";
import { VIDEO } from "../lib/media";
import { MILESTONES, PROGRESS } from "../lib/progress";
import { fileURLToPath } from "node:url";
import { THEME_BOOT, allowedScripts } from "../scripts/script-allowlist.mjs";
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
import * as privacy from "./privacy/page.site";
import * as security from "./security/page.site";
import * as terms from "./terms/page.site";
import ChangelogPage from "./changelog/page.site";
import DownloadPage from "./download/page.site";
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
  privacy: PrivacyPage,
  terms: TermsPage,
  security: SecurityPage,
  notFound: NotFound,
};

describe("public site", () => {
  it("F-WB-01 home leads with ownership, shows the dev status, every engine and a way to get in touch (D-1841)", () => {
    const h = html(<HomePage />);
    for (const line of HERO.title) expect(h).toContain(line);
    for (const o of OWNERSHIP) expect(h).toContain(o.text);
    expect(h).toContain(`Released · ${RELEASE.version} for Linux`);
    for (const e of ENGINES) expect(h).toContain(`>${e}<`);
    expect(h).toContain(`mailto:${SITE.contactEmail}`);
    for (const id of ["try", "principles", "engines", "features", "capabilities", "faq"])
      expect(h).toContain(`id="${id}"`);
    // Built with Claude and the figures are about us: they moved to /about/
    // with the founder's note (DESIGN §5.1, D-1881).
    for (const id of ["claude", "team"]) expect(h).not.toContain(`id="${id}"`);
    const about = html(<AboutPage />);
    expect(about).toContain('id="claude"');
    // The four principles repeat the ownership chips on the home page; they
    // live on /about/ (D-1897, D-1885).
    for (const p of PRINCIPLES) {
      const body = p.body.replace(/'/g, "&#x27;");
      expect(h).not.toContain(body);
      expect(about).toContain(body);
    }
    // The roadmap and the release tiles live on /changelog/, the video on /demo/.
    for (const id of ["roadmap", "new", "demo"]) expect(h).not.toContain(`id="${id}"`);
    const log = html(<ChangelogPage />);
    for (const id of ["roadmap", "new"]) expect(log).toContain(`id="${id}"`);
    expect(log).toContain("None of this is in the download yet");
  });

  it("F-WB-01 the founder is named on the home and About pages, and Terms and Privacy name who runs the site", () => {
    expect(html(<AboutPage />)).toContain(`${SITE.founder.role}, ${SITE.company}`);
    // The home page's closing band names who builds it and links About (D-1881).
    expect(html(<HomePage />)).toMatch(new RegExp(`Built by ${SITE.founder.name}\\.[\\s\\S]*?<a href="/about/"`));
    for (const Page of [TermsPage, PrivacyPage]) expect(html(<Page />)).toContain(OPERATOR);
    expect(html(<PrivacyPage />)).toContain(`Our host, ${SITE.host}`);
  });

  it("F-WB-01 the founder's name links to their X profile on About", () => {
    expect(SITE.founder.link).toBe("https://x.com/jus_eng_njainam");
    for (const Page of [AboutPage])
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
    // Code mode's capture is on /demo/; the home page shows it as a scene (D-1881).
    const h = html(<HomePage />) + html(<DemoPage />);
    expect(h).toContain(`src="/screens/code-mode.png"`);
    expect(existsSync(join(import.meta.dirname, "../public/screens/code-mode.png"))).toBe(true);
    // The engines section runs its grid full width without the old launch
    // grid capture (D-1897, D-1885).
    expect(html(<HomePage />)).not.toContain("/screens/launch-grid");
    // Only the labelled interactive demo (D-128) runs on sample data.
    const outsideDemo = html(<HomePage />).replace(/<section id="try"[\s\S]*?<\/section>/, "");
    expect(outsideDemo).not.toContain("sample data");
  });

  it("F-WB-01 Built with Claude separates what's shipped from what's built but not live (on /about/, D-1881)", () => {
    const h = html(<AboutPage />);
    expect(h).toContain("Built with Claude");
    expect(h).toContain("--resume");
    // Hosted features take DESIGN §9.5's phrase, word for word (D-869).
    expect(h).toContain("Comes with Vibezeo accounts");
    expect(h).toContain("It uses Vibezeo&#x27;s servers and switches on when Vibezeo accounts open.");
    expect(h).not.toMatch(/\bcredits?\b|\bPro\b|premium|upgrade/i);
    expect(h).not.toContain("not built yet");
    // Dashboard approvals have no real source until Thread mode: never claim them.
    expect(h).not.toMatch(/approve or deny tool requests right on the card/i);
  });

  it("F-WB-01 the home page leads to the download and takes feedback by email; /about/ shows the real progress figures", () => {
    const h = html(<HomePage />);
    expect(h).toContain(`href="/download/"`);
    expect(h).toContain("Download for Linux");
    expect(EARLY_ACCESS_HREF.startsWith(`mailto:${SITE.contactEmail}?subject=`)).toBe(true);
    expect(html(<AboutPage />)).toContain(PROGRESS.automatedTests.toLocaleString("en-US"));
    expect(h).toContain(`href="/changelog/"`);
  });

  it("F-WB-01 the demo video is a recording of the real app that ships with the site, on /demo/ (D-1841)", () => {
    const h = html(<DemoPage />);
    expect(h).toMatch(/<video[^>]*controls/);
    expect(h).toContain("Watch it run (1 min)");
    expect(html(<HomePage />)).toContain('href="/demo/#video"');
    // It loads nothing until it's played and below the fold; its poster
    // waits for the browser's lazy loading of the still above it.
    expect(h).toMatch(/<video[^>]*preload="none"/);
    expect(h).toContain(`poster="${VIDEO.poster}"`);
    expect(existsSync(join(import.meta.dirname, `../public${VIDEO.poster}`))).toBe(true);
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
    const pages = [HomePage, DownloadPage].map((P) => html(<P />)).join("\n");
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
    // Each package keeps its format's naming: Name_version_arch.deb, Name-version-release.arch.rpm (D-832).
    for (const d of DOWNLOADS) for (const a of d.assets) expect(a.file, a.file).toMatch(/^Vibezeo[_-]\d/);
    // A principle, on /about/ with the others (D-1885).
    expect(html(<AboutPage />)).toContain("The goal is that Linux, macOS and Windows get the same features on the same day");
    // The figures moved to /about/ (D-1881).
    expect(html(<AboutPage />)).toContain(`milestones in the ${RELEASE.version} release`);
  });

  it("F-WB-01 search engines get a canonical URL for every page, a share image and PNG icons", () => {
    const canon = { "/": home, "/about/": about, "/changelog/": changelog, "/download/": download, "/demo/": demo, "/privacy/": privacy, "/security/": security, "/terms/": terms };
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

  it("F-WB-01 Vibezeo is free: no pricing page, no prices on any page, and /pricing/ goes to the download page (D-869)", () => {
    expect(existsSync(join(import.meta.dirname, "pricing"))).toBe(false);
    expect(SITE_PAGES).not.toContain("/pricing/");
    const vercel = JSON.parse(readFileSync(join(import.meta.dirname, "../vercel.json"), "utf8")) as {
      redirects: { source: string; destination: string; permanent: boolean }[];
    };
    for (const source of ["/pricing", "/pricing/:rest(.*)"])
      expect(vercel.redirects).toContainEqual({ source, destination: "/download/", permanent: true });
    for (const [name, Page] of Object.entries(PAGES)) {
      const h = html(<Page />);
      expect(h, name).not.toMatch(/\$\d/);
      expect(h, name).not.toContain('href="/pricing/"');
      expect(h, name).not.toMatch(/\bPro and Team\b|per seat|money-back/);
    }
    const home = html(<HomePage />);
    expect(home).toContain("Yes. Vibezeo is free, with no paid plans and no limits on what runs on your computer.");
    expect(html(<DownloadPage />)).toContain("Free, with no paid plans.");
  });

  it("F-TH-06 threads run every chat engine, six side by side, with no plan (D-2900); the home page links Thread mode (D-1881)", () => {
    const home = html(<HomePage />);
    // The home page names Thread mode only as a link to its docs, so the
    // sentence lives in the feature's own text, which the docs carry.
    expect(home).toMatch(/<a href="\/docs\/thread-mode\/"[^>]*>Thread mode<\/a>/);
    expect(FEATURES.find((f) => f.title === "Thread mode")?.body).toContain(
      "Threads run Claude Code, Codex, Cursor Agent, Grok and OpenCode, up to six side by side.",
    );
    expect(home).not.toMatch(
      /need Pro|next release|in the next\b|the next (shows|has|runs|adds|lifts)|this release (has|includes|keeps|shows|runs)/i,
    );
  });

  it("F-WB-01 the changelog lists every closed milestone", () => {
    const h = html(<ChangelogPage />);
    for (const m of MILESTONES) expect(h).toContain(`id="${m.id.toLowerCase()}"`);
    expect(MILESTONES.map((m) => m.id)).toEqual(["M16", "M17", "M14", "M12", "M11", "M10", "M9", "M8", "M7", "M6", "M5", "M4", "M3", "M2", "M1", "M0"]);
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

  it("F-WB-01 AC2 (D-128, D-132) only the home page's demo loader, the download page's OS note and a blog post's JSON-LD and reading script (F-WB-06) load scripts, all first-party; no page has a frame or a third-party resource", () => {
    for (const [name, Page] of Object.entries(PAGES)) {
      const h = html(<Page />);
      const scripts = [...h.matchAll(/<script[^>]*>/g)].map((m) => m[0]);
      const allowed: Record<string, string[]> = {
        home: ['<script src="/demo/embed.js" defer="">'],
        download: ['<script src="/download/detect.js" defer="">'],
      };
      expect(scripts, name).toEqual(allowed[name] ?? []);
      expect(h, name).not.toMatch(/<iframe[\s>]/);
      const external = [...h.matchAll(/src="((?:https?:)?\/\/[^"]+)"/g)].map((m) => m[1]);
      expect(external, name).toEqual([]);
    }
    // A blog post page (F-WB-06, D-1852, D-1860) keeps its JSON-LD and its
    // reading script; every page opens with the theme script (D-1871), which
    // the root layout adds; strip-runtime fails the build on anything else.
    expect(allowedScripts("blog/a-post/index.html")).toEqual([
      THEME_BOOT,
      '<script type="application/ld+json">',
      '<script src="/blog/blog.js" defer="">',
    ]);
    for (const page of ["blog/index.html", "blog/tags/release/index.html", "about/index.html", "changelog/0.8.0/index.html"])
      expect(allowedScripts(page), page).toEqual([THEME_BOOT]);
    // The loader names no other origin and frames only /demo/app/.
    const loader = readFileSync(join(import.meta.dirname, "../public/demo/embed.js"), "utf8");
    expect(loader).not.toMatch(/https?:\/\//);
    expect(loader).toContain("IntersectionObserver");
    expect(loader).toContain('"/demo/app/?embed=1"');
  });

  it("demo AC1 the home page shows the interactive demo: label, line, a still image of the same size, Explore full screen and Tap to explore", () => {
    const h = html(<HomePage />);
    expect(h).toContain("Interactive demo · sample data");
    expect(h).toContain("This is Vibezeo itself, running in your browser with sample data. Click around.");
    expect(h).toContain("agents are simulated");
    expect(h).toMatch(/<img[^>]*src="\/demo\/still\.jpg"[^>]*>/);
    expect(h).toMatch(/data-demo-embed=""/);
    expect(h).toContain("Explore full screen");
    expect(h).toContain("Tap to explore");
    expect(existsSync(join(import.meta.dirname, "../public/demo/still.jpg"))).toBe(true);
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
    expect(header("X-Frame-Options")).toBe("DENY");
    // D-128: only the demo app may be framed, and only by this site.
    const demo = config.headers.find((r) => r.source === "/demo/app/(.*)")?.headers ?? [];
    const demoHeader = (key: string) => demo.find((h) => h.key === key)?.value ?? "";
    expect(config.headers.indexOf(config.headers.find((r) => r.source === "/demo/app/(.*)") as Rule)).toBeGreaterThan(
      config.headers.findIndex((r) => r.source === "/(.*)"),
    );
    expect(demoHeader("X-Frame-Options")).toBe("SAMEORIGIN");
    const demoCsp = demoHeader("Content-Security-Policy");
    expect(demoCsp).toContain("frame-ancestors 'self'");
    expect(demoCsp).toContain("connect-src 'self'");
    expect(demoCsp).not.toMatch(/https?:/);
    expect(header("Permissions-Policy")).toContain("microphone=()");
  });

  it("F-WB-01 Vercel's ignore step builds main only (D-829), and builds when the last deployed commit isn't in its shallow clone (D-809)", () => {
    const web = join(import.meta.dirname, "..");
    const config = JSON.parse(readFileSync(join(web, "vercel.json"), "utf8")) as Record<string, unknown>;
    // Vercel rejects a vercel.json whose commands are over 256 characters.
    for (const key of ["installCommand", "buildCommand", "ignoreCommand"]) {
      expect(String(config[key]).length, key).toBeLessThanOrEqual(256);
    }
    const ignoreCommand = String(config.ignoreCommand);
    // Vercel runs the step in the Root Directory: exit 0 skips, 1 builds.
    const step = (ref: string, previous = "") =>
      spawnSync("sh", ["-c", ignoreCommand], {
        cwd: web,
        env: { ...process.env, VERCEL_GIT_COMMIT_REF: ref, VERCEL_GIT_PREVIOUS_SHA: previous },
      }).status;
    expect(step("m10-teammates-gateway")).toBe(0);
    // No previews since D-829: the integration and release branches are skipped too.
    expect(step("product-m0-m5")).toBe(0);
    expect(step("release-0.3.0")).toBe(0);
    // A commit Vercel's clone doesn't have builds instead of failing on "bad object".
    expect(step("main", "0123456789abcdef0123456789abcdef01234567")).toBe(1);
    const head = spawnSync("git", ["rev-parse", "HEAD"], { cwd: web, encoding: "utf8" }).stdout.trim();
    expect(step("main", head)).toBe(0);
  });

  it("F-WB-01 only main creates Vercel deployments, one per release (D-829)", () => {
    const config = JSON.parse(readFileSync(join(import.meta.dirname, "../vercel.json"), "utf8")) as {
      git?: { deploymentEnabled?: Record<string, boolean> };
    };
    // Minimatch rules; a branch deploys when any rule that matches it is true.
    expect(config.git?.deploymentEnabled).toEqual({ "**": false, main: true });
  });

  it("F-WB-01 the contact address is contact@vibezeo.com, and no docs page names the old one (D-1818)", () => {
    expect(SITE.contactEmail).toBe("contact@vibezeo.com");
    const content = join(import.meta.dirname, "..", "content");
    const files = readdirSync(content, { recursive: true, encoding: "utf8" }).filter((f) => /\.mdx?$/.test(f));
    expect(files.length).toBeGreaterThan(0);
    for (const f of files) expect(readFileSync(join(content, f), "utf8"), f).not.toContain("hello@vibezeo.com");
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
