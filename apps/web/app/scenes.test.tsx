import { readFileSync } from "node:fs";
import { join } from "node:path";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { DashboardScene } from "../components/site/scenes/DashboardScene";
import { LandScene } from "../components/site/scenes/LandScene";
import { ReviewScene } from "../components/site/scenes/ReviewScene";
import { Scene } from "../components/site/scenes/Scene";
import { WorkspaceScene } from "../components/site/scenes/WorkspaceScene";
import { SCENE_ROWS } from "../lib/home";
import { RELEASE } from "../lib/release";
import { LANDS, SAMPLE, SCENE_BUILD, atLeast, t } from "../lib/scenes";
import HomePage from "./page.site";
import VersionPage from "./changelog/[version]/page.site";
import { THEME_BOOT, allowedScripts } from "../scripts/script-allowlist.mjs";

// The animated product scenes (F-WB-01 AC9, D-1880, D-1881).

const home = renderToStaticMarkup(<HomePage />);
const v090 = renderToStaticMarkup(await VersionPage({ params: Promise.resolve({ version: "0.9.0" }) }));
const css = readFileSync(join(import.meta.dirname, "../components/site/scenes/scenes.css"), "utf8").replace(/\/\*[\s\S]*?\*\//g, "");
const ALL = { workspace: <WorkspaceScene />, dashboard: <DashboardScene />, review: <ReviewScene />, land: <LandScene /> };

describe("F-WB-01 AC9 product scenes", () => {
  it("F-WB-01 AC9 a scene is decorative, sits in the bezel and says what happens in it", () => {
    const h = renderToStaticMarkup(
      <Scene id="x" text="What happens." caption="Short.">
        <WorkspaceScene />
      </Scene>,
    );
    expect(h).toMatch(/^<figure data-scene="x"/);
    expect(h).toMatch(/<div data-stage="" aria-hidden="true" class="frame-screen scene-box">/);
    expect(h).toContain(`Scene from Vibezeo ${SCENE_BUILD}, simplified`);
    expect(h).toContain('<span class="sr-only">What happens.</span>');
    // The home page's scenes, and the Land scene on 0.9.0's page (D-1885).
    const pages: Array<[string, string]> = ["workspace", ...SCENE_ROWS.filter((r) => atLeast(SCENE_BUILD, r.release)).map((r) => r.id)].map((id) => [id, home]);
    if (LANDS) pages.push(["land", v090]);
    for (const [id, page] of pages) {
      const fig = new RegExp(`<figure data-scene="${id}"[\\s\\S]*?</figure>`).exec(page)?.[0] ?? "";
      expect(fig, id).toContain('aria-hidden="true"');
      expect(fig, id).toMatch(/<span class="sr-only">[^<]{80,}<\/span>/);
    }
  });

  it("F-WB-01 AC9 the app's own words come from its catalog, with plurals, and a missing one fails the build", () => {
    expect(t("review.stat", { n: SAMPLE.diff.files, adds: 62, dels: 18 })).toBe("4 files · +62 −18");
    expect(t("review.stat", { n: 1, adds: 1, dels: 0 })).toBe("1 file · +1 −0");
    expect(t("dashboard.chip.waiting", { n: 1 })).toBe("Waiting 1");
    expect(t("rail.live", { n: 4 })).toBe("4 running sessions");
    // A value inside a plural's form (landing.done's {base}).
    expect(t("landing.done", { n: 3, base: "main" })).toBe("Landed 3 worktrees in main");
    expect(t("landing.done", { n: 1, base: "main" })).toBe("Landed 1 worktree in main");
    expect(() => t("no.such.key")).toThrow(/no "no.such.key"/);
    const w = renderToStaticMarkup(<WorkspaceScene />);
    for (const s of [t("titlebar.mode.code"), t("pane.engine", { engine: "Claude Code", n: 1 }), t("dashboard.allowOnce"), t("review.diffTitle", { name: "Claude Code" })])
      expect(w, s).toContain(s);
  });

  it("F-WB-01 AC9 D-128 the 0.9.0 scenes and the intent radar show only once that release is out", async () => {
    expect(LANDS).toBe(atLeast(RELEASE.version, "0.9.0"));
    expect(home.includes(t("radar.notice.warn"))).toBe(LANDS);
    // The Land scene plays on 0.9.0's changelog page, not the home page
    // (D-1897, D-1885), with the scene player allowed there.
    expect(home).not.toContain('data-scene="land"');
    expect(v090.includes('data-scene="land"')).toBe(LANDS);
    expect(v090.includes('<script src="/demo/embed.js" defer=""')).toBe(LANDS);
    expect(allowedScripts("changelog/0.9.0/index.html")).toEqual([THEME_BOOT, '<script src="/demo/embed.js" defer="">']);
    const v080 = renderToStaticMarkup(await VersionPage({ params: Promise.resolve({ version: "0.8.0" }) }));
    expect(v080).not.toContain("data-scene");
    expect(v080).not.toContain("embed.js");
    // The scene itself renders, ready for the release.
    expect(renderToStaticMarkup(<LandScene />)).toContain(t("landing.done", { n: 3, base: "main" }));
    expect(atLeast("0.10.0", "0.9.0")).toBe(true);
    expect(atLeast("0.8.0", "0.9.0")).toBe(false);
  });

  it("F-WB-01 AC8 AC9 the markup is the final frame: what leaves is marked, and hidden there", () => {
    const final = renderToStaticMarkup(<ReviewScene />);
    expect(final).toContain(t("worktree.opinion.done", { engine: "Codex", n: 2 }));
    expect(final).toContain(t("worktree.review.send"));
    for (const c of SAMPLE.comments) expect(final).toContain(c.text);
    // "Codex is reviewing…" comes and goes.
    expect(final).toMatch(new RegExp(`data-fx-visit=""[^>]*>[\\s\\S]{0,1500}?${t("worktree.opinion.reviewing", { engine: "Codex" })}`));
    expect(css).toMatch(/\[data-fx-visit\], \[data-fx-out\] \{ opacity: 0; \}/);
  });

  it("F-WB-01 AC8 AC9 effects run only with motion allowed and the script present, paused until the scene is on screen", () => {
    const [before, rest] = css.split("@media screen and (prefers-reduced-motion: no-preference) {");
    expect(rest).toBeDefined();
    expect(before).not.toMatch(/animation\s*:/);
    // data-fx is the theme script's mark, kept only while the scene player
    // runs (it drops it at load otherwise), so a blocked player shows the
    // final frame rather than frame 0.
    expect(rest).toContain(":root[data-fx] [data-scene] {");
    expect(css).toMatch(/\[data-scene\]:not\(\[data-live\]\) :is\(\.tn-dot, \.tn-attention\) \{ animation-play-state: paused; \}/);
    for (const m of (rest ?? "").matchAll(/& \[data-fx-[\w]+\]\s*\{ animation: ([^;]+);/g)) expect(m[1], m[0]).toMatch(/ paused$/);
    expect(rest).toMatch(/&\[data-live\] :is\([^)]*\) \{\s*animation-play-state: running;/);
  });

  it("F-WB-01 AC9 scenes draw with the app's tokens, never literal colours (D-1883)", () => {
    expect(css).not.toMatch(/#[0-9a-f]{3,8}\b/i);
    for (const m of css.matchAll(/rgba?\(.{0,30}/g)) expect(m[0]).toMatch(/^rgb\(var\(--tn-shadow-color\) \//);
  });

  it("F-WB-01 AC9 scene keyframes move only transform and opacity (and the counter)", () => {
    for (const m of css.matchAll(/@keyframes ([\w-]+) \{([\s\S]*?\})\s*\}/g)) {
      const props = [...(m[2] ?? "").matchAll(/([\w-]+)\s*:/g)].map((p) => p[1]);
      for (const p of props) expect(["opacity", "translate", "scale", "--sc-n"], `${m[1]}: ${p}`).toContain(p);
    }
  });

  it("F-WB-01 AC9 the scene script plays a scene only while it's on screen and the tab is visible", () => {
    const js = readFileSync(join(import.meta.dirname, "../public/demo/embed.js"), "utf8");
    expect(js).toContain('document.querySelectorAll("[data-scene]")');
    expect(js).toContain('toggleAttribute("data-live", seen.has(scene) && !document.hidden)');
    expect(js).toContain('addEventListener("visibilitychange", sync)');
    expect(js).toContain('document.documentElement.setAttribute("data-scenes", "")');
    // The hero's window starts low on a laptop: 120 px of it is enough.
    expect(js).toContain("x.intersectionRect.height >= 120");
  });

  it("F-WB-01 AC9 every scene renders, always in the app's dark theme", () => {
    for (const [id, el] of Object.entries(ALL)) {
      const h = renderToStaticMarkup(el);
      expect(h, id).toMatch(/^<div data-theme="dark" class="sc-window/);
      expect(h, id).not.toMatch(/sample data/i);
    }
  });
});
