import { useCallback, useEffect, useState } from "react";
import { AlertTriangle, Check } from "lucide-react";
import type { OverlapReport, SharedSymbol, SymbolChange, WorkspaceSummary } from "@tandem/protocol";
import { Banner, Button, Dialog, Skeleton, useToast } from "@tandem/ui";
import { t } from "../../i18n";
import { listen } from "../../ipc/events";
import { useWorkspaces } from "../../stores/workspaces";
import { loadRadar, radarClaim, worktreeOverlap } from "../../stores/worktrees";

type Load = { at: "loading" } | { at: "error"; message: string } | { at: "ready"; report: OverlapReport };

/** Loads `lane_overlap` for a worktree (F-WT-03). */
export function useOverlap(laneId: string): { load: Load; retry: () => void } {
  const [load, setLoad] = useState<Load>({ at: "loading" });
  // `quiet`: a refetch keeps showing the last report until the new one.
  const fetch = useCallback(
    (quiet = false) => {
      let live = true;
      if (!quiet) setLoad({ at: "loading" });
      void worktreeOverlap(laneId).then((out) => {
        if (!live) return;
        setLoad(out.ok ? { at: "ready", report: out.value } : { at: "error", message: out.error.message });
        // F-WT-10: the check parsed this worktree and its siblings again.
        if (out.ok) {
          const ids = useWorkspaces
            .getState()
            .list.filter((w) => w.parentId !== null && w.laneBranch !== null && w.kind === "local")
            .map((w) => w.id);
          void loadRadar(ids);
        }
      });
      return () => {
        live = false;
      };
    },
    [laneId],
  );
  useEffect(() => fetch(), [fetch]);
  // F-WT-10: a watched change updated the symbols of this worktree or one of
  // its siblings; read them again.
  useEffect(() => {
    let live = true;
    let off: (() => void) | undefined;
    void listen<{ laneIds: string[] }>("radar:changed", (e) => {
      if (!live) return;
      const list = useWorkspaces.getState().list;
      const parent = list.find((w) => w.id === laneId)?.parentId ?? null;
      const ours = (id: string) =>
        id === laneId || (parent !== null && list.some((w) => w.id === id && w.parentId === parent));
      if (e.laneIds.some(ours)) fetch(true);
    }).then((u) => {
      if (live) off = u;
      else u();
    });
    return () => {
      live = false;
      off?.();
    };
  }, [fetch, laneId]);
  return { load, retry: () => fetch() };
}

/** What each side did to a shared symbol (F-WT-10). */
function symbolText(s: SharedSymbol): string {
  const did = (c: SymbolChange) =>
    c === "renamed" && s.renamedTo
      ? t("radar.change.renamedTo", { name: s.renamedTo })
      : t(`radar.change.${c}`);
  return t("radar.symbol", { here: did(s.here), there: did(s.there) });
}

/** "Claim these files" (F-WT-10): claims, for this worktree, the files where
 * it changed what it shares with the sibling, as F-TK-05 claims. Only on the
 * click. */
function ClaimFiles({ laneId, otherId }: { laneId: string; otherId: string }) {
  const { show } = useToast();
  const [busy, setBusy] = useState(false);
  return (
    <Button
      size="sm"
      variant="ghost"
      data-testid="overlap-claim"
      disabled={busy}
      onClick={() => {
        setBusy(true);
        void radarClaim(laneId, otherId).then((out) => {
          setBusy(false);
          if (out.ok) show({ tone: "success", title: t("radar.claimed", { n: out.value.length }) });
          else show({ tone: "error", title: t("radar.claimFailed", { reason: out.error.message }) });
        });
      }}
    >
      {t("radar.claim")}
    </Button>
  );
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
        <p className="text-xs text-fg-subtle">
          {t("worktree.overlap.uncommitted", { n: report.uncommitted })}
        </p>
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
                        {s.files.length > 0
                          ? t("worktree.overlap.sibling", { branch: s.branch, n: s.files.length })
                          : t("radar.siblingSymbols", { branch: s.branch })}
                      </p>
                      <ul className="pl-4 font-mono text-xs text-fg-muted">
                        {s.files.map((f) => (
                          <li key={f} className="truncate">
                            {f}
                          </li>
                        ))}
                      </ul>
                      {s.symbols.length > 0 ? (
                        <div className="mt-1 flex flex-col gap-0.5 pl-4" data-testid="overlap-symbols">
                          <p className="text-xs font-medium text-fg">{t("radar.sharedHeading")}</p>
                          {s.symbols.some((sym) => sym.here !== "used" && sym.here !== "added") ? (
                            <div className="self-start">
                              <ClaimFiles laneId={wt.id} otherId={s.laneId} />
                            </div>
                          ) : null}
                          <ul className="flex flex-col gap-0.5 text-xs">
                            {s.symbols.map((sym) => (
                              <li
                                key={sym.qualified}
                                data-testid="overlap-symbol"
                                data-symbol={sym.qualified}
                                className="flex gap-2"
                              >
                                <span className="truncate font-mono text-fg">{sym.qualified}</span>
                                <span className="text-fg-muted">{symbolText(sym)}</span>
                              </li>
                            ))}
                          </ul>
                        </div>
                      ) : null}
                    </li>
                  ))}
                </ul>
              )}
            </section>
            {load.report.unparsed > 0 ? (
              <p className="text-xs text-fg-subtle" data-testid="overlap-unparsed">
                {t("radar.unparsed", { n: load.report.unparsed })}
              </p>
            ) : null}
            <section className="flex flex-col gap-2">
              <h3 className="text-sm font-medium text-fg">
                {t("worktree.overlap.forecastHeading", { base })}
              </h3>
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
