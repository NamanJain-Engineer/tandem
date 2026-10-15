import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { IpcError, PreviewState, Tab, WorkspaceSummary } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { setInvokeBackend } from "../../ipc/invoke";
import { useLayout } from "../../stores/layout";
import { useWorkspaces } from "../../stores/workspaces";
import { comparePreviews, paneLabel, useWorktrees } from "../../stores/worktrees";
import { fakeBackend, resetStores, type Call } from "../../test/fakeBackend";
import { BestOf, useBestOf } from "../bestof/BestOf";
import { WorktreeDialogs, worktreeMenu } from "./Worktrees";

const tab = (ws: string, id = `tb_${ws}`): Tab => ({
  id,
  workspaceId: ws,
  name: "Tab 1",
  sortKey: "a0",
  createdAt: 0,
});
const ws = (id: string, over: Partial<WorkspaceSummary> = {}): WorkspaceSummary =>
  ({
    id,
    kind: "local",
    hostId: "local",
    root: `/code/${id}`,
    name: id,
    parentId: null,
    laneBranch: null,
    missing: false,
    branch: "main",
    tabs: [tab(id)],
    ...over,
  }) as WorkspaceSummary;
const REPO = ws("shop");
const A = ws("wt_a", { parentId: "shop", laneBranch: "tandem/alpha", name: "alpha" });
const B = ws("wt_b", { parentId: "shop", laneBranch: "tandem/beta", name: "beta" });
const C = ws("wt_c", { parentId: "shop", laneBranch: "tandem/gamma", name: "gamma" });

let calls: Call[] = [];
const named = (cmd: string) => calls.filter((c) => c.cmd === cmd);
let status: Record<string, PreviewState["status"]> = {};
let taken: string[] = [];
let paneN = 0;

const conflict = (message: string): IpcError => ({
  code: "conflict",
  message,
  stage: null,
  hint: null,
  retryable: false,
});

beforeEach(() => {
  resetStores();
  status = { wt_a: "ready", wt_b: "ready", wt_c: "none" };
  taken = [];
  paneN = 0;
  useWorktrees.setState({ previews: {}, ask: null });
  useLayout.setState({ tabs: {} });
  calls = fakeBackend({
    workspace_list: () => [REPO, A, B, C],
    layout_get: () => [],
    lane_previews: (a): PreviewState[] =>
      ((a?.laneIds as string[]) ?? []).map((id, i) => ({
        laneId: id,
        status: status[id] ?? "none",
        url: status[id] === "none" ? null : `http://${id.slice(3)}.localhost:${41000 + i * 10}/`,
        port: status[id] === "none" ? null : 41000 + i * 10,
        trusted: true,
      })),
    lane_preview_start: (a) => {
      status[a?.laneId as string] = "ready";
      return { id: "pn_prev", tabId: "tb_wt_b", kind: "terminal", cwd: "/x", engineId: null };
    },
    tab_create: (a) => tab(a?.workspaceId as string, "tb_cmp"),
    tab_rename: () => null,
    pane_create: (a) => {
      paneN += 1;
      return {
        id: `pn_b${paneN}`,
        tabId: a?.tabId,
        kind: "browser",
        cwd: null,
        engineId: null,
        humanName: null,
        url: a?.url,
      };
    },
    pane_rename: (a) => {
      const name = a?.humanName as string;
      if (taken.includes(name)) throw conflict(`Another pane is already called ${name}`);
      taken.push(name);
      return name;
    },
  });
  useWorkspaces.setState({ list: [REPO, A, B, C], activeId: "wt_a", activeTab: {} });
});
afterEach(() => setInvokeBackend(null));

describe("side by side (F-WT-09)", () => {
  it("F-WT-09.5 two answering previews open in a new tab of the repository, side by side, named by branch", async () => {
    const out = await comparePreviews(["wt_a", "wt_b"]);
    expect(out.ok).toBe(true);
    expect(named("tab_create")[0]?.args).toEqual({ workspaceId: "shop" });
    expect(named("pane_create").map((c) => c.args)).toEqual([
      { tabId: "tb_cmp", kind: "browser", url: "http://a.localhost:41000/" },
      { tabId: "tb_cmp", kind: "browser", url: "http://b.localhost:41010/" },
    ]);
    const t = useLayout.getState().tabs["tb_cmp"];
    expect(t?.tree).toEqual({
      type: "split",
      dir: "row",
      ratio: 0.5,
      a: { type: "leaf", paneId: "pn_b1" },
      b: { type: "leaf", paneId: "pn_b2" },
    });
    expect(t?.panes["pn_b1"]?.humanName).toBe("tandem/alpha");
    expect(t?.panes["pn_b2"]?.humanName).toBe("tandem/beta");
    expect(named("tab_rename")[0]?.args).toEqual({ id: "tb_cmp", name: "tandem/alpha | tandem/beta" });
    expect(useWorkspaces.getState().activeId).toBe("shop");
    expect(useWorkspaces.getState().activeTab["shop"]).toBe("tb_cmp");
    // Nothing started: both were running.
    expect(named("lane_preview_start")).toHaveLength(0);
  });

  it("F-WT-09.5 a preview that isn't running is started and waited for", async () => {
    status.wt_b = "down";
    const out = await comparePreviews(["wt_a", "wt_b"], 10_000);
    expect(out.ok).toBe(true);
    expect(named("lane_preview_start")[0]?.args).toEqual({ laneId: "wt_b", tabId: "tb_wt_b" });
    expect(named("pane_create")).toHaveLength(2);
  }, 10_000);

  it("F-WT-09.5 a name another pane already has gets a number; long branches fit the 32-character limit", async () => {
    taken = ["tandem/alpha"];
    await comparePreviews(["wt_a", "wt_b"]);
    expect(useLayout.getState().tabs["tb_cmp"]?.panes["pn_b1"]?.humanName).toBe("tandem/alpha 2");
    expect(paneLabel("tandem/a-very-long-branch-name-for-tests")).toHaveLength(32);
    expect(paneLabel("tandem/a-very-long-branch-name-for-tests", 3)).toMatch(/… 3$/);
  });

  it("F-WT-09.5 a worktree without a preview, or previews that never answer, open nothing", async () => {
    const none = await comparePreviews(["wt_a", "wt_c"]);
    expect(none.ok ? "" : none.error.message).toBe("tandem/gamma has no preview");
    status.wt_b = "down";
    setInvokeBackend(null);
    calls = fakeBackend({
      workspace_list: () => [REPO, A, B, C],
      lane_previews: (a): PreviewState[] =>
        ((a?.laneIds as string[]) ?? []).map((id) => ({
          laneId: id,
          status: id === "wt_a" ? "ready" : "down",
          url: "http://x/",
          port: 1,
          trusted: true,
        })),
      lane_preview_start: () => ({
        id: "pn_prev",
        tabId: "tb_wt_b",
        kind: "terminal",
        cwd: "/x",
        engineId: null,
      }),
    });
    const late = await comparePreviews(["wt_a", "wt_b"], 0);
    expect(late.ok ? "" : late.error.message).toContain("didn't answer within a minute");
    expect(named("tab_create")).toHaveLength(0);
  });

  it('F-WT-09.5 "Compare previews…" in a worktree\'s menu: this one is chosen, pick the other, open', async () => {
    const menu = worktreeMenu(A, "main", () => 0);
    const item = menu.find((e) => "id" in e && e.id === "wt-compare-previews");
    expect(item && "disabled" in item ? item.disabled : null).toBe(false);
    const select = item && "onSelect" in item ? item.onSelect : undefined;
    render(
      <UiProvider>
        <WorktreeDialogs />
      </UiProvider>,
    );
    act(() => select?.());
    const rows = await screen.findAllByTestId("compare-previews-row");
    // gamma's repository has no preview for it: it isn't offered.
    expect(rows.map((r) => r.dataset.branch)).toEqual(["tandem/alpha", "tandem/beta"]);
    const open = screen.getByTestId("compare-previews-open");
    expect(open).toHaveProperty("disabled", true);
    fireEvent.click(within(rows[1]!).getByRole("checkbox"));
    expect(open).toHaveProperty("disabled", false);
    fireEvent.click(open);
    await waitFor(() => expect(screen.queryByTestId("compare-previews")).toBeNull());
    expect(named("pane_create")).toHaveLength(2);
  });

  it("F-WT-09.5 the dialog says so when fewer than two worktrees have a preview", async () => {
    status = { wt_a: "ready", wt_b: "none", wt_c: "none" };
    render(
      <UiProvider>
        <WorktreeDialogs />
      </UiProvider>,
    );
    act(() =>
      useWorktrees
        .getState()
        .open({ kind: "comparePreviews", candidates: ["wt_a", "wt_b", "wt_c"], chosen: ["wt_a"] }),
    );
    expect(await screen.findByTestId("compare-previews-empty")).toBeTruthy();
    expect(screen.getByTestId("compare-previews-open")).toHaveProperty("disabled", true);
  });

  it("F-WT-09.5 Best of N: Side by side offers the run's lanes, both chosen, and closes Compare once open", async () => {
    const RUN = {
      id: "bo_1",
      workspaceId: "shop",
      title: "Fix",
      tabId: null,
      base: "abc",
      checkCommand: null,
      state: "running",
      createdAt: 0,
      lanes: [A, B].map((w, seat) => ({
        seat,
        laneId: w.id,
        branch: w.laneBranch,
        engineId: "claude",
        accountId: null,
        paneId: null,
        error: null,
        check: "none",
        checkExit: null,
        checkOutput: null,
      })),
    };
    setInvokeBackend(null);
    calls = fakeBackend({
      workspace_list: () => [REPO, A, B, C],
      layout_get: () => [],
      bestof_get: () => ({ run: RUN, diffs: [] }),
      lane_previews: (a): PreviewState[] =>
        ((a?.laneIds as string[]) ?? []).map((id) => ({
          laneId: id,
          status: "ready",
          url: `http://${id}/`,
          port: 1,
          trusted: true,
        })),
      tab_create: (a) => tab(a?.workspaceId as string, "tb_cmp"),
      tab_rename: () => null,
      pane_create: (a) => ({
        id: `pn_${String(a?.url)}`,
        tabId: a?.tabId,
        kind: "browser",
        cwd: null,
        engineId: null,
        humanName: null,
      }),
      pane_rename: (a) => a?.humanName,
    });
    render(
      <UiProvider>
        <BestOf />
        <WorktreeDialogs />
      </UiProvider>,
    );
    act(() => useBestOf.getState().openCompare("bo_1"));
    fireEvent.click(await screen.findByTestId("bestof-side-by-side", {}, { timeout: 4000 }));
    const rows = await screen.findAllByTestId("compare-previews-row");
    expect(rows.map((r) => within(r).getByRole("checkbox").getAttribute("aria-checked"))).toEqual([
      "true",
      "true",
    ]);
    fireEvent.click(screen.getByTestId("compare-previews-open"));
    await waitFor(() => expect(useBestOf.getState().compare).toBeNull());
    expect(named("tab_create")[0]?.args).toEqual({ workspaceId: "shop" });
  });
});
