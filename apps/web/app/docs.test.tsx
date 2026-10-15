import { existsSync, readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { DocsShell } from "../components/docs/DocsShell";
import { navGroups } from "../lib/docs/nav";
import { SITE } from "../lib/site";

// The docs (F-WB-02): a server-rendered shell on the site's header and
// footer (D-1839) that runs no script at all (D-1843).

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


describe("docs (F-WB-02)", () => {
  it("F-WB-02 the docs run no script: strip-runtime treats them as marketing pages, and no docs component needs the router (D-1843)", () => {
    const strip = readFileSync(join(import.meta.dirname, "../scripts/strip-runtime.mjs"), "utf8");
    const skip = /const SKIP = \[([^\]]*)\]/.exec(strip)?.[1] ?? "";
    expect(skip).not.toContain('"docs"');
    expect(skip).toContain('join("demo", "app")');
    for (const f of ["../components/docs/DocsShell.tsx", "docs/[[...slug]]/page.site.tsx", "docs/layout.site.tsx"])
      expect(readFileSync(join(import.meta.dirname, f), "utf8"), f).not.toMatch(/next\/link|fumadocs-ui/);
    const dir = join(import.meta.dirname, "../components/docs");
    for (const f of readdirSync(dir).filter((n) => n.endsWith(".tsx")))
      expect(readFileSync(join(dir, f), "utf8"), f).not.toMatch(/^\s*["']use client["']/m);
    expect(existsSync(join(import.meta.dirname, "../public/docs-boot.js"))).toBe(false);
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
    expect(h).toMatch(/<a[^>]*href="\/docs\/code-mode\/"[^>]*data-active="true"/);
    expect(h.match(/data-active="true"/g)?.length).toBe(2); // the sidebar and the phone menu
    expect(h).toContain('href="#worktrees"');
    expect(h).toContain('href="#setup"');
    expect(h.match(/<h1[\s>]/g)?.length).toBe(1);
    for (const href of ["/privacy/", "/terms/", "/security/", "/about/"]) expect(h, href).toContain(`href="${href}"`);
    expect(h).toContain(`mailto:${SITE.contactEmail}`);
    expect(h).not.toContain("<script");
  });
});
