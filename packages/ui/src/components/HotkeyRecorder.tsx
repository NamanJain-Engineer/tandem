import { useRef, useState, type KeyboardEvent, type MouseEvent } from "react";
import { cx } from "../cx";
import { Button } from "./Button";

/** The combo pressed while recording, as the keyboard reported it. */
export type RecordedKeys = { code: string; metaKey: boolean; ctrlKey: boolean; altKey: boolean; shiftKey: boolean };

export type HotkeyRecorderProps = {
  /** Names the action, e.g. "Record a shortcut for Split right". */
  label: string;
  /** "Record…" */
  idleText: string;
  /** "Press keys…", also announced to screen readers. */
  recordingText: string;
  /** The first combo pressed while recording (modifier keys alone don't count). */
  onRecord: (keys: RecordedKeys) => void;
  /** Backspace while recording; without it, Backspace is a key like any other. */
  onClear?: () => void;
  disabled?: boolean;
  /** Modifiers pressed and released on their own count as a combo
   * (`code` is ""): global push-to-talk keys like Ctrl+Alt (F-DI-01). */
  modifierOnly?: boolean;
  /** Mouse buttons 4 and 5 count (`code` "Mouse4" / "Mouse5"). */
  mouseButtons?: boolean;
  className?: string;
};

const MODIFIER_CODES = new Set([
  "ShiftLeft",
  "ShiftRight",
  "ControlLeft",
  "ControlRight",
  "AltLeft",
  "AltRight",
  "MetaLeft",
  "MetaRight",
  "OSLeft",
  "OSRight",
  "CapsLock",
  "Fn",
]);

/** A button that captures one key combo (UI_SPEC §2.19, UI-113). Click it,
 * then press the keys: Esc or leaving the button cancels, Backspace clears
 * (with `onClear`), and Tab moves on as usual. While it records it carries
 * `data-recording`, so app shortcuts stay out of the way. */
export function HotkeyRecorder({
  label,
  idleText,
  recordingText,
  onRecord,
  onClear,
  disabled,
  modifierOnly = false,
  mouseButtons = false,
  className,
}: HotkeyRecorderProps) {
  const [recording, setRecording] = useState(false);
  // Modifier-only combos: the most modifiers held at once, recorded when
  // they're all released without another key in between.
  const peak = useRef<RecordedKeys | null>(null);

  const onKeyDown = (e: KeyboardEvent<HTMLButtonElement>) => {
    if (!recording) return;
    const held = e.metaKey || e.ctrlKey || e.altKey;
    if (e.key === "Tab" && !held) {
      setRecording(false);
      return;
    }
    e.preventDefault();
    e.stopPropagation();
    if (MODIFIER_CODES.has(e.code) || e.key === "Dead" || e.key === "Process") {
      if (modifierOnly && (e.ctrlKey || e.altKey || e.shiftKey || e.metaKey)) {
        const p = peak.current;
        const now = { code: "", metaKey: e.metaKey, ctrlKey: e.ctrlKey, altKey: e.altKey, shiftKey: e.shiftKey };
        const count = (k: RecordedKeys) => [k.metaKey, k.ctrlKey, k.altKey, k.shiftKey].filter(Boolean).length;
        if (!p || count(now) >= count(p)) peak.current = now;
      }
      return;
    }
    peak.current = null;
    setRecording(false);
    if (e.key === "Escape" && !held && !e.shiftKey) return;
    if (e.key === "Backspace" && !held && !e.shiftKey && onClear) {
      onClear();
      return;
    }
    onRecord({ code: e.code, metaKey: e.metaKey, ctrlKey: e.ctrlKey, altKey: e.altKey, shiftKey: e.shiftKey });
  };

  const onKeyUp = (e: KeyboardEvent<HTMLButtonElement>) => {
    if (!recording || !modifierOnly || !peak.current) return;
    if (e.ctrlKey || e.altKey || e.shiftKey || e.metaKey) return;
    e.preventDefault();
    const keys = peak.current;
    peak.current = null;
    setRecording(false);
    onRecord(keys);
  };

  const onMouseDown = (e: MouseEvent<HTMLButtonElement>) => {
    if (!recording || !mouseButtons || (e.button !== 3 && e.button !== 4)) return;
    e.preventDefault();
    peak.current = null;
    setRecording(false);
    onRecord({ code: e.button === 3 ? "Mouse4" : "Mouse5", metaKey: e.metaKey, ctrlKey: e.ctrlKey, altKey: e.altKey, shiftKey: e.shiftKey });
  };

  return (
    <>
      <Button
        size="sm"
        variant="secondary"
        aria-label={label}
        aria-pressed={recording}
        data-recording={recording ? "true" : undefined}
        disabled={disabled}
        onClick={() => setRecording((r) => !r)}
        onKeyDown={onKeyDown}
        onKeyUp={onKeyUp}
        onMouseDown={onMouseDown}
        onBlur={() => {
          peak.current = null;
          setRecording(false);
        }}
        className={cx(recording && "border-accent text-accent", className)}
      >
        {recording ? recordingText : idleText}
      </Button>
      <span role="status" className="sr-only">
        {recording ? recordingText : ""}
      </span>
    </>
  );
}
