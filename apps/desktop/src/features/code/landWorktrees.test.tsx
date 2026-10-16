import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type {
  DashboardItem,
  IpcError,
  LandingCandidate,
  LandingPlan,
  LandingRun,
  LandingStep,
  Tab,
  WorkspaceSummary,
} from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { setInvokeBackend } from "../../ipc/invoke";
import { useDashboard } from "../../stores/dashboard";
import { useWorkspaces } from "../../stores/workspaces";
import { useWorktrees } from "../../stores/worktrees";
import { fakeBackend, resetStores, type Call } from "../../test/fakeBackend";
import { WorktreeDialogs, worktreeMenu } from "./Worktrees";

const tab = (ws: string): Tab => ({
  id: `tb_${ws}`,
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
const A = ws("wt_a", { parentId: "shop", laneBranch: "tandem/a", name: "a" });

const candidate = (branch: string, over: Partial<LandingCandidate> = {}): LandingCandidate => ({
  laneId: `wt_${branch}`,
  branch: `tandem/${branch}`,
  ahead: 1,
  forecast: { kind: "clean" },
  uncommitted: 0,
  missing: false,
  ...over,
});

const PLAN: LandingPlan = {
  repoId: "shop",
  base: "main",
  baseDirty: false,
  check: "pnpm test",
  candidates: [
    candidate("a"),
    candidate("b", { forecast: { kind: "conflicts", files: ["x.ts"] } }),
    candidate("c", { forecast: { kind: "unavailable", reason: "old git" }, ahead: 3 }),
    candidate("dirty", { uncommitted: 2 }),
  ],
  undo: null,
  interrupted: null,
};

const step = (branch: string, state: LandingStep["state"], over: Partial<LandingStep> = {}): LandingStep => ({
  laneId: `wt_${branch}`,
  branch: `tandem/${branch}`,
  state,
  commit: null,
  exit: null,
  output: null,
  conflicts: [],
  prompt: null,
  ...over,
});

const runOf = (state: LandingRun["state"], steps: LandingStep[]): LandingRun => ({
  repoId: "shop",
  base: "main",
  before: "abc",
  check: "pnpm test",
  state,
  steps,
  error: null,
});

const agent = (paneId: string, workspaceId: string): DashboardItem => ({
  key: `pane:${paneId}`,
  source: { type: "pane", paneId },
  status: "working",
  reason: null,
  since: 0,
  title: null,
  humanName: "Ada",
  engineId: "claude",
  engineLabel: "Claude Code",
  engineLogo: null,
  accountLabel: null,
  workspaceId,
  workspace: null,
  branch: null,
  preview: null,
  pendingApproval: null,
});

let calls: Call[] = [];
const named = (cmd: string) => calls.filter((c) => c.cmd === cmd);
let plan: LandingPlan = PLAN;
let current: LandingRun | null = null;
let undoAnswer: IpcError | null = null;

function setup() {
  calls = fakeBackend({
    workspace_list: () => [REPO, A],
    merge_queue_plan: () => plan,
    merge_queue_get: () => current,
    merge_queue_start: (a) => {
      current = runOf(
        "running",
        ((a?.laneIds as string[]) ?? []).map((id) => step(id.slice(3), "waiting")),
      );
      return current;
    },
    merge_queue_stop: () => null,
    merge_queue_undo: () => {
      if (undoAnswer) throw undoAnswer;
      return null;
    },
    pty_type_prompt: () => null,
  });
  render(
    <UiProvider>
      <WorktreeDialogs />
    </UiProvider>,
  );
  act(() => useWorktrees.getState().open({ kind: "land", workspaceId: "shop" }));
}

beforeEach(() => {
  resetStores();
  plan = PLAN;
  current = null;
  undoAnswer = null;
  useDashboard.setState({ items: {} });
  useWorkspaces.setState({ list: [REPO, A], activeId: "shop", activeTab: {} });
});
afterEach(() => setInvokeBackend(null));

describe("Land worktrees (F-WT-07, UI-155)", () => {
  it("F-WT-07.1 the candidates show in landing order with their forecast, and Land lands the ticked ones in that order", async () => {
    setup();
    const rows = await screen.findAllByTestId("landing-candidate");
    expect(rows.map((r) => r.dataset.branch)).toEqual(["tandem/a", "tandem/b", "tandem/c", "tandem/dirty"]);
    expect(rows[0]?.textContent).toContain("1 commit · No conflicts expected");
    expect(rows[1]?.textContent).toContain("Conflicts in 1 file");
    expect(rows[2]?.textContent).toContain("3 commits · No forecast");
    expect((screen.getByTestId("landing-check") as HTMLInputElement).value).toBe("pnpm test");
    fireEvent.click(screen.getByTestId("landing-land"));
    await waitFor(() => expect(named("merge_queue_start")).toHaveLength(1));
    expect(named("merge_queue_start")[0]?.args).toEqual({
      workspaceId: "shop",
      laneIds: ["wt_a", "wt_b", "wt_c"],
      check: "pnpm test",
    });
    // The steps replace the list, and follow the run as it goes.
    expect(await screen.findAllByTestId("landing-step")).toHaveLength(3);
    current = runOf("done", [
      step("a", "landed", { commit: "1111111aaaa" }),
      step("b", "landed", { commit: "2222222bbbb" }),
      step("c", "landed", { commit: "3333333cccc" }),
    ]);
    plan = { ...PLAN, undo: { base: "main", landed: 3 } };
    await waitFor(() => expect(screen.getByTestId("landing-steps").dataset.state).toBe("done"), {
      timeout: 3000,
    });
    expect(screen.getByText("Landed 3 worktrees in main")).toBeTruthy();
    expect(screen.getAllByTestId("landing-step")[0]?.textContent).toContain("Landed 1111111");
    expect(await screen.findByTestId("landing-undo")).toBeTruthy();
  });

  it("F-WT-07.5 a worktree with uncommitted changes can't be ticked, and a dirty base can't land", async () => {
    setup();
    const rows = await screen.findAllByTestId("landing-candidate");
    const dirty = within(rows[3]!).getByRole("checkbox");
    expect(dirty.getAttribute("aria-checked")).toBe("false");
    expect(dirty).toHaveProperty("disabled", true);
    expect(rows[3]?.textContent).toContain("Commit or stash its changes first");
    expect(screen.getByTestId("landing-land").textContent).toBe("Land 3");
  });

  it("F-WT-07.5 the base's uncommitted changes disable Land and say why", async () => {
    plan = { ...PLAN, baseDirty: true };
    setup();
    expect(
      await screen.findByText("shop has uncommitted changes. Commit or stash them, then land."),
    ).toBeTruthy();
    expect(screen.getByTestId("landing-land")).toHaveProperty("disabled", true);
  });

  it("F-WT-07 a landing the app quit during says its merge was aborted", async () => {
    plan = { ...PLAN, interrupted: "shop" };
    setup();
    expect((await screen.findByTestId("landing-interrupted")).textContent).toBe(
      "The last landing was interrupted. Vibezeo aborted its unfinished merge in shop.",
    );
  });

  it("F-WT-07 empty: no worktree has commits the base doesn't", async () => {
    plan = { ...PLAN, candidates: [], check: null };
    setup();
    expect((await screen.findByTestId("landing-empty")).textContent).toBe(
      "No worktrees have commits that main doesn't.",
    );
    expect(screen.getByText("Without a check, worktrees land untested.")).toBeTruthy();
  });

  it("F-WT-07.2 a failed check shows its output, and Send failure to agent types the fenced prompt into the worktree's agent", async () => {
    current = runOf("stopped", [
      step("a", "landed", { commit: "1111111" }),
      step("b", "failed", { exit: 1, output: "FAIL a.test.ts", prompt: "PROMPT with ````text fence" }),
      step("c", "waiting"),
    ]);
    useDashboard.setState({ items: { "pane:pn_b": agent("pn_b", "wt_b") } });
    setup();
    const failed = (await screen.findAllByTestId("landing-step"))[1]!;
    expect(failed.dataset.state).toBe("failed");
    expect(failed.textContent).toContain("Check failed (exit 1)");
    expect(failed.textContent).toContain("FAIL a.test.ts");
    expect(named("pty_type_prompt")).toHaveLength(0);
    fireEvent.click(within(failed).getByTestId("landing-send"));
    await waitFor(() => expect(named("pty_type_prompt")).toHaveLength(1));
    expect(named("pty_type_prompt")[0]?.args).toEqual({ paneId: "pn_b", text: "PROMPT with ````text fence" });
  });

  it("F-WT-07.2 with no agent in the worktree, the failure can be copied instead", async () => {
    current = runOf("stopped", [step("b", "failed", { exit: 2, output: "boom", prompt: "P" })]);
    setup();
    expect(await screen.findByTestId("landing-copy")).toBeTruthy();
    expect(screen.queryByTestId("landing-send")).toBeNull();
  });

  it("F-WT-07.6 Stop asks the core to stop a running landing", async () => {
    current = runOf("running", [step("a", "checking"), step("b", "waiting")]);
    setup();
    fireEvent.click(await screen.findByTestId("landing-stop"));
    await waitFor(() => expect(named("merge_queue_stop")[0]?.args).toEqual({ workspaceId: "shop" }));
  });

  it("F-WT-07.4 Undo landing asks the core, which confirms natively; a refusal is shown, a Cancel says nothing", async () => {
    plan = { ...PLAN, undo: { base: "main", landed: 2 } };
    undoAnswer = {
      code: "conflict",
      message: "The landing was pushed to origin/main. Undo it with a revert instead.",
      stage: null,
      hint: null,
      retryable: false,
    };
    setup();
    fireEvent.click(await screen.findByTestId("landing-undo"));
    expect(
      await screen.findByText("The landing was pushed to origin/main. Undo it with a revert instead."),
    ).toBeTruthy();
    undoAnswer = {
      code: "cancelled",
      message: "Kept the landing",
      stage: null,
      hint: null,
      retryable: false,
    };
    fireEvent.click(screen.getByTestId("landing-undo"));
    await waitFor(() => expect(named("merge_queue_undo")).toHaveLength(2));
    expect(screen.queryByText("Kept the landing")).toBeNull();
    undoAnswer = null;
    fireEvent.click(screen.getByTestId("landing-undo"));
    await waitFor(() => expect(named("merge_queue_undo")).toHaveLength(3));
    expect(await screen.findByText("main is back where it was before the landing")).toBeTruthy();
  });

  it("F-WT-07 a worktree's menu has Land worktrees…, off on an SSH host", () => {
    const disabled = (w: WorkspaceSummary): boolean | undefined => {
      const e = worktreeMenu(w, "main", () => 0).find((x) => "id" in x && x.id === "wt-land");
      return e && "disabled" in e ? e.disabled : undefined;
    };
    expect(disabled(A)).toBe(false);
    expect(disabled({ ...A, kind: "ssh" } as WorkspaceSummary)).toBe(true);
  });
});
