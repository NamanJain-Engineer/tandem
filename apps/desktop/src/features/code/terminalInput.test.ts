import { describe, expect, it } from "vitest";
import { keyAction, lastLines, lineCount, needsConfirm, type KeyLike } from "./terminalInput";

const key = (over: Partial<KeyLike>): KeyLike => ({
  type: "keydown",
  key: "",
  code: "",
  ctrlKey: false,
  metaKey: false,
  shiftKey: false,
  altKey: false,
  isComposing: false,
  ...over,
});

const linux = { mac: false, hasSelection: false, agent: false };

describe("terminal keys (F-CD-05, F-CD-07)", () => {
  it("F-CD-05.1 Ctrl+C copies a selection and is SIGINT without one", () => {
    const ctrlC = key({ code: "KeyC", key: "c", ctrlKey: true });
    expect(keyAction(ctrlC, linux)).toBe("pass");
    expect(keyAction(ctrlC, { ...linux, hasSelection: true })).toBe("copy");
    expect(keyAction(key({ code: "KeyC", key: "C", ctrlKey: true, shiftKey: true }), linux)).toBe("copy");
  });

  it("F-CD-05 paste keys per platform; Ctrl+V stays the shell's", () => {
    expect(keyAction(key({ code: "KeyV", key: "V", ctrlKey: true, shiftKey: true }), linux)).toBe("paste");
    expect(keyAction(key({ key: "Insert", code: "Insert", shiftKey: true }), linux)).toBe("paste");
    expect(keyAction(key({ code: "KeyV", key: "v", ctrlKey: true }), linux)).toBe("pass");
    const mac = { ...linux, mac: true };
    expect(keyAction(key({ code: "KeyV", key: "v", metaKey: true }), mac)).toBe("paste");
    expect(keyAction(key({ code: "KeyC", key: "c", metaKey: true }), { ...mac, hasSelection: true })).toBe("copy");
    // macOS terminals keep every Ctrl key.
    expect(keyAction(key({ code: "KeyC", key: "c", ctrlKey: true }), { ...mac, hasSelection: true })).toBe("pass");
  });

  it("F-CD-07.1 Shift+Enter in an agent pane types the newline sequence; shells get Enter", () => {
    const shiftEnter = key({ key: "Enter", code: "Enter", shiftKey: true });
    expect(keyAction(shiftEnter, { ...linux, agent: true })).toBe("newline");
    expect(keyAction(shiftEnter, linux)).toBe("pass");
    expect(keyAction(key({ key: "Enter", code: "Enter" }), { ...linux, agent: true })).toBe("pass");
  });

  it("F-CD-07.2 nothing is intercepted during IME composition", () => {
    const composing = key({ key: "Enter", code: "Enter", shiftKey: true, isComposing: true });
    expect(keyAction(composing, { ...linux, agent: true })).toBe("pass");
    expect(keyAction(key({ code: "KeyC", ctrlKey: true, isComposing: true }), { ...linux, hasSelection: true })).toBe("pass");
  });
});

describe("paste and copy helpers (F-CD-05)", () => {
  it("F-CD-05.2 shells confirm multi-line pastes; agents and single lines don't", () => {
    expect(lineCount("a\nb")).toBe(2);
    expect(lineCount("a\n")).toBe(1);
    expect(lineCount("a\r\nb\r\nc")).toBe(3);
    expect(needsConfirm("a\nb", { agent: false, confirmSetting: true })).toBe(true);
    expect(needsConfirm("a\nb", { agent: true, confirmSetting: true })).toBe(false);
    expect(needsConfirm("a\nb", { agent: false, confirmSetting: false })).toBe(false);
    expect(needsConfirm("one line\n", { agent: false, confirmSetting: true })).toBe(false);
  });

  it("F-CD-05.4 copy last N lines returns exactly those lines", () => {
    const lines = Array.from({ length: 300 }, (_, i) => `line ${i + 1}`).concat(["", "  "]);
    const text = lastLines(lines, 100);
    expect(text.split("\n")).toHaveLength(100);
    expect(text.startsWith("line 201\n")).toBe(true);
    expect(text.endsWith("line 300")).toBe(true);
    expect(lastLines(lines, null).split("\n")).toHaveLength(300);
  });
});
