import { act, fireEvent, render, renderHook, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { UiProvider } from "@tandem/ui";
import { setInvokeBackend } from "../../ipc/invoke";
import { useSettings } from "../../stores/settings";
import { useUi } from "../../stores/ui";
import { fakeBackend, resetStores } from "../../test/fakeBackend";
import { useShortcuts } from "../shell/useShortcuts";
import { Cheatsheet } from "./Cheatsheet";
import { useShortcutKeys } from "./useShortcutKeys";

function Harness() {
  useShortcuts();
  return (
    <>
      <button type="button">elsewhere</button>
      <Cheatsheet />
    </>
  );
}

beforeEach(resetStores);
afterEach(() => setInvokeBackend(null));

const rows = () =>
  Object.fromEntries(
    within(screen.getByRole("dialog"))
      .queryAllByTestId("cheatsheet-row")
      .map((r) => [r.getAttribute("data-id"), r.textContent]),
  );

describe("shortcut cheatsheet (F-SH-09, UI-113)", () => {
  it("F-SH-09 Mod+/ lists every shortcut in effect, rebound ones included, and links to Settings › Shortcuts", () => {
    fakeBackend();
    useSettings.setState({ values: { shortcut_overrides: { "pane.split_right": "Mod+Alt+R", "pane.close": "" } }, loaded: true });
    render(
      <UiProvider>
        <Harness />
      </UiProvider>,
    );
    act(() => {
      screen.getByText("elsewhere").dispatchEvent(new KeyboardEvent("keydown", { code: "Slash", key: "/", ctrlKey: true, bubbles: true, cancelable: true }));
    });
    expect(screen.getByRole("dialog", { name: "Keyboard shortcuts" })).toBeTruthy();
    const all = rows();
    expect(all["pane.split_right"]).toBe("Split rightCtrlAltR");
    expect(all["pane.split_down"]).toBe("Split downCtrlShiftD");
    expect(all["terminal.find"]).toBe("Find in terminalCtrlFIn a terminal: Ctrl+Shift+F");
    // No keys, no row.
    expect(all["pane.close"]).toBeUndefined();
    expect(all["workspace.open_folder"]).toBeUndefined();
    fireEvent.change(within(screen.getByRole("dialog")).getByRole("searchbox", { name: "Search shortcuts" }), { target: { value: "split" } });
    expect(Object.keys(rows())).toEqual(["pane.split_right", "pane.split_down"]);
    fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Change shortcuts" }));
    expect(useUi.getState()).toMatchObject({ cheatsheetOpen: false, settingsOpen: true, settingsPage: "shortcuts" });
  });
});

describe("shortcut hints (F-SH-09 P2)", () => {
  it("F-SH-09.6 tooltips and menus show a command's keys in effect, or none", () => {
    fakeBackend();
    const { result, rerender } = renderHook(() => useShortcutKeys());
    expect(result.current("pane.split_right")).toEqual(["Mod", "D"]);
    expect(result.current("pane.focus_left")).toEqual(["Mod", "Alt", "←"]);
    act(() => useSettings.setState({ values: { shortcut_overrides: { "pane.split_right": "Mod+Alt+R", "pane.close": "" } }, loaded: true }));
    rerender();
    expect(result.current("pane.split_right")).toEqual(["Mod", "Alt", "R"]);
    expect(result.current("pane.close")).toBeUndefined();
    expect(result.current("no.such_command")).toBeUndefined();
  });
});

