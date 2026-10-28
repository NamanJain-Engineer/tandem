import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AttemptState, AttemptView } from "@tandem/protocol";
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
