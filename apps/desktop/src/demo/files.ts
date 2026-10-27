// The browser demo's Files tab and git panel: the sample repository
// (repo.ts) as each checkout sees it, the repository's own folder on main
// and each worktree on its branch. Edits stay in the page and checks work
// like the core's (a stale hash is a conflict); nothing is saved. The demo
// runs no git actions (D-204).
import type { Branch, EditorTabs, FileChange, FileContent, FileDiff, FileStatus, FsEntry, GitPanel } from "@tandem/protocol";
import { MAIN, fileDiff } from "./repo";

export const NO_GIT = "The browser demo doesn't run git commands.";

/** One checkout: its folder and branch, what its branch has committed
 * beyond main's files, and what's changed in it and not committed. */
export type CheckoutSeed = {
  id: string;
  root: string;
  branch: string;
  committed: Readonly<Record<string, string>>;
  uncommitted: Readonly<Record<string, string>>;
};

type Checkout = {
  root: string;
  branch: string;
  /** Its branch's files, as committed. */
  head: Map<string, string>;
  /** Files changed since, by an agent or in the page. */
  work: Map<string, string>;
};

const hash = (text: string) => {
  let h = 0x811c9dc5;
  for (let i = 0; i < text.length; i++) h = Math.imul(h ^ text.charCodeAt(i), 0x01000193);
  return `demo-${(h >>> 0).toString(16)}`;
};

const missing = () => ({ code: "not_found", message: "That file or folder doesn't exist" });

export class DemoFiles {
  private checkouts = new Map<string, Checkout>();
  /** The editor's open files, by context ("ws:<id>"). */
  private tabs = new Map<string, EditorTabs>();

  constructor(seeds: CheckoutSeed[]) {
    for (const s of seeds) {
      this.checkouts.set(s.id, {
        root: s.root,
        branch: s.branch,
        head: new Map(Object.entries({ ...MAIN, ...s.committed })),
        work: new Map(Object.entries(s.uncommitted)),
      });
    }
  }

  private at(id: string): Checkout {
    const c = this.checkouts.get(id);
    if (!c) throw { code: "not_found", message: "That folder isn't open in the demo." };
    return c;
  }

  /** A file as it is in the checkout's folder now. */
  text(id: string, path: string): string | undefined {
    const c = this.at(id);
    return c.work.get(path) ?? c.head.get(path);
  }

  /** Every file in the folder now, by path. */
  paths(id: string): string[] {
    const c = this.at(id);
    return [...new Set([...c.head.keys(), ...c.work.keys()])].sort();
  }

  /** How a file differs from its branch's commit: modified, untracked
   * (new and not added), or not at all. */
  private change(c: Checkout, path: string): FileChange | null {
    const now = c.work.get(path);
    if (now === undefined) return null;
    const was = c.head.get(path);
    if (was === undefined) return "untracked";
    return was === now ? null : "modified";
  }

  /** Changed files and their changes, by path. */
  changes(id: string): FileStatus[] {
    const c = this.at(id);
    return [...c.work.keys()]
      .sort()
      .flatMap((path) => {
        const change = this.change(c, path);
        return change ? [{ path, change }] : [];
      });
  }

  list(id: string, dir: string): FsEntry[] {
    const c = this.at(id);
    const prefix = dir ? `${dir}/` : "";
    const dirs = new Set<string>();
    const files: string[] = [];
    for (const path of this.paths(id)) {
      if (!path.startsWith(prefix)) continue;
      const rest = path.slice(prefix.length);
      const at = rest.indexOf("/");
      if (at >= 0) dirs.add(rest.slice(0, at));
      else files.push(rest);
    }
    const changed = this.changes(id);
    // A folder is Untracked when everything changed in it is new, else Modified.
    const folderStatus = (p: string): FileChange | undefined => {
      const inside = changed.filter((f) => f.path.startsWith(`${p}/`));
      if (inside.length === 0) return undefined;
      return inside.every((f) => f.change === "untracked") ? "untracked" : "modified";
    };
    const sort = (a: string, b: string) => a.toLowerCase().localeCompare(b.toLowerCase());
    return [
      ...[...dirs].sort(sort).map((name): FsEntry => {
        const path = prefix + name;
        return { name, path, kind: "dir", symlink: false, ignored: false, gitStatus: folderStatus(path) };
      }),
      ...files.sort(sort).map((name): FsEntry => {
        const path = prefix + name;
        return { name, path, kind: "file", symlink: false, ignored: false, gitStatus: this.change(c, path) ?? undefined };
      }),
    ];
  }

  read(id: string, path: string): FileContent {
    const text = this.text(id, path);
    if (text === undefined) throw missing();
    return {
      path,
      kind: "text",
      text,
      size: text.length,
      readonly: false,
      readonlyReason: null,
      truncated: false,
      hash: hash(text),
      mtime: 0,
      dataUrl: null,
      hex: null,
    };
  }

  write(id: string, path: string, text: string, expected: string | null): { hash: string; mtime: number } {
    const now = this.text(id, path);
    if (now === undefined ? expected !== null : expected !== hash(now)) {
      throw { code: "conflict", message: "File changed on disk" };
    }
    this.at(id).work.set(path, text);
    return { hash: hash(text), mtime: Date.now() };
  }

  /** An agent's edit, in its checkout's folder. */
  edit(id: string, path: string, text: string): void {
    this.at(id).work.set(path, text);
  }

  /** How many tracked files have uncommitted changes (new, untracked files
   * don't count), as Land worktrees counts them. */
  dirty(id: string): number {
    return this.changes(id).filter((f) => f.change !== "untracked").length;
  }

  /** Commits everything changed in the folder, new files included. */
  commit(id: string): void {
    const c = this.at(id);
    for (const [path, text] of c.work) c.head.set(path, text);
    c.work.clear();
  }

  /** What the checkout's branch has committed that main's files don't
   * have, by path. */
  committedChanges(id: string): Record<string, string> {
    const c = this.at(id);
    return Object.fromEntries([...c.head].filter(([path, text]) => MAIN[path] !== text));
  }

  /** The files `id`'s branch has committed, as they stand. */
  head(id: string): Map<string, string> {
    return new Map(this.at(id).head);
  }

  /** Sets the branch's committed files (a landing, or its undo). */
  setHead(id: string, files: Map<string, string>): void {
    this.at(id).head = new Map(files);
  }

  compare(id: string, path: string, text: string): FileDiff {
    return fileDiff(path, this.text(id, path) ?? "", text);
  }

  gitStatus(id: string): GitPanel {
    const c = this.at(id);
    return {
      root: c.root,
      folder: "",
      status: {
        branch: c.branch,
        upstream: null,
        ahead: 0,
        behind: 0,
        staged: [],
        unstaged: this.changes(id),
        noCommits: false,
      },
    };
  }

  /** What `git diff` shows for one changed file: its commit against now. */
  gitDiff(id: string, path: string): FileDiff {
    const c = this.at(id);
    const now = this.text(id, path);
    if (now === undefined) throw missing();
    const d = fileDiff(path, c.head.get(path) ?? null, now);
    return { ...d, change: this.change(c, path) ?? d.change };
  }

  /** The local branches: main and every worktree's, the checkout's current. */
  gitBranches(id: string): Branch[] {
    const current = this.at(id).branch;
    const names = [...new Set([...this.checkouts.values()].map((c) => c.branch))];
    return names.map((name) => ({ name, current: name === current, remote: false, upstream: null, ahead: 0, behind: 0 }));
  }

  savedTabs(key: string): EditorTabs {
    return this.tabs.get(key) ?? { tabs: [], active: null, expanded: [], showIgnored: false, divider: null, view: null };
  }

  saveTabs(key: string, state: EditorTabs): void {
    this.tabs.set(key, state);
  }
}
