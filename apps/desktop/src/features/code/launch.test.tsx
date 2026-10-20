import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type {
  EngineView,
  IpcError,
  LaunchReport,
  Preset,
  Seat,
  Tab,
  WorkspaceSummary,
} from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { setInvokeBackend } from "../../ipc/invoke";
import { useEngines } from "../../stores/engines";
import {
  agentSeat,
  browserSeat,
  branchPreview,
  defaultEngine,
  lineupFrom,
  shellSeat,
  slugPreview,
  useLaunch,
} from "../../stores/launch";
import { useLayout } from "../../stores/layout";
import { useSession } from "../../stores/session";
import { useWorkspaces } from "../../stores/workspaces";
import { FREE_VIEW, fakeBackend, resetStores, type Call } from "../../test/fakeBackend";
import { LaunchSheet, launchLabel } from "./LaunchSheet";

const engine = (id: string, label: string, installed = true): EngineView =>
  ({
    spec: { id, label, logo: label[0] },
    install: installed ? { version: "1.0.0", path: `/bin/${id}` } : null,
    chatCapable: false,
    unsupportedReason: null,
    extraArgs: [],
  }) as unknown as EngineView;

function ws(id: string, branch: string | null): WorkspaceSummary {
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
    branch,
    liveCount: 0,
    tabs: [tab],
  };
}

const builtin = (slug: string, name: string, seats: Seat[], isolation: "shared" | "worktree"): Preset => ({
  id: `pst_builtin_${slug}`,
  name,
  seats,
  isolation,
  builtin: true,
  createdAt: 0,
});
const BUILTINS: Preset[] = [
  builtin("solo", "Solo", [agentSeat(null)], "shared"),
  builtin("pair", "Pair", [agentSeat(null), agentSeat(null, "Reviewer")], "shared"),
  builtin("bench", "Bench", [agentSeat(null), shellSeat()], "shared"),
  builtin(
    "fleet",
    "Fleet",
    [agentSeat(null), agentSeat(null), agentSeat(null, "Tester"), agentSeat(null, "Reviewer")],
    "worktree",
  ),
];

let calls: Call[] = [];
const named = (cmd: string) => calls.filter((c) => c.cmd === cmd);

function open(
  workspace: WorkspaceSummary,
  handlers: Parameters<typeof fakeBackend>[0] = {},
  detected = true,
) {
  calls = fakeBackend({
    presets_list: () => BUILTINS,
    workspace_list: () => [workspace],
    layout_get: () => [],
    ...handlers,
  });
  useWorkspaces.setState({ list: [workspace], activeId: workspace.id, activeTab: {} });
  useEngines.setState(
    detected
      ? {
          list: [
            engine("codex", "Codex"),
            engine("claude", "Claude Code"),
            engine("gemini", "Gemini CLI", false),
          ],
          loaded: true,
        }
      : { list: [], loaded: false },
  );
  useLaunch.setState({ workspaceId: null, presets: [], loaded: false });
  useLaunch.getState().open(workspace.id);
  return render(
    <UiProvider>
      <LaunchSheet />
    </UiProvider>,
  );
}

const go = () => screen.getByTestId("launch-go");
const preset = async (name: string) => {
  fireEvent.click(await screen.findByRole("radio", { name }));
};

beforeEach(() => {
  resetStores();
  useLayout.setState({ tabs: {}, loaded: {}, boxes: {}, dragging: false, picker: null });
});
afterEach(() => setInvokeBackend(null));

describe("launch sheet (F-CD-14, UI-12)", () => {
  it("F-CD-14 the default engine is Claude Code when installed, and presets resolve to it", () => {
    expect(defaultEngine([engine("codex", "Codex"), engine("claude", "Claude Code")])).toBe("claude");
    expect(defaultEngine([engine("claude", "Claude Code", false), engine("codex", "Codex")])).toBe("codex");
    expect(defaultEngine([])).toBeNull();
    expect(lineupFrom(BUILTINS[2] as Preset, "claude").map((s) => [s.kind, s.engineId])).toEqual([
      ["agent", "claude"],
      ["shell", null],
    ]);
  });

  it("F-CD-15.1 two worktrees for 'fix auth' preview as tandem/fix-auth and tandem/fix-auth-2", () => {
    expect(slugPreview("Fix Auth!")).toBe("fix-auth");
    expect(slugPreview("")).toBe("session");
    expect(branchPreview("fix auth", [{ ...agentSeat("claude"), count: 2 }, shellSeat()])).toEqual([
      "tandem/fix-auth",
      "tandem/fix-auth-2",
    ]);
    // Without a task, each agent's role names its branch.
    expect(branchPreview("", [agentSeat("claude"), agentSeat("claude", "Reviewer")])).toEqual([
      "tandem/builder",
      "tandem/reviewer",
    ]);
  });

  it("F-CD-14 the Launch button names what it will open, per preset", async () => {
    open(ws("shop", "main"));
    await preset("Fleet");
    expect(go().textContent).toContain("Launch 4 agents");
    await preset("Bench");
    expect(go().textContent).toContain("Launch 1 agent + 1 shell");
    expect(launchLabel([shellSeat()])).toBe("Launch 1 shell");
  });

  it("F-CD-14 a browser seat takes an address, stays in the checkout and launches with the others", async () => {
    const report: LaunchReport = { launched: [], failed: [], sharedTabId: "tb_shop" };
    open(ws("shop", "main"), { launch_lineup: () => report });
    fireEvent.keyDown(await screen.findByTestId("launch-add-seat"), { key: "Enter" });
    fireEvent.click(await screen.findByRole("menuitem", { name: "Browser URL" }));
    const seats = () => screen.getAllByTestId("launch-seat");
    await waitFor(() => expect(seats()).toHaveLength(2));
    expect(seats()[1]?.dataset.kind).toBe("browser");
    // No address yet: nothing to launch it with.
    expect((go() as HTMLButtonElement).disabled).toBe(true);
    fireEvent.change(within(seats()[1] as HTMLElement).getByRole("textbox", { name: "Address for seat 2" }), {
      target: { value: "localhost:3000" },
    });
    expect(go().textContent).toContain("Launch 1 agent + 1 browser");
    expect(screen.getByTestId("launch-summary").textContent).toBe("This opens 2 panes.");
    expect(screen.getByText("Shell and browser seats stay in the repository folder.")).toBeTruthy();
    fireEvent.click(go());
    await waitFor(() => expect(named("launch_lineup")).toHaveLength(1));
    const sent = (named("launch_lineup")[0]?.args as { seats: Seat[] }).seats;
    expect(sent[1]).toEqual({ ...browserSeat("localhost:3000") });
    expect(launchLabel([browserSeat("a"), { ...browserSeat("b"), count: 2 }])).toBe("Launch 3 browsers");
  });

  it("F-CD-15 Fleet in a repository previews a worktree per agent; Pair shares the checkout", async () => {
    open(ws("shop", "main"));
    await preset("Fleet");
    fireEvent.change(screen.getByTestId("launch-task"), { target: { value: "fix flaky tests" } });
    expect(screen.getByTestId("launch-summary").textContent).toBe(
      "This opens 4 panes and 4 worktrees (tandem/fix-flaky-tests, tandem/fix-flaky-tests-2, tandem/fix-flaky-tests-3, tandem/fix-flaky-tests-4).",
    );
    await preset("Pair");
    expect(screen.getByTestId("launch-summary").textContent).toBe("This opens 2 panes.");
  });

  it("F-RM-01 a folder on a host lists the host's engines, and Fleet gives each a worktree there", async () => {
    const remote = { ...ws("project", "main"), kind: "ssh", hostId: "hst_1", root: "/home/tandem/project" };
    open(remote, {
      engines_list: (a) =>
        (a as { hostId?: string }).hostId === "hst_1"
          ? [engine("aider", "Aider")]
          : [engine("claude", "Claude Code")],
    });
    await preset("Fleet");
    // The host's Aider, not this computer's Claude Code.
    const seats = await screen.findAllByRole("combobox", { name: /Engine/ });
    expect(seats[0]?.textContent).toContain("Aider");
    // Worktrees are made by the host's git (D-1602).
    const box = screen.getByRole("checkbox", { name: /Give each agent its own worktree/ });
    expect(box.getAttribute("aria-checked")).toBe("true");
    expect(box.getAttribute("data-disabled")).toBeNull();
    expect(named("engines_list").some((c) => (c.args as { hostId?: string }).hostId === "hst_1")).toBe(true);
  });

  it("F-RM-02 a WSL folder launches shared and says why", async () => {
    open({ ...ws("shop", "main"), kind: "wsl", hostId: "hst_2", wslDistro: "Ubuntu" });
    await preset("Fleet");
    const box = screen.getByRole("checkbox", { name: /Give each agent its own worktree/ });
    expect(box.getAttribute("aria-checked")).toBe("false");
    expect(screen.getByText("Worktrees aren't available in WSL folders yet")).toBeTruthy();
  });

  it("F-CD-22.2 a multi-repo folder's worktree box needs Pro, and is on with it", async () => {
    open({ ...ws("shop", null), repos: ["api", "web"] });
    await preset("Fleet");
    const box = () => screen.getByRole("checkbox", { name: /Give each agent its own worktree/ });
    expect(box().getAttribute("data-disabled")).not.toBeNull();
    expect(screen.getByText("Worktrees across several repositories come with Pro. On Free, agents can still work in this folder directly, and adding one repository on its own gives it worktrees.")).toBeTruthy();
  });

  it("F-CD-22.1 on Pro a multi-repo folder can give each agent its own worktree", async () => {
    useSession.setState({
      entitlement: { ...FREE_VIEW, plan: "pro", limits: { ...FREE_VIEW.limits, multiRepo: true } },
    });
    open({ ...ws("shop", null), repos: ["api", "web"] });
    await preset("Fleet");
    const box = screen.getByRole("checkbox", { name: /Give each agent its own worktree/ });
    expect(box.getAttribute("data-disabled")).toBeNull();
    expect(screen.queryByText("Worktrees across several repositories come with Pro. On Free, agents can still work in this folder directly, and adding one repository on its own gives it worktrees.")).toBeNull();
  });

  it("UI-12 outside a git repository the worktree box is off and says why", async () => {
    open(ws("notes", null));
    await preset("Fleet");
    const box = screen.getByRole("checkbox", { name: /Give each agent its own worktree/ });
    expect(box.getAttribute("data-disabled")).not.toBeNull();
    expect(box.getAttribute("aria-checked")).toBe("false");
    expect(screen.getByText("Worktrees need a git repository")).toBeTruthy();
  });

  it("F-CD-14.1 Launch sends the lineup with the task and isolation; Cancel creates nothing", async () => {
    const report: LaunchReport = { launched: [], failed: [], sharedTabId: null };
    open(ws("shop", "main"), { launch_lineup: () => report });
    await preset("Fleet");
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(useLaunch.getState().workspaceId).toBeNull());
    expect(named("launch_lineup")).toHaveLength(0);

    useLaunch.getState().open("shop");
    await preset("Fleet");
    fireEvent.change(screen.getByTestId("launch-task"), { target: { value: "fix flaky tests" } });
    fireEvent.click(go());
    await waitFor(() => expect(named("launch_lineup")).toHaveLength(1));
    const args = named("launch_lineup")[0]?.args as {
      workspaceId: string;
      seats: Seat[];
      task: string;
      isolation: string;
    };
    expect(args.workspaceId).toBe("shop");
    expect(args.task).toBe("fix flaky tests");
    expect(args.isolation).toBe("worktree");
    expect(args.seats.map((s) => [s.engineId, s.role])).toEqual([
      ["claude", "Builder"],
      ["claude", "Builder"],
      ["claude", "Tester"],
      ["claude", "Reviewer"],
    ]);
    await waitFor(() => expect(useLaunch.getState().workspaceId).toBeNull());
  });

  it("F-CD-14.3 a seat that failed is listed with Retry, and Retry launches only that seat", async () => {
    const lane: IpcError = {
      code: "conflict",
      message: "Branch tandem/fix-auth-3 already exists",
      stage: "git",
      hint: null,
      retryable: false,
    };
    const failedSeat: Seat = { ...agentSeat("claude"), count: 1 };
    const report: LaunchReport = {
      launched: [],
      failed: [{ seatIndex: 2, seat: failedSeat, error: lane }],
      sharedTabId: "tab_shared",
    };
    open(ws("shop", "main"), {
      launch_lineup: () => report,
      launch_retry_seat: () => ({
        seatIndex: 0,
        pane: { id: "pn_1", tabId: "tab_x", workspaceId: "shop", kind: "agent", createdAt: 1 },
        workspaceId: "shop",
        tabId: "tab_x",
        laneBranch: "tandem/fix-auth-4",
      }),
    });
    await preset("Fleet");
    fireEvent.click(go());
    const list = await screen.findByTestId("launch-failures");
    expect(within(list).getByText(/Seat 3/)).toBeTruthy();
    expect(within(list).getByText(/already exists/)).toBeTruthy();
    expect(screen.getByText("1 seat didn't launch")).toBeTruthy();
    fireEvent.click(within(list).getByRole("button", { name: "Retry" }));
    await waitFor(() => expect(named("launch_retry_seat")).toHaveLength(1));
    expect(named("launch_retry_seat")[0]?.args).toMatchObject({
      workspaceId: "shop",
      seat: failedSeat,
      isolation: "worktree",
      tabId: "tab_shared",
    });
    await waitFor(() => expect(useLaunch.getState().workspaceId).toBeNull());
  });

  it("UI-12 opened before engines are detected, the sheet loads them and then starts on the default engine", async () => {
    let resolve: (list: EngineView[]) => void = () => undefined;
    const list = new Promise<EngineView[]>((r) => (resolve = r));
    open(ws("shop", "main"), { engines_list: () => list, engines_report: () => null }, false);
    expect(await screen.findByRole("status", { name: "Loading launch options" })).toBeTruthy();
    expect(screen.queryByText(/No coding agents are installed/)).toBeNull();
    resolve([engine("codex", "Codex"), engine("claude", "Claude Code")]);
    await waitFor(() => expect(go().textContent).toContain("Launch 1 agent"));
    expect(screen.queryByText(/No coding agents are installed/)).toBeNull();
    fireEvent.click(go());
    await waitFor(() => expect(named("launch_lineup")).toHaveLength(1));
    expect((named("launch_lineup")[0]?.args as { seats: Seat[] }).seats[0]?.engineId).toBe("claude");
  });

  it("F-CD-14.4 Save as preset stores the current lineup under a name", async () => {
    open(ws("shop", "main"), {
      preset_save: (a) => ({
        id: "pst_01",
        name: a?.name,
        seats: a?.seats,
        isolation: a?.isolation,
        builtin: false,
        createdAt: 1,
      }),
    });
    await preset("Bench");
    fireEvent.click(screen.getByTestId("launch-save"));
    fireEvent.change(await screen.findByTestId("preset-name"), { target: { value: "Nightly" } });
    fireEvent.click(screen.getByTestId("preset-save"));
    await waitFor(() => expect(named("preset_save")).toHaveLength(1));
    expect(named("preset_save")[0]?.args).toMatchObject({ id: null, name: "Nightly", isolation: "shared" });
    expect((named("preset_save")[0]?.args as { seats: Seat[] }).seats.map((s) => s.kind)).toEqual([
      "agent",
      "shell",
    ]);
  });
});
