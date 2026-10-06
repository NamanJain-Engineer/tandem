import { create } from "zustand";
import type { IpcError, PaneRecord, Tab, TabLayout } from "@tandem/protocol";
import { invoke } from "../ipc/invoke";
import {
  GUTTER,
  autoGrid,
  canSplit,
  contains,
  equalizeAt as equalizeAtNode,
  leaves,
  leaf,
  move as moveNode,
  neighbor,
  remove as removeNode,
  setRatio as setRatioNode,
  split as splitNode,
  swap as swapNode,
  type LayoutNode,
  type Side,
} from "../features/code/layout/tree";
import { releasePaneHost } from "../features/code/paneHosts";

/** One tab's panes and split tree (F-CD-02). The UI owns the tree; Rust
 * validates and persists it (`layout_save`, debounced 500 ms). */
export type TabPanes = {
  tree: LayoutNode | null;
  active: string | null;
  maximized: string | null;
  panes: Record<string, PaneRecord>;
  /** Show the empty state instead of starting a shell (the user closed the
   * last pane, or this tab replaced the last closed tab). */
  keepEmpty: boolean;
};

/** What a new pane runs: a shell, or an engine under the login shell. */
export type Launch = { kind: "terminal" } | { kind: "agent"; engineId: string };

export type SplitRequest = { tabId: string; target: string | null; side: Side };
export type SplitResult = { ok: true; pane: PaneRecord } | { ok: false; error: IpcError };
export type RequestResult = "picker" | "no_room" | "no_tab";
export type Box = { w: number; h: number };

type LayoutState = {
  tabs: Record<string, TabPanes>;
  loaded: Record<string, boolean>;
  /** Grid sizes per tab, for the 240 × 120 split rule. */
  boxes: Record<string, Box>;
  /** A divider drag is in progress: terminals defer PTY resizes (F-CD-02 AC1). */
  dragging: boolean;
  /** The pane picker for a pending split. */
  picker: SplitRequest | null;
  /** Card view per tab (F-CD-11): panes shown as title cards, and the ones
   * flipped back to their terminal. */
  cards: Record<string, { flipped: Record<string, true> }>;
  load: (workspaceId: string) => Promise<void>;
  ensurePane: (tabId: string) => Promise<void>;
  markEmpty: (tabId: string) => void;
  requestSplit: (tabId: string, target: string | null, side: Side) => RequestResult;
  closePicker: () => void;
  split: (tabId: string, target: string | null, side: Side, launch?: Launch) => Promise<SplitResult>;
  close: (tabId: string, paneId: string) => Promise<void>;
  focus: (tabId: string, paneId: string) => void;
  focusDirection: (tabId: string, direction: Side) => void;
  focusCycle: (tabId: string, delta: 1 | -1) => void;
  toggleMaximize: (tabId: string, paneId?: string) => void;
  setRatio: (tabId: string, path: string, ratio: number) => void;
  equalizeAt: (tabId: string, path: string) => void;
  swap: (tabId: string, a: string, b: string) => void;
  move: (tabId: string, paneId: string, target: string, side: Side) => void;
  moveToNewTab: (tabId: string, paneId: string) => Promise<Tab | null>;
  arrange: (tabId: string) => void;
  toggleCards: (tabId: string) => void;
  flipCard: (tabId: string, paneId: string) => void;
  setBox: (tabId: string, box: Box) => void;
  setDragging: (dragging: boolean) => void;
  forgetTab: (tabId: string) => void;
};

const EMPTY: TabPanes = { tree: null, active: null, maximized: null, panes: {}, keepEmpty: false };
const timers = new Map<string, ReturnType<typeof setTimeout>>();
const loading = new Map<string, Promise<void>>();
const ensuring = new Map<string, Promise<void>>();

function save(tabId: string, t: TabPanes): void {
  if (!t.tree) return;
  invoke("layout_save", { tabId, tree: t.tree, activePaneId: t.active, maximizedPaneId: t.maximized }).catch(
    () => undefined,
  );
}

function persist(tabId: string, t: TabPanes): void {
  clearTimeout(timers.get(tabId));
  timers.set(
    tabId,
    setTimeout(() => {
      timers.delete(tabId);
      save(tabId, t);
    }, 500),
  );
}

/** Saves any pending layouts now (quit, tests). */
export function flushLayouts(): void {
  for (const [tabId, timer] of timers) {
    clearTimeout(timer);
    timers.delete(tabId);
    const t = useLayout.getState().tabs[tabId];
    if (t) save(tabId, t);
  }
}

/** Drops a pane from a tab's state; the tab keeps its empty state after its last pane. */
function without(t: TabPanes, paneId: string): TabPanes {
  const tree = t.tree ? removeNode(t.tree, paneId) : null;
  const panes = { ...t.panes };
  delete panes[paneId];
  const ids = leaves(tree);
  return {
    tree,
    panes,
    maximized: t.maximized === paneId ? null : t.maximized,
    active: t.active === paneId ? (ids[0] ?? null) : t.active,
    keepEmpty: tree === null ? true : t.keepEmpty,
  };
}

export const useLayout = create<LayoutState>((set, get) => {
  const update = (tabId: string, f: (t: TabPanes) => TabPanes, persistIt = true) => {
    const current = get().tabs[tabId];
    if (!current) return;
    const next = f(current);
    if (next === current) return;
    set({ tabs: { ...get().tabs, [tabId]: next } });
    if (persistIt) persist(tabId, next);
  };

  const put = (tabId: string, next: TabPanes) => {
    set({ tabs: { ...get().tabs, [tabId]: next } });
    persist(tabId, next);
  };

  return {
    tabs: {},
    loaded: {},
    boxes: {},
    dragging: false,
    picker: null,
    cards: {},

    load: (workspaceId) => {
      const pending = loading.get(workspaceId);
      if (pending) return pending;
      const run = (async () => {
        const layouts = await invoke<TabLayout[]>("layout_get", { workspaceId });
        const tabs = { ...get().tabs };
        for (const l of layouts) {
          if (tabs[l.tab.id]?.tree) continue;
          const panes = await invoke<PaneRecord[]>("pane_list", { tabId: l.tab.id });
          const byId = Object.fromEntries(panes.map((p) => [p.id, p]));
          let tree = (l.tree as LayoutNode | null) ?? null;
          // Drop leaves whose pane closed; add open panes the tree doesn't know.
          for (const id of leaves(tree)) if (!byId[id] && tree) tree = removeNode(tree, id);
          for (const p of panes)
            if (!contains(tree, p.id))
              tree = tree ? splitNode(tree, leaves(tree).at(-1) ?? "", p.id, "right") : leaf(p.id);
          tabs[l.tab.id] = {
            tree,
            active: l.activePaneId && byId[l.activePaneId] ? l.activePaneId : (leaves(tree)[0] ?? null),
            maximized:
              l.maximizedPaneId && byId[l.maximizedPaneId] && leaves(tree).length > 1
                ? l.maximizedPaneId
                : null,
            panes: byId,
            keepEmpty: tabs[l.tab.id]?.keepEmpty ?? false,
          };
        }
        set({ tabs, loaded: { ...get().loaded, [workspaceId]: true } });
      })().finally(() => loading.delete(workspaceId));
      loading.set(workspaceId, run);
      return run;
    },

    ensurePane: (tabId) => {
      const t = get().tabs[tabId];
      if (t?.tree || t?.keepEmpty) return Promise.resolve();
      const pending = ensuring.get(tabId);
      if (pending) return pending;
      const run = (async () => {
        const pane = await invoke<PaneRecord>("pane_create", { tabId, kind: "terminal" });
        const base = get().tabs[tabId] ?? EMPTY;
        put(tabId, {
          ...base,
          tree: leaf(pane.id),
          active: pane.id,
          panes: { ...base.panes, [pane.id]: pane },
          keepEmpty: false,
        });
      })()
        .catch(() => {
          // At the pane limit (or the tab is gone): show the empty state.
          get().markEmpty(tabId);
        })
        .finally(() => ensuring.delete(tabId));
      ensuring.set(tabId, run);
      return run;
    },

    markEmpty: (tabId) => {
      const t = get().tabs[tabId] ?? EMPTY;
      if (t.keepEmpty) return;
      set({ tabs: { ...get().tabs, [tabId]: { ...t, keepEmpty: true } } });
    },

    requestSplit: (tabId, target, side) => {
      const t = get().tabs[tabId];
      if (!t) return "no_tab";
      const anchor = target ?? t.active;
      const box = get().boxes[tabId];
      if (
        t.tree &&
        anchor &&
        box &&
        box.w > 0 &&
        !t.maximized &&
        !canSplit(t.tree, anchor, side, { x: 0, y: 0, ...box })
      )
        return "no_room";
      set({ picker: { tabId, target: anchor, side } });
      return "picker";
    },

    closePicker: () => set({ picker: null }),

    split: async (tabId, target, side, launch = { kind: "terminal" }) => {
      try {
        const pane = await invoke<PaneRecord>("pane_create", {
          tabId,
          kind: launch.kind,
          engineId: launch.kind === "agent" ? launch.engineId : undefined,
        });
        const t = get().tabs[tabId] ?? EMPTY;
        const anchor = target && contains(t.tree, target) ? target : (t.active ?? leaves(t.tree)[0] ?? null);
        const tree = t.tree && anchor ? splitNode(t.tree, anchor, pane.id, side) : leaf(pane.id);
        put(tabId, {
          ...t,
          tree,
          active: pane.id,
          maximized: null,
          panes: { ...t.panes, [pane.id]: pane },
          keepEmpty: false,
        });
        return { ok: true, pane };
      } catch (e) {
        return { ok: false, error: e as IpcError };
      }
    },

    close: async (tabId, paneId) => {
      await invoke("pane_close", { paneId }).catch(() => undefined);
      update(tabId, (t) => without(t, paneId));
      releasePaneHost(paneId);
    },

    focus: (tabId, paneId) =>
      update(tabId, (t) => (t.active === paneId || !contains(t.tree, paneId) ? t : { ...t, active: paneId })),

    focusDirection: (tabId, direction) =>
      update(tabId, (t) => {
        if (!t.tree || !t.active || t.maximized) return t;
        const box = get().boxes[tabId];
        const next = neighbor(
          t.tree,
          t.active,
          direction,
          box && box.w > 0 ? { x: 0, y: 0, ...box } : undefined,
        );
        return next ? { ...t, active: next } : t;
      }),

    focusCycle: (tabId, delta) =>
      update(tabId, (t) => {
        const ids = leaves(t.tree);
        if (ids.length < 2 || t.maximized) return t;
        const i = t.active ? ids.indexOf(t.active) : -1;
        const next = ids[(i + delta + ids.length) % ids.length] ?? null;
        return next ? { ...t, active: next } : t;
      }),

    toggleMaximize: (tabId, paneId) =>
      update(tabId, (t) => {
        const target = paneId ?? t.active;
        if (t.maximized && (!target || t.maximized === target || !paneId)) return { ...t, maximized: null };
        if (!target || leaves(t.tree).length < 2) return t;
        return { ...t, maximized: target, active: target };
      }),

    setRatio: (tabId, path, ratio) =>
      update(tabId, (t) => (t.tree ? { ...t, tree: setRatioNode(t.tree, path, ratio) } : t)),
    equalizeAt: (tabId, path) =>
      update(tabId, (t) => (t.tree ? { ...t, tree: equalizeAtNode(t.tree, path) } : t)),
    swap: (tabId, a, b) => update(tabId, (t) => (t.tree ? { ...t, tree: swapNode(t.tree, a, b) } : t)),
    move: (tabId, paneId, target, side) =>
      update(tabId, (t) =>
        t.tree ? { ...t, tree: moveNode(t.tree, paneId, target, side), active: paneId } : t,
      ),

    moveToNewTab: async (tabId, paneId) => {
      const source = get().tabs[tabId];
      const pane = source?.panes[paneId];
      if (!source || !pane) return null;
      try {
        const tab = await invoke<Tab>("tab_create", { workspaceId: pane.workspaceId });
        await invoke("pane_move", { paneId, tabId: tab.id });
        // The new tab exists in the store before the UI shows it, so it never
        // starts a shell of its own.
        put(tab.id, {
          ...EMPTY,
          tree: leaf(paneId),
          active: paneId,
          panes: { [paneId]: { ...pane, tabId: tab.id } },
        });
        update(tabId, (t) => without(t, paneId));
        return tab;
      } catch {
        return null;
      }
    },

    arrange: (tabId) => {
      const box = get().boxes[tabId];
      update(tabId, (t) => (t.tree ? { ...t, tree: autoGrid(leaves(t.tree), box, GUTTER), maximized: null } : t));
    },

    toggleCards: (tabId) => {
      const cards = { ...get().cards };
      if (cards[tabId]) delete cards[tabId];
      else cards[tabId] = { flipped: {} };
      set({ cards });
    },

    flipCard: (tabId, paneId) => {
      const cur = get().cards[tabId];
      if (!cur) return;
      set({ cards: { ...get().cards, [tabId]: { flipped: { ...cur.flipped, [paneId]: true } } } });
    },

    setBox: (tabId, box) => {
      const prev = get().boxes[tabId];
      if (prev && prev.w === box.w && prev.h === box.h) return;
      set({ boxes: { ...get().boxes, [tabId]: box } });
    },

    setDragging: (dragging) => {
      if (get().dragging !== dragging) set({ dragging });
    },

    forgetTab: (tabId) => {
      clearTimeout(timers.get(tabId));
      timers.delete(tabId);
      const tabs = { ...get().tabs };
      delete tabs[tabId];
      set({ tabs });
    },
  };
});

/** Live panes across loaded tabs. */
export function paneCount(): number {
  return Object.values(useLayout.getState().tabs).reduce((n, t) => n + leaves(t.tree).length, 0);
}
