import { useEffect, useState } from "react";
import type { PrStep, PrsRun, WorkspaceSummary } from "@tandem/protocol";
import { Button, Dialog, Spinner } from "@tandem/ui";
import { t } from "../../i18n";
import { PRS_POLL_MS, pullRequestsRun } from "../../stores/worktrees";

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
  useEffect(() => {
    let live = true;
    const read = () =>
      void pullRequestsRun(wt.id).then((out) => {
        if (live && out.ok) setRun(out.value);
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
      <ul data-testid="prs" data-running={run?.running ? "true" : "false"} className="flex flex-col gap-2">
        {steps.map((s) => (
          <li
            key={s.repo}
            data-testid="prs-step"
            data-repo={s.repo}
            data-state={s.state}
            className="flex items-center gap-2 rounded-sm border border-border px-2 py-1.5 text-sm"
          >
            {BUSY.has(s.state) && run?.running !== false ? <Spinner size={12} /> : null}
            <span className="font-mono text-fg">{s.repo}</span>
            <span className="ml-auto truncate text-fg-muted">{stepText(s)}</span>
            {s.url ? (
              <a
                href={s.url}
                target="_blank"
                rel="noreferrer"
                className="text-accent underline"
                data-testid="prs-link"
              >
                {t("prs.link")}
              </a>
            ) : null}
          </li>
        ))}
      </ul>
    </Dialog>
  );
}
