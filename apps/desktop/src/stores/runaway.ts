import { create } from "zustand";
import type { GuardCard, GuardCardChange, IpcError } from "@tandem/protocol";
import { listen } from "../ipc/events";
import { invoke } from "../ipc/invoke";
import type { Outcome } from "./launch";

/** Runaway guard cards per pane (F-AC-09, UI-170). */
type RunawayState = {
  byPane: Record<string, GuardCard>;
  set: (paneId: string, card: GuardCard | null) => void;
  load: () => Promise<void>;
};

export const useRunaway = create<RunawayState>((set) => ({
  byPane: {},
  set: (paneId, card) =>
    set((s) => {
      const byPane = { ...s.byPane };
      if (card) byPane[paneId] = card;
      else delete byPane[paneId];
      return { byPane };
    }),
  load: async () => {
    const list = await invoke<GuardCard[]>("runaway_list", {}).catch(() => [] as GuardCard[]);
    set({ byPane: Object.fromEntries((list ?? []).map((c) => [c.paneId, c])) });
  },
}));

/** Keeps the cards current (`runaway:changed`). */
export function followRunaway(): () => void {
  void useRunaway.getState().load();
  const stop = listen<GuardCardChange>("runaway:changed", (c) => useRunaway.getState().set(c.paneId, c.card));
  return () => void stop.then((s) => s());
}

async function run(cmd: string, paneId: string): Promise<Outcome<void>> {
  try {
    return { ok: true, value: await invoke<void>(cmd, { paneId }, { timeoutMs: 30_000 }) };
  } catch (e) {
    return { ok: false, error: e as IpcError };
  }
}

export const pauseAgent = (paneId: string) => run("runaway_pause", paneId);
export const resumeAgent = (paneId: string) => run("runaway_resume", paneId);
export const dismissGuard = (paneId: string) => run("runaway_dismiss", paneId);
