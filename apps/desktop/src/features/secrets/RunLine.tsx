import { t } from "../../i18n";
import { markLine, type RunApproval } from "./runApproval";

/** The whole line a `vibezeo run` asks for, wrapped and never cut or
 * scrolled, left to right whatever its text, with every character outside
 * plain ASCII marked, the folder it runs in and every variable it gets:
 * shown wherever Allow is offered. */
export function RunLine({ run }: { run: RunApproval }) {
  const pieces = markLine(run.command);
  const folder = markLine(run.cwd);
  const marked = pieces.some((p) => p.marked) || folder.some((p) => p.marked);
  return (
    <div className="flex flex-col gap-1" data-testid="run-line">
      <pre
        dir="ltr"
        style={{ unicodeBidi: "bidi-override" }}
        className="whitespace-pre-wrap break-all rounded-sm border border-border bg-surface-1 px-2 py-1 font-mono text-sm text-fg"
        data-testid="run-line-command"
      >
        {pieces.map((p, i) =>
          p.marked ? (
            <mark key={i} className="rounded-sm bg-waiting-soft text-warning-text">
              {p.text}
            </mark>
          ) : (
            p.text
          ),
        )}
      </pre>
      {marked && (
        <p className="text-xs text-warning-text" data-testid="run-line-marked">
          {t("runLine.marked")}
        </p>
      )}
      <p dir="ltr" className="break-all text-xs text-fg-muted" data-testid="run-line-cwd">
        {t("runLine.cwd", {
          cwd: folder.map((p) => p.text).join(""),
        })}
      </p>
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
