import { useMemo, useState } from "react";
import type { IpcError } from "@tandem/protocol";
import { Button, Select, useToast } from "@tandem/ui";
import { t } from "../../i18n";
import { invoke } from "../../ipc/invoke";
import { useDashboard } from "../../stores/dashboard";
import { agentName, worktreeAgents } from "./WorktreeReview";

type Prompt = string | (() => Promise<{ ok: true; value: string } | { ok: false; error: IpcError }>);

/** "Send failure to agent" (F-WT-07, F-WT-08): types `prompt` into an agent
 * running in the worktree (chosen as F-WT-02's review bar does), or offers
 * Copy when none runs there. Only ever on a click; the prompt can be read
 * from the core at that moment. */
export function SendToAgent({ laneId, prompt, testId }: { laneId: string; prompt: Prompt; testId: string }) {
  const items = useDashboard((s) => s.items);
  const agents = useMemo(() => worktreeAgents(items, laneId), [items, laneId]);
  const [target, setTarget] = useState<string>("");
  const [busy, setBusy] = useState(false);
  const { show } = useToast();
  const chosen = agents.find((a) => a.key === target) ?? agents[0];

  const text = async (): Promise<string | null> => {
    if (typeof prompt === "string") return prompt;
    const out = await prompt();
    if (out.ok) return out.value;
    show({ tone: "error", title: t("failure.sendFailed", { reason: out.error.message }) });
    return null;
  };
  const copy = async () => {
    setBusy(true);
    const value = await text();
    setBusy(false);
    if (value === null) return;
    void navigator.clipboard?.writeText(value);
    show({ title: t("failure.copied") });
  };
  const send = async () => {
    if (!chosen || chosen.source.type !== "pane") return;
    setBusy(true);
    try {
      const value = await text();
      if (value === null) return;
      await invoke("pty_type_prompt", { paneId: chosen.source.paneId, text: value });
      show({ tone: "success", title: t("failure.sent", { name: agentName(chosen) }) });
    } catch (e) {
      show({ tone: "error", title: t("failure.sendFailed", { reason: (e as IpcError).message }) });
    } finally {
      setBusy(false);
    }
  };

  if (!chosen)
    return (
      <Button
        size="sm"
        variant="secondary"
        loading={busy}
        data-testid={`${testId}-copy`}
        onClick={() => void copy()}
      >
        {t("failure.copy")}
      </Button>
    );
  return (
    <div className="flex flex-wrap items-center gap-2">
      {agents.length > 1 && (
        <Select
          size="sm"
          ariaLabel={t("worktree.review.agent")}
          value={chosen.key}
          onValueChange={setTarget}
          options={agents.map((a) => ({ value: a.key, label: agentName(a) }))}
        />
      )}
      <Button
        size="sm"
        variant="secondary"
        loading={busy}
        onClick={() => void send()}
        data-testid={`${testId}-send`}
      >
        {t("failure.send")}
      </Button>
    </div>
  );
}
