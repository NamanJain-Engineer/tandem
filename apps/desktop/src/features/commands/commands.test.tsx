import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { WorkspaceSummary } from "@tandem/protocol";
import { CodeEditor, NoteEditor, UiProvider } from "@tandem/ui";
import en from "../../i18n/en.json";
import { setInvokeBackend } from "../../ipc/invoke";
import { useLayout } from "../../stores/layout";
import { useSession } from "../../stores/session";
import { useSettings } from "../../stores/settings";
import { useThreads } from "../../stores/threads";
import { useUi } from "../../stores/ui";
import { useWorkspaces } from "../../stores/workspaces";
import { BOOT, FREE_VIEW, fakeBackend, resetStores } from "../../test/fakeBackend";
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
  laneBase: null,
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
    tabs: {
      tb_1: {
        tree: { type: "leaf", paneId: "pn_1" },
        active: "pn_1",
        maximized: null,
        panes: {},
        keepEmpty: false,
      },
    },
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
    expect(
      matches(
        parseBinding("Ctrl+Tab"),
        { code: "Tab", metaKey: false, ctrlKey: true, shiftKey: false, altKey: false },
        true,
      ),
    ).toBe(true);
  });

  it("F-SH-09.1 with a terminal focused on Linux, Ctrl+W/T/D/[/] reach the PTY", () => {
    setup();
    const spies = [
      "pane.close",
      "workspace.new_tab",
      "pane.split_right",
      "pane.focus_prev",
      "pane.focus_next",
    ].map((id) => vi.spyOn(handlers, id));
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
    expect(press(screen.getByLabelText("field"), { code: "KeyD", key: "d", ctrlKey: true }).prevented).toBe(
      false,
    );
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
    expect(press(screen.getByText("elsewhere"), { key: "r", code: "KeyR", ctrlKey: true }).prevented).toBe(
      true,
    );
    // Ctrl+R still reaches the shell for reverse search.
    expect(press(terminal(), { key: "r", code: "KeyR", ctrlKey: true }).reachedTarget).toBe(true);
    expect(
      isWebviewShortcut({ key: "i", code: "KeyI", ctrlKey: true, shiftKey: true, metaKey: false }, false),
    ).toBe(true);
    expect(
      isWebviewShortcut({ key: "c", code: "KeyC", ctrlKey: true, shiftKey: false, metaKey: false }, false),
    ).toBe(false);
  });

  it("F-SH-09.3 in release builds an editor still gets the keys it handles: Mod+S saves a file, Mod+U underlines a note", () => {
    setup("linux", false);
    const onSave = vi.fn();
    const onNote = vi.fn();
    render(
      <>
        <CodeEditor value="x" docKey="f" path="a.ts" label="file" onSave={onSave} />
        <NoteEditor value="word" docKey="n" label="note" taskLabel="Done" onChange={onNote} />
      </>,
    );
    const content = (label: string) => screen.getByLabelText(label);
    expect(press(content("file"), { key: "s", code: "KeyS", ctrlKey: true }).prevented).toBe(true);
    expect(onSave).toHaveBeenCalledTimes(1);
    // The cursor starts before "word": the marks go in around it.
    const note = content("note");
    expect(press(note, { key: "u", code: "KeyU", ctrlKey: true }).prevented).toBe(true);
    expect(onNote).toHaveBeenLastCalledWith("<u></u>word");
    // What the editor doesn't handle is still kept from the webview.
    expect(press(note, { key: "p", code: "KeyP", ctrlKey: true }).prevented).toBe(true);
    expect(press(note, { key: "r", code: "KeyR", ctrlKey: true }).prevented).toBe(true);
  });

  it("F-SH-09.4 every registry command is in the palette and runs the shortcut's handler", () => {
    setup();
    const elsewhere = screen.getByText("elsewhere");
    for (const spec of COMMANDS) {
      if (spec.id === "palette.open") continue;
      // Each command where it's enabled: Thread mode with a thread open for
      // the thread commands, Code mode with a pane for the rest.
      const inThread = spec.enabledWhen === "thread_open";
      // Plan-gated commands where the plan has them (absent otherwise:
      // features/tasks/hosted.test.tsx).
      const routines = spec.enabledWhen === "routines";
      const conductor = spec.enabledWhen === "conductor";
      act(() => {
        useSession.setState({
          entitlement: { ...FREE_VIEW, limits: { ...FREE_VIEW.limits, routines, conductor } },
        });
        useThreads.setState({ selectedId: inThread ? "thr_open" : null });
        useUi.setState({
          paletteOpen: true,
          mode: inThread ? "thread" : "code",
          settingsOpen: false,
          upgrade: { open: false },
        });
      });
      const spy = vi.spyOn(handlers, spec.id).mockImplementation(() => undefined);
      // A plain selector: getAllByRole walks the accessibility tree, too slow
      // to repeat once per command.
      const option = Array.from(document.querySelectorAll<HTMLElement>('[role="option"]')).find((o) =>
        o.textContent?.includes(commandTitle(spec.id)),
      );
      expect(option, spec.id).toBeTruthy();
      fireEvent.click(option as HTMLElement);
      expect(spy, spec.id).toHaveBeenCalledTimes(1);
      // The same handler runs from its shortcut.
      const keys = bindingFor(spec, { mac: false, terminalFirst: false });
      if (keys) {
        const b = parseBinding(keys);
        press(elsewhere, {
          code: b.code,
          key: b.code,
          ctrlKey: b.mod || b.ctrl,
          shiftKey: b.shift,
          altKey: b.alt,
        });
        expect(spy, `${spec.id} via ${keys}`).toHaveBeenCalledTimes(2);
      }
      spy.mockRestore();
    }
  }, 20_000); // one palette round trip per command
});

describe("rebound shortcuts (F-SH-09 P2)", () => {
  const rebind = (overrides: Record<string, string>, priority = "terminal") =>
    act(() =>
      useSettings.setState({
        values: {
          ...useSettings.getState().values,
          shortcut_overrides: overrides,
          term_shortcut_priority: priority,
        },
      }),
    );
  const elsewhere = () => screen.getByText("elsewhere");

  it("F-SH-09.6 a new shortcut runs the command, the old keys don't, and the palette shows it", () => {
    setup();
    const split = vi.spyOn(handlers, "pane.split_right").mockImplementation(() => undefined);
    rebind({ "pane.split_right": "Mod+Alt+R" });
    expect(press(elsewhere(), { code: "KeyD", key: "d", ctrlKey: true }).prevented).toBe(false);
    expect(split).not.toHaveBeenCalled();
    press(elsewhere(), { code: "KeyR", key: "r", ctrlKey: true, altKey: true });
    expect(split).toHaveBeenCalledTimes(1);
    // Ctrl+Alt+… is on the terminal's intercept list, so it works there too,
    // and the old terminal alias is gone with the old shortcut.
    press(terminal(), { code: "KeyR", key: "r", ctrlKey: true, altKey: true });
    expect(split).toHaveBeenCalledTimes(2);
    expect(press(terminal(), { code: "KeyE", key: "E", ctrlKey: true, shiftKey: true }).reachedTarget).toBe(
      true,
    );
    expect(split).toHaveBeenCalledTimes(2);
    act(() => useUi.setState({ paletteOpen: true, mode: "code", settingsOpen: false }));
    const option = screen
      .getAllByRole("option")
      .find((o) => o.textContent?.startsWith(commandTitle("pane.split_right")));
    expect(option?.textContent).toBe(`${commandTitle("pane.split_right")}CtrlAltR`);
  });

  it("F-SH-09.6 a command set to no shortcut runs only from the palette", () => {
    setup();
    const split = vi.spyOn(handlers, "pane.split_right").mockImplementation(() => undefined);
    rebind({ "pane.split_right": "" });
    press(elsewhere(), { code: "KeyD", key: "d", ctrlKey: true });
    press(terminal(), { code: "KeyE", key: "E", ctrlKey: true, shiftKey: true });
    expect(split).not.toHaveBeenCalled();
  });

  it("F-SH-09.6 under Terminal first, a new shortcut a shell needs still reaches the shell", () => {
    setup();
    const split = vi.spyOn(handlers, "pane.split_right").mockImplementation(() => undefined);
    rebind({ "pane.split_right": "Mod+E" });
    expect(press(terminal(), { code: "KeyE", key: "e", ctrlKey: true })).toEqual({
      prevented: false,
      reachedTarget: true,
    });
    expect(split).not.toHaveBeenCalled();
    press(elsewhere(), { code: "KeyE", key: "e", ctrlKey: true });
    expect(split).toHaveBeenCalledTimes(1);
    rebind({ "pane.split_right": "Mod+E" }, "tandem");
    press(terminal(), { code: "KeyE", key: "e", ctrlKey: true });
    expect(split).toHaveBeenCalledTimes(2);
  });

  it("F-SH-09.6 no shortcut fires while a shortcut recorder listens", () => {
    setup();
    const split = vi.spyOn(handlers, "pane.split_down").mockImplementation(() => undefined);
    const recorder = document.createElement("button");
    recorder.setAttribute("data-recording", "true");
    document.body.append(recorder);
    expect(press(recorder, { code: "KeyD", key: "D", ctrlKey: true, shiftKey: true }).reachedTarget).toBe(
      true,
    );
    expect(split).not.toHaveBeenCalled();
    recorder.remove();
  });
});
