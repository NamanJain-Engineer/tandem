import { useEffect, useState } from "react";
import type { AgentsMdPlan, IpcError } from "@tandem/protocol";
import { Button, Callout, Dialog, DiffView, Skeleton, useToast } from "@tandem/ui";
import { t } from "../../i18n";
import { agentsApply, agentsPlan } from "../../stores/secrets";

function count(patch: string, sign: "+" | "-"): number {
  return patch.split("\n").filter((l) => l.startsWith(sign) && !l.startsWith(`${sign}${sign}${sign}`)).length;
}

/** "Tell agents in this folder" (F-TL-12): the AGENTS.md change, shown
 * first, written only on Add, and only if the file is still what was shown. */
export function TellAgents({
  paneId,
  open,
  onOpenChange,
}: {
  paneId: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const { show } = useToast();
  const [plan, setPlan] = useState<AgentsMdPlan | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  useEffect(() => {
    if (!open) return;
    let live = true;
    setPlan(null);
    setError(null);
    agentsPlan(paneId)
      .then((p) => live && setPlan(p))
      .catch((e: IpcError) => live && setError(e.message ?? String(e)));
    return () => {
      live = false;
    };
  }, [open, paneId]);
  const apply = async () => {
    if (!plan) return;
    setSaving(true);
    setError(null);
    try {
      await agentsApply(paneId, plan.before);
      show({ title: t("tellAgents.done") });
      onOpenChange(false);
    } catch (e) {
      setError((e as IpcError).message ?? t("tellAgents.failed"));
    } finally {
      setSaving(false);
    }
  };
  return (
    <Dialog
      open={open}
      onOpenChange={onOpenChange}
      title={t("tellAgents.title")}
      footer={
        <>
          <Button variant="ghost" onClick={() => onOpenChange(false)}>
            {t("common.cancel")}
          </Button>
          <Button
            variant="primary"
            data-testid="tell-agents-apply"
            loading={saving}
            disabled={!plan || plan.unchanged}
            onClick={() => void apply()}
          >
            {t("tellAgents.apply")}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-3" data-testid="tell-agents">
        <p className="text-sm text-fg-muted">{t("tellAgents.body")}</p>
        {error && <Callout tone="error">{error}</Callout>}
        {plan === null ? (
          !error && <Skeleton className="h-24" />
        ) : plan.unchanged ? (
          <p className="text-sm text-fg" data-testid="tell-agents-unchanged">
            {t("tellAgents.unchanged")}
          </p>
        ) : (
          <DiffView
            files={[
              {
                path: "AGENTS.md",
                change: plan.before === "" ? "added" : "modified",
                additions: count(plan.patch, "+"),
                deletions: count(plan.patch, "-"),
                binary: false,
                patch: plan.patch,
                truncated: false,
              },
            ]}
          />
        )}
      </div>
    </Dialog>
  );
}
