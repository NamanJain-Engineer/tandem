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

  it("F-TL-04.2 About › Licenses shows the notices the build carries", async () => {
    fakeBackend({ app_licenses: () => "# Third-party notices\n\nParakeet TDT 0.6B v3 by NVIDIA, CC BY 4.0\nGNU Lesser General Public License" });
    open();
    fireEvent.change(screen.getByLabelText("Filter settings"), { target: { value: "diagnostics" } });
    fireEvent.click(await screen.findByTestId("open-licenses"));
    await waitFor(() => expect(screen.getByTestId("licenses-text").textContent).toContain("Parakeet TDT 0.6B v3 by NVIDIA"));
    expect(screen.getByRole("dialog", { name: "Licenses" })).toBeTruthy();
  });

  it("F-SH-06 Settings › Updates is a page with automatic checks and Check now", async () => {
    fakeBackend({
      updates_state: () => ({ stage: "idle", current: "0.1.0", channel: "stable", version: null, notes: null, percent: null, errorStage: null, error: null, lastCheck: null, upToDate: false, minVersion: null }),
    });
    open();
    fireEvent.change(screen.getByLabelText("Filter settings"), { target: { value: "channel" } });
    await waitFor(() => expect(screen.getByRole("heading", { name: "Updates" })).toBeTruthy());
    expect(screen.getByTestId("updates-check-now")).toBeTruthy();
    expect(document.querySelector('[data-setting="auto_check_updates"]')).toBeTruthy();
  });
});
