import { create } from "zustand";
import type { PaneSandbox, SandboxStatus } from "@tandem/protocol";
import { listen } from "../ipc/events";
import { invoke } from "../ipc/invoke";

/** The agent sandbox (F-TL-11): whether it can run here, and each pane's note. */
type SandboxState = {
  status: SandboxStatus | null;
  statusState: "idle" | "loading" | "ready" | "error";
  byPane: Record<string, PaneSandbox>;
  loadStatus: () => Promise<void>;
};

export const useSandbox = create<SandboxState>((set) => ({
  status: null,
  statusState: "idle",
  byPane: {},
  loadStatus: async () => {
    set({ statusState: "loading" });
    try {
      const status = await invoke<SandboxStatus>("sandbox_status", {}, { timeoutMs: 30_000 });
      set({ status, statusState: "ready" });
    } catch {
      set({ statusState: "error" });
    }
  },
}));

/** Keeps the panes' notes current (`sandbox:pane`). */
export function followSandbox(): () => void {
  void invoke<PaneSandbox[]>("sandbox_panes", {})
    .then((list) =>
      useSandbox.setState({ byPane: Object.fromEntries((list ?? []).map((n) => [n.paneId, n])) }),
    )
    .catch(() => undefined);
  const stop = listen<PaneSandbox>("sandbox:pane", (n) =>
    useSandbox.setState((s) => ({ byPane: { ...s.byPane, [n.paneId]: n } })),
  );
  return () => void stop.then((s) => s());
}
