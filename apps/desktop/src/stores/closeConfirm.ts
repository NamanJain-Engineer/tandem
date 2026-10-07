import { create } from "zustand";
import { useEngines } from "./engines";
import { useLayout } from "./layout";
import { paneKey, useStatus } from "./status";

type Pending = { tabId: string; paneId: string; name: string };

type CloseConfirmState = {
  pending: Pending | null;
  /** Closes a pane; a working agent asks first (F-CD-12 AC2). */
  request: (tabId: string, paneId: string) => void;
  confirm: () => void;
  cancel: () => void;
};

export const useCloseConfirm = create<CloseConfirmState>((set, get) => ({
  pending: null,
  request: (tabId, paneId) => {
    const pane = useLayout.getState().tabs[tabId]?.panes[paneId];
    const working = useStatus.getState().records[paneKey(paneId)]?.status === "working";
    if (pane?.kind === "agent" && working) {
      const name = pane.humanName ?? useEngines.getState().label(pane.engineId) ?? "";
      set({ pending: { tabId, paneId, name } });
      return;
    }
    void useLayout.getState().close(tabId, paneId);
  },
  confirm: () => {
    const p = get().pending;
    set({ pending: null });
    if (p) void useLayout.getState().close(p.tabId, p.paneId);
  },
  cancel: () => set({ pending: null }),
}));
