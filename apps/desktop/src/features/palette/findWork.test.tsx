import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { FindWorkResult, WorkspaceSummary } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { useFlash } from "../../stores/focus";
import { useLayout } from "../../stores/layout";
import { useSession } from "../../stores/session";
import { useSettings } from "../../stores/settings";
import { useTeammates } from "../../stores/teammates";
import { useThreads } from "../../stores/threads";
import { useUi } from "../../stores/ui";
import { useWorkspaces } from "../../stores/workspaces";
import { BOOT, SIGNED_IN, fakeBackend, resetStores } from "../../test/fakeBackend";
import { commandTitle, runCommand } from "../commands/commands";
import { useBoot } from "../shell/useBoot";
import { Palette } from "./Palette";

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
  liveCount: 3,
  tabs: [{ id: "tb_1", workspaceId: "ws_1", name: "Tab 1", sortKey: "a0", createdAt: 0 }],
};

const AUTH: FindWorkResult = {
  matches: [
    {
      source: { type: "pane", paneId: "pn_auth" },
      kind: "pane",
      label: "auth tests",
      detail: "12 passing",
      workspaceId: "ws_1",
      tabId: "tb_1",
      score: 2.5,
    },
  ],
  clear: true,
  ranking: "local",
};

function setup(findWork: (args?: Record<string, unknown>) => unknown) {
  const calls = fakeBackend({ find_work: findWork });
  useBoot.setState({ boot: BOOT });
  useWorkspaces.setState({ list: [WS], activeId: WS.id, activeTab: { [WS.id]: "tb_1" } });
  useLayout.setState({
    tabs: {
      tb_1: {
        tree: {
          type: "split",
          dir: "row",
          ratio: 0.5,
          a: { type: "leaf", paneId: "pn_docs" },
          b: { type: "leaf", paneId: "pn_auth" },
        },
        active: "pn_docs",
        maximized: null,
        panes: {},
        keepEmpty: false,
      },
    },
    loaded: { [WS.id]: true },
    boxes: {},
    dragging: false,
    picker: null,
  });
  render(
    <UiProvider>
      <Palette />
    </UiProvider>,
  );
  return calls;
}

const type = (text: string) => fireEvent.change(screen.getByRole("combobox"), { target: { value: text } });

beforeEach(() => {
  resetStores();
  vi.useFakeTimers({ shouldAdvanceTime: true });
});
afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe("Find work in the palette (F-CN-05)", () => {
  it('F-CN-05.1 "login testing" lists the auth tests pane with what it was doing, and choosing it shows and flashes the pane', async () => {
    const calls = setup(() => AUTH);
    act(() => useUi.getState().setPalette(true));
    // "Find work…" is a palette command that turns the palette into the search.
    fireEvent.click(screen.getByRole("option", { name: new RegExp(commandTitle("work.find")) }));
    expect(useUi.getState().paletteMode).toBe("find");
    expect(screen.getByPlaceholderText("Describe the work, like “the login tests”")).toBeTruthy();
    type("login testing");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(200);
    });
    expect(calls.filter((c) => c.cmd === "find_work").map((c) => c.args)).toEqual([
      { query: "login testing" },
    ]);
    const option = await screen.findByRole("option", { name: /auth tests/ });
    expect(option.textContent).toContain("12 passing");
    fireEvent.click(option);
    expect(useUi.getState().paletteOpen).toBe(false);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(useLayout.getState().tabs.tb_1?.active).toBe("pn_auth");
    expect(useFlash.getState().flashing.pn_auth).toBeDefined();
    // Closing it goes back to commands for the next Mod+K.
    expect(useUi.getState().paletteMode).toBe("commands");
  });

  it("F-CN-05 typing is debounced, and a thread match opens the thread", async () => {
    const calls = setup(() => ({
      matches: [
        {
          source: { type: "thread", threadId: "thr_9" },
          kind: "thread",
          label: "Fix flaky checkout",
          detail: "The Stripe webhook retries twice",
          workspaceId: "ws_1",
          tabId: null,
          score: 1.5,
        },
      ],
      clear: true,
      ranking: "local",
    }));
    act(() => runCommand("work.find"));
    for (const partial of ["s", "st", "str", "stripe"]) type(partial);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(200);
    });
    expect(calls.filter((c) => c.cmd === "find_work").map((c) => c.args)).toEqual([{ query: "stripe" }]);
    fireEvent.click(await screen.findByRole("option", { name: /Fix flaky checkout/ }));
    expect(useUi.getState().mode).toBe("thread");
    expect(useThreads.getState().selectedId).toBe("thr_9");
  });

  it("F-CN-05 the empty, nothing-found and failed states say what happened", async () => {
    let answer: unknown = { matches: [], clear: false };
    setup(() => {
      if (answer instanceof Error) throw { code: "internal", message: answer.message };
      return answer;
    });
    act(() => runCommand("work.find"));
    expect(screen.getByText(/Open panes, threads and teammates are searched on this computer/)).toBeTruthy();
    type("kubernetes");
    expect(screen.getByText("Looking…")).toBeTruthy();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(200);
    });
    expect(screen.getByText("Nothing open matches “kubernetes”")).toBeTruthy();
    answer = new Error("The store is busy.");
    type("helm");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(200);
    });
    expect(screen.getByText("Couldn't search: The store is busy.")).toBeTruthy();
  });

  it("F-CN-05 a build without Vibezeo accounts searches on this computer even with hosted ranking on (D-833)", async () => {
    setup(() => AUTH);
    useSession.setState({ auth: { ...SIGNED_IN, signedIn: false, email: null, hosted: false } });
    useSettings.setState((st) => ({ values: { ...st.values, find_work_hosted: true } }));
    act(() => runCommand("work.find"));
    expect(screen.getByText(/Open panes, threads and teammates are searched on this computer/)).toBeTruthy();
    expect(screen.queryByText(/1 credit a search/)).toBeNull();
    type("login testing");
    // The local search's short pause, not the hosted one's.
    await act(async () => {
      await vi.advanceTimersByTimeAsync(200);
    });
    expect(await screen.findByRole("option", { name: /auth tests/ })).toBeTruthy();
  });

  it("F-CN-05 a teammate match opens its page, and a hosted ranking says so", async () => {
    setup(() => ({
      matches: [
        {
          source: { type: "teammate", teammateId: "tm_rel" },
          kind: "teammate",
          label: "Release scribe",
          detail: "Drafted the 0.5.0 notes",
          workspaceId: null,
          tabId: null,
          score: 2.4,
        },
      ],
      clear: true,
      ranking: "hosted",
    }));
    useSettings.setState((st) => ({ values: { ...st.values, find_work_hosted: true } }));
    act(() => runCommand("work.find"));
    expect(
      screen.getByText(/Vibezeo's AI ranks open panes, threads and teammates \(1 credit a search\)/),
    ).toBeTruthy();
    type("changelog");
    // A credit a search: it waits for a longer pause.
    await act(async () => {
      await vi.advanceTimersByTimeAsync(200);
    });
    expect(screen.queryByRole("option", { name: /Release scribe/ })).toBeNull();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(600);
    });
    expect(screen.getByText("Find work · ranked by Vibezeo's AI")).toBeTruthy();
    fireEvent.click(await screen.findByRole("option", { name: /Release scribe/ }));
    expect(useUi.getState().mode).toBe("agent");
    expect(useTeammates.getState().selectedId).toBe("tm_rel");
  });
});
