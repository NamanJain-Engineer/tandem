import { LIMITS } from "@tandem/protocol/plans";
import type { ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { RELEASES, versionPath } from "../lib/changelog";
import { formatSize, releaseAssets } from "../lib/manifest";
import { planLimitsTable } from "../lib/docs/tables";
import { LIMIT_ROWS } from "../lib/limits";
import { RELEASE } from "../lib/release";
import AboutPage from "./about/page.site";
import ChangelogPage from "./changelog/page.site";
import VersionPage from "./changelog/[version]/page.site";
import DemoPage from "./demo/page.site";
import DownloadPage from "./download/page.site";
import NotFound from "./not-found.site";
import HomePage from "./page.site";
import PrivacyPage from "./privacy/page.site";
import SecurityPage from "./security/page.site";
import TermsPage from "./terms/page.site";

// The redesigned inner pages (M15, §4.4): generated, honest, and free of
// glyphs the self-hosted fonts don't have.

const html = (el: ReactElement) => renderToStaticMarkup(el);

describe("inner pages", () => {
  it("F-WB-01 the docs' limits table is this release's LIMITS.free, with no plans beside it (D-869)", () => {
    const table = planLimitsTable();
    expect(table.head).toEqual(["Limit", "This release"]);
    const value = (key: string) => LIMIT_ROWS.find((r) => r.key === key)?.value;
    expect(value("panes")).toBe(String(LIMITS.free.panes));
    expect(value("workspaces")).toBe(String(LIMITS.free.workspaces));
    expect(value("threadGrid")).toBe(String(LIMITS.free.threadGrid));
    expect(value("accountsPerEngine")).toBe(String(LIMITS.free.accountsPerEngine));
    expect(value("chatEngines")).toBe("Claude Code, Codex");
    expect(table.rows.map((r) => r.key)).toEqual(LIMIT_ROWS.map((r) => r.key));
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
    const pages = [HomePage, AboutPage, ChangelogPage, DemoPage, DownloadPage, PrivacyPage, TermsPage, SecurityPage, NotFound].map((P) => html(<P />));
    pages.push(html(await VersionPage({ params: Promise.resolve({ version: RELEASE.version }) })));
    for (const [i, h] of pages.entries()) {
      const text = h.replace(/<kbd[\s\S]*?<\/kbd>/g, "").replace(/<[^>]+>/g, "");
      for (const glyph of ["→", "←", "✓", "⌘", "⇧"]) expect(text, `${glyph} in page ${i}: ${/.{40}[→←✓⌘⇧].{10}/.exec(text)?.[0] ?? ""}`).not.toContain(glyph);
    }
  });
});
