import type { ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { RELEASES, versionPath } from "../lib/changelog";
import { formatSize, releaseAssets } from "../lib/manifest";
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
  it("F-WB-01 /download says it's free with no limits, and never that a release keeps or removes them (D-869, D-2900)", () => {
    const h = html(<DownloadPage />);
    expect(h).toContain("Free, with no paid plans.");
    expect(h).toContain("No limits on what runs on your computer");
    expect(h).not.toMatch(/next release|In this release: up to|\bPro\b/);
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
