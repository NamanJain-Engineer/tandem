import { create } from "zustand";
import type { IpcError, ReviewItem, ReviewQueueView } from "@tandem/protocol";
import { listen } from "../ipc/events";
import { invoke } from "../ipc/invoke";

export type ReviewSort = "risk" | "time";

type ReviewState = {
  items: ReviewItem[];
  loaded: boolean;
  error: string | null;
  /** The Dashboard panel's view (F-DB-04). */
  view: "agents" | "review";
  sort: ReviewSort;
  setView: (v: "agents" | "review") => void;
  setSort: (s: ReviewSort) => void;
  load: (refresh?: boolean) => Promise<void>;
  apply: (q: ReviewQueueView) => void;
};

export const useReview = create<ReviewState>((set) => ({
  items: [],
  loaded: false,
  error: null,
  view: "agents",
  sort: "risk",
  setView: (view) => set({ view }),
  setSort: (sort) => set({ sort }),
  load: async (refresh = false) => {
    try {
      const q = await invoke<ReviewQueueView>("review_queue", { refresh });
      set({ items: q?.items ?? [], loaded: true, error: null });
    } catch (e) {
      set({ loaded: true, error: (e as IpcError).message });
    }
  },
  apply: (q) => set({ items: q.items, loaded: true, error: null }),
}));

/** Waiting to be looked at: not approved, not snoozed. */
export function toReview(items: ReviewItem[]): ReviewItem[] {
  return items.filter((i) => i.state === "review" || i.state === "changed");
}

/** Risk: more rules first, then the oldest finish; time: oldest first. */
export function sortItems(items: ReviewItem[], sort: ReviewSort): ReviewItem[] {
  return [...items].sort(
    (a, b) =>
      (sort === "risk" ? b.risks.length - a.risks.length : 0) ||
      a.finishedAt - b.finishedAt ||
      a.key.localeCompare(b.key),
  );
}

/** Keeps the queue current: `review:queue`. */
export function followReview(): () => void {
  const stop = listen<ReviewQueueView>("review:queue", (q) => useReview.getState().apply(q));
  return () => void stop.then((s) => s());
}
