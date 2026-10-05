import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { axe } from "vitest-axe";
import type { Tab, WorkspaceSummary } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { setInvokeBackend } from "../../ipc/invoke";
import { useLayout } from "../../stores/layout";
import { useSettings } from "../../stores/settings";
import { reordered, useWorkspaces } from "../../stores/workspaces";
import { fakeBackend, resetStores, type Call } from "../../test/fakeBackend";
import { CodeRail } from "../code/CodeRail";
import { placement } from "./RailList";

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
    sortKey: "a0",
    pinned: false,
    collapsed: true,
    sessionsCollapsed: false,
    createdAt: 0,
    missing: false,
    branch: null,
    liveCount: 0,
    tabs: [tab],
    ...over,
  };
}

let calls: Call[] = [];
let list: WorkspaceSummary[] = [];

function setup(initial: WorkspaceSummary[]) {
  list = initial;
  calls = fakeBackend({
    workspace_list: () => list,
    rail_move: (a) => {
      list = reordered(list, String(a?.itemId), (a?.afterId as string | null) ?? null, Boolean(a?.pinned));
      return null;
    },
    workspace_rename: (a) => {
      list = list.map((w) => (w.id === a?.id ? { ...w, name: String(a?.name) } : w));
      return null;
    },
    workspace_remove: (a) => {
      list = list.filter((w) => w.id !== a?.id);
      return null;
    },
    layout_get: () => [],
  });
  useWorkspaces.setState({ list: initial, activeId: initial[0]?.id ?? null, activeTab: {} });
  useLayout.setState({ tabs: {}, loaded: {}, boxes: {}, dragging: false, picker: null });
  return render(
    <UiProvider>
      <CodeRail />
    </UiProvider>,
  );
}

const rows = () => screen.getAllByTestId("workspace-row");
const names = () => rows().map((r) => within(r).getByTestId("rail-row-name").textContent);
const named = (cmd: string) => calls.filter((c) => c.cmd === cmd);

beforeEach(resetStores);
afterEach(() => setInvokeBackend(null));

describe("rail (F-SH-03)", () => {
  it("F-SH-03.1 opening a workspace that isn't first leaves the order alone", () => {
    setup([ws("a"), ws("b"), ws("c")]);
    fireEvent.click(rows()[2] as HTMLElement);
    expect(useWorkspaces.getState().activeId).toBe("c");
    expect(names()).toEqual(["a", "b", "c"]);
    expect(named("rail_move")).toHaveLength(0);
  });

  it("F-SH-03.2 a row dragged above the band is pinned; below it, unpinned", () => {
    const order = ["p1", "__band__", "a", "b"];
    // b dragged to the top: pinned, first.
    expect(placement(["b", "p1", "__band__", "a"], "b")).toEqual({ pinned: true, afterId: null });
    // a dropped right under p1, still above the band: pinned after p1.
    expect(placement(["p1", "a", "__band__", "b"], "a")).toEqual({ pinned: true, afterId: "p1" });
    // p1 dragged below the band: unpinned, first in All.
    expect(placement(["__band__", "p1", "a", "b"], "p1")).toEqual({ pinned: false, afterId: null });
    expect(placement(order, "b")).toEqual({ pinned: false, afterId: "a" });
  });

  it("F-SH-03.2 Pin from the context menu moves the row into Pinned and saves it", async () => {
    setup([ws("a"), ws("b"), ws("c")]);
    fireEvent.contextMenu(rows()[2] as HTMLElement);
    fireEvent.click(await screen.findByRole("menuitem", { name: "Pin" }));
    await waitFor(() => expect(names()).toEqual(["c", "a", "b"]));
    expect(named("rail_move")[0]?.args).toEqual({ mode: "code", itemId: "c", afterId: null, pinned: true });
    expect(screen.getByRole("button", { name: "Pinned" })).toBeTruthy();
    expect(rows()[0]?.getAttribute("data-pinned")).toBe("true");
  });

  it("F-SH-03 Move down puts the row after its next neighbour", async () => {
    setup([ws("a"), ws("b"), ws("c")]);
    fireEvent.contextMenu(rows()[0] as HTMLElement);
    fireEvent.click(await screen.findByRole("menuitem", { name: "Move down" }));
    await waitFor(() => expect(names()).toEqual(["b", "a", "c"]));
    expect(named("rail_move")[0]?.args).toMatchObject({ itemId: "a", afterId: "b", pinned: false });
  });

  it("F-SH-03.3 a missing folder shows the warning style, and Remove only removes the row", async () => {
    setup([ws("a"), ws("gone", { missing: true })]);
    const row = rows()[1] as HTMLElement;
    expect(row.className).toContain("text-warning");
    expect(within(row).getByLabelText("Folder not found")).toBeTruthy();
    fireEvent.keyDown(row, { key: "Delete" });
    fireEvent.click(await screen.findByRole("button", { name: "Remove" }));
    await waitFor(() => expect(names()).toEqual(["a"]));
    const touched = calls
      .map((c) => c.cmd)
      .filter((c) => c.startsWith("workspace_") && c !== "workspace_list");
    expect(touched).toEqual(["workspace_remove"]);
  });

  it("F-SH-03 F2 renames inline; Escape cancels", async () => {
    setup([ws("a"), ws("b")]);
    fireEvent.keyDown(rows()[1] as HTMLElement, { key: "F2" });
    const field = screen.getByLabelText("New name for b");
    fireEvent.change(field, { target: { value: "Billing" } });
    fireEvent.keyDown(field, { key: "Enter" });
    await waitFor(() => expect(names()).toEqual(["a", "Billing"]));
    expect(named("workspace_rename")[0]?.args).toEqual({ id: "b", name: "Billing" });
    fireEvent.keyDown(rows()[0] as HTMLElement, { key: "F2" });
    fireEvent.keyDown(screen.getByLabelText("New name for a"), { key: "Escape" });
    expect(named("workspace_rename")).toHaveLength(1);
  });

  it("F-SH-03 arrow keys move between rows and Enter opens", () => {
    setup([ws("a"), ws("b"), ws("c")]);
    const first = rows()[0] as HTMLElement;
    first.focus();
    fireEvent.keyDown(first, { key: "ArrowDown" });
    expect(document.activeElement).toBe(rows()[1]);
    fireEvent.keyDown(rows()[1] as HTMLElement, { key: "End" });
    expect(document.activeElement).toBe(rows()[2]);
    fireEvent.keyDown(rows()[2] as HTMLElement, { key: "Enter" });
    expect(useWorkspaces.getState().activeId).toBe("c");
  });

  it("F-SH-03 the rail resizes between 180 and 360 px and the width persists", async () => {
    setup([ws("a")]);
    const nav = screen.getByRole("navigation", { name: "Workspaces" });
    const edge = screen.getByRole("separator", { name: "Resize the rail" });
    expect(nav.style.width).toBe("248px");
    fireEvent.keyDown(edge, { key: "ArrowRight" });
    await waitFor(() =>
      expect(named("settings_set").at(-1)?.args).toEqual({ key: "rail_width", value: 256 }),
    );
    act(() => useSettings.setState({ values: { rail_width: 356 } }));
    fireEvent.keyDown(edge, { key: "ArrowRight" });
    await waitFor(() =>
      expect(named("settings_set").at(-1)?.args).toEqual({ key: "rail_width", value: 360 }),
    );
    expect(nav.style.width).toBe("360px");
  });

  it("F-SH-03 expanding a workspace lists its open panes", async () => {
    setup([ws("a")]);
    act(() =>
      useLayout.setState({
        loaded: { a: true },
        tabs: {
          tb_a: {
            tree: { type: "leaf", paneId: "pn_1" },
            active: "pn_1",
            maximized: null,
            keepEmpty: false,
            panes: {
              pn_1: {
                id: "pn_1",
                tabId: "tb_a",
                workspaceId: "a",
                kind: "terminal",
                cwd: null,
                engineId: null,
                humanName: null,
                title: null,
                createdAt: 1,
              },
            },
          },
        },
      }),
    );
    expect(screen.queryByTestId("rail-session")).toBeNull();
    fireEvent.keyDown(rows()[0] as HTMLElement, { key: "ArrowRight" });
    expect((await screen.findByTestId("rail-session")).textContent).toContain("Shell · 1");
    expect(named("workspace_set_flags")[0]?.args).toEqual({ id: "a", collapsed: false });
  });

  it("F-SH-03 the rail has no serious axe violations", async () => {
    const { container } = setup([
      ws("a", { pinned: true, collapsed: false }),
      ws("b", { branch: "main", liveCount: 2, collapsed: false }),
      ws("c", { missing: true }),
    ]);
    // Expanded rows list their panes (a level-2 row) or "No open panes".
    act(() =>
      useLayout.setState({
        loaded: { a: true, b: true },
        tabs: {
          tb_b: {
            tree: { type: "leaf", paneId: "pn_1" },
            active: "pn_1",
            maximized: null,
            keepEmpty: false,
            panes: {
              pn_1: {
                id: "pn_1",
                tabId: "tb_b",
                workspaceId: "b",
                kind: "terminal",
                cwd: null,
                engineId: null,
                humanName: null,
                title: null,
                createdAt: 1,
              },
            },
          },
        },
      }),
    );
    expect(screen.getByTestId("rail-session")).toBeTruthy();
    expect(screen.getByText("No open panes")).toBeTruthy();
    const results = await axe(container, { rules: { region: { enabled: false } } });
    expect(results.violations.filter((v) => v.impact === "serious" || v.impact === "critical")).toEqual([]);
  });
});

describe("tab strip (F-CD-01)", () => {
  it("F-CD-01.2 tabs reorder through tab_reorder and the order sticks", async () => {
    const w = ws("a", {
      tabs: ["t1", "t2", "t3"].map((id, i) => ({
        id,
        workspaceId: "a",
        name: `Tab ${i + 1}`,
        sortKey: `a${i}`,
        createdAt: i,
      })),
    });
    setup([w]);
    let reorderedIds: string[] = [];
    fakeBackend({
      workspace_list: () => [
        { ...w, tabs: reorderedIds.map((id) => w.tabs.find((x) => x.id === id) as Tab) },
      ],
      tab_reorder: (a) => {
        reorderedIds = a?.ids as string[];
        return null;
      },
    });
    await act(() => useWorkspaces.getState().reorderTabs("a", ["t3", "t1", "t2"]));
    expect(useWorkspaces.getState().list[0]?.tabs.map((x) => x.id)).toEqual(["t3", "t1", "t2"]);
  });
});
