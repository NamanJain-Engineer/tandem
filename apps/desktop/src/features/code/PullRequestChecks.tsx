import { useCallback, useEffect, useState } from "react";
import { Ban, Check, XCircle } from "lucide-react";
import type { LaneChecks, PrCheck, WorkspaceSummary } from "@tandem/protocol";
import { Badge, Banner, Button, Dialog, Skeleton, Spinner, cx } from "@tandem/ui";
import { t } from "../../i18n";
import {
  CHECKS_POLL_MS,
  allowChecks,
  checkFailurePrompt,
  loadChecks,
  pollChecks,
  useWorktrees,
} from "../../stores/worktrees";
import { SendToAgent } from "./SendToAgent";

/** F-WT-08: polls the worktrees' checks every 2 minutes while `active` and
 * the window is focused (see `pollChecks` for which), and once on start. */
export function useChecksPolling(laneIds: string[], active: boolean) {
  const key = laneIds.join(",");
  useEffect(() => {
    if (!active || !key) return;
    const ids = key.split(",");
    const tick = () => {
      if (typeof document === "undefined" || document.hasFocus()) void pollChecks(ids);
    };
    void pollChecks(ids);
    const timer = setInterval(tick, CHECKS_POLL_MS);
    return () => clearInterval(timer);
  }, [key, active]);
}

const failedOf = (c: LaneChecks | undefined): PrCheck[] =>
  c?.state === "ready" ? c.checks.filter((x) => x.bucket === "fail") : [];

/** A pull request's checks in one word: any failure wins, then anything
 * still running, then a cancelled run; "passed" only when none of those
 * (skipped checks don't count against it). */
export function checksState(checks: PrCheck[]): "failed" | "running" | "cancelled" | "passed" {
  if (checks.some((c) => c.bucket === "fail")) return "failed";
  if (checks.some((c) => c.bucket === "pending")) return "running";
  if (checks.some((c) => c.bucket === "cancel")) return "cancelled";
  return "passed";
}

/** The badge's words: the status bar's, without its count (D-2700). */
const BADGE_WORDS = {
  failed: "checks.badge.failed",
  running: "status.ciRunning",
  cancelled: "status.ciCancelled",
  passed: "status.ciPassed",
} as const;

/** The rail row's summary: failed, running, cancelled or passed; nothing
 * otherwise. Its icon leads, and its words are the status bar's ("CI
 * failed"), whole or not at all (D-2700). Beside the row's status ("● 1
 * working", "Needs you", "Failed"), or when the row folds it, it's the icon
 * alone (D-2613, D-2614). Its name starts with its words and says which
 * checks failed (WCAG 2.5.3); its title is the checks' summary. */
export function ChecksBadge({
  wt,
  beside,
  fold = false,
}: {
  wt: WorkspaceSummary;
  beside: boolean;
  fold?: boolean;
}) {
  const checks = useWorktrees((s) => s.checks[wt.id]);
  if (checks?.state !== "ready" || !checks.pr || checks.checks.length === 0) return null;
  const failed = failedOf(checks);
  const open = () => useWorktrees.getState().open({ kind: "checks", worktreeId: wt.id });
  const state = checksState(checks.checks);
  const compact = fold || beside;
  const words = t(BADGE_WORDS[state]);
  const summary =
    state === "failed"
      ? t("checks.failed", { n: failed.length, names: failed.map((c) => c.name).join(", ") })
      : t(state === "running" ? "checks.running" : state === "cancelled" ? "checks.cancelled" : "checks.passed");
  return (
    <button
      type="button"
      data-testid="worktree-checks"
      data-state={state}
      data-compact={compact ? "" : undefined}
      aria-label={state === "failed" ? t("checks.badge.name", { words, summary }) : words}
      title={summary}
      onClick={(e) => {
        e.stopPropagation();
        open();
      }}
      onKeyDown={(e) => e.stopPropagation()}
      className={cx(
        "inline-flex h-7 shrink-0 items-center gap-1 whitespace-nowrap rounded-sm px-1 text-sm font-medium hover:bg-surface-4 focus-visible:shadow-focus focus-visible:outline-none",
        state === "failed" ? "text-danger" : state === "running" ? "text-fg-subtle" : state === "cancelled" ? "text-fg-muted" : "text-success",
      )}
    >
      {state === "failed" ? (
        <XCircle size={12} aria-hidden="true" className="shrink-0" />
      ) : state === "running" ? (
        <Spinner size={10} className="shrink-0" />
      ) : state === "cancelled" ? (
        <Ban size={12} aria-hidden="true" className="shrink-0" />
      ) : (
        <Check size={12} aria-hidden="true" className="shrink-0" />
      )}
      {!compact && <span data-words="">{words}</span>}
    </button>
  );
}

/** UI-156 "Check pull requests for this repository?" */
export function AllowChecksDialog({
  wt,
  folder,
  onClose,
}: {
  wt: WorkspaceSummary;
  folder: string;
  onClose: () => void;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const allow = async () => {
    setBusy(true);
    setError(null);
    const out = await allowChecks(wt.id, true);
    setBusy(false);
    if (!out.ok) {
      setError(out.error.message);
      return;
    }
    useWorktrees.getState().open({ kind: "checks", worktreeId: wt.id });
  };
  return (
    <Dialog
      open
      onOpenChange={(o) => !o && onClose()}
      title={t("checks.allowTitle")}
      closeLabel={t("common.close")}
      footer={
        <>
          <Button variant="secondary" onClick={onClose} data-testid="checks-not-now">
            {t("checks.notNow")}
          </Button>
          <Button variant="primary" loading={busy} onClick={() => void allow()} data-testid="checks-allow">
            {t("checks.allow")}
          </Button>
        </>
      }
    >
      <div data-testid="checks-ask" className="flex flex-col gap-3">
        <p className="text-sm text-fg">{t("checks.allowBody", { folder })}</p>
        {error ? <Banner tone="error">{error}</Banner> : null}
      </div>
    </Dialog>
  );
}

const BUCKET_VARIANT = {
  pass: "done",
  fail: "error",
  pending: "outline",
  skipping: "outline",
  cancel: "outline",
} as const;

/** UI-157 "Checks for {branch}". */
export function ChecksDialog({ wt, onClose }: { wt: WorkspaceSummary; onClose: () => void }) {
  const checks = useWorktrees((s) => s.checks[wt.id]);
  const [loading, setLoading] = useState(true);
  const refresh = useCallback(() => {
    setLoading(true);
    void loadChecks([wt.id]).then(() => setLoading(false));
  }, [wt.id]);
  useEffect(() => refresh(), [refresh]);
  const branch = wt.laneBranch ?? wt.name;

  let body: React.ReactNode;
  if (loading && !checks)
    body = (
      <div className="flex flex-col gap-2" aria-busy="true">
        <Skeleton height={28} />
        <Skeleton height={28} />
      </div>
    );
  else if (!checks || checks.state === "error")
    body = (
      <Banner tone="error" action={{ label: t("worktree.setup.retry"), onClick: refresh }}>
        {checks?.error ?? ""}
      </Banner>
    );
  else if (checks.state === "no_gh") body = <Banner tone="warning">{t("checks.noGh")}</Banner>;
  else if (checks.state === "gitlab") body = <p className="text-sm text-fg-subtle">{t("checks.gitlab")}</p>;
  else if (checks.state === "no_pr" || checks.state === "not_allowed")
    body = <p className="text-sm text-fg-subtle">{t("checks.noPr")}</p>;
  else if (checks.checks.length === 0)
    body = (
      <p data-testid="checks-empty" className="text-sm text-fg-subtle">
        {t("checks.empty")}
      </p>
    );
  else
    body = (
      <ul className="flex flex-col gap-2">
        {checks.checks.map((c) => (
          <li
            key={`${c.workflow ?? ""}/${c.name}`}
            data-testid="check-row"
            data-name={c.name}
            data-bucket={c.bucket}
            className="flex flex-col gap-1 rounded-sm border border-border px-2 py-1.5"
          >
            <div className="flex items-center gap-2 text-sm">
              <span className="truncate text-fg">{c.name}</span>
              {c.workflow ? <span className="truncate text-xs text-fg-subtle">{c.workflow}</span> : null}
              <Badge variant={BUCKET_VARIANT[c.bucket]} className="ml-auto">
                {t(`checks.bucket.${c.bucket}`)}
              </Badge>
            </div>
            {c.bucket === "fail" ? (
              <SendToAgent laneId={wt.id} testId="check" prompt={() => checkFailurePrompt(wt.id, c.name)} />
            ) : null}
          </li>
        ))}
      </ul>
    );

  return (
    <Dialog
      open
      onOpenChange={(o) => !o && onClose()}
      title={t("checks.title", { branch })}
      description={checks?.pr ? t("checks.pr", { n: checks.pr.number }) : undefined}
      closeLabel={t("common.close")}
      footer={
        <>
          <Button variant="secondary" loading={loading} onClick={refresh} data-testid="checks-refresh">
            {t("checks.refresh")}
          </Button>
          <Button variant="primary" onClick={onClose}>
            {t("common.close")}
          </Button>
        </>
      }
    >
      <div data-testid="checks" data-state={checks?.state ?? "loading"} className="flex flex-col gap-3">
        {body}
      </div>
    </Dialog>
  );
}
