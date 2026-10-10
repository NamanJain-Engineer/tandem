import { create } from "zustand";
import type { UsageView, UsageWindow } from "@tandem/protocol";
import { listen } from "../ipc/events";
import { invoke } from "../ipc/invoke";
import { useLayout } from "./layout";
import { useStatus } from "./status";

/** Usage per engine account (F-AC-04). */
type UsageState = {
  byAccount: Record<string, UsageView>;
  set: (views: UsageView[]) => void;
  refresh: (accountId?: string | null) => Promise<void>;
  /** The latest reading for an account not read yet (probes once). */
  ensure: (accountId: string) => void;
};

const asking = new Set<string>();

export const useUsage = create<UsageState>((set) => ({
  byAccount: {},
  set: (views) =>
    set((s) => ({
      byAccount: { ...s.byAccount, ...Object.fromEntries(views.map((v) => [v.accountId, v])) },
    })),
  refresh: async (accountId = null) => {
    const views = await invoke<UsageView[]>("usage_refresh", { accountId }, { timeoutMs: 60_000 }).catch(
      () => [] as UsageView[],
    );
    useUsage.getState().set(views ?? []);
  },
  ensure: (accountId) => {
    if (asking.has(accountId) || useUsage.getState().byAccount[accountId]) return;
    asking.add(accountId);
    void invoke<UsageView[]>("usage_get", { accountId }, { timeoutMs: 60_000 })
      .then((views) => useUsage.getState().set(views ?? []))
      .catch(() => undefined)
      .finally(() => asking.delete(accountId));
  },
}));

/** The window closest to exhaustion. */
export function fullest(windows: UsageWindow[]): UsageWindow | null {
  return windows.reduce<UsageWindow | null>(
    (top, w) => (top === null || w.usedPct > top.usedPct ? w : top),
    null,
  );
}

const EVERY = 5 * 60_000;
/** A finished turn refreshes its account at most this often. */
const AFTER_TURN = 30_000;

/** Usage from the core, every 5 minutes and after each turn. */
export function followUsage(): () => void {
  void invoke<UsageView[]>("usage_get", {}, { timeoutMs: 60_000 })
    .then((views) => useUsage.getState().set(views ?? []))
    .catch(() => undefined);
  const stop = listen<UsageView>("usage:changed", (v) => useUsage.getState().set([v]));
  const timer = setInterval(() => void useUsage.getState().refresh(), EVERY);
  const last: Record<string, number> = {};
  const was: Record<string, string> = {};
  const unsub = useStatus.subscribe((s) => {
    for (const [key, rec] of Object.entries(s.records)) {
      const before = was[key];
      was[key] = rec.status;
      if (rec.status !== "done" || before === "done" || !key.startsWith("pane:")) continue;
      const paneId = key.slice(5);
      const tabs = useLayout.getState().tabs;
      const pane = Object.values(tabs)
        .map((t) => t?.panes[paneId])
        .find(Boolean);
      const account = pane?.accountId;
      if (!account || Date.now() - (last[account] ?? 0) < AFTER_TURN) continue;
      last[account] = Date.now();
      void useUsage.getState().refresh(account);
    }
  });
  return () => {
    clearInterval(timer);
    unsub();
    void stop.then((s) => s());
  };
}
