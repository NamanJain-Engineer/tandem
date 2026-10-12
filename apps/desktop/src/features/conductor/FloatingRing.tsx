import { useEffect, useState } from "react";
import { Mic, MicOff, ScrollText, X } from "lucide-react";
import type { CallView, RingState } from "@tandem/protocol";
import { IconButton, UiProvider, WaveformRing, type ThemeSetting } from "@tandem/ui";
import { t, type MessageKey } from "../../i18n";
import { emitTo, listen } from "../../ipc/events";
import { invoke } from "../../ipc/invoke";
import { IDLE, inCall } from "../../stores/conductor";
import { ringState } from "./ring";
import { useAppActive } from "./useAppActive";

/** The floating ring (F-CN-04, UI-61): a 56 px ring that floats over
 * everything and drags anywhere; a click starts or ends a call, and hovering
 * shows Mute, Call log and End. Its own window, with only the commands it
 * needs (`capabilities/ring.json`). */
export function FloatingRing({ view, level }: { view: CallView; level: number }) {
  const [hover, setHover] = useState(false);
  const active = useAppActive();
  const live = inCall(view);
  const label = live ? t("conductor.ring.end") : t("conductor.ring.start");
  const toggle = () =>
    void (live ? invoke("conductor_end_call", {}) : invoke("conductor_start_call", { wake: false })).catch(
      () => undefined,
    );
  return (
    <div
      data-tauri-drag-region
      data-testid="floating-ring"
      className="flex h-screen w-screen items-center justify-center gap-1"
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      onMouseUp={() => {
        void import("@tauri-apps/api/window")
          .then(({ getCurrentWindow }) => getCurrentWindow().outerPosition())
          .then((p) => invoke("conductor_ring_set_position", { x: p.x, y: p.y }))
          .catch(() => undefined);
      }}
    >
      {hover && live && (
        <IconButton
          size="sm"
          variant="secondary"
          icon={view.muted ? <MicOff size={14} /> : <Mic size={14} />}
          label={view.muted ? t("conductor.unmute") : t("conductor.mute")}
          pressed={view.muted}
          onClick={() => void invoke("conductor_mute", { muted: !view.muted }).catch(() => undefined)}
        />
      )}
      <button
        type="button"
        aria-label={label}
        data-phase={view.phase}
        onClick={toggle}
        className="rounded-full bg-surface-3 shadow-3 focus-visible:shadow-focus focus-visible:outline-none"
      >
        <WaveformRing
          state={ringState(view)}
          level={level}
          size={56}
          label={
            view.phase === "idle"
              ? t("conductor.ring.idle")
              : t(`conductor.phase.${view.phase}` as MessageKey)
          }
          paused={!active || !live}
        />
      </button>
      {hover && (
        <IconButton
          size="sm"
          variant="secondary"
          icon={<ScrollText size={14} />}
          label={t("conductor.callLog")}
          onClick={() => void emitTo("main", "conductor:open-log", null).catch(() => undefined)}
        />
      )}
      {hover && live && (
        <IconButton
          size="sm"
          variant="secondary"
          icon={<X size={14} />}
          label={t("conductor.end")}
          onClick={() => void invoke("conductor_end_call", {}).catch(() => undefined)}
        />
      )}
    </div>
  );
}

/** The ring window's root: the call and the app's theme, nothing else. */
export function RingRoot() {
  const [view, setView] = useState<CallView>(IDLE);
  const [level, setLevel] = useState(0);
  const [theme, setTheme] = useState<ThemeSetting>("dark");
  useEffect(() => {
    void invoke<RingState>("conductor_ring_state", {}).then(
      (s) => {
        setView(s.view);
        setTheme((s.theme as ThemeSetting) || "dark");
      },
      () => undefined,
    );
    const offs = [
      listen<CallView>("conductor:state", (v) => {
        setView(v);
        if (!inCall(v)) setLevel(0);
      }),
      listen<number>("conductor:level", setLevel),
    ];
    return () => offs.forEach((p) => void p.then((f) => f()));
  }, []);
  return (
    <UiProvider theme={theme}>
      <FloatingRing view={view} level={level} />
    </UiProvider>
  );
}
