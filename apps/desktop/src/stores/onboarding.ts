import { create } from "zustand";
import type { OnboardingState } from "@tandem/protocol";
import { invoke } from "../ipc/invoke";
import { useSettings } from "./settings";

type OnboardingStore = {
  state: OnboardingState | null;
  /** The folder chosen on Your first folder (its root), for Done. A relaunch
   * forgets it; Done then reads the newest workspace (F-SH-08 AC2). */
  folder: string | null;
  /** The engine Done starts, when the user picked one on Engines; otherwise
   * the first signed-in engine is the default (D-2649). */
  engine: string | null;
  load: () => Promise<void>;
  /** Moves to a step (persisted: quitting resumes there, F-SH-08 AC2). */
  go: (step: string) => Promise<void>;
  finish: () => Promise<void>;
  /** Settings › General → "Run setup again". */
  restart: () => Promise<void>;
  setFolder: (root: string | null) => void;
  setEngine: (engineId: string) => void;
};

export const useOnboarding = create<OnboardingStore>((set, get) => ({
  state: null,
  folder: null,
  engine: null,
  load: async () => {
    const state = await invoke<OnboardingState>("onboarding_get").catch(() => null);
    // Without an answer the app opens as usual.
    set({ state: state ?? { step: null, done: true, steps: [] } });
  },
  go: async (step) => {
    const prev = get().state;
    if (prev) set({ state: { ...prev, step, done: false } });
    const state = await invoke<OnboardingState>("onboarding_set", { step }).catch(() => null);
    if (state) set({ state });
  },
  finish: async () => {
    const prev = get().state;
    if (prev) set({ state: { ...prev, step: null, done: true } });
    // The core arms the first finish's toast as setup ends (F-SH-08
    // behaviour 4); the settings store learns it here.
    const values = useSettings.getState().values;
    if ((values.first_finish ?? "unset") === "unset")
      useSettings.setState({ values: { ...values, first_finish: "armed" } });
    const state = await invoke<OnboardingState>("onboarding_set", { done: true }).catch(() => null);
    if (state) set({ state });
  },
  restart: async () => {
    const first = get().state?.steps[0] ?? "engines";
    set({ folder: null, engine: null });
    await get().go(first);
  },
  setFolder: (folder) => set({ folder }),
  setEngine: (engine) => set({ engine }),
}));

/** Tests. */
export function resetOnboarding(): void {
  useOnboarding.setState({
    state: { step: null, done: true, steps: ["engines", "workspace", "finish"] },
    folder: null,
    engine: null,
  });
}
