import { readFileSync } from "node:fs";
import { join } from "node:path";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { DashboardScene } from "../components/site/scenes/DashboardScene";
import { LandScene } from "../components/site/scenes/LandScene";
import { ReviewScene } from "../components/site/scenes/ReviewScene";
import { Scene } from "../components/site/scenes/Scene";
import { WorkspaceScene } from "../components/site/scenes/WorkspaceScene";
import { Glyph } from "../components/site/scenes/parts";
import { PLAN_RUNS, PlanScene, planAt, planTimes } from "../components/site/scenes/PlanScene";
import { RELEASE_SCENES, releaseScenes } from "../components/site/scenes/releaseScenes";
import { RELEASES, addedCard } from "../lib/changelog";
import { SCENE_ROWS } from "../lib/home";
import { RELEASE } from "../lib/release";
import { LANDS, SAMPLE, SCENE_BUILD, atLeast, t } from "../lib/scenes";
import HomePage from "./page.site";
import VersionPage from "./changelog/[version]/page.site";
import { SCENE_PAGES, THEME_BOOT, allowedScripts } from "../scripts/script-allowlist.mjs";

// The animated product scenes (F-WB-01 AC9, D-1880, D-1881).

const home = renderToStaticMarkup(<HomePage />);
const v090 = renderToStaticMarkup(await VersionPage({ params: Promise.resolve({ version: "0.9.0" }) }));
const css = readFileSync(join(import.meta.dirname, "../components/site/scenes/scenes.css"), "utf8").replace(/\/\*[\s\S]*?\*\//g, "");
const ALL = { workspace: <WorkspaceScene />, dashboard: <DashboardScene />, review: <ReviewScene />, land: <LandScene />, plan: <PlanScene /> };
/** Text as React writes it into HTML. */
const esc = (s: string) => s.replace(/&/g, "&amp;").replace(/'/g, "&#x27;").replace(/"/g, "&quot;").replace(/</g, "&lt;");
const versionHtml = async (version: string) => renderToStaticMarkup(await VersionPage({ params: Promise.resolve({ version }) }));

describe("F-WB-01 AC9 product scenes", () => {
  it("F-WB-01 AC9 a scene is decorative, sits in the bezel and says what happens in it", async () => {
    const h = renderToStaticMarkup(
      <Scene id="x" text="What happens." caption="Short.">
        <WorkspaceScene />
      </Scene>,
    );
    expect(h).toMatch(/^<figure data-scene="x"/);
    expect(h).toMatch(/<div data-stage="" aria-hidden="true" class="frame-screen scene-box">/);
    expect(h).toContain(`Scene from Vibezeo ${SCENE_BUILD}, simplified`);
    expect(h).toContain('<span class="sr-only">What happens.</span>');
    // The home page's scenes, and each release page's (D-1885, D-1888).
    const pages: Array<[string, string]> = ["workspace", ...SCENE_ROWS.filter((r) => atLeast(SCENE_BUILD, r.release)).map((r) => r.id)].map((id) => [id, home]);
    for (const version of Object.keys(RELEASE_SCENES)) {
      const h = await versionHtml(version);
      for (const s of releaseScenes(version)) pages.push([s.id, h]);
    }
    for (const [id, page] of pages) {
      const fig = new RegExp(`<figure data-scene="${id}"[\\s\\S]*?</figure>`).exec(page)?.[0] ?? "";
      expect(fig, id).toContain('aria-hidden="true"');
      expect(fig, id).toMatch(/<span class="sr-only">[^<]{80,}<\/span>/);
    }
  });

  it("F-WB-01 AC9 D-1898 a release page plays each of its scenes full width, directly above its own Added card", async () => {
    expect(Object.keys(RELEASE_SCENES).length).toBeGreaterThan(0);
    for (const version of Object.keys(RELEASE_SCENES)) {
      const h = await versionHtml(version);
      const titles = (RELEASES.find((r) => r.version === version)?.entries ?? [])
        .filter((e) => e.category === "Added")
        .map((e) => addedCard(e.text).title.replace(/\.$/, ""));
      for (const s of releaseScenes(version)) {
        const i = titles.indexOf(s.card);
        expect(i, `${version}: no Added card "${s.card}"`).toBeGreaterThanOrEqual(0);
        const fig = h.indexOf(`<figure data-scene="${s.id}"`);
        const card = h.indexOf(`>${esc(s.card)}.</strong>`);
        expect(fig, `${version} ${s.id}`).toBeGreaterThan(-1);
        expect(fig, `${version} ${s.id} above its card`).toBeLessThan(card);
        // Nothing between the card before it (or the summary) and the scene,
        // and only its own card's opening between the scene and its title.
        const before = i > 0 ? h.indexOf(`>${esc(titles[i - 1] ?? "")}.</strong>`) : h.indexOf("</h1>");
        expect(before).toBeLessThan(fig);
        expect(h.slice(before, fig).match(/<li class="reveal surface/g) ?? []).toEqual([]);
        expect(h.slice(fig, card).match(/<li class="reveal surface/g)?.length).toBe(1);
        const cls = /<figure data-scene="[^"]+" class="([^"]*)"/.exec(h.slice(fig))?.[1] ?? "";
        expect(cls, "full width").toContain("lg:col-span-12");
        // Near the first screen's fold: no scroll reveal to leave it half faded at load.
        expect(cls.split(" "), "no rise").not.toContain("rise");
      }
    }
  });

  it("F-WB-01 AC2 AC9 the scene player loads on exactly the release pages with scenes", async () => {
    expect([...SCENE_PAGES].sort()).toEqual(Object.keys(RELEASE_SCENES).map((v) => `changelog/${v}/index.html`).sort());
    for (const page of SCENE_PAGES) expect(allowedScripts(page), page).toEqual([THEME_BOOT, '<script src="/demo/embed.js" defer="">']);
    for (const r of RELEASES) {
      const h = await versionHtml(r.version);
      expect(h.includes('<script src="/demo/embed.js" defer=""'), r.version).toBe(releaseScenes(r.version).length > 0);
    }
  });

  it("F-WB-01 AC9 the 0.12.0 scene follows a plan through: each run reviewed, two approved, one sent back, Land 2 approved… (D-1888)", () => {
    const h = renderToStaticMarkup(<PlanScene />);
    const { goal, tasks } = SAMPLE.plan;
    // In the shop world, not the goal field's own example (design lead).
    expect(goal).not.toBe(t("plan.goalPlaceholder"));
    // Agent mode's Tasks page, and the app's own words.
    expect(h).toContain(`<span class="sc-mode sc-mode-on">${t("titlebar.mode.agent")}</span>`);
    for (const s of [
      t("tasks.title"),
      t("plan.open"),
      t("tasks.refresh"),
      t("plan.chip", { goal }),
      t("plan.titleOf", { goal }),
      t("plan.started", { n: tasks.length }),
      t("plan.removePlan"),
      t("common.close"),
      `${t("tasks.runs.state.in_review")} · ${t("plan.verdict.approve")}`,
      `${t("tasks.runs.state.in_review")} · ${t("plan.verdict.changes")}`,
      t("tasks.runs.state.running"),
      t("tasks.runs.state.reviewing"),
    ])
      expect(h, s).toContain(esc(s));
    for (const task of tasks) expect(h).toContain(`<span class="sc-mono sc-run-id">${task.id}</span>`);
    // Every run is reviewed before its verdict: Queued, Running, Reviewing, Ready to review.
    expect(PLAN_RUNS.map((r) => r.steps.map((s) => s.state))).toEqual(tasks.map(() => ["queued", "running", "reviewing", "in_review"]));
    expect(PLAN_RUNS.map((r) => r.verdict).sort()).toEqual(["approve", "approve", "changes_requested"]);
    // It ends on 2 approved · 1 with changes requested, with Land 2 approved…
    // primary and Close ghost (PlanWithLead's started footer).
    const end = planAt(Infinity);
    expect(end.summary).toBe(`${t("plan.count.approved", { n: 2 })} · ${t("plan.count.changes", { n: 1 })}`);
    expect(end.chip).toBe("waiting");
    expect(h).toMatch(new RegExp(`<span data-fx-pop=""[^>]*><span class="sc-btn sc-btn-primary">${t("plan.land", { n: 2 })}</span></span>`));
    expect(h).toMatch(new RegExp(`<span data-fx-visit=""[^>]*><span class="sc-btn sc-btn-primary">${t("plan.land", { n: 1 })}</span></span>`));
    expect(h).toMatch(new RegExp(`<span data-fx-out=""[^>]*><span class="sc-btn sc-btn-primary">${t("common.close")}</span></span>`));
    expect(h).toMatch(new RegExp(`<span data-fx-pop=""[^>]*><span class="sc-btn sc-btn-ghost">${t("common.close")}</span></span>`));
    // The count line and the button agree at every moment: both come from the runs.
    for (const at of planTimes()) {
      const s = planAt(at);
      if (s.approved > 0) expect(s.summary, `${at}s`).toContain(t("plan.count.approved", { n: s.approved }));
      else expect(s.summary, `${at}s`).not.toContain(t("plan.verdict.approve").toLowerCase());
      expect(s.chip, `${at}s`).toBe(s.summary.includes(t("plan.count.running", { n: 1 }).slice(2)) || s.summary.includes(t("plan.count.queued", { n: 1 }).slice(2)) ? "working" : "waiting");
      expect(h, `${at}s`).toContain(esc(s.summary));
    }
    // The header chip truncates at 12rem, as TasksPanel's max-w-48: one of
    // the scene's em is the app's rem, and the chip has its own font size.
    const chipFont = Number(/\.sc-plan-chip \{[^}]*font-size: ([\d.]+)em;/.exec(css)?.[1]);
    const goalMax = Number(/\.sc-plan-chip-goal \{[^}]*max-width: ([\d.]+)em;/.exec(css)?.[1]);
    expect(chipFont * goalMax).toBeCloseTo(12, 1);
    expect(css).toMatch(/\.sc-plan-chip-goal \{[^}]*text-overflow: ellipsis;/);
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
    // Drawn from the current build, on the page of the release that brought it (D-1886).
    expect(v090.includes(`Scene from Vibezeo ${SCENE_BUILD}, simplified. New in 0.9.0.`)).toBe(LANDS && (SCENE_BUILD as string) !== "0.9.0");
    expect(home).not.toContain("New in");
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

  it("F-WB-01 AC9 the scenes draw engines as the app does: a tinted 4 px tile per engine, letters in fg (D-1895, D-1887)", () => {
    const g = renderToStaticMarkup(<Glyph engine="Claude Code" />);
    expect(g).toContain("background-color:var(--tn-engine-claude)");
    expect(g).toContain(">C<");
    expect(renderToStaticMarkup(<Glyph engine="Codex" />)).toContain("var(--tn-engine-codex)");
    const rule = /\.sc-glyph \{([^}]*)\}/.exec(css)?.[1] ?? "";
    expect(rule).not.toContain("999px");
    expect(rule).toContain("color: var(--tn-text)");
    // Every engine glyph in every scene carries its tint.
    for (const [id, el] of Object.entries(ALL))
      for (const m of renderToStaticMarkup(el).matchAll(/<span class="sc-glyph(?! sc-glyph-shell)[^"]*"[^>]*>/g)) expect(m[0], id).toMatch(/--tn-engine-/);
  });

  it("F-WB-01 AC9 the scenes follow app-ui's T2 and T3: lit raised surfaces, status as text rather than pills (D-1887)", () => {
    for (const cls of ["sc-ask", "sc-toast", "sc-comment", "sc-dialog"]) expect(new RegExp(`\\.${cls} \\{[^}]*var\\(--tn-elev-[23]\\)`).test(css), cls).toBe(true);
    for (const cls of ["sc-card", "sc-item"]) expect(new RegExp(`\\.${cls} \\{[^}]*var\\(--tn-elev-1\\)`).test(css), cls).toBe(true);
    // A risk is an outlined tag, a check's result is text, a warning sent is a status.
    expect(/\.sc-chip \{([^}]*)\}/.exec(css)?.[1]).not.toMatch(/status-waiting/);
    expect(css).not.toMatch(/\.sc-chip-ok \{[^}]*background/);
    expect(css).not.toContain(".sc-btn-done");
    const w = renderToStaticMarkup(<WorkspaceScene />);
    if (LANDS) expect(w).toMatch(new RegExp(`<span class="sc-status"[^>]*><span class="sc-dot">[\\s\\S]*?</span> ${t("radar.notice.warned")}</span>`));
  });

  it("F-WB-01 AC9 a scene's status bar is the download's: the branch, then the version, and no connection (D-816, D-1888)", () => {
    for (const [id, el] of Object.entries(ALL)) {
      const h = renderToStaticMarkup(el);
      const bar = /<div class="sc-statusbar">[\s\S]*?<\/div>/.exec(h)?.[0] ?? "";
      expect(bar, id).toContain(t("status.version", { version: SCENE_BUILD }));
      expect(bar, id).toMatch(/<span class="sc-statusbar-branch"><svg[^>]*>[\s\S]*?<\/svg>[\w-]+<\/span>/);
      for (const s of [t("status.online"), "Branch ", t("dashboard.barLabel")]) expect(h, `${id}: ${s}`).not.toContain(s);
    }
  });

  it("F-WB-01 AC9 every scene renders, always in the app's dark theme", () => {
    for (const [id, el] of Object.entries(ALL)) {
      const h = renderToStaticMarkup(el);
      expect(h, id).toMatch(/^<div data-theme="dark" class="sc-window/);
      expect(h, id).not.toMatch(/sample data/i);
    }
  });
});
