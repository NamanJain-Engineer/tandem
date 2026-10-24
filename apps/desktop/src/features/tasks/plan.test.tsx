import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { AttemptState, AttemptView, PlanView, TasksMeta } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { emitLocal } from "../../ipc/events";
import { setInvokeBackend } from "../../ipc/invoke";
import { followPlans, usePlans } from "../../stores/plan";
import { useSession } from "../../stores/session";
import { useTasks } from "../../stores/tasks";
import { useWorkspaces } from "../../stores/workspaces";
import { useWorktrees } from "../../stores/worktrees";
import { FREE, FREE_VIEW, SIGNED_IN, fakeBackend, resetStores, type Call } from "../../test/fakeBackend";
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
    projectId: null,
    issueIds: [],
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

let calls: Call[] = [];

function backend(extra: Record<string, (a?: Record<string, unknown>) => unknown> = {}) {
  calls = fakeBackend({
    tasks_meta: () => META,
    tasks_list: () => ({ items: [], next_cursor: null }),
    teammate_list: () => [],
    plan_start: () => plan(),
    plan_create_tasks: (a) =>
      plan({
        state: "created",
        tasks: a?.tasks as PlanView["tasks"],
        projectId: "prj_1",
        issueIds: ["TAN-1", "TAN-2"],
      }),
    plan_execute: () => plan({ state: "started", issueIds: ["TAN-1", "TAN-2"] }),
    plan_discard: () => null,
    ...extra,
  });
}

function signedIn(planName: "free" | "pro") {
  useSession.setState({
    auth: SIGNED_IN,
    entitlement: { ...FREE_VIEW, plan: planName, limits: { ...FREE, taskExecution: planName === "pro" } },
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
  signedIn("free");
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

  it("F-TK-06.4 Free: Execute all isn't offered; the plan says why and the tasks stay", async () => {
    usePlans.setState({
      plans: { pl_1: plan({ state: "created", tasks: READY.tasks, issueIds: ["TAN-1", "TAN-2"] }) },
      openId: "pl_1",
    });
    wrap();
    const created = await screen.findByTestId("plan-created");
    expect(created.textContent).toBe("Created 2 tasks: TAN-1, TAN-2.");
    expect(screen.queryByTestId("plan-execute")).toBeNull();
    expect(screen.getByTestId("plan-execute-blocked").textContent).toBe(
      "Running tasks is part of Pro. You can still track tasks for free. Your tasks are saved; run them later or one at a time.",
    );
    expect(calls.some((c) => c.cmd === "plan_execute")).toBe(false);
  });

  it("F-TK-06.4 Pro: Execute all queues the plan's tasks", async () => {
    signedIn("pro");
    usePlans.setState({
      plans: { pl_1: plan({ state: "created", tasks: READY.tasks, issueIds: ["TAN-1", "TAN-2"] }) },
      openId: "pl_1",
    });
    wrap();
    fireEvent.click(await screen.findByTestId("plan-execute"));
    await waitFor(() => expect(calls.find((c) => c.cmd === "plan_execute")?.args).toEqual({ id: "pl_1" }));
    await waitFor(() =>
      expect(screen.getByTestId("plan-created").textContent).toBe(
        "Queued 2 tasks. Each runs in its own worktree; finished work shows in the review queue.",
      ),
    );
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
      task_get: () => ({ issue: null, comments: [], history: [] }),
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
  });

  it("F-TK-07.4 Land N approved opens Land worktrees with the approved worktrees in plan order", async () => {
    signedIn("pro");
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
