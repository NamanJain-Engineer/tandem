// The sample repository's files (story.json's `repo`): the base branch's,
// and the ones each worktree's agent and the demo's first thread changed.
// They're kept as `shop/<place>/<path>.sample`, so nothing type-checks or
// lints them, and every diff and line count the demo shows is worked out
// from them.
import type { FileDiff } from "@tandem/protocol";
import { unifiedDiff } from "./udiff";

const RAW = import.meta.glob<string>("./shop/**/*.sample", { query: "?raw", import: "default", eager: true });

/** Files by place ("main", a worktree's slug, "thread") and path. */
const TREE: Record<string, Record<string, string>> = {};
for (const [key, text] of Object.entries(RAW)) {
  const m = /^\.\/shop\/([^/]+)\/(.+)\.sample$/.exec(key);
  if (m?.[1] && m[2]) (TREE[m[1]] ??= {})[m[2]] = text;
}

/** The base branch's files. */
export const MAIN: Readonly<Record<string, string>> = TREE.main ?? {};

/** The files `place` changed, as they are there now. */
export function changedIn(place: string): Readonly<Record<string, string>> {
  return TREE[place] ?? {};
}

/** One file's change, as the core's diffs describe it. */
export function fileDiff(path: string, before: string | null, after: string): FileDiff {
  const d = unifiedDiff(path, before, after);
  return {
    path,
    change: before === null ? "added" : "modified",
    additions: d.additions,
    deletions: d.deletions,
    binary: false,
    patch: d.patch,
    truncated: false,
  };
}

/** What `place` changed against the base branch, a file each, by path. */
export function diffsOf(place: string): FileDiff[] {
  return Object.entries(changedIn(place))
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([path, text]) => fileDiff(path, MAIN[path] ?? null, text));
}

/** How many tests a sample test file has: one per `it(`, and one per row
 * of an `it.each` table. */
export function testsIn(text: string): number {
  const lines = text.split("\n").map((l) => l.trim());
  let n = 0;
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i] ?? "";
    if (line.startsWith("it(")) n++;
    else if (line.startsWith("it.each([")) {
      // `it.each([a, b])(` on one line, or a row a line up to `])(`.
      const inline = /^it\.each\(\[(.*)\]\)\(/.exec(line);
      if (inline?.[1] !== undefined) n += inline[1].split(",").length;
      else
        for (; i + 1 < lines.length && !lines[i + 1]?.startsWith("])("); i++) n++;
    }
  }
  return n;
}

/** What `pnpm test [filter]` prints in a checkout with these files: vitest
 * runs the test files whose path has `filter` in it, and they pass. */
export function testRun(files: Readonly<Record<string, string>>, filter = ""): string[] {
  const run = Object.keys(files)
    .filter((p) => /\.test\.tsx?$/.test(p) && p.includes(filter))
    .sort();
  const head = ["> shop@1.4.0 test", `> vitest run${filter ? ` ${filter}` : ""}`, ""];
  if (run.length === 0) return [...head, `No test files found, exiting with code 1`, `filter: ${filter}`];
  const counts = run.map((p) => testsIn(files[p] ?? ""));
  const total = counts.reduce((a, b) => a + b, 0);
  return [
    ...head,
    ...run.map((p, i) => ` ✓ ${p} (${counts[i]} tests) ${8 + (counts[i] ?? 0) * 2}ms`),
    "",
    ` Test Files  ${run.length} passed (${run.length})`,
    `      Tests  ${total} passed (${total})`,
  ];
}
