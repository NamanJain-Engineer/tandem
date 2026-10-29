import { create } from "zustand";
import type { ApprovalView, DashboardItem, StatusRecord } from "@tandem/protocol";
import { listen } from "../ipc/events";
import { invoke } from "../ipc/invoke";

export type Filter = "all" | "waiting" | "working" | "done" | "idle" | "failed";
export type GroupBy = "status" | "project";

type DashboardState = {
  items: Record<string, DashboardItem>;
  loaded: boolean;
  query: string;
  filter: Filter;
  groupBy: GroupBy;
  /** Group ids folded away (persisted per viewer). */
  collapsed: Record<string, true>;
  load: () => Promise<void>;
  setQuery: (q: string) => void;
  setFilter: (f: Filter) => void;
  setGroupBy: (g: GroupBy) => void;
  toggleGroup: (id: string) => void;
  /** Drops cards the core closed. */
  forget: (keys: string[]) => void;
};

const PREFS = "tandem.dashboard";

function loadPrefs(): Pick<DashboardState, "groupBy" | "collapsed"> {
  try {
    const raw = localStorage.getItem(PREFS);
    if (raw) {
      const p = JSON.parse(raw) as Partial<DashboardState>;
      return { groupBy: p.groupBy === "project" ? "project" : "status", collapsed: p.collapsed ?? {} };
    }
  } catch {
    /* storage unavailable: defaults */
  }
  return { groupBy: "status", collapsed: {} };
}

function savePrefs(s: Pick<DashboardState, "groupBy" | "collapsed">): void {
  try {
    localStorage.setItem(PREFS, JSON.stringify({ groupBy: s.groupBy, collapsed: s.collapsed }));
  } catch {
    /* not persisted */
  }
}

export const useDashboard = create<DashboardState>((set, get) => ({
  items: {},
  loaded: false,
  query: "",
  filter: "all",
  ...loadPrefs(),
  load: async () => {
    const list = await invoke<DashboardItem[]>("status_snapshot").catch(() => null);
    if (!list) {
      set({ loaded: true });
      return;
    }
    set({ items: Object.fromEntries(list.map((i) => [i.key, i])), loaded: true });
  },
  setQuery: (query) => set({ query }),
  setFilter: (filter) => set({ filter }),
  setGroupBy: (groupBy) => {
    set({ groupBy });
    savePrefs(get());
  },
  toggleGroup: (id) => {
    const collapsed = { ...get().collapsed };
    if (collapsed[id]) delete collapsed[id];
    else collapsed[id] = true;
    set({ collapsed });
    savePrefs(get());
  },
  forget: (keys) => {
    const items = { ...get().items };
    for (const k of keys) delete items[k];
    set({ items });
  },
}));

let refetch: ReturnType<typeof setTimeout> | undefined;
/** A new or departed source: ask for the cards again (once per burst). */
function refetchSoon(): void {
  clearTimeout(refetch);
  refetch = setTimeout(() => void useDashboard.getState().load(), 80);
}

/** Applies status changes in place, so a card moves within one event
 * (F-DB-01 AC5: ≤500 ms); unknown sources trigger a fresh snapshot. */
export function applyStatus(records: StatusRecord[], removed: string[]): void {
  const items = { ...useDashboard.getState().items };
  let unknown = removed.length > 0;
  for (const k of removed) delete items[k];
  for (const r of records) {
    const cur = items[r.key];
    if (!cur) {
      unknown = true;
      continue;
    }
    items[r.key] = {
      ...cur,
      status: r.status,
      reason: r.reason,
      since: r.since,
      title: r.title ?? cur.title,
      // dashboard.rs's line: the record's preview, else its tool. The store
      // keeps a preview and replaces the tool with each report, so a tool's
      // name goes when its agent stops (D-2627).
      preview: r.preview ?? r.tool ?? null,
    };
  }
  useDashboard.setState({ items });
  if (unknown) refetchSoon();
}

function applyApprovals(upserted: ApprovalView[], resolved: string[]): void {
  const items = { ...useDashboard.getState().items };
  const sameSource = (a: ApprovalView, item: DashboardItem) =>
    JSON.stringify(a.source) === JSON.stringify(item.source);
  let cleared = false;
  for (const [key, item] of Object.entries(items)) {
    const shown = item.pendingApproval;
    const open = shown && !resolved.includes(shown.id);
    // The shown approval stays until it's answered: another one from the
    // same source mustn't take its place under the person's cursor.
    const fresh = upserted.find((a) => sameSource(a, item) && (!open || a.id === shown.id));
    if (fresh) items[key] = { ...item, pendingApproval: fresh };
    else if (shown && !open) {
      items[key] = { ...item, pendingApproval: null };
      // Another may wait behind it.
      cleared = true;
    }
  }
  useDashboard.setState({ items });
  if (cleared || upserted.some((a) => !Object.values(items).some((i) => sameSource(a, i)))) refetchSoon();
}

let following = false;

export function followDashboard(): void {
  if (following) return;
  following = true;
  void listen<{ records: StatusRecord[]; removed?: string[] }>("status:changed", ({ records, removed }) =>
    applyStatus(records, removed ?? []),
  );
  void listen<{ upserted: ApprovalView[]; resolved: string[] }>(
    "approvals:changed",
    ({ upserted, resolved }) => applyApprovals(upserted, resolved),
  );
}

/** Tests: a clean slate. */
export function resetDashboard(): void {
  clearTimeout(refetch);
  useDashboard.setState({
    items: {},
    loaded: false,
    query: "",
    filter: "all",
    groupBy: "status",
    collapsed: {},
  });
}
