import { useEffect } from "react";
import { invoke } from "../../ipc/invoke";
import { useLayout } from "../../stores/layout";
import { useSidebar } from "../../stores/sidebar";
import { paneKey } from "../../stores/status";
import { useUi } from "../../stores/ui";
import { useWorkspaces } from "../../stores/workspaces";
import { leaves } from "../code/layout/tree";

/** The agents on screen: Code mode, Settings closed, the window shown, the
 * active tab's panes (just the maximized one when one is), not under a
 * full-screen sidebar. */
export function visibleKeys(): string[] {
  const ui = useUi.getState();
  if (ui.settingsOpen || ui.mode !== "code") return [];
  if (typeof document !== "undefined" && document.visibilityState === "hidden") return [];
  const sb = useSidebar.getState();
  if (sb.open && sb.fullscreen) return [];
  const ws = useWorkspaces.getState();
  const tabId = ws.activeId ? ws.activeTab[ws.activeId] : undefined;
  const tab = tabId ? useLayout.getState().tabs[tabId] : undefined;
  if (!tab?.tree) return [];
  return (tab.maximized ? [tab.maximized] : leaves(tab.tree)).map(paneKey);
}

/** Reports `visibleKeys()` to the core (`ui_visibility`) whenever it
 * changes, so it doesn't notify about what the user can see (F-DB-03). */
export function useVisibilityReport(): void {
  useEffect(() => {
    let last = "";
    let timer: ReturnType<typeof setTimeout> | undefined;
    const report = () => {
      clearTimeout(timer);
      timer = setTimeout(() => {
        const keys = visibleKeys();
        const sig = keys.join(",");
        if (sig === last) return;
        last = sig;
        invoke("ui_visibility", { visible: keys }).catch(() => undefined);
      }, 150);
    };
    const offs = [
      useUi.subscribe(report),
      useSidebar.subscribe(report),
      useWorkspaces.subscribe(report),
      useLayout.subscribe(report),
    ];
    document.addEventListener("visibilitychange", report);
    report();
    return () => {
      clearTimeout(timer);
      offs.forEach((off) => off());
      document.removeEventListener("visibilitychange", report);
    };
  }, []);
}
