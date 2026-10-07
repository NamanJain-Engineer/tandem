import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import type { DashboardItem, UpdateState } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { emitLocal } from "../../ipc/events";
import { followUpdates, useUpdates } from "../../stores/updates";
import { fakeBackend, resetStores, type Call } from "../../test/fakeBackend";
import { Banners } from "../shell/Banners";
import { RestartDialog } from "./RestartDialog";
import { UnsupportedOverlay, hoursLeft } from "./UnsupportedOverlay";
import { UpdatePill } from "./UpdatePill";
import { UpdatesPanel } from "./UpdatesPanel";

const base: UpdateState = {
  stage: "idle",
  current: "0.1.0",
  channel: "stable",
  version: null,
  notes: null,
  percent: null,
  errorStage: null,
  error: null,
  lastCheck: null,
  upToDate: false,
  minVersion: null,
};
const ready: UpdateState = { ...base, stage: "ready", version: "9.9.0", percent: 100 };

const agent = (name: string): DashboardItem => ({
  key: `pane:pn_${name}`,
  source: { type: "pane", paneId: `pn_${name}` },
  status: "working",
  reason: null,
  since: 0,
  title: "api refactor",
  humanName: name,
  engineId: "claude",
  engineLabel: "Claude Code",
  engineLogo: "C",
  accountLabel: null,
  workspaceId: "ws_1",
  workspace: "shop",
  branch: null,
  preview: null,
  pendingApproval: null,
});

function setup(state: UpdateState, extra: Record<string, (a?: Record<string, unknown>) => unknown> = {}): Call[] {
  let current = state;
  return fakeBackend({
    updates_state: () => current,
    updates_check_now: () => current,
    updates_install: () => {
      current = { ...current, stage: "idle" };
      return current;
    },
    updates_set_channel: (a) => {
      current = { ...current, channel: String(a?.channel) };
      return current;
    },
    status_snapshot: () => [],
    ...extra,
  });
}

function mount(node: React.ReactNode) {
  return render(<UiProvider>{node}</UiProvider>);
}

describe("updates (F-SH-06)", () => {
  beforeEach(() => resetStores());

  it("F-SH-06.1 a staged update shows Restart to update; the dialog lists live agents and Restart now installs", async () => {
    const calls = setup(ready, { status_snapshot: () => [agent("Ada"), agent("Bea")] });
    await useUpdates.getState().load();
    mount(
      <>
        <UpdatePill />
        <RestartDialog />
      </>,
    );
    const pill = screen.getByTestId("update-pill");
    expect(pill.textContent).toBe("Restart to update");
    fireEvent.click(pill);
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByText("Restart to update to 9.9.0?")).toBeTruthy();
    await waitFor(() => expect(within(dialog).getAllByTestId("update-live-agent").map((li) => li.textContent)).toEqual(["Adashop", "Beashop"]));
    expect(within(dialog).getByText(/Agents stop and can be resumed after the restart/)).toBeTruthy();
    fireEvent.click(within(dialog).getByRole("button", { name: "Restart now" }));
    await waitFor(() => expect(calls.some((c) => c.cmd === "updates_install")).toBe(true));
  });

  it("F-SH-06.1 with nothing running the dialog says Tandem restarts in a few seconds; Later keeps the pill", async () => {
    setup(ready);
    await useUpdates.getState().load();
    mount(
      <>
        <UpdatePill />
        <RestartDialog />
      </>,
    );
    fireEvent.click(screen.getByTestId("update-pill"));
    const dialog = await screen.findByRole("dialog");
    await waitFor(() => expect(within(dialog).getByText("Tandem will restart in a few seconds.")).toBeTruthy());
    fireEvent.click(within(dialog).getByRole("button", { name: "Later" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(screen.getByTestId("update-pill")).toBeTruthy();
  });

  it("F-SH-06.2 a bad signature shows the failed pill at the verify stage and says it can't be verified", async () => {
    setup({ ...base, stage: "error", version: "9.9.0", errorStage: "verify", error: "Update can't be verified, so it wasn't installed." });
    await useUpdates.getState().load();
    mount(
      <>
        <UpdatePill />
        <UpdatesPanel />
      </>,
    );
    const pill = screen.getByTestId("update-pill");
    expect(pill.textContent).toBe("Update failed — Retry");
    expect(pill.dataset.stage).toBe("verify");
    expect(screen.getByTestId("updates-status").textContent).toBe("Update can't be verified, so it wasn't installed.");
  });

  it("F-SH-06.3 nothing newer shows no pill and You're up to date", async () => {
    setup({ ...base, upToDate: true });
    await useUpdates.getState().load();
    mount(
      <>
        <UpdatePill />
        <UpdatesPanel />
      </>,
    );
    expect(screen.queryByTestId("update-pill")).toBeNull();
    expect(screen.getByTestId("updates-status").textContent).toBe("You're up to date");
  });

  it("F-SH-06 a failed check never shows the pill; downloads report progress", async () => {
    setup({ ...base, stage: "error", errorStage: "check" });
    followUpdates();
    await useUpdates.getState().load();
    mount(
      <>
        <UpdatePill />
        <UpdatesPanel />
      </>,
    );
    expect(screen.queryByTestId("update-pill")).toBeNull();
    expect(screen.getByTestId("updates-status").textContent).toBe("Couldn't check for updates.");
    act(() => emitLocal("updates:state", { ...base, stage: "downloading", version: "9.9.0", percent: 42 }));
    expect(screen.getByTestId("updates-status").textContent).toBe("Downloading 9.9.0… 42%");
    expect(screen.getByRole("progressbar")).toBeTruthy();
  });

  it("F-SH-06 deb installs say the package manager handles updates", async () => {
    setup({ ...base, stage: "managed" });
    await useUpdates.getState().load();
    mount(<UpdatesPanel />);
    expect(screen.getByTestId("updates-panel").textContent).toContain("Updates are managed by your package manager.");
    expect(screen.queryByTestId("updates-check-now")).toBeNull();
  });

  it("F-SH-06 the channel switches through the core", async () => {
    const calls = setup(base);
    await useUpdates.getState().load();
    mount(<UpdatesPanel />);
    fireEvent.click(screen.getByRole("radio", { name: "Beta" }));
    await waitFor(() => expect(calls.find((c) => c.cmd === "updates_set_channel")?.args).toEqual({ channel: "beta" }));
  });

  it("F-SH-06.4 during the grace a banner keeps terminals usable; after it the overlay is modal", async () => {
    const graceUntil = Date.now() + 71 * 3_600_000 + 60_000;
    setup({ ...base, minVersion: { minVersion: "9.0.0", graceUntil, modal: false } });
    await useUpdates.getState().load();
    const { rerender } = mount(
      <>
        <Banners />
        <UnsupportedOverlay />
      </>,
    );
    expect(screen.getByTestId("banner-unsupported").textContent).toContain("Terminals keep working for 72 more hours.");
    expect(screen.queryByTestId("unsupported-overlay")).toBeNull();
    act(() => useUpdates.setState({ state: { ...base, minVersion: { minVersion: "9.0.0", graceUntil: Date.now() - 1, modal: true } } }));
    rerender(
      <UiProvider>
        <Banners />
        <UnsupportedOverlay />
      </UiProvider>,
    );
    const overlay = screen.getByTestId("unsupported-overlay");
    expect(within(overlay).getByRole("heading").textContent).toBe("This version of Tandem is no longer supported");
    expect(within(overlay).getByRole("button", { name: "Update now" })).toBeTruthy();
    expect(screen.queryByTestId("banner-unsupported")).toBeNull();
  });

  it("hours left round up and never go negative", () => {
    expect(hoursLeft(10 * 3_600_000 + 1, 0)).toBe(11);
    expect(hoursLeft(0, 5)).toBe(0);
  });
});
