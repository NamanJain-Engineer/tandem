import { useMemo } from "react";
import type { WorkspaceSummary } from "@tandem/protocol";
import { useLayout } from "../../stores/layout";
import { paneKey, useStatus } from "../../stores/status";

/** The right slot of a workspace row (F-SH-03): n working, Needs you,
 * Failed, or nothing; and the unread dot when something finished unseen. */
export function useWorkspaceStatus(ws: WorkspaceSummary): {
  working: number;
  waiting: boolean;
  failed: boolean;
  unread: boolean;
} {
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
