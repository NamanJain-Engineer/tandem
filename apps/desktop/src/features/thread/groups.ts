import type { Thread, WorkspaceSummary } from "@tandem/protocol";
import { t } from "../../i18n";

export type RailGroup = {
  /** "pinned", "none" (No folder), "other" or a workspace id. */
  id: string;
  /** Uppercased in the rail; null for the fixed groups (labelled by key). */
  name: string | null;
  kind: "pinned" | "none" | "folder" | "other";
  workspaceId: string | null;
  threads: Thread[];
};

/** Rail order (F-TH-01, UI-20): Pinned, No folder, then one group per
 * folder (rail order), then Other (folders since removed). Archived threads
 * only show with "Show archived"; `query` filters by title. */
export function groupThreads(
  threads: Thread[],
  workspaces: WorkspaceSummary[],
  { showArchived, query }: { showArchived: boolean; query: string },
): RailGroup[] {
  const q = query.trim().toLowerCase();
  const visible = threads
    .filter((t) => t.kind === "thread" && (showArchived || !t.archived))
    .filter((t) => !q || t.title.toLowerCase().includes(q))
    .sort((a, b) => b.updatedAt - a.updatedAt);
  const known = new Set(workspaces.map((w) => w.id));
  const pinned = visible.filter((t) => t.pinned);
  const rest = visible.filter((t) => !t.pinned);
  const groups: RailGroup[] = [];
  if (pinned.length)
    groups.push({ id: "pinned", name: null, kind: "pinned", workspaceId: null, threads: pinned });
  const none = rest.filter((t) => t.workspaceId === null);
  if (none.length) groups.push({ id: "none", name: null, kind: "none", workspaceId: null, threads: none });
  for (const ws of workspaces) {
    const mine = rest.filter((t) => t.workspaceId === ws.id);
    if (mine.length)
      groups.push({ id: ws.id, name: ws.name, kind: "folder", workspaceId: ws.id, threads: mine });
  }
  const other = rest.filter((t) => t.workspaceId !== null && !known.has(t.workspaceId));
  if (other.length)
    groups.push({ id: "other", name: null, kind: "other", workspaceId: null, threads: other });
  return groups;
}

/** "2m", "3h", "yesterday", "4d", or a short date. */
export function age(ts: number, now: number): string {
  const s = Math.max(0, Math.floor((now - ts) / 1000));
  if (s < 60) return t("thread.age.now");
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h}h`;
  const d = Math.floor(h / 24);
  if (d === 1) return t("thread.age.yesterday");
  if (d < 7) return `${d}d`;
  return new Date(ts).toLocaleDateString(undefined, { month: "short", day: "numeric" });
}
