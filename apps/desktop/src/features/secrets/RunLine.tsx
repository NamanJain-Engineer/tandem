import { t } from "../../i18n";
import type { RunApproval } from "./runApproval";

/** The whole line a `vibezeo run` asks for, wrapped and never cut, with
 * every variable it gets: shown wherever Allow is offered. */
export function RunLine({ run }: { run: RunApproval }) {
  return (
    <div className="flex flex-col gap-1" data-testid="run-line">
      <pre className="max-h-48 overflow-y-auto whitespace-pre-wrap break-all rounded-sm border border-border bg-surface-1 px-2 py-1 font-mono text-sm text-fg">
        {run.command}
      </pre>
      <p className="text-xs text-fg-muted" data-testid="run-line-vars">
        {run.vars.length === 0
          ? t("runLine.noVars")
          : t("runLine.vars", {
              vars: run.vars.map((v) => (v.secret ? `${v.name} (${v.secret})` : v.name)).join(", "),
            })}
      </p>
      {run.envChanged && <p className="text-xs text-warning-text">{t("runLine.envChanged")}</p>}
    </div>
  );
}
