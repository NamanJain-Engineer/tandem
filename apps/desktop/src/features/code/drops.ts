// File drops into terminals (F-CD-06, ARCHITECTURE §5.9). The OS drop arrives
// as one Tauri event with absolute paths and a position; the pane under that
// point gets the shell-quoted paths at its caret, never followed by Enter.
// Hovering a background tab for 600 ms brings it forward first.
import { useEffect } from "react";
import { create } from "zustand";
import type { IpcError } from "@tandem/protocol";
import { t } from "../../i18n";
import { inTauri, invoke, usingFakeBackend } from "../../ipc/invoke";
import { useWorkspaces } from "../../stores/workspaces";

export const TAB_HOVER_MS = 600;

type Toast = (t: { tone?: "info" | "success" | "warning" | "error"; title: string }) => void;

/** The pane a drag is over, for its drop highlight. */
export const useDropTarget = create<{ paneId: string | null }>(() => ({ paneId: null }));

/** The pane (or tab) at a point in CSS pixels. */
export function hitTest(
  x: number,
  y: number,
): { paneId: string | null; tabId: string | null; workspaceId: string | null } {
  const el = document.elementFromPoint(x, y);
  const pane = el?.closest<HTMLElement>('[data-testid="pane"]');
  const tab = el?.closest<HTMLElement>('[data-testid="tab"]');
  return {
    paneId: pane?.dataset.paneId ?? null,
    tabId: tab?.dataset.tabId ?? null,
    workspaceId: useWorkspaces.getState().activeId,
  };
}

/** Types `paths`, quoted for the pane's shell, at its caret. */
export async function dropPaths(paneId: string, paths: string[], toast: Toast): Promise<boolean> {
  if (paths.length === 0) return false;
  try {
    const quoted = await invoke<string>("term_quote_paths", { paneId, paths });
    await invoke("pty_write", { paneId, data: quoted });
    return true;
  } catch (e) {
    const err = e as IpcError;
    toast({ tone: "error", title: err.code === "unsafe_filename" ? t("drop.unsafe") : err.message });
    return false;
  }
}

/** Follows OS drag-and-drop over the window while Code mode is mounted. */
export function useFileDrops(toast: Toast): void {
  useEffect(() => {
    let hoverTab: { id: string; timer: ReturnType<typeof setTimeout> } | null = null;
    const clearHover = () => {
      if (hoverTab) clearTimeout(hoverTab.timer);
      hoverTab = null;
    };
    const over = (x: number, y: number) => {
      const hit = hitTest(x, y);
      useDropTarget.setState({ paneId: hit.paneId });
      if (hit.tabId !== hoverTab?.id) {
        clearHover();
        if (hit.tabId && hit.workspaceId) {
          const { tabId, workspaceId } = { tabId: hit.tabId, workspaceId: hit.workspaceId };
          hoverTab = {
            id: tabId,
            timer: setTimeout(() => useWorkspaces.getState().selectTab(workspaceId, tabId), TAB_HOVER_MS),
          };
        }
      }
    };
    const drop = async (paths: string[], x: number, y: number) => {
      clearHover();
      useDropTarget.setState({ paneId: null });
      const { paneId } = hitTest(x, y);
      if (paneId) await dropPaths(paneId, paths, toast);
    };
    // Debug builds: the E2E harness drives drops through the same path,
    // because WebDriver can't produce an OS file drop.
    const test = window as unknown as {
      __tandemDrop?: (paths: string[], x: number, y: number) => Promise<void>;
    };
    test.__tandemDrop = drop;
    let unlisten: (() => void) | undefined;
    let live = true;
    if (inTauri() && !usingFakeBackend()) {
      void import("@tauri-apps/api/webview").then(({ getCurrentWebview }) =>
        getCurrentWebview()
          .onDragDropEvent((event) => {
            const scale = window.devicePixelRatio || 1;
            const p = event.payload;
            if (p.type === "enter" || p.type === "over") over(p.position.x / scale, p.position.y / scale);
            else if (p.type === "drop") void drop(p.paths, p.position.x / scale, p.position.y / scale);
            else {
              clearHover();
              useDropTarget.setState({ paneId: null });
            }
          })
          .then((f) => {
            if (live) unlisten = f;
            else f();
          }),
      );
    }
    return () => {
      live = false;
      clearHover();
      unlisten?.();
      delete test.__tandemDrop;
    };
  }, [toast]);
}
