import { create } from "zustand";
import type { DictateStateEvent, DictateStats, DictateStatus, ModelsView } from "@tandem/protocol";
import { listen } from "../ipc/events";
import { invoke } from "../ipc/invoke";

type DictateState = {
  status: DictateStatus | null;
  models: ModelsView | null;
  stats: DictateStats | null;
  /** The latest take state (widget, composers, status bar). */
  take: DictateStateEvent | null;
  level: number;
  /** Bumped when the history changes (`dictate:history`). */
  historyRev: number;
  loadStatus: () => Promise<void>;
  loadModels: () => Promise<void>;
  loadStats: () => Promise<void>;
};

export const useDictate = create<DictateState>((set) => ({
  status: null,
  models: null,
  stats: null,
  take: null,
  level: 0,
  historyRev: 0,
  loadStatus: async () => {
    const status = await invoke<DictateStatus>("dictate_status").catch(() => null);
    if (status) set({ status });
  },
  loadModels: async () => {
    const models = await invoke<ModelsView>("dictate_models").catch(() => null);
    if (models) set({ models });
  },
  loadStats: async () => {
    const stats = await invoke<DictateStats>("dictate_stats").catch(() => null);
    if (stats) set({ stats });
  },
}));

/** Keeps the store current from the voice service's events (ARCHITECTURE §3.3). */
export async function watchDictate(): Promise<() => void> {
  const offs = await Promise.all([
    listen<DictateStatus>("dictate:status", (status) => useDictate.setState({ status })),
    listen<ModelsView>("dictate:models", (models) => useDictate.setState({ models })),
    listen<DictateStateEvent>("dictate:state", (take) =>
      useDictate.setState(take.state === "listening" ? { take } : { take, level: 0 }),
    ),
    listen<number>("dictate:level", (level) => useDictate.setState({ level })),
    listen<null>("dictate:history", () => {
      useDictate.setState((s) => ({ historyRev: s.historyRev + 1 }));
      void useDictate.getState().loadStats();
    }),
  ]);
  return () => offs.forEach((off) => off());
}
