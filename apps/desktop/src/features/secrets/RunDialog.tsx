import { useEffect, useState } from "react";
import type { IpcError, RunPlan } from "@tandem/protocol";
import { Button, Callout, Dialog, Input, Skeleton } from "@tandem/ui";
import { t } from "../../i18n";
import { useLayout } from "../../stores/layout";
import { runPlan, runWithSecrets } from "../../stores/secrets";

/** The folder's last segment ("checkout"). */
function folderName(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}

/** Run with secrets (F-TL-12): a command in a new pane beside `paneId`, with
 * the folder's `.env`, references resolved by the core. */
export function RunDialog({
  paneId,
  tabId,
  open,
  onOpenChange,
}: {
  paneId: string;
  tabId: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const [plan, setPlan] = useState<RunPlan | null>(null);
  const [planError, setPlanError] = useState<string | null>(null);
  const [command, setCommand] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [running, setRunning] = useState(false);
  useEffect(() => {
    if (!open) return;
    let live = true;
    setPlan(null);
    setPlanError(null);
    setError(null);
    runPlan(paneId)
      .then((p) => live && (p ? setPlan(p) : setPlanError(t("run.unavailable"))))
      .catch((e: IpcError) => live && setPlanError(e.message ?? String(e)));
    return () => {
      live = false;
    };
  }, [open, paneId]);
  const missing = plan?.vars.find((v) => v.missing)?.secret ?? null;
  const ready = plan !== null && missing === null && command.trim() !== "";
  const run = async () => {
    setRunning(true);
    setError(null);
    try {
      const pane = await runWithSecrets(paneId, tabId, command.trim());
      useLayout.getState().insert(tabId, pane, paneId, "right");
      onOpenChange(false);
    } catch (e) {
      setError((e as IpcError).message ?? t("run.failed"));
    } finally {
      setRunning(false);
    }
  };
  return (
    <Dialog
      open={open}
      onOpenChange={onOpenChange}
      title={t("run.title")}
      footer={
        <>
          <Button variant="ghost" onClick={() => onOpenChange(false)}>
            {t("common.cancel")}
          </Button>
          <Button
            variant="primary"
            data-testid="run-start"
            loading={running}
            disabled={!ready}
            onClick={() => void run()}
          >
            {t("run.start")}
          </Button>
        </>
      }
    >
      <form
        className="flex flex-col gap-4"
        data-testid="run-dialog"
        onSubmit={(e) => {
          e.preventDefault();
          if (ready) void run();
        }}
      >
        <Input
          label={t("run.command")}
          data-testid="run-command"
          value={command}
          onChange={(e) => setCommand(e.target.value)}
          placeholder="pnpm dev"
          monospace
          spellCheck={false}
          autoFocus
        />
        {planError ? (
          <Callout tone="error">{planError}</Callout>
        ) : plan === null ? (
          <Skeleton className="h-12" />
        ) : plan.envFile === null ? (
          <p className="text-sm text-fg-muted" data-testid="run-no-env">
            {t("run.noEnv")}
          </p>
        ) : (
          <div className="flex flex-col gap-1.5" data-testid="run-plan">
            <p className="text-sm text-fg-muted">{t("run.uses", { folder: folderName(plan.folder) })}</p>
            <ul className="flex flex-col gap-0.5 font-mono text-sm">
              {plan.vars.map((v) => (
                <li
                  key={v.name}
                  data-testid="run-var"
                  data-name={v.name}
                  data-missing={v.missing ? "true" : "false"}
                >
                  {v.secret ? (
                    <span className={v.missing ? "text-danger" : "text-fg"}>
                      {t("run.var", { name: v.name, secret: v.secret })}
                      {v.missing && (
                        <span className="ml-2 font-sans">{t("run.missing", { name: v.secret })}</span>
                      )}
                    </span>
                  ) : (
                    <span className="text-fg-muted">{v.name}</span>
                  )}
                </li>
              ))}
            </ul>
          </div>
        )}
        {error && <Callout tone="error">{error}</Callout>}
        <p className="text-xs text-fg-subtle">{t("run.footnote")}</p>
      </form>
    </Dialog>
  );
}
