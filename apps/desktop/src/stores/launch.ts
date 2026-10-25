import { create } from "zustand";
import type {
  EngineView,
  IpcError,
  Isolation,
  LaunchReport,
  LaunchedSeat,
  Preset,
  Seat,
  WorkspaceSummary,
} from "@tandem/protocol";
import { invoke } from "../ipc/invoke";
import { useLayout } from "./layout";
import { useWorkspaces } from "./workspaces";
import { prepareWorktree } from "./worktrees";

/** Role suggestions in the lineup (UI_SPEC UI-12); any text works. */
export const ROLES = ["Builder", "Reviewer", "Tester", "Planner", "Explorer", "Lead"] as const;
export const MAX_COUNT = 8;

/** The engine a new agent seat starts with: Claude Code when it's installed,
 * else the first installed engine. */
export function defaultEngine(engines: EngineView[]): string | null {
  const installed = engines.filter((e) => e.install !== null && e.unsupportedReason === null);
  return (installed.find((e) => e.spec.id === "claude") ?? installed[0])?.spec.id ?? null;
}

export function agentSeat(engineId: string | null, role = "Builder"): Seat {
  return { kind: "agent", engineId, accountId: null, role, task: null, command: null, url: null, count: 1 };
}

export function shellSeat(command: string | null = null): Seat {
  return {
    kind: "shell",
    engineId: null,
    accountId: null,
    role: "Tests",
    task: null,
    command,
    url: null,
    count: 1,
  };
}

/** A browser seat (F-CD-14): its page opens tiled with the terminals. */
export function browserSeat(url: string | null = null): Seat {
  return {
    kind: "browser",
    engineId: null,
    accountId: null,
    role: null,
    task: null,
    command: null,
    url,
    count: 1,
  };
}

/** A preset's rows, with "the default engine" resolved. */
export function lineupFrom(preset: Preset, engine: string | null): Seat[] {
  return preset.seats.map((s) => (s.kind === "agent" ? { ...s, engineId: s.engineId ?? engine } : { ...s }));
}

export function counts(seats: Seat[]): { agents: number; shells: number; browsers: number } {
  let agents = 0;
  let shells = 0;
  let browsers = 0;
  for (const s of seats) {
    if (s.kind === "agent") agents += s.count;
    if (s.kind === "shell") shells += s.count;
    if (s.kind === "browser") browsers += s.count;
  }
  return { agents, shells, browsers };
}

/** kebab-case from the prompt's first words, as the core names branches
 * (F-CD-15): up to 40 characters, "session" when empty. */
export function slugPreview(text: string): string {
  const slug = text
    .toLowerCase()
    .normalize("NFKD")
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 40)
    .replace(/-+$/g, "");
  return slug || "session";
}

/** The branches "Worktree per agent" will create, in seat order: each
 * agent's slug comes from its task, the shared task or its role, and
 * repeats get -2, -3, … like the core (F-CD-15 AC1). */
export function branchPreview(task: string, seats: Seat[]): string[] {
  const taken = new Map<string, number>();
  const out: string[] = [];
  for (const s of seats) {
    if (s.kind !== "agent") continue;
    const slug = slugPreview(s.task?.trim() || task.trim() || s.role?.trim() || "");
    for (let i = 0; i < s.count; i++) {
      const n = (taken.get(slug) ?? 0) + 1;
      taken.set(slug, n);
      out.push(n === 1 ? `tandem/${slug}` : `tandem/${slug}-${n}`);
    }
  }
  return out;
}

export function isRepo(ws: WorkspaceSummary | undefined): boolean {
  return Boolean(ws?.branch) || Boolean(ws?.parentId) || isMultiRepo(ws);
}

/** F-CD-22: a folder holding several repositories (not one of its worktrees). */
export function isMultiRepo(ws: WorkspaceSummary | undefined): boolean {
  return !ws?.parentId && (ws?.repos?.length ?? 0) >= 2;
}

export type LaunchState = {
  /** The workspace the sheet launches into; null when closed. */
  workspaceId: string | null;
  presets: Preset[];
  loaded: boolean;
  open: (workspaceId: string | null) => void;
  close: () => void;
  loadPresets: () => Promise<void>;
};

export const useLaunch = create<LaunchState>((set) => ({
  workspaceId: null,
  presets: [],
  loaded: false,
  open: (workspaceId) => {
    const id = workspaceId ?? useWorkspaces.getState().activeId;
    if (id) set({ workspaceId: id });
  },
  close: () => set({ workspaceId: null }),
  loadPresets: async () => {
    const presets = await invoke<Preset[]>("presets_list", {}).catch(() => [] as Preset[]);
    set({ presets: presets ?? [], loaded: true });
  },
}));

export type Outcome<T> = { ok: true; value: T } | { ok: false; error: IpcError };

/** Puts launched panes into their tabs, tiles each tab, and shows the
 * lineup: the shared tab, else the first worktree. */
export async function place(launched: LaunchedSeat[], focusTab: string | null): Promise<void> {
  const ws = useWorkspaces.getState();
  await ws.refresh();
  const layout = useLayout.getState;
  const tabs = new Set<string>();
  for (const l of launched) {
    if (!layout().loaded[l.workspaceId])
      await layout()
        .load(l.workspaceId)
        .catch(() => undefined);
    if (!layout().tabs[l.tabId]?.panes[l.pane.id]) layout().insert(l.tabId, l.pane, null, "right");
    tabs.add(l.tabId);
  }
  for (const tabId of tabs) layout().arrange(tabId);
  const first = launched.find((l) => l.tabId === focusTab) ?? launched[0];
  if (!first) return;
  const parent = useWorkspaces.getState().list.find((w) => w.id === first.workspaceId)?.parentId;
  if (parent) useWorkspaces.getState().setCollapsed(parent, false);
  useWorkspaces.getState().select(first.workspaceId);
  useWorkspaces.getState().selectTab(first.workspaceId, first.tabId);
}

export async function launchLineup(
  workspaceId: string,
  seats: Seat[],
  task: string,
  isolation: Isolation,
): Promise<Outcome<LaunchReport>> {
  try {
    const report = await invoke<LaunchReport>(
      "launch_lineup",
      { workspaceId, seats, task: task.trim() || null, isolation },
      { timeoutMs: 120_000 },
    );
    await place(report.launched, report.sharedTabId);
    // F-WT-01: each new worktree's copy report, and its setup or a review.
    for (const seat of report.launched) if (seat.laneBranch) void prepareWorktree(seat.workspaceId);
    return { ok: true, value: report };
  } catch (e) {
    return { ok: false, error: e as IpcError };
  }
}

export async function retrySeat(
  workspaceId: string,
  seat: Seat,
  task: string,
  isolation: Isolation,
  tabId: string | null,
): Promise<Outcome<LaunchedSeat>> {
  try {
    const launched = await invoke<LaunchedSeat>("launch_retry_seat", {
      workspaceId,
      seat,
      task: task.trim() || null,
      isolation,
      tabId,
    });
    await place([launched], tabId);
    if (launched.laneBranch) void prepareWorktree(launched.workspaceId);
    return { ok: true, value: launched };
  } catch (e) {
    return { ok: false, error: e as IpcError };
  }
}

export async function savePreset(
  name: string,
  seats: Seat[],
  isolation: Isolation,
): Promise<Outcome<Preset>> {
  try {
    const preset = await invoke<Preset>("preset_save", { id: null, name, seats, isolation });
    await useLaunch.getState().loadPresets();
    return { ok: true, value: preset };
  } catch (e) {
    return { ok: false, error: e as IpcError };
  }
}
