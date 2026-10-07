import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { DashboardItem, IpcError, MergeReport, Tab, WorkspaceSummary } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { setInvokeBackend } from "../../ipc/invoke";
import { useLayout } from "../../stores/layout";
import { useWorkspaces } from "../../stores/workspaces";
import { topLevel, useWorktrees, worktreesOf } from "../../stores/worktrees";
import { fakeBackend, resetStores, type Call } from "../../test/fakeBackend";
import { group, type Lanes } from "../dashboard/model";
import { CodeRail } from "./CodeRail";
import { WorktreeDialogs } from "./Worktrees";

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

const lane = (id: string, parentId: string, slug: string) =>
  ws(id, {
    parentId,
    laneBranch: `tandem/${slug}`,
    laneBase: "abc123",
    root: `/code/${parentId}/.tandem/worktrees/${slug}`,
    name: slug,
  });

const REPO = ws("shop", { branch: "main" });
const PLAIN = ws("notes");
const FIX = lane("wt_1", "shop", "fix-auth");
const FIX2 = lane("wt_2", "shop", "fix-auth-2");

let calls: Call[] = [];
const named = (cmd: string) => calls.filter((c) => c.cmd === cmd);

function backend(handlers: Parameters<typeof fakeBackend>[0] = {}) {
  calls = fakeBackend({
    workspace_list: () => [REPO, FIX, FIX2, PLAIN],
    layout_get: () => [],
    sessions_saved_list: () => [],
    ...handlers,
  });
}

function dialogs() {
  return render(
    <UiProvider>
      <WorktreeDialogs />
    </UiProvider>,
  );
}

beforeEach(() => {
  resetStores();
  useWorktrees.setState({ ask: null });
  useLayout.setState({ tabs: {}, loaded: {}, boxes: {}, dragging: false, picker: null });
  useWorkspaces.setState({ list: [REPO, FIX, FIX2, PLAIN], activeId: "shop", activeTab: {} });
});
afterEach(() => setInvokeBackend(null));

describe("worktrees (F-CD-15)", () => {
  it("F-CD-15.4 worktrees nest under their repository in the rail, not beside it", () => {
    backend();
    render(
      <UiProvider>
        <CodeRail />
      </UiProvider>,
    );
    const tops = screen.getAllByTestId("workspace-row").map((r) => within(r).getByTestId("rail-row-name").textContent);
    expect(tops).toEqual(["shop", "notes"]);
    const nested = screen.getAllByTestId("worktree-row");
    expect(nested.map((r) => r.getAttribute("data-branch"))).toEqual(["tandem/fix-auth", "tandem/fix-auth-2"]);
    for (const row of nested) {
      const groupEl = row.closest('[role="group"]');
      expect(groupEl).not.toBeNull();
      // The group belongs to the repository's row, the element just before it.
      expect(within(groupEl?.previousElementSibling as HTMLElement).getByTestId("rail-row-name").textContent).toBe("shop");
    }
    expect(topLevel([REPO, FIX, PLAIN]).map((w) => w.id)).toEqual(["shop", "notes"]);
    expect(worktreesOf([REPO, FIX, FIX2, PLAIN], "notes")).toEqual([]);
  });

  it("F-CD-15.1 New worktree sends the task text, and the new worktree opens under its repository", async () => {
    let created = false;
    backend({
      lane_create: () => {
        created = true;
        return FIX;
      },
      workspace_list: () => (created ? [REPO, FIX, PLAIN] : [REPO, PLAIN]),
    });
    useWorkspaces.setState({ list: [{ ...REPO, collapsed: true }, PLAIN], activeId: "shop" });
    useWorktrees.getState().open({ kind: "new", workspaceId: "shop" });
    dialogs();
    fireEvent.change(screen.getByTestId("worktree-name"), { target: { value: "fix auth" } });
    fireEvent.click(screen.getByTestId("worktree-create"));
    await waitFor(() => expect(useWorktrees.getState().ask).toBeNull());
    expect(named("lane_create")[0]?.args).toEqual({ workspaceId: "shop", slugSource: "fix auth" });
    expect(useWorkspaces.getState().activeId).toBe("wt_1");
    expect(screen.getByText("Created tandem/fix-auth")).toBeTruthy();
  });

  it("F-CD-15.2 discarding asks for force only after a plain removal refuses, behind a typed confirmation", async () => {
    const forced: boolean[] = [];
    backend({
      lane_remove: (a) => {
        forced.push(Boolean(a?.force));
        if (!a?.force) {
          const err: IpcError = { code: "conflict", message: "Uncommitted changes", stage: null, hint: null, retryable: false };
          throw err;
        }
        return null;
      },
    });
    useWorktrees.getState().open({ kind: "discard", worktreeId: "wt_1" });
    dialogs();
    const confirm = screen.getByRole("button", { name: "Discard worktree" }) as HTMLButtonElement;
    expect(confirm.disabled).toBe(true);
    fireEvent.change(screen.getByLabelText("Type tandem/fix-auth to confirm"), { target: { value: "tandem/fix-auth" } });
    expect(confirm.disabled).toBe(false);
    fireEvent.click(confirm);
    await waitFor(() => expect(forced).toEqual([false, true]));
    expect(await screen.findByText("Discarded tandem/fix-auth")).toBeTruthy();
  });

  it("F-CD-15.2 cancelling the native confirm keeps the worktree, with no error", async () => {
    backend({
      lane_remove: (a) => {
        const err: IpcError = a?.force
          ? { code: "cancelled", message: "Kept the worktree", stage: null, hint: null, retryable: false }
          : { code: "conflict", message: "Uncommitted changes", stage: null, hint: null, retryable: false };
        throw err;
      },
    });
    useWorktrees.getState().open({ kind: "discard", worktreeId: "wt_1" });
    dialogs();
    fireEvent.change(screen.getByLabelText("Type tandem/fix-auth to confirm"), { target: { value: "tandem/fix-auth" } });
    fireEvent.click(screen.getByRole("button", { name: "Discard worktree" }));
    await waitFor(() => expect(named("lane_remove")).toHaveLength(2));
    expect(screen.queryByText("Kept the worktree")).toBeNull();
    expect(screen.queryByText(/Discarded/)).toBeNull();
  });

  it("F-CD-15.3 a merge that conflicts lists the files and says nothing was committed", async () => {
    const report: MergeReport = { merged: false, conflicts: ["src/auth.rs", "README.md"], commit: null };
    backend({ lane_merge: () => report });
    useWorktrees.getState().open({ kind: "merge", worktreeId: "wt_1" });
    dialogs();
    expect(screen.getByText("Merge tandem/fix-auth into main?")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Merge" }));
    const list = await screen.findByTestId("merge-conflicts");
    expect(within(list).getAllByRole("listitem").map((li) => li.textContent)).toEqual(["Csrc/auth.rs", "CREADME.md"]);
    expect(screen.getByText("Merge stopped: 2 files conflict")).toBeTruthy();
    expect(screen.getByText(/Nothing was committed/)).toBeTruthy();
  });

  it("F-CD-15.4 the Dashboard's project grouping nests worktrees under their repository", () => {
    const item = (key: string, workspaceId: string | null, workspace: string): DashboardItem =>
      ({ key, workspaceId, workspace, status: "working", since: 1 }) as unknown as DashboardItem;
    const lanes: Lanes = {
      wt_1: { parentId: "shop", parentName: "shop", branch: "tandem/fix-auth" },
      wt_2: { parentId: "shop", parentName: "shop", branch: "tandem/fix-auth-2" },
    };
    const groups = group(
      [item("p1", "shop", "shop"), item("p2", "wt_2", "fix-auth-2"), item("p3", "wt_1", "fix-auth"), item("p4", "notes", "notes")],
      "project",
      ["shop", "wt_1", "wt_2", "notes"],
      "No folder",
      lanes,
    );
    expect(groups.map((g) => g.label)).toEqual(["shop", "notes"]);
    const shop = groups[0];
    expect(shop?.items.map((i) => i.key)).toEqual(["p1"]);
    expect(shop?.children?.map((c) => [c.label, c.items.map((i) => i.key)])).toEqual([
      ["tandem/fix-auth", ["p3"]],
      ["tandem/fix-auth-2", ["p2"]],
    ]);
    // A worktree whose repository has no agents of its own still groups under it.
    const only = group([item("p3", "wt_1", "fix-auth")], "project", ["shop", "wt_1"], "No folder", lanes);
    expect(only.map((g) => [g.label, g.items.length, g.children?.length])).toEqual([["shop", 0, 1]]);
  });
});
