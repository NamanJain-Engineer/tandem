import { create } from "zustand";
import type { SidebarState, SidebarTab, SidebarTabKind } from "@tandem/protocol";
import { invoke } from "../ipc/invoke";

export const SIDEBAR_MIN = 320;
export const SIDEBAR_MAX = 900;
/** Below this window width the sidebar floats over the content (F-SH-10). */
export const FLOAT_BELOW = 1_100;

const DEFAULT: SidebarState = {
  open: false,
  tabs: [{ id: "dashboard", kind: "dashboard", title: null, url: null }],
  active: "dashboard",
  width: 470,
  fullscreen: false,
  pinned: false,
};

type SidebarStore = SidebarState & {
  loaded: boolean;
  load: () => Promise<void>;
  setOpen: (open: boolean) => void;
  toggle: () => void;
  /** "+" menu: a singleton that's open already is focused, not doubled. */
  openTab: (kind: SidebarTabKind) => void;
  activate: (id: string) => void;
  close: (id: string) => void;
  reorder: (ids: string[]) => void;
  setWidth: (width: number) => void;
  setFullscreen: (on: boolean) => void;
  togglePin: () => void;
};

let saveTimer: ReturnType<typeof setTimeout> | undefined;
const SAVE_DELAY_MS = 300;

const clamp = (w: number) => Math.round(Math.min(SIDEBAR_MAX, Math.max(SIDEBAR_MIN, w)));
const singleton = (kind: SidebarTabKind) => kind !== "browser";

export const useSidebar = create<SidebarStore>((set, get) => {
  const state = (): SidebarState => {
    const { open, tabs, active, width, fullscreen, pinned } = get();
    return { open, tabs, active, width, fullscreen, pinned };
  };
  /** Applies a change now and saves it shortly after the last one. */
  const change = (patch: Partial<SidebarState>) => {
    set(patch);
    clearTimeout(saveTimer);
    saveTimer = setTimeout(() => {
      void invoke<SidebarState>("sidebar_save", { sidebar: state() }).catch(() => undefined);
    }, SAVE_DELAY_MS);
  };
  return {
    ...DEFAULT,
    loaded: false,
    load: async () => {
      const saved = await invoke<SidebarState>("sidebar_get").catch(() => null);
      set({ ...(saved ?? DEFAULT), loaded: true });
    },
    setOpen: (open) => change(open ? { open } : { open, fullscreen: false }),
    toggle: () => get().setOpen(!get().open),
    openTab: (kind) => {
      const existing = singleton(kind) ? get().tabs.find((x) => x.kind === kind) : undefined;
      if (existing) {
        change({ open: true, active: existing.id });
        return;
      }
      const id = singleton(kind) ? kind : `${kind}-${Date.now().toString(36)}`;
      const tab: SidebarTab = { id, kind, title: null, url: null };
      change({ open: true, tabs: [...get().tabs, tab], active: id });
    },
    activate: (id) => change({ active: id }),
    close: (id) => {
      const tabs = get().tabs;
      const at = tabs.findIndex((x) => x.id === id);
      if (at < 0) return;
      const rest = tabs.filter((x) => x.id !== id);
      const active = get().active === id ? (rest[Math.min(at, rest.length - 1)]?.id ?? null) : get().active;
      change({ tabs: rest, active, ...(rest.length === 0 ? { open: false, fullscreen: false } : {}) });
    },
    reorder: (ids) => {
      const byId = new Map(get().tabs.map((x) => [x.id, x]));
      const tabs = ids.map((id) => byId.get(id)).filter((x): x is SidebarTab => x !== undefined);
      if (tabs.length === get().tabs.length) change({ tabs });
    },
    setWidth: (width) => change({ width: clamp(width) }),
    setFullscreen: (fullscreen) => change({ fullscreen, ...(fullscreen ? { open: true } : {}) }),
    togglePin: () => change({ pinned: !get().pinned }),
  };
});

/** Tests: back to defaults without saving. */
export function resetSidebar(): void {
  clearTimeout(saveTimer);
  useSidebar.setState({ ...DEFAULT, loaded: false });
}
