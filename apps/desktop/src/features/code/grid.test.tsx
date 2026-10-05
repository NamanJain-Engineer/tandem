import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { PaneRecord, Tab, TabLayout, WorkspaceSummary } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { setInvokeBackend } from "../../ipc/invoke";
import { useLayout } from "../../stores/layout";
import { useSession } from "../../stores/session";
import { useWorkspaces } from "../../stores/workspaces";
import { FREE_VIEW, fakeBackend, resetStores, type Call } from "../../test/fakeBackend";
import { applyDrop } from "./dnd";
import { leaves } from "./layout/tree";

// xterm needs a real browser; the grid's own behaviour is what's under test.
const mounts = new Map<string, number>();
const lastProps = new Map<string, { visible: boolean; focused: boolean }>();
vi.mock("./TerminalView", async () => {
  const React = await import("react");
  return {
    terminalHandles: new Map(),
    TerminalView: ({ paneId, visible, focused }: { paneId: string; visible: boolean; focused: boolean }) => {
      React.useEffect(() => {
        mounts.set(paneId, (mounts.get(paneId) ?? 0) + 1);
      }, [paneId]);
      lastProps.set(paneId, { visible, focused });
      return React.createElement("div", { "data-testid": "terminal", "data-pane-id": paneId });
    },
  };
});

const { CodeMain } = await import("./CodeMain");

const TAB: Tab = { id: "tb_1", workspaceId: "ws_1", name: "Tab 1", sortKey: "a0", createdAt: 0 };
const WS: WorkspaceSummary = {
  id: "ws_1",
  kind: "local",
  hostId: "local",
  root: "/code/shop",
  wslDistro: null,
  name: "shop",
  parentId: null,
  laneBranch: null,
  sortKey: "a0",
  pinned: false,
  collapsed: false,
  sessionsCollapsed: false,
  createdAt: 0,
  missing: false,
  branch: "main",
  liveCount: 0,
  tabs: [TAB],
};

/** A small core: tabs, panes and saved layouts. */
function core(
  opts: { live?: number; panes?: string[]; tree?: unknown; missing?: boolean; picked?: string } = {},
) {
  let n = 0;
  const tabs: Tab[] = [TAB];
  const panes: PaneRecord[] = (opts.panes ?? []).map((id, i) => pane(id, TAB.id, i));
  let saved: { tree: unknown; activePaneId: string | null; maximizedPaneId: string | null } | null = opts.tree
    ? { tree: opts.tree, activePaneId: null, maximizedPaneId: null }
    : null;
  function pane(id: string, tabId: string, at: number): PaneRecord {
    return {
      id,
      tabId,
      workspaceId: WS.id,
      kind: "terminal",
      cwd: null,
      engineId: null,
      humanName: null,
      title: null,
      createdAt: at,
    };
  }
  const calls: Call[] = fakeBackend({
    workspace_list: () => [
      { ...WS, tabs, liveCount: opts.live ?? panes.length, missing: opts.missing ?? false },
    ],
    tab_rename: (a) => {
      const tab = tabs.find((x) => x.id === a?.id);
      if (tab) tab.name = String(a?.name);
      return null;
    },
    workspace_pick_folder: () => opts.picked ?? null,
    engines_list: () => [
      {
        spec: {
          id: "claude",
          label: "Claude Code",
          vendor: "Anthropic",
          commands: ["claude"],
          providerHomeEnv: null,
          supportsResume: true,
          newlineSequence: "\u001b\r",
        },
        install: { path: "/bin/claude" },
        chatCapable: false,
        unsupportedReason: null,
      },
      {
        spec: {
          id: "kimi",
          label: "Kimi",
          vendor: "Moonshot",
          commands: ["kimi"],
          providerHomeEnv: null,
          supportsResume: false,
          newlineSequence: "\u001b\r",
        },
        install: null,
        chatCapable: false,
        unsupportedReason: null,
      },
    ],
    workspace_locate: () => {
      opts.missing = false;
      return null;
    },
    layout_get: () =>
      tabs.map((tab): TabLayout => ({
        tab,
        tree: tab.id === TAB.id ? ((saved?.tree as TabLayout["tree"]) ?? null) : null,
        activePaneId: null,
        maximizedPaneId: null,
      })),
    layout_save: (a) => {
      saved = a as typeof saved;
      return null;
    },
    pane_list: (a) => panes.filter((p) => p.tabId === a?.tabId),
    pane_create: (a) => {
      if (opts.live !== undefined && opts.live >= 4)
        throw { code: "limit_reached", message: "Free includes 4 live panes" };
      const p = {
        ...pane(`pn_${++n}`, String(a?.tabId), 100 + n),
        kind: String(a?.kind),
        engineId: (a?.engineId as string | undefined) ?? null,
      };
      panes.push(p);
      return p;
    },
    pane_close: (a) => {
      const i = panes.findIndex((p) => p.id === a?.paneId);
      if (i >= 0) panes.splice(i, 1);
      return null;
    },
    tab_create: () => {
      const tab: Tab = { ...TAB, id: `tb_${tabs.length + 1}`, name: `Tab ${tabs.length + 1}` };
      tabs.push(tab);
      return tab;
    },
    pane_move: (a) => {
      const p = panes.find((x) => x.id === a?.paneId);
      if (p) p.tabId = String(a?.tabId);
      return p;
    },
  });
  return { calls, panes, saved: () => saved };
}

function renderCode() {
  useWorkspaces.setState({ list: [WS], activeId: WS.id, activeTab: { [WS.id]: TAB.id } });
  return render(
    <UiProvider>
      <CodeMain visible />
    </UiProvider>,
  );
}

const tree = () => useLayout.getState().tabs[TAB.id]?.tree ?? null;
const paneEls = () => screen.queryAllByTestId("pane");

beforeEach(() => {
  resetStores();
  useLayout.setState({ tabs: {}, loaded: {}, boxes: {}, dragging: false, picker: null });
  mounts.clear();
  lastProps.clear();
});
afterEach(() => setInvokeBackend(null));

describe("pane grid (F-CD-02)", () => {
  it("F-CD-02 a fresh tab starts one shell, alone without a maximize control", async () => {
    const { calls } = core();
    renderCode();
    await waitFor(() => expect(paneEls()).toHaveLength(1));
    expect(calls.filter((c) => c.cmd === "pane_create")).toHaveLength(1);
    expect(screen.queryByRole("button", { name: "Maximize pane" })).toBeNull();
    expect(within(paneEls()[0] as HTMLElement).getByTestId("pane-title").textContent).toBe("Shell · 1");
  });

  it("F-CD-02 split right opens the picker; Shell adds a pane to the right of the focused one", async () => {
    core({ panes: ["pn_a"], tree: { type: "leaf", paneId: "pn_a" } });
    renderCode();
    await waitFor(() => expect(paneEls()).toHaveLength(1));
    fireEvent.keyDown(screen.getByRole("button", { name: "Actions for Shell · 1" }), { key: "Enter" });
    fireEvent.click(await screen.findByRole("menuitem", { name: /Split right/ }));
    const picker = await screen.findByTestId("pane-picker");
    fireEvent.click(within(picker).getByTestId("tile-shell"));
    await waitFor(() => expect(paneEls()).toHaveLength(2));
    expect(tree()).toMatchObject({ type: "split", dir: "row", a: { paneId: "pn_a" }, b: { type: "leaf" } });
    const created = leaves(tree())[1] as string;
    expect(useLayout.getState().tabs[TAB.id]?.active).toBe(created);
    expect(screen.getAllByRole("button", { name: "Maximize pane" })).toHaveLength(2);
  });

  it("F-CD-03 at the Free pane limit the picker shows the upgrade card and creates nothing", async () => {
    const { calls } = core({ live: 4, panes: ["pn_a"], tree: { type: "leaf", paneId: "pn_a" } });
    useSession.setState({ entitlement: FREE_VIEW });
    renderCode();
    await waitFor(() => expect(paneEls()).toHaveLength(1));
    act(() => void useLayout.getState().requestSplit(TAB.id, "pn_a", "right"));
    expect(await screen.findByTestId("pane-limit")).toBeTruthy();
    expect(screen.getByText("Free includes 4 live panes")).toBeTruthy();
    expect(screen.getByRole("button", { name: "See plans" })).toBeTruthy();
    expect(screen.queryByTestId("tile-shell")).toBeNull();
    expect(calls.filter((c) => c.cmd === "pane_create")).toHaveLength(0);
  });

  it("F-CD-02 maximize hides siblings without unmounting them, and restores", async () => {
    core({
      panes: ["pn_a", "pn_b"],
      tree: {
        type: "split",
        dir: "row",
        ratio: 0.5,
        a: { type: "leaf", paneId: "pn_a" },
        b: { type: "leaf", paneId: "pn_b" },
      },
    });
    renderCode();
    await waitFor(() => expect(paneEls()).toHaveLength(2));
    const firstMaximize = screen.getAllByRole("button", { name: "Maximize pane" })[0] as HTMLElement;
    fireEvent.click(firstMaximize);
    await waitFor(() => expect(lastProps.get("pn_b")?.visible).toBe(false));
    expect(lastProps.get("pn_a")?.visible).toBe(true);
    expect(document.querySelector('[data-slot="pn_b"]')).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Restore pane" }));
    await waitFor(() => expect(lastProps.get("pn_b")?.visible).toBe(true));
    expect(mounts.get("pn_a")).toBe(1);
    expect(mounts.get("pn_b")).toBe(1);
  });

  it("F-CD-02 swapping two panes moves them without remounting their terminals", async () => {
    core({
      panes: ["pn_a", "pn_b"],
      tree: {
        type: "split",
        dir: "row",
        ratio: 0.5,
        a: { type: "leaf", paneId: "pn_a" },
        b: { type: "leaf", paneId: "pn_b" },
      },
    });
    renderCode();
    await waitFor(() => expect(paneEls()).toHaveLength(2));
    const order = () =>
      Array.from(document.querySelectorAll("[data-slot] [data-testid=pane]")).map((el) =>
        el.getAttribute("data-pane-id"),
      );
    expect(order()).toEqual(["pn_a", "pn_b"]);
    await act(() =>
      applyDrop(
        { kind: "pane", paneId: "pn_a", tabId: TAB.id, title: "" },
        { kind: "zone", paneId: "pn_b", tabId: TAB.id, zone: "center" },
      ),
    );
    expect(order()).toEqual(["pn_b", "pn_a"]);
    expect(mounts.get("pn_a")).toBe(1);
    expect(mounts.get("pn_b")).toBe(1);
  });

  it("F-CD-02 an edge drop splits next to the target; dropping on itself changes nothing", async () => {
    core({
      panes: ["pn_a", "pn_b"],
      tree: {
        type: "split",
        dir: "row",
        ratio: 0.5,
        a: { type: "leaf", paneId: "pn_a" },
        b: { type: "leaf", paneId: "pn_b" },
      },
    });
    renderCode();
    await waitFor(() => expect(paneEls()).toHaveLength(2));
    const before = tree();
    await act(() =>
      applyDrop(
        { kind: "pane", paneId: "pn_a", tabId: TAB.id, title: "" },
        { kind: "zone", paneId: "pn_a", tabId: TAB.id, zone: "left" },
      ),
    );
    expect(tree()).toBe(before);
    await act(() =>
      applyDrop(
        { kind: "pane", paneId: "pn_a", tabId: TAB.id, title: "" },
        { kind: "zone", paneId: "pn_b", tabId: TAB.id, zone: "bottom" },
      ),
    );
    expect(tree()).toMatchObject({ type: "split", dir: "col", a: { paneId: "pn_b" }, b: { paneId: "pn_a" } });
  });

  it("F-CD-02 dropping on the tab strip moves the pane to a new tab with the same id", async () => {
    const { panes } = core({
      panes: ["pn_a", "pn_b"],
      tree: {
        type: "split",
        dir: "row",
        ratio: 0.5,
        a: { type: "leaf", paneId: "pn_a" },
        b: { type: "leaf", paneId: "pn_b" },
      },
    });
    renderCode();
    await waitFor(() => expect(paneEls()).toHaveLength(2));
    await act(() =>
      applyDrop(
        { kind: "pane", paneId: "pn_b", tabId: TAB.id, title: "" },
        { kind: "new_tab", workspaceId: WS.id },
      ),
    );
    expect(leaves(tree())).toEqual(["pn_a"]);
    expect(useLayout.getState().tabs.tb_2?.tree).toEqual({ type: "leaf", paneId: "pn_b" });
    expect(panes.find((p) => p.id === "pn_b")?.tabId).toBe("tb_2");
    await waitFor(() => expect(useWorkspaces.getState().activeTab[WS.id]).toBe("tb_2"));
  });

  it("F-CD-02 closing the last pane leaves the empty state with launch tiles", async () => {
    const { calls } = core({ panes: ["pn_a"], tree: { type: "leaf", paneId: "pn_a" } });
    renderCode();
    await waitFor(() => expect(paneEls()).toHaveLength(1));
    fireEvent.click(screen.getByRole("button", { name: "Close Shell · 1" }));
    expect(await screen.findByTestId("empty-tab")).toBeTruthy();
    expect(screen.getByText("Start something")).toBeTruthy();
    expect(calls.filter((c) => c.cmd === "pane_create")).toHaveLength(0);
    fireEvent.click(screen.getByTestId("tile-shell"));
    await waitFor(() => expect(paneEls()).toHaveLength(1));
  });

  it("F-CD-02 layout changes are saved once, 500 ms after the last change", async () => {
    const { calls, saved } = core({
      panes: ["pn_a", "pn_b"],
      tree: {
        type: "split",
        dir: "row",
        ratio: 0.5,
        a: { type: "leaf", paneId: "pn_a" },
        b: { type: "leaf", paneId: "pn_b" },
      },
    });
    renderCode();
    await waitFor(() => expect(paneEls()).toHaveLength(2));
    vi.useFakeTimers();
    try {
      act(() => {
        useLayout.getState().setRatio(TAB.id, "", 0.3);
        useLayout.getState().setRatio(TAB.id, "", 0.4);
        useLayout.getState().swap(TAB.id, "pn_a", "pn_b");
      });
      await vi.advanceTimersByTimeAsync(499);
      expect(calls.filter((c) => c.cmd === "layout_save")).toHaveLength(0);
      await vi.advanceTimersByTimeAsync(1);
      expect(calls.filter((c) => c.cmd === "layout_save")).toHaveLength(1);
    } finally {
      vi.useRealTimers();
    }
    expect(saved()?.tree).toMatchObject({ ratio: 0.4, a: { paneId: "pn_b" }, b: { paneId: "pn_a" } });
  });

  it("F-CD-02 Mod+Alt+Arrow logic moves focus to the spatial neighbour", async () => {
    core({
      panes: ["pn_a", "pn_b"],
      tree: {
        type: "split",
        dir: "row",
        ratio: 0.5,
        a: { type: "leaf", paneId: "pn_a" },
        b: { type: "leaf", paneId: "pn_b" },
      },
    });
    renderCode();
    await waitFor(() => expect(paneEls()).toHaveLength(2));
    act(() => useLayout.getState().focus(TAB.id, "pn_a"));
    act(() => useLayout.getState().focusDirection(TAB.id, "right"));
    expect(useLayout.getState().tabs[TAB.id]?.active).toBe("pn_b");
    expect(lastProps.get("pn_b")?.focused).toBe(true);
    act(() => useLayout.getState().focusDirection(TAB.id, "right"));
    expect(useLayout.getState().tabs[TAB.id]?.active).toBe("pn_b");
    act(() => useLayout.getState().focusCycle(TAB.id, 1));
    expect(useLayout.getState().tabs[TAB.id]?.active).toBe("pn_a");
  });

  it("F-CD-01 a tab renames with F2 or a double-click; Escape keeps the old name", async () => {
    const { calls } = core({ panes: ["pn_a"], tree: { type: "leaf", paneId: "pn_a" } });
    renderCode();
    await waitFor(() => expect(paneEls()).toHaveLength(1));
    const tabButton = () => within(screen.getByTestId("tab")).getAllByRole("button")[0] as HTMLElement;
    fireEvent.keyDown(tabButton(), { key: "F2" });
    fireEvent.keyDown(screen.getByLabelText("New name for Tab 1"), { key: "Escape" });
    expect(calls.filter((c) => c.cmd === "tab_rename")).toHaveLength(0);
    fireEvent.doubleClick(tabButton());
    const field = screen.getByLabelText("New name for Tab 1");
    fireEvent.change(field, { target: { value: "Server" } });
    fireEvent.keyDown(field, { key: "Enter" });
    await waitFor(() => expect(tabButton().textContent).toBe("Server"));
    expect(calls.filter((c) => c.cmd === "tab_rename").map((c) => c.args)).toEqual([
      { id: TAB.id, name: "Server" },
    ]);
  });

  it("F-CD-01 a missing folder blocks launches and offers Locate… and Remove", async () => {
    const { calls } = core({ missing: true, picked: "/code/shop-moved" });
    useWorkspaces.setState({
      list: [{ ...WS, missing: true }],
      activeId: WS.id,
      activeTab: { [WS.id]: TAB.id },
    });
    render(
      <UiProvider>
        <CodeMain visible />
      </UiProvider>,
    );
    expect(await screen.findByText("This folder is missing")).toBeTruthy();
    expect(calls.filter((c) => c.cmd === "pane_create")).toHaveLength(0);
    fireEvent.click(screen.getByRole("button", { name: "Remove" }));
    expect(useWorkspaces.getState().confirmRemove?.id).toBe(WS.id);
    act(() => useWorkspaces.getState().askRemove(null));
    fireEvent.click(screen.getByRole("button", { name: "Locate…" }));
    await waitFor(() =>
      expect(calls.find((c) => c.cmd === "workspace_locate")?.args).toEqual({
        id: WS.id,
        newRoot: "/code/shop-moved",
      }),
    );
    await waitFor(() => expect(screen.queryByText("This folder is missing")).toBeNull());
  });

  it("F-CD-02 the picker offers installed engines; an agent pane is titled by its engine", async () => {
    const { calls } = core({ panes: ["pn_a"], tree: { type: "leaf", paneId: "pn_a" } });
    renderCode();
    await waitFor(() => expect(paneEls()).toHaveLength(1));
    act(() => void useLayout.getState().requestSplit(TAB.id, "pn_a", "right"));
    const picker = await screen.findByTestId("pane-picker");
    await waitFor(() => expect(within(picker).getByTestId("tile-claude")).toBeTruthy());
    expect(within(picker).queryByTestId("tile-kimi")).toBeNull();
    fireEvent.click(within(picker).getByTestId("tile-claude"));
    await waitFor(() => expect(paneEls()).toHaveLength(2));
    expect(calls.find((c) => c.cmd === "pane_create" && c.args?.kind === "agent")?.args).toMatchObject({
      engineId: "claude",
    });
    const titles = screen.getAllByTestId("pane-title").map((el) => el.textContent);
    expect(titles).toEqual(["Shell · 1", "Claude Code · 1"]);
  });
});
