import registry from "../../../crates/tandem-engines/registry.json";
import { statSync } from "node:fs";
import { join } from "node:path";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { engineLetters } from "../components/site/home/Engines";
import { RELEASES } from "../lib/changelog";
import { CAPABILITIES, FEATURES, MODE_ROWS, RELEASE_TILE_MAP, linuxFacts, releaseTiles } from "../lib/home";
import { formatSize, releaseAssets } from "../lib/manifest";
import { VIDEO } from "../lib/media";
import { RELEASE } from "../lib/release";
import { ENGINES, SITE } from "../lib/site";
import HomePage from "./page.site";

// The redesigned home page (M15, §4.3): everything on it is generated from
// code or checked against it.

const home = renderToStaticMarkup(<HomePage />);
const text = (html: string) => html.replace(/<[^>]+>/g, "");

describe("home page", () => {
  it("F-WB-01 the h1 is the tagline, word for word", () => {
    const h1 = /<h1[^>]*>([\s\S]*?)<\/h1>/.exec(home)?.[1] ?? "";
    expect(text(h1)).toBe(SITE.tagline);
  });

  it('F-WB-01 "New in" tiles come from CHANGELOG', () => {
    const release = RELEASES.find((r) => r.version === RELEASE.version);
    const added = release?.entries.filter((e) => e.category === "Added") ?? [];
    const tiles = releaseTiles();
    expect(tiles.length).toBeGreaterThan(0);
    expect(tiles.map((t) => t.title)).toEqual(
      added.map((e) => /^\*\*(.+?)\.?\*\*/.exec(e.text)?.[1]).filter((t) => t !== undefined),
    );
    for (const t of tiles) {
      expect(t.text.startsWith(t.body), t.title).toBe(true);
      expect(RELEASE_TILE_MAP[t.title]?.docs, t.title).toMatch(/^\/docs\//);
      expect(home).toContain(`href="${t.docs}"`);
      expect(home).toContain(`>${t.title}<`);
    }
    expect(home).toContain(`id="new"`);
  });

  it("F-WB-01 every feature appears once: a mode row, a release tile or the capability index", () => {
    const placed = [
      ...MODE_ROWS.flatMap((r) => [...r.features]),
      ...CAPABILITIES.map((c) => c.title),
      ...Object.values(RELEASE_TILE_MAP).flatMap((m) => (m.feature ? [m.feature] : [])),
    ];
    expect([...placed].sort()).toEqual(FEATURES.map((f) => f.title).sort());
  });

  it("F-WB-01 engine monograms come from the registry", () => {
    for (const e of ENGINES) {
      const entry = registry.find((r) => r.label === e);
      expect(entry, e).toBeDefined();
      expect(engineLetters(e)).toBe(entry?.logo);
      expect(home).toContain(`>${e}<`);
    }
  });

  it("F-WB-01 the video and screenshots name the build they show (H1)", () => {
    const caption = /<figcaption[^>]*>(Every shot of the app[^<]*)<\/figcaption>/.exec(home)?.[1] ?? "";
    expect(caption).not.toBe("");
    expect(caption).toContain(VIDEO.recordedWith);
    if ((VIDEO.recordedWith as string) !== RELEASE.version) expect(caption).not.toContain(RELEASE.version);
    expect(home).toMatch(/<video[^>]*preload="none"/);
    expect(home).not.toMatch(/<video[^>]*autoplay/i);
  });

  it("F-WB-01 the download size shown is the served file's", () => {
    const deb = releaseAssets().get("linux")?.[0];
    expect(deb).toBeDefined();
    const bytes = statSync(join(process.cwd(), "public/downloads", deb?.file ?? "")).size;
    expect(home).toContain(formatSize(bytes));
  });

  it("F-WB-01 no fallback glyphs in the home page's text (icons instead)", () => {
    const outsideKbd = text(home.replace(/<kbd[\s\S]*?<\/kbd>/g, ""));
    for (const glyph of ["→", "✓", "⌘", "⇧"]) expect(outsideKbd, glyph).not.toContain(glyph);
  });

  it("F-WB-01 the phone menu, the demo stage and the FAQ need no script beyond the demo loader", () => {
    expect(home.match(/<script/g)?.length).toBe(1);
    expect(home).toMatch(/<details[^>]*name="faq"/);
    expect(home).toMatch(/<img[^>]*src="\/demo\/still\.jpg"[^>]*fetchpriority="high"/);
  });

  it("F-WB-01 \"Linux first\" says only what this build ships", () => {
    const facts = linuxFacts();
    expect(facts[0]).toContain("Ubuntu 22.04+");
    const notes = (RELEASES.find((r) => r.version === RELEASE.version)?.entries ?? []).map((e) => e.text).join(" ");
    for (const word of ["X11", "Wayland", "arm64", "x86-64"]) expect(notes, word).toContain(word);
    for (const f of facts) expect(text(home)).toContain(f);
  });
});
