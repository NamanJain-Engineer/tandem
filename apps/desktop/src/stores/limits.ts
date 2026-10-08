import { create } from "zustand";
import type { HandoffResult, IpcError, LimitChanged, LimitView } from "@tandem/protocol";
import { listen } from "../ipc/events";
import { invoke } from "../ipc/invoke";
import { showHandoff } from "./handoff";
import type { Outcome } from "./launch";

/** Usage limits showing on panes (F-AC-05, UI-36). */
type LimitsState = {
  byPane: Record<string, LimitView>;
  set: (paneId: string, limit: LimitView | null) => void;
  load: () => Promise<void>;
};

export const useLimits = create<LimitsState>((set) => ({
  byPane: {},
  set: (paneId, limit) =>
    set((s) => {
      const byPane = { ...s.byPane };
      if (limit) byPane[paneId] = limit;
      else delete byPane[paneId];
      return { byPane };
    }),
  load: async () => {
    const list = await invoke<LimitView[]>("limits_list", {}).catch(() => [] as LimitView[]);
    set({ byPane: Object.fromEntries((list ?? []).map((l) => [l.paneId, l])) });
  },
}));

/** Keeps the limits current: `limit:hit` and `limit:changed`. */
export function followLimits(): () => void {
  void useLimits.getState().load();
  const stops = [
    listen<LimitView>("limit:hit", (l) => useLimits.getState().set(l.paneId, l)),
    listen<LimitChanged>("limit:changed", (c) => useLimits.getState().set(c.paneId, c.limit)),
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

export const resumeAtReset = (paneId: string) => run<LimitView>("limit_resume_at_reset", { paneId });
export const cancelResume = (paneId: string) => run<LimitView>("limit_cancel_resume", { paneId });
export const resumeNow = (paneId: string) => run<void>("limit_resume_now", { paneId });
export const dismissLimit = (paneId: string) => run<void>("limit_dismiss", { paneId });

/** "Continue on another account": the new pane opens split right. */
export async function continueElsewhere(
  paneId: string,
  engineId: string,
  accountId: string | null,
): Promise<Outcome<HandoffResult>> {
  const out = await run<HandoffResult>("limit_continue", { paneId, engineId, accountId });
  if (out.ok) await showHandoff(paneId, out.value, "same");
  return out;
}

/** "4:12 PM" for a reset time. */
export function resetClock(at: number): string {
  return new Date(at).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
}

/** Hours and minutes until `at` (never negative). */
export function until(at: number, now: number): { h: number; m: number } {
  const mins = Math.max(0, Math.round((at - now) / 60_000));
  return { h: Math.floor(mins / 60), m: mins % 60 };
}
