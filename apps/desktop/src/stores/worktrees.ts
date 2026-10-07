import { create } from "zustand";
import type { FileDiff, IpcError, MergeReport, PaneRecord, Workspace, WorkspaceSummary } from "@tandem/protocol";
import { invoke } from "../ipc/invoke";
import { useLayout } from "./layout";
import { useWorkspaces } from "./workspaces";

/** A worktree is a child workspace on a `tandem/<slug>` branch (F-CD-15).
 * Users see "worktree"; "lane" is the internal word (UI_SPEC §6.2). */
export function isWorktree(ws: Pick<WorkspaceSummary, "parentId" | "laneBranch">): boolean {
  return ws.parentId !== null && ws.laneBranch !== null;
}

/** Top-level workspaces, in rail order (worktrees nest under these). */
export function topLevel(list: WorkspaceSummary[]): WorkspaceSummary[] {
  return list.filter((w) => w.parentId === null);
}

export function worktreesOf(list: WorkspaceSummary[], parentId: string): WorkspaceSummary[] {
  return list.filter((w) => w.parentId === parentId && isWorktree(w));
}

/** The branch a worktree merges into: its repository's current branch. */
export function baseBranch(list: WorkspaceSummary[], wt: WorkspaceSummary): string {
  return list.find((w) => w.id === wt.parentId)?.branch ?? "HEAD";
}

export type WorktreeAsk =
  | { kind: "new"; workspaceId: string }
  | { kind: "diff" | "merge" | "discard"; worktreeId: string }
  | null;

type WorktreesState = {
  ask: WorktreeAsk;
  open: (ask: WorktreeAsk) => void;
};

/** Which worktree dialog is open (rail rows and pane menus open them). */
export const useWorktrees = create<WorktreesState>((set) => ({
  ask: null,
  open: (ask) => set({ ask }),
}));

export type Outcome<T> = { ok: true; value: T } | { ok: false; error: IpcError };

async function run<T>(cmd: string, args: Record<string, unknown>): Promise<Outcome<T>> {
  try {
    return { ok: true, value: await invoke<T>(cmd, args) };
  } catch (e) {
    return { ok: false, error: e as IpcError };
  }
}

/** `lane_create`, then the new worktree is selected in the rail. */
export async function createWorktree(workspaceId: string, slugSource: string): Promise<Outcome<Workspace>> {
  const out = await run<Workspace>("lane_create", { workspaceId, slugSource });
  if (out.ok) {
    const ws = useWorkspaces.getState();
    await ws.refresh();
    ws.setCollapsed(out.value.parentId ?? workspaceId, false);
    ws.select(out.value.id);
  }
  return out;
}

export function worktreeDiff(laneId: string): Promise<Outcome<FileDiff[]>> {
  return run<FileDiff[]>("lane_diff", { laneId });
}

export function mergeWorktree(laneId: string): Promise<Outcome<MergeReport>> {
  return run<MergeReport>("lane_merge", { laneId });
}

/** Removes a worktree. Uncommitted work refuses without `force`; with it,
 * the core asks once more natively. */
export async function removeWorktree(laneId: string, force: boolean): Promise<Outcome<null>> {
  const out = await run<null>("lane_remove", { laneId, force });
  if (out.ok) await useWorkspaces.getState().refresh();
  return out;
}

/** A shell running `gh pr create` in the worktree, beside its active pane. */
export async function createPullRequest(wt: WorkspaceSummary): Promise<Outcome<PaneRecord>> {
  const ws = useWorkspaces.getState();
  const tabId = ws.activeTab[wt.id] ?? wt.tabs[0]?.id;
  if (!tabId)
    return {
      ok: false,
      error: { code: "not_found", message: "This worktree has no tab", stage: null, hint: null, retryable: false },
    };
  const out = await run<PaneRecord>("lane_create_pr", { laneId: wt.id, tabId });
  if (out.ok) {
    ws.select(wt.id);
    ws.selectTab(wt.id, tabId);
    const layout = useLayout.getState();
    if (!layout.loaded[wt.id]) await layout.load(wt.id).catch(() => undefined);
    const tab = useLayout.getState().tabs[tabId];
    if (!tab?.panes[out.value.id]) useLayout.getState().insert(tabId, out.value, null, "right");
  }
  return out;
}
