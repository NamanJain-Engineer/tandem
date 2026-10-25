import { useEffect, useState } from "react";
import { create } from "zustand";
import type { GovernorStatus, IpcError, PaneResources, PaneUsage } from "@tandem/protocol";
import { listen } from "../ipc/events";
import { invoke } from "../ipc/invoke";

/** Usage refreshes this often while something shows it (F-EX-06). */
export const USAGE_MS = 2_000;

/** The resource governor (F-EX-06): what this system can do, and each pane's chip. */
type ResourcesState = {
  status: GovernorStatus | null;
  statusState: "idle" | "loading" | "ready" | "error";
  byPane: Record<string, PaneResources>;
  loadStatus: () => Promise<void>;
};

export const useResources = create<ResourcesState>((set) => ({
  status: null,
  statusState: "idle",
  byPane: {},
  loadStatus: async () => {
    set({ statusState: "loading" });
    try {
      const status = await invoke<GovernorStatus>("resources_status", {}, { timeoutMs: 30_000 });
      set({ status, statusState: "ready" });
    } catch {
      set({ statusState: "error" });
    }
  },
}));

function isClear(n: PaneResources): boolean {
  return !n.frozen && n.cappedAt == null && n.notCapped == null;
}

/** Keeps the panes' chips current (`resources:pane`). */
export function followResources(): () => void {
  void useResources.getState().loadStatus();
  void invoke<PaneResources[]>("resources_notes", {})
    .then((list) =>
      useResources.setState({ byPane: Object.fromEntries((list ?? []).map((n) => [n.paneId, n])) }),
    )
    .catch(() => undefined);
  const stop = listen<PaneResources>("resources:pane", (n) =>
    useResources.setState((s) => {
      const byPane = { ...s.byPane };
      if (isClear(n)) delete byPane[n.paneId];
      else byPane[n.paneId] = n;
      return { byPane };
    }),
  );
  return () => void stop.then((s) => s());
}

export type UsageLoad =
  { at: "loading" } | { at: "error"; message: string } | { at: "ready"; panes: PaneUsage[] };

/** Every live local pane's usage, refreshed while `visible`; never read
 * while hidden. */
export function usePaneUsage(visible: boolean): { load: UsageLoad; refresh: () => void } {
  const [load, setLoad] = useState<UsageLoad>({ at: "loading" });
  const [tick, setTick] = useState(0);
  useEffect(() => {
    if (!visible) return;
    let live = true;
    const read = async () => {
      try {
        const panes = await invoke<PaneUsage[] | null>("resources_list", {});
        if (live) setLoad({ at: "ready", panes: panes ?? [] });
      } catch (e) {
        if (live) setLoad({ at: "error", message: (e as IpcError).message ?? String(e) });
      }
    };
    void read();
    const timer = setInterval(() => void read(), USAGE_MS);
    return () => {
      live = false;
      clearInterval(timer);
    };
  }, [visible, tick]);
  return { load, refresh: () => setTick((n) => n + 1) };
}

/** "512 MB", "1.5 GB". */
export function formatBytes(bytes: number): string {
  const mb = bytes / (1 << 20);
  if (mb < 1024) return `${Math.round(mb)} MB`;
  const gb = mb / 1024;
  return `${gb >= 10 ? Math.round(gb) : Math.round(gb * 10) / 10} GB`;
}

/** "CPU 12%", or "CPU —" before a second reading. */
export function formatCpu(percent: number | null): string {
  return percent === null ? "—" : `${Math.round(percent)}%`;
}

/** Freeze or thaw a pane; the chip follows `resources:pane`. */
export async function setFrozen(paneId: string, frozen: boolean): Promise<void> {
  await invoke(frozen ? "resources_freeze" : "resources_thaw", { paneId }, { timeoutMs: 10_000 });
}
