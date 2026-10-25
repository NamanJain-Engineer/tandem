import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { PaneUsage } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { emitLocal } from "../../ipc/events";
import { setInvokeBackend } from "../../ipc/invoke";
import { followResources, formatBytes, formatCpu, useResources } from "../../stores/resources";
import { fakeBackend, resetStores } from "../../test/fakeBackend";
import { ResourceChip } from "../code/ResourceChip";
import { MemoryCapSetting } from "../settings/MemoryCapSetting";
import { PaneUsageSection } from "./PaneUsage";

const usage = (over: Partial<PaneUsage>): PaneUsage => ({
  paneId: "pn_1",
  paneName: "claude",
  workspaceId: "ws_1",
  workspaceName: "app",
  laneBranch: null,
  cpuPercent: null,
  memoryBytes: 300 * 2 ** 20,
  memoryCapBytes: null,
  scope: null,
  frozen: false,
  ...over,
});

beforeEach(() => {
  resetStores();
  useResources.setState({ status: null, statusState: "idle", byPane: {} });
});
afterEach(() => setInvokeBackend(null));

describe("resource governor (F-EX-06)", () => {
  it("formats CPU and memory", () => {
    expect(formatCpu(null)).toBe("—");
    expect(formatCpu(12.4)).toBe("12%");
    expect(formatBytes(840 * 2 ** 20)).toBe("840 MB");
    expect(formatBytes(1536 * 2 ** 20)).toBe("1.5 GB");
  });

  it("F-EX-06.1 the Panes section shows each pane's CPU and memory, grouped by worktree", async () => {
    fakeBackend({
      resources_list: () => [
        usage({ cpuPercent: 12, memoryCapBytes: 512 * 2 ** 20, scope: "vibezeo-pane-pn_1-1.scope" }),
        usage({ paneId: "pn_2", paneName: "codex", laneBranch: "tandem/fix", cpuPercent: null }),
      ],
      resources_status: () => ({ grouping: true, capping: true, freezing: true, reason: null }),
    });
    void useResources.getState().loadStatus();
    render(
      <UiProvider>
        <PaneUsageSection visible />
      </UiProvider>,
    );
    const rows = await screen.findAllByTestId("pane-usage-row");
    expect(rows.map((r) => r.dataset.pane)).toEqual(["pn_1", "pn_2"]);
    expect(screen.getAllByTestId("pane-usage-cpu").map((c) => c.textContent)).toEqual(["CPU 12%", "CPU —"]);
    expect(screen.getAllByTestId("pane-usage-memory")[0]?.textContent).toBe("300 MB of 512 MB");
    expect(screen.getByTestId("pane-usage-group").dataset.branch).toBe("tandem/fix");
  });

  it("F-EX-06.4 Freeze and Thaw call the core and the row follows", async () => {
    const calls: string[] = [];
    let frozen = false;
    fakeBackend({
      resources_list: () => [usage({ frozen })],
      resources_status: () => ({ grouping: false, capping: false, freezing: true, reason: "x" }),
      resources_freeze: () => {
        calls.push("freeze");
        frozen = true;
        return null;
      },
      resources_thaw: () => {
        calls.push("thaw");
        frozen = false;
        return null;
      },
    });
    await useResources.getState().loadStatus();
    render(
      <UiProvider>
        <PaneUsageSection visible />
      </UiProvider>,
    );
    fireEvent.click(await screen.findByTestId("pane-usage-freeze"));
    await waitFor(() => expect(screen.getByTestId("pane-usage-row").dataset.frozen).toBe("true"));
    fireEvent.click(screen.getByTestId("pane-usage-freeze"));
    await waitFor(() => expect(screen.getByTestId("pane-usage-row").dataset.frozen).toBe("false"));
    expect(calls).toEqual(["freeze", "thaw"]);
  });

  it("F-EX-06.4 without Freeze here, the row offers none", async () => {
    fakeBackend({
      resources_list: () => [usage({})],
      resources_status: () => ({
        grouping: false,
        capping: false,
        freezing: false,
        reason: "Not available on this system yet",
      }),
    });
    await useResources.getState().loadStatus();
    render(
      <UiProvider>
        <PaneUsageSection visible />
      </UiProvider>,
    );
    await screen.findByTestId("pane-usage-row");
    expect(screen.queryByTestId("pane-usage-freeze")).toBeNull();
  });

  it("F-EX-06.3 F-EX-06.6 the chip says frozen, stopped at the cap, or not capped, and clears", async () => {
    fakeBackend({ resources_notes: () => [], resources_status: () => null });
    const stop = followResources();
    const view = () => (
      <UiProvider>
        <ResourceChip paneId="pn_1" />
      </UiProvider>
    );
    const { rerender } = render(view());
    act(() =>
      emitLocal("resources:pane", { paneId: "pn_1", frozen: false, cappedAt: "512 MB", notCapped: null }),
    );
    rerender(view());
    expect((await screen.findByTestId("resource-chip")).textContent).toBe("Stopped: memory cap 512 MB");
    act(() =>
      emitLocal("resources:pane", { paneId: "pn_1", frozen: true, cappedAt: null, notCapped: "No session" }),
    );
    rerender(view());
    expect(screen.getAllByTestId("resource-chip").map((c) => c.dataset.kind)).toEqual([
      "frozen",
      "not-capped",
    ]);
    act(() =>
      emitLocal("resources:pane", { paneId: "pn_1", frozen: false, cappedAt: null, notCapped: null }),
    );
    rerender(view());
    expect(screen.queryByTestId("resource-chip")).toBeNull();
    stop();
  });

  it("F-EX-06.2 without caps here, the setting says why and only Off can be chosen", async () => {
    fakeBackend({
      resources_status: () => ({
        grouping: false,
        capping: false,
        freezing: true,
        reason: "Memory caps need a systemd user session.",
      }),
      settings_get_all: () => ({ agent_memory_cap: "off" }),
    });
    render(
      <UiProvider>
        <MemoryCapSetting ids={{ id: "cap" }} />
      </UiProvider>,
    );
    const status = await screen.findByTestId("memory-cap-status");
    expect(status.dataset.available).toBe("false");
    expect(status.textContent).toBe("Memory caps need a systemd user session.");
  });
});
