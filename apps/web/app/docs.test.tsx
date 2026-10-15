import { existsSync, readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { DocsShell } from "../components/docs/DocsShell";
import { navGroups } from "../lib/docs/nav";
import { SITE } from "../lib/site";
import { deferChunks } from "../scripts/defer-docs.mjs";

// The docs (F-WB-02): a server-rendered shell on the site's header and
// footer (D-1839), whose Next chunks start only after the first paint
// (D-1819).

const TREE = {
  name: "Docs",
  children: [
    { type: "separator", name: "Get started" },
    { type: "page", name: "Getting started", url: "/docs/" },
    { type: "separator", name: "Modes" },
    { type: "page", name: "Code mode", url: "/docs/code-mode/" },
    { type: "page", name: "Thread mode", url: "/docs/thread-mode/" },
  ],
} as const;

const HTML = [
  '<html><head><link rel="preload" as="script" fetchPriority="low" href="/_next/static/chunks/webpack-1.js"/>',
  '<link rel="stylesheet" href="/_next/static/css/a.css"/></head><body><p>Hi</p>',
  '<script src="/_next/static/chunks/a-1.js" async=""></script>',
  '<script src="/_next/static/chunks/app/docs/%5B%5B...slug%5D%5D/page.site-2.js" async=""></script>',
  '<script src="/_next/static/chunks/polyfills-3.js" noModule=""></script>',
  '<script src="/_next/static/chunks/webpack-1.js" id="_R_" async=""></script>',
  "<script>self.__next_f.push([1])</script></body></html>",
].join("");

describe("docs (F-WB-02)", () => {
  it("F-WB-02 the docs' Next chunks start after the first paint, through one first-party loader (D-1819)", () => {
    const out: string = deferChunks(HTML);
    expect(out).not.toMatch(/<script[^>]*src="\/_next\/static\/chunks/);
    expect(out).not.toMatch(/<link[^>]*as="script"/);
    // The page's own markup, its stylesheet and the inline payload stay.
    expect(out).toContain('<link rel="stylesheet" href="/_next/static/css/a.css"/>');
    expect(out).toContain("<script>self.__next_f.push([1])</script>");
    const loader = /<script src="\/docs-boot\.js" data-chunks="([^"]*)" defer=""><\/script><\/body>/.exec(out);
    expect(loader?.[1]?.split(" ")).toEqual([
      "/_next/static/chunks/a-1.js",
      "/_next/static/chunks/app/docs/%5B%5B...slug%5D%5D/page.site-2.js",
      "/_next/static/chunks/webpack-1.js#_R_",
    ]);
    expect(deferChunks(out)).toBe(out);
  });

  it("F-WB-02 the loader waits for a painted frame and touches nothing but script tags (D-1819)", () => {
    const boot = readFileSync(join(import.meta.dirname, "../public/docs-boot.js"), "utf8");
    expect(boot).toContain("requestAnimationFrame");
    expect(boot).toContain("setTimeout");
    for (const banned of ["http:", "https:", "cookie", "localStorage", "sessionStorage", "fetch(", "sendBeacon"])
      expect(boot, banned).not.toContain(banned);
  });

  it("F-WB-02 the docs render on the server: no Fumadocs client layout, no client components (D-1819)", () => {
    for (const f of ["docs/layout.site.tsx", "docs/[[...slug]]/page.site.tsx"])
      expect(readFileSync(join(import.meta.dirname, f), "utf8"), f).not.toMatch(/fumadocs-ui/);
    const dir = join(import.meta.dirname, "../components/docs");
    for (const f of readdirSync(dir).filter((n) => n.endsWith(".tsx")))
      expect(readFileSync(join(dir, f), "utf8"), f).not.toMatch(/^\s*["']use client["']/m);
    expect(existsSync(join(import.meta.dirname, "../public/docs-boot.js"))).toBe(true);
  });

  it("F-WB-02 the sidebar groups every page under its label, at its slash URL (D-1839)", () => {
    expect(navGroups(TREE as never)).toEqual([
      { title: "Get started", pages: [{ title: "Getting started", url: "/docs/" }] },
      {
        title: "Modes",
        pages: [
          { title: "Code mode", url: "/docs/code-mode/" },
          { title: "Thread mode", url: "/docs/thread-mode/" },
        ],
      },
    ]);
  });

  it("F-WB-02 the shell marks the page shown, lists its sections and carries the site's header and footer (D-1839)", () => {
    const h = renderToStaticMarkup(
      <DocsShell
        tree={TREE as never}
        url="/docs/code-mode/"
        toc={[
          { title: "Worktrees", url: "#worktrees", depth: 2 },
          { title: "Setup", url: "#setup", depth: 3 },
        ]}
      >
        <h1>Code mode</h1>
      </DocsShell>,
    );
    expect(h).toContain('id="nd-sidebar"');
    // next/link adds the slash back only under the export's trailingSlash
    // (docs-nav.spec.ts checks the built href).
    expect(h).toMatch(/<a[^>]*data-active="true"[^>]*href="\/docs\/code-mode\/?"/);
    expect(h.match(/data-active="true"/g)?.length).toBe(2); // the sidebar and the phone menu
    expect(h).toContain('href="#worktrees"');
    expect(h).toContain('href="#setup"');
    expect(h.match(/<h1[\s>]/g)?.length).toBe(1);
    for (const href of ["/privacy/", "/terms/", "/security/", "/about/"]) expect(h, href).toContain(`href="${href}"`);
    expect(h).toContain(`mailto:${SITE.contactEmail}`);
    expect(h).not.toContain("<script");
  });
});
