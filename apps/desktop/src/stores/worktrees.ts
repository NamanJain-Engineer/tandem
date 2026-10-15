import { create } from "zustand";
import type {
  FileDiff,
  Forge,
  IpcError,
  LaneSetup,
  MergeReport,
  OverlapReport,
  PaneRecord,
  PortRange,
  PreviewState,
  RemoveReport,
  SetupState,
  Tab,
  TeardownOutcome,
  Workspace,
  WorkspaceSummary,
  WorktreeSetup,
} from "@tandem/protocol";
import type { ToastInput } from "@tandem/ui";
import { t } from "../i18n";
import { invoke } from "../ipc/invoke";
import { useLayout } from "./layout";
import { useSidebar } from "./sidebar";
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
  /** UI-151: untrusted setup commands for new worktrees, reviewed once. */
  | { kind: "review"; worktreeIds: string[]; state: SetupState; after?: "preview" }
  /** UI-154: two worktrees' previews side by side (F-WT-09). */
  | { kind: "comparePreviews"; candidates: string[]; chosen: string[]; onDone?: () => void }
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
  /** F-WT-06: each worktree's port block, once read (null: none). */
  runtimes: Record<string, PortRange | null>;
  /** F-WT-09: each local worktree's preview, polled while the rail shows. */
  previews: Record<string, PreviewState>;
};

/** Which worktree dialog is open (rail rows and pane menus open them). */
export const useWorktrees = create<WorktreesState>((set, get) => ({
  ask: null,
  open: (ask) => set({ ask }),
  notices: [],
  forges: {},
  runtimes: {},
  previews: {},
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
  cmd: "lane_create_pr" | "lane_setup_run" | "lane_preview_start",
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
export async function trustAndRun(
  worktreeIds: string[],
  digest: string,
  after?: "preview",
): Promise<Outcome<SetupState>> {
  const first = worktreeIds[0];
  if (!first) return { ok: false, error: noTab };
  const out = await run<SetupState>("worktree_setup_trust", { workspaceId: first, digest });
  if (!out.ok) return out;
  const config = out.value.config;
  // F-WT-09: the review came from Start preview, or the file has a preview
  // and no setup to start it after.
  if (after === "preview" || (!config?.setup && config?.preview)) {
    for (const id of worktreeIds) await startPreview(id);
    return out;
  }
  // A file with only `env` (F-WT-06) has nothing to run.
  if (!config?.setup) return out;
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
      body:
        [
          // F-WT-09: files the filesystem cloned rather than copied.
          copy.cloned > 0 ? t("worktree.setup.cloned", { n: copy.cloned }) : null,
          copy.partial ? t("worktree.setup.partial") : null,
        ]
          .filter((x): x is string => x !== null)
          .join(" ") || undefined,
    });
  if (state.error) {
    notify({ tone: "error", title: t("worktree.setup.invalid", { reason: state.error }) });
    return;
  }
  // F-WT-06: a port block that moved past busy ports says so.
  const runtime = out.value.runtime;
  if (runtime?.movedFrom && runtime.ports) {
    const branch = useWorkspaces.getState().list.find((w) => w.id === laneId)?.laneBranch ?? "";
    notify({
      tone: "warning",
      title: t("worktree.runtime.moved", {
        from: `${runtime.movedFrom.start}–${runtime.movedFrom.end}`,
        to: `${runtime.ports.start}–${runtime.ports.end}`,
        branch,
      }),
    });
  }
  const hasEnv = Object.keys(state.config?.env ?? {}).length > 0;
  const hasPreview = Boolean(state.config?.preview);
  if (!state.config?.setup && !hasEnv && !hasPreview) return;
  if (state.trusted) {
    // F-WT-09: without a setup to chain it after, the preview starts alone.
    if (!state.config?.setup) {
      if (hasPreview) await startPreview(laneId);
      return;
    }
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

// ---- Worktree runtime (F-WT-06) --------------------------------------------

/** `NAME=value` lines to a map; the first bad line is reported (1-based). */
export function parseEnv(text: string): { env: Record<string, string>; badLine: number | null } {
  const env: Record<string, string> = {};
  const lines = text.split("\n");
  for (let i = 0; i < lines.length; i++) {
    const line = (lines[i] ?? "").trim();
    if (!line || line.startsWith("#")) continue;
    const eq = line.indexOf("=");
    if (eq <= 0) return { env, badLine: i + 1 };
    env[line.slice(0, eq).trim()] = line.slice(eq + 1).trim();
  }
  return { env, badLine: null };
}

export function formatEnv(env: Record<string, string> | undefined): string {
  return Object.entries(env ?? {})
    .map(([k, v]) => `${k}=${v}`)
    .join("\n");
}

/** The names whose values look like secrets (the core's redactor). */
export async function secretLooking(env: Record<string, string>): Promise<string[]> {
  if (Object.keys(env).length === 0) return [];
  const out = await run<string[] | null>("worktree_env_check", { env });
  return out.ok ? (out.value ?? []) : [];
}

/** The worktree's port block, once read (the rail row's tooltip). */
export async function loadRuntime(laneId: string): Promise<void> {
  if (laneId in useWorktrees.getState().runtimes) return;
  const out = await run<PortRange | null>("lane_runtime", { laneId });
  if (out.ok) useWorktrees.setState((s) => ({ runtimes: { ...s.runtimes, [laneId]: out.value ?? null } }));
}

// ---- Previews (F-WT-09) -----------------------------------------------------

/** How often the rail asks whether previews answer. */
export const PREVIEW_POLL_MS = 2_000;

export async function pollPreviews(laneIds: string[]): Promise<void> {
  if (laneIds.length === 0) return;
  const out = await run<PreviewState[] | null>("lane_previews", { laneIds });
  if (!out.ok || !out.value) return;
  const next = { ...useWorktrees.getState().previews };
  for (const p of out.value) next[p.laneId] = p;
  useWorktrees.setState({ previews: next });
}

/** Opens `url` in a new sidebar browser tab. */
export function openPreview(url: string): void {
  const sidebar = useSidebar.getState();
  const id = sidebar.openTab("browser");
  sidebar.setPage(id, null, url);
}

/** "Start preview": a Preview pane in the worktree running the trusted
 * command. An untrusted one opens the review first. */
export async function startPreview(laneId: string): Promise<Outcome<PaneRecord>> {
  const wt = useWorkspaces.getState().list.find((w) => w.id === laneId);
  if (!wt) return { ok: false, error: { ...noTab, message: "That worktree is gone" } };
  const out = await shellInWorktree(wt, "lane_preview_start");
  if (!out.ok && out.error.code === "permission_denied") {
    const state = await getWorktreeSetup(laneId);
    if (state.ok)
      useWorktrees
        .getState()
        .open({ kind: "review", worktreeIds: [laneId], state: state.value, after: "preview" });
  } else if (!out.ok) {
    useWorktrees
      .getState()
      .notify({ tone: "error", title: t("worktree.preview.startFailed", { reason: out.error.message }) });
  }
  return out;
}

/** How long side by side waits for both previews to answer. */
export const COMPARE_WAIT_MS = 60_000;

const failed = (message: string): Outcome<never> => ({
  ok: false,
  error: { code: "not_found", message, stage: null, hint: null, retryable: false },
});

/** A pane name (1–32 characters) for `branch`, `n` > 1 when it's taken. */
export function paneLabel(branch: string, n = 1): string {
  const suffix = n > 1 ? ` ${n}` : "";
  const room = 32 - suffix.length;
  const base = branch.length > room ? `${branch.slice(0, room - 1)}…` : branch;
  return base + suffix;
}

async function namePane(paneId: string, branch: string): Promise<string | null> {
  for (let n = 1; n <= 5; n += 1) {
    const out = await run<string>("pane_rename", { paneId, humanName: paneLabel(branch, n) });
    if (out.ok) return out.value;
    if (out.error.code !== "conflict") return null;
  }
  return null;
}

/** F-WT-09 side by side: starts whichever of the two previews isn't
 * running, waits for both to answer, then opens a tab in the repository
 * with a browser pane per preview, each named after its branch. */
export async function comparePreviews(
  laneIds: [string, string],
  wait = COMPARE_WAIT_MS,
): Promise<Outcome<Tab>> {
  const list = useWorkspaces.getState().list;
  const lanes = laneIds.map((id) => list.find((w) => w.id === id));
  const [a, b] = lanes;
  if (!a || !b) return failed(t("worktree.compare.gone"));
  const branch = (w: WorkspaceSummary) => w.laneBranch ?? w.name;
  await pollPreviews(laneIds);
  for (const w of [a, b]) {
    const p = useWorktrees.getState().previews[w.id];
    if (!p || p.status === "none") return failed(t("worktree.compare.none", { branch: branch(w) }));
    if (p.status === "down") {
      const out = await startPreview(w.id);
      if (!out.ok) return out;
    }
  }
  const ready = () => laneIds.every((id) => useWorktrees.getState().previews[id]?.status === "ready");
  const deadline = Date.now() + wait;
  while (!ready()) {
    if (Date.now() >= deadline) return failed(t("worktree.compare.timeout"));
    await new Promise((r) => setTimeout(r, PREVIEW_POLL_MS));
    await pollPreviews(laneIds);
  }
  const parentId = a.parentId ?? a.id;
  const tab = await run<Tab>("tab_create", { workspaceId: parentId });
  if (!tab.ok) return tab;
  // The panes go into the tab before the UI shows it, so it never starts
  // a shell of its own.
  let left: string | null = null;
  for (const w of [a, b]) {
    const url = useWorktrees.getState().previews[w.id]?.url ?? null;
    const pane = await run<PaneRecord>("pane_create", { tabId: tab.value.id, kind: "browser", url });
    if (!pane.ok) return pane;
    const name = await namePane(pane.value.id, branch(w));
    useLayout
      .getState()
      .insert(tab.value.id, { ...pane.value, humanName: name ?? pane.value.humanName }, left, "right");
    left = pane.value.id;
  }
  const workspaces = useWorkspaces.getState();
  const title = `${branch(a)} | ${branch(b)}`;
  await workspaces.renameTab(parentId, tab.value.id, title.length > 80 ? `${title.slice(0, 79)}…` : title);
  workspaces.select(parentId);
  workspaces.selectTab(parentId, tab.value.id);
  return tab;
}
