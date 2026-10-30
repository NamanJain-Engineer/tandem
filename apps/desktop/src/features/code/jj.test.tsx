import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { LaneKinds, Tab, WorkspaceSummary } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { setInvokeBackend } from "../../ipc/invoke";
import { useLayout } from "../../stores/layout";
import { useWorkspaces } from "../../stores/workspaces";
import { useWorktrees } from "../../stores/worktrees";
import { fakeBackend, resetStores, type Call } from "../../test/fakeBackend";
import { WorktreeDialogs, WorktreeRow, worktreeMenu } from "./Worktrees";

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
    branch: "main",
    liveCount: 0,
    tabs: [tab],
    ...over,
  };
}

const APP = ws("app");
const JJ_LANE = ws("wt_1", {
  parentId: "app",
  laneBranch: "tidy",
  laneBase: "abc",
  root: "/code/app/.tandem/worktrees/tidy",
  name: "tidy",
  branch: null,
  laneVcs: "jj",
});

let kinds: LaneKinds;
let calls: Call[] = [];

function setup(list: WorkspaceSummary[] = [APP, JJ_LANE]) {
  useWorkspaces.setState({ list, activeId: "app", activeTab: {} });
  calls = fakeBackend({
    workspace_list: () => list,
    layout_get: () => [],
    sessions_saved_list: () => [],
    lane_kind_get: () => kinds,
    lane_create: () => ({ ...JJ_LANE, id: "wt_2", laneBranch: "fix-login" }),
    lane_setup: () => null,
    lsp_trust: () => {
      kinds = { ...kinds, trusted: true };
      return null;
    },
  });
}

beforeEach(() => {
  resetStores();
  useWorktrees.setState({ ask: null });
  useLayout.setState({ tabs: {}, loaded: {}, boxes: {}, dragging: false, picker: null });
  kinds = {
    colocated: true,
    jjVersion: "0.34.0",
    jjOnly: false,
    remote: false,
    preferred: "jj",
    trusted: true,
  };
});
afterEach(() => setInvokeBackend(null));

function openNew(id = "app") {
  render(
    <UiProvider>
      <WorktreeDialogs />
    </UiProvider>,
  );
  useWorktrees.getState().open({ kind: "new", workspaceId: id });
}

describe("jj workspaces (F-CD-24)", () => {
  it("F-CD-24.1 New worktree preselects jj workspace in a co-located repository and asks for one", async () => {
    setup();
    openNew();
    const group = await screen.findByRole("radiogroup", { name: "Make" });
    const jj = within(group).getByRole("radio", { name: "jj workspace" });
    await waitFor(() => expect(jj.getAttribute("aria-checked")).toBe("true"));
    fireEvent.change(screen.getByTestId("worktree-name"), { target: { value: "fix login" } });
    fireEvent.click(screen.getByTestId("worktree-create"));
    await waitFor(() => expect(calls.some((c) => c.cmd === "lane_create")).toBe(true));
    expect(calls.find((c) => c.cmd === "lane_create")?.args).toEqual({
      workspaceId: "app",
      slugSource: "fix login",
      kind: "jj",
    });
  });

  it("F-CD-24.1 the remembered choice comes back, and git worktree is one click away", async () => {
    kinds = { ...kinds, preferred: "git" };
    setup();
    openNew();
    const group = await screen.findByRole("radiogroup", { name: "Make" });
    const git = within(group).getByRole("radio", { name: "git worktree" });
    await waitFor(() => expect(git.getAttribute("aria-checked")).toBe("true"));
    fireEvent.click(within(group).getByRole("radio", { name: "jj workspace" }));
    fireEvent.click(screen.getByTestId("worktree-create"));
    await waitFor(() =>
      expect(calls.find((c) => c.cmd === "lane_create")?.args).toMatchObject({ kind: "jj" }),
    );
  });

  it("F-CD-24 jj waits for the repository's trust, and Trust repository… asks for it", async () => {
    kinds = { ...kinds, trusted: false };
    setup();
    openNew();
    expect(
      await screen.findByText(
        "Vibezeo runs jj only in repositories you trust, since jj reads the repository's own settings.",
      ),
    ).toBeTruthy();
    expect(screen.queryByRole("radiogroup", { name: "Make" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Trust repository…" }));
    await screen.findByRole("radiogroup", { name: "Make" });
    expect(calls.find((c) => c.cmd === "lsp_trust")?.args).toEqual({
      context: { type: "workspace", workspaceId: "app" },
      path: ".",
    });
  });

  it("F-CD-24 a .jj that isn't jj's own is refused, and says why", async () => {
    kinds = {
      colocated: false,
      jjOnly: false,
      remote: false,
      preferred: "git",
      trusted: true,
      refused: "This repository's .jj folder is committed to git, so Vibezeo won't run jj in it",
    };
    setup();
    openNew();
    expect(
      await screen.findByText(
        "This repository's .jj folder is committed to git, so Vibezeo won't run jj in it",
      ),
    ).toBeTruthy();
    expect(screen.queryByRole("radiogroup", { name: "Make" })).toBeNull();
  });

  it("F-CD-24.5 a jj-only repository says why, and offers no jj workspace", async () => {
    kinds = { colocated: false, jjOnly: true, remote: false, preferred: "git", trusted: false };
    setup();
    openNew();
    expect(
      await screen.findByText(
        "This repository uses jj without git. Vibezeo works with jj in repositories co-located with git.",
      ),
    ).toBeTruthy();
    expect(screen.queryByRole("radiogroup", { name: "Make" })).toBeNull();
  });

  it("F-CD-24.5 a folder on another machine shows nothing about jj, and asks without a kind", async () => {
    kinds = { colocated: false, jjOnly: false, remote: true, preferred: "git", trusted: false };
    setup();
    openNew();
    await waitFor(() => expect(calls.some((c) => c.cmd === "lane_kind_get")).toBe(true));
    expect(screen.queryByRole("radiogroup", { name: "Make" })).toBeNull();
    fireEvent.click(screen.getByTestId("worktree-create"));
    await waitFor(() => expect(calls.some((c) => c.cmd === "lane_create")).toBe(true));
    expect(calls.find((c) => c.cmd === "lane_create")?.args).not.toHaveProperty("kind");
  });

  it("F-CD-24.1 a jj workspace's rail row is marked jj", () => {
    setup();
    render(
      <UiProvider>
        <WorktreeRow wt={JJ_LANE} base="main" />
      </UiProvider>,
    );
    expect(screen.getByTestId("worktree-jj").textContent).toBe("jj");
  });

  it("F-CD-24.3 a jj workspace's menu keeps diff, files, overlap and discard, and says why the rest are off", () => {
    setup();
    const menu = worktreeMenu(JJ_LANE, "main", () => 0);
    expect(menu.map((e) => e.id).slice(0, 3)).toEqual(["wt-diff", "wt-files", "wt-jj-note"]);
    expect(menu[2]).toEqual({
      id: "wt-jj-note",
      type: "label",
      label: "Not available for jj workspaces yet",
    });
    const off = (id: string) => {
      const e = menu.find((x) => "id" in x && x.id === id);
      return e && "disabled" in e ? Boolean(e.disabled) : false;
    };
    for (const id of ["wt-diff", "wt-files", "wt-overlap", "wt-discard"]) expect(off(id), id).toBe(false);
    for (const id of [
      "wt-merge",
      "wt-pr",
      "wt-preview",
      "wt-compare-previews",
      "wt-checks",
      "wt-land",
      "wt-second-opinion",
      "wt-setup",
    ])
      expect(off(id), id).toBe(true);
    // A git worktree's menu is as before.
    const plain = worktreeMenu({ ...JJ_LANE, laneVcs: undefined }, "main", () => 0);
    expect(plain.some((e) => e.id === "wt-jj-note")).toBe(false);
  });
});
