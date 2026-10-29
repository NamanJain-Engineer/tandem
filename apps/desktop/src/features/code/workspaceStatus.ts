import { useMemo } from "react";
import type { WorkspaceSummary } from "@tandem/protocol";
import { useLayout } from "../../stores/layout";
import { paneKey, useStatus } from "../../stores/status";

/** What a workspace's agents are doing, for its rail row (F-SH-03). */
export type WorkspaceStatus = {
  working: number;
  waiting: boolean;
  failed: boolean;
  unread: boolean;
};

/** Whether the row says so in words: n working, Needs you or Failed.
 * Beside them, the row's marks are their icons and its branch gives way
 * (D-2603, D-2700). */
export function showsStatus(s: WorkspaceStatus): boolean {
  return s.waiting || s.working > 0 || s.failed;
}

/** The right slot of a workspace row (F-SH-03): n working, Needs you,
 * Failed, or nothing; and the unread dot when something finished unseen.
 * A row reads it once and hands it to its parts. */
export function useWorkspaceStatus(ws: WorkspaceSummary): WorkspaceStatus {
  const tabs = useLayout((s) => s.tabs);
  const records = useStatus((s) => s.records);
  const unread = useStatus((s) => s.unread);
  return useMemo(() => {
    const keys = ws.tabs.flatMap((tab) => Object.keys(tabs[tab.id]?.panes ?? {})).map(paneKey);
    const statuses = keys.map((k) => records[k]?.status);
    return {
      working: statuses.filter((x) => x === "working").length,
      waiting: statuses.includes("waiting"),
      failed: statuses.includes("failed"),
      unread: keys.some((k) => unread[k]),
    };
  }, [ws.tabs, tabs, records, unread]);
}
