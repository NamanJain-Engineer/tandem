import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { DashboardItem, Tab, WorkspaceSummary } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { emitLocal } from "../../ipc/events";
import { setInvokeBackend } from "../../ipc/invoke";
import { useDashboard } from "../../stores/dashboard";
import { useLayout } from "../../stores/layout";
import { useStatus } from "../../stores/status";
import { useWorkspaces } from "../../stores/workspaces";
import { useWorktrees } from "../../stores/worktrees";
import { fakeBackend, resetStores, type Call } from "../../test/fakeBackend";
import { RadarMark, useIntentRadar } from "./IntentRadar";
import { showsStatus, useWorkspaceStatus } from "./workspaceStatus";

const tab = (ws: string): Tab => ({
  id: `tb_${ws}`,
  workspaceId: ws,
  name: "Tab 1",
  sortKey: "a0",
  createdAt: 0,
});
const wt = (id: string, branch: string) =>
  ({
    id,
    kind: "local",
    root: `/code/shop/.tandem/worktrees/${id}`,
    name: id,
    parentId: "shop",
    laneBranch: branch,
    missing: false,
    branch,
    liveCount: 0,
    tabs: [tab(id)],
  }) as unknown as WorkspaceSummary;
const A = wt("wt_a", "tandem/rename");
const B = wt("wt_b", "tandem/caller");

const agent = (paneId: string, workspaceId: string): DashboardItem => ({
  key: `pane:${paneId}`,
  source: { type: "pane", paneId },
  status: "working",
  reason: null,
  since: 0,
  title: null,
  humanName: "Ada",
  engineId: "claude",
  engineLabel: "Claude Code",
  engineLogo: null,
  accountLabel: null,
  workspaceId,
  workspace: null,
  branch: null,
  preview: null,
  pendingApproval: null,
});

let calls: Call[] = [];
const named = (cmd: string) => calls.filter((c) => c.cmd === cmd);
let marks: Array<{ laneId: string; symbols: string[] }> = [];

/** A mark as its row draws it: beside the row's status or not. */
function Mark({ wt }: { wt: WorkspaceSummary }) {
  return <RadarMark wt={wt} beside={showsStatus(useWorkspaceStatus(wt))} />;
}

function Radar({ ids }: { ids: string[] }) {
  useIntentRadar(ids, true);
  return (
    <>
      <Mark wt={A} />
      <Mark wt={B} />
    </>
  );
}

beforeEach(() => {
  resetStores();
  marks = [];
  useWorktrees.setState({ radar: {}, ask: null });
  useDashboard.setState({ items: {} });
  useWorkspaces.setState({ list: [A, B], activeId: "shop", activeTab: {} });
  calls = fakeBackend({
    lane_radar: () => marks,
    lane_radar_note: (a) => `NOTE for ${String(a?.laneId)} about ${String(a?.otherId)}`,
    pty_type_prompt: () => null,
  });
});
afterEach(() => setInvokeBackend(null));

const overlap = {
  repoId: "shop",
  a: { laneId: "wt_b", branch: "tandem/caller" },
  b: { laneId: "wt_a", branch: "tandem/rename" },
  symbols: ["Session"],
};

describe("Intent radar (F-WT-10)", () => {
  it("F-WT-10.1 the rail marks the worktrees that share symbols, and the mark opens Check overlap", async () => {
    marks = [
      { laneId: "wt_a", symbols: ["Session"] },
      { laneId: "wt_b", symbols: ["Session", "Session.refresh"] },
    ];
    render(
      <UiProvider>
        <Radar ids={["wt_a", "wt_b"]} />
      </UiProvider>,
    );
    const found = await screen.findAllByTestId("worktree-radar");
    // D-2700: the symbol, or how many; its name starts with those words
    // (D-2701, WCAG 2.5.3) and says what they mean.
    expect(found.map((m) => m.textContent)).toEqual(["Session", "2 symbols"]);
    expect(found.map((m) => m.getAttribute("aria-label"))).toEqual([
      "Session, shared with another worktree",
      "2 symbols shared with another worktree: Session, Session.refresh",
    ]);
    // A symbol is code, and a long one is cut at its end; a count is never cut.
    expect(found[0]!.querySelector(".font-mono")?.textContent).toBe("Session");
    expect(found[0]!.querySelector(".font-mono")?.className).toContain("truncate");
    expect(found[1]!.querySelector(".truncate")).toBeNull();
    expect(named("lane_radar")[0]?.args).toEqual({ laneIds: ["wt_a", "wt_b"] });
    fireEvent.click(found[0]!);
    expect(useWorktrees.getState().ask).toEqual({ kind: "overlap", worktreeId: "wt_a" });
  });

  it("F-WT-10.1 D-2613 D-2700 beside the row's status, or folded, the mark is the radar alone, its name and title kept", async () => {
    marks = [
      { laneId: "wt_a", symbols: ["Session"] },
      { laneId: "wt_b", symbols: ["Session", "Session.refresh"] },
    ];
    // wt_a's agent is working: its row says "● 1 working" (D-2603).
    useLayout.setState({
      tabs: {
        tb_wt_a: { tree: null, active: null, maximized: null, panes: { p1: {} as never }, keepEmpty: false },
      },
    });
    useStatus.setState({
      records: {
        "pane:p1": {
          key: "pane:p1",
          status: "working",
          reason: null,
          since: 1,
          ts: 1,
          tool: null,
          title: null,
          preview: null,
        },
      },
    });
    const row = vi.fn();
    render(
      <UiProvider>
        <div onClick={row} onKeyDown={row}>
          <Radar ids={["wt_a", "wt_b"]} />
        </div>
      </UiProvider>,
    );
    const busy = await screen.findByRole("button", { name: "Session, shared with another worktree" });
    const idle = screen.getByRole("button", {
      name: "2 symbols shared with another worktree: Session, Session.refresh",
    });
    expect(busy.hasAttribute("data-compact")).toBe(true);
    expect(busy.textContent).toBe("");
    expect(busy.querySelector("svg")).not.toBeNull();
    expect(busy.title).toBe("Session, shared with another worktree");
    expect(idle.hasAttribute("data-compact")).toBe(false);
    expect(idle.textContent).toBe("2 symbols");
    expect(idle.title).toBe("2 symbols shared with another worktree: Session, Session.refresh");
    // The mark opens Check overlap, not the row under it.
    fireEvent.keyDown(busy, { key: "Enter" });
    fireEvent.click(busy);
    expect(useWorktrees.getState().ask).toEqual({ kind: "overlap", worktreeId: "wt_a" });
    expect(row).not.toHaveBeenCalled();
  });

  it("F-WT-10.1 D-2700 a mark its row folds is the radar alone, never its words cut short", () => {
    useWorktrees.setState({ radar: { wt_b: ["Session", "Session.refresh"] } });
    render(
      <UiProvider>
        <RadarMark wt={B} beside={false} fold />
      </UiProvider>,
    );
    const mark = screen.getByRole("button", {
      name: "2 symbols shared with another worktree: Session, Session.refresh",
    });
    expect(mark.hasAttribute("data-compact")).toBe(true);
    expect(mark.textContent).toBe("");
    expect(mark.title).toBe("2 symbols shared with another worktree: Session, Session.refresh");
  });

  it("F-WT-10.7 a watched change reloads the rail's marks, with no notice of its own", async () => {
    render(
      <UiProvider>
        <Radar ids={["wt_a", "wt_b"]} />
      </UiProvider>,
    );
    await waitFor(() => expect(named("lane_radar")).toHaveLength(1));
    expect(screen.queryAllByTestId("worktree-radar")).toHaveLength(0);
    marks = [{ laneId: "wt_a", symbols: ["parse"] }];
    act(() => emitLocal("radar:changed", { laneIds: ["wt_a"] }));
    await waitFor(() => expect(named("lane_radar")).toHaveLength(2));
    expect(await screen.findByTestId("worktree-radar")).toBeTruthy();
    expect(screen.queryByText("Warn both agents")).toBeNull();
  });

  it("F-WT-10.4 a new overlap shows one notice; Warn both agents types each note only on the click", async () => {
    useDashboard.setState({
      items: { "pane:pn_a": agent("pn_a", "wt_a"), "pane:pn_b": agent("pn_b", "wt_b") },
    });
    render(
      <UiProvider>
        <Radar ids={["wt_a", "wt_b"]} />
      </UiProvider>,
    );
    await waitFor(() => expect(named("lane_radar")).toHaveLength(1));
    marks = [{ laneId: "wt_a", symbols: ["Session"] }];
    act(() => emitLocal("radar:overlap", overlap));
    expect(await screen.findByText("tandem/caller and tandem/rename both touch Session")).toBeTruthy();
    await waitFor(() => expect(named("lane_radar")).toHaveLength(2));
    expect(named("pty_type_prompt")).toHaveLength(0);
    fireEvent.click(screen.getByRole("button", { name: "Warn both agents" }));
    await waitFor(() => expect(named("pty_type_prompt")).toHaveLength(2));
    expect(named("pty_type_prompt").map((c) => c.args)).toEqual(
      expect.arrayContaining([
        { paneId: "pn_b", text: "NOTE for wt_b about wt_a" },
        { paneId: "pn_a", text: "NOTE for wt_a about wt_b" },
      ]),
    );
    expect(await screen.findByText("Warned both agents")).toBeTruthy();
  });

  it("F-WT-10.4 a worktree without an agent is named, and the other is still warned", async () => {
    useDashboard.setState({ items: { "pane:pn_a": agent("pn_a", "wt_a") } });
    render(
      <UiProvider>
        <Radar ids={["wt_a", "wt_b"]} />
      </UiProvider>,
    );
    await waitFor(() => expect(named("lane_radar")).toHaveLength(1));
    act(() => emitLocal("radar:overlap", overlap));
    fireEvent.click(await screen.findByRole("button", { name: "Warn both agents" }));
    expect(await screen.findByText("No agent is running in tandem/caller")).toBeTruthy();
    expect(named("pty_type_prompt").map((c) => c.args)).toEqual([
      { paneId: "pn_a", text: "NOTE for wt_a about wt_b" },
    ]);
  });
});
