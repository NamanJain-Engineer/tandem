import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { UiProvider } from "@tandem/ui";
import { setInvokeBackend } from "../../ipc/invoke";
import { fakeBackend, resetStores } from "../../test/fakeBackend";
import { useSettings } from "../../stores/settings";
import { useUi } from "../../stores/ui";
import { SettingsView } from "./SettingsView";

beforeEach(resetStores);
afterEach(() => setInvokeBackend(null));

const open = () =>
  render(
    <UiProvider>
      <button type="button">Opener</button>
      <SettingsView />
    </UiProvider>,
  );

describe("Settings (F-SH-04)", () => {
  it("F-SH-04.4 typing scroll shows Terminal and highlights Scrollback lines", async () => {
    fakeBackend();
    open();
    fireEvent.change(screen.getByLabelText("Filter settings"), { target: { value: "scroll" } });
    await waitFor(() => expect(screen.getByRole("heading", { name: "Terminal" })).toBeTruthy());
    const row = document.querySelector('[data-setting="term_scrollback"]');
    expect(row?.getAttribute("data-highlighted")).toBe("true");
    expect(screen.getAllByRole("button", { name: "Terminal" })).toHaveLength(1);
  });

  it("F-SH-04.4 no match says so", async () => {
    fakeBackend();
    open();
    fireEvent.change(screen.getByLabelText("Filter settings"), { target: { value: "zzzz" } });
    await waitFor(() => expect(screen.getByText('No settings match "zzzz"')).toBeTruthy());
  });

  it("F-SH-04.1 Esc closes Settings and restores focus to the opener", async () => {
    fakeBackend();
    open();
    const opener = screen.getByRole("button", { name: "Opener" });
    opener.focus();
    act(() => useUi.getState().openSettings());
    expect(useUi.getState().settingsOpen).toBe(true);
    (document.activeElement as HTMLElement | null)?.blur();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(useUi.getState().settingsOpen).toBe(false);
    await waitFor(() => expect(document.activeElement).toBe(opener));
  });

  it("F-SH-04.3 Settings reopens on the last page viewed", async () => {
    const calls = fakeBackend();
    useSettings.setState({ values: { settings_last_page: "privacy" }, loaded: true });
    open();
    expect(screen.getByRole("heading", { name: "Privacy & Telemetry" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Appearance" }));
    await waitFor(() =>
      expect(
        calls.some(
          (c) =>
            c.cmd === "settings_set" &&
            c.args?.key === "settings_last_page" &&
            c.args?.value === "appearance",
        ),
      ).toBe(true),
    );
  });

  it("F-SH-04 a failed write reverts the control and offers Retry", async () => {
    fakeBackend({
      settings_set: () => {
        throw { code: "io", message: "disk full" };
      },
    });
    open();
    fireEvent.click(screen.getByRole("button", { name: "General" }));
    const toggle = screen.getByRole("switch", { name: "Launch at login" });
    fireEvent.click(toggle);
    await waitFor(() => expect(screen.getByText("Couldn't save that setting. Try again.")).toBeTruthy());
    expect(toggle.getAttribute("aria-checked")).toBe("false");
    expect(screen.getByRole("button", { name: "Retry" })).toBeTruthy();
  });
});

describe("Privacy (F-TL-01)", () => {
  it("F-TL-01 both toggles are off by default and the flows table follows them", async () => {
    fakeBackend();
    useSettings.setState({ values: { settings_last_page: "privacy" }, loaded: true });
    open();
    const usage = screen.getByRole("switch", { name: "Share anonymous usage statistics" });
    const crash = screen.getByRole("switch", { name: "Send crash reports" });
    expect(usage.getAttribute("aria-checked")).toBe("false");
    expect(crash.getAttribute("aria-checked")).toBe("false");
    const usageRow = () => screen.getByRole("cell", { name: "Usage statistics" }).parentElement;
    expect(usageRow()?.textContent).toContain("Off");
    fireEvent.click(usage);
    await waitFor(() => expect(usageRow()?.textContent).toContain("On"));
  });
});
