import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AttemptState, AttemptView, IssueSummary, TasksMeta } from "@tandem/protocol";
import { emitLocal, listen } from "../ipc/events";
import { setInvokeBackend } from "../ipc/invoke";
import { resetTasks, useTasks } from "./tasks";

// The real listener, which a test can hold back as Tauri's registration does
// (an import, then an IPC call).
vi.mock("../ipc/events", async (importOriginal) => {
  const real = await importOriginal<typeof import("../ipc/events")>();
  return { ...real, listen: vi.fn(real.listen) };
});

function run(issue: string, state: AttemptState, at: number): AttemptView {
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
      reviewVerdict: state === "in_review" ? "approve" : null,
      error: null,
    },
    queuePosition: null,
    waiting: false,
  };
}

const META: TasksMeta = {
  version: 1,
  next_number: 2,
  projects: [],
  labels: [],
  statuses: ["backlog", "todo", "in_progress", "in_review", "done", "canceled"],
  actors: [{ id: "me", kind: "human", name: "You" }],
  teams: [{ id: "local", name: "Local" }],
};

function issue(id: string, title: string): IssueSummary {
  return {
    id,
    number: 1,
    revision: 1,
    title,
    status: "todo",
    priority: 0,
    labels: [],
    project: null,
    assignee: null,
    updated_at: 1,
    archived: false,
  };
}

const states = () => useTasks.getState().attempts.map((a) => a.attempt.state);
const tick = () => new Promise((r) => setTimeout(r, 0));

beforeEach(() => resetTasks());
afterEach(() => setInvokeBackend(null));

describe("the runs' first read (F-TK-07)", () => {
  it("F-TK-07.2 the first read goes out once its listener is up, so a run that moves meanwhile isn't missed", async () => {
    const real = await vi.importActual<typeof import("../ipc/events")>("../ipc/events");
    let up!: () => void;
    const registered = new Promise<void>((ok) => (up = ok));
    vi.mocked(listen).mockImplementationOnce(async (event, handler) => {
      await registered;
      return real.listen(event, handler);
    });
    // The list as it stands when it's read.
    let runs = [run("TAN-1", "running", 1)];
    const calls: string[] = [];
    setInvokeBackend(async (cmd) => {
      calls.push(cmd);
      return cmd === "task_attempts_list" ? runs : null;
    });
    const loading = useTasks.getState().loadAttempts();
    await tick();
    expect(calls).toEqual([]);
    // The run reaches review while the listener is still registering: its
    // event reaches no one, so only a read made after it shows it.
    runs = [run("TAN-1", "in_review", 1)];
    emitLocal("tasks:attempt", runs[0]);
    up();
    await loading;
    expect(calls).toEqual(["task_attempts_list"]);
    expect(states()).toEqual(["in_review"]);
  });

  it("F-TK-07.2 a listener that can't start fails the read with its reason, and Retry listens again", async () => {
    vi.mocked(listen).mockRejectedValueOnce(new Error("no event bridge"));
    setInvokeBackend(async (cmd) => (cmd === "task_attempts_list" ? [run("TAN-1", "running", 1)] : null));
    await useTasks.getState().loadAttempts();
    expect(useTasks.getState().attemptsStatus).toBe("error");
    expect(useTasks.getState().attemptsError).toContain("no event bridge");
    await useTasks.getState().loadAttempts();
    expect(useTasks.getState().attemptsStatus).toBe("ready");
    expect(states()).toEqual(["running"]);
    // Listening again, it follows the run.
    emitLocal("tasks:attempt", run("TAN-1", "in_review", 1));
    expect(states()).toEqual(["in_review"]);
  });
});

describe("the tracker's first read (F-TK-01)", () => {
  it("F-TK-01 the first read goes out once its listener is up, so a change meanwhile shows", async () => {
    const real = await vi.importActual<typeof import("../ipc/events")>("../ipc/events");
    let up!: () => void;
    const registered = new Promise<void>((ok) => (up = ok));
    vi.mocked(listen).mockImplementationOnce(async (event, handler) => {
      await registered;
      return real.listen(event, handler);
    });
    // The tracker as it stands when it's read.
    let issues = [issue("TAN-1", "Add the routes")];
    const calls: string[] = [];
    setInvokeBackend(async (cmd) => {
      calls.push(cmd);
      if (cmd === "tasks_meta") return META;
      return cmd === "tasks_list" ? { items: issues, next_cursor: null } : null;
    });
    const loading = useTasks.getState().load();
    await tick();
    expect(calls).toEqual([]);
    // An agent renames the task while the listener is still registering:
    // its `tasks:changed` reaches no one, so only a read made after it shows it.
    issues = [issue("TAN-1", "Add the OAuth routes")];
    emitLocal("tasks:changed", { ids: ["TAN-1"] });
    up();
    await loading;
    expect(useTasks.getState().issues.map((i) => i.title)).toEqual(["Add the OAuth routes"]);
  });

  it("F-TK-01 an earlier read that settles last takes back neither the tracker nor its status", async () => {
    // Several views and every tasks:changed read the tracker; the latest
    // read's list is the one shown.
    // The first and third reads wait for the test to settle them.
    const settle: Array<(fail: boolean) => void> = [];
    let reads = 0;
    setInvokeBackend(async (cmd) => {
      if (cmd === "tasks_meta") return META;
      if (cmd !== "tasks_list") return null;
      reads += 1;
      if (reads === 1 || reads === 3) {
        const fail = await new Promise<boolean>((ok) => settle.push(ok));
        if (fail) throw new Error("the store was busy");
        return { items: [issue("TAN-1", "Add the routes")], next_cursor: null };
      }
      return { items: [issue("TAN-1", "Add the OAuth routes")], next_cursor: null };
    });
    const titles = () => useTasks.getState().issues.map((i) => i.title);
    const older = useTasks.getState().load();
    await tick();
    await useTasks.getState().load();
    expect(titles()).toEqual(["Add the OAuth routes"]);
    settle[0]!(false);
    await older;
    expect(titles()).toEqual(["Add the OAuth routes"]);
    // Nor does one that fails last.
    const failing = useTasks.getState().load();
    await tick();
    await useTasks.getState().load();
    settle[1]!(true);
    await failing;
    expect(useTasks.getState().status).toBe("ready");
    expect(useTasks.getState().error).toBeNull();
    expect(titles()).toEqual(["Add the OAuth routes"]);
  });

  it("F-TK-01 a listener that can't start fails the read with its reason, and Retry listens again", async () => {
    vi.mocked(listen).mockRejectedValueOnce(new Error("no event bridge"));
    let issues = [issue("TAN-1", "Add the routes")];
    setInvokeBackend(async (cmd) => {
      if (cmd === "tasks_meta") return META;
      return cmd === "tasks_list" ? { items: issues, next_cursor: null } : null;
    });
    await useTasks.getState().load();
    expect(useTasks.getState().status).toBe("error");
    expect(useTasks.getState().error).toContain("no event bridge");
    await useTasks.getState().load();
    expect(useTasks.getState().status).toBe("ready");
    // Listening again, a change reloads the tracker.
    issues = [issue("TAN-1", "Add the OAuth routes")];
    emitLocal("tasks:changed", { ids: ["TAN-1"] });
    await vi.waitFor(() =>
      expect(useTasks.getState().issues.map((i) => i.title)).toEqual(["Add the OAuth routes"]),
    );
  });
});
