import { create } from "zustand";
import type { EngineView } from "@tandem/protocol";
import { invoke } from "../ipc/invoke";

/** Engines from the registry with where they're installed (F-EN-02, M3
 * slice: PATH detection). Installed ones become launch tiles. */
type EnginesState = {
  list: EngineView[];
  loaded: boolean;
  refresh: () => Promise<void>;
  label: (engineId: string | null | undefined) => string | null;
};

export const useEngines = create<EnginesState>((set, get) => ({
  list: [],
  loaded: false,
  refresh: async () => {
    const list = await invoke<EngineView[]>("engines_list", {}).catch(() => get().list);
    set({ list: list ?? [], loaded: true });
  },
  label: (engineId) => get().list.find((e) => e.spec.id === engineId)?.spec.label ?? null,
}));
