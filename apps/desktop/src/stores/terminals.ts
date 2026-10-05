import { create } from "zustand";
import type { PtyState } from "@tandem/protocol";
import { listen } from "../ipc/events";

/** `pty:state` per pane (ARCHITECTURE §5.7): modes for typing and paste,
 * the program's title. Attach seeds it; events keep it current. */
type TerminalsState = {
  states: Record<string, PtyState>;
  put: (state: PtyState) => void;
  forget: (paneId: string) => void;
};

export const useTerminals = create<TerminalsState>((set, get) => ({
  states: {},
  put: (state) => set({ states: { ...get().states, [state.paneId]: state } }),
  forget: (paneId) => {
    const states = { ...get().states };
    delete states[paneId];
    set({ states });
  },
}));

let started = false;

/** Starts following `pty:state` (once per app). */
export function followPtyState(): void {
  if (started) return;
  started = true;
  void listen<PtyState>("pty:state", (s) => useTerminals.getState().put(s));
}
