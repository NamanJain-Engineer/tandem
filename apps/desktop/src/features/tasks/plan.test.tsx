import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type {
  AttemptState,
  AttemptView,
  PlanLaunch,
  PlanQuestion,
  PlanView,
  TasksMeta,
} from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { emitLocal } from "../../ipc/events";
import { setInvokeBackend } from "../../ipc/invoke";
import { followPlans, usePlans } from "../../stores/plan";
import { useSession } from "../../stores/session";
import { useTasks } from "../../stores/tasks";
import { useWorkspaces } from "../../stores/workspaces";
import { useWorktrees } from "../../stores/worktrees";
import { FREE_VIEW, SIGNED_IN, fakeBackend, resetStores, type Call } from "../../test/fakeBackend";
import { TasksPage } from "./TasksPanel";
import { engineOption } from "./PlanWithLead";

const META: TasksMeta = {
  version: 1,
  next_number: 1,
  projects: [],
  labels: [],
  statuses: ["backlog", "todo", "in_progress", "in_review", "done", "canceled"],
  actors: [{ id: "me", kind: "human", name: "You" }],
  teams: [{ id: "local", name: "Local" }],
};

const ENGINES: PlanView["engines"] = [
  {
    engineId: "claude",
    label: "Claude Code",
    accounts: [
      { accountId: "ac_1", label: "Work", isDefault: true, window: "five_hour", usedPct: 62, resetsAt: null },
      {
        accountId: "ac_2",
        label: "Home",
        isDefault: false,
        window: "five_hour",
        usedPct: 10.4,
        resetsAt: null,
      },
    ],
  },
  { engineId: "codex", label: "Codex", accounts: [] },
];

function plan(over: Partial<PlanView> = {}): PlanView {
  return {
    id: "pl_1",
    goal: "Add OAuth sign-in\nwith Google",
    workspaceId: "ws_1",
    threadId: "th_1",
    state: "planning",
    tasks: [],
    engines: ENGINES,
    leadId: "tm_lead",
    leadEngine: "claude",
    error: null,
    waiting: false,
    projectId: null,
    issueIds: [],
    queued: [],
    skipped: [],
    supervise: false,
    baseCommit: null,
    questions: [],
    createdAt: 1,
    ...over,
  };
}

const READY = plan({
  state: "ready",
  tasks: [
    {
      title: "Add the routes",
      description: "Done when /auth works.",
      engineId: "codex",
      why: "Codex has room",
      fellBack: false,
    },
    { title: "Sign-in page", description: "", engineId: "claude", fellBack: true },
  ],
});

const CREATED = plan({
  state: "created",
  tasks: READY.tasks,
  projectId: "prj_1",
  issueIds: ["TAN-1", "TAN-2"],
});

/** Execute all's check: TAN-2 already runs from its own Execute…. */
const LAUNCH: PlanLaunch = {
  blocked: null,
  runs: [
    {
      issueId: "TAN-1",
      title: "Add the routes",
      engineId: "codex",
      engineLabel: "Codex",
      accountId: "ac_3",
      accountLabel: "Work",
      reviewEngineId: "claude",
      reviewEngineLabel: "Claude Code",
      skip: null,
    },
    {
      issueId: "TAN-2",
      title: "Sign-in page",
      engineId: "claude",
      engineLabel: "Claude Code",
      accountId: null,
      accountLabel: null,
      reviewEngineId: "codex",
      reviewEngineLabel: "Codex",
      skip: "running",
    },
  ],
};

let calls: Call[] = [];

function backend(extra: Record<string, (a?: Record<string, unknown>) => unknown> = {}) {
  calls = fakeBackend({
    tasks_meta: () => META,
    tasks_list: () => ({ items: [], next_cursor: null }),
    teammate_list: () => [],
    plan_list: () => [],
    plan_start: () => plan(),
    plan_create_tasks: (a) =>
      plan({
        state: "created",
        tasks: a?.tasks as PlanView["tasks"],
        projectId: "prj_1",
        issueIds: ["TAN-1", "TAN-2"],
      }),
    plan_prepare: () => LAUNCH,
    plan_execute: () =>
      plan({
        state: "started",
        tasks: READY.tasks,
        issueIds: ["TAN-1", "TAN-2"],
        queued: ["TAN-1"],
        skipped: [{ issueId: "TAN-2", reason: "running" }],
      }),
    plan_discard: () => null,
    ...extra,
  });
}

/** No account: runs are on for everyone (D-869, D-2902). */
function noAccount() {
  useSession.setState({
    auth: { ...SIGNED_IN, signedIn: false, email: null, offline: true, hosted: false },
    entitlement: { ...FREE_VIEW, limits: { ...FREE_VIEW.limits, taskExecution: true } },
  });
}

const wrap = () =>
  render(
    <UiProvider>
      <TasksPage />
    </UiProvider>,
  );

beforeEach(() => {
  resetStores();
  usePlans.setState({ plans: {}, openId: null });
  useWorkspaces.setState({
    list: [
      { id: "ws_1", name: "shop", kind: "local", parentId: null, missing: false },
      { id: "ws_2", name: "box", kind: "ssh", parentId: null, missing: false },
    ] as never,
  });
  noAccount();
  backend();
});
afterEach(() => setInvokeBackend(null));

describe("Plan with Lead (F-TK-06)", () => {
  it("F-TK-06.1 the goal goes to Lead in a local workspace; Lead plans in its thread", async () => {
    const stop = followPlans();
    wrap();
    fireEvent.click(await screen.findByTestId("plan-with-lead"));
    expect((screen.getByTestId("plan-start") as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByText(/workspaces on SSH hosts and in WSL aren't listed/)).toBeTruthy();
    fireEvent.change(screen.getByTestId("plan-goal"), {
      target: { value: "Add OAuth sign-in\nwith Google" },
    });
    fireEvent.click(screen.getByTestId("plan-start"));
    await screen.findByTestId("plan-planning");
    expect(calls.find((c) => c.cmd === "plan_start")?.args).toEqual({
      workspaceId: "ws_1",
      goal: "Add OAuth sign-in\nwith Google",
    });
    expect(screen.getByTestId("plan-planning").textContent).toBe("Lead is planning in its thread");
    expect(screen.getByTestId("plan-open-thread")).toBeTruthy();
    // Closed while planning, the plan stays one click away in the header.
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    const chip = await screen.findByTestId("plan-reopen");
    expect(chip.textContent).toContain("Plan: Add OAuth sign-in");
    fireEvent.click(chip);
    await screen.findByTestId("plan-planning");
    stop();
  });

  it("F-TK-06.2 F-TK-06.3 the draft is editable, says where an engine fell back, and Create tasks sends the edits", async () => {
    const stop = followPlans();
    usePlans.setState({ plans: { pl_1: plan() }, openId: "pl_1" });
    wrap();
    await screen.findByTestId("plan-planning");
    act(() => emitLocal("tasks:plan", READY));
    const rows = await screen.findAllByTestId("plan-row");
    expect(rows).toHaveLength(2);
    expect(screen.getByText("Codex has room")).toBeTruthy();
    expect(screen.getByTestId("plan-row-fell-back").textContent).toBe(
      "Lead named an engine that can't run tasks here, so this uses Lead's own.",
    );
    const titles = screen.getAllByTestId("plan-row-title") as HTMLInputElement[];
    fireEvent.change(titles[0]!, { target: { value: "OAuth routes" } });
    // Nothing is written before Create tasks.
    expect(calls.some((c) => c.cmd === "plan_create_tasks")).toBe(false);
    fireEvent.click(screen.getAllByRole("button", { name: "Remove task" })[1]!);
    expect(screen.getAllByTestId("plan-row")).toHaveLength(1);
    fireEvent.click(screen.getByTestId("plan-create"));
    await waitFor(() => expect(calls.some((c) => c.cmd === "plan_create_tasks")).toBe(true));
    expect(calls.find((c) => c.cmd === "plan_create_tasks")?.args).toEqual({
      id: "pl_1",
      tasks: [
        {
          title: "OAuth routes",
          description: "Done when /auth works.",
          engineId: "codex",
          why: "Codex has room",
          fellBack: false,
        },
      ],
    });
    stop();
  });

  it("F-TK-06.4 Execute all… shows each run before any starts, with no account, and starts only on Run", async () => {
    usePlans.setState({ plans: { pl_1: CREATED }, openId: "pl_1" });
    wrap();
    expect((await screen.findByTestId("plan-created")).textContent).toBe("Created 2 tasks: TAN-1, TAN-2.");
    fireEvent.click(screen.getByTestId("plan-execute"));
    const runs = await screen.findAllByTestId("plan-launch-run");
    expect(calls.find((c) => c.cmd === "plan_prepare")?.args).toEqual({ id: "pl_1" });
    expect(runs.map((r) => r.textContent)).toEqual([
      "TAN-1 · Add the routesCodex · WorkEdits in its own worktree · reviewed by Claude Code",
      "TAN-2 · Sign-in pageClaude Code · its own sign-inLeft alone: already running",
    ]);
    expect(runs.map((r) => r.getAttribute("data-skip"))).toEqual(["", "running"]);
    expect(calls.some((c) => c.cmd === "plan_execute")).toBe(false);
    // Back: nothing started.
    fireEvent.click(screen.getByTestId("plan-confirm-back"));
    await screen.findByTestId("plan-created");
    expect(calls.some((c) => c.cmd === "plan_execute")).toBe(false);
    fireEvent.click(screen.getByTestId("plan-execute"));
    const run = await screen.findByTestId("plan-confirm-run");
    expect(run.textContent).toBe("Run 1 task");
    fireEvent.click(run);
    await waitFor(() =>
      expect(calls.find((c) => c.cmd === "plan_execute")?.args).toEqual({ id: "pl_1", supervise: true }),
    );
    await waitFor(() =>
      expect(screen.getByTestId("plan-created").textContent).toBe(
        "Queued 1 task. Each runs in its own worktree; finished work shows in the review queue.",
      ),
    );
    expect(screen.getByTestId("plan-skipped").textContent).toBe("Left alone: TAN-2 (already running).");
    expect(document.body.textContent).not.toMatch(/\bPro\b|Upgrade/);
  });

  it("F-TK-06.4 when the limits turn runs off, the check says why where Run would be", async () => {
    backend({
      plan_prepare: () => ({
        ...LAUNCH,
        blocked: "Running tasks is turned off for this account. You can still track tasks.",
      }),
    });
    usePlans.setState({ plans: { pl_1: CREATED }, openId: "pl_1" });
    wrap();
    fireEvent.click(await screen.findByTestId("plan-execute"));
    expect((await screen.findByTestId("plan-execute-blocked")).textContent).toBe(
      "Running tasks is turned off for this account. You can still track tasks. Your tasks are saved; run them later or one at a time.",
    );
    expect(screen.queryByTestId("plan-confirm-run")).toBeNull();
    expect(document.body.textContent).not.toMatch(/\bPro\b|Upgrade/);
  });

  it("F-TK-06.4 a plan that's starting its runs offers no second start, even reopened", async () => {
    usePlans.setState({ plans: { pl_1: plan({ ...CREATED, state: "starting" }) }, openId: "pl_1" });
    wrap();
    expect((await screen.findByTestId("plan-starting")).textContent).toBe("Starting the runs…");
    expect(screen.queryByTestId("plan-execute")).toBeNull();
    fireEvent.click(screen.getByTestId("plan-close"));
    fireEvent.click(await screen.findByTestId("plan-reopen"));
    await screen.findByTestId("plan-starting");
    expect(screen.queryByTestId("plan-execute")).toBeNull();
  });

  it("F-TK-06.2 Lead waiting for your answer: the plan says so", async () => {
    usePlans.setState({ plans: { pl_1: plan({ waiting: true }) }, openId: "pl_1" });
    wrap();
    expect((await screen.findByTestId("plan-planning")).textContent).toBe(
      "Lead is waiting for your answer in its thread",
    );
    expect(screen.getByTestId("plan-open-thread")).toBeTruthy();
  });

  it("F-TK-06.3 rows Create tasks wrote before it stopped stay as they are", async () => {
    usePlans.setState({
      plans: { pl_1: { ...READY, projectId: "prj_1", issueIds: ["TAN-1"], error: "The disk is full" } },
      openId: "pl_1",
    });
    wrap();
    await screen.findAllByTestId("plan-row");
    expect(screen.getByTestId("plan-row-created").textContent).toBe("Created as TAN-1");
    const titles = screen.getAllByTestId("plan-row-title") as HTMLInputElement[];
    expect(titles.map((x) => x.disabled)).toEqual([true, false]);
    expect(screen.getAllByRole("button", { name: "Remove task" })).toHaveLength(1);
    expect(screen.getByTestId("plan-error").textContent).toBe("The disk is full");
    expect(screen.getByTestId("plan-create").textContent).toBe("Create 1 task");
  });

  it("F-TK-06 the app's plans come back after a reload, and Remove plan leaves the tasks", async () => {
    backend({ plan_list: () => [CREATED] });
    const stop = followPlans();
    wrap();
    fireEvent.click(await screen.findByTestId("plan-reopen"));
    await screen.findByTestId("plan-created");
    fireEvent.click(screen.getByTestId("plan-remove"));
    await waitFor(() => expect(calls.find((c) => c.cmd === "plan_discard")?.args).toEqual({ id: "pl_1" }));
    await waitFor(() => expect(screen.queryByTestId("plan-reopen")).toBeNull());
    expect(calls.some((c) => c.cmd.startsWith("task_") && c.cmd !== "task_attempts_list")).toBe(false);
    stop();
  });

  it("F-TK-06.2 a failed plan says why and offers Plan again with the same goal", async () => {
    backend({ plan_start: () => plan({ id: "pl_2" }) });
    usePlans.setState({
      plans: { pl_1: plan({ state: "failed", error: "Lead's reply had no plan" }) },
      openId: "pl_1",
    });
    wrap();
    expect((await screen.findByTestId("plan-failed")).textContent).toBe("Lead's reply had no plan");
    fireEvent.click(screen.getByTestId("plan-again"));
    await waitFor(() =>
      expect(calls.find((c) => c.cmd === "plan_start")?.args).toEqual({
        workspaceId: "ws_1",
        goal: "Add OAuth sign-in\nwith Google",
      }),
    );
    expect(calls.find((c) => c.cmd === "plan_discard")?.args).toEqual({ id: "pl_1" });
  });

  it("F-TK-06 the engine menu shows each engine's account with the most room", () => {
    expect(engineOption(ENGINES[0]!).label).toBe("Claude Code · 10 % used");
    expect(engineOption(ENGINES[1]!).label).toBe("Codex");
  });
});

/** A run of `issue` queued at `at` (F-TK-04's attempt). */
function run(
  issue: string,
  state: AttemptState,
  at: number,
  over: Partial<AttemptView["attempt"]> = {},
): AttemptView {
  return {
    attempt: {
      id: `atm_${issue}_${at}`,
      issueId: issue,
      workspaceId: "ws_1",
      laneWorkspaceId: `wt_${issue}`,
      threadId: null,
      teammateId: null,
      engineId: "codex",
      accountId: null,
      access: "edits",
      review: true,
      state,
      queuedAt: at,
      startedAt: null,
      endedAt: null,
      reviewVerdict: null,
      error: null,
      ...over,
    },
    queuePosition: state === "queued" ? 1 : null,
    waiting: false,
  };
}

const THREE: PlanView["tasks"] = [
  { title: "Routes", description: "", engineId: "codex", fellBack: false },
  { title: "Page", description: "", engineId: "claude", fellBack: false },
  { title: "Docs", description: "", engineId: "claude", fellBack: false },
];

const states = () => screen.getAllByTestId("plan-run").map((r) => r.dataset.state);

describe("Follow a plan through (F-TK-07)", () => {
  it("F-TK-07.2 each task shows its latest run, its verdict and a count, and follows the runs", async () => {
    let runs: AttemptView[] = [
      run("TAN-1", "failed", 1),
      run("TAN-1", "in_review", 2, { reviewVerdict: "approve" }),
      run("TAN-2", "running", 3),
    ];
    backend({ task_attempts_list: () => runs });
    usePlans.setState({
      plans: { pl_1: plan({ state: "started", tasks: THREE, issueIds: ["TAN-1", "TAN-2", "TAN-3"] }) },
      openId: "pl_1",
    });
    wrap();
    await waitFor(() => expect(states()).toEqual(["in_review", "running", "none"]));
    const rows = screen.getAllByTestId("plan-run");
    expect(rows[0]?.textContent).toContain("Routes");
    expect(rows[0]?.textContent).toContain("Ready to review · Approved");
    expect(rows[1]?.textContent).toContain("Running");
    expect(rows[2]?.textContent).toContain("Not started");
    expect(screen.getByTestId("plan-summary").textContent).toBe("1 running · 1 approved · 1 not started");
    // A run moves on: the plan follows it.
    runs = [...runs, run("TAN-2", "in_review", 4, { reviewVerdict: "changes_requested" })];
    act(() => emitLocal("tasks:attempt", runs[3]));
    await waitFor(() => expect(states()).toEqual(["in_review", "in_review", "none"]));
    expect(screen.getAllByTestId("plan-run")[1]?.textContent).toContain("Changes requested");
    expect(screen.getByTestId("plan-summary").textContent).toBe(
      "1 approved · 1 changes requested · 1 not started",
    );
  });

  it("F-TK-07.2 a run's row opens its task", async () => {
    backend({
      task_attempts_list: () => [run("TAN-2", "running", 3)],
      task_get: () => ({
        issue: {
          id: "TAN-2",
          number: 2,
          revision: 1,
          title: "Page",
          description: "",
          status: "in_progress",
          priority: 0,
          labels: [],
          project: "prj_1",
          assignee: null,
          created_at: 1,
          updated_at: 1,
          archived: false,
          result: null,
          attempts: [],
        },
        comments: [],
        history: [],
      }),
    });
    usePlans.setState({
      plans: { pl_1: plan({ state: "started", tasks: THREE, issueIds: ["TAN-1", "TAN-2", "TAN-3"] }) },
      openId: "pl_1",
    });
    wrap();
    await waitFor(() => expect(states()).toEqual(["none", "running", "none"]));
    fireEvent.click(screen.getAllByTestId("plan-run-open")[1]!);
    await waitFor(() => expect(useTasks.getState().selectedId).toBe("TAN-2"));
    expect(usePlans.getState().openId).toBeNull();
    // The task opens.
    expect(((await screen.findByTestId("task-title")) as HTMLInputElement).value).toBe("Page");
  });

  it("F-TK-07.4 Land N approved opens Land worktrees with the approved worktrees in plan order", async () => {
    backend({
      task_attempts_list: () => [
        run("TAN-3", "in_review", 5, { reviewVerdict: "approve" }),
        run("TAN-1", "in_review", 6, { reviewVerdict: "approve" }),
        run("TAN-2", "in_review", 7, { reviewVerdict: "changes_requested" }),
      ],
    });
    usePlans.setState({
      plans: { pl_1: plan({ state: "started", tasks: THREE, issueIds: ["TAN-1", "TAN-2", "TAN-3"] }) },
      openId: "pl_1",
    });
    wrap();
    const land = await screen.findByTestId("plan-land");
    expect(land.textContent).toBe("Land 2 approved…");
    fireEvent.click(land);
    expect(useWorktrees.getState().ask).toEqual({
      kind: "land",
      workspaceId: "ws_1",
      laneIds: ["wt_TAN-1", "wt_TAN-3"],
    });
    expect(usePlans.getState().openId).toBeNull();
  });

  it("F-TK-07.4 without an approved run there's nothing to land", async () => {
    backend({ task_attempts_list: () => [run("TAN-1", "in_review", 2)] });
    usePlans.setState({
      plans: { pl_1: plan({ state: "started", tasks: THREE, issueIds: ["TAN-1", "TAN-2", "TAN-3"] }) },
      openId: "pl_1",
    });
    wrap();
    await waitFor(() => expect(states()).toEqual(["in_review", "none", "none"]));
    expect(screen.queryByTestId("plan-land")).toBeNull();
    expect(screen.getByTestId("plan-summary").textContent).toBe("1 ready to review · 2 not started");
  });

  it("F-TK-07.2 while the runs load the plan says so, and a failed read says why with Retry", async () => {
    let mode: "hold" | "fail" | "ok" = "hold";
    const held: Array<(v: AttemptView[]) => void> = [];
    backend({
      task_attempts_list: () => {
        if (mode === "hold") return new Promise<AttemptView[]>((r) => held.push(r));
        if (mode === "fail")
          throw { code: "io", message: "The disk is busy", stage: null, hint: null, retryable: true };
        return [run("TAN-1", "running", 1)];
      },
    });
    usePlans.setState({
      plans: { pl_1: plan({ state: "started", tasks: THREE, issueIds: ["TAN-1", "TAN-2", "TAN-3"] }) },
      openId: "pl_1",
    });
    wrap();
    // Unknown runs aren't "Not started".
    await screen.findByTestId("plan-runs-loading");
    expect(screen.queryByTestId("plan-run")).toBeNull();
    act(() => held.forEach((r) => r([])));
    await waitFor(() => expect(states()).toEqual(["none", "none", "none"]));
    // A read that fails says why, and Retry reads again.
    mode = "fail";
    await act(() => useTasks.getState().loadAttempts());
    expect((await screen.findByTestId("plan-runs-error")).textContent).toBe(
      "The runs couldn't be read: The disk is busy",
    );
    expect(screen.queryByTestId("plan-run")).toBeNull();
    mode = "ok";
    fireEvent.click(screen.getByTestId("plan-runs-retry"));
    await waitFor(() => expect(states()).toEqual(["running", "none", "none"]));
  });

  it("F-TK-07.2 a started plan's chip follows its runs: working, waiting on you, then done", async () => {
    backend({ task_attempts_list: () => [run("TAN-1", "running", 1)] });
    usePlans.setState({
      plans: { pl_1: plan({ state: "started", tasks: THREE, issueIds: ["TAN-1", "TAN-2", "TAN-3"] }) },
      openId: null,
    });
    wrap();
    const shape = () =>
      screen.getByTestId("plan-reopen").querySelector('[role="img"]')?.getAttribute("aria-label");
    await waitFor(() => expect(useTasks.getState().attemptsStatus).toBe("ready"));
    expect(shape()).toBe("Working");
    act(() => emitLocal("tasks:attempt", run("TAN-1", "in_review", 1, { reviewVerdict: "approve" })));
    await waitFor(() => expect(shape()).toBe("Waiting for you"));
    act(() => emitLocal("tasks:attempt", run("TAN-1", "done", 1)));
    await waitFor(() => expect(shape()).toBe("Done"));
  });

  it("F-TK-07.3 plans kept from before a restart are listed again, and Remove plan forgets one", async () => {
    const kept = plan({ state: "started", tasks: THREE, issueIds: ["TAN-1", "TAN-2", "TAN-3"] });
    backend({ plan_list: () => [kept], task_attempts_list: () => [] });
    const stop = followPlans();
    wrap();
    const chip = await screen.findByTestId("plan-reopen");
    expect(chip.textContent).toContain("Plan: Add OAuth sign-in");
    fireEvent.click(chip);
    fireEvent.click(await screen.findByTestId("plan-remove"));
    await waitFor(() => expect(calls.find((c) => c.cmd === "plan_discard")?.args).toEqual({ id: "pl_1" }));
    await waitFor(() => expect(screen.queryByTestId("plan-reopen")).toBeNull());
    stop();
  });
});

/** A run's question in the plan (F-TK-08). */
function asked(issue: string, attempt: string, over: Partial<PlanQuestion> = {}): PlanQuestion {
  return {
    id: `pq_${issue}_${attempt}`,
    issueId: issue,
    attemptId: attempt,
    question: "Vitest or Jest?",
    state: "answered",
    reason: null,
    answer: "Vitest.",
    askedAt: 1,
    ...over,
  };
}

const STARTED = plan({
  state: "started",
  tasks: THREE,
  issueIds: ["TAN-1", "TAN-2", "TAN-3"],
  queued: ["TAN-1", "TAN-2", "TAN-3"],
  supervise: true,
  baseCommit: "c0ffee12",
});

const row = (i: number) => screen.getAllByTestId("plan-run")[i]!;

describe("Lead keeps a plan moving (F-TK-08)", () => {
  it("F-TK-08 Execute all's check has Lead's answers on, says whose engine they use, and sends your choice", async () => {
    usePlans.setState({ plans: { pl_1: CREATED }, openId: "pl_1" });
    wrap();
    fireEvent.click(await screen.findByTestId("plan-execute"));
    const option = await screen.findByTestId("plan-supervise-option");
    expect(option.textContent).toContain(
      "Lead answers routine questions from the repository's instructions, using your engine and subscription",
    );
    expect(option.textContent).toContain("always come to you");
    const box = screen.getByRole("checkbox");
    expect(box.getAttribute("aria-checked")).toBe("true");
    fireEvent.click(box);
    expect(box.getAttribute("aria-checked")).toBe("false");
    fireEvent.click(screen.getByTestId("plan-confirm-run"));
    await waitFor(() =>
      expect(calls.find((c) => c.cmd === "plan_execute")?.args).toEqual({ id: "pl_1", supervise: false }),
    );
  });

  it("F-TK-08 a check with nothing to run doesn't offer Lead's answers", async () => {
    backend({
      plan_prepare: () => ({ ...LAUNCH, blocked: "Running tasks is turned off for this account." }),
    });
    usePlans.setState({ plans: { pl_1: CREATED }, openId: "pl_1" });
    wrap();
    fireEvent.click(await screen.findByTestId("plan-execute"));
    await screen.findByTestId("plan-execute-blocked");
    expect(screen.queryByTestId("plan-supervise-option")).toBeNull();
  });

  it("F-TK-08.2 each row counts Lead's answers and shows Lead answering", async () => {
    backend({
      task_attempts_list: () => [run("TAN-1", "running", 2), run("TAN-2", "running", 3)],
    });
    usePlans.setState({
      plans: {
        pl_1: {
          ...STARTED,
          questions: [
            asked("TAN-1", "atm_TAN-1_1"),
            asked("TAN-1", "atm_TAN-1_2", { id: "pq_2" }),
            asked("TAN-2", "atm_TAN-2_3", { state: "answering", answer: null }),
          ],
        },
      },
      openId: "pl_1",
    });
    wrap();
    await waitFor(() => expect(states()).toEqual(["running", "running", "none"]));
    expect(within(row(0)).getByTestId("plan-run-answered").textContent).toBe("Lead answered 2");
    expect(within(row(1)).queryByTestId("plan-run-answered")).toBeNull();
    expect(within(row(1)).getByTestId("plan-run-answering").textContent).toBe(
      "Lead is answering its question",
    );
    expect(row(1).dataset.question).toBe("answering");
    expect(screen.queryByTestId("plan-run-needs-you")).toBeNull();
  });

  it("F-TK-08.3 a question that came to you says why, and Answer… sends your answer to the run", async () => {
    const pushing = asked("TAN-2", "atm_TAN-2_3", {
      id: "pq_7",
      question: "Should I push the branch?",
      state: "escalated",
      reason: "It's about pushing, which always comes to you.",
      answer: null,
    });
    const withQuestion: PlanView = {
      ...STARTED,
      // An older run's question isn't the latest run's.
      questions: [asked("TAN-1", "atm_TAN-1_1", { state: "escalated", answer: null }), pushing],
    };
    backend({
      task_attempts_list: () => [run("TAN-1", "running", 2), run("TAN-2", "running", 3)],
      plan_answer: () => ({
        ...withQuestion,
        questions: [
          withQuestion.questions[0]!,
          { ...pushing, state: "answered_by_you", answer: "No, I'll push." },
        ],
      }),
    });
    usePlans.setState({ plans: { pl_1: withQuestion }, openId: "pl_1" });
    wrap();
    const box = await screen.findByTestId("plan-run-needs-you");
    expect(screen.getAllByTestId("plan-run-needs-you")).toHaveLength(1);
    expect(within(row(1)).getByTestId("plan-run-needs-you")).toBe(box);
    expect(within(box).getByTestId("plan-question").textContent).toBe("Should I push the branch?");
    expect(within(box).getByTestId("plan-question-why").textContent).toBe(
      "It's about pushing, which always comes to you.",
    );
    fireEvent.click(within(box).getByTestId("plan-answer-open"));
    const send = within(box).getByTestId("plan-answer-send") as HTMLButtonElement;
    expect(send.disabled).toBe(true);
    fireEvent.change(within(box).getByTestId("plan-answer-text"), { target: { value: "No, I'll push." } });
    fireEvent.click(send);
    await waitFor(() =>
      expect(calls.find((c) => c.cmd === "plan_answer")?.args).toEqual({
        id: "pl_1",
        questionId: "pq_7",
        text: "No, I'll push.",
      }),
    );
    await waitFor(() => expect(screen.queryByTestId("plan-run-needs-you")).toBeNull());
  });

  it("F-TK-08.4 a run waiting for an approval says so, and Lead doesn't answer it", async () => {
    backend({ task_attempts_list: () => [run("TAN-1", "running", 2)] });
    usePlans.setState({ plans: { pl_1: STARTED }, openId: "pl_1" });
    wrap();
    await waitFor(() => expect(states()).toEqual(["running", "none", "none"]));
    expect(within(row(0)).queryByTestId("plan-run-approval")).toBeNull();
    act(() => emitLocal("tasks:attempt", { ...run("TAN-1", "running", 2), waiting: true }));
    expect((await within(row(0)).findByTestId("plan-run-approval")).textContent).toBe(
      "Waiting for your approval",
    );
    expect(calls.some((c) => c.cmd === "plan_answer")).toBe(false);
  });

  it("F-TK-08.3 F-TK-08.4 a started plan's chip says it waits on you for an answer or an approval", async () => {
    backend({ task_attempts_list: () => [run("TAN-1", "running", 2)] });
    usePlans.setState({ plans: { pl_1: STARTED }, openId: null });
    wrap();
    const shape = () =>
      screen.getByTestId("plan-reopen").querySelector('[role="img"]')?.getAttribute("aria-label");
    await waitFor(() => expect(useTasks.getState().attemptsStatus).toBe("ready"));
    expect(shape()).toBe("Working");
    // Its question came to you.
    act(() =>
      usePlans
        .getState()
        .apply({
          ...STARTED,
          questions: [asked("TAN-1", "atm_TAN-1_2", { state: "escalated", answer: null })],
        }),
    );
    await waitFor(() => expect(shape()).toBe("Waiting for you"));
    // Answered, it works on; then it waits for an approval.
    act(() => usePlans.getState().apply({ ...STARTED, questions: [asked("TAN-1", "atm_TAN-1_2")] }));
    await waitFor(() => expect(shape()).toBe("Working"));
    act(() => emitLocal("tasks:attempt", { ...run("TAN-1", "running", 2), waiting: true }));
    await waitFor(() => expect(shape()).toBe("Waiting for you"));
  });

  it("F-TK-08.5 the started plan's switch turns Lead's answers off at once", async () => {
    backend({
      task_attempts_list: () => [],
      plan_supervise: (a) => ({ ...STARTED, supervise: a?.on as boolean }),
    });
    usePlans.setState({ plans: { pl_1: STARTED }, openId: "pl_1" });
    wrap();
    const panel = await screen.findByTestId("plan-supervise");
    expect(panel.textContent).toContain("Lead answers routine questions");
    expect(panel.textContent).toContain("using your engine and subscription");
    const toggle = within(panel).getByRole("switch");
    expect(toggle.getAttribute("aria-checked")).toBe("true");
    fireEvent.click(toggle);
    await waitFor(() =>
      expect(calls.find((c) => c.cmd === "plan_supervise")?.args).toEqual({ id: "pl_1", on: false }),
    );
    await waitFor(() => expect(toggle.getAttribute("aria-checked")).toBe("false"));
  });
});
