// Plan with Lead (F-TK-06): the plans the app holds, loaded when the
// window opens and kept fresh from `tasks:plan` as Lead finishes, tasks are
// created and runs are queued.
import { create } from "zustand";
import type { IpcError, PlanLaunch, PlanTask, PlanView } from "@tandem/protocol";
import { listen } from "../ipc/events";
import { invoke } from "../ipc/invoke";
import { isIpcError } from "./tasks";

type PlanState = {
  plans: Record<string, PlanView>;
  /** The plan the dialog shows, if any. */
  openId: string | null;
  open: (id: string | null) => void;
  apply: (v: PlanView) => void;
  /** The plans the app holds (a reloaded window starts empty). */
  load: () => Promise<void>;
  start: (workspaceId: string, goal: string) => Promise<PlanView | IpcError>;
  createTasks: (id: string, tasks: PlanTask[]) => Promise<PlanView | IpcError>;
  /** What Execute all starts, for you to confirm. */
  prepare: (id: string) => Promise<PlanLaunch | IpcError>;
  /** Starts what `prepare` showed. */
  execute: (id: string) => Promise<PlanView | IpcError>;
  discard: (id: string) => Promise<void>;
};

async function call<T>(cmd: string, args: Record<string, unknown>): Promise<T | IpcError> {
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    return isIpcError(e)
      ? e
      : { code: "internal", message: String(e), stage: null, hint: null, retryable: false };
  }
}

export const usePlans = create<PlanState>((set, get) => ({
  plans: {},
  openId: null,
  open: (id) => set({ openId: id }),
  apply: (v) => set((s) => ({ plans: { ...s.plans, [v.id]: v } })),
  load: async () => {
    const r = await call<PlanView[]>("plan_list", {});
    if (isIpcError(r)) return;
    // What arrived on `tasks:plan` meanwhile is newer.
    set((s) => ({ plans: { ...Object.fromEntries(r.map((v) => [v.id, v])), ...s.plans } }));
  },
  start: async (workspaceId, goal) => {
    const r = await call<PlanView>("plan_start", { workspaceId, goal });
    if (!isIpcError(r)) set((s) => ({ plans: { ...s.plans, [r.id]: r }, openId: r.id }));
    return r;
  },
  createTasks: async (id, tasks) => {
    const r = await call<PlanView>("plan_create_tasks", { id, tasks });
    if (!isIpcError(r)) get().apply(r);
    return r;
  },
  prepare: (id) => call<PlanLaunch>("plan_prepare", { id }),
  execute: async (id) => {
    const r = await call<PlanView>("plan_execute", { id });
    if (!isIpcError(r)) get().apply(r);
    return r;
  },
  discard: async (id) => {
    await call<null>("plan_discard", { id });
    set((s) => {
      const plans = { ...s.plans };
      delete plans[id];
      return { plans, openId: s.openId === id ? null : s.openId };
    });
  },
}));

/** Keeps plans current: the app's plans, then `tasks:plan`. */
export function followPlans(): () => void {
  const stop = listen<PlanView>("tasks:plan", (v) => usePlans.getState().apply(v));
  void stop.then(() => usePlans.getState().load());
  return () => void stop.then((s) => s());
}
