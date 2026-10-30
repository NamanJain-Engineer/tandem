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
    // Updates that come while the snapshot is read are newer than it: kept,
    // to apply over it.
    const meanwhile: StatusChange[] = [];
    reading.add(meanwhile);
    const mine = ++started;
    const list = await invoke<DashboardItem[]>("status_snapshot")
      .catch(() => null)
      .finally(() => reading.delete(meanwhile));
    if (!list) {
      set({ loaded: true });
      return;
    }
    // A snapshot asked for later is already shown, and this older one would
    // take away a card that came meanwhile. Each change during this read is
    // in that snapshot, in its replay or applied since, so none is lost.
    if (mine < landed) return;
    landed = mine;
    set({ items: Object.fromEntries(list.map((i) => [i.key, i])), loaded: true });
    for (const change of meanwhile) applyStatus(change.records, change.removed, false);
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

type StatusChange = { records: StatusRecord[]; removed: string[] };
/** For each snapshot being read, the updates that came meanwhile. */
const reading = new Set<StatusChange[]>();
/** Snapshots asked for, and the newest one shown: they can land out of order. */
let started = 0;
let landed = 0;

/** A card's line as dashboard.rs draws it: the record's preview, else its
 * tool, and none when that's blank. The store keeps a preview and replaces
 * the tool with each report, so a tool's name goes when its agent stops
 * (D-2627). */
function lineOf(r: StatusRecord): string | null {
  const line = r.preview ?? r.tool;
  return line?.trim() ? line : null;
}

/** Applies status changes in place, so a card moves within one event
 * (F-DB-01 AC5: ≤500 ms); unknown sources trigger a fresh snapshot. A change
 * replayed over a snapshot (not `live`) was kept for the other reads and
 * asked for its snapshot when it came, so it does neither again: two reads
 * could otherwise hand it to each other for good. */
export function applyStatus(records: StatusRecord[], removed: string[], live = true): void {
  if (live) for (const meanwhile of reading) meanwhile.push({ records, removed });
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
      preview: lineOf(r),
    };
  }
  useDashboard.setState({ items });
  if (unknown && live) refetchSoon();
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
