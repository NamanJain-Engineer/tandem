import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { WorkspaceSummary } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import en from "../../i18n/en.json";
import { setInvokeBackend } from "../../ipc/invoke";
import { useLayout } from "../../stores/layout";
import { useUi } from "../../stores/ui";
import { useWorkspaces } from "../../stores/workspaces";
import { BOOT, fakeBackend, resetStores } from "../../test/fakeBackend";
import { Palette } from "../palette/Palette";
import { useBoot } from "../shell/useBoot";
import { isWebviewShortcut, useShortcuts } from "../shell/useShortcuts";
import { COMMANDS, bindingFor, commandTitle, handlers } from "./commands";
import { matches, parseBinding } from "./keys";

const WS: WorkspaceSummary = {
  id: "ws_1",
  kind: "local",
  hostId: "local",
  root: "/code/shop",
  wslDistro: null,
  name: "shop",
  parentId: null,
  laneBranch: null,
  sortKey: "a0",
  pinned: false,
  collapsed: false,
  sessionsCollapsed: false,
  createdAt: 0,
  missing: false,
  branch: "main",
  liveCount: 1,
  tabs: [{ id: "tb_1", workspaceId: "ws_1", name: "Tab 1", sortKey: "a0", createdAt: 0 }],
};

function Harness() {
  useShortcuts();
  return (
    <>
      <div className="xterm">
        <textarea aria-label="terminal input" className="xterm-helper-textarea" />
      </div>
      <input aria-label="field" />
      <button type="button">elsewhere</button>
      <Palette />
    </>
  );
}

/** A code-mode session with one focused pane, so every command is enabled. */
function setup(platform: "linux" | "macos" = "linux", dev = true) {
  fakeBackend();
  useBoot.setState({ boot: { ...BOOT, platform, dev } });
  useWorkspaces.setState({ list: [WS], activeId: WS.id, activeTab: { [WS.id]: "tb_1" } });
  useLayout.setState({
    tabs: { tb_1: { tree: { type: "leaf", paneId: "pn_1" }, active: "pn_1", maximized: null, panes: {}, keepEmpty: false } },
    loaded: { [WS.id]: true },
    boxes: {},
    dragging: false,
    picker: null,
  });
  render(
    <UiProvider>
      <Harness />
    </UiProvider>,
  );
}

/** Dispatches a keydown on `target` and reports whether the app took it. */
function press(target: Element, init: KeyboardEventInit): { prevented: boolean; reachedTarget: boolean } {
  let reachedTarget = false;
  const listener = () => {
    reachedTarget = true;
  };
  target.addEventListener("keydown", listener);
  const event = new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init });
  act(() => {
    target.dispatchEvent(event);
  });
  target.removeEventListener("keydown", listener);
  return { prevented: event.defaultPrevented, reachedTarget };
}

const terminal = () => screen.getByLabelText("terminal input");

beforeEach(resetStores);
afterEach(() => {
  vi.restoreAllMocks();
  setInvokeBackend(null);
});

describe("command registry (F-SH-09)", () => {
  it("F-SH-09 every registry command has a handler and a title", () => {
    expect(COMMANDS.length).toBeGreaterThan(20);
    for (const c of COMMANDS) {
      expect(handlers[c.id], c.id).toBeTypeOf("function");
      expect(Object.keys(en)).toContain(`cmd.${c.id}`);
      expect(commandTitle(c.id)).not.toBe(`cmd.${c.id}`);
    }
  });

  it("F-SH-09 bindings parse to the physical key and exact modifiers", () => {
    const b = parseBinding("Mod+Shift+[");
    expect(b).toEqual({ mod: true, ctrl: false, shift: true, alt: false, code: "BracketLeft" });
    const ev = { code: "BracketLeft", metaKey: false, ctrlKey: true, shiftKey: true, altKey: false };
    expect(matches(b, ev, false)).toBe(true);
    expect(matches(b, { ...ev, altKey: true }, false)).toBe(false);
    expect(matches(b, { ...ev, ctrlKey: false, metaKey: true }, true)).toBe(true);
    expect(matches(parseBinding("Ctrl+Tab"), { code: "Tab", metaKey: false, ctrlKey: true, shiftKey: false, altKey: false }, true)).toBe(true);
  });

  it("F-SH-09.1 with a terminal focused on Linux, Ctrl+W/T/D/[/] reach the PTY", () => {
    setup();
    const spies = ["pane.close", "workspace.new_tab", "pane.split_right", "pane.focus_prev", "pane.focus_next"].map((id) => vi.spyOn(handlers, id));
    for (const code of ["KeyW", "KeyT", "KeyD", "BracketLeft", "BracketRight"]) {
      const r = press(terminal(), { code, key: code.slice(-1).toLowerCase(), ctrlKey: true });
      expect(r, code).toEqual({ prevented: false, reachedTarget: true });
    }
    for (const s of spies) expect(s).not.toHaveBeenCalled();
  });

  it("F-SH-09.2 Ctrl+Shift+D in a terminal splits the pane and sends nothing", () => {
    setup();
    const split = vi.spyOn(handlers, "pane.split_down");
    const r = press(terminal(), { code: "KeyD", key: "D", ctrlKey: true, shiftKey: true });
    expect(split).toHaveBeenCalledTimes(1);
    expect(r).toEqual({ prevented: true, reachedTarget: false });
    expect(useLayout.getState().picker).toMatchObject({ tabId: "tb_1", side: "bottom" });
  });

  it("F-SH-09 outside a terminal, Ctrl+D splits right; Ctrl+Shift+E is the terminal alias", () => {
    setup();
    const right = vi.spyOn(handlers, "pane.split_right");
    press(screen.getByText("elsewhere"), { code: "KeyD", key: "d", ctrlKey: true });
    expect(right).toHaveBeenCalledTimes(1);
    press(terminal(), { code: "KeyE", key: "E", ctrlKey: true, shiftKey: true });
    expect(right).toHaveBeenCalledTimes(2);
  });

  it("F-SH-09 macOS terminals keep every Ctrl key; Cmd shortcuts still work", () => {
    setup("macos");
    const close = vi.spyOn(handlers, "pane.close").mockImplementation(() => undefined);
    expect(press(terminal(), { code: "KeyW", key: "w", ctrlKey: true }).reachedTarget).toBe(true);
    expect(close).not.toHaveBeenCalled();
    press(terminal(), { code: "KeyW", key: "w", metaKey: true });
    expect(close).toHaveBeenCalledTimes(1);
  });

  it("F-SH-09 text fields keep their keys, except the global few", () => {
    setup();
    const split = vi.spyOn(handlers, "pane.split_right");
    expect(press(screen.getByLabelText("field"), { code: "KeyD", key: "d", ctrlKey: true }).prevented).toBe(false);
    expect(split).not.toHaveBeenCalled();
    press(screen.getByLabelText("field"), { code: "Digit3", key: "3", ctrlKey: true });
    expect(useUi.getState().mode).toBe("thread");
  });

  it("F-SH-09.5 Mod+1 during IME composition doesn't switch mode", () => {
    setup();
    const field = screen.getByLabelText("field");
    act(() => {
      field.dispatchEvent(new CompositionEvent("compositionstart", { bubbles: true }));
    });
    press(field, { code: "Digit1", key: "1", ctrlKey: true });
    expect(useUi.getState().mode).toBe("code");
    act(() => {
      field.dispatchEvent(new CompositionEvent("compositionend", { bubbles: true }));
    });
    press(field, { code: "Digit1", key: "1", ctrlKey: true });
    expect(useUi.getState().mode).toBe("agent");
  });

  it("F-SH-09.3 release builds swallow reload, print and devtools keys", () => {
    setup("linux", false);
    expect(press(screen.getByText("elsewhere"), { key: "F5", code: "F5" }).prevented).toBe(true);
    expect(press(screen.getByText("elsewhere"), { key: "r", code: "KeyR", ctrlKey: true }).prevented).toBe(true);
    // Ctrl+R still reaches the shell for reverse search.
    expect(press(terminal(), { key: "r", code: "KeyR", ctrlKey: true }).reachedTarget).toBe(true);
    expect(isWebviewShortcut({ key: "i", code: "KeyI", ctrlKey: true, shiftKey: true, metaKey: false }, false)).toBe(true);
    expect(isWebviewShortcut({ key: "c", code: "KeyC", ctrlKey: true, shiftKey: false, metaKey: false }, false)).toBe(false);
  });

  it("F-SH-09.4 every registry command is in the palette and runs the shortcut's handler", () => {
    setup();
    for (const spec of COMMANDS) {
      if (spec.id === "palette.open") continue;
      act(() => useUi.setState({ paletteOpen: true, mode: "code", settingsOpen: false, upgrade: { open: false } }));
      const spy = vi.spyOn(handlers, spec.id).mockImplementation(() => undefined);
      const option = screen.getAllByRole("option").find((o) => o.textContent?.includes(commandTitle(spec.id)));
      expect(option, spec.id).toBeTruthy();
      fireEvent.click(option as HTMLElement);
      expect(spy, spec.id).toHaveBeenCalledTimes(1);
      // The same handler runs from its shortcut.
      const keys = bindingFor(spec, { mac: false, terminalFirst: false });
      if (keys) {
        const b = parseBinding(keys);
        press(screen.getByText("elsewhere"), { code: b.code, key: b.code, ctrlKey: b.mod || b.ctrl, shiftKey: b.shift, altKey: b.alt });
        expect(spy, `${spec.id} via ${keys}`).toHaveBeenCalledTimes(2);
      }
      spy.mockRestore();
    }
  });
});
