import { create } from "zustand";
import type { AppBoot } from "@tandem/protocol";

/** Boot info from `app_ready`, shared by the shell. */
export const useBoot = create<{ boot: AppBoot | null; setBoot: (b: AppBoot) => void }>((set) => ({
  boot: null,
  setBoot: (boot) => set({ boot }),
}));

export function isDesktopChrome(platform: string | undefined): boolean {
  // Windows and Linux draw their own window controls; macOS uses traffic lights.
  return platform !== "macos";
}
