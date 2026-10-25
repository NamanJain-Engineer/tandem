import { Tooltip, WaveformRing, cx } from "@tandem/ui";
import { t, type MessageKey } from "../../i18n";
import { inCall, setMuted, startCall, toggleCall, useConductor } from "../../stores/conductor";
import { useSession } from "../../stores/session";
import { useShortcutKeys } from "../commands/useShortcutKeys";
import { usePressOrHold } from "./hold";
import { ringState } from "./ring";
import { useAppActive } from "./useAppActive";

/** The title bar's Conductor button (UI-01 item 8): a 28px ring. A click
 * starts or ends a call; holding it talks while held. Hidden without
 * Conductor (hosted), where the palette's call command says it comes with
 * Vibezeo accounts (D-2903). */
export function ConductorButton() {
  const enabled = useSession((s) => !!s.entitlement?.limits.conductor);
  const view = useConductor((s) => s.view);
  const level = useConductor((s) => s.level);
  const active = useAppActive();
  const keys = useShortcutKeys();
  const press = usePressOrHold({
    onClick: toggleCall,
    // Held: talk while held. A call starts muted for push-to-talk.
    onHoldStart: () => {
      if (!inCall(useConductor.getState().view)) void startCall(true).then(() => setMuted(false));
      else setMuted(false);
    },
    onHoldEnd: () => setMuted(true),
  });
  if (!enabled) return null;
  const live = inCall(view);
  const label = live ? t("conductor.ring.end") : t("conductor.ring.start");

  return (
    <Tooltip label={label} kbd={keys("conductor.call")}>
      <button
        type="button"
        aria-label={label}
        aria-pressed={live}
        data-testid="conductor-button"
        data-phase={view.phase}
        {...press}
        onKeyDown={(e) => {
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            toggleCall();
          }
        }}
        className={cx(
          "inline-flex h-7 w-7 items-center justify-center rounded-md hover:bg-surface-4 focus-visible:shadow-focus focus-visible:outline-none",
        )}
      >
        <WaveformRing
          state={ringState(view)}
          level={level}
          size={28}
          label={
            view.phase === "idle"
              ? t("conductor.ring.idle")
              : t(`conductor.phase.${view.phase}` as MessageKey)
          }
          paused={!active || !live}
        />
      </button>
    </Tooltip>
  );
}
