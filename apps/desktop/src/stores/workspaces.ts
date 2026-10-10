import { create } from "zustand";
import type { AddedWorkspace, IpcError, Tab, WorkspaceSummary } from "@tandem/protocol";
import { invoke } from "../ipc/invoke";
import { useLayout } from "./layout";

export type AddResult =
  | { ok: true; added: AddedWorkspace }
  | { ok: false; limit: true; error: IpcError }
  | { ok: false; limit: false; error: IpcError };

type WorkspacesState = {
  list: WorkspaceSummary[];
  activeId: string | null;
  activeTab: Record<string, string>;
  refresh: () => Promise<void>;
  add: (root: string) => Promise<AddResult>;
  /** An SSH or WSL folder (F-RM-01, F-RM-02); the error, or null once it's active. */
  addRemote: (w: { kind: "ssh" | "wsl"; root: string; hostId?: string; wslDistro?: string }) => Promise<IpcError | null>;
  remove: (id: string) => Promise<void>;
  select: (id: string) => void;
  selectTab: (workspaceId: string, tabId: string) => void;
  createTab: (workspaceId: string) => Promise<Tab | null>;
  closeTab: (workspaceId: string, tabId: string) => Promise<void>;
  /** Reorders or pins a rail row; the list updates before the core answers. */
  move: (id: string, afterId: string | null, pinned: boolean) => Promise<void>;
  rename: (id: string, name: string) => Promise<IpcError | null>;
  setCollapsed: (id: string, collapsed: boolean) => void;
  /** "Show terminal sessions" off: panes collapse to one line (F-CD-11). */
  setSessionsCollapsed: (id: string, collapsed: boolean) => Promise<void>;
  locate: (id: string, root: string) => Promise<IpcError | null>;
  renameTab: (workspaceId: string, tabId: string, name: string) => Promise<IpcError | null>;
  reorderTabs: (workspaceId: string, ids: string[]) => Promise<void>;
  /** The workspace whose Remove confirmation is open (rail or main area). */
  confirmRemove: WorkspaceSummary | null;
  askRemove: (ws: WorkspaceSummary | null) => void;
};

/** `list` with `id` moved after `afterId` in its band (the order `workspace_list` returns). */
export function reordered(
  list: WorkspaceSummary[],
  id: string,
  afterId: string | null,
  pinned: boolean,
): WorkspaceSummary[] {
  const item = list.find((w) => w.id === id);
  if (!item) return list;
  const rest = list.filter((w) => w.id !== id);
  const moved = { ...item, pinned };
  const band = rest.filter((w) => w.pinned === pinned);
  const after = band.find((w) => w.id === afterId);
  const first = band[0];
  // After its neighbour; else first in its band; an empty band starts after the pinned rows.
  const index = after
    ? rest.indexOf(after) + 1
    : first
      ? rest.indexOf(first)
      : rest.filter((w) => w.pinned).length;
  return [...rest.slice(0, index), moved, ...rest.slice(index)];
}

export const useWorkspaces = create<WorkspacesState>((set, get) => ({
  list: [],
  activeId: null,
  activeTab: {},
  refresh: async () => {
    const list = await invoke<WorkspaceSummary[]>("workspace_list");
    const { activeId, activeTab } = get();
    const stillThere = list.some((w) => w.id === activeId);
    const tabs = { ...activeTab };
    for (const w of list) {
      if (!tabs[w.id] || !w.tabs.some((t) => t.id === tabs[w.id])) {
        const first = w.tabs[0];
        if (first) tabs[w.id] = first.id;
        else delete tabs[w.id];
      }
    }
    set({ list, activeId: stillThere ? activeId : (list[0]?.id ?? null), activeTab: tabs });
  },
  add: async (root) => {
    try {
      const added = await invoke<AddedWorkspace>("workspace_add", { root });
      await get().refresh();
      set({ activeId: added.workspace.id });
      return { ok: true, added };
    } catch (e) {
      const error = e as IpcError;
      return { ok: false, limit: error.code === "limit_reached", error } as AddResult;
    }
  },
  addRemote: async (w) => {
    try {
      const added = await invoke<AddedWorkspace>(
        "workspace_add",
        { root: w.root, kind: w.kind, hostId: w.hostId ?? null, wslDistro: w.wslDistro ?? null },
        { timeoutMs: 60_000 },
      );
      await get().refresh();
      set({ activeId: added.workspace.id });
      return null;
    } catch (e) {
      return e as IpcError;
    }
  },
  remove: async (id) => {
    await invoke("workspace_remove", { id });
    await get().refresh();
  },
  select: (id) => set({ activeId: id }),
  selectTab: (workspaceId, tabId) => set({ activeTab: { ...get().activeTab, [workspaceId]: tabId } }),
  createTab: async (workspaceId) => {
    try {
      const tab = await invoke<Tab>("tab_create", { workspaceId });
      await get().refresh();
      get().selectTab(workspaceId, tab.id);
      return tab;
    } catch {
      return null;
    }
  },
  move: async (id, afterId, pinned) => {
    set({ list: reordered(get().list, id, afterId, pinned) });
    await invoke("rail_move", { mode: "code", itemId: id, afterId, pinned }).catch(() => undefined);
    await get().refresh();
  },
  rename: async (id, name) => {
    try {
      set({ list: get().list.map((w) => (w.id === id ? { ...w, name } : w)) });
      await invoke("workspace_rename", { id, name });
      return null;
    } catch (e) {
      return e as IpcError;
    } finally {
      await get().refresh();
    }
  },
  setSessionsCollapsed: async (id, sessionsCollapsed) => {
    set({ list: get().list.map((w) => (w.id === id ? { ...w, sessionsCollapsed } : w)) });
    await invoke("workspace_set_flags", { id, showSessionsCollapsed: sessionsCollapsed }).catch(
      () => undefined,
    );
  },
  setCollapsed: (id, collapsed) => {
    set({ list: get().list.map((w) => (w.id === id ? { ...w, collapsed } : w)) });
    invoke("workspace_set_flags", { id, collapsed }).catch(() => undefined);
  },
  locate: async (id, root) => {
    try {
      await invoke("workspace_locate", { id, newRoot: root });
      await get().refresh();
      return null;
    } catch (e) {
      return e as IpcError;
    }
  },
  renameTab: async (workspaceId, tabId, name) => {
    const patch = (w: WorkspaceSummary) =>
      w.id === workspaceId ? { ...w, tabs: w.tabs.map((x) => (x.id === tabId ? { ...x, name } : x)) } : w;
    set({ list: get().list.map(patch) });
    try {
      await invoke("tab_rename", { id: tabId, name });
      return null;
    } catch (e) {
      return e as IpcError;
    } finally {
      await get().refresh();
    }
  },
  reorderTabs: async (workspaceId, ids) => {
    const order = new Map(ids.map((id, i) => [id, i]));
    set({
      list: get().list.map((w) =>
        w.id === workspaceId
          ? { ...w, tabs: [...w.tabs].sort((a, b) => (order.get(a.id) ?? 0) - (order.get(b.id) ?? 0)) }
          : w,
      ),
    });
    await invoke("tab_reorder", { workspaceId, ids }).catch(() => undefined);
    await get().refresh();
  },
  confirmRemove: null,
  askRemove: (ws) => set({ confirmRemove: ws }),
  closeTab: async (workspaceId, tabId) => {
    await invoke("tab_close", { id: tabId });
    const ws = get().list.find((w) => w.id === workspaceId);
    if (ws && ws.tabs.length <= 1) {
      // Every workspace keeps at least one tab (F-CD-01); it shows the empty state.
      const tab = await invoke<Tab>("tab_create", { workspaceId });
      useLayout.getState().markEmpty(tab.id);
    }
    await get().refresh();
  },
}));
