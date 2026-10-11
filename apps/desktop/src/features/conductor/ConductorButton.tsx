import { useRef } from "react";
import { Tooltip, WaveformRing, cx } from "@tandem/ui";
import { t, type MessageKey } from "../../i18n";
import { inCall, setMuted, startCall, toggleCall, useConductor } from "../../stores/conductor";
import { useSession } from "../../stores/session";
import { useShortcutKeys } from "../commands/useShortcutKeys";
import { ringState } from "./ring";
import { useAppActive } from "./useAppActive";

/** A press this long on the ring is push-to-talk (UI-61). */
export const HOLD_MS = 300;

/** The title bar's Conductor button (UI-01 item 8): a 28px ring. A click
 * starts or ends a call; holding it talks while held. Hidden on plans
 * without Conductor, where the palette offers the upgrade instead. */
export function ConductorButton() {
  const enabled = useSession((s) => !!s.entitlement?.limits.conductor);
  const view = useConductor((s) => s.view);
  const level = useConductor((s) => s.level);
  const active = useAppActive();
  const keys = useShortcutKeys();
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const holding = useRef(false);
  if (!enabled) return null;
  const live = inCall(view);
  const label = live ? t("conductor.ring.end") : t("conductor.ring.start");

  const down = () => {
    holding.current = false;
    timer.current = setTimeout(() => {
      holding.current = true;
      // Held: talk while held. A call starts muted for push-to-talk.
      if (!inCall(useConductor.getState().view)) void startCall(true).then(() => setMuted(false));
      else setMuted(false);
    }, HOLD_MS);
  };
  const up = () => {
    clearTimeout(timer.current);
    if (holding.current) {
      holding.current = false;
      setMuted(true);
      return;
    }
    toggleCall();
  };

  return (
    <Tooltip label={label} kbd={keys("conductor.call")}>
      <button
        type="button"
        aria-label={label}
        aria-pressed={live}
        data-testid="conductor-button"
        data-phase={view.phase}
        onPointerDown={down}
        onPointerUp={up}
        onPointerLeave={() => clearTimeout(timer.current)}
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
          label={view.phase === "idle" ? t("conductor.ring.idle") : t(`conductor.phase.${view.phase}` as MessageKey)}
          paused={!active || !live}
        />
      </button>
    </Tooltip>
  );
}
