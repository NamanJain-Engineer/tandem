import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { BestOfLane, BestOfRun, PreviewState, Tab, WorkspaceSummary } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { setInvokeBackend } from "../../ipc/invoke";
import { useEngines } from "../../stores/engines";
import { useSidebar } from "../../stores/sidebar";
import { useWorkspaces } from "../../stores/workspaces";
import { useWorktrees } from "../../stores/worktrees";
import { fakeBackend, resetStores, type Call } from "../../test/fakeBackend";
import { BestOf, useBestOf } from "./BestOf";

const tab = (ws: string): Tab => ({ id: `tb_${ws}`, workspaceId: ws, name: "Tab 1", sortKey: "a0", createdAt: 0 });
const WS = { id: "ws_1", kind: "local", root: "/work/shop", name: "shop", parentId: null, laneBranch: null, tabs: [tab("ws_1")], branch: "main" } as unknown as WorkspaceSummary;
const L0 = { ...WS, id: "ws_l0", parentId: "ws_1", laneBranch: "tandem/fix-1", name: "fix-1", tabs: [tab("ws_l0")] } as WorkspaceSummary;
const L1 = { ...WS, id: "ws_l1", parentId: "ws_1", laneBranch: "tandem/fix-2", name: "fix-2", tabs: [tab("ws_l1")] } as WorkspaceSummary;

function lane(seat: number): BestOfLane {
  return { seat, laneId: `ws_l${seat}`, branch: `tandem/fix-${seat + 1}`, engineId: "claude", accountId: null, paneId: `pn_${seat}`, error: null, check: "none", checkExit: null, checkOutput: null };
}
const RUN: BestOfRun = { id: "bo_1", workspaceId: "ws_1", title: "Fix", tabId: null, base: "abc", checkCommand: null, state: "running", createdAt: 0, lanes: [lane(0), lane(1)] };

let calls: Call[] = [];
let l0Ready = false;
const named = (cmd: string) => calls.filter((c) => c.cmd === cmd);

beforeEach(() => {
  resetStores();
  l0Ready = false;
  useWorktrees.setState({ previews: {} });
  calls = fakeBackend({
    workspace_list: () => [WS, L0, L1],
    layout_get: () => [],
    bestof_get: () => ({ run: RUN, diffs: [] }),
    lane_previews: (): PreviewState[] => [
      { laneId: "ws_l0", status: l0Ready ? "ready" : "down", url: "http://fix-1.localhost:41000/", port: 41000, trusted: true },
      { laneId: "ws_l1", status: "none", url: null, port: null, trusted: false },
    ],
    lane_preview_start: () => ({ id: "pn_prev", tabId: "tb_ws_l0", kind: "terminal", cwd: "/x", engineId: null }),
  });
  useWorkspaces.setState({ list: [WS, L0, L1], activeId: "ws_1", activeTab: {} });
  useEngines.setState({ list: [], loaded: true });
});
afterEach(() => setInvokeBackend(null));

describe("previews in Compare (F-WT-09)", () => {
  it("F-WT-09.4 Previews starts each lane's preview and opens a tab per lane once it answers; a lane without one says so", async () => {
    render(
      <UiProvider>
        <BestOf />
      </UiProvider>,
    );
    act(() => useBestOf.getState().openCompare("bo_1"));
    const button = await screen.findByTestId("bestof-previews", {}, { timeout: 4000 });
    const columns = screen.getAllByTestId("bestof-column");
    expect(within(columns[1]!).getByTestId("bestof-preview-none").textContent).toBe("No preview");
    fireEvent.click(button);
    await waitFor(() => expect(named("lane_preview_start")).toHaveLength(1));
    expect(named("lane_preview_start")[0]?.args).toEqual({ laneId: "ws_l0", tabId: "tb_ws_l0" });
    expect(await screen.findByTestId("bestof-preview-starting")).toBeTruthy();
    l0Ready = true;
    await waitFor(
      () => expect(useSidebar.getState().tabs.find((x) => x.kind === "browser")?.url).toBe("http://fix-1.localhost:41000/"),
      { timeout: 5000 },
    );
    expect(useSidebar.getState().tabs.filter((x) => x.kind === "browser")).toHaveLength(1);
    expect(await screen.findByTestId("bestof-preview-open")).toBeTruthy();
  });
});
