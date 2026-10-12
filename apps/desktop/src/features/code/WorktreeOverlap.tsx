import { useCallback, useEffect, useState } from "react";
import { AlertTriangle, Check } from "lucide-react";
import type { OverlapReport, WorkspaceSummary } from "@tandem/protocol";
import { Banner, Dialog, Skeleton } from "@tandem/ui";
import { t } from "../../i18n";
import { worktreeOverlap } from "../../stores/worktrees";

type Load = { at: "loading" } | { at: "error"; message: string } | { at: "ready"; report: OverlapReport };

/** Loads `lane_overlap` for a worktree (F-WT-03). */
export function useOverlap(laneId: string): { load: Load; retry: () => void } {
  const [load, setLoad] = useState<Load>({ at: "loading" });
  const fetch = useCallback(() => {
    let live = true;
    setLoad({ at: "loading" });
    void worktreeOverlap(laneId).then((out) => {
      if (!live) return;
      setLoad(out.ok ? { at: "ready", report: out.value } : { at: "error", message: out.error.message });
    });
    return () => {
      live = false;
    };
  }, [laneId]);
  useEffect(() => fetch(), [fetch]);
  return { load, retry: fetch };
}

/** The forecast's verdict, with the conflicted files (UI-153 and the merge
 * confirm). */
export function ForecastVerdict({ report }: { report: OverlapReport }) {
  const f = report.forecast;
  return (
    <div data-testid="merge-forecast" data-verdict={f.kind} className="flex flex-col gap-1 text-sm">
      {f.kind === "clean" ? (
        <p className="flex items-center gap-1.5 text-fg">
          <Check size={14} aria-hidden="true" className="text-success" />
          {t("worktree.overlap.clean")}
        </p>
      ) : f.kind === "conflicts" ? (
        <>
          <p className="flex items-center gap-1.5 text-fg">
            <AlertTriangle size={14} aria-hidden="true" className="text-warning" />
            {t("worktree.overlap.conflicts", { n: f.files.length })}
          </p>
          <ul className="flex flex-col gap-0.5 pl-5 font-mono text-xs text-fg-muted">
            {f.files.map((path) => (
              <li key={path} className="truncate">
                {path}
              </li>
            ))}
          </ul>
        </>
      ) : (
        <p className="text-fg-muted">{t("worktree.overlap.unavailable", { reason: f.reason })}</p>
      )}
      {report.uncommitted > 0 ? (
        <p className="text-xs text-fg-subtle">{t("worktree.overlap.uncommitted", { n: report.uncommitted })}</p>
      ) : null}
    </div>
  );
}

/** UI-153 "Overlap": sibling worktrees that changed the same files, and the
 * merge forecast. Opened from a worktree's ⋯ menu. */
export function OverlapDialog({
  wt,
  base,
  onClose,
}: {
  wt: WorkspaceSummary;
  base: string;
  onClose: () => void;
}) {
  const { load, retry } = useOverlap(wt.id);
  return (
    <Dialog
      open
      onOpenChange={(o) => !o && onClose()}
      title={t("worktree.overlap.title", { branch: wt.laneBranch ?? wt.name })}
      description={t("worktree.overlap.description", { base })}
      closeLabel={t("common.close")}
    >
      <div data-testid="worktree-overlap" className="flex flex-col gap-4">
        {load.at === "loading" ? (
          <div className="flex flex-col gap-2" aria-busy="true">
            <Skeleton height={20} />
            <Skeleton height={48} />
          </div>
        ) : load.at === "error" ? (
          <Banner tone="error" action={{ label: t("worktree.overlap.retry"), onClick: retry }}>
            {t("worktree.overlap.failed", { reason: load.message })}
          </Banner>
        ) : (
          <>
            <p className="text-sm text-fg-muted">{t("worktree.overlap.files", { n: load.report.files })}</p>
            <section className="flex flex-col gap-2">
              <h3 className="text-sm font-medium text-fg">{t("worktree.overlap.siblingsHeading")}</h3>
              {load.report.siblings.length === 0 ? (
                <p data-testid="overlap-none" className="text-sm text-fg-subtle">
                  {t("worktree.overlap.none")}
                </p>
              ) : (
                <ul className="flex flex-col gap-2">
                  {load.report.siblings.map((s) => (
                    <li key={s.laneId} data-testid="overlap-sibling" data-branch={s.branch}>
                      <p className="text-sm text-fg">
                        {t("worktree.overlap.sibling", { branch: s.branch, n: s.files.length })}
                      </p>
                      <ul className="pl-4 font-mono text-xs text-fg-muted">
                        {s.files.map((f) => (
                          <li key={f} className="truncate">
                            {f}
                          </li>
                        ))}
                      </ul>
                    </li>
                  ))}
                </ul>
              )}
            </section>
            <section className="flex flex-col gap-2">
              <h3 className="text-sm font-medium text-fg">{t("worktree.overlap.forecastHeading", { base })}</h3>
              <ForecastVerdict report={load.report} />
            </section>
          </>
        )}
      </div>
    </Dialog>
  );
}

/** The merge confirm's forecast line: loading, a failure, or the verdict. */
export function MergeForecast({ laneId }: { laneId: string }) {
  const { load } = useOverlap(laneId);
  if (load.at === "loading")
    return (
      <p className="text-sm text-fg-subtle" aria-busy="true">
        {t("worktree.overlap.checking")}
      </p>
    );
  if (load.at === "error")
    return <p className="text-sm text-fg-muted">{t("worktree.overlap.failed", { reason: load.message })}</p>;
  return <ForecastVerdict report={load.report} />;
}
