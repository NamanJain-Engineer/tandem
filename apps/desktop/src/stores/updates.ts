import { create } from "zustand";
import type { IpcError, UpdateState } from "@tandem/protocol";
import { listen } from "../ipc/events";
import { invoke } from "../ipc/invoke";

type UpdatesStore = {
  state: UpdateState | null;
  /** The restart dialog (UI-130). */
  restartOpen: boolean;
  setRestartOpen: (open: boolean) => void;
  load: () => Promise<void>;
  checkNow: () => Promise<void>;
  setChannel: (channel: "stable" | "beta") => Promise<void>;
  /** "Restart now": resolves with an error message, or null. */
  install: () => Promise<string | null>;
};

export const useUpdates = create<UpdatesStore>((set) => ({
  state: null,
  restartOpen: false,
  setRestartOpen: (restartOpen) => set({ restartOpen }),
  load: async () => {
    const state = await invoke<UpdateState>("updates_state").catch(() => null);
    if (state) set({ state });
  },
  checkNow: async () => {
    const state = await invoke<UpdateState>("updates_check_now", {}, { timeoutMs: 600_000 }).catch(
      () => null,
    );
    if (state) set({ state });
  },
  setChannel: async (channel) => {
    const state = await invoke<UpdateState>("updates_set_channel", { channel }).catch(() => null);
    if (state) set({ state });
  },
  install: async () => {
    try {
      const state = await invoke<UpdateState>("updates_install", {}, { timeoutMs: 120_000 });
      set({ state, restartOpen: false });
      return null;
    } catch (e) {
      await useUpdates.getState().load();
      return (e as IpcError).message ?? "";
    }
  },
}));

/** The title-bar pill shows a staged update, or a failed one (UI-130). */
export function pillOf(s: UpdateState | null): "ready" | "failed" | null {
  if (!s) return null;
  if (s.stage === "ready") return "ready";
  if (s.stage === "error" && s.errorStage && s.errorStage !== "check") return "failed";
  return null;
}

let following = false;

export function followUpdates(): void {
  if (following) return;
  following = true;
  void useUpdates.getState().load();
  void listen<UpdateState>("updates:state", (state) => useUpdates.setState({ state }));
  // The notice's state lives in UpdateState; refetch it when it changes.
  void listen("app:min-version", () => void useUpdates.getState().load());
}

/** Tests. */
export function resetUpdates(): void {
  useUpdates.setState({ state: null, restartOpen: false });
}
