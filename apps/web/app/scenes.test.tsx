import { execFileSync } from "node:child_process";
import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { DashboardScene } from "../components/site/scenes/DashboardScene";
import { LandScene } from "../components/site/scenes/LandScene";
import { ReviewScene } from "../components/site/scenes/ReviewScene";
import { Scene } from "../components/site/scenes/Scene";
import { HERO_DONE, WorkspaceScene } from "../components/site/scenes/WorkspaceScene";
import { Glyph } from "../components/site/scenes/parts";
import { PLAN_RUNS, PlanScene, planAt, planTimes } from "../components/site/scenes/PlanScene";
import { RELEASE_SCENES, releaseScenes } from "../components/site/scenes/releaseScenes";
import { RELEASES, addedCard } from "../lib/changelog";
import { SCENE_ROWS } from "../lib/home";
import { RELEASE } from "../lib/release";
import { LANDS, SAMPLE, SCENE_BUILD, atLeast, promptTitle, t } from "../lib/scenes";
import HomePage from "./page.site";
import VersionPage from "./changelog/[version]/page.site";
import { SCENE_PAGES, THEME_BOOT, allowedScripts } from "../scripts/script-allowlist.mjs";
import story from "../../desktop/src/demo/story.json";
import { REVIEW_DIFF, REVIEW_FILE, notesFor } from "../components/site/scenes/ReviewScene";
import names from "../../desktop/src/data/names.json";
import { parsePatch } from "@tandem/ui/components/DiffView";

// The animated product scenes (F-WB-01 AC9, D-1880, D-1881).

const home = renderToStaticMarkup(<HomePage />);
const v090 = renderToStaticMarkup(await VersionPage({ params: Promise.resolve({ version: "0.9.0" }) }));
const css = readFileSync(join(import.meta.dirname, "../components/site/scenes/scenes.css"), "utf8").replace(/\/\*[\s\S]*?\*\//g, "");
const ALL = { workspace: <WorkspaceScene />, dashboard: <DashboardScene />, review: <ReviewScene />, land: <LandScene />, plan: <PlanScene /> };
/** Text as React writes it into HTML. */
const esc = (s: string) => s.replace(/&/g, "&amp;").replace(/'/g, "&#x27;").replace(/"/g, "&quot;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
/** A string as a regular expression matches it. */
const re = (x: string) => x.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
/** git's own diff of `file` from main to `place` in the demo's sample
 * repository, as DiffView parses it, in the scenes' row form: what the app
 * shows, since its diffs are git's. */
function gitDiff(place: string, file: string): Array<readonly [string, string, number | null, number | null]> {
  const shop = join(import.meta.dirname, "../../desktop/src/demo/shop");
  let patch = "";
  try {
    execFileSync("git", ["diff", "--no-index", "--no-color", "-U3", join(shop, "main", `${file}.sample`), join(shop, place, `${file}.sample`)], { encoding: "utf8" });
  } catch (e) {
    // git diff exits 1 when the files differ.
    patch = (e as { stdout: string }).stdout;
  }
  return parsePatch(patch).flatMap((l) =>
    l.kind === "context" || l.kind === "add" || l.kind === "del" ? [[l.kind === "add" ? "+" : l.kind === "del" ? "−" : " ", l.text, l.oldNo, l.newNo] as const] : [],
  );
}
/** True when `rows` run, in order and whole, somewhere in `all`. */
const runsIn = (rows: ReadonlyArray<readonly unknown[]>, all: ReadonlyArray<readonly unknown[]>) =>
  all.some((_, i) => rows.every((r, k) => JSON.stringify(all[i + k]) === JSON.stringify(r)));
/** HTML's text, tags dropped (entities kept). */
const text = (h: string) => h.replace(/<[^>]+>/g, "");
const versionHtml = async (version: string) => renderToStaticMarkup(await VersionPage({ params: Promise.resolve({ version }) }));
/** When the pop that last starts before `s` in `h` plays: the time of the
 * element that brings `s` in. */
const popBefore = (h: string, s: string) => [...h.slice(0, h.indexOf(esc(s))).matchAll(/data-fx-pop="" style="--at:([\d.]+)s"/g)].at(-1)?.[1];
/** A line comment in the Review scene's diff (DiffView's annotation row). */
const NOTE = /<span class="sc-diff-note"[^>]*><span class="sc-diff-note-text">[^<]*<\/span>(?:<span class="sc-btn sc-btn-ghost">[^<]*<\/span>)*<\/span>/g;
/** The Review scene's diff body: the rows down to the first commented line,
 * then the stack under it: the rows as they were (`was`, leaving at
 * `wasAt`) and with the comments (`now`, arriving at `nowAt`), with each
 * stack's effect (`wasFx`, `nowFx`). */
function reviewBody(h: string) {
  const body = /<div class="sc-diff-body">([\s\S]*?)<\/div>/.exec(h)?.[1] ?? "";
  const m =
    /^([\s\S]*?)<span class="sc-diff-swap"><span class="sc-diff-rows" data-fx-(\w+)="" style="--at:([\d.]+)s">([\s\S]*?)<\/span><span class="sc-diff-rows" data-fx-(\w+)="" style="--at:([\d.]+)s">([\s\S]*)<\/span><\/span>$/.exec(
      body,
    );
  return { above: m?.[1] ?? "", wasFx: m?.[2], wasAt: m?.[3], was: m?.[4] ?? "", nowFx: m?.[5], nowAt: m?.[6], now: m?.[7] ?? "" };
}

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
    // It ends on 2 approved · 1 changes requested, with Land 2 approved…
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

  it("F-WB-01 AC9 D-1899 the scenes' sample world is story.json's, the one the demo shows, with no copy kept here (D-2609)", () => {
    expect(SAMPLE.workspace).toBe(story.repo);
    expect(SAMPLE.base).toBe(story.base);
    expect(SAMPLE.check).toBe(story.check);
    expect(SAMPLE.symbol).toBe(story.overlap.symbol);
    // Each role is the worktree in that state: one done, one working, one
    // waiting on you, whichever engines the story gives them (D-3000).
    for (const [role, state] of [["finished", "done"], ["working", "working"], ["waiting", "waiting"]] as const) {
      const all = story.worktrees.filter((w) => w.state === state);
      expect(all.length, state).toBe(1);
      const w = all[0];
      expect(SAMPLE.worktrees[role].branch).toBe(w?.slug);
      expect(SAMPLE.worktrees[role].engine).toBe(story.engines[w?.engine as keyof typeof story.engines].label);
      // Its agent's first name (F-CD-12), from the app's own list.
      expect(SAMPLE.worktrees[role].name).toBe(w?.agentName);
      expect(names).toContain(w?.agentName);
      expect(SAMPLE.lines[role][0]).toBe(`› ${w?.task}`);
      // Every step the story gives the agent is a line in its pane.
      expect(SAMPLE.lines[role].length).toBe(1 + (w?.steps ?? []).reduce((n, st) => n + ("passed" in st && st.passed ? 2 : 1), 0));
    }
    expect(SAMPLE.lines.working).toContain("  $ pnpm test coupon");
    expect(SAMPLE.lines.shell).toEqual([`$ ${story.dev.command}`, `  ready on localhost:${story.dev.port}`, ...story.dev.requests.map((r) => `  ${r}`)]);
    expect(SAMPLE.ask).toBe(story.worktrees.find((w) => w.state === "waiting")?.ask);
    // The review card's figures are the sum of the finished worktree's edits.
    const done = story.worktrees.find((w) => w.state === "done");
    const edits = (done?.steps ?? []).filter((st) => "edit" in st) as Array<{ edit: string; add: number; del: number }>;
    expect(SAMPLE.diff).toEqual({ files: edits.length, adds: edits.reduce((n, e) => n + e.add, 0), dels: edits.reduce((n, e) => n + e.del, 0) });
    // "Touches auth paths" is earned: one of those files is on an auth path (review_queue.rs is_auth).
    expect(edits.some((e) => e.edit.split("/").some((seg) => /auth|oauth|login|password|secret|crypto|permission/i.test(seg) && !/^author/i.test(seg)))).toBe(true);
    expect(SAMPLE.comments).toEqual(story.secondOpinion.comments.map((c) => ({ file: c.file, line: c.line, text: c.body })));
    expect(SAMPLE.reviewer).toBe(story.engines[story.secondOpinion.engine as keyof typeof story.engines].label);
    // The worktrees in the story's order, each with its own merge commit.
    expect(SAMPLE.order.map((w) => [w.branch, w.merge])).toEqual(story.worktrees.map((w) => [w.slug, w.mergeCommit]));
    expect(SAMPLE.failedCheck).toBe(story.pr.checks.find((c) => c.bucket === "fail")?.name);
    // The plan (0.12.0's scene): its goal, and its tasks in order, numbered
    // as the tracker numbers them, each with its engine's name.
    expect(SAMPLE.plan.goal).toBe(story.plan.goal);
    expect(SAMPLE.plan.tasks).toEqual(story.plan.tasks.map((x, i) => ({ id: `TAN-${31 + i}`, title: x.title, engine: story.engines[x.engine as keyof typeof story.engines].label })));
    // No copies: the world's names come only from story.json, in lib/scenes.ts
    // and in every scene (D-3000).
    const dir = join(import.meta.dirname, "../components/site/scenes");
    const sources = [join(import.meta.dirname, "../lib/scenes.ts"), ...readdirSync(dir).filter((f) => f.endsWith(".tsx")).map((f) => join(dir, f))];
    const copies = [
      story.repo,
      story.overlap.symbol,
      ...story.worktrees.flatMap((w) => [w.slug, w.task, w.mergeCommit, w.agentName]),
      story.plan.goal,
      ...story.plan.tasks.map((x) => x.title),
      `"${story.check}"`,
      `>${story.check}<`,
    ];
    // Engines' names too, except in the Dashboard scene, which moves onto
    // the story in the next slice.
    const engines = Object.values(story.engines).map((e) => `"${e.label}"`);
    for (const f of sources) {
      // A diff's rows are the sample files' own lines, checked against git's diff of them.
      const source = readFileSync(f, "utf8").replace(/export const \w+_DIFF\b[\s\S]*?\n\];/g, "");
      for (const name of [...copies, ...(f.endsWith("DashboardScene.tsx") ? [] : engines)]) expect(source, `${name} in ${f}`).not.toContain(name);
    }
  });

  it("F-WB-01 AC9 D-3000 an agent goes by its first name, and its pane by the first words of its task, as the app titles them", () => {
    // promptTitle (features/code/titles.ts): the first eight words, then an ellipsis.
    const titles = readFileSync(join(import.meta.dirname, "../../desktop/src/features/code/titles.ts"), "utf8");
    expect(titles).toContain("const head = words.slice(0, 8).join(\" \");");
    expect(titles).toContain("return words.length > 8 ? `${head}…` : head;");
    expect(promptTitle("one two three four five six seven eight")).toBe("one two three four five six seven eight");
    expect(promptTitle("  one two three four five six seven eight nine ")).toBe("one two three four five six seven eight…");
    const w = renderToStaticMarkup(<WorkspaceScene />);
    for (const role of ["finished", "working", "waiting"] as const) {
      const { name, title } = SAMPLE.worktrees[role];
      expect(title).toBe(promptTitle(story.worktrees.find((x) => x.agentName === name)?.task ?? ""));
      // PaneChrome: the first name, then the title.
      expect(w, role).toContain(`<span class="sc-pane-name">${name}</span><span class="sc-pane-title">${esc(title)}</span>`);
    }
    // The shell is "Shell · {folder}".
    expect(w).toContain(`<span class="sc-pane-title">${esc(t("pane.shellIn", { folder: SAMPLE.workspace }))}</span>`);
    for (const engine of Object.values(story.engines)) expect(w).not.toContain(`>${esc(t("pane.engine", { engine: engine.label, n: 1 }))}<`);
  });

  it("F-WB-01 AC9 D-3000 the hero's panes print every story line before what comes next in them, however many lines the story has", () => {
    const h = renderToStaticMarkup(<WorkspaceScene />);
    // Each pane, from its root (not its head) to the next one's.
    const panes = h.split(/<div class="sc-pane(?:"| )/).slice(1);
    const pane = (name: string) => panes.find((p) => p.includes(`<span class="sc-pane-name">${name}</span>`)) ?? "";
    const times = (p: string, re: RegExp) => [...p.matchAll(re)].map((m) => Number(m[1]));
    const lines = (p: string) => times(p, /<span class="sc-line" data-fx-line="" style="--at:([\d.]+)s">/g);
    const { finished, working, waiting } = SAMPLE.worktrees;
    // The finished one: all its lines, then done, when its header flashes.
    const c = pane(finished.name);
    expect(lines(c).length).toBe(SAMPLE.lines.finished.length);
    expect(times(c, /class="sc-flash" data-fx-visit="" style="--at:([\d.]+)s/g)).toEqual([HERO_DONE]);
    expect(Math.max(...lines(c))).toBeLessThan(HERO_DONE);
    // The working one: all its lines, then the caret waiting on its tests.
    const x = pane(working.name);
    expect(Math.max(...lines(x).slice(0, SAMPLE.lines.working.length))).toBeLessThan(times(x, /class="sc-line sc-caret" data-fx-line="" style="--at:([\d.]+)s/g)[0] ?? 0);
    // The waiting one: all its lines, then its ask to run the tests.
    const g = pane(waiting.name);
    expect(Math.max(...lines(g).slice(0, SAMPLE.lines.waiting.length))).toBeLessThan(times(g, /class="sc-ask" data-fx-visit="" style="--at:([\d.]+)s/g)[0] ?? 0);
    // The radar sees the shared function only once both panes have edited
    // its file: its rail marks and its notice come after both coupon.ts lines.
    const edited = (p: string) => times(p, new RegExp(`data-fx-line="" style="--at:([\\d.]+)s">  editing ${re(story.overlap.file)}`, "g"));
    const both = Math.max(...edited(c), ...edited(x));
    expect(edited(c).length * edited(x).length).toBe(1);
    if (LANDS) {
      const radar = [...times(h, /<span class="sc-radar" data-fx-pop="" style="--at:([\d.]+)s">/g), ...times(h, /<div class="sc-toast sc-toast-top" data-fx-slide="" style="--at:([\d.]+)s">/g)];
      expect(radar.length).toBe(3);
      for (const r of radar) expect(r).toBeGreaterThan(both);
    }
  });

  it("F-WB-01 AC9 D-1899 the Land scene lands each worktree with its own commits and merge commit, and runs the story's check", () => {
    const h = renderToStaticMarkup(<LandScene />);
    const rows = h.split('<span class="sc-lane-row"').slice(1);
    expect(rows.length).toBe(story.worktrees.length);
    story.worktrees.forEach((w, i) => {
      expect(rows[i], w.slug).toContain(`>${w.slug}<`);
      expect(rows[i], w.slug).toContain(`${t("landing.commits", { n: w.commits.length })} · ${t("landing.forecast.clean")}`);
      expect(rows[i], w.slug).toContain(esc(t("landing.step.landed", { commit: w.mergeCommit })));
    });
    expect(h).toContain(`<span class="sc-input">${esc(story.check)}</span>`);
  });

  it("F-WB-01 AC9 D-1899 the Review scene's diff is the finished worktree's real one, as git shows it", () => {
    // A run of git's own diff of the file, marks and both line numbers included (D-3000).
    const done = story.worktrees.find((w) => w.state === "done");
    expect(REVIEW_FILE).toBe(story.secondOpinion.comments[0]?.file);
    expect(runsIn(REVIEW_DIFF, gitDiff(done?.slug ?? "", REVIEW_FILE))).toBe(true);
    // Its header names that file.
    const h = renderToStaticMarkup(<ReviewScene />);
    expect(h).toContain(`<span class="sc-branch">${REVIEW_FILE}</span>`);
    // The second opinion's comments sit under the rows at their file and line, in its words.
    const { above, now } = reviewBody(h);
    const final = text(above + now);
    for (const c of story.secondOpinion.comments) {
      const row = REVIEW_DIFF.findIndex(([, , , newNo]) => newNo === c.line);
      expect(row, `line ${c.line}`).toBeGreaterThan(-1);
      expect(final).toContain(`${esc(REVIEW_DIFF[row]?.[1] ?? "")}${esc(c.body)}`);
      expect(h).toContain(`<span class="sc-diff-note-text">${esc(c.body)}</span>`);
    }
    // One agent in the worktree: the bar names it by its first name (WorktreeReview's sendTo, agentName).
    expect(h).toContain(`>${esc(t("worktree.review.sendTo", { name: SAMPLE.worktrees.finished.name }))}<`);
    expect(h).not.toContain(`>${t("worktree.review.send")}<`);
    // The card is titled by its first name, with the engine, workspace and branch under it (ReviewQueue).
    expect(h).toContain(`<span class="sc-card-title">${SAMPLE.worktrees.finished.name}</span>`);
    expect(h).toContain(`<span class="sc-card-meta">${esc([SAMPLE.worktrees.finished.engine, SAMPLE.workspace, SAMPLE.worktrees.finished.branch].join(" · "))}</span>`);
    expect(h).not.toContain(esc(t("review.diffTitle", { name: SAMPLE.worktrees.finished.engine })));
    // Only finished work waits for review: one card.
    expect(h.match(/class="sc-item[ "]/g)?.length).toBe(1);
    // A note starts where the code does: the gutter's width in the diff's
    // font, over the note's own font size (D-3000).
    const gutter = (/\.sc-diff-line \{[^}]*grid-template-columns: ([\d.]+)em ([\d.]+)em ([\d.]+)em /.exec(css) ?? []).slice(1, 4).reduce((a, b) => a + Number(b), 0);
    const note = /\.sc-diff-note \{[^}]*padding: [^;]* calc\(([\d.]+)em \/ ([\d.]+)\);[^}]*font-size: ([\d.]+)em;/.exec(css);
    expect(gutter).toBe(7);
    expect([Number(note?.[1]), Number(note?.[2])]).toEqual([gutter, Number(note?.[3])]);
  });

  it("F-WB-01 AC9 D-3000 a comment is drawn only under its own file's line, a line can have two, and none reads as the app's none", () => {
    const c = (file: string, line: number, text: string) => ({ file, line, text });
    expect(notesFor(REVIEW_FILE, REVIEW_DIFF, [c(REVIEW_FILE, 22, "a"), c(REVIEW_FILE, 22, "b")])).toEqual(new Map([[5, ["a", "b"]]]));
    // On another file, or a line the hunk doesn't show: the build fails rather than draw it wrong.
    expect(() => notesFor(REVIEW_FILE, REVIEW_DIFF, [c("src/auth/session.ts", 25, "a")])).toThrow(/src\/auth\/session\.ts/);
    expect(() => notesFor(REVIEW_FILE, REVIEW_DIFF, [c(REVIEW_FILE, 99, "a")])).toThrow(/99/);
    expect(notesFor(REVIEW_FILE, REVIEW_DIFF, [])).toEqual(new Map());
    // The done line counts the comments with the app's words, and has a "none" for zero.
    expect(t("worktree.opinion.none", { engine: SAMPLE.reviewer })).toBe(`${SAMPLE.reviewer} had no comments`);
  });

  it("F-WB-01 AC9 D-3000 Codex's comments arrive together, with its done line, as review:second_opinion adds them, and no gap waits for them", () => {
    const h = renderToStaticMarkup(<ReviewScene />);
    const doneAt = popBefore(h, t("worktree.opinion.done", { engine: SAMPLE.reviewer, n: SAMPLE.comments.length }));
    expect(doneAt).toBeDefined();
    // Above the first commented line nothing changes. Under it, the rows as
    // they were leave as the same rows with the comments under their lines
    // come in, both when Codex is done, stacked in one cell, so the lines
    // under a comment never wait apart from the line above it.
    const { above, was, now, wasAt, nowAt, wasFx, nowFx } = reviewBody(h);
    expect(above).not.toContain("sc-diff-note");
    expect(was).not.toContain("sc-diff-note");
    expect(now.startsWith('<span class="sc-diff-note"')).toBe(true);
    expect(now.match(NOTE)?.length).toBe(SAMPLE.comments.length);
    expect(now.replace(NOTE, "")).toBe(was);
    expect([wasAt, nowAt]).toEqual([doneAt, doneAt]);
    // The app inserts them in one go and the rows below just move down: the
    // two stacks swap at once, with no fade to draw the rows twice (the
    // design lead's must-fix), and only the comment rows fade in.
    expect([wasFx, nowFx]).toEqual(["cut", "show"]);
    expect(css).toMatch(/& \[data-fx-cut\]\s+\{ animation: sc-held var\(--at\) linear paused; \}/);
    expect(css).toMatch(/& \[data-fx-show\]\s+\{ animation: sc-unseen var\(--at\) linear paused; \}/);
    expect(css).toMatch(/\[data-fx-visit\], \[data-fx-out\], \[data-fx-cut\] \{ opacity: 0; \}/);
    for (const note of now.match(NOTE) ?? []) expect(note).toMatch(new RegExp(`^<span class="sc-diff-note" data-fx-in="" style="--at:${doneAt}s">`));
    expect(css).toMatch(/\.sc-diff-swap \{ display: grid; \}/);
    expect(css).toMatch(/\.sc-diff-swap > \* \{ grid-area: 1 \/ 1;/);
    // "Send to Ada" comes once they're in.
    expect(Number(popBefore(h, t("worktree.review.sendTo", { name: SAMPLE.worktrees.finished.name })))).toBeGreaterThan(Number(doneAt));
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
    for (const s of [t("titlebar.mode.code"), t("pane.shellIn", { folder: SAMPLE.workspace }), t("dashboard.allowOnce"), t("review.stat", { n: SAMPLE.diff.files, adds: 0, dels: 0 }).split(" · ")[0] ?? ""])
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
    expect(final).toContain(t("worktree.opinion.done", { engine: SAMPLE.reviewer, n: SAMPLE.comments.length }));
    expect(final).toContain(t("worktree.review.sendTo", { name: SAMPLE.worktrees.finished.name }));
    for (const c of SAMPLE.comments) expect(final).toContain(c.text);
    // "Codex is reviewing…" comes and goes.
    expect(final).toMatch(new RegExp(`data-fx-visit=""[^>]*>[\\s\\S]{0,1500}?${t("worktree.opinion.reviewing", { engine: SAMPLE.reviewer })}`));
    expect(css).toMatch(/\[data-fx-visit\], \[data-fx-out\], \[data-fx-cut\] \{ opacity: 0; \}/);
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

  it("F-WB-01 AC9 D-3000 a terminal line is printed at once and only slides into place, so no reading of the page finds it half drawn", () => {
    // Lighthouse's axe read lines mid-fade as low contrast (2.48:1 at 2.9 s).
    // A terminal prints a line whole: its opacity is full from the first
    // percent; only its slide takes the effect's time.
    expect(css).toMatch(/@keyframes sc-line\s+\{ from \{ opacity: 0; translate: -0\.25em 0; \} 1% \{ opacity: 1; \} \}/);
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
    for (const cls of ["sc-ask", "sc-toast", "sc-dialog"]) expect(new RegExp(`\\.${cls} \\{[^}]*var\\(--tn-elev-[23]\\)`).test(css), cls).toBe(true);
    // A line comment isn't raised: it's DiffView's annotation row under its line (D-3000).
    expect(/\.sc-diff-note \{[^}]*\}/.exec(css)?.[0]).not.toMatch(/elev|shadow/);
    for (const cls of ["sc-card", "sc-item"]) expect(new RegExp(`\\.${cls} \\{[^}]*var\\(--tn-elev-1\\)`).test(css), cls).toBe(true);
    // A risk is an outlined tag, a check's result is text, a warning sent is a status.
    expect(/\.sc-chip \{([^}]*)\}/.exec(css)?.[1]).not.toMatch(/status-waiting/);
    expect(css).not.toMatch(/\.sc-chip-ok \{[^}]*background/);
    expect(css).not.toContain(".sc-btn-done");
    const w = renderToStaticMarkup(<WorkspaceScene />);
    if (LANDS) expect(w).toMatch(new RegExp(`<span class="sc-status"[^>]*><span class="sc-dot">[\\s\\S]*?</span> ${t("radar.notice.warned")}</span>`));
  });

  it("F-WB-01 AC9 D-3000 every token a scene draws with is one the app defines, and a dialog's scrim is the app's modal scrim", () => {
    const styles = join(import.meta.dirname, "../../../packages/ui/src/styles");
    const defined = new Set(["tokens.css", "brand.css", "site.css"].flatMap((f) => [...readFileSync(join(styles, f), "utf8").matchAll(/(--tn-[\w-]+)\s*:/g)].map((m) => m[1])));
    const used = [...new Set([...css.matchAll(/var\((--tn-[\w-]+)/g)].map((m) => m[1]))];
    expect(used.length).toBeGreaterThan(10);
    expect(used.filter((v) => !defined.has(v))).toEqual([]);
    expect(/\.sc-plan-layer \{[^}]*background: var\(--tn-overlay\)/.test(css)).toBe(true);
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
