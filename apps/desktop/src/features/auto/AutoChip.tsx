import { X, Zap } from "lucide-react";
import type { AutoRunView } from "@tandem/protocol";
import { IconButton, Menu, Spinner, Tooltip, type MenuEntry } from "@tandem/ui";
import { t } from "../../i18n";
import { dismissAuto, pauseAuto, resumeAuto, stopAuto, useAuto } from "../../stores/auto";

/** The words a run's end shows: Done, Stuck or Stopped, then why. */
export function endLabel(run: AutoRunView): { head: string; reason: string } {
  const reason = run.endReason ?? "user_stopped";
  const head = reason === "done" ? t("auto.chip.done") : reason === "stuck" ? t("auto.chip.stuck") : t("auto.chip.stopped");
  return { head, reason: run.detail ?? t(`auto.end.${reason}`) };
}

/** The pane header chip while Auto runs ("⚡ Finish · 3/20"), and its end
 * until dismissed (F-AU-01). */
export function AutoChip({ paneId }: { paneId: string }) {
  const run = useAuto((s) => s.byPane[paneId]);
  if (!run) return null;
  const mode = t(`auto.mode.${run.mode}`);
  const chip =
    "inline-flex h-5 shrink-0 items-center gap-1 rounded-full border border-border bg-surface-3 px-1.5 text-xs text-fg-muted";

  if (run.state === "ended") {
    const { head, reason } = endLabel(run);
    return (
      <span className={chip} data-testid="auto-chip" data-state="ended" data-reason={run.endReason ?? undefined}>
        <Zap size={12} aria-hidden />
        <Tooltip label={reason}>
          <span tabIndex={0} className="max-w-[220px] truncate rounded-xs focus-visible:shadow-focus focus-visible:outline-none">
            {t("auto.chip.ended", { head, reason })}
          </span>
        </Tooltip>
        <IconButton
          size="sm"
          icon={<X size={12} />}
          label={t("auto.chip.dismiss")}
          onClick={() => void dismissAuto(run.runId)}
          data-testid="auto-dismiss"
        />
      </span>
    );
  }

  const approval = run.paused === "approval_pending";
  const entries: MenuEntry[] = [
    approval
      ? { id: "waiting", type: "label", label: t("auto.chip.approval") }
      : run.paused === "user"
        ? { id: "resume", label: t("auto.resume"), onSelect: () => void resumeAuto(run.runId) }
        : { id: "pause", label: t("auto.pause"), onSelect: () => void pauseAuto(run.runId) },
    { id: "stop", label: t("auto.stop"), danger: true, onSelect: () => void stopAuto(run.runId) },
  ];
  const label = t("auto.chip.live", { mode, n: run.turns, of: run.ceiling });
  return (
    <Menu
      entries={entries}
      trigger={
        <button
          type="button"
          data-testid="auto-chip"
          data-state={run.state}
          aria-label={run.paused ? t("auto.chip.pausedLabel", { label }) : label}
          className={`${chip} hover:text-fg focus-visible:shadow-focus focus-visible:outline-none`}
        >
          {run.state === "running" ? <Spinner size={12} /> : <Zap size={12} aria-hidden />}
          <span>{label}</span>
          {run.paused && <span className="text-fg-subtle">{t("auto.chip.paused")}</span>}
        </button>
      }
    />
  );
}
