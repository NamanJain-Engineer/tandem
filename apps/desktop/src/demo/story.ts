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
  task: string;
  state: StoryState;
  steps: StoryStep[];
  /** A finished agent's last words. */
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

export type Story = {
  repo: string;
  base: string;
  /** The repository's check command (Land worktrees, the review queue). */
  check: string;
  /** The base branch's commits, newest first. */
  history: StoryCommit[];
  remote: string;
  engines: Record<string, { label: string; version: string }>;
  worktrees: StoryWorktree[];
  dev: { command: string; port: number; requests: string[] };
  /** The symbol two worktrees both change (the intent radar), and those two. */
  overlap: { symbol: string; file: string; between: string[] };
  /** Another engine's line comments on a worktree's diff (Second opinion). */
  secondOpinion: { worktree: string; engine: string; comments: StoryComment[] };
  pr: {
    number: number;
    worktree: string;
    title: string;
    url: string;
    checks: Array<{ name: string; bucket: "pass" | "fail" }>;
    /** A reviewer's line comments, in their own words. */
    comments: Array<StoryComment & { author: string }>;
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
