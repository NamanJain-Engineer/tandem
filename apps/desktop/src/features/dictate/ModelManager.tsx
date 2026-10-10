import { useEffect, useState } from "react";
import type { IpcError, ModelView } from "@tandem/protocol";
import { Badge, Button, ConfirmDialog, ProgressBar, Sheet, Spinner, useToast } from "@tandem/ui";
import { t } from "../../i18n";
import { invoke } from "../../ipc/invoke";
import { useDictate } from "../../stores/dictate";
import { formatBytes, formatDuration } from "./format";

function StateCell({ m, onRetry }: { m: ModelView; onRetry: () => void }) {
  const p = m.progress;
  switch (m.state) {
    case "downloading": {
      const done = p && p.stage === "downloading" ? Number(p.done) : 0;
      const total = p && p.stage === "downloading" ? Number(p.total) : Number(m.bytes);
      const speed = p && p.stage === "downloading" ? Number(p.speed) : 0;
      const pct = total > 0 ? Math.round((done / total) * 100) : 0;
      const left = speed > 0 ? ((total - done) / speed) * 1000 : 0;
      return (
        <div className="flex min-w-48 flex-col gap-1" data-testid={`model-progress-${m.id}`}>
          <ProgressBar value={pct} label={t("dictate.models.progressLabel", { name: m.name })} />
          <span className="text-xs text-fg-muted">
            {speed > 0
              ? t("dictate.models.progress", { pct, speed: formatBytes(speed), left: formatDuration(left) })
              : t("dictate.models.progressPct", { pct })}
          </span>
        </div>
      );
    }
    case "queued":
      return <span className="text-sm text-fg-muted">{t("dictate.models.queued")}</span>;
    case "verifying":
      return (
        <span className="flex items-center gap-1.5 text-sm text-fg-muted">
          <Spinner /> {t("dictate.models.verifying")}
        </span>
      );
    case "loading":
      return (
        <span className="flex items-center gap-1.5 text-sm text-fg-muted">
          <Spinner /> {t("dictate.models.loading")}
        </span>
      );
    case "active":
      return <Badge variant="done">{t("dictate.models.active")}</Badge>;
    case "downloaded":
      return <span className="text-sm text-fg-muted">{t("dictate.models.downloaded")}</span>;
    case "failed":
      return (
        <span className="flex items-center gap-2 text-sm text-[var(--tn-status-error-text,var(--tn-danger))]" data-testid={`model-failed-${m.id}`}>
          {m.error ?? t("dictate.models.failed")}
          <Button size="sm" variant="secondary" onClick={onRetry} data-testid={`model-retry-${m.id}`}>
            {t("dictate.models.retry")}
          </Button>
        </span>
      );
    default:
      return null;
  }
}

/** UI-63: the dictation models, their downloads and which one is active. */
export function ModelManager({ open, onClose }: { open: boolean; onClose: () => void }) {
  const view = useDictate((s) => s.models);
  const { show } = useToast();
  const [confirmUnused, setConfirmUnused] = useState(false);
  useEffect(() => {
    if (open) void useDictate.getState().loadModels();
  }, [open]);
  const call = (cmd: string, modelId?: string) =>
    invoke("dictate_" + cmd, modelId ? { modelId } : undefined).then(
      () => void useDictate.getState().loadModels(),
      (e: IpcError) => show({ tone: "error", title: e.message }),
    );
  const models = view?.models ?? [];
  const unused = models.filter((m) => m.state === "downloaded");
  const unusedBytes = unused.reduce((n, m) => n + Number(m.bytes), 0);
  return (
    <Sheet open={open} onOpenChange={(o) => !o && onClose()} title={t("dictate.models.title")} size="md">
      {!view ? (
        <span className="flex items-center gap-2 text-sm text-fg-muted">
          <Spinner /> {t("dictate.loading")}
        </span>
      ) : (
        <div className="flex flex-col gap-3" data-testid="model-manager">
          {view.pending && <p className="text-sm text-fg-muted">{t("dictate.models.afterTake")}</p>}
          <ul aria-label={t("dictate.models.title")} className="flex flex-col divide-y divide-divider rounded-md border border-border">
            {models.map((m) => {
              const selectable = m.available && m.runnable && (m.state === "downloaded" || m.state === "active");
              return (
                <li key={m.id} className="flex flex-wrap items-center gap-3 px-3 py-2.5" data-model={m.id} data-state={m.state}>
                  <div className="flex min-w-0 flex-1 flex-col">
                    <span className="flex items-center gap-2 text-base text-fg">
                      {m.name}
                      {m.recommended && <Badge variant="accent">{t("dictate.models.recommended")}</Badge>}
                      {m.cpuWarning && <Badge variant="waiting">{t("dictate.models.cpu")}</Badge>}
                    </span>
                    <span className="text-sm text-fg-muted">
                      {formatBytes(Number(m.bytes))} · {m.langsLabel}
                      {!m.available && ` · ${t("dictate.models.notOnThisMac")}`}
                      {m.available && !m.runnable && ` · ${t("dictate.models.notInBuild")}`}
                    </span>
                  </div>
                  <StateCell m={m} onRetry={() => void call("model_download", m.id)} />
                  {m.state === "absent" && m.available && (
                    <Button size="sm" variant="secondary" onClick={() => void call("model_download", m.id)} data-testid={`model-download-${m.id}`}>
                      {t("dictate.models.download")}
                    </Button>
                  )}
                  {(m.state === "downloading" || m.state === "queued") && (
                    <Button size="sm" variant="ghost" onClick={() => void call("model_cancel", m.id)} aria-label={t("dictate.models.cancel", { name: m.name })}>
                      {t("dictate.models.pause")}
                    </Button>
                  )}
                  {selectable && m.state === "downloaded" && (
                    <Button size="sm" variant="secondary" onClick={() => void call("model_select", m.id)} data-testid={`model-use-${m.id}`}>
                      {t("dictate.models.use")}
                    </Button>
                  )}
                  {(m.state === "downloaded" || m.state === "active") && (
                    <Button size="sm" variant="ghost" onClick={() => void call("model_remove", m.id)} data-testid={`model-remove-${m.id}`}>
                      {t("dictate.models.remove")}
                    </Button>
                  )}
                </li>
              );
            })}
          </ul>
          <div className="flex items-center justify-between text-sm text-fg-muted">
            <span>{t("dictate.models.using", { size: formatBytes(Number(view.usedBytes)) })}</span>
            <Button size="sm" variant="ghost" disabled={unused.length === 0} onClick={() => setConfirmUnused(true)}>
              {t("dictate.models.removeUnused")}
            </Button>
          </div>
          <p className="text-xs text-fg-subtle">{t("dictate.models.licences")}</p>
        </div>
      )}
      <ConfirmDialog
        open={confirmUnused}
        onOpenChange={setConfirmUnused}
        title={t("dictate.models.removeUnusedTitle", { n: unused.length, size: formatBytes(unusedBytes) })}
        confirmLabel={t("dictate.models.remove")}
        danger
        onConfirm={() => void call("models_remove_unused")}
      />
    </Sheet>
  );
}
