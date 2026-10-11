import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import type { ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import shortcuts from "@tandem/protocol/shortcuts.json";
import settings from "@tandem/protocol/settings.json";
import en from "../../../desktop/src/i18n/en.json";
import { DocsFooter } from "../../components/docs/DocsFooter";
import { DOCS_COMPONENTS, DOCS_SETTINGS_PAGES, NO_SHORTCUT } from "../../components/docs/Generated";
import { LIMITS } from "@tandem/protocol/plans";
import { DOCS_PAGES } from "../../lib/docs/pages";
import { sitemapXml } from "../../lib/site";

const html = (el: ReactElement) => renderToStaticMarkup(el);
const content = (slug: string) => join(import.meta.dirname, "../../content/docs", `${slug || "index"}.mdx`);
const labels = en as Record<string, string>;

describe("docs (F-WB-02)", () => {
  it("F-WB-02.1 the shortcuts page is the command registry's export, row for row", () => {
    const h = html(<DOCS_COMPONENTS.ShortcutsTable />);
    const rows = [...h.matchAll(/<tr data-command="([^"]+)">(.*?)<\/tr>/g)];
    expect(rows.map((r) => r[1])).toEqual(shortcuts.map((s) => s.id));
    for (const [i, s] of shortcuts.entries()) {
      const row = rows[i]?.[2] ?? "";
      expect(row, s.id).toContain(labels[`cmd.${s.id}`]);
      expect(row, s.id).toContain(s.mac ? `<kbd>${s.mac}</kbd>` : `<td>${NO_SHORTCUT}</td>`);
      expect(row, s.id).toContain(s.pc ? `<kbd>${s.pc}</kbd>` : `<td>${NO_SHORTCUT}</td>`);
    }
    expect(readFileSync(content("keyboard-shortcuts"), "utf8")).toContain("<ShortcutsTable />");
  });

  it("F-WB-02.2 every Settings page has a docs anchor, and every setting is listed under its page", () => {
    const h = html(<DOCS_COMPONENTS.SettingsReference />);
    const pages = new Set(settings.map((s) => s.page));
    for (const p of Object.keys(labels)
      .filter((k) => /^settings\.[a-z]+$/.test(k))
      .map((k) => k.slice(9)))
      if (DOCS_SETTINGS_PAGES.includes(p)) pages.add(p);
    for (const p of pages) expect(h, p).toContain(`id="settings-${p}"`);
    for (const s of settings) expect(h, s.key).toContain(`<code>${s.key}</code>`);
    expect(readFileSync(content("settings"), "utf8")).toContain("<SettingsReference />");
  });

  it("F-WB-02 every page the spec lists exists, and is in the sitemap", () => {
    const titles = DOCS_PAGES.map((p) => p.title);
    for (const t of [
      "Getting started",
      "Code mode",
      "Thread mode",
      "Agent mode",
      "Dictate",
      "Conductor",
      "Tasks",
      "Routines",
      "Plugins and approvals",
      "Accounts and limits",
      "Remote (SSH, WSL, phone)",
      "Privacy and data flows",
      "Keyboard shortcuts",
      "Troubleshooting",
    ])
      expect(titles, t).toContain(t);
    const map = sitemapXml();
    for (const p of DOCS_PAGES) {
      expect(existsSync(content(p.slug)), p.slug).toBe(true);
      const mdx = readFileSync(content(p.slug), "utf8");
      expect(mdx, p.slug).toContain(`title: ${p.title}`);
      expect(map, p.slug).toContain(`/docs/${p.slug ? `${p.slug}/` : ""}</loc>`);
    }
  });

  it("F-WB-02 engines and platforms come from the registry and the release", () => {
    const engines = html(<DOCS_COMPONENTS.EnginesTable />);
    for (const id of ["claude", "codex", "gemini", "cursor-agent", "aider"])
      expect(engines).toContain(`data-engine="${id}"`);
    expect(html(<DOCS_COMPONENTS.PlatformMatrix />)).toContain("Linux");
  });

  it("F-WB-02 plan limits come from the plan definitions the app enforces", () => {
    const h = html(<DOCS_COMPONENTS.PlanLimitsTable />);
    const row = (key: string) => h.match(new RegExp(`<tr data-limit="${key}">(.*?)</tr>`))?.[1] ?? "";
    expect(row("panes")).toContain(`<td>${LIMITS.free.panes}</td>`);
    expect(row("workspaces")).toContain(`<td>${LIMITS.free.workspaces}</td>`);
    expect(row("threadGrid")).toContain(`<td>${LIMITS.pro.threadGrid}</td>`);
    expect(row("chatEngines")).toContain("<td>Claude Code, Codex</td>");
    expect(readFileSync(content("accounts-and-limits"), "utf8")).toContain("<PlanLimitsTable />");
  });

  it("F-WB-02 the sidebar lists the pages in DOCS_PAGES order", () => {
    const meta = JSON.parse(
      readFileSync(join(import.meta.dirname, "../../content/docs/meta.json"), "utf8"),
    ) as {
      pages: string[];
    };
    // Group labels ("---Modes---") are separators, not pages (D-1812).
    expect(meta.pages.filter((p) => !p.startsWith("---"))).toEqual(DOCS_PAGES.map((p) => p.slug || "index"));
  });

  it("F-WB-02 pages for features not in the release say so", () => {
    for (const slug of ["conductor"])
      expect(readFileSync(content(slug), "utf8"), slug).toContain("**Not in the app yet.**");
    // Shipped: Agent mode since 0.4.0; Dictate, Tasks, Plugins and SSH workspaces since 0.5.0.
    for (const slug of ["agent-mode", "dictate", "tasks", "plugins", "routines", "remote"])
      expect(readFileSync(content(slug), "utf8"), slug).not.toContain("Not in the app yet");
    // Routines are in the app but run only on Pro, which this download can't have.
    expect(readFileSync(content("routines"), "utf8")).toContain("**Part of Pro.**");
  });

  it("F-WB-02 every docs page has the legal links and contact (D-1812)", () => {
    const h = html(<DocsFooter />);
    for (const href of ["/privacy/", "/terms/", "/security/", "/about/"]) expect(h, href).toContain(`href="${href}"`);
    expect(h).toContain("hello@vibezeo.com");
    expect(h).not.toMatch(/\b(19|20)\d{2}\b/);
    const page = readFileSync(join(import.meta.dirname, "[[...slug]]/page.site.tsx"), "utf8");
    expect(page).toContain("<DocsFooter />");
  });
});
