import { useEffect, useState } from "react";
import { Button, ProgressBar } from "@tandem/ui";
import { t } from "../../i18n";
import { invoke } from "../../ipc/invoke";
import { useUpdates } from "../../stores/updates";

/** "Update now" from the unsupported-version notice: check, download, then
 * the restart dialog. */
export async function updateNow(): Promise<void> {
  const store = useUpdates.getState();
  if (store.state?.stage !== "ready") await store.checkNow();
  if (useUpdates.getState().state?.stage === "ready") useUpdates.getState().setRestartOpen(true);
}

/** Hours of grace left, rounded up. */
export function hoursLeft(graceUntil: number, now = Date.now()): number {
  return Math.max(0, Math.ceil((graceUntil - now) / 3_600_000));
}

/** UI-131 after the 72 h grace: a modal overlay (terminals keep working
 * under the banner until then). It comes back on every focus. */
export function UnsupportedOverlay() {
  const notice = useUpdates((s) => s.state?.minVersion ?? null);
  const stage = useUpdates((s) => s.state?.stage);
  const percent = useUpdates((s) => s.state?.percent ?? 0);
  const [, tick] = useState(0);
  useEffect(() => {
    const refresh = () => void useUpdates.getState().load();
    window.addEventListener("focus", refresh);
    const id = setInterval(() => {
      refresh();
      tick((n) => n + 1);
    }, 60_000);
    return () => {
      window.removeEventListener("focus", refresh);
      clearInterval(id);
    };
  }, []);
  if (!notice?.modal) return null;
  return (
    <div
      role="alertdialog"
      aria-modal="true"
      aria-labelledby="unsupported-title"
      data-testid="unsupported-overlay"
      className="fixed inset-0 z-[var(--z-max)] flex items-center justify-center bg-overlay p-6"
    >
      <div className="flex max-w-md flex-col gap-4 rounded-lg border border-border bg-surface-3 p-6 shadow-2">
        <h2 id="unsupported-title" className="text-lg font-semibold text-fg">
          {t("unsupported.title")}
        </h2>
        <p className="text-sm text-fg">{t("unsupported.bodyModal")}</p>
        {stage === "downloading" && <ProgressBar value={percent / 100} label={t("unsupported.update")} />}
        <div className="flex justify-end gap-2">
          <Button variant="ghost" onClick={() => invoke("app_quit").catch(() => undefined)}>
            {t("unsupported.quit")}
          </Button>
          <Button variant="primary" data-testid="unsupported-update" disabled={stage === "checking" || stage === "downloading"} onClick={() => void updateNow()}>
            {t("unsupported.update")}
          </Button>
        </div>
      </div>
    </div>
  );
}
