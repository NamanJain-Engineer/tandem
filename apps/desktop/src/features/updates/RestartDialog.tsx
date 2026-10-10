import { useEffect, useState } from "react";
import type { DashboardItem } from "@tandem/protocol";
import { Button, Dialog, StatusDot, useToast } from "@tandem/ui";
import { t } from "../../i18n";
import { invoke } from "../../ipc/invoke";
import { useUpdates } from "../../stores/updates";
import { statusView } from "../../stores/status";

/** UI-130's restart dialog: lists what's still running; "Later" keeps the pill. */
export function RestartDialog() {
  const open = useUpdates((s) => s.restartOpen);
  const version = useUpdates((s) => s.state?.version ?? "");
  const [live, setLive] = useState<DashboardItem[] | null>(null);
  const [busy, setBusy] = useState(false);
  const { show } = useToast();
  useEffect(() => {
    if (!open) return;
    setLive(null);
    void invoke<DashboardItem[]>("status_snapshot")
      .then(setLive)
      .catch(() => setLive([]));
  }, [open]);
  const restart = async () => {
    setBusy(true);
    const error = await useUpdates.getState().install();
    setBusy(false);
    if (error) show({ tone: "error", title: error });
  };
  return (
    <Dialog
      open={open}
      onOpenChange={(o) => useUpdates.getState().setRestartOpen(o)}
      title={t("update.dialog.title", { version })}
      footer={
        <>
          <Button variant="ghost" onClick={() => useUpdates.getState().setRestartOpen(false)}>
            {t("update.dialog.later")}
          </Button>
          <Button
            variant="primary"
            data-testid="update-restart-now"
            disabled={busy}
            onClick={() => void restart()}
          >
            {t("update.dialog.now")}
          </Button>
        </>
      }
    >
      <div data-testid="update-restart-dialog" className="flex flex-col gap-3 text-sm text-fg">
        {live && live.length > 0 ? (
          <>
            <p>{t("update.dialog.body")}</p>
            <ul className="flex flex-col gap-1.5" data-testid="update-live-agents">
              {live.map((item) => (
                <li key={item.key} className="flex items-center gap-2" data-testid="update-live-agent">
                  <StatusDot status={statusView(item.status)} size={8} />
                  <span className="truncate">{item.humanName ?? item.title ?? item.engineLabel ?? ""}</span>
                  {item.workspace && <span className="truncate text-fg-muted">{item.workspace}</span>}
                </li>
              ))}
            </ul>
          </>
        ) : (
          <p>{t("update.dialog.idle")}</p>
        )}
      </div>
    </Dialog>
  );
}
