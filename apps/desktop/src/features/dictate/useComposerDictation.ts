import { useEffect, useRef, type RefObject } from "react";
import type { ComposerDictation, ComposerHandle } from "@tandem/ui";
import type { IpcError } from "@tandem/protocol";
import { t } from "../../i18n";
import { listen } from "../../ipc/events";
import { invoke } from "../../ipc/invoke";
import { useDictate } from "../../stores/dictate";

/** A press longer than this on the mic is press-and-hold; shorter is a click. */
export const HOLD_MS = 300;

/**
 * Dictation for any composer (F-DI-12, UI-68): threads now, teammate chats
 * and Create with AI later. Give it a stable `targetId` and the composer's
 * handle; spread the result into `<Composer dictation={…} onFocusChange={…}>`.
 * Text arrives through `dictate:insert` and goes in at the caret, never
 * through the clipboard. Holding the global Dictate key while this composer
 * has focus does the same (the core asks for the focused composer).
 */
export function useComposerDictation(
  targetId: string,
  composer: RefObject<ComposerHandle | null>,
  onError?: (message: string) => void,
): { dictation: ComposerDictation; onFocusChange: (focused: boolean) => void } {
  const take = useDictate((s) => s.take);
  const level = useDictate((s) => s.level);
  const focused = useRef(false);
  const mine = take?.composer === targetId;
  const state: ComposerDictation["state"] =
    mine && take?.state === "listening" ? "listening" : mine && take?.state === "transcribing" ? "transcribing" : "idle";
  const stateRef = useRef(state);
  stateRef.current = state;
  const errorRef = useRef(onError);
  errorRef.current = onError;

  useEffect(() => {
    let off: (() => void) | undefined;
    let gone = false;
    void listen<{ targetId: string; text: string }>("dictate:insert", (e) => {
      if (e.targetId === targetId) composer.current?.insertAtCaret(e.text);
    }).then((u) => (gone ? u() : (off = u)));
    return () => {
      gone = true;
      off?.();
      // Unmounted while focused: the core shouldn't keep pointing here.
      if (focused.current) void invoke("dictate_composer_focus", { targetId: null }).catch(() => undefined);
    };
  }, [targetId, composer]);

  const start = () =>
    invoke("dictate_start_take", { origin: { type: "composer", targetId } }).catch((e: IpcError) =>
      errorRef.current?.(e.message),
    );
  const stop = () => invoke("dictate_stop_take").catch(() => undefined);
  const wasRunning = useRef(false);

  return {
    dictation: {
      state,
      level: mine ? level : 0,
      // Conductor (M16) owning the mic disables it; nothing else does.
      disabledReason: undefined,
      listeningPlaceholder: t("dictate.composer.listening"),
      listeningLabel: t("dictate.ring.listening"),
      transcribingLabel: t("dictate.ring.transcribing"),
      onPressStart: () => {
        wasRunning.current = stateRef.current !== "idle";
        if (wasRunning.current) void stop();
        else void start();
      },
      onPressEnd: (heldMs) => {
        // Held: it recorded while held, so the release stops it. A click
        // leaves it hands-free until the next click.
        if (!wasRunning.current && heldMs >= HOLD_MS) void stop();
      },
      onHoldKey: (down) => {
        if (down && stateRef.current === "idle") void start();
        if (!down) void stop();
      },
    },
    onFocusChange: (on) => {
      focused.current = on;
      void invoke("dictate_composer_focus", { targetId: on ? targetId : null }).catch(() => undefined);
    },
  };
}
