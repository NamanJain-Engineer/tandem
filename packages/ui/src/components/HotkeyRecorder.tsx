import { useState, type KeyboardEvent } from "react";
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
export function HotkeyRecorder({ label, idleText, recordingText, onRecord, onClear, disabled, className }: HotkeyRecorderProps) {
  const [recording, setRecording] = useState(false);

  const onKeyDown = (e: KeyboardEvent<HTMLButtonElement>) => {
    if (!recording) return;
    const held = e.metaKey || e.ctrlKey || e.altKey;
    if (e.key === "Tab" && !held) {
      setRecording(false);
      return;
    }
    e.preventDefault();
    e.stopPropagation();
    if (MODIFIER_CODES.has(e.code) || e.key === "Dead" || e.key === "Process") return;
    setRecording(false);
    if (e.key === "Escape" && !held && !e.shiftKey) return;
    if (e.key === "Backspace" && !held && !e.shiftKey && onClear) {
      onClear();
      return;
    }
    onRecord({ code: e.code, metaKey: e.metaKey, ctrlKey: e.ctrlKey, altKey: e.altKey, shiftKey: e.shiftKey });
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
        onBlur={() => setRecording(false)}
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
