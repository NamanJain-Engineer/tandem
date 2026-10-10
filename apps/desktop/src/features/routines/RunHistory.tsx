// UI-53 run history (F-RT-04): a routine's runs, newest first, with their
// trigger, duration, result and chat; failed runs show why.
import { useCallback, useEffect, useState } from "react";
import type { RoutineRun, RoutineView } from "@tandem/protocol";
import {
  Badge,
  Button,
  EmptyState,
  SegmentedControl,
  Sheet,
  Skeleton,
  useToast,
  type BadgeVariant,
} from "@tandem/ui";
import { t } from "../../i18n";
import { isIpcError, routineRuns, useRoutines } from "../../stores/routines";
import { openChat } from "./RoutinesPage";
import { describe } from "./schedule";

const RESULT: Record<RoutineRun["status"], BadgeVariant> = {
  running: "working",
  ok: "done",
  error: "error",
  skipped: "idle",
};

function trigger(r: RoutineRun): string {
  if (r.manual) return t("routines.history.manual");
  if (r.late) return t("routines.history.late");
  return t("routines.history.scheduled");
}

function duration(r: RoutineRun): string {
  if (!r.startedAt || !r.endedAt) return "—";
  const s = Math.max(0, Math.round((r.endedAt - r.startedAt) / 1000));
  return s < 60
    ? t("routines.history.seconds", { n: s })
    : t("routines.history.minutes", { n: Math.round(s / 60) });
}

export function RunHistory({ view, onClose }: { view: RoutineView; onClose: () => void }) {
  const r = view.routine;
  const [runs, setRuns] = useState<RoutineRun[] | null>(null);
  const [more, setMore] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState<"all" | "failed">("all");
  const { show: toast } = useToast();
  const load = useCallback(
    async (before: number | null) => {
      const page = await routineRuns(r.id, before);
      if (isIpcError(page)) {
        setError(page.message);
        return;
      }
      setError(null);
      setRuns((cur) => (before && cur ? [...cur, ...page.runs] : page.runs));
      setMore(page.nextBefore);
    },
    [r.id],
  );
  // Reload when the routine changes (a run starts or ends).
  const stamp = `${r.lastStatus}-${r.updatedAt}-${view.running?.id ?? ""}-${view.today.length}`;
  useEffect(() => {
    void load(null);
  }, [load, stamp]);
  const act = async (e: Promise<unknown>) => {
    const err = await e;
    if (isIpcError(err)) toast({ tone: "error", title: err.message });
  };
  const shown = (runs ?? []).filter((x) => filter === "all" || x.status === "error");
  return (
    <Sheet
      open
      size="lg"
      onOpenChange={(o) => !o && onClose()}
      title={r.name}
      subtitle={view.words ? describe(view.words) : undefined}
      closeLabel={t("routines.close")}
    >
      <div className="flex flex-col gap-3" data-testid="run-history">
        <div className="flex flex-wrap items-center gap-2">
          <Button size="sm" variant="secondary" onClick={() => void act(useRoutines.getState().runNow(r.id))}>
            {t("routines.row.runNow")}
          </Button>
          <Button
            size="sm"
            variant="ghost"
            onClick={() => void act(useRoutines.getState().pause(r.id, !r.paused))}
          >
            {r.paused ? t("routines.row.resume") : t("routines.row.pause")}
          </Button>
          <span className="flex-1" />
          <SegmentedControl
            size="sm"
            label={t("routines.history.filter")}
            value={filter}
            onValueChange={(v) => setFilter(v as "all" | "failed")}
            items={[
              { id: "all", label: t("routines.history.all") },
              { id: "failed", label: t("routines.history.failed") },
            ]}
          />
        </div>
        {error ? (
          <EmptyState
            title={t("routines.history.error")}
            body={error}
            primary={
              <Button variant="secondary" onClick={() => void load(null)}>
                {t("routines.retry")}
              </Button>
            }
          />
        ) : runs === null ? (
          <div className="flex flex-col gap-2">
            {[0, 1, 2].map((i) => (
              <Skeleton key={i} height={28} />
            ))}
          </div>
        ) : shown.length === 0 ? (
          <EmptyState
            title={filter === "failed" ? t("routines.history.noFailed") : t("routines.history.empty")}
            primary={
              filter === "all" ? (
                <Button variant="primary" onClick={() => void act(useRoutines.getState().runNow(r.id))}>
                  {t("routines.row.runNow")}
                </Button>
              ) : undefined
            }
          />
        ) : (
          <ul className="flex flex-col" data-testid="run-list">
            {shown.map((run) => (
              <li
                key={run.id}
                className="flex flex-col gap-1 border-b border-divider py-2 last:border-b-0"
                data-testid="run-row"
                data-status={run.status}
                data-late={run.late ? "true" : "false"}
              >
                <div className="flex flex-wrap items-center gap-3 text-sm">
                  <span className="text-fg">
                    {new Date(run.dueAt).toLocaleString("en-GB", {
                      day: "numeric",
                      month: "short",
                      hour: "numeric",
                      minute: "2-digit",
                      hour12: true,
                    })}
                  </span>
                  <span className="text-fg-muted">{trigger(run)}</span>
                  <span className="text-fg-muted">{duration(run)}</span>
                  <Badge variant={RESULT[run.status]}>{t(`routines.history.status.${run.status}`)}</Badge>
                  <span className="flex-1" />
                  {run.chatId && r.teammateId && (
                    <Button
                      size="sm"
                      variant="ghost"
                      onClick={() => {
                        openChat(r.teammateId!, run.chatId!);
                        onClose();
                      }}
                    >
                      {t("routines.history.openChat")}
                    </Button>
                  )}
                </div>
                {run.error && <p className="text-sm text-error">{run.error}</p>}
              </li>
            ))}
          </ul>
        )}
        {more !== null && (
          <Button size="sm" variant="ghost" onClick={() => void load(more)}>
            {t("routines.history.more")}
          </Button>
        )}
        <p className="text-xs text-fg-subtle">{t("routines.history.retention")}</p>
      </div>
    </Sheet>
  );
}
