import { useRef } from "react";

/** A press this long on a Conductor ring is push-to-talk (UI-61). */
export const HOLD_MS = 300;

type Press = {
  /** A short press: start or end the call. */
  onClick: () => void;
  /** Held past {@link HOLD_MS}: start talking. */
  onHoldStart: () => void;
  /** Released after a hold: stop talking. */
  onHoldEnd: () => void;
};

/** Pointer handlers that tell a click from a hold, shared by the title bar's
 * ring and the floating ring (F-CN-01: "click to start or end a call, hold
 * for push-to-talk"). Leaving the button before the hold starts cancels it. */
export function usePressOrHold({ onClick, onHoldStart, onHoldEnd }: Press) {
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const holding = useRef(false);
  return {
    onPointerDown: () => {
      holding.current = false;
      timer.current = setTimeout(() => {
        holding.current = true;
        onHoldStart();
      }, HOLD_MS);
    },
    onPointerUp: () => {
      clearTimeout(timer.current);
      if (holding.current) {
        holding.current = false;
        onHoldEnd();
        return;
      }
      onClick();
    },
    onPointerLeave: () => clearTimeout(timer.current),
  };
}
