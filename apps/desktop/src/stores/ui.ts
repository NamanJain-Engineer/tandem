import { create } from "zustand";

export type Mode = "agent" | "code" | "thread";
export type UpgradeGate = {
  kind: "workspaces" | "panes" | "thread_grid" | "chat_engines" | "teammates";
  limit: number;
};

type UiState = {
  mode: Mode;
  settingsOpen: boolean;
  /** Focus returns here when Settings closes (F-SH-04 AC1). */
  returnFocus: HTMLElement | null;
  railCollapsed: boolean;
  paletteOpen: boolean;
  /** What the palette searches: commands, or open work by meaning (F-CN-05). */
  paletteMode: "commands" | "find";
  /** The keyboard shortcuts cheatsheet (Mod+/). */
  cheatsheetOpen: boolean;
  /** Help › Diagnostics, the live panel (§5.7). */
  diagnosticsOpen: boolean;
  upgrade: { open: boolean; gate?: UpgradeGate; retry?: () => void };
  setMode: (m: Mode) => void;
  openSettings: (page?: string) => void;
  closeSettings: () => void;
  settingsPage: string | null;
  toggleRail: () => void;
  setPalette: (open: boolean) => void;
  /** Opens the palette as "Find work…". */
  openFindWork: () => void;
  setCheatsheet: (open: boolean) => void;
  setDiagnostics: (open: boolean) => void;
  openUpgrade: (gate?: UpgradeGate, retry?: () => void) => void;
  closeUpgrade: () => void;
};

export const useUi = create<UiState>((set, get) => ({
  mode: "code",
  settingsOpen: false,
  returnFocus: null,
  railCollapsed: false,
  paletteOpen: false,
  paletteMode: "commands",
  cheatsheetOpen: false,
  diagnosticsOpen: false,
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
  // Closing it goes back to commands, so Mod+K always opens the commands.
  setPalette: (paletteOpen) => set(paletteOpen ? { paletteOpen } : { paletteOpen, paletteMode: "commands" }),
  openFindWork: () => set({ paletteOpen: true, paletteMode: "find" }),
  setCheatsheet: (cheatsheetOpen) => set({ cheatsheetOpen }),
  setDiagnostics: (diagnosticsOpen) => set({ diagnosticsOpen }),
  openUpgrade: (gate, retry) => set({ upgrade: { open: true, gate, retry } }),
  closeUpgrade: () => set({ upgrade: { open: false } }),
}));
