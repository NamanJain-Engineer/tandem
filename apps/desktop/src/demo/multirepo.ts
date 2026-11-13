// The demo's second workspace (F-CD-22, D-2570): a folder that isn't itself
// a repository, holding two repositories side by side (story.json's
// `multiRepo`), and one worktree across both. Their files are kept as
// `bookings/<place>/<repo>/<path>.sample`: `main` each repository's
// committed files, the worktree's slug the files it changed. Paths here
// start with the repository's name (`api/src/routes.ts`), as the folder's
// Files tab and the worktree's diff show them.
import type { FileDiff } from "@tandem/protocol";
import { fileDiff } from "./repo";
import { STORY, branchOf } from "./story";

const RAW = import.meta.glob<string>("./bookings/**/*.sample", {
  query: "?raw",
  import: "default",
  eager: true,
});

/** Files by place ("main", the worktree's slug) and `repo/path`. */
const TREE: Record<string, Record<string, string>> = {};
for (const [key, text] of Object.entries(RAW)) {
  const m = /^\.\/bookings\/([^/]+)\/(.+)\.sample$/.exec(key);
  if (m?.[1] && m[2]) (TREE[m[1]] ??= {})[m[2]] = text;
}

export const MULTI = STORY.multiRepo;

/** The folder, and the worktree's parent folder inside it, as the core
 * makes it: `<folder>/.tandem/worktrees/<slug>`. */
export const MULTI_ROOT = `/home/you/${MULTI.folder}`;
export const MULTI_LANE_ROOT = `${MULTI_ROOT}/.tandem/worktrees/${MULTI.worktree}`;
export const MULTI_BRANCH = branchOf(MULTI.worktree);

/** Each repository's committed files. */
export const MULTI_MAIN: Readonly<Record<string, string>> = TREE.main ?? {};

/** What the worktree changed, in every repository, as it is now. */
export const MULTI_CHANGED: Readonly<Record<string, string>> = TREE[MULTI.worktree] ?? {};

/** The worktree's diff as `lane_diff` returns it for a multi-repo worktree:
 * each repository's changes in turn, its name leading every path. */
export function multiDiff(
  files: Readonly<Record<string, string>>,
  base: Readonly<Record<string, string>> = MULTI_MAIN,
): FileDiff[] {
  return MULTI.repos.flatMap((repo) =>
    Object.keys(files)
      .filter((path) => path.startsWith(`${repo}/`) && files[path] !== base[path])
      .sort()
      .map((path) => fileDiff(path, base[path] ?? null, files[path] ?? "")),
  );
}
