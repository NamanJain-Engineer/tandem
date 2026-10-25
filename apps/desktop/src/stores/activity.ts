import { create } from "zustand";
import type { ActivityEvent } from "@tandem/protocol";
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

export function activityList(source: ActivitySource): Promise<ActivityEvent[]> {
  return invoke<ActivityEvent[] | null>("activity_list", { source: source.source, id: source.id }).then(
    (l) => l ?? [],
  );
}

/** Writes the ledger as JSON Lines where the person picks; `null` when they cancel. */
export function activityExport(source: ActivitySource): Promise<string | null> {
  return invoke<string | null>(
    "activity_export",
    { source: source.source, id: source.id },
    { timeoutMs: 120_000 },
  );
}
