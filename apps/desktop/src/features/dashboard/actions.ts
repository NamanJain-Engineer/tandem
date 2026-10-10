import type { DashboardItem, IpcError } from "@tandem/protocol";
import { invoke } from "../../ipc/invoke";
import { useDashboard } from "../../stores/dashboard";
import { focusSource } from "../../stores/focus";
import { useHandoff } from "../../stores/handoff";
import { useLayout } from "../../stores/layout";
import { FLOAT_BELOW, useSidebar } from "../../stores/sidebar";

/** Jump to a card's source (F-DB-01 AC4). Full view exits; a floating
 * sidebar steps aside unless pinned (UI-42). */
export async function jumpTo(item: DashboardItem): Promise<boolean> {
  const ok = await focusSource(item.source);
  const sb = useSidebar.getState();
  if (sb.fullscreen) sb.setFullscreen(false);
  else if (window.innerWidth < FLOAT_BELOW && !sb.pinned) sb.setOpen(false);
  return ok;
}

/** "Hand off to…" (UI-13) for the card's pane. */
export function handOff(item: DashboardItem): void {
  if (item.source.type !== "pane" || !item.workspaceId) return;
  useHandoff
    .getState()
    .open({
      paneId: item.source.paneId,
      workspaceId: item.workspaceId,
      kind: "agent",
      engineId: item.engineId,
    });
}

/** Ends the agent's process; the pane stays with its output. */
export async function stopAgent(item: DashboardItem): Promise<void> {
  if (item.source.type === "pane")
    await invoke("pty_kill", { paneId: item.source.paneId }).catch(() => undefined);
}

/** Closes the card's pane (its conversation is saved by the engine). */
export async function closeAgent(item: DashboardItem): Promise<void> {
  if (item.source.type !== "pane") return;
  const paneId = item.source.paneId;
  const layout = useLayout.getState();
  const tabId = Object.keys(layout.tabs).find((id) => layout.tabs[id]?.panes[paneId]);
  if (tabId) await layout.close(tabId, paneId);
  else await invoke("pane_close", { paneId }).catch(() => undefined);
  useDashboard.getState().forget([item.key]);
}

/** Close idle: the core re-checks each is idle and closes only those. */
export async function closeIdle(keys: string[]): Promise<string[]> {
  const res = await invoke<{ closed: string[] }>("dashboard_close_idle", { keys }).catch(() => ({
    closed: [] as string[],
  }));
  for (const key of res.closed) if (key.startsWith("pane:")) useLayout.getState().dropPane(key.slice(5));
  useDashboard.getState().forget(res.closed);
  return res.closed;
}

export async function answer(
  approvalId: string,
  allow: boolean,
  via: "dashboard" | "notification" = "dashboard",
): Promise<IpcError | null> {
  try {
    await invoke("approval_resolve", {
      approvalId,
      decision: allow ? { type: "allow", scope: "once" } : { type: "deny", reason: null },
      via,
    });
    return null;
  } catch (e) {
    return e as IpcError;
  }
}
