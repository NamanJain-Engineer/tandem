// The animated product scenes (F-WB-01 AC9, D-1880): their words. Every
// label the app itself would draw comes from its catalog by key, so a scene
// says exactly what the app says; what's left is sample work (branches,
// files, terminal lines), which the captions call simplified, and which
// comes from the in-app demo's story.json (D-1899, D-2609).
import en from "../../desktop/src/i18n/en.json";
import story from "../../desktop/src/demo/story.json";
import { RELEASE } from "./release";

const CATALOG = en as Record<string, string>;

/** True when `version` is `min` or later (x.y.z). */
export function atLeast(version: string, min: string): boolean {
  const a = version.split(".").map(Number);
  const b = min.split(".").map(Number);
  for (let i = 0; i < 3; i++) if ((a[i] ?? 0) !== (b[i] ?? 0)) return (a[i] ?? 0) > (b[i] ?? 0);
  return true;
}

/** The release the scenes show, and whether it has 0.9.0's worktree landing,
 * pull request checks and intent radar (F-WT-07, F-WT-08, F-WT-10). Before
 * that release ships, the scenes leave them out (D-128). */
export const SCENE_BUILD = RELEASE.version;
export const LANDS = atLeast(SCENE_BUILD, "0.9.0");

/** One of the app's strings, with its `{name}` values and simple plurals
 * (`{n, plural, one {# file} other {# files}}`) filled in. Throws on a key
 * the catalog doesn't have, so a renamed string fails the build. */
export function t(key: string, vars: Record<string, string | number> = {}): string {
  const raw = CATALOG[key];
  if (raw === undefined) throw new Error(`scenes: no "${key}" in the app's catalog`);
  return format(raw, vars);
}

function format(s: string, vars: Record<string, string | number>): string {
  let out = "";
  for (let i = 0; i < s.length; i++) {
    if (s[i] !== "{") {
      out += s[i];
      continue;
    }
    // Find the matching brace.
    let depth = 0;
    let j = i;
    for (; j < s.length; j++) {
      if (s[j] === "{") depth++;
      else if (s[j] === "}" && --depth === 0) break;
    }
    const body = s.slice(i + 1, j);
    const [name, kind] = body.split(",").map((x) => x.trim());
    if (kind === "plural") {
      const n = Number(vars[name ?? ""] ?? 0);
      const forms = pluralForms(body.slice(body.indexOf("plural") + "plural".length));
      const form = forms[`=${n}`] ?? (n === 1 ? forms.one : undefined) ?? forms.other ?? "";
      out += format(form.replace(/#/g, String(n)), vars);
    } else if (kind === "number") out += String(vars[name ?? ""] ?? "");
    else out += String(vars[name ?? ""] ?? `{${name}}`);
    i = j;
  }
  return out;
}

/** A plural's forms (`one {# file} other {# files in {base}}`), braces
 * inside a form included. */
function pluralForms(rest: string): Record<string, string> {
  const forms: Record<string, string> = {};
  const re = /(=\d+|zero|one|two|few|many|other)\s*\{/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(rest))) {
    let depth = 1;
    let j = re.lastIndex;
    for (; j < rest.length && depth > 0; j++) {
      if (rest[j] === "{") depth++;
      else if (rest[j] === "}") depth--;
    }
    forms[m[1] as string] = rest.slice(re.lastIndex, j - 1);
    re.lastIndex = j;
  }
  return forms;
}

/** One thing an agent did, as story.json says it: read a file, edit one
 * (lines added and removed), or run a command (and the tests that passed). */
type Step = { read: string } | { edit: string; add: number; del: number } | { run: string; passed?: number };
type Edit = Extract<Step, { edit: string }>;
type EngineId = keyof typeof story.engines;

/** The first eight words of a prompt: how the app titles an agent's pane
 * until its engine names the work (promptTitle, features/code/titles.ts). */
export function promptTitle(prompt: string): string {
  const words = prompt.trim().split(/\s+/).filter(Boolean);
  const head = words.slice(0, 8).join(" ");
  return words.length > 8 ? `${head}…` : head;
}

/** The story's worktree in `state`: one is done, one working and one
 * waiting on you, whichever engines run them. */
function worktree(state: "done" | "working" | "waiting") {
  const all = story.worktrees.filter((x) => x.state === state);
  const w = all[0];
  if (!w || all.length > 1) throw new Error(`scenes: story.json needs one ${state} worktree, not ${all.length}`);
  return { ...w, steps: w.steps as readonly Step[] };
}

/** An agent's terminal as the scenes print it: its task, then each step. */
function terminal(w: { task: string; steps: readonly Step[] }): string[] {
  return [
    `› ${w.task}`,
    ...w.steps.flatMap((s): string[] => {
      if ("read" in s) return [`  reading ${s.read}`];
      if ("edit" in s) return [`  editing ${s.edit}  +${s.add}${s.del ? ` −${s.del}` : ""}`];
      return [`  $ ${s.run}`, ...(s.passed ? [`  Tests  ${s.passed} passed`] : [])];
    }),
  ];
}

const finished = worktree("done");
const working = worktree("working");
const waiting = worktree("waiting");
const edits = finished.steps.filter((s): s is Edit => "edit" in s);
const label = (engine: string) => {
  const e = story.engines[engine as EngineId];
  if (!e) throw new Error(`scenes: story.json has no engine ${engine}`);
  return e.label;
};
/** A worktree as the scenes draw it: its engine, its agent's first name
 * (F-CD-12), its pane's title, its branch, and its commits and the merge
 * commit it lands as. */
const lane = (w: { engine: string; agentName: string; task: string; slug: string; commits: readonly unknown[]; mergeCommit: string }) => ({
  engine: label(w.engine),
  name: w.agentName,
  title: promptTitle(w.task),
  branch: w.slug,
  commits: w.commits.length,
  merge: w.mergeCommit,
});

/** The sample work the scenes show: one repository and its worktrees. It's
 * the world the in-app demo shows too, read from its story.json (D-1899,
 * D-2609), so the two never drift; the files the agents change are beside
 * it in the demo's sample repository. */
export const SAMPLE = {
  workspace: story.repo,
  base: story.base,
  /** The repository's check command, which Land worktrees runs. */
  check: story.check,
  /** The worktrees by what they're doing (D-3000). */
  worktrees: { finished: lane(finished), working: lane(working), waiting: lane(waiting) },
  /** All of them, in the story's order (the order they land in). */
  order: story.worktrees.map(lane),
  symbol: story.overlap.symbol,
  /** What each agent's terminal prints, in order: the story's steps. */
  lines: {
    finished: terminal(finished),
    working: terminal(working),
    waiting: terminal(waiting),
    shell: [`$ ${story.dev.command}`, `  ready on localhost:${story.dev.port}`, ...story.dev.requests.map((r) => `  ${r}`)],
  },
  /** The command the waiting agent asks to run. */
  ask: waiting.ask ?? "",
  /** The finished worktree's diff: the sum of its agent's edits. */
  diff: { files: edits.length, adds: edits.reduce((n, e) => n + e.add, 0), dels: edits.reduce((n, e) => n + e.del, 0) },
  /** The engine that gives the second opinion, and its line comments. */
  reviewer: label(story.secondOpinion.engine),
  comments: story.secondOpinion.comments.map((c) => ({ file: c.file, line: c.line, text: c.body })),
  failedCheck: story.pr.checks.find((c) => c.bucket === "fail")?.name ?? "",
  /** A plan from Lead in the same repository (0.12.0's F-TK-07): its goal,
   * and the tasks Execute all ran, each in its own worktree, numbered as the
   * tracker numbers them (`ISSUE_PREFIX`, crates/tandem-tasks/src/model.rs). */
  plan: {
    goal: story.plan.goal,
    tasks: story.plan.tasks.map((x, i) => ({ id: `TAN-${31 + i}`, title: x.title, engine: label(x.engine) })),
  },
} as const;
