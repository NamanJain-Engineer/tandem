import { LIMITS, type Plan as PlanId } from "@tandem/protocol/plans";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import type { ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { RELEASES, versionPath } from "../lib/changelog";
import { formatSize, releaseAssets } from "../lib/manifest";
import { PLAN_GROUPS, PLAN_ROWS, PLANS } from "../lib/pricing";
import { RELEASE } from "../lib/release";
import AboutPage from "./about/page.site";
import ChangelogPage from "./changelog/page.site";
import VersionPage from "./changelog/[version]/page.site";
import DemoPage from "./demo/page.site";
import DownloadPage from "./download/page.site";
import NotFound from "./not-found.site";
import HomePage from "./page.site";
import PricingPage from "./pricing/page.site";
import PrivacyPage from "./privacy/page.site";
import SecurityPage from "./security/page.site";
import TermsPage from "./terms/page.site";

// The redesigned inner pages (M15, §4.4): generated, honest, and free of
// glyphs the self-hosted fonts don't have.

const html = (el: ReactElement) => renderToStaticMarkup(el);
const IDS: PlanId[] = ["free", "pro", "team"];

describe("inner pages", () => {
  it("F-WB-01 /download's second step quotes the welcome screen's real button (DESIGN §5.2, D-1876)", () => {
    const en = JSON.parse(readFileSync(join(import.meta.dirname, "../../desktop/src/i18n/en.json"), "utf8")) as Record<string, string>;
    const h = html(<DownloadPage />);
    expect(en["signin.earlyAccess.start"]).toBe("Start using Vibezeo");
    expect(h).toContain(`choose “${en["signin.earlyAccess.start"]}” on the welcome screen`);
    expect(h).not.toContain("choose Start ");
  });

  it("F-WB-01 the comparison table is LIMITS", () => {
    const h = html(<PricingPage />);
    for (const row of PLAN_ROWS) {
      const tr = new RegExp(`<tr data-limit="${row.key}"[\\s\\S]*?</tr>`).exec(h)?.[0] ?? "";
      expect(tr, row.key).not.toBe("");
      for (const id of IDS) {
        const v = row.value(id);
        const cell = new RegExp(`<td data-plan="${id}"[^>]*>([\\s\\S]*?)</td>`).exec(tr)?.[1] ?? "";
        if (v === true) expect(cell, `${row.key} ${id}`).toContain("Included");
        else if (v === false) expect(cell, `${row.key} ${id}`).toContain("Not included");
        else expect(cell, `${row.key} ${id}`).toContain(`>${v}<`);
      }
    }
    expect(PLAN_ROWS.find((r) => r.key === "panes")?.value("pro")).toBe(String(LIMITS.pro.panes));
  });

  it("F-WB-01 every comparison feature is ticked in REMAINING", () => {
    const remaining = readFileSync(join(import.meta.dirname, "../../../docs/REMAINING.md"), "utf8");
    for (const row of PLAN_GROUPS.flatMap((g) => g.rows))
      if (row.feature) expect(remaining, row.feature).toContain(`- [x] ${row.feature} `);
  });

  it("F-WB-01 plan copy matches enforced limits (H3)", () => {
    const copy = PLANS.flatMap((p) => p.features.map((f) => [p.id, f.text] as const));
    for (const [id, text] of copy) {
      if (/unlimited panes|unlimited live panes|no limit on (live )?panes/i.test(text))
        expect(LIMITS[id].panes, `${id}: ${text}`).toBeNull();
      if (/unlimited workspaces/i.test(text)) expect(LIMITS[id].workspaces, `${id}: ${text}`).toBeNull();
    }
  });

  it("F-WB-01 /download shows the build's size, file and install command", () => {
    const h = html(<DownloadPage />);
    const deb = releaseAssets().get("linux")?.[0];
    expect(deb).toBeDefined();
    expect(h).toContain(formatSize(deb?.bytes ?? 0));
    expect(h).toContain(`sudo apt install ./${deb?.file}`);
    expect(h).toContain(`sha256sum ${deb?.file}`);
    // Shortcuts come from the registry (H5): never the macOS-only spelling.
    expect(h).toContain(">Ctrl<");
    expect(h).not.toContain("Mod+");
  });

  it("F-WB-03 version pages link the newer and older release and offer the download when current", async () => {
    for (const [i, r] of RELEASES.entries()) {
      const h = html(await VersionPage({ params: Promise.resolve({ version: r.version }) }));
      const newer = RELEASES[i - 1];
      const older = RELEASES[i + 1];
      if (newer) expect(h, r.version).toContain(`href="${versionPath(newer.version)}"`);
      if (older) expect(h, r.version).toContain(`href="${versionPath(older.version)}"`);
      if (r.version === RELEASE.version) expect(h).toContain(`href="/download/"`);
    }
  });

  it("F-WB-01 no fallback glyphs in any page's text (icons instead)", async () => {
    const pages = [HomePage, AboutPage, ChangelogPage, DemoPage, DownloadPage, PricingPage, PrivacyPage, TermsPage, SecurityPage, NotFound].map((P) => html(<P />));
    pages.push(html(await VersionPage({ params: Promise.resolve({ version: RELEASE.version }) })));
    for (const [i, h] of pages.entries()) {
      const text = h.replace(/<kbd[\s\S]*?<\/kbd>/g, "").replace(/<[^>]+>/g, "");
      for (const glyph of ["→", "←", "✓", "⌘", "⇧"]) expect(text, `${glyph} in page ${i}: ${/.{40}[→←✓⌘⇧].{10}/.exec(text)?.[0] ?? ""}`).not.toContain(glyph);
    }
  });
});
