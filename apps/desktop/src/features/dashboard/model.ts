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

/** "Claude Code · shop · tandem/fix-auth · Work", leaving out what's unknown. */
export function cardMeta(item: DashboardItem): string {
  return [item.engineLabel ?? item.engineId, item.workspace, item.branch, item.accountLabel].filter(Boolean).join(" · ");
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
    [title, item.humanName, item.engineLabel, item.engineId, item.workspace, item.branch, item.accountLabel, item.preview]
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

export type Group = { id: string; label: string; items: DashboardItem[]; status?: AgentStatus };

/** Status groups in a fixed order, or workspaces in rail order with "No
 * folder" last. Empty groups are left out. Newest first within a group. */
export function group(items: DashboardItem[], by: GroupBy, workspaceOrder: string[], noFolder: string): Group[] {
  const newest = (a: DashboardItem, b: DashboardItem) => b.since - a.since || a.key.localeCompare(b.key);
  if (by === "status")
    return STATUS_ORDER.map((status) => ({
      id: `status:${status}`,
      label: status,
      status,
      items: items.filter((i) => i.status === status).sort(newest),
    })).filter((g) => g.items.length > 0);
  const byWs = new Map<string, DashboardItem[]>();
  for (const i of items) {
    const k = i.workspaceId ?? "";
    byWs.set(k, [...(byWs.get(k) ?? []), i]);
  }
  const order = [...workspaceOrder.filter((id) => byWs.has(id)), ...[...byWs.keys()].filter((k) => k && !workspaceOrder.includes(k))];
  const groups: Group[] = order.map((id) => {
    const list = byWs.get(id) ?? [];
    return { id: `ws:${id}`, label: list[0]?.workspace ?? id, items: [...list].sort(newest) };
  });
  const loose = byWs.get("");
  if (loose) groups.push({ id: "ws:none", label: noFolder, items: [...loose].sort(newest) });
  return groups;
}

/** What "Close idle" closes: the idle cards on screen, nothing else. */
export function closeIdleTargets(shown: DashboardItem[]): string[] {
  return shown.filter((i) => i.status === "idle").map((i) => i.key);
}
