import { create } from "zustand";
import type { IpcError, PaneRecord, PaneRestore, RestoreSnapshot } from "@tandem/protocol";
import { listen } from "../ipc/events";
import { invoke } from "../ipc/invoke";

type CardState = PaneRestore["state"];

type RestoreStore = {
  /** Paused agent cards by pane (F-SH-07, UI-132). */
  cards: Record<string, PaneRestore>;
  /** "Resume agents automatically" was on at launch. */
  autoResume: boolean;
  loaded: boolean;
  load: () => Promise<void>;
  setState: (paneId: string, state: CardState) => void;
  /** Resume or Start fresh; resolves with an error message, or null. */
  resume: (paneId: string, fresh?: boolean) => Promise<string | null>;
  cancel: (paneId: string) => Promise<void>;
};

export const useRestore = create<RestoreStore>((set, get) => ({
  cards: {},
  autoResume: false,
  loaded: false,
  load: async () => {
    const snap = await invoke<RestoreSnapshot>("restore_get_snapshot").catch(() => null);
    const cards: Record<string, PaneRestore> = {};
    for (const p of snap?.panes ?? []) cards[p.paneId] = p;
    // Events that arrived while loading win over the snapshot.
    set({ cards: { ...cards, ...get().cards }, autoResume: snap?.autoResume ?? false, loaded: true });
  },
  setState: (paneId, state) => {
    const card = get().cards[paneId];
    if (card) set({ cards: { ...get().cards, [paneId]: { ...card, state } } });
  },
  resume: async (paneId, fresh = false) => {
    get().setState(paneId, "queued");
    try {
      await invoke(fresh ? "restore_start_fresh" : "restore_resume", { paneId }, { timeoutMs: 120_000 });
      get().setState(paneId, "live");
      return null;
    } catch (e) {
      const err = e as IpcError;
      // Back to the card it came from, unless Ctrl+C cancelled it meanwhile.
      if (err.code !== "cancelled") get().setState(paneId, get().cards[paneId]?.state === "cancelled" ? "cancelled" : "paused");
      return err.code === "cancelled" ? null : (err.message ?? "");
    }
  },
  cancel: async (paneId) => {
    get().setState(paneId, "cancelled");
    await invoke("restore_cancel", { paneId }).catch(() => undefined);
  },
}));

/** An agent pane shows its card instead of a terminal until it runs. */
export function waitingOnCard(pane: Pick<PaneRecord, "id" | "kind" | "restoreState">, card: PaneRestore | undefined): boolean {
  if (pane.kind !== "agent") return false;
  if (card) return card.state !== "live";
  return pane.restoreState === "paused" || pane.restoreState === "cancelled";
}

let following = false;

export function followRestore(): void {
  if (following) return;
  following = true;
  void listen<{ paneId: string; state: CardState }>("restore:changed", ({ paneId, state }) =>
    useRestore.getState().setState(paneId, state),
  );
}

/** Once the app shell shows: paused agents resume by themselves when the
 * user asked for that (the core starts them two at a time). */
export function startAutoResume(): void {
  if (useRestore.getState().autoResume) invoke("restore_auto_resume").catch(() => undefined);
}

/** Tests. */
export function resetRestore(): void {
  useRestore.setState({ cards: {}, autoResume: false, loaded: false });
}
