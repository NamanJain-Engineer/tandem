import { useState } from "react";
import { KeyRound } from "lucide-react";
import { Button, useToast } from "@tandem/ui";
import { t } from "../../i18n";
import { useDashboard } from "../../stores/dashboard";
import { answer } from "../dashboard/actions";

/** A pane's pending approval, on the pane itself: what an agent's
 * `vibezeo run` wants (F-TL-12), with Allow once, Allow for this agent
 * (that exact command, until the pane closes) and Deny. */
export function ApprovalBanner({ paneId }: { paneId: string }) {
  const approval = useDashboard((s) => s.items[`pane:${paneId}`]?.pendingApproval ?? null);
  const [busy, setBusy] = useState(false);
  const { show } = useToast();
  if (!approval) return null;
  const decide = async (allow: boolean, scope: "once" | "conversation" = "once") => {
    setBusy(true);
    const err = await answer(approval.id, allow, "inline", scope);
    setBusy(false);
    if (err) show({ tone: "error", title: err.message });
  };
  return (
    <div
      data-testid="pane-approval"
      role="status"
      className="flex min-h-8 shrink-0 flex-wrap items-center gap-2 border-b border-divider bg-waiting-soft px-2 py-1 text-sm text-fg"
    >
      <KeyRound size={14} aria-hidden="true" className="shrink-0 text-warning-text" />
      <span
        className="min-w-[220px] flex-1 truncate"
        title={approval.detail}
        data-testid="pane-approval-summary"
      >
        {t("dashboard.wants", { summary: approval.summary })}
      </span>
      <Button
        size="sm"
        variant="primary"
        loading={busy}
        data-testid="pane-approval-once"
        onClick={() => void decide(true)}
      >
        {t("dashboard.allowOnce")}
      </Button>
      {approval.options.includes("conversation") && (
        <Button
          size="sm"
          variant="secondary"
          disabled={busy}
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
  );
}
