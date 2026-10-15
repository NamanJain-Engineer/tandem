import shortcuts from "@tandem/protocol/shortcuts.json";
import { existsSync, readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { Keys, keyLabel, linuxKeys } from "../components/site/Keys";
import { SiteFooter } from "../components/site/SiteFooter";
import { SiteHeader } from "../components/site/SiteHeader";
import { FOOTER, NAV } from "../components/site/nav";
import { SITE } from "../lib/site";
import RootLayout, { viewport } from "./layout.site";

// The site's frame (M15 redesign, D-1800–D-1802): header, phone menu,
// footer and theme, all without a script.

describe("site frame", () => {
  it("F-WB-01 phones get every header link from a menu that needs no script (Popover API)", () => {
    const h = renderToStaticMarkup(<SiteHeader />);
    expect(h).toMatch(/popovertarget="site-menu"/i);
    expect(h).toMatch(/<div id="site-menu" popover="auto"/);
    const menu = h.slice(h.indexOf('id="site-menu"'));
    for (const n of NAV) expect(menu, n.href).toContain(`href="${n.href}"`);
    expect(h).not.toContain("<script");
    expect(h).not.toMatch(/<h1[\s>]/);
  });

  it("F-WB-01 the header marks the page being shown", () => {
    const h = renderToStaticMarkup(<SiteHeader current="/pricing/" />);
    expect(h).toMatch(/<a href="\/pricing\/" aria-current="page"/);
    expect(h.match(/aria-current="page"/g)?.length).toBe(2); // the bar and the menu
  });

  it("F-WB-01 the footer carries the legal links, About, the contact address and no year", () => {
    const h = renderToStaticMarkup(<SiteFooter />);
    for (const href of ["/privacy/", "/terms/", "/security/", "/about/", "/download/", "/docs/"])
      expect(h, href).toContain(`href="${href}"`);
    expect(h).toContain(`mailto:${SITE.contactEmail}`);
    expect(h).not.toMatch(/\b(19|20)\d{2}\b/);
    expect(FOOTER.map((c) => c.title)).toEqual(["Product", "Docs", "Company", "Legal"]);
  });

  it("F-WB-01 the site follows the OS theme with CSS only (D-1801)", () => {
    const h = renderToStaticMarkup(<RootLayout>{null}</RootLayout>);
    expect(h).not.toContain("data-theme");
    expect(h).not.toContain("<script");
    expect(viewport.colorScheme).toBe("dark light");
    const css = readFileSync(join(import.meta.dirname, "globals.css"), "utf8");
    expect(css).toContain('@import "@tandem/ui/site.css"');
    // Fonts are self-hosted through next/font, never a remote stylesheet.
    expect(css).not.toMatch(/https?:/);
  });

  it("F-WB-01 shortcuts on the site come from the registry (H5)", () => {
    const split = shortcuts.find((s) => s.id === "pane.split_right");
    expect(linuxKeys("pane.split_right", "terminal")).toEqual(split?.pcTerminal?.split("+"));
    expect(linuxKeys("pane.split_right")).toEqual(split?.pc?.split("+"));
    expect(keyLabel(linuxKeys("pane.split_right", "terminal"))).toBe("Ctrl+Shift+E");
    const caps = renderToStaticMarkup(<Keys id="palette.open" />);
    expect(caps).toContain(">Ctrl<");
    expect(caps).toContain(">K<");
    expect(() => linuxKeys("no.such.command")).toThrow();
  });

  it("F-WB-01 AC2 site components stay server-only, so stripping the runtime can't break them (D-1813)", () => {
    const dir = join(import.meta.dirname, "../components/site");
    const files = readdirSync(dir, { recursive: true, encoding: "utf8" }).filter((f) => /\.tsx?$/.test(f));
    expect(files.length).toBeGreaterThan(10);
    for (const f of files) expect(readFileSync(join(dir, f), "utf8"), f).not.toMatch(/^\s*["']use client["']/m);
  });

  it("F-WB-01 the site's fonts are trimmed to the weights it draws, and it draws no other (D-1844)", () => {
    const fonts = readFileSync(join(import.meta.dirname, "fonts.ts"), "utf8");
    expect(fonts).toContain('src: "./fonts/inter-site.woff2"');
    expect(fonts).toContain('weight: "400 650"');
    expect(fonts).toContain('src: "./fonts/mono-site.woff2"');
    expect(fonts).toContain('weight: "400 500"');
    for (const f of ["inter-site.woff2", "mono-site.woff2", "LICENSE-Inter.txt", "LICENSE-JetBrainsMono.txt"])
      expect(existsSync(join(import.meta.dirname, "fonts", f)), f).toBe(true);
    // Every weight the site's CSS and markup ask for is inside Inter's range.
    const css = readFileSync(join(import.meta.dirname, "../../../packages/ui/src/styles/site.css"), "utf8");
    for (const m of css.matchAll(/font-weight:\s*(\d+)/g)) {
      expect(Number(m[1])).toBeGreaterThanOrEqual(400);
      expect(Number(m[1])).toBeLessThanOrEqual(650);
    }
    const sources = [join(import.meta.dirname, "../components/site"), join(import.meta.dirname, "../components/docs"), import.meta.dirname];
    for (const dir of sources)
      for (const f of readdirSync(dir, { recursive: true, encoding: "utf8" }).filter((n) => /\.site\.tsx$|^[^/]*\/?[A-Z][\w/]*\.tsx$/.test(n) && !n.includes("test")))
        expect(readFileSync(join(dir, f), "utf8"), f).not.toMatch(/\bfont-(thin|extralight|light|bold|extrabold|black)\b/);
  });

  it("F-WB-01 the stylesheet scans every packages/ui component the site and portal import, and no other (D-1844)", () => {
    const css = readFileSync(join(import.meta.dirname, "globals.css"), "utf8");
    const listed = (/packages\/ui\/src\/components\/\{([^}]+)\}\.tsx/.exec(css)?.[1] ?? "").split(",");
    const ui = join(import.meta.dirname, "../../../packages/ui/src/components");
    const fileOf = (name: string) =>
      readdirSync(ui).find((f) => f.endsWith(".tsx") && new RegExp(`export (const|function) ${name}\\b`).test(readFileSync(join(ui, f), "utf8")));
    const used = new Set<string>();
    const web = join(import.meta.dirname, "..");
    for (const dir of ["app", "components", "lib"])
      for (const f of readdirSync(join(web, dir), { recursive: true, encoding: "utf8" }).filter((n) => /\.tsx?$/.test(n) && !n.includes("test"))) {
        const src = readFileSync(join(web, dir, f), "utf8");
        for (const m of src.matchAll(/@tandem\/ui\/components\/(\w+)"/g)) used.add(`${m[1]}.tsx`);
        for (const m of src.matchAll(/import \{([^}]*)\} from "@tandem\/ui"/g))
          for (const name of (m[1] ?? "").split(",").map((n) => n.trim()).filter((n) => /^[A-Z]/.test(n))) {
            const file = fileOf(name);
            if (file) used.add(file);
          }
      }
    // What those components import from each other.
    for (const f of [...used])
      for (const m of readFileSync(join(ui, f), "utf8").matchAll(/from "\.\/(\w+)"/g)) used.add(`${m[1]}.tsx`);
    expect([...used].filter((f) => !listed.includes(f.replace(/\.tsx$/, "")))).toEqual([]);
  });
});
