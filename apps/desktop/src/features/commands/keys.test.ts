import { describe, expect, it } from "vitest";
import { keyNameOf, recordShortcut, sameShortcut, terminalMayIntercept } from "./keys";

const combo = (code: string, mods: { meta?: boolean; ctrl?: boolean; alt?: boolean; shift?: boolean } = {}) => ({
  code,
  metaKey: !!mods.meta,
  ctrlKey: !!mods.ctrl,
  altKey: !!mods.alt,
  shiftKey: !!mods.shift,
});

describe("shortcut grammar (F-SH-09 P2)", () => {
  it("F-SH-09.6 names keys by their physical position", () => {
    expect(["KeyR", "Digit3", "F6", "BracketLeft", "Slash", "Quote", "ArrowLeft", "PageDown"].map(keyNameOf)).toEqual([
      "R",
      "3",
      "F6",
      "[",
      "/",
      "'",
      "Left",
      "PageDown",
    ]);
    expect(keyNameOf("Numpad1")).toBeNull();
    expect(keyNameOf("IntlBackslash")).toBeNull();
  });

  it("F-SH-09.6 records Ctrl as Mod on Windows and Linux, and ⌘ as Mod with ⌃ kept apart on macOS", () => {
    expect(recordShortcut(combo("KeyR", { ctrl: true, alt: true }), "linux")).toEqual({ ok: true, keys: "Mod+Alt+R" });
    expect(recordShortcut(combo("KeyR", { meta: true, alt: true }), "macos")).toEqual({ ok: true, keys: "Mod+Alt+R" });
    expect(recordShortcut(combo("KeyR", { ctrl: true }), "macos")).toEqual({ ok: true, keys: "Ctrl+R" });
    expect(recordShortcut(combo("KeyD", { meta: true, ctrl: true, alt: true, shift: true }), "macos")).toEqual({ ok: true, keys: "Mod+Ctrl+Alt+Shift+D" });
    expect(recordShortcut(combo("Slash", { ctrl: true, shift: true }), "windows")).toEqual({ ok: true, keys: "Mod+Shift+/" });
    expect(recordShortcut(combo("F2"), "linux")).toEqual({ ok: true, keys: "F2" });
    expect(recordShortcut(combo("F6", { shift: true }), "linux")).toEqual({ ok: true, keys: "Shift+F6" });
  });

  it("F-SH-09.8 refuses combos the system or text editing owns, plain keys and keys it can't name", () => {
    const refused = (code: string, mods: Parameters<typeof combo>[1], platform: string) => recordShortcut(combo(code, mods), platform);
    for (const [code, mods] of [
      ["KeyQ", { meta: true }],
      ["KeyH", { meta: true }],
      ["Space", { meta: true }],
      ["Tab", { meta: true }],
      ["KeyC", { meta: true }],
      ["KeyZ", { meta: true, shift: true }],
    ] as const) {
      expect(refused(code, mods, "macos"), `${code}`).toEqual({ ok: false, reason: "system" });
    }
    for (const platform of ["windows", "linux"]) {
      for (const [code, mods] of [
        ["F4", { alt: true }],
        ["Tab", { alt: true }],
        ["Delete", { ctrl: true, alt: true }],
        ["KeyV", { ctrl: true }],
        ["KeyA", { ctrl: true }],
        ["KeyR", { meta: true, ctrl: true }],
      ] as const) {
        expect(refused(code, mods, platform), `${platform} ${code}`).toEqual({ ok: false, reason: "system" });
      }
    }
    // GNOME opens a terminal on Ctrl+Alt+T (UI_SPEC §5); Windows doesn't.
    expect(refused("KeyT", { ctrl: true, alt: true }, "linux")).toEqual({ ok: false, reason: "system" });
    expect(refused("KeyT", { ctrl: true, alt: true }, "windows")).toEqual({ ok: true, keys: "Mod+Alt+T" });
    expect(refused("KeyR", {}, "linux")).toEqual({ ok: false, reason: "modifier" });
    expect(refused("KeyR", { shift: true }, "macos")).toEqual({ ok: false, reason: "modifier" });
    expect(refused("Numpad1", { ctrl: true }, "linux")).toEqual({ ok: false, reason: "key" });
  });

  it("F-SH-09.6 the terminal intercept list matches the core's", () => {
    for (const k of ["Mod+Shift+D", "Mod+Alt+R", "Mod+K", "Mod+,", "Mod+2", "Ctrl+Tab", "Ctrl+Shift+Tab", "F6", "Mod+Shift+/"]) {
      expect(terminalMayIntercept(k), k).toBe(true);
    }
    for (const k of ["Mod+D", "Mod+W", "Mod+T", "Mod+[", "Mod+/", "Mod+4", "Alt+R", "F2"]) {
      expect(terminalMayIntercept(k), k).toBe(false);
    }
  });

  it("F-SH-09.7 Ctrl and Mod are the same key on Windows and Linux, not on macOS", () => {
    expect(sameShortcut("Ctrl+Tab", "Mod+Tab", false)).toBe(true);
    expect(sameShortcut("Ctrl+Tab", "Mod+Tab", true)).toBe(false);
    expect(sameShortcut("Mod+Alt+Shift+D", "Mod+Shift+Alt+D", true)).toBe(true);
    expect(sameShortcut("Mod+D", "Mod+Shift+D", false)).toBe(false);
  });
});
