import { useEffect, useState } from "react";
import type { PrStep, PrsRun, WorkspaceSummary } from "@tandem/protocol";
import { Banner, Button, Dialog, Spinner } from "@tandem/ui";
import { t } from "../../i18n";
import { invoke } from "../../ipc/invoke";
import { PRS_POLL_MS, pullRequestsRun } from "../../stores/worktrees";

/** Pull request pages open in the system browser, through the core. */
const openExternal = (url: string) => void invoke("app_open_external", { url }).catch(() => undefined);

function stepText(s: PrStep): string {
  switch (s.state) {
    case "waiting":
      return t("prs.step.waiting");
    case "pushing":
      return t("prs.step.pushing");
    case "creating":
      return t("prs.step.creating");
    case "linking":
      return t("prs.step.linking");
    case "done":
      return s.reason ? t("prs.step.openedNote", { reason: s.reason }) : t("prs.step.opened");
    case "skipped":
      return s.reason === "no new commits" ? t("prs.step.noCommits") : t("prs.step.gitlab");
    case "failed":
      return t("prs.step.failed", { reason: s.reason ?? "" });
  }
}

const BUSY = new Set(["waiting", "pushing", "creating", "linking"]);

/** UI-158 "Pull requests for {branch}" (F-CD-22): each repository's step,
 * polled while the run lasts. */
export function PullRequestsDialog({ wt, onClose }: { wt: WorkspaceSummary; onClose: () => void }) {
  const [run, setRun] = useState<PrsRun | null>(null);
  /** The run was read and there's none (the app restarted), or reading failed. */
  const [gone, setGone] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    const read = () =>
      void pullRequestsRun(wt.id).then((out) => {
        if (!live) return;
        if (!out.ok) {
          setError(out.error.message);
          return;
        }
        setError(null);
        setRun(out.value);
        setGone(out.value === null);
      });
    read();
    const timer = setInterval(read, PRS_POLL_MS);
    return () => {
      live = false;
      clearInterval(timer);
    };
  }, [wt.id]);
  const steps =
    run?.steps ??
    (wt.repos ?? []).map((repo) => ({ repo, state: "waiting", url: null, reason: null }) as PrStep);
  return (
    <Dialog
      open
      onOpenChange={(o) => !o && onClose()}
      title={t("prs.title", { branch: wt.laneBranch ?? wt.name })}
      closeLabel={t("common.close")}
      footer={
        <Button variant="primary" onClick={onClose}>
          {t("common.close")}
        </Button>
      }
    >
      {error && (
        <Banner tone="error">
          <span data-testid="prs-error">{t("prs.error", { reason: error })}</span>
        </Banner>
      )}
      {gone && !error && (
        <p className="text-sm text-fg-muted" data-testid="prs-gone">
          {t("prs.gone")}
        </p>
      )}
      <ul data-testid="prs" data-running={run?.running ? "true" : "false"} className="flex flex-col gap-2">
        {steps.map((s) => (
          <li
            key={s.repo}
            data-testid="prs-step"
            data-repo={s.repo}
            data-state={s.state}
            className="flex items-center gap-2 rounded-sm border border-border px-2 py-1.5 text-sm"
          >
            {BUSY.has(s.state) && run?.running === true && !error ? <Spinner size={12} /> : null}
            <span className="font-mono text-fg">{s.repo}</span>
            <span className="ml-auto truncate text-fg-muted">{stepText(s)}</span>
            {s.url ? (
              <Button
                size="sm"
                variant="ghost"
                data-testid="prs-link"
                onClick={() => openExternal(s.url ?? "")}
              >
                {t("prs.link")}
              </Button>
            ) : null}
          </li>
        ))}
      </ul>
    </Dialog>
  );
}
