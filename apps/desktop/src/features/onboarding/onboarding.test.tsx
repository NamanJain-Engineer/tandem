import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import type { EngineView, PaneRecord, StatusRecord, WorkspaceSummary } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { useLayout } from "../../stores/layout";
import { useOnboarding } from "../../stores/onboarding";
import { useSettings } from "../../stores/settings";
import { useSidebar } from "../../stores/sidebar";
import { useStatus } from "../../stores/status";
import { useUi } from "../../stores/ui";
import { useWorkspaces } from "../../stores/workspaces";
import { useAccounts } from "../../stores/accounts";
import { useEngines } from "../../stores/engines";
import { fakeBackend, resetStores, type Call } from "../../test/fakeBackend";
import { FirstFinish } from "./FirstFinish";
import { Onboarding } from "./Onboarding";

const STEPS = ["engines", "workspace", "finish"];

const WS = (root: string, branch: string | null = "main"): WorkspaceSummary => ({
  id: "ws_1",
  kind: "local",
  hostId: "local",
  root,
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
  branch,
  liveCount: 0,
  tabs: [{ id: "tb_1", workspaceId: "ws_1", name: "Tab 1", sortKey: "a0", createdAt: 0 }],
});

const engine = (
  id: string,
  label: string,
  logo: string,
  version: string | null,
  homeStrategy = "env",
): EngineView =>
  ({
    spec: {
      id,
      label,
      logo,
      chatAdapter: id === "kimi" ? null : id,
      homeStrategy,
      install: { lin: `npm i -g ${id}` },
    },
    install: version === null ? null : { path: `/bin/${id}`, version, source: "shell" },
    chatCapable: false,
    unsupportedReason: null,
    extraArgs: [],
  }) as unknown as EngineView;

const ENGINES = [
  engine("claude", "Claude Code", "C", "1.2.3"),
  engine("codex", "Codex", "X", "0.52.0"),
  // Signs in globally: its accounts can't be kept apart.
  engine("cursor-agent", "Cursor Agent", "Cu", "1.0.0", "none"),
  engine("kimi", "Kimi", "K", null, "none"),
];

const account = (id: string, engineId: string) => ({
  id,
  engineId,
  label: "Work",
  homePath: `/p/${id}`,
  isDefault: true,
  planLabel: null,
  createdAt: 0,
});

const PANE: PaneRecord = {
  id: "pn_1",
  tabId: "tb_1",
  workspaceId: "ws_1",
  kind: "agent",
  cwd: "/code/shop",
  engineId: "claude",
  accountId: null,
  humanName: null,
  title: null,
  restoreState: "none",
  createdAt: 0,
  browserUrl: null,
};

function setup(handlers: Parameters<typeof fakeBackend>[0] = {}): Call[] {
  let step: string | null = "engines";
  let list: WorkspaceSummary[] = [];
  return fakeBackend({
    onboarding_set: (a) => {
      step = a?.done ? null : String(a?.step);
      return { step, done: step === null, steps: STEPS };
    },
    engines_list: () => ENGINES,
    engines_report: () => null,
    workspace_pick_folder: () => "/code/shop",
    workspace_add: (a) => {
      list = [WS(String(a?.root))];
      return { workspace: list[0], created: true };
    },
    workspace_list: () => list,
    accounts_list: () => [],
    accounts_overrides: () => [],
    pane_create: (a) => ({ ...PANE, tabId: String(a?.tabId), engineId: String(a?.engineId) }),
    ...handlers,
  });
}

function mount() {
  return render(
    <UiProvider>
      <Onboarding />
    </UiProvider>,
  );
}

const at = (step: string) => useOnboarding.setState({ state: { step, done: false, steps: STEPS } });

describe("onboarding wizard (F-SH-08)", () => {
  beforeEach(() => {
    resetStores();
    useLayout.setState({ tabs: {}, loaded: {} });
    useAccounts.setState({ list: [], overrides: [], loaded: false, failed: false, signIn: {} });
    at("engines");
  });

  it("F-SH-08 DESIGN-5.3 the stepper shows three steps: Engines, Your first folder, Done", () => {
    setup();
    mount();
    const steps = within(screen.getByRole("navigation", { name: "Setup progress" })).getAllByRole("listitem");
    expect(steps.map((li) => li.textContent?.replace(/^\d/, ""))).toEqual([
      "Engines",
      "Your first folder",
      "Done",
    ]);
    expect(steps[0]?.getAttribute("aria-current")).toBe("step");
  });

  it("F-SH-08.4 the engines step lists found engines first with their version and sign-in, and folds the missing ones", async () => {
    const calls = setup({
      accounts_list: () => [account("acct_c", "claude")],
      accounts_status: () => "signed_in",
    });
    mount();
    expect(screen.getByRole("heading", { level: 1 }).textContent).toBe("Your coding agents");
    const rows = await screen.findByTestId("onboarding-engines");
    // Found engines only, in the registry's order; Kimi isn't installed.
    expect(Array.from(rows.querySelectorAll("li")).map((li) => li.dataset.engine)).toEqual([
      "claude",
      "codex",
      "cursor-agent",
    ]);
    const row = (id: string) => rows.querySelector<HTMLElement>(`[data-engine="${id}"]`) as HTMLElement;
    expect(row("claude").textContent).toContain("1.2.3");
    await waitFor(() => expect(row("claude").dataset.signedIn).toBe("true"));
    expect(row("claude").textContent).toContain("Signed in");
    expect(within(row("claude")).queryByRole("button", { name: /Sign in/ })).toBeNull();
    // Cursor Agent signs in by itself, globally: its version only.
    expect(row("cursor-agent").textContent).toContain("1.0.0");
    expect(within(row("cursor-agent")).queryByRole("button", { name: /Sign in/ })).toBeNull();

    // The missing ones fold under one line.
    const more = screen.getByRole("button", { name: "1 more you can install" });
    expect(more.getAttribute("aria-expanded")).toBe("false");
    expect(screen.queryByTestId("onboarding-missing-kimi")).toBeNull();
    fireEvent.click(more);
    expect(screen.getByTestId("onboarding-missing-kimi").textContent).toContain("npm i -g kimi");

    // Codex has no account yet: its sign-in is its own login, as a new account named first.
    fireEvent.click(within(row("codex")).getByRole("button", { name: "Sign in to Codex" }));
    const nickname = (await screen.findByTestId("login-nickname")) as HTMLInputElement;
    expect(nickname.value).toBe("Personal");
    expect(calls.some((c) => c.cmd === "accounts_begin_add")).toBe(false);
  });

  it("F-SH-08.4 the first signed-in engine is preselected as the default, and another can be chosen", async () => {
    setup({
      accounts_list: () => [account("acct_c", "claude"), account("acct_x", "codex")],
      accounts_status: (a) => (a?.accountId === "acct_x" ? "signed_in" : "signed_out"),
    });
    mount();
    const rows = await screen.findByTestId("onboarding-engines");
    const row = (id: string) => rows.querySelector<HTMLElement>(`[data-engine="${id}"]`) as HTMLElement;
    await waitFor(() => expect(row("codex").dataset.default).toBe("true"));
    expect(row("codex").textContent).toContain("Default");
    expect(row("claude").dataset.default).toBe("false");
    expect(within(row("claude")).getByRole("button", { name: "Sign in to Claude Code" })).toBeTruthy();
    fireEvent.click(within(row("claude")).getByRole("button", { name: "Make Claude Code the default" }));
    await waitFor(() => expect(row("claude").dataset.default).toBe("true"));
    expect(row("codex").dataset.default).toBe("false");
    expect(useOnboarding.getState().engine).toBe("claude");
  });

  it("F-SH-08 the engines step with nothing found says so, and offers Rescan", async () => {
    setup({ engines_list: () => [engine("kimi", "Kimi", "K", null, "none")] });
    mount();
    expect((await screen.findByTestId("onboarding-engines-empty")).textContent).toBe(
      "No coding agents found yet. Install one below, then Rescan.",
    );
    expect(screen.getByRole("button", { name: "Rescan" })).toBeTruthy();
  });

  it("F-SH-08.2 Skip and Back move through persisted steps", async () => {
    const calls = setup();
    mount();
    expect((screen.getByTestId("onboarding-back") as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(screen.getByTestId("onboarding-skip"));
    await waitFor(() => expect(screen.getByTestId("onboarding").dataset.step).toBe("workspace"));
    expect(screen.getByRole("heading", { level: 1 }).textContent).toBe("Pick a project folder");
    fireEvent.click(screen.getByTestId("onboarding-back"));
    await waitFor(() => expect(screen.getByTestId("onboarding").dataset.step).toBe("engines"));
    expect(calls.filter((c) => c.cmd === "onboarding_set").map((c) => c.args?.step)).toEqual([
      "workspace",
      "engines",
    ]);
  });

  it("F-SH-08.1 choosing a folder moves to Done, whose Start {engine} in {folder} opens Code mode with that agent launching", async () => {
    const calls = setup({
      accounts_list: () => [account("acct_c", "claude")],
      accounts_status: () => "signed_in",
    });
    at("workspace");
    useUi.setState({ mode: "thread" });
    mount();
    fireEvent.click(screen.getByTestId("onboarding-choose-folder"));
    await waitFor(() => expect(screen.getByTestId("onboarding").dataset.step).toBe("finish"));
    expect(screen.getByRole("heading", { level: 1 }).textContent).toBe("You're set");
    const start = await screen.findByRole("button", { name: "Start Claude Code in shop" });
    expect(screen.getByTestId("onboarding-skip").textContent).toBe("Open shop without an agent");
    fireEvent.click(start);
    await waitFor(() => expect(useOnboarding.getState().state?.done).toBe(true));
    // The launch grid's own path: one agent pane in the workspace's first tab.
    expect(calls.filter((c) => c.cmd === "pane_create").map((c) => c.args)).toEqual([
      { tabId: "tb_1", kind: "agent", engineId: "claude", url: undefined },
    ]);
    expect(calls.filter((c) => c.cmd === "onboarding_set").at(-1)?.args).toEqual({ done: true });
    expect(useLayout.getState().tabs.tb_1?.active).toBe("pn_1");
    expect(useWorkspaces.getState().activeId).toBe("ws_1");
    expect(useUi.getState().mode).toBe("code");
    // Setup's end arms the first finish (the core does the same, D-2651).
    expect(useSettings.getState().values.first_finish).toBe("armed");
  });

  it("F-SH-08.1 a launch that fails says why and still lands on the workspace's launch grid", async () => {
    setup({
      pane_create: () => {
        throw { code: "spawn_failed", message: "Couldn't start Claude Code" };
      },
    });
    useWorkspaces.setState({ list: [WS("/code/shop")] });
    useOnboarding.setState({ folder: "/code/shop" });
    at("finish");
    mount();
    fireEvent.click(await screen.findByRole("button", { name: "Start Claude Code in shop" }));
    await waitFor(() => expect(useOnboarding.getState().state?.done).toBe(true));
    expect(await screen.findByText("Couldn't start Claude Code")).toBeTruthy();
    expect(useLayout.getState().tabs.tb_1?.keepEmpty).toBe(true);
    expect(useUi.getState().mode).toBe("code");
  });

  it("F-SH-08 F-TL-01 Done says usage statistics and crash reports stay off, and links to Settings › Privacy", async () => {
    const calls = setup();
    useWorkspaces.setState({ list: [WS("/code/shop")] });
    at("finish");
    mount();
    expect(screen.getByTestId("onboarding-privacy").textContent).toBe(
      "Usage statistics and crash reports stay off. You can turn them on in Settings › Privacy.",
    );
    // No switches on the way: the line is the whole of it.
    expect(screen.queryByRole("switch")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Settings › Privacy" }));
    await waitFor(() => expect(useOnboarding.getState().state?.done).toBe(true));
    expect(useUi.getState().settingsOpen).toBe(true);
    expect(useUi.getState().settingsPage).toBe("privacy");
    expect(calls.some((c) => c.cmd === "pane_create")).toBe(false);
    expect(calls.some((c) => c.cmd === "settings_set" && String(c.args?.key).startsWith("telemetry"))).toBe(
      false,
    );
  });

  it("F-SH-08.3 Open {folder} without an agent finishes on that workspace's launch grid", async () => {
    const calls = setup();
    useWorkspaces.setState({ list: [WS("/code/shop")] });
    at("finish");
    mount();
    fireEvent.click(await screen.findByRole("button", { name: "Open shop without an agent" }));
    await waitFor(() => expect(useOnboarding.getState().state?.done).toBe(true));
    expect(useLayout.getState().tabs.tb_1?.keepEmpty).toBe(true);
    expect(useWorkspaces.getState().activeId).toBe("ws_1");
    expect(calls.some((c) => c.cmd === "pane_create")).toBe(false);
  });

  it("F-SH-08.3 skipping every step finishes without a workspace or an agent", async () => {
    const calls = setup();
    mount();
    fireEvent.click(screen.getByTestId("onboarding-skip"));
    await waitFor(() => expect(screen.getByTestId("onboarding").dataset.step).toBe("workspace"));
    expect(screen.queryByTestId("onboarding-continue")).toBeNull();
    fireEvent.click(screen.getByTestId("onboarding-skip"));
    await waitFor(() => expect(screen.getByTestId("onboarding").dataset.step).toBe("finish"));
    expect(screen.queryByRole("button", { name: /^Start / })).toBeNull();
    expect(screen.getByTestId("onboarding-skip").textContent).toBe("Finish setup");
    fireEvent.click(screen.getByTestId("onboarding-skip"));
    await waitFor(() => expect(useOnboarding.getState().state?.done).toBe(true));
    expect(calls.some((c) => c.cmd === "pane_create" || c.cmd === "workspace_add")).toBe(false);
  });
});

describe("the first finish (F-SH-08 behaviour 4)", () => {
  const record = (status: StatusRecord["status"], ts: number, key = "pane:pn_1"): StatusRecord => ({
    key,
    status,
    reason: null,
    since: ts,
    ts,
    tool: null,
    title: null,
    preview: null,
  });

  function world(firstFinish: string, branch: string | null = "main") {
    useSettings.setState({ values: { first_finish: firstFinish }, loaded: true });
    useWorkspaces.setState({ list: [WS("/code/shop", branch)], activeId: null, activeTab: {} });
    useEngines.setState({ list: ENGINES, loaded: true });
    useLayout.setState({
      tabs: {
        tb_1: {
          tree: { type: "leaf", paneId: "pn_1" },
          active: "pn_1",
          maximized: null,
          panes: { pn_1: PANE, pn_2: { ...PANE, id: "pn_2" } },
          keepEmpty: false,
        },
      } as never,
      loaded: { ws_1: true },
    });
  }

  function mountFinish() {
    return render(
      <UiProvider>
        <FirstFinish tipDelayMs={0} />
      </UiProvider>,
    );
  }

  beforeEach(() => {
    resetStores();
    useStatus.setState({ records: {}, unread: {}, focusedKey: null });
  });

  it("F-SH-08.5 the first agent to finish brings one toast whose action opens its changes, and never again", async () => {
    const calls = setup();
    world("armed");
    mountFinish();
    act(() => useStatus.getState().apply([record("working", 1)], []));
    expect(screen.queryByText(/finished in/)).toBeNull();
    act(() => useStatus.getState().apply([record("done", 2)], []));
    expect(await screen.findByText("Claude Code finished in shop")).toBeTruthy();
    await waitFor(() =>
      expect(calls.find((c) => c.cmd === "settings_set" && c.args?.key === "first_finish")?.args?.value).toBe(
        "shown",
      ),
    );
    // A second agent finishing brings nothing.
    act(() => useStatus.getState().apply([record("working", 3, "pane:pn_2")], []));
    act(() => useStatus.getState().apply([record("done", 4, "pane:pn_2")], []));
    expect(screen.queryAllByText(/finished in/)).toHaveLength(1);

    fireEvent.click(screen.getByRole("button", { name: "Review its changes" }));
    expect(useWorkspaces.getState().activeId).toBe("ws_1");
    expect(useUi.getState().mode).toBe("code");
    expect(useSidebar.getState().tabs.find((tab) => tab.id === useSidebar.getState().active)?.kind).toBe(
      "files",
    );
  });

  it("F-SH-08.5 a pane already Done when the app starts, or a finish once it was shown, brings no toast", async () => {
    setup();
    world("armed");
    mountFinish();
    act(() => useStatus.getState().apply([record("done", 1)], []));
    expect(screen.queryByText(/finished in/)).toBeNull();

    world("shown");
    act(() => useStatus.getState().apply([record("working", 2, "pane:pn_2")], []));
    act(() => useStatus.getState().apply([record("done", 3, "pane:pn_2")], []));
    expect(screen.queryByText(/finished in/)).toBeNull();
  });

  it("F-SH-08.5 an agent started this session counts when its start and finish arrive together", async () => {
    setup();
    world("armed");
    const layout = useLayout.getState().tabs.tb_1;
    mountFinish();
    useLayout.setState({
      tabs: {
        tb_1: {
          ...layout!,
          panes: { ...layout!.panes, pn_3: { ...PANE, id: "pn_3", createdAt: Date.now() + 1 } },
        },
      },
    });
    act(() => useStatus.getState().apply([record("done", 2, "pane:pn_3")], []));
    expect(await screen.findByText("Claude Code finished in shop")).toBeTruthy();
  });

  it("F-SH-08.5 a profile that never ran the new setup isn't armed", async () => {
    setup();
    world("unset");
    mountFinish();
    act(() => useStatus.getState().apply([record("working", 1)], []));
    act(() => useStatus.getState().apply([record("done", 2)], []));
    expect(screen.queryByText(/finished in/)).toBeNull();
  });

  it("F-SH-08.5 outside a git repository the action opens the pane", async () => {
    setup();
    world("armed", null);
    useWorkspaces.setState({ activeId: null });
    mountFinish();
    act(() => useStatus.getState().apply([record("working", 1)], []));
    act(() => useStatus.getState().apply([record("done", 2)], []));
    fireEvent.click(await screen.findByRole("button", { name: "Open it" }));
    expect(useWorkspaces.getState().activeId).toBe("ws_1");
    expect(useLayout.getState().tabs.tb_1?.active).toBe("pn_1");
  });

  it("F-SH-08.5 the Dictate tip follows the first finish once, and Set up Dictate opens its settings", async () => {
    setup({
      dictate_status: () => ({ enabled: true, hold: { label: "Right Ctrl" } }),
    });
    world("armed");
    mountFinish();
    act(() => useStatus.getState().apply([record("working", 1)], []));
    act(() => useStatus.getState().apply([record("done", 2)], []));
    expect(await screen.findByText("Try Dictate")).toBeTruthy();
    expect(
      screen.getByText(
        "Hold Right Ctrl in any app and speak: Dictate types where your cursor is, recognized on this computer.",
      ),
    ).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Set up Dictate" }));
    expect(useUi.getState().settingsOpen).toBe(true);
    expect(useUi.getState().settingsPage).toBe("dictate");
  });

  it("F-SH-08.5 with Dictate off, no tip follows", async () => {
    setup({ dictate_status: () => ({ enabled: false, hold: { label: "Right Ctrl" } }) });
    world("armed");
    mountFinish();
    act(() => useStatus.getState().apply([record("working", 1)], []));
    act(() => useStatus.getState().apply([record("done", 2)], []));
    expect(await screen.findByText("Claude Code finished in shop")).toBeTruthy();
    await new Promise((r) => setTimeout(r, 20));
    expect(screen.queryByText("Try Dictate")).toBeNull();
  });
});
