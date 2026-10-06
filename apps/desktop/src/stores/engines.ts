import { create } from "zustand";
import type { DetectionReport, EngineView } from "@tandem/protocol";
import { listen } from "../ipc/events";
import { invoke } from "../ipc/invoke";

/** Engines from the registry joined with detection (F-EN-01, F-EN-02):
 * installed ones become launch tiles, the rest offer Install. */
type EnginesState = {
  list: EngineView[];
  loaded: boolean;
  /** The last detection report (diagnostics when nothing was found). */
  report: DetectionReport | null;
  /** Check again is running. */
  detecting: boolean;
  refresh: () => Promise<void>;
  /** Re-reads the registry and detects again (Check again). */
  detect: () => Promise<DetectionReport | null>;
  label: (engineId: string | null | undefined) => string | null;
  glyph: (engineId: string | null | undefined) => string;
};

export const useEngines = create<EnginesState>((set, get) => ({
  list: [],
  loaded: false,
  report: null,
  detecting: false,
  refresh: async () => {
    const list = await invoke<EngineView[]>("engines_list", {}).catch(() => get().list);
    set({ list: list ?? [], loaded: true });
  },
  detect: async () => {
    set({ detecting: true });
    try {
      const report = await invoke<DetectionReport>("engines_detect", {}, { timeoutMs: 30_000 });
      set({ report });
      await get().refresh();
      return report;
    } catch {
      return null;
    } finally {
      set({ detecting: false });
    }
  },
  label: (engineId) => get().list.find((e) => e.spec.id === engineId)?.spec.label ?? null,
  glyph: (engineId) => get().list.find((e) => e.spec.id === engineId)?.spec.logo ?? "",
}));

let following = false;

/** Keeps the list current when detection finishes anywhere (startup, focus). */
export function followEngines(): void {
  if (following) return;
  following = true;
  void listen<{ hostId: string; report: DetectionReport }>("engines:detected", ({ hostId, report }) => {
    if (hostId !== "local") return;
    useEngines.setState({ report });
    void useEngines.getState().refresh();
  });
}
