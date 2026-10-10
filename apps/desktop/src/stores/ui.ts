import { create } from "zustand";

export type Mode = "agent" | "code" | "thread";
export type UpgradeGate = { kind: "workspaces" | "panes" | "thread_grid" | "chat_engines"; limit: number };

type UiState = {
  mode: Mode;
  settingsOpen: boolean;
  /** Focus returns here when Settings closes (F-SH-04 AC1). */
  returnFocus: HTMLElement | null;
  railCollapsed: boolean;
  paletteOpen: boolean;
  /** The keyboard shortcuts cheatsheet (Mod+/). */
  cheatsheetOpen: boolean;
  upgrade: { open: boolean; gate?: UpgradeGate; retry?: () => void };
  setMode: (m: Mode) => void;
  openSettings: (page?: string) => void;
  closeSettings: () => void;
  settingsPage: string | null;
  toggleRail: () => void;
  setPalette: (open: boolean) => void;
  setCheatsheet: (open: boolean) => void;
  openUpgrade: (gate?: UpgradeGate, retry?: () => void) => void;
  closeUpgrade: () => void;
};

export const useUi = create<UiState>((set, get) => ({
  mode: "code",
  settingsOpen: false,
  returnFocus: null,
  railCollapsed: false,
  paletteOpen: false,
  cheatsheetOpen: false,
  upgrade: { open: false },
  settingsPage: null,
  setMode: (mode) => set({ mode }),
  openSettings: (page) => {
    if (get().settingsOpen) {
      if (page) set({ settingsPage: page });
      return;
    }
    const active = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    set({ settingsOpen: true, returnFocus: active, settingsPage: page ?? get().settingsPage });
  },
  closeSettings: () => {
    const target = get().returnFocus;
    set({ settingsOpen: false, returnFocus: null });
    // Restore focus after the previous mode is visible again.
    requestAnimationFrame(() => target?.isConnected && target.focus());
  },
  toggleRail: () => set({ railCollapsed: !get().railCollapsed }),
  setPalette: (paletteOpen) => set({ paletteOpen }),
  setCheatsheet: (cheatsheetOpen) => set({ cheatsheetOpen }),
  openUpgrade: (gate, retry) => set({ upgrade: { open: true, gate, retry } }),
  closeUpgrade: () => set({ upgrade: { open: false } }),
}));
