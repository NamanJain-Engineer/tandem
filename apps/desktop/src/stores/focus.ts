import { create } from "zustand";
import type { SourceRef } from "@tandem/protocol";
import { listen } from "../ipc/events";
import { invoke } from "../ipc/invoke";
import { switchMode } from "../features/shell/actions";
import { useLayout } from "./layout";
import { useThreads } from "./threads";
import { useWorkspaces } from "./workspaces";

/** How long a focused pane shows its accent ring (F-CD-12 AC3). */
export const FLASH_MS = 1_200;

type FlashState = {
  /** Pane id → when its flash started. */
  flashing: Record<string, number>;
  flash: (paneId: string) => void;
};

const timers = new Map<string, ReturnType<typeof setTimeout>>();

export const useFlash = create<FlashState>((set, get) => ({
  flashing: {},
  flash: (paneId) => {
    clearTimeout(timers.get(paneId));
    set({ flashing: { ...get().flashing, [paneId]: Date.now() } });
    timers.set(
      paneId,
      setTimeout(() => {
        const next = { ...get().flashing };
        delete next[paneId];
        set({ flashing: next });
        timers.delete(paneId);
      }, FLASH_MS),
    );
  },
}));

export type FocusLocation = { workspaceId: string; tabId: string };

/** Shows a source and flashes it: the workspace and tab, Code mode, the
 * pane focused (from a Dashboard card, a notification, an OS notification). */
export async function showSource(source: SourceRef, location?: FocusLocation | null): Promise<void> {
  if (source.type === "thread") {
    // A thread card or notification opens the thread in Thread mode.
    switchMode("thread");
    useThreads.getState().select(source.threadId);
    return;
  }
  if (source.type !== "pane") return;
  const paneId = source.paneId;
  const layout = useLayout.getState();
  const known = Object.entries(layout.tabs).find(([, tab]) => tab.panes[paneId]);
  const tabId = known?.[0] ?? location?.tabId;
  const workspaceId = known?.[1].panes[paneId]?.workspaceId ?? location?.workspaceId;
  if (!tabId || !workspaceId) return;
  const ws = useWorkspaces.getState();
  ws.select(workspaceId);
  ws.selectTab(workspaceId, tabId);
  switchMode("code");
  if (!layout.loaded[workspaceId]) await layout.load(workspaceId).catch(() => undefined);
  useLayout.getState().focus(tabId, paneId);
  useFlash.getState().flash(paneId);
}

/** Asks the core to focus a source; it brings the window forward and
 * answers with `ui:focus`. */
export async function focusSource(source: SourceRef): Promise<boolean> {
  try {
    await invoke("focus_source", { source });
    return true;
  } catch {
    return false;
  }
}

let following = false;

export function followFocus(): void {
  if (following) return;
  following = true;
  void listen<{ source: SourceRef; flash: boolean; location?: FocusLocation | null }>(
    "ui:focus",
    ({ source, location }) => {
      void showSource(source, location);
    },
  );
}
