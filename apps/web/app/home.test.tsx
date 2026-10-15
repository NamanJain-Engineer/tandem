import registry from "../../../crates/tandem-engines/registry.json";
import { existsSync, statSync } from "node:fs";
import { join } from "node:path";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { engineLetters } from "../components/site/home/Engines";
import { RELEASES } from "../lib/changelog";
import { CAPABILITIES, FEATURES, MODE_ROWS, RELEASE_TILE_MAP, linuxFacts, releaseTiles, tilesRelease } from "../lib/home";
import { formatSize, releaseAssets } from "../lib/manifest";
import { SCREENS, VIDEO } from "../lib/media";
import { DOWNLOADS, RELEASE } from "../lib/release";
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

  it('F-WB-01 "New in" tiles come from CHANGELOG: this release\'s Added entries, or the last release that added any', () => {
    // A release that only changes and fixes things (0.7.1) keeps the last
    // feature release's tiles, under that release's version.
    const shipped = RELEASES.slice(RELEASES.findIndex((r) => r.version === RELEASE.version));
    const release = shipped.find((r) => r.entries.some((e) => e.category === "Added"));
    expect(tilesRelease()?.version).toBe(release?.version);
    const added = release?.entries.filter((e) => e.category === "Added") ?? [];
    const tiles = releaseTiles();
    expect(tiles.length).toBeGreaterThan(0);
    expect(home).toContain(`New in ${release?.version}`);
    expect(home).toContain(`href="/changelog/${release?.version}/"`);
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
    expect(home).toMatch(/<img[^>]*data-demo-still[^>]*src="\/demo\/still\.jpg"/);
  });

  it("F-WB-01 AC1 no image downloads with the page: each waits until it's near the screen (D-1816)", () => {
    // The h1 is the largest paint. On a slow connection anything fetched with
    // the page shares it with the stylesheet the h1 waits on, and a video's
    // `poster` is always fetched with the page.
    const imgs = home.match(/<img[^>]*>/g) ?? [];
    expect(imgs.length).toBeGreaterThan(2);
    for (const img of imgs) {
      expect(img).toContain('loading="lazy"');
      expect(img).not.toContain("fetchpriority");
    }
    // The video's poster waits for /demo/embed.js to see it near.
    expect(home).not.toMatch(/<video[^>]*\sposter=/);
    expect(home).toMatch(/<video[^>]*data-poster="\/screens\/product-poster\.jpg"/);
  });

  it("F-WB-01 \"Linux first\" says only what this build ships", () => {
    const facts = linuxFacts();
    const linux = DOWNLOADS.find((d) => d.os === "linux" && d.available);
    for (const a of linux?.assets ?? []) expect(facts[0]).toContain(a.file.slice(a.file.lastIndexOf(".")));
    expect(facts[0]).toContain(linux?.requirement.split(": ")[1] ?? "?");
    // The download carries every release's features up to this one.
    const shipped = RELEASES.slice(RELEASES.findIndex((r) => r.version === RELEASE.version));
    const notes = shipped.flatMap((r) => r.entries.map((e) => e.text)).join(" ");
    for (const word of ["X11", "Wayland", "arm64", "x86-64"]) expect(notes, word).toContain(word);
    for (const f of facts) expect(text(home)).toContain(f);
  });

  it("F-WB-01 every capture of the app names a released build, and its 1x and 2x files ship (D-1838)", () => {
    const versions = RELEASES.map((r) => r.version);
    const figures = home.match(/<figure[^>]*data-capture=""[^>]*>/g) ?? [];
    expect(figures.length).toBe(Object.keys(SCREENS).length);
    for (const shot of Object.values(SCREENS)) {
      expect(versions, shot.src).toContain(shot.build);
      expect(home).toContain(`data-build="${shot.build}"`);
      expect(home).toContain(`Vibezeo ${shot.build} · Linux`);
      const base = join(process.cwd(), "public", shot.src.replace(/\.png$/, ""));
      for (const f of [".png", ...shot.formats.flatMap((e) => [`.${e}`, `@2x.${e}`])])
        expect(existsSync(base + f), base + f).toBe(true);
      expect(home).toContain(`${shot.src.replace(/\.png$/, "")}@2x.avif ${shot.width * 2}w`);
    }
  });
});

