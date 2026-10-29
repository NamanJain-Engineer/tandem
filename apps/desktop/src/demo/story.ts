// The sample world the browser demo and the site's product scenes share
// (D-1899, D-2609): one repository, its three worktrees and what their
// agents did. Both read `story.json`, so neither keeps a copy of its names;
// the repository's files are beside it in `shop/` (see repo.ts). What would
// come from outside the computer (a pull request, its remote and reviewer,
// engine versions) says it's a sample in the data itself, since the app's
// own strings can't say so.
import story from "./story.json";

/** One thing an agent did: read a file, edit one (lines added and removed),
 * or run a command (and how many tests passed, once they have). */
export type StoryStep =
  | { read: string }
  | { edit: string; add: number; del: number }
  | { run: string; passed?: number };

export type StoryEdit = Extract<StoryStep, { edit: string }>;

/** Where a worktree's agent is when the demo opens. */
export type StoryState = "done" | "working" | "waiting";

export type StoryWorktree = {
  slug: string;
  /** A key of `engines`. */
  engine: string;
  /** Its agent pane's first name, as the app gives every agent pane one
   * (F-CD-12; from the app's `names.json`). */
  agentName: string;
  task: string;
  state: StoryState;
  steps: StoryStep[];
  /** A finished agent's last words (the waiting one's, once it's allowed
   * to run its command and finishes). */
  closing?: string;
  /** The command a waiting agent asks to run. */
  ask?: string;
  /** Its commits once its agent has committed its work, newest first (a
   * finished agent's are made; a working or waiting one's are still to come). */
  commits: StoryCommit[];
  /** The merge commit in the base once it lands. */
  mergeCommit: string;
};

export type StoryCommit = { sha: string; subject: string };

export type StoryComment = { file: string; line: number; body: string };

/** A pull request check as gh reports it (F-WT-08). The failed one carries
 * its job's log as the failed step printed it. */
export type StoryCheck = {
  name: string;
  bucket: "pass" | "fail";
  workflow: string;
  /** The job's page, `…/actions/runs/<run>/job/<job>`. */
  link: string;
  log?: string[];
};

export type Story = {
  repo: string;
  base: string;
  /** The repository's check command (Land worktrees, the review queue). */
  check: string;
  /** The base branch's commits, newest first. */
  history: StoryCommit[];
  remote: string;
  /** Each engine's label and sample version, and what its hooks report
   * (registry.json's `hooksSupport`, which drift.test.ts holds it to). */
  engines: Record<string, { label: string; version: string; hooksSupport: string }>;
  worktrees: StoryWorktree[];
  dev: { command: string; port: number; requests: string[] };
  /** The symbol two worktrees both change (the intent radar), and those two. */
  overlap: { symbol: string; file: string; between: string[] };
  /** Plan with Lead's goal and the tasks it splits into, an engine each
   * (the site's plan scene; the demo drafts from whatever goal is typed). */
  plan: { goal: string; tasks: Array<{ title: string; engine: string }> };
  /** Another engine's line comments on a worktree's diff (Second opinion). */
  secondOpinion: { worktree: string; engine: string; comments: StoryComment[] };
  pr: {
    number: number;
    worktree: string;
    title: string;
    url: string;
    checks: StoryCheck[];
    /** A reviewer's line comments, in their own words. */
    comments: Array<StoryComment & { author: string }>;
    /** "Send failure to agent": what the worktree's agent does about the
     * failed check (its files are in `shop/ci-fix/`), and the CI run that
     * pushing its commit starts. */
    fix: { steps: StoryStep[]; commit: StoryCommit; closing: string; run: number };
  };
};

export const STORY = story as Story;

/** A worktree's branch, as the core names it (`tandem/<slug>`). */
export function branchOf(slug: string): string {
  return `tandem/${slug}`;
}

export function worktree(slug: string): StoryWorktree {
  const w = STORY.worktrees.find((x) => x.slug === slug);
  if (!w) throw new Error(`story.json has no worktree ${slug}`);
  return w;
}

/** The worktree whose agent is in `state` when the demo opens. */
export function worktreeIn(state: StoryState): StoryWorktree {
  const w = STORY.worktrees.find((x) => x.state === state);
  if (!w) throw new Error(`story.json has no ${state} worktree`);
  return w;
}

export function engineLabel(engine: string): string {
  return STORY.engines[engine]?.label ?? engine;
}

/** Each engine's own question before it runs a command, as its terminal
 * puts it. The app sees an agent pane wait only when its screen matches
 * the engine's waitingPatterns (registry.json), so the demo asks in words
 * they match (D-2624). */
const ASKS: Record<string, (command: string) => string> = {
  gemini: (command) => `Allow execution of: '${command}'?`,
};

/** The waiting agent's question in its terminal: the engine's own, since
 * 0.12.0 raises no approval for it (only `vibezeo run` does). The demo's
 * pane and the site's scenes both read it from here (D-1899). */
export function askLine(w: StoryWorktree): string {
  const ask = ASKS[w.engine];
  if (!ask) throw new Error(`story.json: no question in ${w.engine}'s words`);
  return ask(w.ask ?? "");
}

/** The tool Claude Code's hooks name for a step (hook.rs's PreToolUse). */
export function toolOf(step: StoryStep): string {
  return "read" in step ? "Read" : "edit" in step ? "Edit" : "Bash";
}

/** The line under an agent's title on its Dashboard card, as dashboard.rs
 * draws it from its status record: the preview, else the tool. An agent
 * pane's record has no preview, and a tool only while Claude Code's hooks
 * report one at work; Codex's notify and an engine without hooks name
 * none (D-2624). So no card in the opening frame has a line: Ada has
 * finished, Arlo is Codex, and Bea's Gemini CLI has no hooks. */
export function previewOf(w: StoryWorktree): string | null {
  if (w.state !== "working" || STORY.engines[w.engine]?.hooksSupport !== "claude-hooks") return null;
  const step = w.steps.at(-1);
  return step ? toolOf(step) : null;
}

export function editsOf(w: StoryWorktree): StoryEdit[] {
  return w.steps.filter((s): s is StoryEdit => "edit" in s);
}

/** What a worktree's diff adds up to: its files, lines added and removed. */
export function totalsOf(w: StoryWorktree): { files: number; adds: number; dels: number } {
  const edits = editsOf(w);
  return {
    files: new Set(edits.map((e) => e.edit)).size,
    adds: edits.reduce((n, e) => n + e.add, 0),
    dels: edits.reduce((n, e) => n + e.del, 0),
  };
}
