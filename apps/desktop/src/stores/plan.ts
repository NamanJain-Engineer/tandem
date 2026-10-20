// Plan with Lead (F-TK-06): the plans this run holds, kept fresh from
// `tasks:plan` as Lead finishes, tasks are created and runs are queued.
import { create } from "zustand";
import type { IpcError, PlanTask, PlanView } from "@tandem/protocol";
import { listen } from "../ipc/events";
import { invoke } from "../ipc/invoke";
import { isIpcError } from "./tasks";

type PlanState = {
  plans: Record<string, PlanView>;
  /** The plan the dialog shows, if any. */
  openId: string | null;
  open: (id: string | null) => void;
  apply: (v: PlanView) => void;
  start: (workspaceId: string, goal: string) => Promise<PlanView | IpcError>;
  createTasks: (id: string, tasks: PlanTask[]) => Promise<PlanView | IpcError>;
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

/** Keeps plans current: `tasks:plan`. */
export function followPlans(): () => void {
  const stop = listen<PlanView>("tasks:plan", (v) => usePlans.getState().apply(v));
  return () => void stop.then((s) => s());
}
