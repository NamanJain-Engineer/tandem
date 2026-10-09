import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { BrowserState, PaneRecord, SidebarState, Tab, TabLayout, WorkspaceSummary } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { emitLocal } from "../../ipc/events";
import { setInvokeBackend } from "../../ipc/invoke";
import { followContexts } from "../../stores/contexts";
import { followFocus } from "../../stores/focus";
import { followBrowser } from "../../stores/browser";
import { flushLayouts, useLayout } from "../../stores/layout";
import { followBrowserTabs, useSidebar } from "../../stores/sidebar";
import { useRestore } from "../../stores/restore";
import { useStatus } from "../../stores/status";
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

vi.mock("./RestoredScrollback", async () => {
  const React = await import("react");
  return {
    RestoredScrollback: ({ paneId }: { paneId: string }) =>
      React.createElement("div", { "data-testid": "restored-scrollback", "data-pane-id": paneId }, "saved output"),
  };
});

const { CodeMain } = await import("./CodeMain");
const { Sidebar } = await import("../shell/Sidebar");
const { InstallHost } = await import("./InstallSheet");

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
  laneBase: null,
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

const REPORT = {
  host: "local",
  shellsAsked: ["/bin/zsh -lic"],
  foldersChecked: ["~/.local/bin", "~/.nvm/versions/node/v22.3.0/bin"],
  found: [],
  missing: ["claude", "kimi"],
  fixes: [],
  slowShell: null,
};

/** A browser page's state as the core reports it, each newer than the last. */
let pageSeq = 0;
function page(browserId: string, url: string, title = ""): BrowserState {
  return {
    seq: ++pageSeq,
    browserId,
    url,
    title,
    canBack: false,
    canForward: false,
    loading: false,
    device: "desktop",
    rotated: false,
    frame: { x: 0, y: 0, width: 400, height: 300 },
    zoom: 1,
    blocked: null,
    picking: false,
  };
}

/** A small core: tabs, panes and saved layouts. */
function core(
  opts: {
    live?: number;
    panes?: string[];
    tree?: unknown;
    missing?: boolean;
    picked?: string;
    noEngines?: boolean;
    /** Agent panes paused after a restart, with the reason Resume isn't offered. */
    paused?: Record<string, string | null>;
    /** Browser panes and the page each was on. */
    browsers?: Record<string, string | null>;
    /** The saved sidebar, for tests that show it. */
    sidebar?: SidebarState;
  } = {},
) {
  let n = 0;
  const tabs: Tab[] = [TAB];
  const panes: PaneRecord[] = (opts.panes ?? []).map((id, i) =>
    opts.paused && id in opts.paused
      ? { ...pane(id, TAB.id, i), kind: "agent", engineId: "claude", humanName: "Ada", restoreState: "paused" }
      : opts.browsers && id in opts.browsers
        ? { ...pane(id, TAB.id, i), kind: "browser", browserUrl: opts.browsers[id] ?? null }
        : pane(id, TAB.id, i),
  );
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
      accountId: null,
      humanName: null,
      title: null,
      restoreState: "none",
      browserUrl: null,
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
          logo: "C",
          commands: ["claude"],
          providerHomeEnv: null,
          supportsResume: true,
          newlineSequence: "\u001b\r",
        },
        install: opts.noEngines ? null : { path: "/bin/claude", version: "1.0.0", source: "shell" },
        chatCapable: false,
        unsupportedReason: null,
        extraArgs: [],
      },
      {
        spec: {
          id: "kimi",
          label: "Kimi",
          vendor: "Moonshot",
          logo: "K",
          commands: ["kimi"],
          providerHomeEnv: null,
          supportsResume: false,
          newlineSequence: "\u001b\r",
        },
        install: null,
        chatCapable: false,
        unsupportedReason: null,
        extraArgs: [],
      },
    ],
    engines_detect: () => REPORT,
    engines_report: () => (opts.noEngines ? REPORT : null),
    engines_install_command: () => ({ display: "npm install -g kimi-cli", shell: "/bin/zsh" }),
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
      const names = ["Ada", "Bea", "Cleo"];
      const p = {
        ...pane(`pn_${++n}`, String(a?.tabId), 100 + n),
        kind: String(a?.kind),
        engineId: (a?.engineId as string | undefined) ?? null,
        humanName: a?.kind === "agent" ? (names[panes.filter((x) => x.kind === "agent").length] ?? null) : null,
        browserUrl: a?.kind === "browser" ? ((a?.url as string | null | undefined) ?? null) : null,
      };
      panes.push(p);
      return p;
    },
    pane_rename: (a) => {
      const name = String(a?.humanName).trim();
      if (panes.some((x) => x.id !== a?.paneId && x.humanName === name))
        throw { code: "conflict", message: `Another pane is already called ${name}` };
      const p = panes.find((x) => x.id === a?.paneId);
      if (p) p.humanName = name;
      return name;
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
    restore_get_snapshot: () => ({
      panes: Object.entries(opts.paused ?? {}).map(([paneId, reason]) => ({
        paneId,
        workspaceId: WS.id,
        engineId: "claude",
        state: "paused",
        canResume: reason === null,
        reason,
        hasScrollback: true,
      })),
      interruptedThreads: 0,
      autoResume: false,
    }),
    restore_resume: () => ({ pid: 42, startedAt: 1 }),
    restore_start_fresh: () => ({ pid: 43, startedAt: 1 }),
    restore_cancel: () => null,
    browser_open: (a) => page(String(a?.browserId), (a?.url as string | null) ?? ""),
    browser_set_bounds: () => null,
    browser_close: () => null,
    browser_move: (a) => page(String(a?.to), "http://localhost:5173/", "Fixtures"),
    sidebar_get: () => opts.sidebar ?? null,
    sidebar_save: () => null,
  });
  return { calls, panes, saved: () => saved };
}

function renderCode() {
  useWorkspaces.setState({ list: [WS], activeId: WS.id, activeTab: { [WS.id]: TAB.id } });
  return render(
    <UiProvider>
      <CodeMain visible />
      <InstallHost />
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
// Pending layout saves are module state (500 ms timers): store them with
// this test's backend, so none lands in a later test's calls.
afterEach(async () => {
  await flushLayouts();
  setInvokeBackend(null);
});

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
    expect(screen.getByText("Start in shop")).toBeTruthy();
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

  it("F-CD-13.1 with only claude installed the grid shows Claude Code first and the rest under not installed", async () => {
    core({ panes: ["pn_a"], tree: { type: "leaf", paneId: "pn_a" } });
    renderCode();
    await waitFor(() => expect(paneEls()).toHaveLength(1));
    fireEvent.click(screen.getByRole("button", { name: "Close Shell · 1" }));
    const grid = await screen.findByTestId("launch-grid");
    await waitFor(() => expect(within(grid).getByTestId("tile-claude")).toBeTruthy());
    const tiles = within(grid).getAllByRole("button").filter((b) => b.dataset.testid?.startsWith("tile-"));
    expect(tiles.map((b) => b.dataset.testid)).toEqual(["tile-claude", "tile-shell"]);
    const more = within(grid).getByTestId("more-engines");
    expect(more.textContent).toContain("1 more not installed");
    fireEvent.click(more);
    expect(within(grid).getByTestId("missing-kimi")).toBeTruthy();
    expect(within(grid).getByTestId("install-kimi")).toBeTruthy();
    expect(within(grid).queryByTestId("engines-diagnostics")).toBeNull();
  });

  it("F-CD-13.2 with none installed the diagnostics list the shells probed and Check again re-detects", async () => {
    const { calls } = core({ panes: ["pn_a"], tree: { type: "leaf", paneId: "pn_a" }, noEngines: true });
    renderCode();
    await waitFor(() => expect(paneEls()).toHaveLength(1));
    fireEvent.click(screen.getByRole("button", { name: "Close Shell · 1" }));
    const diag = await screen.findByTestId("engines-diagnostics");
    // The cached report shows at once, before any Check again (a webview reload).
    await waitFor(() => expect(within(diag).getByTestId("shells-asked").textContent).toContain("/bin/zsh -lic"));
    expect(calls.some((c) => c.cmd === "engines_detect")).toBe(false);
    fireEvent.click(within(diag).getByTestId("check-again"));
    await waitFor(() => expect(calls.some((c) => c.cmd === "engines_detect")).toBe(true));
    await waitFor(() => expect(within(diag).getByTestId("shells-asked").textContent).toContain("/bin/zsh -lic"));
    expect(within(diag).getByText(/~\/\.nvm\/versions\/node/)).toBeTruthy();
  });

  it("F-EN-03.1 the install sheet outlives the empty tab's grid: Run types the exact command, the end mark re-detects", async () => {
    const { calls } = core({ panes: ["pn_a"], tree: { type: "leaf", paneId: "pn_a" } });
    renderCode();
    await waitFor(() => expect(paneEls()).toHaveLength(1));
    fireEvent.click(screen.getByRole("button", { name: "Close Shell · 1" }));
    const grid = await screen.findByTestId("launch-grid");
    fireEvent.click(within(grid).getByTestId("more-engines"));
    fireEvent.click(within(grid).getByTestId("install-kimi"));
    expect((await screen.findByTestId("install-command")).textContent).toBe("npm install -g kimi-cli");
    fireEvent.click(screen.getByTestId("install-run"));
    // The installer's pane replaces the grid; the sheet stays.
    await waitFor(() => expect(paneEls()).toHaveLength(1));
    expect(screen.queryByTestId("launch-grid")).toBeNull();
    const paneId = paneEls()[0]?.dataset.paneId ?? "";
    act(() => emitLocal("pty:mark", { paneId, kind: "A", exit: null, ts: 1 }));
    await waitFor(() =>
      expect(calls.find((c) => c.cmd === "pty_write" && c.args?.paneId === paneId)?.args?.data).toBe("npm install -g kimi-cli\r"),
    );
    act(() => emitLocal("pty:mark", { paneId, kind: "D", exit: 0, ts: 2 }));
    await waitFor(() => expect(calls.some((c) => c.cmd === "engines_detect")).toBe(true));
    expect(screen.getByTestId("install-sheet")).toBeTruthy();
  });

  it("F-CD-12.1 agent panes show their name; renaming to a name in use says so", async () => {
    core({ panes: ["pn_a"], tree: { type: "leaf", paneId: "pn_a" } });
    renderCode();
    await waitFor(() => expect(paneEls()).toHaveLength(1));
    for (const _ of [0, 1]) {
      act(() => void useLayout.getState().requestSplit(TAB.id, null, "right"));
      const picker = await screen.findByTestId("pane-picker");
      await waitFor(() => expect(within(picker).getByTestId("tile-claude")).toBeTruthy());
      fireEvent.click(within(picker).getByTestId("tile-claude"));
      await waitFor(() => expect(document.querySelector("[role=dialog]")).toBeNull());
    }
    await waitFor(() => expect(screen.getAllByTestId("pane-name").map((el) => el.textContent)).toEqual(["Ada", "Bea"]));
    fireEvent.doubleClick(screen.getAllByTestId("pane-name")[1] as HTMLElement);
    const input = await screen.findByRole("textbox", { name: "New name for Bea" });
    fireEvent.change(input, { target: { value: "Ada" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect((await screen.findByRole("alert")).textContent).toBe("Another pane is already called Ada");
    fireEvent.change(input, { target: { value: "Rhea" } });
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => expect(screen.getAllByTestId("pane-name").map((el) => el.textContent)).toEqual(["Ada", "Rhea"]));
  });

  it("F-CD-12.2 closing a working agent asks first and closes only after Stop and close", async () => {
    const { calls } = core({ panes: ["pn_a"], tree: { type: "leaf", paneId: "pn_a" } });
    renderCode();
    await waitFor(() => expect(paneEls()).toHaveLength(1));
    act(() => void useLayout.getState().requestSplit(TAB.id, null, "right"));
    const picker = await screen.findByTestId("pane-picker");
    await waitFor(() => expect(within(picker).getByTestId("tile-claude")).toBeTruthy());
    fireEvent.click(within(picker).getByTestId("tile-claude"));
    await waitFor(() => expect(paneEls()).toHaveLength(2));
    // The picker's closing leaves the rest aria-hidden until it unmounts.
    await waitFor(() => expect(document.querySelector("[role=dialog]")).toBeNull());
    const agent = paneEls()[1]?.dataset.paneId ?? "";
    act(() =>
      useStatus.getState().apply([{ key: `pane:${agent}`, status: "working", reason: null, since: 1, ts: 1, tool: null, title: null, preview: null }], []),
    );
    fireEvent.click(within(paneEls()[1] as HTMLElement).getByRole("button", { name: /^Close / }));
    const dialog = await screen.findByRole("alertdialog");
    expect(within(dialog).getByText("Stop Ada? It's working.")).toBeTruthy();
    fireEvent.click(within(dialog).getByRole("button", { name: "Keep running" }));
    expect(calls.some((c) => c.cmd === "pane_close")).toBe(false);
    fireEvent.click(within(paneEls()[1] as HTMLElement).getByRole("button", { name: /^Close / }));
    fireEvent.click(within(await screen.findByRole("alertdialog")).getByRole("button", { name: "Stop and close" }));
    await waitFor(() => expect(paneEls()).toHaveLength(1));
    expect(calls.filter((c) => c.cmd === "pane_close").map((c) => c.args?.paneId)).toEqual([agent]);
  });

  it("F-CD-12.3 ui:focus shows the pane and flashes it for 1.2 s", async () => {
    core({
      panes: ["pn_a", "pn_b"],
      tree: { type: "split", dir: "row", ratio: 0.5, a: { type: "leaf", paneId: "pn_a" }, b: { type: "leaf", paneId: "pn_b" } },
    });
    renderCode();
    followFocus();
    await waitFor(() => expect(paneEls()).toHaveLength(2));
    vi.useFakeTimers({ shouldAdvanceTime: true });
    act(() => emitLocal("ui:focus", { source: { type: "pane", paneId: "pn_b" }, flash: true, location: { workspaceId: WS.id, tabId: TAB.id } }));
    await waitFor(() => expect(paneEls()[1]?.className).toContain("tn-flash"));
    expect(useLayout.getState().tabs[TAB.id]?.active).toBe("pn_b");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1_100);
    });
    expect(paneEls()[1]?.className).toContain("tn-flash");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(200);
    });
    expect(paneEls()[1]?.className).not.toContain("tn-flash");
    vi.useRealTimers();
  });
});

describe("restore (F-SH-07)", () => {
  async function restored(paused: Record<string, string | null>) {
    const backend = core({ panes: Object.keys(paused), tree: { type: "leaf", paneId: Object.keys(paused)[0] }, paused });
    await useRestore.getState().load();
    renderCode();
    await waitFor(() => expect(paneEls()).toHaveLength(1));
    return backend;
  }
  const card = () => screen.getByTestId("paused-card");

  it("F-AC-06.1 an agent pane's header shows its context fill: 150k of 200k is 75% and neutral, 170k amber", async () => {
    core({ panes: ["pn_a"], tree: { type: "leaf", paneId: "pn_a" }, paused: { pn_a: null } });
    const stop = followContexts();
    renderCode();
    await waitFor(() => expect(paneEls()).toHaveLength(1));
    expect(screen.queryByRole("meter", { name: "Context" })).toBeNull();
    act(() => emitLocal("context:changed", { paneId: "pn_a", used: 150_000, window: 200_000 }));
    const meter = await screen.findByRole("meter", { name: "Context" });
    expect(meter.getAttribute("aria-valuenow")).toBe("75");
    expect(meter.textContent).toBe("150K / 200K");
    expect(meter.dataset.tone).toBe("muted");
    expect(meter.title).toBe("Context used by this session. Near the limit, the engine may compact or stop.");
    act(() => emitLocal("context:changed", { paneId: "pn_a", used: 170_000, window: 200_000 }));
    await waitFor(() => expect(screen.getByRole("meter", { name: "Context" }).dataset.tone).toBe("warning"));
    stop();
  });

  it("F-SH-07.1 a restored agent pane shows its paused card over the saved output, never a terminal", async () => {
    const { calls } = await restored({ pn_a: null });
    expect(card().dataset.state).toBe("paused");
    expect(within(card()).getByText("Paused after restart")).toBeTruthy();
    expect(within(card()).getByTestId("restored-scrollback")).toBeTruthy();
    expect(screen.queryByTestId("terminal")).toBeNull();
    expect(calls.some((c) => c.cmd === "pty_spawn" || c.cmd === "restore_resume")).toBe(false);
  });

  it("F-SH-07.2 Resume starts the agent and the pane becomes its terminal", async () => {
    const { calls } = await restored({ pn_a: null });
    fireEvent.click(within(card()).getByRole("button", { name: "Resume" }));
    await waitFor(() => expect(screen.getByTestId("terminal").dataset.paneId).toBe("pn_a"));
    expect(calls.filter((c) => c.cmd === "restore_resume").map((c) => c.args?.paneId)).toEqual(["pn_a"]);
  });

  it("F-SH-07.4 a missing launch configuration disables Resume and says why; Start fresh still works", async () => {
    const { calls } = await restored({ pn_a: "Launch configuration unavailable" });
    const resume = within(card()).getByRole("button", { name: "Resume" }) as HTMLButtonElement;
    expect(resume.disabled).toBe(true);
    expect(within(card()).getByTestId("restore-reason").textContent).toBe("Launch configuration unavailable");
    fireEvent.click(resume);
    expect(calls.some((c) => c.cmd === "restore_resume")).toBe(false);
    fireEvent.click(within(card()).getByRole("button", { name: "Start fresh" }));
    await waitFor(() => expect(screen.getByTestId("terminal")).toBeTruthy());
    expect(calls.filter((c) => c.cmd === "restore_start_fresh")).toHaveLength(1);
  });

  it("F-SH-07.5 Ctrl+C on a card cancels its restore and keeps the saved output", async () => {
    const { calls } = await restored({ pn_a: null });
    fireEvent.keyDown(card(), { key: "c", ctrlKey: true });
    await waitFor(() => expect(card().dataset.state).toBe("cancelled"));
    expect(calls.filter((c) => c.cmd === "restore_cancel").map((c) => c.args?.paneId)).toEqual(["pn_a"]);
    expect(within(card()).getByTestId("restored-scrollback")).toBeTruthy();
    expect(within(card()).getByRole("status").textContent).toContain("Restore cancelled");
    expect(screen.queryByTestId("terminal")).toBeNull();
  });
});

describe("browser panes (F-CD-17)", () => {
  beforeEach(() => {
    followBrowser();
    followBrowserTabs();
  });

  it("F-CD-17 the picker opens a Browser pane: no process, the browser's empty state, named after its page", async () => {
    const { calls } = core({ panes: ["pn_a"], tree: { type: "leaf", paneId: "pn_a" } });
    renderCode();
    await waitFor(() => expect(paneEls()).toHaveLength(1));
    act(() => void useLayout.getState().requestSplit(TAB.id, "pn_a", "right"));
    fireEvent.click(within(await screen.findByTestId("pane-picker")).getByTestId("tile-browser"));
    await waitFor(() => expect(paneEls()).toHaveLength(2));
    const browserPane = () => paneEls().find((el) => el.dataset.kind === "browser") as HTMLElement;
    expect(calls.find((c) => c.cmd === "pane_create" && c.args?.kind === "browser")?.args).toMatchObject({ url: null });
    expect(within(browserPane()).getByTestId("pane-title").textContent).toBe("Browser");
    expect(within(browserPane()).getByText("Open a page")).toBeTruthy();
    // Only the shell has a terminal.
    expect(screen.getAllByTestId("terminal")).toHaveLength(1);
    const id = String(browserPane().dataset.paneId);
    act(() => emitLocal("browser:state", page(id, "http://localhost:5173/", "Fixtures")));
    await waitFor(() => expect(within(browserPane()).getByTestId("pane-title").textContent).toBe("Fixtures"));
  });

  it("F-CD-17 Open in sidebar moves the page, history and all, then closes the pane", async () => {
    const { calls } = core({
      panes: ["pn_a", "pn_b"],
      browsers: { pn_b: "http://localhost:5173/" },
      tree: { type: "split", dir: "row", ratio: 0.5, a: { type: "leaf", paneId: "pn_a" }, b: { type: "leaf", paneId: "pn_b" } },
    });
    renderCode();
    await waitFor(() => expect(paneEls()).toHaveLength(2));
    act(() => emitLocal("browser:state", page("pn_b", "http://localhost:5173/", "Fixtures")));
    fireEvent.keyDown(await screen.findByRole("button", { name: "Actions for Fixtures" }), { key: "Enter" });
    fireEvent.click(await screen.findByRole("menuitem", { name: "Open in sidebar" }));
    await waitFor(() => expect(paneEls()).toHaveLength(1));
    const tab = useSidebar.getState().tabs.find((x) => x.kind === "browser");
    expect(tab).toMatchObject({ title: "Fixtures", url: "http://localhost:5173/" });
    const moves = calls.filter((c) => c.cmd === "browser_move" || c.cmd === "pane_close").map((c) => [c.cmd, c.args]);
    expect(moves).toEqual([
      ["browser_move", { from: "pn_b", to: tab?.id }],
      ["pane_close", { paneId: "pn_b" }],
    ]);
    // The page goes on in the sidebar: nothing closed it.
    expect(calls.some((c) => c.cmd === "browser_close")).toBe(false);
  });

  it("F-CD-17 Move to a pane brings a sidebar page back beside the focused pane", async () => {
    const saved: SidebarState = {
      open: true,
      tabs: [{ id: "browser-1", kind: "browser", title: "Fixtures", url: "http://localhost:5173/" }],
      active: "browser-1",
      width: 470,
      fullscreen: false,
      pinned: false,
    };
    const { calls } = core({ panes: ["pn_a"], tree: { type: "leaf", paneId: "pn_a" }, sidebar: saved });
    renderCode();
    render(
      <UiProvider>
        <Sidebar />
      </UiProvider>,
    );
    await waitFor(() => expect(paneEls()).toHaveLength(1));
    act(() => emitLocal("browser:state", page("browser-1", "http://localhost:5173/", "Fixtures")));
    fireEvent.keyDown(await screen.findByTestId("browser-more"), { key: "Enter" });
    fireEvent.click(await screen.findByRole("menuitem", { name: "Move to a pane" }));
    await waitFor(() => expect(paneEls()).toHaveLength(2));
    await waitFor(() => expect(useSidebar.getState().tabs).toHaveLength(0));
    const created = useLayout.getState().tabs[TAB.id]?.active as string;
    const sent = calls.filter((c) => ["pane_create", "browser_move"].includes(c.cmd));
    expect(sent.map((c) => c.cmd)).toEqual(["pane_create", "browser_move"]);
    expect(sent[0]?.args).toMatchObject({ kind: "browser", url: null });
    expect(sent[1]?.args).toEqual({ from: "browser-1", to: created });
  });
});
