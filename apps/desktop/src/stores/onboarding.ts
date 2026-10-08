import { create } from "zustand";
import type { OnboardingState } from "@tandem/protocol";
import { invoke } from "../ipc/invoke";

type OnboardingStore = {
  state: OnboardingState | null;
  load: () => Promise<void>;
  /** Moves to a step (persisted: quitting resumes there, F-SH-08 AC2). */
  go: (step: string) => Promise<void>;
  finish: () => Promise<void>;
  /** Settings › General → "Run setup again". */
  restart: () => Promise<void>;
};

export const useOnboarding = create<OnboardingStore>((set, get) => ({
  state: null,
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
    const state = await invoke<OnboardingState>("onboarding_set", { done: true }).catch(() => null);
    if (state) set({ state });
  },
  restart: async () => {
    const first = get().state?.steps[0] ?? "engines";
    await get().go(first);
  },
}));

/** Tests. */
export function resetOnboarding(): void {
  useOnboarding.setState({ state: { step: null, done: true, steps: ["engines", "accounts", "privacy", "workspace"] } });
}
