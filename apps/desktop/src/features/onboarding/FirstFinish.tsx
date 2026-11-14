import { useEffect } from "react";
import type { PaneRecord, StatusRecord } from "@tandem/protocol";
import { useToast } from "@tandem/ui";
import { t } from "../../i18n";
import { useDictate } from "../../stores/dictate";
import { useEngines } from "../../stores/engines";
import { useFiles } from "../../stores/files";
import { gitKey } from "../../stores/git";
import { useLayout } from "../../stores/layout";
import { useSettings } from "../../stores/settings";
import { useSidebar } from "../../stores/sidebar";
import { paneKey, useStatus } from "../../stores/status";
import { useUi } from "../../stores/ui";
import { useWorkspaces } from "../../stores/workspaces";
import { focusPane } from "../ports/PortsPanel";

/** How long the first finish's toast stays, and when the Dictate tip
 * follows it (UI-04). */
const TOAST_MS = 6_000;

function paneOf(key: string): PaneRecord | null {
  for (const tab of Object.values(useLayout.getState().tabs)) {
    for (const pane of Object.values(tab.panes)) if (paneKey(pane.id) === key) return pane;
  }
  return null;
}

/** The agent pane that just turned Done: one that had another status
 * before, or (when the core reported its start and its finish together) one
 * started since `since`. A pane already Done when the app started never
 * counts. */
function justFinished(
  now: Record<string, StatusRecord>,
  before: Record<string, StatusRecord>,
  since: number,
): PaneRecord | null {
  for (const [key, r] of Object.entries(now)) {
    const prev = before[key];
    if (r.status !== "done" || prev?.status === "done" || !key.startsWith("pane:")) continue;
    const pane = paneOf(key);
    if (pane?.kind === "agent" && (prev !== undefined || pane.createdAt >= since)) return pane;
  }
  return null;
}

/** F-SH-08 behaviour 4: the first time an agent finishes after setup, one
 * quiet toast, then the Dictate tip. Once per profile (`first_finish`). */
export function FirstFinish({ tipDelayMs = TOAST_MS }: { tipDelayMs?: number }) {
  const { show } = useToast();
  const armed = useSettings((s) => s.values.first_finish === "armed");
  useEffect(() => {
    if (!armed) return;
    let tip: ReturnType<typeof setTimeout> | null = null;
    const since = Date.now();
    const off = useStatus.subscribe((s, prev) => {
      if (s.records === prev.records) return;
      if (useSettings.getState().values.first_finish !== "armed") return;
      const pane = justFinished(s.records, prev.records, since);
      if (!pane) return;
      off();
      void useSettings.getState().set("first_finish", "shown");
      const engine = useEngines.getState().list.find((e) => e.spec.id === pane.engineId)?.spec.label ?? "";
      const ws = useWorkspaces.getState().list.find((w) => w.id === pane.workspaceId);
      const repo = ws?.kind === "local" && ws.branch !== null;
      show({
        tone: "success",
        title: t("firstFinish.title", { engine, folder: ws?.name ?? "" }),
        duration: TOAST_MS,
        action: {
          label: repo ? t("firstFinish.review") : t("firstFinish.open"),
          onClick: () => {
            focusPane(pane.id, pane.workspaceId);
            if (!repo) return;
            useFiles
              .getState()
              .showView(gitKey({ type: "workspace", workspaceId: pane.workspaceId }), "changes");
            useSidebar.getState().openTab("files");
          },
        },
      });
      // Dictate's tip comes after the finish, never before it (DESIGN §5.5).
      void useDictate
        .getState()
        .loadStatus()
        .then(() => {
          const status = useDictate.getState().status;
          if (!status?.enabled) return;
          tip = setTimeout(
            () =>
              show({
                tone: "info",
                title: t("firstFinish.dictateTitle"),
                body: t("firstFinish.dictateBody", { key: status.hold.label }),
                duration: 12_000,
                action: {
                  label: t("firstFinish.dictateAction"),
                  onClick: () => useUi.getState().openSettings("dictate"),
                },
              }),
            tipDelayMs,
          );
        });
    });
    return () => {
      off();
      if (tip) clearTimeout(tip);
    };
  }, [armed, show, tipDelayMs]);
  return null;
}
