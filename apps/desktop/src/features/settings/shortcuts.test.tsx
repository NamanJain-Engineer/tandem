import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { UiProvider } from "@tandem/ui";
import { setInvokeBackend } from "../../ipc/invoke";
import { useSettings } from "../../stores/settings";
import { BOOT, fakeBackend, resetStores, type Call } from "../../test/fakeBackend";
import { useBoot } from "../shell/useBoot";
import { ShortcutsPage } from "./ShortcutsPage";

beforeEach(resetStores);
afterEach(() => setInvokeBackend(null));

function mount(overrides: Record<string, string> = {}, platform: "linux" | "macos" = "linux", failSave = false) {
  const calls = fakeBackend(
    failSave
      ? {
          settings_set: () => {
            throw { code: "internal", message: "disk full", stage: "settings", hint: null, retryable: false };
          },
        }
      : {},
  );
  useBoot.setState({ boot: { ...BOOT, platform } });
  useSettings.setState({ values: { shortcut_overrides: overrides, term_shortcut_priority: "terminal" }, loaded: true });
  render(
    <UiProvider>
      <ShortcutsPage />
    </UiProvider>,
  );
  return calls;
}

const saves = (calls: Call[]) => calls.filter((c) => c.cmd === "settings_set" && c.args?.key === "shortcut_overrides").map((c) => c.args?.value);
const row = (action: string) => screen.getByTestId(`shortcut-${action}`);
const keysIn = (action: string) => within(row(action)).getByTestId("row-keys").textContent;

/** Clicks the row's Record… and presses keys on it. */
function record(action: string, init: KeyboardEventInit) {
  const button = within(row(action)).getByRole("button", { name: /^Record a shortcut for / });
  fireEvent.click(button);
  act(() => {
    button.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init }));
  });
}

describe("Settings › Shortcuts (F-SH-09 P2, UI-113)", () => {
  it("F-SH-09.6 lists every command by §5 group with its keys, and a recorded combo replaces them", async () => {
    const calls = mount();
    expect(screen.getAllByRole("heading", { level: 2 }).map((h) => h.textContent)).toEqual(["App and navigation", "Code mode: tabs and panes", "Terminal"]);
    expect(keysIn("pane.split_right")).toBe("CtrlD");
    expect(within(row("pane.split_right")).queryByRole("button", { name: /^Reset / })).toBeNull();
    record("pane.split_right", { code: "KeyR", key: "r", ctrlKey: true, altKey: true });
    await waitFor(() => expect(saves(calls)).toEqual([{ "pane.split_right": "Mod+Alt+R" }]));
    expect(keysIn("pane.split_right")).toBe("CtrlAltR");
    expect(within(row("pane.split_right")).getByRole("button", { name: "Reset Split right to its default" })).toBeTruthy();
    expect(screen.getByTestId("shortcuts-announce").textContent).toBe("Split right: Ctrl+Alt+R");
    // Recording the default again is the same as resetting.
    record("pane.split_right", { code: "KeyD", key: "d", ctrlKey: true });
    await waitFor(() => expect(saves(calls).at(-1)).toEqual({}));
  });

  it("F-SH-09.7 a combo another command uses asks first: Cancel changes nothing, Swap trades the shortcuts", async () => {
    const calls = mount();
    record("pane.split_right", { code: "KeyD", key: "D", ctrlKey: true, shiftKey: true });
    const notice = within(row("pane.split_right")).getByRole("alert");
    expect(notice.textContent).toContain("Already used by Split down.");
    fireEvent.click(within(notice).getByRole("button", { name: "Cancel" }));
    expect(within(row("pane.split_right")).queryByRole("alert")).toBeNull();
    expect(saves(calls)).toEqual([]);
    record("pane.split_right", { code: "KeyD", key: "D", ctrlKey: true, shiftKey: true });
    fireEvent.click(within(within(row("pane.split_right")).getByRole("alert")).getByRole("button", { name: "Swap" }));
    await waitFor(() => expect(saves(calls)).toEqual([{ "pane.split_right": "Mod+Shift+D", "pane.split_down": "Mod+D" }]));
    expect(keysIn("pane.split_right")).toBe("CtrlShiftD");
    expect(keysIn("pane.split_down")).toBe("CtrlD");
  });

  it("F-SH-09.8 combos the system owns and keys without a modifier are refused and change nothing", () => {
    const calls = mount();
    record("pane.split_right", { code: "F4", key: "F4", altKey: true });
    expect(within(row("pane.split_right")).getByRole("alert").textContent).toBe("Your system uses this shortcut.");
    record("pane.split_right", { code: "KeyV", key: "v", ctrlKey: true });
    expect(within(row("pane.split_right")).getByRole("alert").textContent).toBe("Your system uses this shortcut.");
    record("pane.split_right", { code: "KeyR", key: "r" });
    expect(within(row("pane.split_right")).getByRole("alert").textContent).toBe("Hold Ctrl or Alt with the key.");
    expect(saves(calls)).toEqual([]);
    expect(keysIn("pane.split_right")).toBe("CtrlD");
  });

  it("F-SH-09.9 ↺ restores one default; Reset all asks first, then restores every default", async () => {
    const calls = mount({ "pane.split_right": "Mod+Alt+R", "pane.close": "" });
    expect(keysIn("pane.close")).toBe("None");
    fireEvent.click(within(row("pane.split_right")).getByRole("button", { name: "Reset Split right to its default" }));
    await waitFor(() => expect(saves(calls)).toEqual([{ "pane.close": "" }]));
    expect(keysIn("pane.split_right")).toBe("CtrlD");
    fireEvent.click(screen.getByRole("button", { name: "Reset all to defaults" }));
    // Role queries over the whole page are slow; the dialog is found by its title.
    const dialog = (await screen.findByText("Reset every shortcut?")).closest("[role=dialog]") as HTMLElement;
    expect(dialog).toBeTruthy();
    fireEvent.click(within(dialog).getByRole("button", { name: "Reset all" }));
    await waitFor(() => expect(saves(calls).at(-1)).toEqual({}));
    expect(keysIn("pane.close")).toBe("CtrlW");
    expect((screen.getByRole("button", { name: "Reset all to defaults" }) as HTMLButtonElement).disabled).toBe(true);
  });

  it("F-SH-09.6 Backspace sets None, and a terminal's keys are explained on Windows and Linux", async () => {
    const calls = mount();
    expect(within(row("pane.split_right")).getByText("In a terminal: Ctrl+Shift+E")).toBeTruthy();
    expect(within(row("workspace.new_tab")).getByText("In a terminal, this key goes to the shell.")).toBeTruthy();
    record("pane.split_right", { code: "Backspace", key: "Backspace" });
    await waitFor(() => expect(saves(calls)).toEqual([{ "pane.split_right": "" }]));
    expect(keysIn("pane.split_right")).toBe("None");
    expect(within(row("pane.split_right")).queryByText(/In a terminal/)).toBeNull();
  });

  it("F-SH-09.6 macOS rows have no terminal notes and refuse ⌘Q", () => {
    const calls = mount({}, "macos");
    expect(screen.queryByText(/In a terminal/)).toBeNull();
    record("pane.split_right", { code: "KeyQ", key: "q", metaKey: true });
    expect(within(row("pane.split_right")).getByRole("alert").textContent).toBe("Your system uses this shortcut.");
    record("pane.split_right", { code: "KeyR", key: "r" });
    expect(within(row("pane.split_right")).getByRole("alert").textContent).toBe("Hold ⌘, ⌃ or ⌥ with the key.");
    expect(saves(calls)).toEqual([]);
  });

  it("F-SH-09.6 search finds actions by name or keys, and says when nothing matches", () => {
    mount();
    const search = screen.getByRole("searchbox", { name: "Search shortcuts" });
    fireEvent.change(search, { target: { value: "ctrl+shift+b" } });
    expect(screen.getAllByTestId(/^shortcut-/).map((r) => r.getAttribute("data-testid"))).toEqual(["shortcut-sidebar.toggle"]);
    fireEvent.change(search, { target: { value: "split" } });
    expect(screen.getAllByTestId(/^shortcut-/).map((r) => r.getAttribute("data-testid"))).toEqual(["shortcut-pane.split_right", "shortcut-pane.split_down"]);
    fireEvent.change(search, { target: { value: "zzz" } });
    expect(screen.getByText('No shortcuts match "zzz"')).toBeTruthy();
  });

  it("F-SH-09.6 a failed save says so and keeps the old keys", async () => {
    mount({}, "linux", true);
    record("pane.split_right", { code: "KeyR", key: "r", ctrlKey: true, altKey: true });
    await waitFor(() => expect(within(row("pane.split_right")).getByRole("alert").textContent).toBe("Couldn't save the shortcut."));
    expect(keysIn("pane.split_right")).toBe("CtrlD");
  });
});
