import { act, fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { HotkeyRecorder } from "./HotkeyRecorder";

function setup(withClear = true) {
  const onRecord = vi.fn();
  const onClear = vi.fn();
  render(
    <HotkeyRecorder
      label="Record a shortcut for Split right"
      idleText="Record…"
      recordingText="Press keys…"
      onRecord={onRecord}
      onClear={withClear ? onClear : undefined}
    />,
  );
  const button = screen.getByRole("button", { name: "Record a shortcut for Split right" });
  return { button, onRecord, onClear };
}

/** Presses a key on the recorder and reports whether its default was prevented. */
function press(el: HTMLElement, init: KeyboardEventInit): boolean {
  const event = new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init });
  act(() => {
    el.dispatchEvent(event);
  });
  return event.defaultPrevented;
}

describe("HotkeyRecorder (F-SH-09 P2, UI-113)", () => {
  it("F-SH-09.6 records the first combo, waiting past modifier keys pressed on their own", () => {
    const { button, onRecord } = setup();
    expect(button.textContent).toBe("Record…");
    expect(button.getAttribute("data-recording")).toBeNull();
    fireEvent.click(button);
    expect(button.textContent).toBe("Press keys…");
    expect(button.getAttribute("data-recording")).toBe("true");
    expect(press(button, { key: "Control", code: "ControlLeft", ctrlKey: true })).toBe(true);
    expect(press(button, { key: "Alt", code: "AltLeft", ctrlKey: true, altKey: true })).toBe(true);
    expect(onRecord).not.toHaveBeenCalled();
    expect(press(button, { key: "r", code: "KeyR", ctrlKey: true, altKey: true })).toBe(true);
    expect(onRecord).toHaveBeenCalledWith({ code: "KeyR", metaKey: false, ctrlKey: true, altKey: true, shiftKey: false });
    // One combo per recording.
    expect(button.textContent).toBe("Record…");
    press(button, { key: "t", code: "KeyT", ctrlKey: true });
    expect(onRecord).toHaveBeenCalledTimes(1);
  });

  it("F-SH-09.6 Esc and leaving the button cancel; Backspace clears; Tab moves on", () => {
    const { button, onRecord, onClear } = setup();
    fireEvent.click(button);
    expect(press(button, { key: "Escape", code: "Escape" })).toBe(true);
    expect(button.textContent).toBe("Record…");
    fireEvent.click(button);
    fireEvent.blur(button);
    expect(button.textContent).toBe("Record…");
    fireEvent.click(button);
    press(button, { key: "Backspace", code: "Backspace" });
    expect(onClear).toHaveBeenCalledTimes(1);
    // Tab isn't recorded and isn't held back: focus moves on as usual.
    fireEvent.click(button);
    expect(press(button, { key: "Tab", code: "Tab" })).toBe(false);
    expect(button.textContent).toBe("Record…");
    expect(onRecord).not.toHaveBeenCalled();
  });

  it("F-SH-09.6 without onClear, Backspace with a modifier is a combo like any other", () => {
    const { button, onRecord } = setup(false);
    fireEvent.click(button);
    press(button, { key: "Backspace", code: "Backspace", metaKey: true });
    expect(onRecord).toHaveBeenCalledWith({ code: "Backspace", metaKey: true, ctrlKey: false, altKey: false, shiftKey: false });
  });

  it("F-SH-09.6 says it's listening to screen readers", () => {
    const { button } = setup();
    const status = screen.getByRole("status");
    expect(status.textContent).toBe("");
    fireEvent.click(button);
    expect(status.textContent).toBe("Press keys…");
  });
});
