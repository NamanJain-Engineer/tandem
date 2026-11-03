import { t } from "../../i18n";
import { markLine, type RunApproval } from "./runApproval";

/** The whole line a `vibezeo run` asks for, wrapped and never cut or
 * scrolled, left to right whatever its text, with every character outside
 * plain ASCII marked, the folder it runs in and every variable it gets:
 * shown wherever Allow is offered.
 *
 * `endRef` takes a marker over the command's last line, the whole width
 * and one line high: all of it in view is the command's end seen. It sat
 * over the run's last line instead, the variables, and a jump to the end
 * turned Allow on with the command's tail never on screen (the review of
 * b3ff3696, M-1). */
export function RunLine({ run, endRef }: { run: RunApproval; endRef?: (el: HTMLElement | null) => void }) {
  const pieces = markLine(run.command);
  const folder = markLine(run.cwd);
  const marked = pieces.some((p) => p.marked) || folder.some((p) => p.marked);
  return (
    <div className="flex flex-col gap-1" data-testid="run-line">
      <pre
        dir="ltr"
        style={{ unicodeBidi: "bidi-override" }}
        className="relative whitespace-pre-wrap break-all rounded-sm border border-border bg-surface-1 px-2 py-1 font-mono text-sm text-fg"
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
        {endRef && (
          // The last line box: the `<pre>` is its containing block
          // (`relative`), `bottom-1` is the content's bottom (above `py-1`),
          // the full width, and one line high (`1lh`, the line's own
          // height). An engine without `lh` drops that height, and the
          // minimum keeps it one `text-sm` line, not 0 px (the review of
          // 8f35e5d0, S-2).
          <span
            ref={endRef}
            aria-hidden="true"
            data-testid="run-line-end"
            className="pointer-events-none absolute inset-x-0 bottom-1"
            style={{ height: "1lh", minHeight: "var(--text-sm--line-height)" }}
          />
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
      <p dir="ltr" className="break-all text-xs text-fg-muted" data-testid="run-line-vars">
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
