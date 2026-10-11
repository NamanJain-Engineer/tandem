import type { CallView } from "@tandem/protocol";
import type { WaveformState } from "@tandem/ui";
import { t, type MessageKey } from "../../i18n";

/** The ring for a call's phase (UI_SPEC §2.30, UI-61 "States"). */
export function ringState(v: CallView): WaveformState {
  switch (v.phase) {
    case "listening":
      return v.muted ? "idle" : "listening";
    case "connecting":
    case "thinking":
    case "acting":
      return "thinking";
    case "speaking":
      return "speaking";
    case "error":
      return "error";
    default:
      return "idle";
  }
}

/** The HUD's state label: CONNECTING, LISTENING, … MUTED. */
export function phaseLabel(v: CallView): string {
  if (v.muted && v.phase === "listening") return t("conductor.phase.muted");
  return t(`conductor.phase.${v.phase}` as MessageKey);
}

/** "4:12" */
export function clock(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

/** The newest 3 chips, and how many older ones fold into "+N" (UI-61). */
export function foldChips<T>(chips: T[], shown = 3): { visible: T[]; folded: T[] } {
  const visible = chips.slice(-shown);
  return { visible, folded: chips.slice(0, Math.max(0, chips.length - shown)) };
}

/** About 120 credits a minute (20 per started 10 s). */
export const CREDITS_PER_MINUTE = 120;
