import type { AgentStatus, DashboardItem } from "@tandem/protocol";
import type { Filter, GroupBy } from "../../stores/dashboard";
import { cleanTitle } from "../code/titles";

export const STATUS_ORDER: AgentStatus[] = ["waiting", "working", "done", "failed", "idle"];

/** "Ada · Fix the auth guard": the agent's name and its work (F-DB-01). */
export function cardTitle(item: DashboardItem, titlePatterns: string[] = []): string {
  const work = item.title ? cleanTitle(item.title, titlePatterns) : "";
  const name = item.humanName ?? "";
  if (name && work) return `${name} · ${work}`;
  return name || work || item.engineLabel || item.engineId || "Agent";
}

/** "Claude Code · shop · tandem/fix-auth · Work", leaving out what's
 * unknown. The account shows only beside another of the same engine's
 * (`accounts` is how many it has): alone, "Default" says nothing (D-2620). */
export function cardMeta(item: DashboardItem, accounts: number): string {
  return [
    item.engineLabel ?? item.engineId,
    item.workspace,
    item.branch,
    accounts > 1 ? item.accountLabel : null,
  ]
    .filter(Boolean)
    .join(" · ");
}

const fold = (s: string) =>
  s
    .normalize("NFD")
    .replace(/\p{Diacritic}/gu, "")
    .toLowerCase();

/** Every word must match one of the card's fields (AND), case- and
 * accent-insensitive (F-DB-01). */
export function matches(item: DashboardItem, query: string, title: string = cardTitle(item)): boolean {
  const words = fold(query).split(/\s+/).filter(Boolean);
  if (words.length === 0) return true;
  const hay = fold(
    [
      title,
      item.humanName,
      item.engineLabel,
      item.engineId,
      item.workspace,
      item.branch,
      item.accountLabel,
      item.preview,
    ]
      .filter(Boolean)
      .join(" \u0000 "),
  );
  return words.every((w) => hay.includes(w));
}

export type Counts = Record<AgentStatus, number> & { total: number };

export function counts(items: DashboardItem[]): Counts {
  const c: Counts = { waiting: 0, working: 0, done: 0, idle: 0, failed: 0, total: items.length };
  for (const i of items) c[i.status] += 1;
  return c;
}

export function visible(
  items: DashboardItem[],
  filter: Filter,
  query: string,
  titleOf: (item: DashboardItem) => string = (i) => cardTitle(i),
): DashboardItem[] {
  return items.filter((i) => (filter === "all" || i.status === filter) && matches(i, query, titleOf(i)));
}

export type Group = {
  id: string;
  label: string;
  items: DashboardItem[];
  status?: AgentStatus;
  /** The cards with a question open, pinned first (F-TL-12.12). */
  asking?: true;
  /** Project grouping: the repository's worktrees, each with its branch. */
  children?: Group[];
};

/** Worktrees by workspace id: the repository they belong to and their branch. */
export type Lanes = Record<string, { parentId: string; parentName: string; branch: string }>;

/** {@link askingGroup} then {@link groupRest}, as one list: the order the
 * Dashboard shows, which renders the two apart (the questions above its
 * header). Used by the tests. A question's card moves only when one asked
 * before it is answered or withdrawn, or an earlier question's card changes
 * size; no other agent's report (status, `since`) moves it. */
export function group(
  items: DashboardItem[],
  by: GroupBy,
  workspaceOrder: string[],
  noFolder: string,
  lanes: Lanes = {},
): Group[] {
  const asking = askingGroup(items);
  const rest = groupRest(
    items.filter((i) => !i.pendingApproval),
    by,
    workspaceOrder,
    noFolder,
    lanes,
  );
  return asking ? [asking, ...rest] : rest;
}

/** The cards with a question open, in the order the questions were asked;
 * `null` when there are none. Built from every card, whatever the filter or
 * search: those follow what agents report, and a question dropping in and
 * out would move every one after it (F-TL-12.12). */
export function askingGroup(items: DashboardItem[]): Group | null {
  const asking = items
    .filter((i) => i.pendingApproval)
    .sort(
      (a, b) =>
        (a.pendingApproval?.createdAt ?? 0) - (b.pendingApproval?.createdAt ?? 0) ||
        (a.pendingApproval?.id ?? "").localeCompare(b.pendingApproval?.id ?? ""),
    );
  return asking.length > 0 ? { id: "asking", label: "asking", items: asking, asking: true } : null;
}

/** Status groups in a fixed order, or workspaces in rail order with "No
 * folder" last. Empty groups are left out. Newest first within a group. */
export function groupRest(
  items: DashboardItem[],
  by: GroupBy,
  workspaceOrder: string[],
  noFolder: string,
  lanes: Lanes = {},
): Group[] {
  const newest = (a: DashboardItem, b: DashboardItem) => b.since - a.since || a.key.localeCompare(b.key);
  if (by === "status")
    return STATUS_ORDER.map((status) => ({
      id: `status:${status}`,
      label: status,
      status,
      items: items.filter((i) => i.status === status).sort(newest),
    })).filter((g) => g.items.length > 0);
  // A worktree's agents count toward its repository's group.
  const home = (i: DashboardItem) => {
    const k = i.workspaceId ?? "";
    return lanes[k]?.parentId ?? k;
  };
  const byWs = new Map<string, DashboardItem[]>();
  for (const i of items) byWs.set(home(i), [...(byWs.get(home(i)) ?? []), i]);
  const order = [
    ...workspaceOrder.filter((id) => byWs.has(id)),
    ...[...byWs.keys()].filter((k) => k && !workspaceOrder.includes(k)),
  ];
  const rank = (id: string) => {
    const n = workspaceOrder.indexOf(id);
    return n < 0 ? Number.MAX_SAFE_INTEGER : n;
  };
  const groups: Group[] = order.map((id) => {
    const list = byWs.get(id) ?? [];
    const own = list.filter((i) => (i.workspaceId ?? "") === id);
    const laneIds = [
      ...new Set(list.map((i) => i.workspaceId ?? "").filter((k) => k !== id && lanes[k])),
    ].sort((a, b) => rank(a) - rank(b) || a.localeCompare(b));
    const children = laneIds.map((laneId) => ({
      id: `ws:${laneId}`,
      label: lanes[laneId]?.branch ?? laneId,
      items: list.filter((i) => i.workspaceId === laneId).sort(newest),
    }));
    const label = Object.values(lanes).find((l) => l.parentId === id)?.parentName || own[0]?.workspace || id;
    return { id: `ws:${id}`, label, items: own.sort(newest), ...(children.length > 0 ? { children } : {}) };
  });
  const loose = byWs.get("");
  if (loose) groups.push({ id: "ws:none", label: noFolder, items: [...loose].sort(newest) });
  return groups;
}

/** What "Close idle" closes: the idle cards on screen, nothing else. */
export function closeIdleTargets(shown: DashboardItem[]): string[] {
  return shown.filter((i) => i.status === "idle").map((i) => i.key);
}
