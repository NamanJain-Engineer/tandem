import { act, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { FileDiff, Tab, WorkspaceSummary } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { setInvokeBackend } from "../../ipc/invoke";
import { isMultiRepo, isRepo } from "../../stores/launch";
import { useLayout } from "../../stores/layout";
import { useWorkspaces } from "../../stores/workspaces";
import { useWorktrees } from "../../stores/worktrees";
import { fakeBackend, resetStores, type Call } from "../../test/fakeBackend";
import { CodeRail } from "./CodeRail";
import { WorktreeDialogs, worktreeMenu } from "./Worktrees";

function ws(id: string, over: Partial<WorkspaceSummary> = {}): WorkspaceSummary {
  const tab: Tab = { id: `tb_${id}`, workspaceId: id, name: "Tab 1", sortKey: "a0", createdAt: 0 };
  return {
    id,
    kind: "local",
    hostId: "local",
    root: `/code/${id}`,
    wslDistro: null,
    name: id,
    parentId: null,
    laneBranch: null,
    laneBase: null,
    sortKey: "a0",
    pinned: false,
    collapsed: false,
    sessionsCollapsed: false,
    createdAt: 0,
    missing: false,
    branch: null,
    liveCount: 0,
    tabs: [tab],
    ...over,
  };
}

const SHOP = ws("shop", { repos: ["api", "web"] });
const LANE = ws("wt_1", {
  parentId: "shop",
  laneBranch: "tandem/tidy",
  laneBase: "",
  root: "/code/shop/.tandem/worktrees/tidy",
  name: "tidy",
  repos: ["api", "web"],
});

const file = (path: string): FileDiff =>
  ({
    path,
    change: "modified",
    additions: 1,
    deletions: 0,
    binary: false,
    truncated: false,
    patch: `diff --git a/${path} b/${path}\n--- a/${path}\n+++ b/${path}\n@@ -1 +1,2 @@\n one\n+two\n`,
  }) as FileDiff;

let calls: Call[] = [];

beforeEach(() => {
  resetStores();
  useWorktrees.setState({ ask: null });
  useLayout.setState({ tabs: {}, loaded: {}, boxes: {}, dragging: false, picker: null });
  useWorkspaces.setState({ list: [SHOP, LANE], activeId: "shop", activeTab: {} });
  calls = fakeBackend({
    workspace_list: () => [SHOP, LANE],
    layout_get: () => [],
    sessions_saved_list: () => [],
    lane_diff: () => [file("api/src/a.ts"), file("api/src/b.ts")],
  });
});
afterEach(() => setInvokeBackend(null));

describe("multi-repo workspaces (F-CD-22)", () => {
  it("F-CD-22.1 a folder holding repositories counts as one for worktrees, and the rail says how many", () => {
    expect(isMultiRepo(SHOP)).toBe(true);
    expect(isRepo(SHOP)).toBe(true);
    expect(isMultiRepo(LANE)).toBe(false);
    expect(isMultiRepo(ws("notes"))).toBe(false);
    render(
      <UiProvider>
        <CodeRail />
      </UiProvider>,
    );
    const row = screen.getAllByTestId("workspace-row")[0]!;
    expect(within(row).getByTestId("workspace-repos").textContent).toBe("2 repositories");
  });

  it("F-CD-22 a multi-repo worktree's menu keeps diff, files and discard, and says why the rest are off", () => {
    const menu = worktreeMenu(LANE, "main", () => 0);
    expect(menu.map((e) => e.id).slice(0, 4)).toEqual(["wt-diff", "wt-files", "wt-prs", "wt-multi-note"]);
    expect(menu[3]).toEqual({
      id: "wt-multi-note",
      type: "label",
      label: "Not available for multi-repo worktrees yet",
    });
    const state = (id: string) => {
      const e = menu.find((x) => "id" in x && x.id === id);
      return e && "disabled" in e ? Boolean(e.disabled) : false;
    };
    expect(state("wt-diff")).toBe(false);
    expect(state("wt-files")).toBe(false);
    expect(state("wt-discard")).toBe(false);
    expect(state("wt-prs")).toBe(false);
    for (const id of ["wt-merge", "wt-pr", "wt-overlap", "wt-land", "wt-setup", "wt-second-opinion"]) {
      expect(state(id), id).toBe(true);
    }
    // An ordinary worktree's menu has neither.
    const plain = worktreeMenu({ ...LANE, repos: undefined }, "main", () => 0);
    expect(plain.some((e) => e.id === "wt-multi-note" || e.id === "wt-prs")).toBe(false);
  });

  it("F-CD-22.3 the diff names each repository's share, and its files carry the repository's name", async () => {
    render(
      <UiProvider>
        <WorktreeDialogs />
      </UiProvider>,
    );
    useWorktrees.getState().open({ kind: "diff", worktreeId: "wt_1" });
    const repos = await screen.findAllByTestId("diff-repo");
    expect(repos.map((r) => r.textContent)).toEqual(["api · 2 files", "web · No changes"]);
    expect(screen.queryByTestId("review-second-opinion")).toBeNull();
  });

  it("F-CD-22.4 Create pull requests starts the run through the core's confirm and shows each repository's step", async () => {
    calls = fakeBackend({
      workspace_list: () => [SHOP, LANE],
      lane_create_prs: () => ({ laneId: "wt_1", branch: "tandem/tidy", running: true, steps: [] }),
      lane_prs_get: () => ({
        laneId: "wt_1",
        branch: "tandem/tidy",
        running: false,
        steps: [
          { repo: "api", state: "done", url: "https://github.com/o/api/pull/1", reason: null },
          { repo: "web", state: "skipped", url: null, reason: "no new commits" },
        ],
      }),
    });
    render(
      <UiProvider>
        <WorktreeDialogs />
      </UiProvider>,
    );
    const prs = worktreeMenu(LANE, "main", () => 0).find((e) => e.id === "wt-prs");
    act(() => {
      if (prs && "onSelect" in prs) prs.onSelect?.();
    });
    const steps = await screen.findAllByTestId("prs-step");
    expect(calls.filter((c) => c.cmd === "lane_create_prs")[0]?.args).toEqual({ laneId: "wt_1" });
    expect(steps.map((s) => [s.dataset.repo, s.dataset.state])).toEqual([
      ["api", "done"],
      ["web", "skipped"],
    ]);
    expect(steps[0]?.textContent).toContain("Opened");
    expect(steps[1]?.textContent).toContain("Skipped: no new commits");
    expect(screen.getByTestId("prs-link").getAttribute("href")).toBe("https://github.com/o/api/pull/1");
  });

  it("F-CD-22.4 a Cancel on the confirm opens nothing and says nothing", async () => {
    calls = fakeBackend({
      workspace_list: () => [SHOP, LANE],
      lane_create_prs: () => {
        throw { code: "cancelled", message: "Nothing was pushed", stage: null, hint: null, retryable: false };
      },
    });
    const shown: unknown[] = [];
    const prs = worktreeMenu(LANE, "main", (x) => {
      shown.push(x);
      return 0;
    }).find((e) => e.id === "wt-prs");
    await act(async () => {
      if (prs && "onSelect" in prs) prs.onSelect?.();
      await new Promise((r) => setTimeout(r, 10));
    });
    expect(useWorktrees.getState().ask).toBeNull();
    expect(shown).toEqual([]);
  });
});
