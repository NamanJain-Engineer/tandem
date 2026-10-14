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
import { CREDIT_PRICES, LIMITS } from "@tandem/protocol/plans";
import voiceTools from "../../../api/voice-tools.json";
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

  it("F-WB-02 Conductor's tools and settings come from the catalog the app and the API check calls against", () => {
    const tools = html(<DOCS_COMPONENTS.ConductorToolsTable />);
    const rows = [...tools.matchAll(/<tr data-tool="([^"]+)">(.*?)<\/tr>/g)];
    expect(rows.map((r) => r[1]).sort()).toEqual(voiceTools.tools.map((t) => t.name).sort());
    for (const t of voiceTools.tools) {
      const row = rows.find((r) => r[1] === t.name)?.[2] ?? "";
      expect(row, t.name).toContain(`<code>${t.name}</code>`);
      expect(row, t.name).toContain(`<td>${t.confirm ? "Yes" : "No"}</td>`);
    }
    // Every tool the F-CN-02 table names, in its groups.
    expect(rows).toHaveLength(47);
    expect(tools).toMatch(/<tr data-tool="run_command_in_shell"><td>Panes and agents<\/td>/);
    const settable = html(<DOCS_COMPONENTS.ConductorSettingsTable />);
    expect([...settable.matchAll(/data-setting="([^"]+)"/g)].map((m) => m[1])).toEqual(voiceTools.settable_settings);
    const mdx = readFileSync(content("conductor"), "utf8");
    expect(mdx).toContain("<ConductorToolsTable />");
    expect(mdx).toContain("<ConductorSettingsTable />");
  });

  it("F-WB-02 the Conductor page's numbers are the app's", () => {
    const mdx = readFileSync(content("conductor"), "utf8");
    const api = readFileSync(join(import.meta.dirname, "../../../api/src/services/voice.ts"), "utf8");
    const capSeconds = Number(api.match(/MAX_CALL_SECONDS = (\d+)/)?.[1]);
    expect(mdx).toContain(`${capSeconds / 60}-minute limit`);
    expect(mdx).toContain(`${CREDIT_PRICES.voice_10s} credits for every started 10 seconds`);
    expect(mdx).toContain(`about ${CREDIT_PRICES.voice_10s * 6} credits a minute`);
    // The shortcuts it names are the registry's.
    for (const id of ["conductor.call", "conductor.mute", "conductor.log"]) {
      const key = shortcuts.find((s) => s.id === id)?.pc;
      expect(mdx, id).toContain(`<kbd>${key}</kbd>`);
    }
    // The settings it names are the app's labels.
    for (const k of [
      "conductor.settings.floatingRing",
      "conductor.settings.keepLogs",
      "conductor.settings.keep.off",
      "conductor.memoryToggle",
      "conductor.wake",
      "conductor.wake.pauseVideos",
      "conductor.speed",
      "conductor.reflection",
      "conductor.reflection.ask",
      "conductor.reflection.auto",
      "conductor.reflection.off",
      "conductor.memory.keep",
      "conductor.memory.discard",
      "conductor.memory.forgetAll",
    ])
      expect(mdx, k).toContain(labels[k]);
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
    // Shipped: Agent mode since 0.4.0; Dictate, Tasks, Plugins and SSH workspaces since 0.5.0.
    for (const slug of ["agent-mode", "dictate", "tasks", "plugins", "routines", "remote", "conductor"])
      expect(readFileSync(content(slug), "utf8"), slug).not.toContain("Not in the app yet");
    // Routines and Conductor are in the app but run only on Pro (Conductor on
    // Team too), which this download can't have.
    for (const slug of ["routines", "conductor"])
      expect(readFileSync(content(slug), "utf8"), slug).toContain("**Part of Pro.**");
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
