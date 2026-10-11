import { create } from "zustand";
import type { SimStatus } from "@tandem/protocol";
import { useBoot } from "../features/shell/useBoot";
import { inTauri, invoke, usingFakeBackend } from "../ipc/invoke";

/** The Simulator exists on macOS only (F-CD-21, D-1400): elsewhere nothing
 * here ever calls the core, and the picker never offers it. */
export function simulatorPlatform(): boolean {
  return useBoot.getState().boot?.platform === "macos";
}

type SimStore = {
  status: SimStatus | null;
  /** Asks the core again (the picker opens, a pane mounts). */
  load: () => Promise<SimStatus | null>;
};

export const useSimulator = create<SimStore>((set) => ({
  status: null,
  load: async () => {
    if (!simulatorPlatform()) return null;
    const status = await invoke<SimStatus>("sim_status").catch(() => null);
    set({ status });
    return status;
  },
}));

/** Offered in the pane picker: macOS, Xcode's tools and an iOS runtime (AC2). */
export function useSimulatorOffered(): boolean {
  const mac = useBoot((s) => s.boot?.platform === "macos");
  const available = useSimulator((s) => s.status?.available ?? false);
  return mac && available;
}

/** What `sim_stream_start` takes as `channel`: PNG bytes per frame. */
export type PngChannel = { onmessage: (m: ArrayBuffer | Uint8Array) => void };

export async function createPngChannel(onPng: (png: Uint8Array) => void): Promise<PngChannel> {
  const wrap = (m: ArrayBuffer | Uint8Array) => onPng(m instanceof Uint8Array ? m : new Uint8Array(m));
  if (inTauri() && !usingFakeBackend()) {
    const core = await import("@tauri-apps/api/core");
    const channel = new core.Channel<ArrayBuffer>();
    channel.onmessage = wrap;
    return channel as unknown as PngChannel;
  }
  return { onmessage: wrap };
}
