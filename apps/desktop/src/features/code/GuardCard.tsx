import { useState } from "react";
import { ShieldAlert } from "lucide-react";
import type { GuardCard as Card } from "@tandem/protocol";
import { Button, useToast } from "@tandem/ui";
import { t } from "../../i18n";
import { dismissGuard, pauseAgent, resumeAgent, useRunaway } from "../../stores/runaway";

/** The card's line for its state (UI-170). */
export function guardLine(card: Card): string {
  switch (card.state) {
    case "pausing":
      return t("guard.pausing");
    case "interrupted":
      return t("guard.interrupted");
    case "paused":
      return t("guard.paused");
    case "cant_pause":
      return card.reason ?? t("guard.cantPause");
    default:
      return card.title;
  }
}

/** UI-170: a tripped runaway guard limit on an agent pane (F-AC-09). */
export function GuardBanner({ paneId }: { paneId: string }) {
  const card = useRunaway((s) => s.byPane[paneId]);
  const [busy, setBusy] = useState(false);
  const { show } = useToast();
  if (!card) return null;
  const act = async (run: () => Promise<{ ok: boolean; error?: { message: string } }>) => {
    setBusy(true);
    const out = await run();
    setBusy(false);
    if (!out.ok && out.error) show({ tone: "error", title: out.error.message });
  };
  return (
    <div
      data-testid="guard-banner"
      data-state={card.state}
      data-trip={card.trip.kind}
      role="status"
      className="flex min-h-8 shrink-0 flex-wrap items-center gap-2 border-b border-divider bg-waiting-soft px-2 py-1 text-sm text-fg"
    >
      <ShieldAlert size={14} aria-hidden="true" className="shrink-0 text-warning-text" />
      <span className="min-w-[220px] flex-1 truncate" title={card.title}>
        {card.state === "tripped" ? card.title : `${card.title} · ${guardLine(card)}`}
      </span>
      {card.state === "tripped" && (
        <Button
          size="sm"
          variant="ghost"
          loading={busy}
          data-testid="guard-pause"
          onClick={() => void act(() => pauseAgent(paneId))}
        >
          {t("guard.pause")}
        </Button>
      )}
      {card.state === "paused" && (
        <Button
          size="sm"
          variant="ghost"
          loading={busy}
          data-testid="guard-resume"
          onClick={() => void act(() => resumeAgent(paneId))}
        >
          {t("guard.resume")}
        </Button>
      )}
      {card.state !== "pausing" && (
        <Button
          size="sm"
          variant="ghost"
          data-testid="guard-dismiss"
          onClick={() => void act(() => dismissGuard(paneId))}
        >
          {t("guard.dismiss")}
        </Button>
      )}
    </div>
  );
}
