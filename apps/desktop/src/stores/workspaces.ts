import { create } from "zustand";
import type { AddedWorkspace, IpcError, Tab, WorkspaceSummary } from "@tandem/protocol";
import { invoke } from "../ipc/invoke";

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
  remove: (id: string) => Promise<void>;
  select: (id: string) => void;
  selectTab: (workspaceId: string, tabId: string) => void;
  createTab: (workspaceId: string) => Promise<Tab | null>;
  closeTab: (workspaceId: string, tabId: string) => Promise<void>;
};

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
  closeTab: async (workspaceId, tabId) => {
    await invoke("tab_close", { id: tabId });
    const ws = get().list.find((w) => w.id === workspaceId);
    if (ws && ws.tabs.length <= 1) {
      // Every workspace keeps at least one tab (F-CD-01).
      await invoke<Tab>("tab_create", { workspaceId });
    }
    await get().refresh();
  },
}));
