import { create } from "zustand";
import type {
  FileDiff,
  Forge,
  IpcError,
  LaneSetup,
  MergeReport,
  OverlapReport,
  PaneRecord,
  RemoveReport,
  SetupState,
  TeardownOutcome,
  Workspace,
  WorkspaceSummary,
  WorktreeSetup,
} from "@tandem/protocol";
import type { ToastInput } from "@tandem/ui";
import { t } from "../i18n";
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
  /** UI-150: the repository's worktree setup (F-WT-01). */
  | { kind: "setup"; workspaceId: string }
  /** UI-153: shared files with sibling worktrees and the merge forecast (F-WT-03). */
  | { kind: "overlap"; worktreeId: string }
  /** F-WT-05: another engine's read-only review of the diff. */
  | { kind: "opinion"; worktreeId: string }
  /** UI-151: untrusted setup commands for new worktrees, reviewed once. */
  | { kind: "review"; worktreeIds: string[]; state: SetupState }
  | null;

type WorktreesState = {
  ask: WorktreeAsk;
  open: (ask: WorktreeAsk) => void;
  /** Toasts raised outside React (worktree setup); WorktreeDialogs shows them. */
  notices: ToastInput[];
  notify: (n: ToastInput) => void;
  takeNotices: () => ToastInput[];
  /** F-WT-04: where each worktree's `origin` is hosted, once read. */
  forges: Record<string, Forge>;
};

/** Which worktree dialog is open (rail rows and pane menus open them). */
export const useWorktrees = create<WorktreesState>((set, get) => ({
  ask: null,
  open: (ask) => set({ ask }),
  notices: [],
  forges: {},
  notify: (n) => set({ notices: [...get().notices, n] }),
  takeNotices: () => {
    const out = get().notices;
    if (out.length) set({ notices: [] });
    return out;
  },
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

/** F-WT-03: reads only; the forecast merges in memory. */
export function worktreeOverlap(laneId: string): Promise<Outcome<OverlapReport>> {
  return run<OverlapReport>("lane_overlap", { laneId });
}

export function mergeWorktree(laneId: string): Promise<Outcome<MergeReport>> {
  return run<MergeReport>("lane_merge", { laneId });
}

/** Removes a worktree. Uncommitted work refuses without `force`; with it,
 * the core asks once more natively. The report says what the repository's
 * teardown did (F-WT-01). */
export async function removeWorktree(laneId: string, force: boolean): Promise<Outcome<RemoveReport | null>> {
  const out = await run<RemoveReport | null>("lane_remove", { laneId, force });
  if (out.ok) await useWorkspaces.getState().refresh();
  return out;
}

/** The toast line for a teardown's outcome (F-WT-01). */
export function teardownText(outcome: TeardownOutcome | null | undefined): string | undefined {
  if (!outcome) return undefined;
  switch (outcome.kind) {
    case "ran":
      return outcome.code === 0
        ? t("worktree.setup.teardownRan")
        : t("worktree.setup.teardownCode", { code: String(outcome.code ?? "?") });
    case "timed_out":
      return t("worktree.setup.teardownTimedOut");
    case "untrusted":
      return t("worktree.setup.teardownUntrusted");
    case "failed":
      return t("worktree.setup.teardownFailed", { reason: outcome.message });
  }
}

const noTab: IpcError = {
  code: "not_found",
  message: "This worktree has no tab",
  stage: null,
  hint: null,
  retryable: false,
};

/** A shell pane `cmd` opens in the worktree, placed beside its active pane. */
async function shellInWorktree(
  wt: WorkspaceSummary,
  cmd: "lane_create_pr" | "lane_setup_run",
): Promise<Outcome<PaneRecord>> {
  const ws = useWorkspaces.getState();
  const tabId = ws.activeTab[wt.id] ?? wt.tabs[0]?.id;
  if (!tabId) return { ok: false, error: noTab };
  const out = await run<PaneRecord>(cmd, { laneId: wt.id, tabId });
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

/** A shell running `gh pr create` in the worktree, beside its active pane. */
export function createPullRequest(wt: WorkspaceSummary): Promise<Outcome<PaneRecord>> {
  return shellInWorktree(wt, "lane_create_pr");
}

// ---- Worktree setup (F-WT-01) ----------------------------------------------

export function getWorktreeSetup(workspaceId: string): Promise<Outcome<SetupState>> {
  return run<SetupState>("worktree_setup_get", { workspaceId });
}

export function saveWorktreeSetup(workspaceId: string, config: WorktreeSetup): Promise<Outcome<SetupState>> {
  return run<SetupState>("worktree_setup_save", { workspaceId, config });
}

/** Patterns typed one per line, blank lines dropped. */
export function patternLines(text: string): string[] {
  return text
    .split("\n")
    .map((l) => l.trim())
    .filter((l) => l.length > 0);
}

/** The trusted setup command in a shell pane in the worktree. */
export async function runWorktreeSetup(laneId: string): Promise<Outcome<PaneRecord>> {
  const wt = useWorkspaces.getState().list.find((w) => w.id === laneId);
  if (!wt) return { ok: false, error: { ...noTab, message: "That worktree is gone" } };
  return shellInWorktree(wt, "lane_setup_run");
}

/** Trusts the reviewed commands (the digest the person saw), then runs the
 * setup in each worktree waiting for it. */
export async function trustAndRun(worktreeIds: string[], digest: string): Promise<Outcome<SetupState>> {
  const first = worktreeIds[0];
  if (!first) return { ok: false, error: noTab };
  const out = await run<SetupState>("worktree_setup_trust", { workspaceId: first, digest });
  if (!out.ok) return out;
  for (const id of worktreeIds) {
    const ran = await runWorktreeSetup(id);
    if (!ran.ok)
      useWorktrees.getState().notify({
        tone: "error",
        title: t("worktree.setup.runFailed", { reason: ran.error.message }),
      });
  }
  return out;
}

/** After a worktree is made from the New worktree dialog or the launch
 * sheet: report the copy, then run a trusted setup or ask for a review.
 * Several new worktrees with the same untrusted setup share one review. */
export async function prepareWorktree(laneId: string): Promise<void> {
  const out = await run<LaneSetup | null>("lane_setup", { laneId });
  const { notify, open } = useWorktrees.getState();
  if (!out.ok) {
    notify({ tone: "error", title: t("worktree.setup.loadFailed", { reason: out.error.message }) });
    return;
  }
  if (!out.value) return;
  const { state, copy, copyError } = out.value;
  if (copyError) notify({ tone: "error", title: t("worktree.setup.copyFailed", { reason: copyError }) });
  else if (copy && copy.copied.length > 0)
    notify({
      tone: "success",
      title: t("worktree.setup.copied", { n: copy.copied.length }),
      body: copy.partial ? t("worktree.setup.partial") : undefined,
    });
  if (state.error) {
    notify({ tone: "error", title: t("worktree.setup.invalid", { reason: state.error }) });
    return;
  }
  if (!state.config?.setup) return;
  if (state.trusted) {
    const ran = await runWorktreeSetup(laneId);
    if (!ran.ok)
      notify({ tone: "error", title: t("worktree.setup.runFailed", { reason: ran.error.message }) });
    return;
  }
  const ask = useWorktrees.getState().ask;
  if (ask?.kind === "review" && ask.state.digest === state.digest)
    open({ ...ask, worktreeIds: [...ask.worktreeIds, laneId] });
  else open({ kind: "review", worktreeIds: [laneId], state });
}

// ---- GitLab merge requests (F-WT-04) ---------------------------------------

/** Reads (once) whether the worktree's `origin` is on GitHub or GitLab, so
 * its menu can say pull or merge request. */
export async function loadForge(laneId: string): Promise<void> {
  if (useWorktrees.getState().forges[laneId]) return;
  const out = await run<Forge | null>("lane_forge", { laneId });
  if (out.ok && out.value)
    useWorktrees.setState((s) => ({ forges: { ...s.forges, [laneId]: out.value as Forge } }));
}
