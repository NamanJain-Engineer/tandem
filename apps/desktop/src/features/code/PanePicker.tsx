import { useSimulator, useSimulatorOffered } from "../../stores/simulator";
import { useEffect, useState } from "react";
import type { IpcError } from "@tandem/protocol";
import { Button, Callout, Dialog, useToast } from "@tandem/ui";
import { t } from "../../i18n";
import { useEngines } from "../../stores/engines";
import { useLayout, type Launch } from "../../stores/layout";
import { useSession } from "../../stores/session";
import { useUi } from "../../stores/ui";
import { useWorkspaces } from "../../stores/workspaces";
import { LaunchGrid } from "./LaunchGrid";

/** Live panes across every workspace, from the core's counts. */
function liveTotal(): number {
  return useWorkspaces.getState().list.reduce((n, w) => n + w.liveCount, 0);
}

export function atPaneLimit(): boolean {
  const limit = useSession.getState().entitlement?.limits.panes ?? null;
  return limit !== null && liveTotal() >= limit;
}

/** "Free includes 4 live panes" (UI_SPEC copy table). No process starts. */
export function PaneLimitCard({ onManage }: { onManage: () => void }) {
  const limit = useSession((s) => s.entitlement?.limits.panes ?? 4);
  const openUpgrade = useUi((s) => s.openUpgrade);
  return (
    <Callout
      tone="accent"
      title={t("limit.panes.title", { n: limit })}
      actions={
        <>
          <Button size="sm" variant="primary" onClick={() => openUpgrade({ kind: "panes", limit })}>
            {t("limit.panes.plans")}
          </Button>
          <Button size="sm" variant="ghost" onClick={onManage}>
            {t("limit.panes.manage")}
          </Button>
        </>
      }
    >
      <p data-testid="pane-limit">{t("limit.panes.body")}</p>
    </Callout>
  );
}

export function launchFor(tileId: string): Launch {
  if (tileId === "shell") return { kind: "terminal" };
  if (tileId === "browser") return { kind: "browser" };
  if (tileId === "simulator") return { kind: "simulator" };
  return { kind: "agent", engineId: tileId };
}

/** The picker a split opens (F-CD-02). At the live-pane limit it shows the
 * upgrade card instead, and nothing is spawned (F-CD-03). */
export function PanePicker({ workspaceName }: { workspaceName: string }) {
  const picker = useLayout((s) => s.picker);
  const closePicker = useLayout((s) => s.closePicker);
  const split = useLayout((s) => s.split);
  const { show } = useToast();
  const [limited, setLimited] = useState(false);
  const [checking, setChecking] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  // A remote or WSL workspace's + menu lists its own engines (F-RM-01, F-RM-02).
  const remoteHost = useWorkspaces((s) => {
    const ws = picker ? s.list.find((w) => w.tabs.some((tab) => tab.id === picker.tabId)) : undefined;
    return ws && (ws.kind === "ssh" || ws.kind === "wsl") ? ws.hostId : null;
  });

  // macOS only; elsewhere this never asks the core (F-CD-21 AC2).
  const simulator = useSimulatorOffered();

  useEffect(() => {
    if (!picker) return;
    void useEngines.getState().refresh();
    void useSimulator.getState().load();
    setBusy(null);
    setLimited(atPaneLimit());
    setChecking(true);
    void useWorkspaces
      .getState()
      .refresh()
      .catch(() => undefined)
      .finally(() => {
        setLimited(atPaneLimit());
        setChecking(false);
      });
  }, [picker]);

  const pick = async (tileId: string) => {
    if (!picker) return;
    setBusy(tileId);
    const res = await split(picker.tabId, picker.target, picker.side, launchFor(tileId));
    setBusy(null);
    if (res.ok) {
      closePicker();
      void useWorkspaces
        .getState()
        .refresh()
        .catch(() => undefined);
    } else if ((res.error as IpcError).code === "limit_reached") {
      setLimited(true);
    } else {
      show({ tone: "error", title: res.error.message });
      closePicker();
    }
  };

  return (
    <Dialog
      open={picker !== null}
      onOpenChange={(o) => !o && closePicker()}
      title={t("picker.title")}
      description={workspaceName}
      size="sm"
    >
      <div data-testid="pane-picker" className="flex flex-col gap-3">
        {limited ? (
          <PaneLimitCard onManage={closePicker} />
        ) : (
          picker && (
            <LaunchGrid
              tabId={picker.tabId}
              busy={busy ?? (checking ? "checking" : null)}
              onPick={(id) => void pick(id)}
              autoFocus
              browser
              simulator={simulator && remoteHost === null}
              hostId={remoteHost}
            />
          )
        )}
      </div>
    </Dialog>
  );
}
