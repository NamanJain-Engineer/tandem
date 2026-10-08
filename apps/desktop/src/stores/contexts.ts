import { create } from "zustand";
import type { PaneContext } from "@tandem/protocol";
import { listen } from "../ipc/events";
import { invoke } from "../ipc/invoke";

/** Context fill per agent pane (F-AC-06), from the engine's own transcript
 * when a turn ends. */
type ContextsState = {
  byPane: Record<string, PaneContext>;
  set: (c: PaneContext) => void;
  load: () => Promise<void>;
};

export const useContexts = create<ContextsState>((set) => ({
  byPane: {},
  set: (c) => set((s) => ({ byPane: { ...s.byPane, [c.paneId]: c } })),
  load: async () => {
    const list = await invoke<PaneContext[]>("contexts_list", {}).catch(() => [] as PaneContext[]);
    // An event that arrived meanwhile is newer than the list.
    set((s) => ({ byPane: { ...Object.fromEntries((list ?? []).map((c) => [c.paneId, c])), ...s.byPane } }));
  },
}));

export function followContexts(): () => void {
  void useContexts.getState().load();
  const stop = listen<PaneContext>("context:changed", (c) => useContexts.getState().set(c));
  return () => void stop.then((s) => s());
}
