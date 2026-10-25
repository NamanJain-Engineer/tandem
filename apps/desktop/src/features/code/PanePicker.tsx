import { useSimulator, useSimulatorOffered } from "../../stores/simulator";
import { useEffect, useState } from "react";
import { Dialog, useToast } from "@tandem/ui";
import { t } from "../../i18n";
import { useEngines } from "../../stores/engines";
import { useLayout, type Launch } from "../../stores/layout";
import { useWorkspaces } from "../../stores/workspaces";
import { LaunchGrid } from "./LaunchGrid";

export function launchFor(tileId: string): Launch {
  if (tileId === "shell") return { kind: "terminal" };
  if (tileId === "browser") return { kind: "browser" };
  if (tileId === "simulator") return { kind: "simulator" };
  return { kind: "agent", engineId: tileId };
}

/** The picker a split opens (F-CD-02). There's no pane limit with no
 * account (D-2900); one a signed token carries comes back from the core as
 * an error, and nothing is spawned (F-CD-03). */
export function PanePicker({ workspaceName }: { workspaceName: string }) {
  const picker = useLayout((s) => s.picker);
  const closePicker = useLayout((s) => s.closePicker);
  const split = useLayout((s) => s.split);
  const { show } = useToast();
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
    setChecking(true);
    void useWorkspaces
      .getState()
      .refresh()
      .catch(() => undefined)
      .finally(() => setChecking(false));
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
        {picker && (
          <LaunchGrid
            tabId={picker.tabId}
            busy={busy ?? (checking ? "checking" : null)}
            onPick={(id) => void pick(id)}
            autoFocus
            browser
            simulator={simulator && remoteHost === null}
            hostId={remoteHost}
          />
        )}
      </div>
    </Dialog>
  );
}
