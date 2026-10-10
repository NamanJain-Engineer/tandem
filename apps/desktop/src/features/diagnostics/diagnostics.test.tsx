import { act, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Diagnostics } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { useUi } from "../../stores/ui";
import { fakeBackend, resetStores } from "../../test/fakeBackend";
import { DiagnosticsPanel, REFRESH_MS } from "./DiagnosticsPanel";

const sample = (over: Partial<Diagnostics> = {}): Diagnostics => ({
  uptimeMs: 125_000,
  subsystems: [
    { name: "gateway", state: "running", sinceBeatMs: null, restarts: 1, lastError: "listener closed" },
    { name: "status", state: "running", sinceBeatMs: 400, restarts: 0, lastError: null },
  ],
  panes: [{ paneId: "pn_1", spawnMs: 42, queuedBytes: 2048, exited: false }],
  bus: [
    { topic: "status", queued: 3 },
    { topic: "pty", queued: 0 },
  ],
  ...over,
});

const mount = () =>
  render(
    <UiProvider>
      <DiagnosticsPanel />
    </UiProvider>,
  );

describe("Help › Diagnostics (§5.7)", () => {
  beforeEach(() => {
    resetStores();
    vi.useFakeTimers({ shouldAdvanceTime: true });
  });
  afterEach(() => {
    act(() => useUi.getState().setDiagnostics(false));
    vi.useRealTimers();
  });

  it("§5.7 shows subsystems, terminals and the event bus, live, and says nothing leaves the computer", async () => {
    let n = 0;
    fakeBackend({ app_diagnostics: () => (n++ === 0 ? sample() : sample({ panes: [{ paneId: "pn_1", spawnMs: 42, queuedBytes: 0, exited: false }] })) });
    mount();
    act(() => useUi.getState().setDiagnostics(true));
    const dialog = await screen.findByRole("dialog", { name: "Diagnostics" });
    expect(await within(dialog).findByText("gateway")).toBeTruthy();
    expect(within(dialog).getByText("1 restart in 10 min")).toBeTruthy();
    expect(within(dialog).getByText("listener closed")).toBeTruthy();
    expect(within(dialog).getByText("Started in 42 ms")).toBeTruthy();
    expect(within(dialog).getByText("2048 bytes waiting")).toBeTruthy();
    expect(within(dialog).getByTestId("diag-bus-status").textContent).toContain("3");
    expect(within(dialog).getByText("Measured on this computer and never sent anywhere.")).toBeTruthy();
    expect(within(dialog).getByTestId("diag-fps")).toBeTruthy();
    // It refreshes while open.
    await act(async () => {
      vi.advanceTimersByTime(REFRESH_MS);
    });
    expect(await within(dialog).findByText("0 bytes waiting")).toBeTruthy();
  });

  it("§5.7 loading, then an empty terminal list", async () => {
    let release: (d: Diagnostics) => void = () => undefined;
    fakeBackend({ app_diagnostics: () => new Promise<Diagnostics>((r) => (release = r)) });
    mount();
    act(() => useUi.getState().setDiagnostics(true));
    const dialog = await screen.findByRole("dialog", { name: "Diagnostics" });
    expect(within(dialog).getByRole("status", { name: "Loading diagnostics…" })).toBeTruthy();
    await act(async () => release(sample({ panes: [] })));
    expect(await within(dialog).findByText("No terminals running.")).toBeTruthy();
  });

  it("§5.7 an error says so", async () => {
    fakeBackend({
      app_diagnostics: () => {
        throw { code: "internal", message: "boom", stage: null, hint: null, retryable: false };
      },
    });
    mount();
    act(() => useUi.getState().setDiagnostics(true));
    const dialog = await screen.findByRole("dialog", { name: "Diagnostics" });
    expect(await within(dialog).findByText("Couldn't read diagnostics: boom")).toBeTruthy();
  });
});
