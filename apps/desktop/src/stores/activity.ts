import { create } from "zustand";
import type { ActivityPage } from "@tandem/protocol";
import { invoke } from "../ipc/invoke";

/** Whose ledger is open (F-EX-07): an agent pane's or a thread's. */
export type ActivitySource = { source: "pane" | "thread"; id: string; title: string };

type ActivityState = {
  open: ActivitySource | null;
  show: (s: ActivitySource) => void;
  close: () => void;
};

export const useActivity = create<ActivityState>((set) => ({
  open: null,
  show: (open) => set({ open }),
  close: () => set({ open: null }),
}));

/** The newest events, or those before `before` (ms). */
export function activityList(source: ActivitySource, before: number | null = null): Promise<ActivityPage> {
  return invoke<ActivityPage | null>("activity_list", { source: source.source, id: source.id, before }).then(
    (p) => p ?? { events: [], more: false },
  );
}

/** A checkpoint the Checkpoints tab should open on: its workspace's folder,
 * with that checkpoint selected (F-EX-07's links). */
export type CheckpointFocus = { workspaceId: string; checkpointId: string };

export const useCheckpointFocus = create<{
  focus: CheckpointFocus | null;
  set: (f: CheckpointFocus | null) => void;
}>((set) => ({ focus: null, set: (focus) => set({ focus }) }));

/** Writes the ledger as JSON Lines where the person picks; `null` when they cancel. */
export function activityExport(source: ActivitySource): Promise<string | null> {
  return invoke<string | null>(
    "activity_export",
    { source: source.source, id: source.id },
    { timeoutMs: 120_000 },
  );
}
