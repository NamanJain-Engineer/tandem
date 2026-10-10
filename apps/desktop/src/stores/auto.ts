import { create } from "zustand";
import type { AutoMode, AutoRunView, IpcError } from "@tandem/protocol";
import { listen } from "../ipc/events";
import { invoke } from "../ipc/invoke";
import type { Outcome } from "./launch";

/** Auto runs by pane (F-AU-01): the live one or the ended one not yet dismissed. */
type AutoStore = {
  byPane: Record<string, AutoRunView>;
  /** The pane whose Auto dialog is open. */
  dialogFor: string | null;
  put: (run: AutoRunView) => void;
  drop: (runId: string) => void;
  load: () => Promise<void>;
  openDialog: (paneId: string) => void;
  closeDialog: () => void;
};

export const useAuto = create<AutoStore>((set) => ({
  byPane: {},
  dialogFor: null,
  put: (run) => set((s) => ({ byPane: { ...s.byPane, [run.paneId]: run } })),
  drop: (runId) =>
    set((s) => ({
      byPane: Object.fromEntries(Object.entries(s.byPane).filter(([, r]) => r.runId !== runId)),
    })),
  load: async () => {
    const list = await invoke<AutoRunView[]>("auto_list", {}).catch(() => [] as AutoRunView[]);
    set({ byPane: Object.fromEntries((list ?? []).map((r) => [r.paneId, r])) });
  },
  openDialog: (paneId) => set({ dialogFor: paneId }),
  closeDialog: () => set({ dialogFor: null }),
}));

/** Keeps the runs current: `auto:changed` and `auto:dismissed`. */
export function followAuto(): () => void {
  void useAuto.getState().load();
  const stops = [
    listen<AutoRunView>("auto:changed", (r) => useAuto.getState().put(r)),
    listen<{ runId: string }>("auto:dismissed", ({ runId }) => useAuto.getState().drop(runId)),
  ];
  return () => stops.forEach((p) => void p.then((stop) => stop()));
}

async function run<T>(cmd: string, args: Record<string, unknown>): Promise<Outcome<T>> {
  try {
    return { ok: true, value: await invoke<T>(cmd, args) };
  } catch (e) {
    return { ok: false, error: e as IpcError };
  }
}

/** The turn ceiling per mode (PRODUCT_SPEC F-AU-01); Custom picks 1–30. */
export const CEILINGS: Record<AutoMode, number> = { finish: 20, harden: 15, review: 8, explore: 10, custom: 10 };
export const MODES: AutoMode[] = ["finish", "harden", "review", "explore", "custom"];

export async function startAuto(
  paneId: string,
  mode: AutoMode,
  goal: string | null,
  ceiling: number | null,
): Promise<Outcome<AutoRunView>> {
  const out = await run<AutoRunView>("auto_start", { paneId, mode, goal, ceiling });
  if (out.ok) useAuto.getState().put(out.value);
  return out;
}

const act = (cmd: string) => async (runId: string) => {
  const out = await run<AutoRunView>(cmd, { runId });
  if (out.ok) useAuto.getState().put(out.value);
  return out;
};
export const pauseAuto = act("auto_pause");
export const resumeAuto = act("auto_resume");
export const stopAuto = act("auto_stop");
export async function dismissAuto(runId: string): Promise<void> {
  useAuto.getState().drop(runId);
  await run<void>("auto_dismiss", { runId });
}
