import { useEffect, useRef } from "react";
import type { PaneRecord, PaneRestore } from "@tandem/protocol";
import { Button, EngineGlyph, Spinner, useToast } from "@tandem/ui";
import { t } from "../../i18n";
import { useEngines } from "../../stores/engines";
import { useRestore } from "../../stores/restore";
import { modalOpen } from "../shell/modal";
import { RestoredScrollback } from "./RestoredScrollback";
import type { TerminalOptions } from "./TerminalView";

/** UI-132 paused agent card: "Resume" / "Start fresh"; Ctrl+C cancels the
 * restore and leaves the saved output to read (F-SH-07). */
export function PausedCard({
  pane,
  title,
  card,
  focused,
  options,
}: {
  pane: PaneRecord;
  title: string;
  card: PaneRestore | undefined;
  focused: boolean;
  options: TerminalOptions;
}) {
  const root = useRef<HTMLDivElement>(null);
  const spec = useEngines((st) => st.list.find((e) => e.spec.id === pane.engineId)?.spec);
  const { show } = useToast();
  const state = card?.state ?? (pane.restoreState === "cancelled" ? "cancelled" : "paused");
  const canResume = card?.canResume ?? false;
  useEffect(() => {
    if (focused && !modalOpen()) root.current?.focus({ preventScroll: true });
  }, [focused]);

  const start = async (fresh: boolean) => {
    const error = await useRestore.getState().resume(pane.id, fresh);
    if (error) show({ tone: "error", title: error });
  };
  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.ctrlKey && !e.metaKey && !e.altKey && e.key.toLowerCase() === "c" && state !== "cancelled") {
      e.preventDefault();
      void useRestore.getState().cancel(pane.id);
    }
  };
  const actions = (
    <div className="flex items-center gap-2">
      {(state !== "cancelled" || canResume) && (
        <Button
          size="sm"
          variant="primary"
          disabled={!canResume || state === "queued"}
          data-testid="restore-resume"
          onClick={() => void start(false)}
        >
          {t("restore.resume")}
        </Button>
      )}
      <Button
        size="sm"
        disabled={state === "queued"}
        data-testid="restore-fresh"
        onClick={() => void start(true)}
      >
        {t("restore.startFresh")}
      </Button>
    </div>
  );

  return (
    <div
      ref={root}
      tabIndex={0}
      role="group"
      aria-label={t("restore.cardLabel", { name: title })}
      data-testid="paused-card"
      data-state={state}
      onKeyDown={onKeyDown}
      className="absolute inset-0 focus-visible:outline-none"
      style={{ background: options.theme.background }}
    >
      {card?.hasScrollback !== false && (
        <RestoredScrollback paneId={pane.id} options={options} dim={state !== "cancelled"} />
      )}
      {state === "cancelled" ? (
        <div
          role="status"
          className="absolute inset-x-0 bottom-0 flex items-center gap-3 border-t border-border bg-surface-3 px-3 py-2 text-sm"
        >
          <span className="flex-1 text-fg">{t("restore.cancelled")}</span>
          {actions}
        </div>
      ) : (
        <div className="absolute inset-0 flex items-center justify-center p-4">
          <div className="flex max-w-sm flex-col items-center gap-3 rounded-lg border border-border bg-surface-2 p-5 text-center shadow-lg">
            {spec && <EngineGlyph letters={spec.logo} size={32} />}
            <span className="line-clamp-2 text-base font-semibold text-fg" data-testid="paused-title">
              {title}
            </span>
            <span className="flex items-center gap-2 text-sm text-fg-muted" data-testid="paused-status">
              {state === "queued" && <Spinner size={14} />}
              {state === "queued" ? t("restore.queued") : t("restore.paused")}
            </span>
            {actions}
            {!canResume && card?.reason && (
              <span className="text-sm text-fg-muted" data-testid="restore-reason">
                {card.reason}
              </span>
            )}
            <span className="text-xs text-fg-subtle">{t("restore.cancelHint")}</span>
          </div>
        </div>
      )}
    </div>
  );
}
