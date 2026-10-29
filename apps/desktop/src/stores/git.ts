// The git panel (F-CD-20): per context, the repository's status, the picked
// change's diff, the commit message and the action in flight. The Files
// tab's Changes view and the status bar's branch share it. A status is read
// again after every action, on the Files watcher's batches, when an agent
// finishes a turn and when the window gets focus; every read is libgit2, so
// polling never runs a repository's helpers (TL-06 rule 6).
import { useEffect } from "react";
import { create } from "zustand";
import type { Branch, Committed, FileContext, FileDiff, GitPanel, IpcError } from "@tandem/protocol";
import { invoke } from "../ipc/invoke";
import { useStatus } from "./status";
import { checksAfterPush } from "./worktrees";

/** A commit runs the repository's hooks, which can take a while. */
const LOCAL_TIMEOUT_MS = 300_000;
/** The core gives up on the network after 120 s. */
const NETWORK_TIMEOUT_MS = 130_000;

export type GitAction = "stage" | "unstage" | "discard" | "commit" | "push" | "pull" | "switch" | "branch";

/** A change in the list: its repository-relative path and which side. */
export type Picked = { path: string; staged: boolean };

export type GitDiff = { picked: Picked; at: "loading" | "ready" | "error"; diff: FileDiff | null; error: string | null };

export type GitView = {
  context: FileContext;
  at: "loading" | "ready" | "error";
  /** `null` outside a repository. */
  panel: GitPanel | null;
  error: string | null;
  picked: Picked | null;
  diff: GitDiff | null;
  busy: GitAction | null;
  message: string;
  branches: Branch[] | null;
};

type GitState = {
  byKey: Record<string, GitView>;
  refresh: (context: FileContext) => Promise<void>;
  /** Refreshes every context something is showing. */
  refreshAll: () => void;
  pick: (context: FileContext, picked: Picked | null) => Promise<void>;
  setMessage: (context: FileContext, message: string) => void;
  stage: (context: FileContext, paths: string[]) => Promise<IpcError | null>;
  unstage: (context: FileContext, paths: string[]) => Promise<IpcError | null>;
  /** The caller has confirmed: this can't be undone. */
  discard: (context: FileContext, paths: string[]) => Promise<IpcError | null>;
  commit: (context: FileContext) => Promise<IpcError | null>;
  push: (context: FileContext, publish?: boolean) => Promise<IpcError | null>;
  pull: (context: FileContext) => Promise<IpcError | null>;
  loadBranches: (context: FileContext) => Promise<void>;
  switchTo: (context: FileContext, branch: string) => Promise<IpcError | null>;
  createBranch: (context: FileContext, name: string) => Promise<IpcError | null>;
};

/** The same keys as the Files tab's contexts (`ws:` / `th:`). */
export function gitKey(context: FileContext): string {
  return context.type === "workspace" ? `ws:${context.workspaceId}` : `th:${context.threadId}`;
}

function asError(e: unknown): IpcError {
  if (e && typeof e === "object" && "message" in e) return e as IpcError;
  return { code: "internal", message: String(e), stage: "git", hint: null, retryable: false };
}

function blank(context: FileContext): GitView {
  return { context, at: "loading", panel: null, error: null, picked: null, diff: null, busy: null, message: "", branches: null };
}

/** Reads in flight per key, and whether another was asked for meanwhile. */
const reading = new Map<string, { again: boolean }>();
/** How many views show each key (the Changes view, the status bar). */
const shown = new Map<string, number>();

export const useGit = create<GitState>((set, get) => {
  const patch = (key: string, next: Partial<GitView> | ((v: GitView) => Partial<GitView>)) =>
    set((s) => {
      const cur = s.byKey[key];
      if (!cur) return s;
      return { byKey: { ...s.byKey, [key]: { ...cur, ...(typeof next === "function" ? next(cur) : next) } } };
    });

  const ensure = (context: FileContext): string => {
    const key = gitKey(context);
    if (!get().byKey[key]) set((s) => ({ byKey: { ...s.byKey, [key]: blank(context) } }));
    return key;
  };

  const loadDiff = async (key: string, picked: Picked) => {
    const v = get().byKey[key];
    if (!v) return;
    try {
      const diff = await invoke<FileDiff>("git_diff", { context: v.context, path: picked.path, staged: picked.staged });
      if (get().byKey[key]?.picked !== picked) return;
      patch(key, { diff: { picked, at: "ready", diff, error: null } });
    } catch (e) {
      if (get().byKey[key]?.picked !== picked) return;
      patch(key, { diff: { picked, at: "error", diff: null, error: asError(e).message } });
    }
  };

  /** Runs an action, then reads the status again. */
  const act = async (context: FileContext, action: GitAction, run: () => Promise<unknown>): Promise<IpcError | null> => {
    const key = ensure(context);
    if (get().byKey[key]?.busy) return null;
    patch(key, { busy: action });
    try {
      await run();
      return null;
    } catch (e) {
      return asError(e);
    } finally {
      patch(key, { busy: null });
      await get().refresh(context);
    }
  };

  return {
    byKey: {},

    refresh: async (context) => {
      const key = ensure(context);
      const inFlight = reading.get(key);
      if (inFlight) {
        inFlight.again = true;
        return;
      }
      const mine = { again: false };
      reading.set(key, mine);
      try {
        do {
          mine.again = false;
          try {
            const panel = await invoke<GitPanel | null>("git_status", { context });
            patch(key, (v) => {
              // A picked change that's gone (committed, discarded) is dropped.
              const list = v.picked ? (v.picked.staged ? panel?.status.staged : panel?.status.unstaged) : null;
              const still = v.picked && list?.some((f) => f.path === v.picked?.path);
              return { at: "ready", panel, error: null, ...(still ? {} : { picked: null, diff: null }) };
            });
            const picked = get().byKey[key]?.picked;
            if (picked) await loadDiff(key, picked);
          } catch (e) {
            patch(key, { at: "error", error: asError(e).message });
          }
        } while (mine.again);
      } finally {
        reading.delete(key);
      }
    },

    refreshAll: () => {
      for (const [key, n] of shown) {
        const v = get().byKey[key];
        if (n > 0 && v) void get().refresh(v.context);
      }
    },

    pick: async (context, picked) => {
      const key = ensure(context);
      patch(key, { picked, diff: picked ? { picked, at: "loading", diff: null, error: null } : null });
      if (picked) await loadDiff(key, picked);
    },

    setMessage: (context, message) => {
      const key = ensure(context);
      patch(key, { message });
    },

    stage: (context, paths) =>
      act(context, "stage", () => invoke("git_stage", { context, paths }, { timeoutMs: LOCAL_TIMEOUT_MS })),
    unstage: (context, paths) =>
      act(context, "unstage", () => invoke("git_unstage", { context, paths }, { timeoutMs: LOCAL_TIMEOUT_MS })),
    discard: (context, paths) =>
      act(context, "discard", () => invoke("git_discard", { context, paths }, { timeoutMs: LOCAL_TIMEOUT_MS })),

    commit: async (context) => {
      const key = ensure(context);
      const message = get().byKey[key]?.message ?? "";
      const err = await act(context, "commit", () =>
        invoke<Committed>("git_commit", { context, message }, { timeoutMs: LOCAL_TIMEOUT_MS }),
      );
      // The message is kept when the commit failed (a hook said no).
      if (!err) patch(key, { message: "" });
      return err;
    },

    push: async (context, publish = false) => {
      // Another action still running: nothing is pushed (act says null).
      if (get().byKey[gitKey(context)]?.busy) return null;
      const err = await act(context, "push", () =>
        invoke("git_push", { context, publish }, { timeoutMs: NETWORK_TIMEOUT_MS }),
      );
      // CI runs again on what was pushed: the rail's checks badge follows.
      if (!err && context.type === "workspace") void checksAfterPush(context.workspaceId);
      return err;
    },
    pull: (context) => act(context, "pull", () => invoke("git_pull", { context }, { timeoutMs: NETWORK_TIMEOUT_MS })),

    loadBranches: async (context) => {
      const key = ensure(context);
      try {
        const branches = await invoke<Branch[]>("git_branches", { context });
        patch(key, { branches });
      } catch {
        patch(key, { branches: [] });
      }
    },

    switchTo: (context, branch) =>
      act(context, "switch", () => invoke("git_switch", { context, branch }, { timeoutMs: LOCAL_TIMEOUT_MS })),
    createBranch: (context, name) =>
      act(context, "branch", () => invoke("git_create_branch", { context, name }, { timeoutMs: LOCAL_TIMEOUT_MS })),
  };
});

let following = false;

/** Reads the loaded statuses again when an agent finishes a turn and when
 * the window comes back into focus. */
export function followGit(): void {
  if (following) return;
  following = true;
  let done = new Set(
    Object.values(useStatus.getState().records)
      .filter((r) => r.status === "done")
      .map((r) => r.key),
  );
  useStatus.subscribe((s) => {
    const now = new Set(
      Object.values(s.records)
        .filter((r) => r.status === "done")
        .map((r) => r.key),
    );
    const finished = [...now].some((k) => !done.has(k));
    done = now;
    if (finished) useGit.getState().refreshAll();
  });
  window.addEventListener("focus", () => useGit.getState().refreshAll());
}

/** Shows a context's git state: reads it now, and again on the triggers
 * above while mounted. `null` shows nothing. */
export function useGitContext(context: FileContext | null): GitView | undefined {
  const key = context ? gitKey(context) : null;
  const json = context ? JSON.stringify(context) : null;
  useEffect(() => {
    if (!key || !json) return;
    shown.set(key, (shown.get(key) ?? 0) + 1);
    void useGit.getState().refresh(JSON.parse(json) as FileContext);
    return () => {
      const n = (shown.get(key) ?? 1) - 1;
      if (n > 0) shown.set(key, n);
      else shown.delete(key);
    };
  }, [key, json]);
  return useGit((s) => (key ? s.byKey[key] : undefined));
}

/** Tests: forget everything. */
export function resetGit(): void {
  reading.clear();
  shown.clear();
  useGit.setState({ byKey: {} });
}
