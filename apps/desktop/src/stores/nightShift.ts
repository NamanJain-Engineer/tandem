import { create } from "zustand";
import type { IpcError, NightAccount, NightInput, NightRun } from "@tandem/protocol";
import { listen } from "../ipc/events";
import { invoke } from "../ipc/invoke";

/** The Night Shift's runs and the accounts' next resets (F-RT-06). */
type NightState = {
  runs: NightRun[];
  accounts: NightAccount[];
  accountsState: "idle" | "loading" | "ready" | "error";
  load: () => Promise<void>;
  loadAccounts: () => Promise<void>;
  schedule: (input: NightInput) => Promise<NightRun | IpcError>;
  cancel: (id: string) => Promise<IpcError | null>;
};

export const useNightShift = create<NightState>((set) => ({
  runs: [],
  accounts: [],
  accountsState: "idle",
  load: async () => {
    const runs = await invoke<NightRun[]>("night_list", {}).catch(() => [] as NightRun[]);
    set({ runs: runs ?? [] });
  },
  loadAccounts: async () => {
    set({ accountsState: "loading" });
    try {
      const accounts = await invoke<NightAccount[]>("night_accounts", {}, { timeoutMs: 30_000 });
      set({ accounts: accounts ?? [], accountsState: "ready" });
    } catch {
      set({ accountsState: "error" });
    }
  },
  schedule: async (input) => {
    try {
      const run = await invoke<NightRun>("night_schedule", { input });
      await useNightShift.getState().load();
      return run;
    } catch (e) {
      return e as IpcError;
    }
  },
  cancel: async (id) => {
    try {
      await invoke<void>("night_cancel", { id });
      await useNightShift.getState().load();
      return null;
    } catch (e) {
      return e as IpcError;
    }
  },
}));

/** Keeps the runs current (`night:changed`). */
export function followNightShift(): () => void {
  void useNightShift.getState().load();
  const stop = listen<NightRun[]>("night:changed", (runs) => useNightShift.setState({ runs }));
  return () => void stop.then((s) => s());
}
