import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, readdirSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { renderToStaticMarkup } from "react-dom/server";
import ts from "typescript";
import { describe, expect, it } from "vitest";
import { DASHBOARD_CAPTION, DASH_AGENTS, DASH_T, DASH_TOOLS, DashboardScene, SINCE_START, cardState, dashboardSceneText } from "../components/site/scenes/DashboardScene";
import { LandScene } from "../components/site/scenes/LandScene";
import { ReviewScene, reviewFile, reviewSceneText } from "../components/site/scenes/ReviewScene";
import { Scene } from "../components/site/scenes/Scene";
import { HERO_AGENTS, HERO_ASK, HERO_DONE, HERO_T, WORKSPACE_CAPTION, WorkspaceScene, workspaceSceneText } from "../components/site/scenes/WorkspaceScene";
import { Changes, Glyph, pasted } from "../components/site/scenes/parts";
import { PLAN_AGENTS, PLAN_RUNS, PlanScene, planAt, planTimes } from "../components/site/scenes/PlanScene";
import { RELEASE_SCENES, releaseScenes } from "../components/site/scenes/releaseScenes";
import { RELEASES, addedCard } from "../lib/changelog";
import { SCENE_ROWS } from "../lib/home";
import { RELEASE } from "../lib/release";
import { DEFANGED, DEFANGS, LANDS, NAMES_TOOLS, SAMPLE, SCENE_BUILD, atLeast, failurePrompt, hookTool, promptTitle, radarNote, t, tabTitle, untrustedLabel } from "../lib/scenes";
import { minifySceneCss, sceneCss } from "../lib/sceneCss";
import HomePage from "./page.site";
import VersionPage from "./changelog/[version]/page.site";
import { SCENE_PAGES, THEME_BOOT, allowedScripts } from "../scripts/script-allowlist.mjs";
import story from "../../desktop/src/demo/story.json";
import { REVIEW_AGENTS, REVIEW_DIFF, REVIEW_FILE, REVIEW_T, notesFor } from "../components/site/scenes/ReviewScene";
import { CI_AGENTS, CI_T, ChecksScene, LOG_TAIL, SESSION_DIFF, ciSceneText, echoed, foldEchoed } from "../components/site/scenes/ChecksScene";
import names from "../../desktop/src/data/names.json";
import { parsePatch } from "@tandem/ui/components/DiffView";

// The animated product scenes (F-WB-01 AC9, D-1880, D-1881).

const home = renderToStaticMarkup(<HomePage />);
const v090 = renderToStaticMarkup(await VersionPage({ params: Promise.resolve({ version: "0.9.0" }) }));
const sheet = (f: string) => readFileSync(join(import.meta.dirname, "../components/site/scenes", f), "utf8").replace(/\/\*[\s\S]*?\*\//g, "");
/** The scenes' rules as a page with the CI scene has them: scenes.css, then
 * the CI scene's own (D-3001). */
const shared = sheet("scenes.css");
const css = `${shared}\n${sheet("ci.css")}`;
const ALL = { workspace: <WorkspaceScene />, dashboard: <DashboardScene />, review: <ReviewScene />, land: <LandScene />, plan: <PlanScene />, checks: <ChecksScene /> };
/** Text as React writes it into HTML. */
const esc = (s: string) => s.replace(/&/g, "&amp;").replace(/'/g, "&#x27;").replace(/"/g, "&quot;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
/** A string as a regular expression matches it. */
const re = (x: string) => x.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
type Row = readonly [string, string, number | null, number | null];
/** git's own diff of `file` from main to `place` in the demo's sample
 * repository, as DiffView parses it: its hunks, each in the scenes' row form.
 * It's what the app shows, since its diffs are git's. */
function gitDiff(place: string, file: string): Row[][] {
  const shop = join(import.meta.dirname, "../../desktop/src/demo/shop");
  // git's defaults, whatever the machine running the test has set: its
  // algorithm and context named, no external diff, no config file read,
  // no GIT_* variable passed on, and run from an empty folder that's no
  // repository's.
  const args = [
    "diff",
    "--no-index",
    "--no-color",
    "--no-ext-diff",
    "--diff-algorithm=myers",
    "--indent-heuristic",
    "-U3",
    join(shop, "main", `${file}.sample`),
    join(shop, place, `${file}.sample`),
  ];
  const env: NodeJS.ProcessEnv = { ...process.env };
  for (const k of Object.keys(env)) if (k.startsWith("GIT_")) delete env[k];
  Object.assign(env, { GIT_CONFIG_GLOBAL: "/dev/null", GIT_CONFIG_SYSTEM: "/dev/null" });
  const cwd = mkdtempSync(join(tmpdir(), "scenes-diff-"));
  let patch = "";
  try {
    execFileSync("git", args, { encoding: "utf8", cwd, env });
  } catch (e) {
    const err = e as { code?: string; status?: number; stdout?: string; stderr?: string };
    if (err.code === "ENOENT") throw new Error("scenes.test needs git: the scenes' diffs are checked against git's own diff of the sample files");
    // git diff exits 1 when the files differ; anything else is a failure.
    if (err.status !== 1) throw new Error(`git diff failed (${err.status}): ${err.stderr ?? ""}`);
    patch = err.stdout ?? "";
  } finally {
    rmSync(cwd, { recursive: true, force: true });
  }
  const hunks: Row[][] = [];
  for (const l of parsePatch(patch)) {
    if (l.kind === "hunk") hunks.push([]);
    else if (l.kind === "context" || l.kind === "add" || l.kind === "del") hunks.at(-1)?.push([l.kind === "add" ? "+" : l.kind === "del" ? "−" : " ", l.text, l.oldNo, l.newNo]);
  }
  return hunks;
}
/** True when `rows` run, in order and whole, inside one of `hunks`: a run
 * across two hunks would skip the lines between them. */
const runsIn = (rows: ReadonlyArray<readonly unknown[]>, hunks: ReadonlyArray<ReadonlyArray<readonly unknown[]>>) =>
  hunks.some((all) => all.some((_, i) => i + rows.length <= all.length && rows.every((r, k) => JSON.stringify(all[i + k]) === JSON.stringify(r))));
/** HTML's text, tags dropped (entities kept). */
const text = (h: string) => h.replace(/<[^>]+>/g, "");
const versionHtml = async (version: string) => renderToStaticMarkup(await VersionPage({ params: Promise.resolve({ version }) }));
/** When the enter that last starts before `s` in `h` plays: the time of the
 * element that brings `s` in. */
const enterBefore = (h: string, s: string) => [...h.slice(0, h.indexOf(esc(s))).matchAll(/data-fx-enter="" style="--at:([\d.]+)s"/g)].at(-1)?.[1];
/** The HTML of each element directly inside the one whose start tag is at `start` in `h`, found by balancing tags. */
function kids(h: string, start: number): string[] {
  const tags = /<(\/?)[a-z][^>]*?(\/?)>/g;
  tags.lastIndex = h.indexOf(">", start) + 1;
  const out: string[] = [];
  let depth = 0;
  let from = 0;
  for (let m = tags.exec(h); m; m = tags.exec(h)) {
    if (m[2]) continue;
    if (!m[1]) {
      if (depth++ === 0) from = m.index;
    } else if (depth-- === 0) break;
    else if (depth === 0) out.push(h.slice(from, m.index + m[0].length));
  }
  return out;
}
/** A value's steps as Changes draws them (D-3002), from the swap whose start tag is at `start`: when each comes in (0
 * for the first) and its HTML. */
const stepsAt = (h: string, start: number) =>
  kids(h, start).map((k) => [Number(/^<span data-fx-enter="" style="--at:([\d.]+)s">/.exec(k)?.[1] ?? 0), k] as const);
/** What a value's steps hold at `seconds`. */
const heldAt = <T,>(steps: ReadonlyArray<readonly [number, T]>, seconds: number) => [...steps].reverse().find(([at]) => at <= seconds)?.[1];
/** The app's own source files (apps/desktop/src), not its tests and not the browser demo, which isn't the app. */
const APP = join(import.meta.dirname, "../../desktop/src");
const appFiles = (dir = "."): string[] =>
  readdirSync(join(APP, dir), { withFileTypes: true }).flatMap((e) =>
    e.isDirectory() ? (e.name === "demo" ? [] : appFiles(join(dir, e.name))) : /\.tsx?$/.test(e.name) && !/\.(test|spec)\./.test(e.name) ? [join(dir, e.name)] : [],
  );
/** Each of the app's files, parsed. */
const appSources = () =>
  appFiles().map((f) => ts.createSourceFile(f, readFileSync(join(APP, f), "utf8"), ts.ScriptTarget.Latest, true, f.endsWith(".tsx") ? ts.ScriptKind.TSX : ts.ScriptKind.TS));
/** What `node` is in, outermost first: named functions and methods, a const holding a function or a call's result, and a
 * property holding a function, a menu entry's by its id. */
function where(node: ts.Node): string {
  const names: string[] = [];
  for (let n = node.parent as ts.Node | undefined; n; n = n.parent) {
    if ((ts.isFunctionDeclaration(n) || ts.isMethodDeclaration(n)) && n.name) names.unshift(n.name.getText());
    else if (ts.isVariableDeclaration(n) && n.initializer && (ts.isArrowFunction(n.initializer) || ts.isFunctionExpression(n.initializer) || ts.isCallExpression(n.initializer)))
      names.unshift(n.name.getText());
    else if (ts.isPropertyAssignment(n) && (ts.isArrowFunction(n.initializer) || ts.isFunctionExpression(n.initializer))) {
      const id = ts.isObjectLiteralExpression(n.parent) ? n.parent.properties.find((p) => ts.isPropertyAssignment(p) && p.name.getText() === "id") : undefined;
      names.unshift(`${id && ts.isPropertyAssignment(id) && ts.isStringLiteral(id.initializer) ? `${id.initializer.text}.` : ""}${n.name.getText()}`);
    }
  }
  return names.join(" › ");
}
/** Every call in the app of one of `names`, as "file: name in where", sorted. */
const callSites = (names: readonly string[]) =>
  appSources()
    .flatMap((sf) => {
      const found: string[] = [];
      const visit = (n: ts.Node) => {
        if (ts.isCallExpression(n) && ts.isIdentifier(n.expression) && names.includes(n.expression.text)) found.push(`${sf.fileName}: ${n.expression.text} in ${where(n)}`);
        ts.forEachChild(n, visit);
      };
      visit(sf);
      return found;
    })
    .sort();
/** Every object property in the app named `name`, written as `name: …` or `{ name }`, as "file: name in where", sorted. */
const propertySites = (name: string) =>
  appSources()
    .flatMap((sf) => {
      const found: string[] = [];
      const visit = (n: ts.Node) => {
        if ((ts.isPropertyAssignment(n) || ts.isShorthandPropertyAssignment(n)) && n.name.getText() === name) found.push(`${sf.fileName}: ${name} in ${where(n)}`);
        ts.forEachChild(n, visit);
      };
      visit(sf);
      return found;
    })
    .sort();
/** A value's step between its first and its last, as Changes draws it (D-3002): its cut, inside the cell that enters. */
const CUT = '(?:<span data-fx-cut="" style="--at:[\\d.]+s">)?';
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
    expect(h).toMatch(new RegExp(`<span data-fx-enter=""[^>]*><span class="sc-btn sc-btn-primary">${t("plan.land", { n: 2 })}</span></span>`));
    expect(h).toMatch(new RegExp(`<span data-fx-enter=""[^>]*><span data-fx-cut=""[^>]*><span class="sc-btn sc-btn-primary">${t("plan.land", { n: 1 })}</span></span></span>`));
    expect(h).toMatch(new RegExp(`<span data-fx-cut=""[^>]*><span class="sc-btn sc-btn-primary">${t("common.close")}</span></span>`));
    expect(h).toMatch(new RegExp(`<span data-fx-enter=""[^>]*><span class="sc-btn sc-btn-ghost">${t("common.close")}</span></span>`));
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
    expect(SAMPLE.edited).toEqual(edits.map((e) => e.edit));
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
    // Each name as a whole word, anywhere in the file: strings, JSX text, template strings and comments
    // alike ("Bea" isn't found in "Beat"). Engines' names too, in every scene (D-3001).
    const storyNames = [
      story.repo,
      story.overlap.symbol,
      ...story.worktrees.flatMap((w) => [w.slug, w.task, w.mergeCommit, w.agentName]),
      story.plan.goal,
      ...story.plan.tasks.map((x) => x.title),
      story.check,
      story.pr.title,
      ...story.pr.comments.flatMap((c) => [c.author, c.body]),
      ...story.secondOpinion.comments.map((c) => c.body),
      ...Object.values(story.engines).map((e) => e.label),
    ];
    const word = (name: string) => new RegExp(`(?<![\\w-])${re(name)}(?![\\w-])`);
    expect(word("Bea").test("Beat")).toBe(false);
    expect(word("Bea").test("{`Bea`}")).toBe(true);
    // A diff's rows are the sample files' own lines, checked against git's diff of them: exempt in its own file only.
    const diffs: Record<string, string> = { "ReviewScene.tsx": "REVIEW_DIFF", "ChecksScene.tsx": "SESSION_DIFF" };
    for (const f of sources) {
      const own = diffs[f.split("/").at(-1) ?? ""];
      const source = own ? readFileSync(f, "utf8").replace(new RegExp(`export const ${own}\\b[\\s\\S]*?\\n\\];`), "") : readFileSync(f, "utf8");
      if (own) expect(source, f).not.toContain(own + ": ");
      for (const name of storyNames) expect(word(name).test(source), `${name} in ${f}`).toBe(false);
    }
    expect(sources.length).toBeGreaterThan(8);
  });

  it("F-WB-01 AC9 D-3001 the shared sample world keeps what the scenes rely on: the sample's coupon code, and every agent's first name", () => {
    // Not a real brand's name (CLAUDE.md, Never): a merge that took another copy of the samples can't bring one back unseen.
    const shop = join(import.meta.dirname, "../../desktop/src/demo/shop");
    const samples = (dir: string): string[] =>
      readdirSync(dir, { withFileTypes: true }).flatMap((e) => (e.isDirectory() ? samples(join(dir, e.name)) : e.name.endsWith(".sample") ? [join(dir, e.name)] : []));
    const files = samples(shop);
    expect(files.length).toBeGreaterThan(20);
    // The 15% bulk code is BULK15 wherever it's defined, and in each of the four files that use it.
    const fifteen = files.flatMap((f) => [...readFileSync(f, "utf8").matchAll(/(\w+): \{ code: "([A-Z0-9]+)", percent: 15,/g)].map((m) => `${m[1]}=${m[2]}`));
    expect(fifteen.length).toBeGreaterThan(0);
    expect([...new Set(fifteen)]).toEqual(["BULK15=BULK15"]);
    for (const f of ["main/src/cart/coupon.ts", "main/src/cart/coupon.test.ts", "checkout-fix/src/cart/coupon.ts", "coupon-rules/src/cart/coupon.ts"])
      expect(readFileSync(join(shop, `${f}.sample`), "utf8"), f).toContain("BULK15");
    // Each worktree's agent has its first name, right after its engine, from the app's own list (F-CD-12).
    for (const w of story.worktrees) {
      expect(Object.keys(w).slice(0, 3), w.slug).toEqual(["slug", "engine", "agentName"]);
      expect(names, w.slug).toContain(w.agentName);
    }
    expect(new Set(story.worktrees.map((w) => w.agentName)).size).toBe(story.worktrees.length);
  });

  it("F-WB-01 AC9 D-3000 D-2702 an agent goes by its first name, its pane by the first words of its task and its tab by the first five, as the app titles them", () => {
    // promptTitle (features/code/titles.ts): the first eight words, then an ellipsis.
    const titles = readFileSync(join(import.meta.dirname, "../../desktop/src/features/code/titles.ts"), "utf8");
    expect(titles).toContain("const head = words.slice(0, 8).join(\" \");");
    expect(titles).toContain("return words.length > 8 ? `${head}…` : head;");
    expect(promptTitle("one two three four five six seven eight")).toBe("one two three four five six seven eight");
    expect(promptTitle("  one two three four five six seven eight nine ")).toBe("one two three four five six seven eight…");
    // tabLabel: a tab still on its default name takes its first agent's task, to five words.
    expect(titles).toContain("export function tabLabel(name: string, workspace: string, task: string | null = null, words = 5): string {");
    expect(titles).toContain('return parts.length > words ? `${parts.slice(0, words).join(" ")}…` : parts.join(" ");');
    expect(tabTitle("one two three four five")).toBe("one two three four five");
    expect(tabTitle(" one two three four five six ")).toBe("one two three four five…");
    for (const role of ["finished", "working", "waiting"] as const) {
      const { name, title, task } = SAMPLE.worktrees[role];
      expect(task).toBe(story.worktrees.find((x) => x.agentName === name)?.task);
      expect(title).toBe(promptTitle(task));
    }
    // The hero's one pane is the finished worktree's agent's (PaneChrome: the first name, then the title), in its tab.
    const w = renderToStaticMarkup(<WorkspaceScene />);
    const { finished } = SAMPLE.worktrees;
    expect(w.match(/<div class="sc-pane[ "]/g)).toHaveLength(1);
    expect(w).toContain(`<span class="sc-pane-name">${finished.name}</span><span class="sc-pane-title">${esc(finished.title)}</span>`);
    expect(w).toContain(`<span class="sc-tab">${esc(tabTitle(finished.task))}</span>`);
    for (const engine of Object.values(story.engines)) expect(w).not.toContain(`>${esc(t("pane.engine", { engine: engine.label, n: 1 }))}<`);
  });

  it("F-WB-01 AC9 D-3000 D-2702 the hero's pane prints every story line before what comes next in it, however many lines the story has", () => {
    const h = renderToStaticMarkup(<WorkspaceScene />);
    const pane = /<div class="sc-pane[ "][\s\S]*?<div class="sc-term">([\s\S]*?)<\/div><\/div>/.exec(h);
    const term = pane?.[1] ?? "";
    const times = (p: string, re: RegExp) => [...p.matchAll(re)].map((m) => Number(m[1]));
    const at = (line: string) => times(term, new RegExp(`data-fx-line="" style="--at:([\\d.]+)s">${re(esc(line))}</span>`, "g"));
    const { finished } = SAMPLE.worktrees;
    // Every line of the finished one's story, in order, each once, then its closing words when it's done and its header flashes.
    const printed = SAMPLE.lines.finished.map((l) => at(l));
    for (const [i, a] of printed.entries()) expect(a, SAMPLE.lines.finished[i]).toHaveLength(1);
    const when = printed.map((a) => a[0] ?? 0);
    expect([...when].sort((a, b) => a - b)).toEqual(when);
    expect(Math.max(...when)).toBeLessThan(HERO_DONE);
    expect(at(`  ${finished.closing}`)).toEqual([HERO_DONE]);
    expect(times(pane?.[0] ?? "", /class="sc-flash" data-fx-visit="" style="--at:([\d.]+)s/g)).toEqual([HERO_DONE]);
    expect(finished.closing).toBe(story.worktrees.find((w) => w.state === "done")?.closing);
    if (LANDS) {
      // The radar sees the shared function once the agent has edited its file: its rail marks and its notice come after
      // that line, and the note it types arrives in the pane after the press, before the tests run.
      const edited = at(SAMPLE.lines.finished.find((l) => l.startsWith(`  editing ${story.overlap.file}`)) ?? "");
      expect(edited).toHaveLength(1);
      const radar = [...times(h, /<span class="sc-wtrow-radar" data-fx-enter="" style="--at:([\d.]+)s">/g), ...times(h, /<div class="sc-note-toast sc-tone-warning" data-fx-visit="" style="--at:([\d.]+)s;/g)];
      expect(radar.length).toBeGreaterThanOrEqual(3);
      for (const r of radar) expect(r).toBeGreaterThan(edited[0] ?? 0);
      const note = pasted(SAMPLE.radar.note).filter((l) => l !== "");
      const typed = note.map((l) => times(term, new RegExp(`<span class="sc-line sc-line-wrap sc-hide-narrow" data-fx-line="" style="--at:([\\d.]+)s">${re(esc(l))}</span>`, "g")));
      for (const [i, a] of typed.entries()) expect(a, note[i]).toHaveLength(1);
      const run = at(SAMPLE.lines.finished.at(-2) ?? "")[0] ?? 0;
      const press = Number(/<span class="sc-btn sc-btn-sm" data-fx-press="" style="--at:([\d.]+)s">/.exec(h)?.[1]);
      for (const a of typed) {
        expect(a[0]).toBeGreaterThan(press);
        expect(a[0]).toBeLessThan(run);
      }
    }
  });

  it("F-WB-01 AC9 D-1899 the Land scene lands each worktree with its own commits and merge commit, and runs the story's check", () => {
    const h = renderToStaticMarkup(<LandScene />);
    const rows = h.split('<span class="sc-lane-row"').slice(1);
    expect(rows.length).toBe(story.worktrees.length);
    // The pull request's worktree has its agent's fix for the failed check too: the demo adds the fix's commit to its
    // own once it's made (backend.ts's fixCi), and the CI scene shows it made.
    const backend = readFileSync(join(import.meta.dirname, "../../desktop/src/demo/backend.ts"), "utf8");
    expect(backend).toContain("this.madeHere[w.slug] = [fix.commit, ...(this.madeHere[w.slug] ?? [])];");
    expect(backend).toContain("return [...(this.madeHere[slug] ?? []), ...(STORY.worktrees.find((w) => w.slug === slug)?.commits ?? [])];");
    story.worktrees.forEach((w, i) => {
      expect(rows[i], w.slug).toContain(`>${w.slug}<`);
      const n = w.commits.length + (w.slug === story.pr.worktree ? 1 : 0);
      expect(rows[i], w.slug).toContain(`${t("landing.commits", { n })} · ${t("landing.forecast.clean")}`);
      expect(rows[i], w.slug).toContain(esc(t("landing.step.landed", { commit: w.mergeCommit })));
    });
    expect(h).toContain(`<span class="sc-input">${esc(story.check)}</span>`);
  });

  it("F-WB-01 AC9 D-1899 the Review scene's diff is the finished worktree's real one, as git shows it", () => {
    // A run of git's own diff of the file, marks and both line numbers included (D-3000).
    const done = story.worktrees.find((w) => w.state === "done");
    expect(REVIEW_FILE).toBe(story.secondOpinion.comments[0]?.file);
    expect(runsIn(REVIEW_DIFF, gitDiff(done?.slug ?? "", REVIEW_FILE))).toBe(true);
    // Within one hunk: rows that end one hunk and start the next skip the lines between them.
    const [h1, h2] = [[[" ", "a", 1, 1]], [[" ", "z", 9, 9]]];
    expect(runsIn([...h1, ...h2], [h1, h2])).toBe(false);
    expect(runsIn(h2, [h1, h2])).toBe(true);
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
    // The card is titled by its first name (ReviewQueue).
    expect(h).toContain(`<span class="sc-rq-title">${SAMPLE.worktrees.finished.name}</span>`);
    // Its meta line is ReviewQueue's: the engine, the workspace, the branch, how long ago it finished ("4m", its ago()).
    const queue = readFileSync(join(import.meta.dirname, "../../desktop/src/features/dashboard/ReviewQueue.tsx"), "utf8");
    expect(queue).toContain('{[engine, item.workspace, item.branch, ago(item.finishedAt, now)].filter(Boolean).join(" · ")}');
    expect(queue).toContain('return m < 60 ? t("time.minutes", { n: m })');
    // A worktree's item has the worktree's own workspace, named for it, and its tandem/ branch (review_queue.rs, lanes.rs),
    // which the site shows as the name (D-1898): so the name once, not the repository's (the coordinator's must-fix on 39601718).
    const rq = readFileSync(join(import.meta.dirname, "../../desktop/src-tauri/src/services/review_queue.rs"), "utf8");
    expect(rq).toMatch(/branch: ws\s*\.lane_branch\s*\.clone\(\)/);
    expect(rq).toContain("workspace: ws.name,");
    const lanesRs = readFileSync(join(import.meta.dirname, "../../desktop/src-tauri/src/services/lanes.rs"), "utf8");
    expect(lanesRs).toContain("name: lane.slug.clone(),");
    const fin = SAMPLE.worktrees.finished;
    expect(h).toContain(`<span class="sc-rq-meta">${esc(`${fin.engine} · ${fin.branch} · ${t("time.minutes", { n: REVIEW_T.finished })}`)}</span>`);
    expect(h).not.toContain(`${esc(fin.engine)} · ${SAMPLE.workspace} ·`);
    // The hero's card is the same line, arriving as the work finishes: 0m.
    expect(renderToStaticMarkup(<WorkspaceScene />)).toContain(`<span class="sc-rq-meta">${esc(`${fin.engine} · ${fin.branch} · ${t("time.minutes", { n: 0 })}`)}</span>`);
    expect(h).not.toContain(esc(t("review.diffTitle", { name: SAMPLE.worktrees.finished.engine })));
    // Only finished work waits for review: one card.
    expect(h.match(/class="sc-rq-card"/g)).toHaveLength(1);
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
    // Drawn with none: no notes and nothing swaps, the app's "none" toast, no count in the bar, still a file in the header
    // (the first one the agent edited), and the text alternative doesn't count zero comments.
    const none = renderToStaticMarkup(<ReviewScene comments={[]} />);
    expect(none).not.toContain("sc-diff-note");
    expect(none).not.toContain("sc-diff-swap");
    expect(none).toContain(`<span class="sc-note-toast-title">${t("worktree.opinion.none", { engine: SAMPLE.reviewer })}</span>`);
    expect(none).not.toContain("sc-review-count");
    // WorktreeReview draws its bar while the reviewer works or there are comments: with none, it goes when the review is done.
    const wr = readFileSync(join(import.meta.dirname, "../../desktop/src/features/code/WorktreeReview.tsx"), "utf8");
    expect(wr).toContain("{(comments.length > 0 || reviewing) && (");
    expect(none).toContain(`<span class="sc-review-bar" data-fx-visit="" style="--at:${REVIEW_T.reviewing}s;--stay:${Math.round((REVIEW_T.done - REVIEW_T.reviewing) * 100) / 100}s">`);
    expect(renderToStaticMarkup(<ReviewScene />)).toContain(`<span class="sc-review-bar" data-fx-in="" style="--at:${REVIEW_T.reviewing}s">`);
    expect(reviewFile([], SAMPLE.edited)).toBe(SAMPLE.edited[0]);
    expect(reviewFile([], SAMPLE.edited)).not.toBe("");
    expect(reviewSceneText([])).toContain(`${SAMPLE.reviewer}, which reads the diff without editing anything and has no comments.`);
    expect(reviewSceneText([])).not.toMatch(/0 line comments/);
    // Two of the same words on one line are both drawn (their rows are keyed by place, not by text).
    const same = c(REVIEW_FILE, 22, "Same words");
    expect(renderToStaticMarkup(<ReviewScene comments={[same, same]} />).match(/<span class="sc-diff-note-text">Same words<\/span>/g)?.length).toBe(2);
  });

  it("F-WB-01 AC9 D-3000 Codex's comments arrive together, with its done toast, as review:second_opinion adds them, and no gap waits for them", () => {
    const h = renderToStaticMarkup(<ReviewScene />);
    const doneAt = String(REVIEW_T.done);
    expect(h).toMatch(new RegExp(`<div class="sc-note-toast sc-tone-success" data-fx-visit="" style="--at:${doneAt}s;[^"]*"><span class="sc-note-toast-text"><span class="sc-note-toast-title">${re(t("worktree.opinion.done", { engine: SAMPLE.reviewer, n: SAMPLE.comments.length }))}<`));
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
    // Each takes no time and waits for its --at as every effect does, filled both ways, so it's on the same clock as the
    // rest in real playback: run from the scene's start, it began a few frames early (the coordinator's SF-1, D-3001).
    expect(css).toMatch(/& \[data-fx-cut\]\s+\{ animation: sc-gone 0s linear var\(--at\) both paused; \}/);
    expect(css).toMatch(/& \[data-fx-show\]\s+\{ animation: sc-shown 0s linear var\(--at\) both paused; \}/);
    expect(css).toMatch(/@keyframes sc-gone\s+\{ from \{ opacity: 1; \} to \{ opacity: 0; \} \}/);
    expect(css).toMatch(/@keyframes sc-shown\s+\{ from \{ opacity: 0; \} to \{ opacity: 1; \} \}/);
    expect(css).toMatch(/\[data-fx-visit\], \[data-fx-out\], \[data-fx-cut\] \{ opacity: 0; \}/);
    for (const note of now.match(NOTE) ?? []) expect(note).toMatch(new RegExp(`^<span class="sc-diff-note" data-fx-in="" style="--at:${doneAt}s">`));
    expect(css).toMatch(/\.sc-diff-swap \{ display: grid;/);
    expect(css).toMatch(/\.sc-diff-swap > \* \{ grid-area: 1 \/ 1;/);
    // "Send to Ada" comes once they're in.
    expect(Number(enterBefore(h, t("worktree.review.sendTo", { name: SAMPLE.worktrees.finished.name })))).toBeGreaterThan(Number(doneAt));
  });

  it("F-WB-01 AC9 D-3001 D-2702 a review card's buttons are ReviewCard's, in its order: Approve the primary, Open diff secondary, the rest ghosts", () => {
    // ReviewCard's buttons as its source lists them: those a worktree's item has (Last turn needs a turn's checkpoint,
    // which the demo's item hasn't; Snooze while it isn't snoozed), each with its variant.
    const queue = readFileSync(join(import.meta.dirname, "../../desktop/src/features/dashboard/ReviewQueue.tsx"), "utf8");
    const card = queue.slice(queue.indexOf("function ReviewCard("), queue.indexOf("function Section("));
    const BUTTONS = [
      ["review-approve", "review.approve", "primary"],
      ["review-open-diff", "review.openDiff", "secondary"],
      ["review-send-back", "review.sendBack", "ghost"],
      ["review-opinion", "review.secondOpinion", "ghost"],
      ["review-merge", "review.merge", "ghost"],
      ["review-run-check", "review.runCheck", "ghost"],
      ["review-snooze", "review.snooze", "ghost"],
      ["review-jump", "review.jump", "ghost"],
      ["review-repo", "review.repo.menu", "ghost"],
    ] as const;
    const ids = [...card.matchAll(/data-testid="(review-[\w-]+)"/g)].map((m) => m[1]);
    expect(ids.filter((id) => BUTTONS.some(([b]) => b === id))).toEqual(BUTTONS.map(([id]) => id));
    expect(ids).toContain("review-turn");
    for (const [id, key, variant] of BUTTONS) {
      const at = card.indexOf(`data-testid="${id}"`);
      const button = card.slice(card.lastIndexOf("<Button", at), at);
      expect(button, id).toMatch(variant === "primary" ? /variant=\{approved \? "ghost" : "primary"\}/ : new RegExp(`variant="${variant}"`));
      expect(card.slice(at, card.indexOf("</Button>", at)), id).toContain(id === "review-approve" ? 't("review.approve")' : `t("${key}")`);
    }
    // Run check once the check isn't running.
    expect(card).toContain('{item.check && item.check.state !== "running" && (');
    const drawn = (h: string) =>
      [...h.matchAll(/<span class="sc-btn sc-btn-sm( sc-btn-primary| sc-btn-ghost)?"[^>]*>([^<]*)<\/span>/g)].map((m) => [m[2], m[1] === " sc-btn-primary" ? "primary" : m[1] === " sc-btn-ghost" ? "ghost" : "secondary"]);
    const all = BUTTONS.map(([, key, variant]) => [esc(t(key)), variant]);
    // The Review scene's card, its check passed: all of them, Second opinion pressed.
    const review = /<span class="sc-rq-card">[\s\S]*$/.exec(renderToStaticMarkup(<ReviewScene />))?.[0] ?? "";
    const actions = /<span class="sc-rq-actions">(?:<span[^>]*>[^<]*<\/span>)*<\/span>/.exec(review)?.[0] ?? "";
    expect(drawn(actions)).toEqual(all);
    expect(actions).toContain(`<span class="sc-btn sc-btn-sm sc-btn-ghost" data-fx-press="" style="--at:${REVIEW_T.ask}s">${t("review.secondOpinion")}</span>`);
    expect(actions.match(/data-fx-press/g)).toHaveLength(1);
    // The hero's, its check running then passed: without Run check, then with it, at once.
    const hero = renderToStaticMarkup(<WorkspaceScene />);
    const set = '(<span class="sc-rq-actions">(?:<span[^>]*>[^<]*</span>)*</span>)';
    const swap = new RegExp(`<span class="sc-rq-actions-swap"><span data-fx-cut="" style="--at:([\\d.]+)s">${set}</span><span data-fx-show="" style="--at:([\\d.]+)s">${set}</span></span>`).exec(hero);
    expect([Number(swap?.[1]), Number(swap?.[3])]).toEqual([HERO_T.passed, HERO_T.passed]);
    expect(drawn(swap?.[2] ?? "")).toEqual(all.filter(([label]) => label !== esc(t("review.runCheck"))));
    expect(drawn(swap?.[4] ?? "")).toEqual(all);
  });

  it("F-WB-01 AC9 D-3001 the second opinion shows where the app shows it: reviewing in the review bar, done as a toast with Show diff, then 2 comments · Copy review · Send", () => {
    const h = renderToStaticMarkup(<ReviewScene />);
    const reviewer = SAMPLE.reviewer;
    const n = SAMPLE.comments.length;
    // The diff's header is its file and nothing else.
    expect(h).toContain(`<div class="sc-diff-head"><span class="sc-branch">${REVIEW_FILE}</span></div>`);
    // WorktreeReview's bar, from the ask: a spinner, "Codex is reviewing…" and Open thread (ghost) until it's done…
    const bar = /<span class="sc-review-bar" data-fx-in="" style="--at:([\d.]+)s"><span class="sc-bar-states">([\s\S]*?)<\/span><\/span><\/div>/.exec(h);
    expect(Number(bar?.[1])).toBe(REVIEW_T.reviewing);
    expect(REVIEW_T.reviewing).toBeGreaterThan(REVIEW_T.ask);
    const [doing, done] = (bar?.[2] ?? "").split(/(?=<span class="sc-bar-state")/);
    expect(doing).toBe(
      `<span class="sc-bar-state" data-fx-visit="" style="--at:${REVIEW_T.reviewing}s;--stay:${Math.round((REVIEW_T.done - REVIEW_T.reviewing) * 100) / 100}s"><span class="sc-spin"></span><span class="sc-review-doing">${esc(t("worktree.opinion.reviewing", { engine: reviewer }))}</span><span class="sc-btn sc-btn-sm sc-btn-ghost">${t("worktree.opinion.openThread")}</span></span>`,
    );
    // …then the comments' count, Copy review (secondary) and Send to the agent (primary), as the comments arrive.
    expect(done).toBe(
      `<span class="sc-bar-state" data-fx-in="" style="--at:${REVIEW_T.done}s"><span class="sc-review-count">${t("worktree.review.count", { n })}</span><span class="sc-btn sc-btn-sm">${t("worktree.review.copy")}</span><span class="sc-btn sc-btn-sm sc-btn-primary" data-fx-enter="" style="--at:${REVIEW_T.send}s">${esc(t("worktree.review.sendTo", { name: SAMPLE.worktrees.finished.name }))}</span></span>`,
    );
    // The done toast (SecondOpinion.tsx's notify), with its Show diff action; Send waits for it to go, as it covers the bar.
    const toast = /<div class="sc-note-toast sc-tone-success" data-fx-visit="" style="--at:([\d.]+)s;--stay:([\d.]+)s"><span class="sc-note-toast-text"><span class="sc-note-toast-title">([^<]*)<\/span><span class="sc-note-toast-action"><span class="sc-btn sc-btn-sm">([^<]*)<\/span><\/span><\/span><svg[^>]*class="[^"]*sc-note-toast-x"[^>]*>[\s\S]*?<\/svg><\/div>/.exec(h);
    expect(toast?.slice(3)).toEqual([t("worktree.opinion.done", { engine: reviewer, n }), t("worktree.opinion.showDiff")]);
    expect(Number(toast?.[1])).toBe(REVIEW_T.done);
    expect(Number(toast?.[1]) + Number(toast?.[2])).toBeLessThan(REVIEW_T.send);
    const source = readFileSync(join(import.meta.dirname, "../../desktop/src/features/code/SecondOpinion.tsx"), "utf8");
    expect(source).toMatch(/title: t\("worktree\.opinion\.done", \{ engine, n \}\),\s*action: \{\s*label: t\("worktree\.opinion\.showDiff"\)/);
    // Not in the header any more, and no done line in the diff.
    expect(/<div class="sc-diff-head">[\s\S]*?<\/div>/.exec(h)?.[0]).not.toContain(reviewer);
  });

  it("F-WB-01 AC9 D-2702 the hero's rail is CodeRail's: the repository with its branch and no panes of its own, then each worktree's row", () => {
    const h = renderToStaticMarkup(<WorkspaceScene />);
    const rail = /<nav class="sc-crail">[\s\S]*?<\/nav>/.exec(h)?.[0] ?? "";
    expect(rail).toContain(`<span class="sc-crail-title">${t("rail.workspacesHeader")}</span>`);
    // The repository's row: its folder and name, and its branch, since none of its own panes is busy (BranchChip).
    const repo = /<span class="sc-wsrow">([\s\S]*?)<\/span><span class="sc-crail-group">/.exec(rail)?.[1] ?? "";
    expect(repo).toContain("lucide-folder");
    expect(repo).toContain(`<span class="sc-wsrow-name">${SAMPLE.workspace}</span>`);
    expect(text(repo).endsWith(SAMPLE.base)).toBe(true);
    // Its panes, none (every pane the demo opens is in a worktree), then its worktrees, in the story's order.
    const code = readFileSync(join(import.meta.dirname, "../../desktop/src/features/code/CodeRail.tsx"), "utf8");
    expect(code).toMatch(/<WorkspaceSessions ws=\{ws\} \/>\s*\{worktreesOf\(list, ws\.id\)\.map\(\(wt\) => \(\s*<WorktreeRow/);
    expect(code).toContain("if (s.waiting || s.working > 0 || s.failed) return null;");
    const backend = readFileSync(join(import.meta.dirname, "../../desktop/src/demo/backend.ts"), "utf8");
    expect(backend).toContain("...STORY.worktrees.map((w) => pane(agentPane(w), \"agent\", w.engine, w.slug, w.agentName)),");
    expect(backend).toContain('pane("pn_shell", "terminal", null, WORKING.slug),');
    expect(rail).toContain(`<span class="sc-crail-none">${t("rail.noSessions")}</span>`);
    const rows = rail.split(/<span class="sc-wtrow[ "]/).slice(1);
    expect(rows.map((r) => /<span class="sc-wtrow-name">([^<]*)<\/span>/.exec(r)?.[1])).toEqual(SAMPLE.order.map((w) => w.branch));
    // Remote hosts at its foot, and no "running sessions" line: in 0.12.0 that's only a count badge's name.
    expect(rail).toContain(`${t("rail.remoteHosts")}</span></span></nav>`);
    expect(code).toContain("{t(\"rail.remoteHosts\")}");
    expect(rail).not.toMatch(/running sessions?/);
  });

  it("F-WB-01 AC9 D-3001 D-2702 the hero follows the story's states: the rail's rows and the status bar follow each agent, the waiting one's Needs you", () => {
    const h = renderToStaticMarkup(<WorkspaceScene />);
    const { finished, working, waiting } = SAMPLE.worktrees;
    const rail = /<nav class="sc-crail">[\s\S]*?<\/nav>/.exec(h)?.[0] ?? "";
    const row = (branch: string) => rail.split(/(?=<span class="sc-wtrow[ "])/).find((r) => r.includes(`<span class="sc-wtrow-name">${branch}</span>`)) ?? "";
    // A row's trailing parts from each time on: [when it shows, when it's cut, what it holds].
    const steps = (r: string) =>
      [...r.matchAll(/<span class="sc-wtrow-step"(?: data-fx-show="" style="--at:([\d.]+)s")?><span class="sc-wtrow-parts"(?: data-fx-cut="" style="--at:([\d.]+)s")?>([\s\S]*?)<\/span><\/span>(?=<span class="sc-wtrow-step"|<\/span><span class="sc-wtrow-menu")/g)].map((m) => [
        Number(m[1] ?? 0),
        m[2] === undefined ? Infinity : Number(m[2]),
        text(m[3] ?? ""),
      ]);
    const working1 = t("rail.working", { n: 1 });
    const mark = t("radar.mark.one", { symbol: SAMPLE.symbol });
    // Each shows until the next one: one row's parts at a time.
    for (const w of SAMPLE.order) {
      const all = steps(row(w.branch));
      for (const [i, [, cut]] of all.entries()) expect(cut, w.branch).toBe(all[i + 1]?.[0] ?? Infinity);
    }
    const [m1 = 0, m2 = 0] = SAMPLE.radar.between.map((b) => Number(new RegExp(`<span class="sc-wtrow-name">${re(b)}</span>[\\s\\S]*?<span class="sc-wtrow-radar" data-fx-enter="" style="--at:([\\d.]+)s">`).exec(rail)?.[1]));
    expect(SAMPLE.radar.between).toEqual([finished.branch, working.branch]);
    // The finished one: at work, marked by the radar, then done, so only its mark and its pane.
    expect(steps(row(finished.branch)).map(([at, , parts]) => [at, parts])).toEqual(
      LANDS
        ? [[0, `${working1}1`], [m1, `${working1}${mark}1`], [HERO_DONE, `${mark}1`]]
        : [[0, `${working1}1`], [HERO_DONE, "1"]],
    );
    // The working one stays at work, beside its shell: two panes.
    expect(steps(row(working.branch)).map(([at, , parts]) => [at, parts])).toEqual(LANDS ? [[0, `${working1}2`], [m2, `${working1}${mark}2`]] : [[0, `${working1}2`]]);
    // The waiting one turns to Needs you, in waiting's quiet badge, when its engine asks, and stays so.
    expect(steps(row(waiting.branch)).map(([at, , parts]) => [at, parts])).toEqual([[0, `${working1}1`], [HERO_ASK, `${t("rail.waiting")}1`]]);
    expect(row(waiting.branch)).toContain(`<span class="sc-badge-quiet sc-badge-waiting">${t("rail.waiting")}</span>`);
    // The worktree in view is the selected row.
    expect(rail.match(/<span class="sc-wtrow sc-wtrow-on">/g)).toHaveLength(1);
    expect(row(finished.branch).startsWith('<span class="sc-wtrow sc-wtrow-on">')).toBe(true);
    // The status bar's agents: waiting first, then working, as each changes (AgentsSegment).
    const n = SAMPLE.order.length;
    expect(HERO_AGENTS).toEqual([
      [0, { waiting: 0, working: n }],
      [HERO_ASK, { waiting: 1, working: n - 1 }],
      [HERO_DONE, { waiting: 1, working: n - 2 }],
    ]);
    const bar = /<div class="sc-statusbar">[\s\S]*?<\/div>/.exec(h)?.[0] ?? "";
    const counts = (k: string) => ({ waiting: Number(/(\d+) waiting/.exec(text(k))?.[1] ?? 0), working: Number(/(\d+) working/.exec(text(k))?.[1] ?? 0) });
    const segment = stepsAt(bar, bar.indexOf('<span class="sc-swap">')).map(([when, k]) => [when, counts(k)] as const);
    expect(segment).toEqual(HERO_AGENTS);
    expect(bar).toContain(`<span class="sc-agents"><span class="sc-agents-working"><span class="sc-dot"><svg`);
    expect(bar).toContain(`<span class="sc-agents"><span class="sc-agents-waiting"><span class="sc-dot"><svg`);
    // At every change the segment holds the rail's count: the rows whose slot says Needs you, and those at work.
    for (const [when, held] of HERO_AGENTS) {
      const now = SAMPLE.order.map((w) => heldAt(steps(row(w.branch)).map(([at, , parts]) => [Number(at), String(parts)] as const), when) ?? "");
      expect({ waiting: now.filter((p) => p.startsWith(t("rail.waiting"))).length, working: now.filter((p) => p.startsWith(working1)).length }, `${when}s`).toEqual(held);
    }
    // Three agents, as the caption says.
    expect(WORKSPACE_CAPTION.split(" ")[0]).toBe(["None", "One", "Two", "Three", "Four", "Five"][n]);
  });

  it("F-WB-01 AC9 D-3002 the waiting agent asks in its engine's own words, which the app reads as waiting; found so, its card has no line", () => {
    // 0.12.0 raises a pane's approval only for `vibezeo run`; an engine's own question is its prompt in its terminal (the
    // design lead's must-fix on 54a05e0d). The app shows the agent waiting only when a hook says so or the line matches
    // its engine's waitingPatterns (the coordinator's M1 on 2966da74): tied here to the app, not the demo.
    const repo = (f: string) => readFileSync(join(import.meta.dirname, "../../..", f), "utf8");
    const asker = story.worktrees.find((w) => w.state === "waiting");
    expect(SAMPLE.ask).toBe(asker?.ask);
    expect(SAMPLE.question).toEqual({ terminal: `Allow execution of: '${SAMPLE.ask}'?` });
    type Engine = { id: string; hooksSupport: string; waitingPatterns: string[] };
    const engine = (JSON.parse(repo("crates/tandem-engines/registry.json")) as Engine[]).find((e) => e.id === asker?.engine);
    const matches = (line: string) => (engine?.waitingPatterns ?? []).some((p) => new RegExp(p).test(line));
    expect(matches(SAMPLE.question.terminal)).toBe(true);
    // The line the scenes had, the demo's, matches none of them: the app would show the agent working, then done.
    expect(matches(`Allow running ${SAMPLE.ask}? (y/n)`)).toBe(false);
    // A pane is watched with its engine's patterns, and the engine has no hooks, so the waiting comes from them. What
    // the heuristics report carries no tool and no preview; the Dashboard's line is the preview or the tool, and AgentCard
    // draws a line only for one.
    expect(engine?.hooksSupport).toBe("none");
    expect(repo("apps/desktop/src-tauri/src/services/terminal.rs")).toMatch(/\.watch\(\s*pane\.as_str\(\),\s*&engine\.waiting_patterns,/);
    expect(repo("apps/desktop/src-tauri/src/services/status.rs")).toContain("heuristics: Heuristics::new(waiting_patterns),");
    const heuristics = repo("crates/tandem-core/src/status_heuristics.rs");
    expect(heuristics.match(/StatusUpdate \{/g)).toHaveLength(1);
    expect(heuristics).toMatch(/StatusUpdate \{\s*status,\s*reason,\s*ts: now,\s*tool: None,\s*title: None,\s*preview: None,\s*\}/);
    expect(repo("apps/desktop/src-tauri/src/services/dashboard.rs").match(/preview: redact\(rec\.preview\.or\(rec\.tool\)\),/g)).toHaveLength(2);
    expect(repo("apps/desktop/src/features/dashboard/Dashboard.tsx")).toMatch(/showPreview &&\s*item\.preview && \(/);
    // In the scenes: the Dashboard's card for the agent has no line at all (the hero shows another worktree's pane:
    // D-2702, below).
    const dash = renderToStaticMarkup(<DashboardScene />);
    const card = /<span class="sc-card-wrap sc-card-ask">([\s\S]*?)<span class="sc-ring"/.exec(dash)?.[1] ?? "";
    expect(card).toContain(`<span class="sc-card-title">${SAMPLE.worktrees.waiting.name}</span>`);
    expect(card).not.toContain("sc-card-line");
    expect(dashboardSceneText()).not.toContain("question");
    // The texts say so too: no answer in place, no Allow once and Deny.
    for (const s of [workspaceSceneText(), dashboardSceneText(), WORKSPACE_CAPTION, DASHBOARD_CAPTION]) {
      expect(s).not.toMatch(/answer in place|Allow once|Deny/);
    }
    expect(workspaceSceneText()).toContain(`asks in its terminal to run ${SAMPLE.ask} and waits for you`);
    // In the hero, whose pane is another worktree's, the rail says so: Needs you, and nothing answers it.
    const hero = renderToStaticMarkup(<WorkspaceScene />);
    expect(hero).not.toContain(esc(SAMPLE.question.terminal));
    for (const s of [t("dashboard.allowOnce"), t("dashboard.deny")]) expect(hero, s).not.toContain(s);
  });

  it("F-WB-01 AC9 D-3001 D-3002 the Dashboard is the story's: one repository, its worktrees' agents by first name, and the waiting one on you", () => {
    const h = renderToStaticMarkup(<DashboardScene />);
    const n = SAMPLE.order.length;
    // The repository's group, then each worktree's, in the story's order, by branch.
    expect(h).toContain(`<span class="sc-group-head">${esc(`${SAMPLE.workspace} · ${n}`)}</span><span class="sc-group-lanes">`);
    const lanes = [...h.matchAll(/<span class="sc-group"><span class="sc-group-head">([^<]*)<\/span>/g)].map((m) => m[1]);
    expect(lanes).toEqual(SAMPLE.order.map((w) => esc(`${w.branch} · 1`)));
    // Each card: the first name, then cardMeta: the engine and the worktree's own workspace, whose tandem/ branch the site shows as
    // the name, so once (D-1898). The story gives no account.
    const model = readFileSync(join(import.meta.dirname, "../../desktop/src/features/dashboard/model.ts"), "utf8");
    expect(model).toContain("return [item.engineLabel ?? item.engineId, item.workspace, item.branch, item.accountLabel]");
    for (const w of SAMPLE.order)
      expect(h, w.branch).toMatch(new RegExp(`<span class="sc-card-title">${w.name}</span><span class="sc-card-state">[\\s\\S]*?</span></span><span class="sc-card-meta">${re(esc(`${w.engine} · ${w.branch}`))}</span>`));
    expect(story.worktrees.some((w) => "account" in w)).toBe(false);
    // One card per agent; the finished one's twice, at work and then done, one at a time (D-3003, below).
    expect(h.match(/<span class="sc-card">/g)?.length).toBe(n + 1);
    for (const engine of Object.values(story.engines)) expect(h).not.toContain(`>${esc(t("pane.engine", { engine: engine.label, n: 1 }))}<`);
    // The waiting one's engine asks in its own terminal, so its card has the waiting border, no line (found by a pattern,
    // its waiting has no preview: D-3002) and no buttons: AgentCard draws Allow and Deny only for a pending approval, and
    // there's none (D-3001).
    const { waiting, finished, working } = SAMPLE.worktrees;
    const ask = /<span class="sc-card-wrap sc-card-ask">([\s\S]*?)<span class="sc-ring"/.exec(h)?.[1] ?? "";
    expect(ask).toContain(`<span class="sc-card-title">${waiting.name}</span>`);
    expect(ask).not.toContain("sc-card-line");
    expect(ask).not.toContain("sc-btn");
    expect(h).toContain(`<span class="sc-ring" data-fx-pop="" style="--at:${DASH_T.ask}s"></span>`);
    for (const s of [t("dashboard.allowOnce"), t("dashboard.deny"), t("dashboard.wants", { summary: "" }).trim()]) expect(h, s).not.toContain(s);
    const source = readFileSync(join(import.meta.dirname, "../../desktop/src/features/dashboard/Dashboard.tsx"), "utf8");
    expect(source).toMatch(/\{approval \? \(/);
    expect(h.indexOf(`>${finished.name}<`)).toBeGreaterThan(-1);
    expect(h.indexOf(`>${working.name}<`)).toBeGreaterThan(-1);
    // The counts end where the story is: one each waiting, working and done, and one to review.
    const ends = [...h.matchAll(/<span data-fx-enter="" style="--at:[\d.]+s">([^<]*)<\/span><\/span><\/span>/g)].map((m) => m[1]);
    for (const s of [t("review.viewReview", { n: 1 }), t("dashboard.chip.waiting", { n: 1 }), t("dashboard.chip.working", { n: n - 2 }), t("dashboard.chip.done", { n: 1 })]) expect(ends, s).toContain(s);
    expect(h).toContain(`>${t("dashboard.chip.all", { n })}<`);
    expect(h).toContain(`>${esc(t("dashboard.count", { n }))}<`);
    // Nothing that isn't in the story: no other projects.
    for (const s of ["api", "docs", "rate-limits", "release-notes", "db:migrate", "Cursor Agent", "OpenCode"]) expect(text(h), s).not.toContain(s);
  });

  it("F-WB-01 AC9 D-3002 each Dashboard card ends its title row on its state, as AgentCard does, and the status bar counts the cards", () => {
    // AgentCard, in 0.12.0 and the tree: its dot, its engine's tile, its title, then STATUS_LABEL · elapsed in muted text,
    // elapsed being the minutes since the state began. The demo starts every pane 42 minutes back, and a new state starts
    // the clock again (backend.ts): a card that keeps its state reads 42m, one that changes it 0m.
    const app = (f: string) => readFileSync(join(import.meta.dirname, "../../desktop/src", f), "utf8");
    const dashboard = app("features/dashboard/Dashboard.tsx");
    expect(dashboard).toMatch(
      /<StatusDot status=\{view\(item\.status\)\} size=\{8\} \/>[\s\S]*?<EngineGlyph[\s\S]*?\{title\}\s*<\/span>\s*<span className="shrink-0 text-sm tabular-nums text-fg-muted">\s*\{STATUS_LABEL\[item\.status\]\(\)\} · \{elapsed\(item\.since, now\)\}/,
    );
    for (const [status, key] of [["waiting", "agentStatus.waiting"], ["working", "agentStatus.working"], ["done", "agentStatus.done"]]) expect(dashboard).toContain(`  ${status}: () => t("${key}"),`);
    expect(dashboard).toContain('return m < 60 ? t("time.minutes", { n: m })');
    const backend = app("demo/backend.ts");
    expect(backend).toContain(`const START = Date.now() - ${SINCE_START} * 60_000;`);
    expect(backend).toMatch(/this\.status\[paneId\] = \{ \.\.\.s, status, preview \};\s*this\.since\[paneId\] = Date\.now\(\);/);
    expect([cardState("working", SINCE_START), cardState("waiting", 0), cardState("done", 0)]).toEqual(["Working · 42m", "Waiting for you · 0m", "Done · 0m"]);
    // In the scene: each card's title row is its dot, its tile and its first name, then its state; the one that asks and
    // the one that finishes change theirs then, and the other keeps its own.
    const h = renderToStaticMarkup(<DashboardScene />);
    const { waiting, finished, working } = SAMPLE.worktrees;
    // A card's state from each time on: its title row's end, a value that changes, or, for a card the app draws again (the
    // finished one's, at Done: D-3003), each card's from when it shows.
    const stateOf = (card: string) => text(/<span class="sc-card-state">([\s\S]*?)<\/span><\/span><span class="sc-card-meta">/.exec(card)?.[1] ?? "");
    const states = (name: string): ReadonlyArray<readonly [number, string]> => {
      const title = h.indexOf(`<span class="sc-card-title">${name}</span><span class="sc-card-state">`);
      const swap = h.lastIndexOf('<span class="sc-card-swap">', title);
      const again = swap < 0 ? [] : kids(h, swap);
      if (again.length === 2 && again.every((k) => k.includes(`<span class="sc-card-title">${name}</span>`)))
        return again.map((k, i) => [i === 0 ? 0 : Number(/^<span data-fx-show="" style="--at:([\d.]+)s">/.exec(k)?.[1]), stateOf(k)] as const);
      const inner = h.indexOf(">", h.indexOf('<span class="sc-card-state">', title)) + 1;
      if (h.startsWith('<span class="sc-swap">', inner)) return stepsAt(h, inner).map(([at, k]) => [at, text(k)] as const);
      return [[0, /^([^<]*)<\/span>/.exec(h.slice(inner))?.[1] ?? ""]];
    };
    expect(states(waiting.name)).toEqual([
      [0, "Working · 42m"],
      [DASH_T.ask, "Waiting for you · 0m"],
    ]);
    expect(states(finished.name)).toEqual([
      [0, "Working · 42m"],
      [DASH_T.done, "Done · 0m"],
    ]);
    expect(states(working.name)).toEqual([[0, "Working · 42m"]]);
    for (const w of SAMPLE.order) expect(h, w.name).toMatch(new RegExp(`<span class="sc-card-head"><span class="sc-dot(?:-swap)?">[\\s\\S]*?<span class="sc-glyph"[^>]*>[^<]*</span><span class="sc-card-title">${w.name}</span>`));
    // The status bar's agents segment is the cards' count at every change, waiting and at work, and so are the filters.
    const counts = (k: string) => ({ waiting: Number(/(\d+) waiting/.exec(text(k))?.[1] ?? 0), working: Number(/(\d+) working/.exec(text(k))?.[1] ?? 0) });
    const segment = stepsAt(h, h.indexOf('<span class="sc-swap">', h.indexOf('<div class="sc-statusbar">'))).map(([at, k]) => [at, counts(k)] as const);
    expect(segment).toEqual(DASH_AGENTS);
    const swaps = [...h.matchAll(/<span class="sc-filter">(?=<span class="sc-swap">)/g)].map((m) => (m.index ?? 0) + m[0].length);
    const filter = (word: string) => {
      const swap = swaps.find((at) => text(kids(h, at)[0] ?? "").startsWith(word)) ?? -1;
      return stepsAt(h, swap).map(([at, k]) => [at, Number(/\d+/.exec(text(k))?.[0])] as const);
    };
    const cards = [states(waiting.name), states(finished.name), states(working.name)];
    for (const [at, n] of DASH_AGENTS) {
      const now = cards.map((c) => heldAt(c, at) ?? "");
      expect({ waiting: now.filter((x) => x.startsWith(t("agentStatus.waiting"))).length, working: now.filter((x) => x.startsWith(t("agentStatus.working"))).length }, `${at}s`).toEqual(n);
      expect([heldAt(filter("Waiting"), at), heldAt(filter("Working"), at)], `${at}s`).toEqual([n.waiting, n.working]);
    }
  });

  it("F-WB-01 AC9 D-3003 a Dashboard card has a line only where the app draws one: the tool an agent's hooks name while it works", () => {
    // dashboard.rs draws a pane card's line as its status's preview, else its tool. A pane's status gets no preview: hook
    // reports and pattern matches both leave it None. Only Claude Code's PreToolUse names a tool, as the agent works;
    // Codex's notify names none, and each report replaces the tool, so a done report clears it (the coordinator's and the
    // design lead's must-fix on 018b37a9).
    const repo = (f: string) => readFileSync(join(import.meta.dirname, "../../..", f), "utf8");
    expect(repo("apps/desktop/src-tauri/src/services/dashboard.rs").match(/preview: redact\(rec\.preview\.or\(rec\.tool\)\),/g)).toHaveLength(2);
    const report = repo("crates/tandem-core/src/status_report.rs");
    expect(report.match(/StatusUpdate \{/g)).toHaveLength(1);
    expect(report).toMatch(/tool: self\.payload\.tool\.clone\(\),\s*title: self\.payload\.title\.clone\(\),\s*preview: None,/);
    expect(repo("crates/tandem-core/src/status_heuristics.rs")).toMatch(/tool: None,\s*title: None,\s*preview: None,/);
    expect(repo("crates/tandem-core/src/status.rs").match(/tool: update\.tool,/g)).toHaveLength(2);
    const hook = repo("crates/tandem-engines/src/hook.rs");
    const claude = hook.slice(hook.indexOf("pub fn claude_report("), hook.indexOf("pub fn codex_report("));
    expect(claude.match(/payload\.tool = /g)).toHaveLength(1);
    expect(claude).toMatch(/"PreToolUse" => \{\s*payload\.tool = field\("tool_name"\)[\s\S]*?ReportEvent::Working\s*\}/);
    expect(hook.slice(hook.indexOf("pub fn codex_report("), hook.indexOf("fn short("))).not.toContain("tool");
    type Engine = { id: string; hooksSupport: string };
    const registry = JSON.parse(repo("crates/tandem-engines/registry.json")) as Engine[];
    expect(registry.filter((e) => e.hooksSupport === "claude-hooks").map((e) => e.id)).toEqual([...NAMES_TOOLS]);
    // In the scene: only an engine whose hooks name a tool gives its card a line, only while its agent works, and the line is
    // the tool for the step it's at: the finished one's edits, then its tests.
    const { finished, working, waiting } = SAMPLE.worktrees;
    expect([finished, working, waiting].map((w) => NAMES_TOOLS.has(w.engineId))).toEqual([true, false, false]);
    expect(finished.steps.slice(-2).map(hookTool)).toEqual(["Edit", "Bash"]);
    expect(DASH_TOOLS).toEqual([
      [0, "Edit"],
      [DASH_T.run, "Bash"],
    ]);
    const h = renderToStaticMarkup(<DashboardScene />);
    expect(h.match(/<span class="sc-card-line">/g)).toHaveLength(1);
    // The finished one's card at work, then done with no line, drawn again in one place at once.
    const [atWork = "", done = ""] = kids(h, h.indexOf('<span class="sc-card-swap">'));
    expect(atWork).toMatch(new RegExp(`^<span data-fx-cut="" style="--at:${DASH_T.done}s"><span class="sc-card">`));
    expect(done).toMatch(new RegExp(`^<span data-fx-show="" style="--at:${DASH_T.done}s"><span class="sc-card">`));
    expect(done).not.toContain("sc-card-line");
    const line = atWork.indexOf('<span class="sc-swap">', atWork.indexOf('<span class="sc-card-line">'));
    expect(stepsAt(atWork, line).map(([at, k]) => [at, text(k)])).toEqual(DASH_TOOLS);
    expect(sheet("scenes.css")).toContain(".sc-card-swap > * { grid-area: 1 / 1; align-self: start; }");
    // No step's output is on a card, and the text says the agent turns Done.
    for (const l of Object.values(SAMPLE.lines).flat().map((x) => x.trim()).filter((x) => x.startsWith("$") || x.startsWith("Tests"))) expect(text(h), l).not.toContain(l);
    expect(dashboardSceneText()).toContain(`${finished.name}, in ${finished.engine}, turns Done, and its card names the tool its hooks report until then: Edit, then Bash.`);
    expect(dashboardSceneText()).not.toMatch(/finishes its tests/);
  });

  it("F-WB-01 AC9 D-3003 the Review and Plan status bars count the threads at work: the second opinion while it reviews, a plan's runs and their reviews", () => {
    // A thread's status goes into the store a pane's does (state.rs's status sink: "Threads are Dashboard items too"), and
    // the status bar's AgentsSegment counts every record in it. The second opinion is a thread (second_opinion.rs), and so
    // are a plan's runs and their reviews (tasks_exec.rs's start_session, a run's, then its review's): the coordinator's
    // and the design lead's answer for D-2702. Each count is only what its frame draws.
    const repo = (f: string) => readFileSync(join(import.meta.dirname, "../../..", f), "utf8");
    const state = repo("apps/desktop/src-tauri/src/state.rs");
    expect(state).toContain("// Threads are Dashboard items too (F-DB-01, D-6).");
    expect(state).toMatch(/threads\.set_status_sink\(Arc::new\(move \|th\| \{[\s\S]*?ThreadState::Working => \(AgentStatus::Working, None\),[\s\S]*?status\.set_external\(/);
    expect(repo("apps/desktop/src-tauri/src/services/status.rs")).toMatch(/pub fn set_external\([\s\S]*?if let Some\(rec\) = self\.store\.apply\(&key, update\) \{\s*self\.queue\(rec\);/);
    expect(repo("apps/desktop/src/features/shell/StatusBar.tsx")).toMatch(
      /const all = Object\.values\(records\);\s*return \{\s*working: all\.filter\(\(r\) => r\.status === "working"\)\.length,\s*waiting: all\.filter\(\(r\) => r\.status === "waiting"\)\.length,/,
    );
    expect(repo("apps/desktop/src-tauri/src/services/second_opinion.rs")).toMatch(/\.create_thread\(\s*&a\.lane,\s*&a\.engine,\s*a\.account\.clone\(\),\s*format!\("Second opinion: \{branch\}"\),/);
    const exec = repo("apps/desktop/src-tauri/src/services/tasks_exec.rs");
    expect([...exec.matchAll(/\.start_session\(SessionStart \{[\s\S]*?review: (true|false),/g)].map((m) => m[1])).toEqual(["false", "true"]);
    const bar = (h: string) => /<div class="sc-statusbar">[\s\S]*?<\/div>/.exec(h)?.[0] ?? "";
    const counts = (k: string) => ({ waiting: Number(/(\d+) waiting/.exec(text(k))?.[1] ?? 0), working: Number(/(\d+) working/.exec(text(k))?.[1] ?? 0) });
    const segment = (h: string) => {
      const b = bar(h);
      return stepsAt(b, b.indexOf('<span class="sc-swap">')).map(([at, k]) => [at, counts(k)] as const);
    };
    // Review: one at work while "Codex is reviewing…" shows, and none before or after (the finished agent is done).
    const review = renderToStaticMarkup(<ReviewScene />);
    expect(segment(review)).toEqual(REVIEW_AGENTS);
    const doing = review.indexOf('<span class="sc-review-doing">');
    const [from = 0, stay = 0] = ([...review.slice(0, doing).matchAll(/data-fx-visit="" style="--at:([\d.]+)s;--stay:([\d.]+)s"/g)].at(-1) ?? []).slice(1).map(Number);
    expect(REVIEW_AGENTS.map(([at, n]) => [Math.round(at * 100) / 100, n.working])).toEqual([
      [0, 0],
      [from, 1],
      [Math.round((from + stay) * 100) / 100, 0],
    ]);
    // Plan: at every change, the runs the dialog shows Running or Reviewing, which its line counts as running.
    const plan = renderToStaticMarkup(<PlanScene />);
    expect(segment(plan)).toEqual(PLAN_AGENTS);
    const labels = [...plan.matchAll(/<span class="sc-run-label">/g)].map((m) => stepsAt(plan, plan.indexOf('<span class="sc-swap">', m.index)).map(([at, k]) => [at, text(k)] as const));
    expect(labels).toHaveLength(PLAN_RUNS.length);
    const busy = [t("tasks.runs.state.running"), t("tasks.runs.state.reviewing")];
    for (const at of planTimes()) {
      const working = labels.filter((l) => busy.some((b) => (heldAt(l, at) ?? "").endsWith(` · ${b}`))).length;
      expect(heldAt(PLAN_AGENTS, at)?.working, `${at}s`).toBe(working);
      if (working > 0) expect(planAt(at).summary, `${at}s`).toContain(t("plan.count.running", { n: working }));
    }
    expect(PLAN_AGENTS.map(([, n]) => n.working)).toEqual([0, 3, 2, 1, 0]);
    // Both draw nothing once no agent is at work, as the app's segment does.
    for (const h of [review, plan]) expect(text(bar(h).slice(bar(h).lastIndexOf('<span data-fx-enter=""')))).not.toMatch(/working|waiting/);
  });

  it("F-WB-01 AC9 the app's own words come from its catalog, with plurals, and a missing one fails the build", () => {
    expect(t("review.stat", { n: SAMPLE.diff.files, adds: 62, dels: 18 })).toBe("4 files · +62 −18");
    expect(t("review.stat", { n: 1, adds: 1, dels: 0 })).toBe("1 file · +1 −0");
    expect(t("dashboard.chip.waiting", { n: 1 })).toBe("Waiting 1");
    expect(t("dashboard.count", { n: 1 })).toBe("1 agent");
    // A value inside a plural's form (landing.done's {base}).
    expect(t("landing.done", { n: 3, base: "main" })).toBe("Landed 3 worktrees in main");
    expect(t("landing.done", { n: 1, base: "main" })).toBe("Landed 1 worktree in main");
    expect(() => t("no.such.key")).toThrow(/no "no.such.key"/);
    const w = renderToStaticMarkup(<WorkspaceScene />);
    for (const s of [t("titlebar.mode.code"), t("rail.noSessions"), t("code.arrange"), t("review.viewReview", { n: 1 }), t("review.stat", SAMPLE.diff && { n: SAMPLE.diff.files, adds: SAMPLE.diff.adds, dels: SAMPLE.diff.dels })])
      expect(w, s).toContain(esc(s));
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
    expect(final).toContain(`>${t("worktree.review.count", { n: SAMPLE.comments.length })}<`);
    expect(final).toContain(t("worktree.review.sendTo", { name: SAMPLE.worktrees.finished.name }));
    for (const c of SAMPLE.comments) expect(final).toContain(esc(c.text));
    // "Codex is reviewing…" and its done toast come and go.
    for (const s of [t("worktree.opinion.reviewing", { engine: SAMPLE.reviewer }), t("worktree.opinion.done", { engine: SAMPLE.reviewer, n: SAMPLE.comments.length })])
      expect(final, s).toMatch(new RegExp(`data-fx-visit=""[^>]*>(?:(?!data-fx-visit)[\\s\\S]){0,400}?${re(s)}`));
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
    // Not even a shadow's: a scrim is the app's --tn-overlay, a raised surface its elevation (D-3001).
    expect(css).not.toMatch(/rgba?\(/);
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
      for (const p of props) expect(["opacity", "translate", "scale", "rotate", "--sc-n"], `${m[1]}: ${p}`).toContain(p);
    }
    // A spinner turns (UI_SPEC §2.18) from when its state comes in, a set number of times, and only while its scene plays.
    expect(css).toMatch(/@keyframes sc-spin\s+\{ to \{ rotate: 1turn; \} \}/);
    expect(css).toMatch(/& \.sc-spin \{ animation: sc-spin 0\.8s linear var\(--at, 0s\) 2 paused; \}/);
    expect(css).toMatch(/&\[data-live\] :is\([^)]*\.sc-spin[^)]*\) \{/);
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
    for (const cls of ["sc-note-toast", "sc-dialog"]) expect(new RegExp(`\\.${cls} \\{[^}]*var\\(--tn-elev-[23]\\)`).test(css), cls).toBe(true);
    // A line comment isn't raised: it's DiffView's annotation row under its line (D-3000).
    expect(/\.sc-diff-note \{[^}]*\}/.exec(css)?.[0]).not.toMatch(/elev|shadow/);
    expect(/\.sc-card \{[^}]*var\(--tn-elev-1\)/.test(css)).toBe(true);
    // A review card is ReviewCard's: bordered on surface-2, no shadow.
    const queue = readFileSync(join(import.meta.dirname, "../../desktop/src/features/dashboard/ReviewQueue.tsx"), "utf8");
    expect(queue).toContain('className="flex flex-col gap-2 rounded-md border border-border bg-surface-2 p-3"');
    expect(home).toBeDefined();
    expect(sheet("home.css")).toMatch(/\.sc-rq-card \{ display: flex; flex-direction: column; gap: calc\(8 \* var\(--sc-px\)\); padding: calc\(12 \* var\(--sc-px\)\); border-radius: calc\(10 \* var\(--sc-px\)\); background: var\(--tn-surface-2\); box-shadow: inset 0 0 0 1px var\(--tn-border\); \}/);
    // A risk is an outlined Badge, a check's result is text (a details summary), never a pill.
    const card = /<span class="sc-rq-card">[\s\S]*?<span class="sc-rq-check">/.exec(renderToStaticMarkup(<ReviewScene />))?.[0] ?? "";
    expect(card).toContain(`<span class="sc-rq-risks"><span class="sc-badge sc-badge-outline">${t("review.rule.auth")}</span></span>`);
    expect(sheet("home.css")).not.toMatch(/\.sc-rq-check \{[^}]*(background|box-shadow|border-radius)/);
    expect(css).not.toContain(".sc-btn-done");
  });

  it("F-WB-01 AC9 D-3000 D-3001 every var() the site draws with is defined, in its stylesheets and inline in its pages, and a dialog's scrim is the app's modal scrim", () => {
    // One undefined var() voids its whole declaration: a scrim with no backdrop, a box-shadow with no ring (D-3001).
    const styles = join(import.meta.dirname, "../../../packages/ui/src/styles");
    const sheets = [
      ...["tokens.css", "brand.css", "base.css", "site.css"].map((f) => join(styles, f)),
      join(import.meta.dirname, "globals.css"),
      join(import.meta.dirname, "docs/docs.css"),
      ...["scenes.css", "ci.css", "home.css"].map((f) => join(import.meta.dirname, "../components/site/scenes", f)),
    ];
    const globals = readFileSync(join(import.meta.dirname, "globals.css"), "utf8");
    for (const f of ["tokens", "brand", "base", "site"]) expect(globals).toContain(`@import "@tandem/ui/${f}.css";`);
    const source = (f: string) => readFileSync(f, "utf8").replace(/\/\*[\s\S]*?\*\//g, "");
    // Defined: in a stylesheet, by next/font (app/fonts.ts), or inline by a scene's effects (parts.tsx's --at, --stay and --to).
    const defined = new Set(sheets.flatMap((f) => [...source(f).matchAll(/(--[\w-]+)\s*:/g)].map((m) => m[1])));
    for (const m of readFileSync(join(import.meta.dirname, "fonts.ts"), "utf8").matchAll(/variable: "(--[\w-]+)"/g)) defined.add(m[1]);
    // The docs' code blocks: Shiki writes each token's two colours inline, as these, since fumadocs turns its default colour off.
    for (const v of ["--shiki-light", "--shiki-dark"]) defined.add(v);
    const tsx = (dir: string): string[] =>
      readdirSync(dir, { withFileTypes: true }).flatMap((e) =>
        e.isDirectory() ? (["node_modules", "out", ".next"].includes(e.name) ? [] : tsx(join(dir, e.name))) : /\.tsx?$/.test(e.name) && !/\.(test|spec)\./.test(e.name) ? [join(dir, e.name)] : [],
      );
    const code = [...tsx(join(import.meta.dirname, "../components")), ...tsx(import.meta.dirname), ...tsx(join(import.meta.dirname, "../lib"))];
    for (const f of code) for (const m of readFileSync(f, "utf8").matchAll(/"(--[\w-]+)":/g)) defined.add(m[1]);
    expect([...defined].filter((v) => ["--at", "--stay", "--to", "--font-inter", "--font-jbm"].includes(v)).sort()).toEqual(["--at", "--font-inter", "--font-jbm", "--stay", "--to"]);
    const missing: string[] = [];
    for (const f of [...sheets, ...code]) {
      const used = new Set([...source(f).matchAll(/var\((--[\w-]+)/g)].map((m) => m[1]));
      for (const v of used) if (!defined.has(v)) missing.push(`${v} in ${f.split("/").slice(-2).join("/")}`);
    }
    expect(missing).toEqual([]);
    // The scenes draw with plenty of the app's tokens, and the inline ones (the mark's rings, the engines' tints) count too.
    expect([...css.matchAll(/var\((--tn-[\w-]+)/g)].length).toBeGreaterThan(10);
    expect(code.some((f) => f.endsWith("scenes/parts.tsx") && readFileSync(f, "utf8").includes('stroke="var(--tn-accent)"'))).toBe(true);
    for (const cls of ["sc-plan-layer", "sc-ci-layer"]) expect(new RegExp(`\\.${cls} \\{[^}]*background: var\\(--tn-overlay\\)`).test(css), cls).toBe(true);
  });

  it("F-WB-01 AC9 D-3001 the CI scene's own rules come only with it: not on the home page or another release's", async () => {
    // Its own classes: the ones no other scene file draws.
    const dir = join(import.meta.dirname, "../components/site/scenes");
    const classes = (f: string) => new Set([...readFileSync(join(dir, f), "utf8").matchAll(/\bsc-[a-z0-9-]*[a-z0-9]/g)].map((m) => m[0]));
    // The Badge part draws a class per variant it's given, so each of its variants is a shared class.
    const variants = (/export type BadgeVariant = ([^;]+);/.exec(readFileSync(join(dir, "parts.tsx"), "utf8"))?.[1] ?? "").match(/[a-z]+/g) ?? [];
    expect(variants).toEqual(["neutral", "waiting", "done", "error", "outline"]);
    const others = new Set([...readdirSync(dir).filter((f) => f.endsWith(".tsx") && f !== "ChecksScene.tsx").flatMap((f) => [...classes(f)]), ...variants.map((v) => `sc-badge-${v}`)]);
    const own = [...classes("ChecksScene.tsx")].filter((c) => !others.has(c));
    expect(own).toEqual(expect.arrayContaining(["sc-ci-layer", "sc-branch-chip", "sc-menu", "sc-checks"]));
    expect(own).not.toContain("sc-badge");
    const named = (sel: string) => own.some((c) => new RegExp(`\\.${c}(?![\\w-])`).test(sel));
    const selectors = (text: string) => [...text.matchAll(/([^{}]+)\{/g)].map((m) => (m[1] ?? "").trim()).filter((x) => !x.startsWith("@"));
    // scenes.css, which every page with a scene loads, has none of its rules; ci.css has only those, and no effects:
    // they stay gated in scenes.css.
    const ci = sheet("ci.css");
    expect(selectors(shared).filter(named)).toEqual([]);
    expect(selectors(ci).filter((sel) => !named(sel))).toEqual([]);
    expect(ci).not.toMatch(/animation|@keyframes/);
    // Only the CI scene names a sheet of its own; its page inlines it whole, in the head after scenes.css (the
    // precedence comes after Next's), and the home page and 0.12.0's don't have it.
    expect(Object.values(RELEASE_SCENES).flat().flatMap((s) => (s.css ? [[s.id, s.css]] : []))).toEqual([["checks", "ci.css"]]);
    // Inlined as the sheet reads: every selector, at-rule, declaration and block end, with its whitespace runs one space.
    // Dropping every space would hide `.a .b` turned into `.a.b` (the coordinator's nit 3 on 2966da74).
    const tokens = (text: string) =>
      text
        .split(/([{};])/)
        .map((x) => x.replace(/\s+/g, " ").trim())
        .filter((x) => x !== "" && x !== ";");
    expect(tokens(sceneCss("ci.css"))).toEqual(tokens(ci));
    expect(tokens(".sc-a .sc-b { margin: 0  1px; }")).not.toEqual(tokens(".sc-a.sc-b { margin: 0 1px; }"));
    expect(v090).toContain(`<style data-precedence="scenes" data-href="scenes/ci.css">${sceneCss("ci.css")}</style>`);
    const v120 = renderToStaticMarkup(await VersionPage({ params: Promise.resolve({ version: "0.12.0" }) }));
    for (const [page, h] of [["home", home], ["0.12.0", v120]] as const) expect(h, page).not.toContain("<style");
  });

  it("F-WB-01 AC9 D-3002 a scene's own sheet is read from beside its scenes, whatever the working directory, and only what closes up safely is inlined", () => {
    // Its comments go and each whitespace run is one space, with none around a brace or a semicolon. A quoted string or a
    // url() could hold whitespace that matters, and "</" would close the <style> it's inlined in, so each is refused
    // (the coordinator's nits 3 and 8 on 2966da74).
    expect(minifySceneCss("x.css", '/* a "quoted" comment */ .sc-a  .sc-b {\n  margin: 0  1px;\n}')).toBe(".sc-a .sc-b{margin: 0 1px;}");
    expect(() => minifySceneCss("x.css", '.sc-a::after { content: "a  b"; }')).toThrow(/quoted string or a url\(\)/);
    expect(() => minifySceneCss("x.css", ".sc-a::after { content: 'a'; }")).toThrow(/quoted string/);
    expect(() => minifySceneCss("x.css", ".sc-a { background: URL(a.png); }")).toThrow(/url\(\)/);
    expect(() => minifySceneCss("x.css", ".sc-a { color: red; } </style>")).toThrow(/"<\/"/);
    // From its module, not the working directory: the same sheet from anywhere.
    const here = process.cwd();
    const away = mkdtempSync(join(tmpdir(), "scene-css-"));
    try {
      process.chdir(away);
      expect(sceneCss("ci.css")).toBe(minifySceneCss("ci.css", readFileSync(join(import.meta.dirname, "../components/site/scenes/ci.css"), "utf8")));
    } finally {
      process.chdir(here);
      rmSync(away, { recursive: true, force: true });
    }
  });

  it("F-WB-01 AC9 D-3001 a dialog's scrim covers the whole window, title bar and status bar too, as the app's fixed overlay does", () => {
    const dialog = readFileSync(join(import.meta.dirname, "../../../packages/ui/src/components/Dialog.tsx"), "utf8");
    expect(dialog).toContain('const OVERLAY = "tn-fade fixed inset-0 z-[var(--z-dialog)] bg-overlay";');
    for (const [cls, el] of [["sc-plan-layer", <PlanScene key="plan" />], ["sc-ci-layer", <ChecksScene key="checks" />]] as const) {
      const h = renderToStaticMarkup(el);
      // The window's own child, after its status bar: not inside the page under the title bar.
      const bar = h.indexOf('<div class="sc-statusbar">');
      const layer = h.indexOf(`<div class="${cls}"`);
      expect(bar, cls).toBeGreaterThan(-1);
      expect(layer, cls).toBeGreaterThan(bar);
      expect(new RegExp(`\\.${cls} \\{ position: absolute; inset: 0; z-index: 3;`).test(css), cls).toBe(true);
    }
    expect(css).toMatch(/\.sc-window \{[^}]*position: relative;/);
  });

  it("F-WB-01 AC9 D-2702 the radar's notice and Warned both agents are the app's toasts, at the bottom right: its tone's bar, its body and action, and ×", () => {
    const app = (f: string) => readFileSync(join(import.meta.dirname, "../../..", f), "utf8");
    const toast = app("packages/ui/src/components/Toast.tsx");
    expect(toast).toContain('className="tn-toast relative flex w-[360px] gap-3 overflow-hidden rounded-md border border-border bg-surface-3 py-3 pl-4 pr-2 shadow-e2"');
    expect(toast).toContain('<span aria-hidden="true" className={cx("absolute inset-y-0 left-0 w-[3px]", BAR[tone])} />');
    expect(toast).toContain("fixed bottom-8 right-4");
    expect(toast).toMatch(/info: "bg-info",\s*success: "bg-success",\s*warning: "bg-warning",\s*error: "bg-danger",/);
    expect(toast).toContain("const remaining = useRef(item.duration ?? 5000);");
    expect(toast).toContain("onClick={() => { item.action?.onClick(); onDismiss(); }}");
    // The scene's, in the app's pixels: 360 wide, 16 in from the right, 32 over the window's bottom (10 over its status bar).
    expect(css).toMatch(/\.sc-note-toast \{ position: absolute; right: calc\(16 \* var\(--sc-px\)\); bottom: calc\(10 \* var\(--sc-px\)\); z-index: 4; display: flex; gap: calc\(12 \* var\(--sc-px\)\); width: calc\(360 \* var\(--sc-px\)\); padding: calc\(12 \* var\(--sc-px\)\) calc\(8 \* var\(--sc-px\)\) calc\(12 \* var\(--sc-px\)\) calc\(16 \* var\(--sc-px\)\);[^}]*border-radius: calc\(10 \* var\(--sc-px\)\); background: var\(--tn-surface-3\); box-shadow: var\(--tn-elev-2\), inset 0 0 0 1px var\(--tn-border\); \}/);
    expect(css).toMatch(/\.sc-note-toast::before \{[^}]*width: calc\(3 \* var\(--sc-px\)\); background: var\(--tn-info\); \}/);
    for (const [tone, token] of [["success", "--tn-success"], ["warning", "--tn-warning"], ["error", "--tn-danger"]]) expect(css).toContain(`.sc-tone-${tone}::before { background: var(${token}); }`);
    const statusbar = Number(/--statusbar-h:\s*(\d+)px/.exec(app("packages/ui/src/styles/tokens.css"))?.[1]);
    expect(32 - statusbar).toBe(10);
    // IntentRadar's: the notice a warning with its body and Warn both agents, then a success once both notes are typed.
    const radar = app("apps/desktop/src/features/code/IntentRadar.tsx");
    expect(radar).toMatch(/tone: "warning",\s*title: t\("radar\.notice\.title", [^\n]*\n\s*body: t\("radar\.notice\.body"\),/);
    expect(radar).toContain('else show({ tone: "success", title: t("radar.notice.warned") });');
    if (!LANDS) return;
    const { finished, working } = SAMPLE.worktrees;
    const hero = renderToStaticMarkup(<WorkspaceScene />);
    const notice = /<div class="sc-note-toast sc-tone-warning" data-fx-visit="" style="--at:([\d.]+)s;--stay:([\d.]+)s"><span class="sc-note-toast-text"><span class="sc-note-toast-title">([^<]*)<\/span><span class="sc-note-toast-body">([^<]*)<\/span><span class="sc-note-toast-action"><span class="sc-btn sc-btn-sm" data-fx-press="" style="--at:([\d.]+)s">([^<]*)<\/span><\/span><\/span><svg[^>]*sc-note-toast-x/.exec(hero);
    expect(notice?.slice(3, 5)).toEqual([esc(t("radar.notice.title", { a: finished.branch, b: working.branch, symbols: SAMPLE.symbol })), esc(t("radar.notice.body"))]);
    expect(notice?.[6]).toBe(t("radar.notice.warn"));
    const [shown, stay, press] = [notice?.[1], notice?.[2], notice?.[5]].map(Number) as [number, number, number];
    expect([shown, press]).toEqual([HERO_T.notice, HERO_T.warn]);
    // The press dismisses it: from the click that ends it (260 ms), it leaves over its fade (the site's --dur-pop), the
    // visit's last stretch.
    const pressMs = Number(/& \[data-fx-press\][^{]*\{ animation: sc-press (\d+)ms/.exec(css)?.[1]);
    const popMs = Number(/--dur-pop:\s*(\d+)ms/.exec(app("packages/ui/src/styles/site.css"))?.[1]);
    expect(css).toMatch(/& \[data-fx-visit\] \{ animation: sc-visit var\(--dur-pop\)[^,]*, sc-leave var\(--dur-pop\) var\(--ease-exit\) calc\(var\(--at\) \+ var\(--stay\) - var\(--dur-pop\)\)/);
    expect(Math.round((shown + stay) * 100)).toBe(Math.round((press + pressMs / 1000 + popMs / 1000) * 100));
    const warned = /<div class="sc-note-toast sc-tone-success" data-fx-visit="" style="--at:([\d.]+)s;--stay:([\d.]+)s"><span class="sc-note-toast-text"><span class="sc-note-toast-title">([^<]*)<\/span><\/span><svg[^>]*sc-note-toast-x/.exec(hero);
    expect(warned?.[3]).toBe(t("radar.notice.warned"));
    // Once the notice has gone, for the 5 s a toast stays: one toast at a time in one place.
    expect(Math.round(Number(warned?.[1]) * 100)).toBe(Math.round((shown + stay) * 100));
    expect(Number(warned?.[2])).toBe(5);
    // Both are the window's own, over the sidebar, never inside a pane.
    expect(/<div class="sc-pane[ "][\s\S]*?<\/div><\/div>/.exec(hero)?.[0]).not.toContain("sc-note-toast");
  });

  it("F-WB-01 AC9 a scene's status bar is the download's: the branch, then the version, and no connection (D-816, D-1888)", () => {
    for (const [id, el] of Object.entries(ALL)) {
      const h = renderToStaticMarkup(el);
      const bar = /<div class="sc-statusbar">[\s\S]*?<\/div>/.exec(h)?.[0] ?? "";
      expect(bar, id).toContain(t("status.version", { version: SCENE_BUILD }));
      // The branch with its icon; the CI scene's has its counts after it, and its CI segment (tested with that scene).
      expect(bar, id).toMatch(id === "checks" ? /<span class="sc-statusbar-branch"><svg[^>]*>[\s\S]*?<\/svg>[\w-]+<span class="sc-swap">/ : /<span class="sc-statusbar-branch"><svg[^>]*>[\s\S]*?<\/svg>[\w-]+<\/span>/);
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

describe("F-WB-01 AC9 D-2702 the hero is one Code-mode window, with one worktree in view", () => {
  const h = renderToStaticMarkup(<WorkspaceScene />);
  const { finished, working, waiting } = SAMPLE.worktrees;
  const app = (f: string) => readFileSync(join(import.meta.dirname, "../../desktop/src", f), "utf8");
  const homeCss = sheet("home.css");

  it("F-WB-01 AC9 D-2702 its title bar is TitleBar's: Agent, Code and Thread, and in Code mode the workspace's button", () => {
    const bar = app("features/shell/TitleBar.tsx");
    expect([...(/const MODES[\s\S]*?\n\];/.exec(bar)?.[0] ?? "").matchAll(/id: "(\w+)"/g)].map((m) => m[1])).toEqual(["agent", "code", "thread"]);
    expect(bar).toContain('{mode === "code" && !settingsOpen && active && (');
    const modes = ["agent", "code", "thread"].map((m) => t(`titlebar.mode.${m}`));
    for (const [id, el] of Object.entries(ALL)) {
      const titlebar = /<div class="sc-titlebar">[\s\S]*?<\/div>/.exec(renderToStaticMarkup(el))?.[0] ?? "";
      expect([...titlebar.matchAll(/<span class="sc-mode[^"]*">([^<]*)<\/span>/g)].map((m) => m[1]), id).toEqual(modes);
    }
    // The workspace in view is the finished worktree, named for it; the button is a small ghost one with its chevron.
    expect(h).toMatch(new RegExp(`<span class="sc-mode sc-mode-on">${t("titlebar.mode.code")}</span><span class="sc-mode">${t("titlebar.mode.thread")}</span></span><span class="sc-btn sc-btn-ghost sc-ws-button">${re(finished.branch)}<svg[^>]*lucide-chevron-down`));
    expect(renderToStaticMarkup(<PlanScene />)).not.toContain("sc-ws-button");
  });

  it("F-WB-01 AC9 D-2702 Code mode is on the finished worktree's tab, its agent's pane alone in it, its header PaneChrome's", () => {
    const tabs = /<div class="sc-tabs">([\s\S]*?)<\/div>/.exec(h)?.[1] ?? "";
    expect(tabs).toMatch(new RegExp(`^<span class="sc-tab">${re(esc(tabTitle(finished.task)))}</span><svg[^>]*lucide-plus[^>]*>[\\s\\S]*?</svg><span class="sc-btn sc-btn-sm sc-btn-ghost sc-tabs-arrange"><svg[^>]*lucide-layout-grid[\\s\\S]*?</svg>${t("code.arrange")}</span>$`));
    const head = /<div class="sc-pane-head">([\s\S]*?)<\/div>/.exec(h)?.[1] ?? "";
    // The status dot first, then the engine's glyph, the first name, the title and the worktree's chip; then its buttons.
    expect(head.startsWith('<span class="sc-pane-status">')).toBe(true);
    expect(head.indexOf("sc-glyph")).toBeGreaterThan(head.indexOf("sc-pane-status"));
    expect(head).toMatch(new RegExp(`<span class="sc-wt-chip"><svg[^>]*lucide-git-branch[\\s\\S]*?</svg>${re(finished.branch)}</span><span class="sc-pane-tools">`));
    // Alone in its tab: no Maximize, and no focus ring, which tells a tab's panes apart (PaneGrid); its menu and Close.
    const tools = /<span class="sc-pane-tools">([\s\S]*)<\/span>$/.exec(head)?.[1] ?? "";
    expect([...tools.matchAll(/lucide-([\w-]+)/g)].map((m) => m[1])).toEqual(["ellipsis", "x"]);
    expect(h).not.toContain("sc-pane-on");
    const chrome = app("features/code/PaneChrome.tsx");
    expect(chrome.indexOf('<StatusDot status={status} size={6} />')).toBeLessThan(chrome.indexOf("{glyph ?? <SquareTerminal size={13} />}"));
    expect(chrome).toContain("{!solo && (");
    expect(chrome).toMatch(/className="flex shrink-0 items-center opacity-50 /);
    expect(app("features/code/PaneGrid.tsx")).toContain('focused && !solo ? "border-border-strong" : "border-border",');
    expect(css).toMatch(/\.sc-pane-tools \{[^}]*opacity: 0\.5;/);
    expect(css).toMatch(/\.sc-pane-head \{[^}]*height: 2\.5em;[^}]*font-size: 0\.75em;/);
    // The worktree in view, as the status bar names it.
    expect(h).toMatch(new RegExp(`<span class="sc-statusbar-branch"><svg[^>]*>[\\s\\S]*?</svg>${re(finished.branch)}</span>`));
  });

  it("F-WB-01 AC9 D-2702 the sidebar's Dashboard is on Review: nothing to review until the agent finishes, then its card, its check running until it passes", () => {
    const side = /<aside class="sc-side">[\s\S]*?<\/aside>/.exec(h)?.[0] ?? "";
    // The Dashboard tab's pill, its waiting count from when the engine asks (Sidebar's Pill).
    const pills = /<span class="sc-pills"><span data-fx-cut="" style="--at:([\d.]+)s">([\s\S]*?)<\/span><span data-fx-show="" style="--at:([\d.]+)s">([\s\S]*?)<\/span><\/span><span class="sc-side-tools">/.exec(side);
    expect([Number(pills?.[1]), Number(pills?.[3])]).toEqual([HERO_ASK, HERO_ASK]);
    expect(text(pills?.[2] ?? "")).toBe(t("sidebar.dashboard"));
    expect(text(pills?.[4] ?? "")).toBe(`${t("sidebar.dashboard")}1`);
    expect(pills?.[4]).toContain('<span class="sc-badge sc-badge-waiting">1</span>');
    // DashboardPanel's switch, on Review: 0, then 1 when the agent's done.
    const seg = /<span class="sc-seg sc-seg-sm"><span class="sc-mode">([^<]*)<\/span><span class="sc-mode sc-mode-on">([\s\S]*?)<\/span><\/span><span class="sc-rq-views">/.exec(side);
    expect(seg?.[1]).toBe(t("review.viewAgents", { n: SAMPLE.order.length }));
    expect(seg?.[2]).toBe(`<span class="sc-swap"><span data-fx-cut="" style="--at:${HERO_DONE}s">${t("review.viewReview", { n: 0 })}</span><span data-fx-enter="" style="--at:${HERO_DONE}s">${t("review.viewReview", { n: 1 })}</span></span>`);
    // "Nothing to review" (ReviewQueue's EmptyState) until then; the queue, at once, from then.
    const empty = /<span class="sc-rq-empty" data-fx-cut="" style="--at:([\d.]+)s">([\s\S]*?)<\/span><span class="sc-rq" data-fx-show="" style="--at:([\d.]+)s">/.exec(side);
    expect([Number(empty?.[1]), Number(empty?.[3])]).toEqual([HERO_DONE, HERO_DONE]);
    expect(empty?.[2]).toContain("lucide-clipboard-check");
    expect(text(empty?.[2] ?? "")).toBe(`${t("review.emptyTitle")}${esc(t("review.emptyBody"))}`);
    // Its sort, by risk, and Refresh; To review · 1; the agent's card, 0m ago, with its closing words.
    const queue = side.slice(side.indexOf('<span class="sc-rq" '));
    expect(queue).toContain(`<span class="sc-rq-sort"><span class="sc-seg sc-seg-sm"><span class="sc-mode sc-mode-on">${t("review.byRisk")}</span><span class="sc-mode">${t("review.byTime")}</span></span><span class="sc-btn sc-btn-sm sc-btn-ghost">${t("review.refresh")}</span></span>`);
    expect(queue).toContain(`<span class="sc-rq-section">${t("review.toReview")} · 1</span>`);
    expect(queue).toContain(`<span class="sc-rq-closing">${esc(finished.closing)}</span>`);
    expect(queue).toContain(`<span class="sc-rq-stat">${t("review.stat", { n: SAMPLE.diff.files, adds: SAMPLE.diff.adds, dels: SAMPLE.diff.dels })}</span>`);
    // The repository's check runs on it as it arrives, then passes (the design lead's addendum).
    expect(queue).toContain(`<span class="sc-rq-check"><span class="sc-swap"><span data-fx-cut="" style="--at:${HERO_T.passed}s">${t("review.check.running")}</span><span data-fx-enter="" style="--at:${HERO_T.passed}s">${t("review.check.passed")}</span></span></span>`);
    expect(HERO_T.passed).toBeGreaterThan(HERO_DONE);
    const rq = app("features/dashboard/ReviewQueue.tsx");
    expect(rq).toMatch(/\{item\.check\.state === "running"\s*\? t\("review\.check\.running"\)\s*: item\.check\.state === "passed"\s*\? t\("review\.check\.passed"\)/);
    expect(rq).toMatch(/icon=\{<ClipboardCheck size=\{20\} \/>\}\s*title=\{t\("review\.emptyTitle"\)\}\s*body=\{t\("review\.emptyBody"\)\}/);
    // The finished work goes to the queue, not to a toast over the panes.
    expect(h).not.toMatch(/<div class="sc-note-toast[^>]*>(?:(?!<\/div>).)*sc-rq/s);
  });

  it("F-WB-01 AC9 D-2702 the note Warn both agents types into the agent is the core's: radar.rs's note_text, for the two that share the symbol", () => {
    // The core's own output: tests/radar_notes.rs checks each case against note_text (cargo test), and the port gives the
    // same note for each. The hero's is the first: no @ in it, so 0.12.0's fence and the tree's fence_defanged agree.
    const dir = join(import.meta.dirname, "../../desktop/src-tauri/tests");
    type Case = { why: string; other: string; symbols: Array<{ name: string; qualified: string; here: "added" | "changed" | "removed" | "used"; there: "added" | "changed" | "removed" | "used" }>; note: string };
    const cases = JSON.parse(readFileSync(join(dir, "fixtures/radar_notes.json"), "utf8")) as Case[];
    expect(readFileSync(join(dir, "radar_notes.rs"), "utf8")).toContain('assert_eq!(note, case.note, "{}", case.why);');
    for (const c of cases) expect(radarNote(c.other, c.symbols, true), c.why).toBe(c.note);
    expect(radarNote(working.branch, cases[0]?.symbols ?? [], false)).toBe(cases[0]?.note);
    expect(cases.length).toBeGreaterThanOrEqual(4);
    expect(cases[0]).toEqual({ why: expect.any(String), other: working.branch, symbols: [{ name: SAMPLE.symbol, qualified: SAMPLE.symbol, here: "changed", there: "changed" }], note: SAMPLE.radar.note });
    expect(SAMPLE.radar.note).not.toMatch(/[@\uFF20]/);
    // Between them: several symbols with each side's change, an @ defanged with the note, one that stays, a fence that
    // outlasts a backtick run.
    const all = cases.map((c) => c.note);
    expect(all.some((x) => x.includes("- Cart.total: changed here, renamed or removed there\n- Coupon: used here, added there"))).toBe(true);
    expect(all.some((x) => x.endsWith("```\n" + DEFANGED))).toBe(true);
    expect(all.some((x) => x.includes("- mail @ home: added here, used there"))).toBe(true);
    expect(all.some((x) => x.includes("````text\n"))).toBe(true);
    // Its end trimmed as the core's trim_end trims it, by Unicode's White_Space, before the defang removes what's
    // removable (D-3030): a branch name ending in a space, U+0085, a space and U+FEFF keeps both spaces, where JS's
    // trimEnd() would stop at the U+0085 and leave one.
    expect(cases.some((c) => c.symbols.length === 0 && c.other === "tandem/edge \u0085 \uFEFF" && c.note.endsWith("Other worktree: tandem/edge  \n```"))).toBe(true);
    expect(SAMPLE.radar.note.startsWith(`Heads-up from Vibezeo: another agent, working in a parallel worktree of this repository,`)).toBe(true);
    expect(SAMPLE.radar.note).toContain(`${untrustedLabel("the overlapping symbols")}\n\`\`\`text\nOther worktree: ${working.branch}\n- ${SAMPLE.symbol}: changed here, changed there\n\`\`\``);
    // The demo marks the same two and types the same note (backend.ts's radarNote).
    expect(app("demo/backend.ts")).toContain('return [{ name: o.symbol, qualified: o.symbol, here: "changed", there: "changed" }];');
    // Into the agent, the way its terminal echoes a paste, and only on a wide frame.
    expect(h).toContain(`<span class="sc-line sc-line-wrap sc-hide-narrow" data-fx-line="" style="--at:`);
  });

  it("F-WB-01 AC9 D-2702 the hero's own rules come only with the home page, after the shared ones", () => {
    const page = (f: string) => readFileSync(join(import.meta.dirname, f), "utf8");
    const homePage = page("page.site.tsx");
    expect(homePage.indexOf('import "../components/site/scenes/home.css";')).toBeGreaterThan(homePage.indexOf('import "../components/site/scenes/scenes.css";'));
    expect(page("changelog/[version]/page.site.tsx")).not.toContain("home.css");
    // Every class it styles is one a home-page scene draws, and none that only a release page's scenes draw.
    const dir = join(import.meta.dirname, "../components/site/scenes");
    const classes = (f: string) => new Set([...readFileSync(join(dir, f), "utf8").matchAll(/\bsc-[a-z0-9-]*[a-z0-9]/g)].map((m) => m[0]));
    const homeDraws = new Set(["WorkspaceScene.tsx", "ReviewScene.tsx", "DashboardScene.tsx", "parts.tsx"].flatMap((f) => [...classes(f)]));
    const styled = new Set([...homeCss.matchAll(/\.(sc-[a-z0-9-]*[a-z0-9])/g)].map((m) => m[1] ?? ""));
    expect([...styled].filter((c) => !homeDraws.has(c))).toEqual([]);
    expect(homeCss).not.toMatch(/animation|@keyframes/);
    for (const f of ["ChecksScene.tsx", "LandScene.tsx", "PlanScene.tsx"]) for (const c of ["sc-crail", "sc-side", "sc-code", "sc-tabs"]) expect(classes(f).has(c), `${c} in ${f}`).toBe(false);
  });

  it("F-WB-01 AC9 D-2702 on a phone the worktrees' rows are a strip over the pane, and the sidebar floats over the pane as the app's does below 1,100 px", () => {
    expect(app("stores/sidebar.ts")).toContain("export const FLOAT_BELOW = 1_100;");
    expect(app("features/shell/Sidebar.tsx")).toContain('? "absolute inset-y-0 right-0 border-l border-border shadow-e3"');
    const narrow = /@container \(max-width: 40rem\) \{([\s\S]*)\}\s*$/.exec(homeCss)?.[1] ?? "";
    expect(narrow).toContain(".sc-code { grid-template-columns: minmax(0, 1fr); grid-template-rows: auto minmax(0, 1fr); }");
    expect(narrow).toContain(".sc-crail-head, .sc-wsrow, .sc-crail-none, .sc-crail-foot { display: none; }");
    expect(narrow).toMatch(/\.sc-side \{ position: absolute; grid-area: 2 \/ 1; inset-block: 0; right: 0;[^}]*box-shadow: var\(--tn-elev-3\); \}/);
    // Docked from 1,100 px: the rail, the main area and the sidebar side by side, the rail wide enough that a worktree's
    // row holds its name, its slot, the radar's mark and its count (the e2e test measures it).
    expect(homeCss).toMatch(/\.sc-code \{ position: relative; display: grid; grid-template-columns: calc\(416 \* var\(--sc-px\)\) minmax\(0, 1fr\) calc\(400 \* var\(--sc-px\)\);/);
    // The radar's note folds on a phone, where the pane has no room for it.
    expect(h.match(/sc-line sc-line-wrap sc-hide-narrow/g)?.length).toBe(pasted(SAMPLE.radar.note).filter((l) => l !== "").length);
  });

  it("F-WB-01 AC9 D-2702 its caption and its text say what happens, by the agents' names", () => {
    expect(WORKSPACE_CAPTION).toBe("Three agents work in one repository, each in its own worktree, and the rail shows the one that needs you.");
    const text = workspaceSceneText();
    for (const s of [`Code mode on ${finished.branch}`, `${finished.name}'s pane alone in its tab`, `${waiting.name}, in ${waiting.engine}, asks in its terminal to run ${SAMPLE.ask} and waits for you`, `the rail shows ${waiting.branch} as Needs you`, `${working.name}, in ${working.engine}, keeps working in ${working.branch}`, "review queue", "check runs and passes"])
      expect(text, s).toContain(s);
    if (LANDS) expect(text).toContain(`marks ${finished.branch} and ${working.branch}, which both change ${SAMPLE.symbol}`);
    for (const s of [text, WORKSPACE_CAPTION]) expect(s).not.toMatch(/\b(she|her|hers|he|him|his)\b/i);
  });
});

describe("F-WB-01 AC9 D-3001 0.9.0's CI results and review comments back to the agent", () => {
  const h = renderToStaticMarkup(<ChecksScene />);
  const pr = story.pr;
  const failed = pr.checks.find((c) => c.bucket === "fail");
  const agent = SAMPLE.worktrees.finished;
  /** When the element whose text starts with `s` comes in: the nearest `--at` before it, not a cut's (a cut takes a
   * value away: Changes cuts a step between inside the cell that brings it in, D-3002). */
  const when = (s: string) => {
    const i = h.indexOf(`>${s}`);
    return i < 0 ? NaN : Number([...h.slice(0, i).matchAll(/(?<!data-fx-cut="" )style="--at:([\d.]+)s/g)].at(-1)?.[1] ?? NaN);
  };
  /** True when `s` is inside something that comes and goes (a visit), not something that stays. */
  const visiting = (s: string) => {
    const i = h.indexOf(esc(s));
    return i > -1 && h.lastIndexOf("data-fx-visit", i) > Math.max(h.lastIndexOf("data-fx-in", i), h.lastIndexOf("data-fx-pop", i));
  };

  it("F-WB-01 AC9 D-3001 it plays on 0.9.0's page, between Land worktrees and its own card", async () => {
    expect(RELEASE_SCENES["0.9.0"]?.map((s) => s.id)).toEqual(["land", "checks"]);
    expect(RELEASE_SCENES["0.9.0"]?.[1]?.card).toBe("CI results and review comments back to the agent");
    if (LANDS) expect(v090.indexOf('data-scene="checks"')).toBeGreaterThan(v090.indexOf("<strong>Land worktrees.</strong>"));
    // On the pull request's worktree, the finished one, with its agent by name.
    expect(pr.worktree).toBe(agent.branch);
    expect(h).toContain(`<span class="sc-pane-name">${agent.name}</span><span class="sc-pane-title">${esc(agent.title)}</span>`);
  });

  it("F-WB-01 AC9 D-3001 the failure reaches the agent as the core writes it: its instruction, the untrusted-data label and the fenced check", () => {
    const check = { name: failed?.name ?? "", workflow: SAMPLE.pr.workflow, link: SAMPLE.pr.link };
    const prompt = failurePrompt(pr.number, check, SAMPLE.pr.log);
    // The core's own output: tests/failure_prompts.rs checks each case against pr_checks.rs's failure_prompt (cargo
    // test), and the port, as the tree writes it, gives the same prompt for each. The scene's own is the first: the
    // story's failed check, workflow, link and log, word for word.
    const dir = join(import.meta.dirname, "../../desktop/src-tauri/tests");
    type Case = { why: string; pr: number; name: string; workflow?: string; link?: string; log: string[]; prompt: string };
    const cases = JSON.parse(readFileSync(join(dir, "fixtures/failure_prompts.json"), "utf8")) as Case[];
    expect(readFileSync(join(dir, "failure_prompts.rs"), "utf8")).toContain('assert_eq!(prompt, case.prompt, "{}", case.why);');
    for (const c of cases) expect(failurePrompt(c.pr, { name: c.name, workflow: c.workflow, link: c.link }, c.log, true), c.why).toBe(c.prompt);
    const listed = story.pr.checks.find((c) => c.bucket === "fail");
    expect([SAMPLE.pr.workflow, SAMPLE.pr.link, SAMPLE.pr.log]).toEqual([listed?.workflow, listed?.link, listed?.log]);
    expect(cases[0]).toEqual({ why: expect.any(String), pr: pr.number, ...check, log: SAMPLE.pr.log, prompt: failurePrompt(pr.number, check, SAMPLE.pr.log, true) });
    // Between them: the instruction and the label, a path defanged with the note, the @ runs that stay and those that
    // don't, a fence that outlasts a backtick run in the log, a check with no workflow or link, and a log of blank lines.
    const all = cases.map((c) => c.prompt);
    expect(prompt.startsWith(`A CI check failed on this worktree's pull request #${pr.number}. Find the cause, fix it, commit, and say what you changed.\n\n${untrustedLabel("the CI check")}\n\`\`\`text\nCheck: ${failed?.name}\n`)).toBe(true);
    expect(all.some((x) => x.includes("\uFF20~/.npm/_logs/x.log\n@@ -1,2 +1,3 \uFF20\uFF20\n```\n" + DEFANGED))).toBe(true);
    expect(all.some((x) => x.includes("mail me@ example\n\uFF20\uFF20both\ntrailing \uFF20\n```\n" + DEFANGED))).toBe(true);
    expect(all.some((x) => x.includes("`````text\nCheck: build\n\nFailed steps' log (last lines):\n```\n"))).toBe(true);
    expect(all.some((x) => x.endsWith("Workflow: CI\n\n(The failed steps printed nothing.)\n```"))).toBe(true);
    // The defang is the app's own TypeScript (D-3023 to D-3033), which the core's defang.json pins to the core: what's
    // removable goes first (U+0085 and U+FEFF among it, D-3030), and an @ run stays only before a space, a tab or a line
    // break (D-3025), so one before U+3000 is defanged too. Its note is the app's words.
    expect(readFileSync(join(import.meta.dirname, "../lib/scenes.ts"), "utf8")).toContain('import { DEFANGED, defangMarked } from "../../desktop/src/features/code/untrusted";');
    const vectors = JSON.parse(readFileSync(join(import.meta.dirname, "../../../crates/tandem-core/tests/fixtures/defang.json"), "utf8")) as { note: string };
    expect(DEFANGED).toBe(vectors.note);
    expect(DEFANGED).toBe("Vibezeo wrote \uFF20 for each at sign in the text above, so none of it attaches a file; read \uFF20 as an at sign.");
    expect(all.some((x) => x.includes("@ a \uFF20\u3000b \uFF20c \uFF20d \uFF20\uFF20\uFF20e\n```\n" + DEFANGED))).toBe(true);
    // The trim before the fence is the core's str::trim, Unicode's White_Space (the coordinator's S2 on 2966da74): a log
    // of U+0085 and U+3000 printed nothing, and one of U+FEFF was printed, then emptied by the defang. JS's trim() differs
    // from it on those two, so the port names the property.
    expect(cases.some((c) => c.log.join("\n") === "\u0085\n \u3000" && c.prompt.endsWith("\n\n(The failed steps printed nothing.)\n```"))).toBe(true);
    expect(cases.some((c) => c.log.join("\n") === "\uFEFF" && c.prompt.endsWith("Failed steps' log (last lines):\n\n```"))).toBe(true);
    expect(["\u0085".trim(), "\uFEFF".trim(), "\u0085".replace(/\p{White_Space}/u, ""), "\uFEFF".replace(/\p{White_Space}/u, "")]).toEqual(["\u0085", "", "", "\uFEFF"]);
    // The story's log has an @ (the package's version), which the tree writes as ＠ with its note after the fence. That's
    // D-3022, after 0.12.0, whose core fenced CI's text as it was: so the scene, on 0.12.0, types it so (D-128).
    expect(SAMPLE.pr.log.some((l) => l.includes("@"))).toBe(true);
    expect(DEFANGS).toBe(atLeast(SCENE_BUILD, "0.13.0"));
    expect(prompt).toBe(DEFANGS ? cases[0]?.prompt : (cases[0]?.prompt ?? "").replace(`\n${DEFANGED}`, "").replaceAll("\uFF20", "@"));
    if (!DEFANGS) expect(prompt).not.toContain(DEFANGED);
    // The agent's pane prints its start, a line at a time once it's sent, then, past what scrolled by, the log's last
    // lines and the fence's end.
    const lines = pasted(prompt);
    const head = lines.indexOf("  Failed steps' log (last lines):");
    expect(head).toBeGreaterThan(0);
    expect(echoed(prompt)).toEqual([...lines.slice(0, head + 1), "  ⋮", ...lines.slice(-(LOG_TAIL + 1))]);
    expect(lines.at(-1)).toBe("  ```");
    let at: number = CI_T.send;
    for (const shown of echoed(prompt).filter((l) => l !== "")) {
      const m = new RegExp(`data-fx-line="" style="--at:([\\d.]+)s">${re(esc(shown))}<`).exec(h);
      expect(m, shown).not.toBeNull();
      expect(Number(m?.[1]), shown).toBeGreaterThan(at);
      at = Number(m?.[1]);
    }
    // On a phone it keeps the prompt's start, the ⋮ and the log's last two lines with the fence's end; a log too short
    // to leave lines out keeps them all. Only blank lines and the link fold then.
    const kept = (typed: string[]) => typed.filter((l, i) => !foldEchoed(typed)(l, i));
    const typed = echoed(prompt);
    expect(kept(typed)).toEqual(typed.filter((l, i) => l !== "" && !l.startsWith("  Link: ") && (i <= typed.indexOf("  ⋮") || i >= typed.length - 3)));
    expect(kept(typed)[0]).toBe(`› ${prompt.split("\n")[0]}`);
    expect(kept(typed).slice(-4)).toEqual(["  ⋮", ...lines.slice(-3)]);
    const short = echoed(failurePrompt(pr.number, check, SAMPLE.pr.log.slice(-2)));
    expect(short).not.toContain("  ⋮");
    expect(kept(short)).toEqual(short.filter((l) => l !== "" && !l.startsWith("  Link: ")));
    expect(kept(short)).toContain(`  ${SAMPLE.pr.log.at(-1)}`);
    // The fix it then makes is the story's: the test file, +12 −8, the tests all passing, its commit, its words.
    const fix = story.pr.fix;
    expect(SAMPLE.pr.fix.lines).toEqual([
      `  reading ${SAMPLE.pr.fix.file}`,
      `  editing ${SAMPLE.pr.fix.file}  +12 −8`,
      `  $ ${story.check}`,
      "  Tests  54 passed",
      `  $ git commit -m "${fix.commit.subject}"`,
      `  ${fix.closing}`,
    ]);
    expect(fix.steps).toEqual([{ read: SAMPLE.pr.fix.file }, { edit: SAMPLE.pr.fix.file, add: 12, del: 8 }, { run: story.check, passed: 54 }]);
    expect(SAMPLE.pr.fix.commit).toEqual({ sha: "6b2e9f4", subject: "Test refreshSession and its renewal" });
    for (const l of SAMPLE.pr.fix.lines) expect(h).toMatch(new RegExp(`data-fx-line="" style="--at:[\\d.]+s">${re(esc(l))}<`));
  });

  it("F-WB-01 AC9 D-3001 Send failure to agent leaves the Checks dialog open: its toast names the agent, then Close closes it", () => {
    // SendToAgent types the prompt and toasts; it has no way to close the dialog it sits in.
    const send = readFileSync(join(import.meta.dirname, "../../desktop/src/features/code/SendToAgent.tsx"), "utf8");
    expect(send).toContain("export function SendToAgent({ laneId, prompt, testId }: { laneId: string; prompt: Prompt; testId: string })");
    expect(send).toContain('show({ tone: "success", title: t("failure.sent", { name: agentName(chosen) }) });');
    // In the scene: Send is pressed, the toast names the agent by its first name, then Close is pressed and the dialog goes.
    expect(h).toContain(`<span class="sc-btn sc-btn-sm" data-fx-press="" style="--at:${CI_T.send}s">${t("failure.send")}</span>`);
    expect(h).toMatch(new RegExp(`<div class="sc-note-toast sc-tone-success" data-fx-visit="" style="--at:${CI_T.sent}s;--stay:1.5s"><span class="sc-note-toast-text"><span class="sc-note-toast-title">${re(esc(t("failure.sent", { name: agent.name })))}</span></span><svg[^>]*sc-note-toast-x`));
    expect(h).toContain(`<span class="sc-btn sc-btn-primary" data-fx-press="" style="--at:${CI_T.close}s">${t("common.close")}</span>`);
    expect(CI_T.close).toBeGreaterThan(CI_T.sent);
    const layer = /<div class="sc-ci-layer" data-fx-visit="" style="--at:([\d.]+)s;--stay:([\d.]+)s">/.exec(h);
    const [rise, stay] = [Number(layer?.[1]), Number(layer?.[2])];
    expect(rise).toBeGreaterThan(CI_T.open);
    // It's there until Close's press is over, then leaves: the press never plays on a fading dialog.
    const press = Number(/& \[data-fx-press\] \{ animation: sc-press (\d+)ms/.exec(css)?.[1]) / 1000;
    const leave = Number(/--dur-pop:\s*(\d+)ms;/.exec(readFileSync(join(import.meta.dirname, "../../../packages/ui/src/styles/site.css"), "utf8"))?.[1]) / 1000;
    expect([press, leave]).toEqual([0.26, 0.18]);
    expect(rise + stay - leave).toBeCloseTo(CI_T.close + press, 5);
    // The agent works from when it's sent until its commit.
    expect(h).toMatch(new RegExp(`<span data-fx-enter="" style="--at:${CI_T.sent}s"><span data-fx-cut="" style="--at:${CI_T.done}s"><span class="sc-dot"><svg[^>]*data-status="working"`));
  });

  it("F-WB-01 AC9 D-3001 the fix is pushed from the branch chip before the checks run again, and they pass", () => {
    const ahead = (n: number) => esc(t("git.branch.short", { ahead: n, behind: 0 }));
    // In order: the commit leaves one to push, Push is pressed, pushed, the row's badge pressed again, running, passed.
    const order = [when(ahead(1)), CI_T.push, when(esc(t("git.pushed", { branch: pr.worktree }))), CI_T.reopen, when(esc(t("checks.running"))), when(esc(t("checks.passed")))];
    expect(order.every(Number.isFinite), String(order)).toBe(true);
    expect([...order].sort((a, b) => a - b)).toEqual(order);
    expect(h).toMatch(new RegExp(`data-fx-press="" style="--at:${CI_T.push}s">${re(esc(t("git.push")))}<`));
    // The menu has the chip's entries; the toast names the branch without tandem/ (D-1898).
    for (const s of [t("git.pull"), t("git.switch"), t("git.newBranch")]) expect(h).toContain(`>${esc(s)}<`);
    expect(h).not.toContain("tandem/");
  });

  it("F-WB-01 AC9 D-3001 a push reads no checks: opening them again does, Refresh spinning, and the new run is Running until a later read passes it", () => {
    // The app: the dialog reads as it opens, and its Refresh (secondary) shows its spinner while it does; finished checks
    // are read every 15 minutes and running ones every 2 with the window in front; and nothing else reads them, a push
    // included (the coordinator's must-fix on 54a05e0d). When a push reads them, this test fails and the scene changes.
    const src = join(import.meta.dirname, "../../desktop/src");
    const app = (f: string) => readFileSync(join(src, f), "utf8");
    const dialog = app("features/code/PullRequestChecks.tsx");
    expect(dialog).toContain("useEffect(() => refresh(), [refresh]);");
    expect(dialog).toContain('<Button variant="secondary" loading={loading} onClick={refresh} data-testid="checks-refresh">');
    const store = app("stores/worktrees.ts");
    for (const s of ["export const CHECKS_POLL_MS = 120_000;", "export const CHECKS_IDLE_MS = 15 * 60_000;"]) expect(store).toContain(s);
    // Who reads them, call by call, with what each is in: the store's three readers, the worktree menu's three entries,
    // the dialog's Refresh and its Allow, and the rail's polling, every two minutes while the window has focus and only
    // while the rail is open. The browser demo isn't the app: its harness (demo/main.tsx) reads when its simulated core
    // says they changed. When a new caller reads them, a push's included, this fails and the scene is checked again.
    expect(callSites(["loadChecks", "openChecks", "pollChecks", "allowChecks", "useChecksPolling"])).toEqual([
      "features/code/CodeRail.tsx: useChecksPolling in CodeRail",
      "features/code/PullRequestChecks.tsx: allowChecks in AllowChecksDialog › allow",
      "features/code/PullRequestChecks.tsx: loadChecks in ChecksDialog › refresh",
      "features/code/PullRequestChecks.tsx: pollChecks in useChecksPolling",
      "features/code/PullRequestChecks.tsx: pollChecks in useChecksPolling › tick",
      "features/code/Worktrees.tsx: allowChecks in singleRepoMenu › wt-checks-forget.onSelect",
      "features/code/Worktrees.tsx: loadChecks in singleRepoMenu › wt-checks-refresh.onSelect",
      "features/code/Worktrees.tsx: openChecks in singleRepoMenu › wt-checks.onSelect",
      "stores/worktrees.ts: loadChecks in allowChecks",
      "stores/worktrees.ts: loadChecks in openChecks",
      "stores/worktrees.ts: loadChecks in pollChecks",
    ]);
    expect(app("features/code/PullRequestChecks.tsx")).toContain("if (typeof document === \"undefined\" || document.hasFocus()) void pollChecks(ids);");
    expect(app("features/code/CodeRail.tsx")).toContain("useChecksPolling(localWorktrees, !collapsed);");
    // Only a read writes them: the store starts them empty, and loadChecks writes each worktree's as its read returns.
    expect(propertySites("checks")).toEqual(["stores/worktrees.ts: checks in loadChecks", "stores/worktrees.ts: checks in useWorktrees"]);
    expect(store).toContain("if (out.ok) useWorktrees.setState((s) => ({ checks: { ...s.checks, [laneId]: out.value } }));");
    expect(store).toMatch(/useWorktrees = create<WorktreesState>\(\(set, get\) => \(\{[\s\S]*?\n {2}checks: \{\},/);
    // The push is git's alone: it reads nothing of the checks.
    const git = app("stores/git.ts");
    expect(git).toContain('push: (context, publish = false) =>\n      act(context, "push", () => invoke("git_push", { context, publish }, { timeoutMs: NETWORK_TIMEOUT_MS })),');
    expect(git).not.toMatch(/checks/i);
    // In the scene: once pushed, the row's badge is pressed again, and the dialog rises again.
    const r = (n: number) => Math.round(n * 100) / 100;
    expect(CI_T.reopen).toBeGreaterThan(CI_T.pushed);
    expect(h).toContain(`<span data-fx-press="" style="--at:${CI_T.reopen}s"><span class="sc-checks-fail" data-fx-press="" style="--at:${CI_T.open}s">`);
    const layers = [...h.matchAll(/<div class="sc-ci-layer" data-fx-visit="" style="--at:([\d.]+)s;--stay:([\d.]+)s">/g)].map((m) => [Number(m[1]), Number(m[2])] as const);
    expect(layers.map(([rise]) => rise)).toEqual([r(CI_T.open + 0.2), r(CI_T.reopen + 0.2)]);
    const second = h.slice(h.lastIndexOf('<div class="sc-ci-layer"'));
    // Each dialog is two boxes in one place: as it reads, with Refresh's spinner from the rise, until the read returns;
    // then as the read leaves it, Refresh's label alone. Each is its own size, so the dialog shrinks as the app's does when
    // Send and the spinner go (the coordinator's nit 7 on 2966da74).
    expect(css).toMatch(/\.sc-ci-layer > \* \{ grid-area: 1 \/ 1; \}/);
    const boxes = (layer: string, read: number, rise: number) => {
      const [reading = "", done = ""] = kids(layer, 0);
      expect(reading.startsWith(`<div class="sc-dialog sc-ci-dialog" data-fx-cut="" style="--at:${read}s">`), reading.slice(0, 120)).toBe(true);
      expect(done.startsWith(`<div class="sc-dialog sc-ci-dialog" data-fx-show="" style="--at:${read}s">`), done.slice(0, 120)).toBe(true);
      expect(reading).toContain(`<span class="sc-btn"><span class="sc-spin" style="--at:${r(rise)}s"></span>${t("checks.refresh")}</span>`);
      expect(done).toContain(`<span class="sc-btn">${t("checks.refresh")}</span>`);
      expect(done).not.toContain("sc-spin");
      expect(kids(layer, 0)).toHaveLength(2);
      return [reading, done] as const;
    };
    // The rows: as they were, the failure with its Send failure to agent (not pressed now), until the read; then the new
    // run, all three Running in Badge's outline variant, and no Send.
    const [was, now] = boxes(second, CI_T.running, CI_T.reopen + 0.2);
    expect(was).toContain(`<span class="sc-badge sc-badge-error">${t("checks.bucket.fail")}</span>`);
    expect(was).toContain(`>${t("failure.send")}<`);
    expect(was).not.toContain("data-fx-press");
    expect(t("checks.bucket.pending")).toBe("Running");
    expect(now.match(/<span class="sc-badge sc-badge-outline">Running<\/span>/g)?.length).toBe(pr.checks.length);
    expect(now).not.toContain(t("failure.send"));
    // The row reads Checks running from the read; Close is pressed, and the dialog leaves once the press is over;
    // passed comes on a later read, once the dialog has gone.
    expect(when(esc(t("checks.running")))).toBe(CI_T.running);
    expect(now).toContain(`<span class="sc-btn sc-btn-primary" data-fx-press="" style="--at:${CI_T.closed}s">${t("common.close")}</span>`);
    const [rise2 = NaN, stay2 = NaN] = layers[1] ?? [];
    expect(rise2 + stay2).toBeCloseTo(CI_T.closed + 0.26 + 0.18, 5);
    expect(when(esc(t("checks.passed")))).toBe(CI_T.passed);
    expect(CI_T.passed).toBeGreaterThan(rise2 + stay2);
    // The first open reads too: Refresh spins until its read, and the rows are as the row said, Send pressed after it.
    const first = h.slice(h.indexOf('<div class="sc-ci-layer"'), h.lastIndexOf('<div class="sc-ci-layer"'));
    const [reading, read] = boxes(first, CI_T.read, CI_T.open + 0.2);
    expect(text(reading).replace(t("checks.refresh"), "")).toBe(text(read).replace(t("checks.refresh"), ""));
    expect(reading).not.toContain("data-fx-press");
    expect(read).toContain(`<span class="sc-btn sc-btn-sm" data-fx-press="" style="--at:${CI_T.send}s">${t("failure.send")}</span>`);
    expect(CI_T.read).toBeLessThan(CI_T.send);
    // The text says how they're read, and that Vibezeo asks once before it reads a repository's pull requests (UI-156).
    const said = ciSceneText();
    expect(said).toContain(
      "A push doesn't read the checks, so the row still shows the failure; opening the Checks dialog again reads them, and they're running. While they run, Vibezeo reads them every two minutes while its window has focus and its Code rail is open, and they pass.",
    );
    expect(said).not.toMatch(/on Refresh|reads the checks again, every|in front/);
    expect(said).toContain("Vibezeo asks once before it reads a repository's pull requests, with your own GitHub CLI.");
  });

  it("F-WB-01 AC9 D-3001 the status bar follows the worktree: its branch with what there is to push, then its CI, failed, running and passed", () => {
    // StatusBar.tsx: the branch, its ↑/↓ once it has an upstream, then ChecksSegment, F-WT-08's state in a word.
    const app = readFileSync(join(import.meta.dirname, "../../desktop/src/features/shell/StatusBar.tsx"), "utf8");
    expect(app).toMatch(/\{branch\}\s*\{status\?\.upstream && \([\s\S]*?t\("git\.branch\.short"[\s\S]*?\{workspaceId && <ChecksSegment workspaceId=\{workspaceId\} \/>\}/);
    expect(app).toMatch(/state === "failed" \? "error" : state === "running" \? "working" : state === "cancelled" \? "idle" : "done"/);
    const bar = /<div class="sc-statusbar">([\s\S]*?)<\/div>/.exec(h)?.[1] ?? "";
    const ahead = (n: number) => re(esc(t("git.branch.short", { ahead: n, behind: 0 })));
    // ↑0, then ↑1 once the fix is committed, ↑0 again once it's pushed, as the Files panel's chip.
    expect(bar).toMatch(new RegExp(`${re(agent.branch)}<span class="sc-swap"><span data-fx-cut="" style="--at:${CI_T.commit}s">${ahead(0)}</span>`));
    for (const [at, n] of [[CI_T.commit, 1], [CI_T.pushed, 0]] as const) expect(bar).toMatch(new RegExp(`style="--at:${at}s">${CUT}${ahead(n)}<`));
    // Then the CI segment: failed, with an error dot; running from the read after the push; passed on the later one.
    const seg = (at: number | null, dot: string, words: string) =>
      new RegExp(`${at === null ? "" : `style="--at:${at}s">${CUT}`}<span class="sc-ci-seg[^"]*"><span class="sc-dot"><svg[^>]*data-status="${dot}"[\\s\\S]*?</span>${re(words)}</span>`);
    expect(bar).toMatch(seg(null, "error", t("status.ciFailed", { n: 1 })));
    expect(bar).toMatch(seg(CI_T.running, "working", t("status.ciRunning")));
    expect(bar).toMatch(seg(CI_T.passed, "done", t("status.ciPassed")));
    expect(bar.indexOf(t("status.ciFailed", { n: 1 }))).toBeGreaterThan(bar.indexOf(agent.branch));
    expect(bar.indexOf(t("status.ciFailed", { n: 1 }))).toBeLessThan(bar.indexOf(t("status.version", { version: SCENE_BUILD })));
  });

  it("F-WB-01 AC9 D-3002 the status bar's agents segment follows its CI, waiting first, and counts the rail's agents at every change", () => {
    // StatusBar.tsx, in 0.12.0 and the tree: ChecksSegment, then AgentsSegment, then the right-hand side with the version.
    // AgentsSegment counts every agent: those waiting first, at medium weight in waiting's text, then those working, each
    // after its 6 px dot, and a count of none isn't drawn (the coordinator's S3 on 2966da74).
    const app = readFileSync(join(import.meta.dirname, "../../desktop/src/features/shell/StatusBar.tsx"), "utf8");
    expect(app).toMatch(/\{workspaceId && <ChecksSegment workspaceId=\{workspaceId\} \/>\}\s*<AgentsSegment \/>\s*<span className="flex-1" \/>/);
    expect(app).toMatch(/\{waiting > 0 && \([\s\S]*?"font-medium text-waiting-text"[\s\S]*?<StatusDot status="waiting" size=\{6\} \/>[\s\S]*?t\("status\.waiting", \{ n: waiting \}\)[\s\S]*?\{working > 0 && \([\s\S]*?<StatusDot status="working" size=\{6\} \/>[\s\S]*?t\("status\.working", \{ n: working \}\)/);
    expect(css).toMatch(/\.sc-agents-waiting \{ font-weight: 500; color: var\(--tn-status-waiting-text\); \}/);
    // The footer's text, every scene's: text-2xs in fg-muted (its token), the app's sans, tabular (nit 5 on 2966da74).
    expect(app).toContain('className="flex h-[var(--statusbar-h)] shrink-0 items-center gap-3 border-t border-divider bg-surface-1 px-3 text-2xs text-fg-muted"');
    const tokens = readFileSync(join(import.meta.dirname, "../../../packages/ui/src/styles/tokens.css"), "utf8");
    expect(tokens).toContain("--color-fg-muted: var(--tn-text-muted);");
    const footer = /\.sc-statusbar \{([^}]*)\}/.exec(css)?.[1] ?? "";
    for (const d of ["font-size: 0.625em", "font-variant-numeric: tabular-nums", "color: var(--tn-text-muted)"]) expect(footer, d).toContain(d);
    expect(footer).not.toContain("font-family");
    const bar = /<div class="sc-statusbar">([\s\S]*?)<\/div>/.exec(h)?.[1] ?? "";
    const at = (s: string) => bar.indexOf(s);
    expect(at(t("status.ciFailed", { n: 1 }))).toBeLessThan(at('class="sc-agents"'));
    expect(at('class="sc-agents"')).toBeLessThan(at(t("status.version", { version: SCENE_BUILD })));
    // The rail's rows: each agent's dot through the scene. At every change the segment holds the rail's count.
    const rail = /<aside class="sc-rail">[\s\S]*?<\/aside>/.exec(h)?.[0] ?? "";
    const rows = [...rail.matchAll(/<span class="sc-lane[ "]/g)].map((m) => {
      const status = rail.indexOf("<span", (m.index ?? 0) + 1);
      return rail.startsWith('<span class="sc-swap">', status)
        ? stepsAt(rail, status).map(([when, k]) => [when, /data-status="(\w+)"/.exec(k)?.[1] ?? ""] as const)
        : [[0, /data-status="(\w+)"/.exec(rail.slice(status))?.[1] ?? ""] as const];
    });
    expect(rows).toHaveLength(SAMPLE.order.length);
    const counts = (k: string) => ({ waiting: Number(/(\d+) waiting/.exec(text(k))?.[1] ?? 0), working: Number(/(\d+) working/.exec(text(k))?.[1] ?? 0) });
    const segment = stepsAt(bar, bar.indexOf('<span class="sc-swap">', at(t("status.ciFailed", { n: 1 })) + 1)).map(([when, k]) => [when, counts(k)] as const);
    expect(segment).toEqual(CI_AGENTS);
    expect(CI_AGENTS.map(([when]) => when)).toEqual([0, CI_T.sent, CI_T.done]);
    for (const [when, n] of CI_AGENTS) {
      const now = rows.map((r) => heldAt(r, when));
      expect({ waiting: now.filter((x) => x === "waiting").length, working: now.filter((x) => x === "working").length }, `${when}s`).toEqual(n);
    }
  });

  it("F-WB-01 AC9 D-3001 D-2702 a check's result is the app's Badge, as every badge the scenes draw: a small box in its tone's text, a 1 px border inside it (neutral's a fill), never a pill", () => {
    const app = (f: string) => readFileSync(join(import.meta.dirname, "../../..", f), "utf8");
    const dialog = app("apps/desktop/src/features/code/PullRequestChecks.tsx");
    const badge = app("packages/ui/src/components/Badge.tsx");
    const tokens = app("packages/ui/src/styles/tokens.css");
    /** A Tailwind colour's token: tokens.css maps --color-X to a --tn- token. */
    const token = (name: string) => new RegExp(`--color-${re(name)}: var\\((--tn-[\\w-]+)\\);`).exec(tokens)?.[1];
    const variants = Object.fromEntries([...(/const BUCKET_VARIANT = \{([^}]*)\}/.exec(dialog)?.[1] ?? "").matchAll(/(\w+): "(\w+)"/g)].map((m) => [m[1], m[2]]));
    expect(variants).toMatchObject({ pass: "done", fail: "error", pending: "outline" });
    for (const c of pr.checks) expect(h).toContain(`<span class="sc-badge sc-badge-${variants[c.bucket]}">${t(`checks.bucket.${c.bucket}`)}</span>`);
    // Every variant the scenes draw: the checks', the hero's (the radar's mark and Needs you in waiting's, a pane count
    // neutral) and a review card's risk (outline). Each as Badge.tsx classes it: its text, then its border or fill.
    const hero = renderToStaticMarkup(<WorkspaceScene />);
    const drawn = new Set([...[h, hero].join("").matchAll(/class="sc-badge(?:-quiet)? sc-badge-(\w+)"/g)].map((m) => m[1]));
    expect([...drawn].sort()).toEqual(["done", "error", "neutral", "outline", "waiting"]);
    for (const v of drawn) {
      const classes = new RegExp(`\\n  ${v}: "([^"]+)",`).exec(/const BADGE[^=]*= \{([\s\S]*?)\n\};/.exec(badge)?.[1] ?? "")?.[1] ?? "";
      const quiet = new RegExp(`\\n  ${v}: "([^"]+)",`).exec(/const QUIET[^=]*= \{([\s\S]*?)\n\};/.exec(badge)?.[1] ?? "")?.[1] ?? "";
      const text = /text-([\w-]+)/.exec(classes)?.[1] ?? "";
      expect(quiet, v).toBe(`text-${text}`);
      expect(css, v).toContain(`.sc-badge-${v} { color: var(${token(text)}); }`);
      const [, line, pct] = /border-([\w-]+?)(?:\/(\d+))? /.exec(`${classes} `) ?? [];
      const fill = /bg-([\w-]+)/.exec(classes)?.[1];
      if (line) {
        const ring = pct ? `color-mix(in oklab, var(${token(line)}) ${pct}%, transparent)` : `var(${token(line)})`;
        expect(css, v).toContain(`.sc-badge.sc-badge-${v} { border: 1px solid ${ring}; }`);
      } else expect(css, v).toContain(`.sc-badge.sc-badge-${v} { background: var(${token(fill ?? "")}); }`);
    }
    // Badge's box: h-4.5, px-1.5, rounded-xs, text-2xs, medium, and a 1 px border inside the box (the site's border-box,
    // as the app's: the coordinator's nit 6 on 2966da74); quiet, no box and text-xs. In the app's pixels.
    expect(badge).toMatch(/className=\{cx\("inline-flex h-4\.5 shrink-0 items-center gap-1 rounded-xs px-1\.5 text-2xs font-medium"/);
    expect(badge).toMatch(/className=\{cx\("inline-flex h-4\.5 shrink-0 items-center gap-1\.5 text-xs font-medium"/);
    const px = (v: string) => Number(new RegExp(`--${v}:\\s*(\\d+)px`).exec(tokens)?.[1]);
    expect([px("text-2xs"), px("text-xs"), px("radius-xs")]).toEqual([10, 11, 4]);
    const rule = (sel: string) => new RegExp(`(?:^|\\n)${re(sel)} \\{([^}]*)\\}`).exec(css)?.[1] ?? "";
    expect(rule(".sc-badge, .sc-badge-quiet")).toContain("height: calc(18 * var(--sc-px)); font-weight: 500;");
    expect(rule(".sc-badge")).toBe(" padding-inline: calc(6 * var(--sc-px)); border-radius: calc(4 * var(--sc-px)); font-size: calc(10 * var(--sc-px)); ");
    expect(rule(".sc-badge-quiet")).toBe(" font-size: calc(11 * var(--sc-px)); ");
    expect(css).not.toMatch(/\.sc-badge[^{]*\{[^}]*(box-shadow|999px)/);
    // One of the app's pixels: the window's em is 16 of them, worked out once on the window.
    expect(css).toContain('@property --sc-px { syntax: "<length>"; inherits: true; initial-value: 1px; }');
    expect(css).toMatch(/\.sc-window \{[^}]*--sc-px: calc\(1em \/ 16\);/);
    expect(css).toMatch(/\.sc-window \{[^}]*font-size: calc\(100cqw \/ 80\);/);
    // A check's row is ChecksDialog's: text-sm, 12 px, its workflow text-xs (nit 6 on 2966da74).
    expect(dialog).toContain('<div className="flex items-center gap-2 text-sm">');
    expect(dialog).toContain('<span className="truncate text-xs text-fg-subtle">{c.workflow}</span>');
    const [row, flow] = [px("text-sm"), px("text-xs")];
    expect([row, flow]).toEqual([12, 11]);
    expect(Number(/\.sc-ci-check-head \{[^}]*font-size: ([\d.]+)em;/.exec(css)?.[1]) * 16).toBe(row);
    expect(css).toMatch(new RegExp(`\\.sc-ci-check-flow \\{ font-size: calc\\(${flow}em / ${row}\\);`));
  });

  it("F-WB-01 AC9 D-3001 the imported comment is the reviewer's own, from the pull request, under its line, with Remove and no Edit", () => {
    const [c] = pr.comments;
    expect(c).toBeDefined();
    const note = /<span class="sc-diff-note"[^>]*>(?:(?!<span class="sc-diff-line).)*/s.exec(h)?.[0] ?? "";
    expect(note).toMatch(new RegExp(`^<span class="sc-diff-note" data-fx-in="" style="--at:${CI_T.imported}s">`));
    expect(note).toContain(esc(t("worktree.review.importedBy", { author: c?.author ?? "" })));
    expect(note).toContain(esc(c?.body ?? ""));
    expect(note).toContain(`>${t("worktree.review.remove")}<`);
    expect(note).not.toContain(`>${t("worktree.review.edit")}<`);
    // Not the second opinion's words.
    for (const s of story.secondOpinion.comments) expect(h).not.toContain(esc(s.body));
    // The diff is a run of git's own diff of that file, and the comment's line is in it.
    expect(runsIn(SESSION_DIFF, gitDiff(pr.worktree, c?.file ?? ""))).toBe(true);
    expect(SESSION_DIFF.some(([, , , newNo]) => newNo === c?.line)).toBe(true);
    expect(h).toContain(`>${esc(t("worktree.review.import", { n: pr.number }))}<`);
    expect(h).toMatch(new RegExp(`<div class="sc-note-toast sc-tone-success" data-fx-visit="" style="--at:${CI_T.imported}s;--stay:1.5s"><span class="sc-note-toast-text"><span class="sc-note-toast-title">${re(t("worktree.review.imported", { n: pr.comments.length }))}</span></span><svg[^>]*sc-note-toast-x`));
  });

  it("F-WB-01 AC9 D-3001 its final frame tells the story: checks passed, the agent's summary, the comment in the review, and 1 comment · Copy review · Send to Ada", () => {
    expect(h).toContain(t("checks.passed"));
    // WorktreeReview's bar: the count, Copy review (secondary), then Send to the worktree's agent by name (primary), once the import's toast has gone.
    expect(h).toContain(
      `<span class="sc-review-bar" data-fx-in="" style="--at:${CI_T.imported}s"><span class="sc-review-count">${t("worktree.review.count", { n: pr.comments.length })}</span><span class="sc-btn sc-btn-sm">${t("worktree.review.copy")}</span><span class="sc-btn sc-btn-sm sc-btn-primary" data-fx-enter="" style="--at:${CI_T.ready}s">${esc(t("worktree.review.sendTo", { name: agent.name }))}</span></span>`,
    );
    expect(CI_T.ready).toBeGreaterThan(CI_T.imported + 1.5);
    // The agent's last word, what it changed, arrives as it's done.
    const summary = SAMPLE.pr.fix.lines.at(-1) ?? "";
    expect(h).toMatch(new RegExp(`data-fx-line="" style="--at:${CI_T.done}s">${re(esc(summary))}<`));
    // What comes and goes isn't in it: the Checks dialog and the toasts.
    for (const s of [t("checks.title", { branch: pr.worktree }), t("failure.sent", { name: agent.name }), t("git.pushed", { branch: pr.worktree }), t("worktree.review.imported", { n: pr.comments.length })])
      expect(visiting(s), s).toBe(true);
    // Send is the one teal thing it ends on: each dialog's Close (primary) comes and goes with its dialog.
    const inVisit = (i: number) => h.lastIndexOf("data-fx-visit", i) > Math.max(h.lastIndexOf("data-fx-in", i), h.lastIndexOf("data-fx-pop", i));
    const primaries = [...h.matchAll(/class="sc-btn[^"]*sc-btn-primary"[^>]*>([^<]*)</g)];
    expect(primaries.map((m) => m[1])).toEqual([esc(t("worktree.review.sendTo", { name: agent.name })), ...Array<string>(4).fill(t("common.close"))]);
    expect(primaries.filter((m) => !inVisit(m.index ?? 0)).map((m) => m[1])).toEqual([esc(t("worktree.review.sendTo", { name: agent.name }))]);
  });
});

describe("F-WB-01 AC9 D-3002 a value's steps never show together, and words come in as DESIGN §4.7's enter", () => {
  it("F-WB-01 AC9 D-3002 Changes cuts each step at the next one's time, as the next enters: the first from the start, the last to stay", () => {
    // The app re-renders a value at once, so no frame draws two of its steps (the design lead's must-fix on 2966da74).
    // The browser test steps every scene's clock to show it; this is the markup that does it.
    const h = renderToStaticMarkup(
      <Changes
        steps={[
          [0, "a"],
          [1.5, "b"],
          [3, "c"],
        ]}
      />,
    );
    expect(h).toBe(
      '<span class="sc-swap"><span data-fx-cut="" style="--at:1.5s">a</span><span data-fx-enter="" style="--at:1.5s"><span data-fx-cut="" style="--at:3s">b</span></span><span data-fx-enter="" style="--at:3s">c</span></span>',
    );
    expect(renderToStaticMarkup(<Changes steps={[[0, "a"]]} />)).toBe('<span class="sc-swap"><span>a</span></span>');
    // A cut takes no time. The enter is a fade and a short rise (4.4 to 5.6 px at the scenes' 11 to 14 px text), at
    // --dur-pop, gated and paused as every effect is.
    expect(css).toMatch(/@keyframes sc-enter \{ from \{ opacity: 0; translate: 0 0\.4em; \} \}/);
    expect(css).toMatch(/& \[data-fx-enter\] \{ animation: sc-enter var\(--dur-pop\) var\(--ease-out-site\) var\(--at\) both paused; \}/);
    expect(css.match(/\[data-fx-line\], \[data-fx-enter\], \[data-fx-pop\]/g)?.length).toBe(2);
    // The final frame: what's cut isn't in it; what enters is.
    expect(css).toMatch(/\[data-fx-visit\], \[data-fx-out\], \[data-fx-cut\] \{ opacity: 0; \}/);
    expect(css).not.toMatch(/\[data-fx-enter\][^{]*\{[^}]*opacity: 0/);
  });

  it("F-WB-01 AC9 D-3002 no scene fades one thing out as another comes in, and only marks pop", () => {
    // A fade away is left only in DotChange: a dot changing colour is §4.7's status change. A pop is left only for a
    // mark: that dot, and the waiting card's ring. Words, a button's included, enter.
    const dir = join(import.meta.dirname, "../components/site/scenes");
    const uses = readdirSync(dir)
      .filter((f) => f.endsWith(".tsx"))
      .sort()
      .flatMap((f) => [...readFileSync(join(dir, f), "utf8").matchAll(/^.*fx\("(?:pop|out)".*$/gm)].map((m) => `${f}: ${m[0].trim()}`));
    expect(uses).toEqual([
      'DashboardScene.tsx: <span className="sc-ring" {...fx("pop", DASH_T.ask)} />',
      'parts.tsx: <span {...fx("out", seconds)}>',
      'parts.tsx: <span {...fx("pop", seconds)}>',
    ]);
    const parts = readFileSync(join(dir, "parts.tsx"), "utf8");
    expect(parts).toMatch(/export function DotChange[\s\S]*?<span \{\.\.\.fx\("out", seconds\)\}>\s*<Dot status=\{from\} \/>[\s\S]*?<span \{\.\.\.fx\("pop", seconds\)\}>\s*<Dot status=\{to\} \/>/);
  });
});
