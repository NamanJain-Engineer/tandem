import { useState } from "react";
import { KeyRound } from "lucide-react";
import { Button, useToast } from "@tandem/ui";
import { t } from "../../i18n";
import { useDashboard } from "../../stores/dashboard";
import { answer } from "../dashboard/actions";
import { RunLine } from "../secrets/RunLine";
import { parseRunApproval } from "../secrets/runApproval";
import { ApprovalBoundary, useSeenEnd, useSettled } from "../secrets/ApprovalGuard";

/** A pane's pending approval, on the pane itself: what an agent's
 * `vibezeo run` wants (F-TL-12), then Allow once, Allow for this agent
 * (that exact command in that folder, until the pane closes) and Deny.
 * The banner scrolls within the pane, and Allow waits until the line's end
 * has been in view. A pane approval that can't be read offers only Deny. */
export function ApprovalBanner({ paneId }: { paneId: string }) {
  const approval = useDashboard((s) => s.items[`pane:${paneId}`]?.pendingApproval ?? null);
  const [busy, setBusy] = useState(false);
  const { show } = useToast();
  const key = approval ? `${approval.id}\n${approval.detail}` : null;
  const settled = useSettled(key);
  const [endRef, seenEnd] = useSeenEnd(key);
  if (!approval) return null;
  const run = parseRunApproval(approval);
  const unreadable = approval.source.type === "pane" && !run;
  const canAllow = settled && seenEnd && !unreadable;
  const decide = async (allow: boolean, scope: "once" | "conversation" = "once") => {
    setBusy(true);
    const err = await answer(approval.id, allow, "inline", scope);
    setBusy(false);
    if (err) show({ tone: "error", title: err.message });
  };
  return (
    <ApprovalBoundary key={approval.id} onDeny={() => void decide(false)}>
      <div
        data-testid="pane-approval"
        role="status"
        className="flex max-h-[60%] min-h-8 shrink-0 flex-col gap-2 overflow-y-auto border-b border-divider bg-waiting-soft px-2 py-1 text-sm text-fg"
      >
        <div className="flex items-start gap-2">
          <KeyRound size={14} aria-hidden="true" className="mt-0.5 shrink-0 text-warning-text" />
          <div className="flex min-w-0 flex-1 flex-col gap-1">
            <span className="break-words" data-testid="pane-approval-summary">
              {unreadable
                ? t("approval.unreadable")
                : run
                  ? t("runLine.title")
                  : t("dashboard.wants", { summary: approval.summary })}
            </span>
            {run && <RunLine run={run} />}
            <span ref={endRef} aria-hidden="true" data-testid="run-line-end" />
          </div>
        </div>
        <div className="flex flex-wrap gap-2">
          {!unreadable && (
            <Button
              size="sm"
              variant="primary"
              loading={busy}
              disabled={!canAllow}
              data-testid="pane-approval-once"
              onClick={() => void decide(true)}
            >
              {t("dashboard.allowOnce")}
            </Button>
          )}
          {!unreadable && approval.options.includes("conversation") && (
            <Button
              size="sm"
              variant="secondary"
              disabled={busy || !canAllow}
              data-testid="pane-approval-agent"
              onClick={() => void decide(true, "conversation")}
            >
              {t("dashboard.allowAgent")}
            </Button>
          )}
          <Button
            size="sm"
            variant="ghost"
            disabled={busy}
            data-testid="pane-approval-deny"
            onClick={() => void decide(false)}
          >
            {t("dashboard.deny")}
          </Button>
        </div>
      </div>
    </ApprovalBoundary>
  );
}
