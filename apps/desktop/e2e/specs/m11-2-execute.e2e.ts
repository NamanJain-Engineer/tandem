import { $, browser } from "@wdio/globals";
import { execFileSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { addFolder, freshUser, home, invoke, mod, waitFor } from "../support/app";
import { expectNoSeriousA11yViolations } from "../support/axe";

const GIT_ENV = {
  ...process.env,
  GIT_AUTHOR_NAME: "E2E",
  GIT_AUTHOR_EMAIL: "e2e@vibezeo.test",
  GIT_COMMITTER_NAME: "E2E",
  GIT_COMMITTER_EMAIL: "e2e@vibezeo.test",
};
const git = (cwd: string, ...args: string[]) =>
  execFileSync("git", ["-C", cwd, ...args], { encoding: "utf8", env: GIT_ENV }).trim();

type Attempt = {
  id: string;
  issueId: string;
  state: string;
  laneWorkspaceId: string | null;
  threadId: string | null;
  reviewVerdict: string | null;
};
type AttemptView = { attempt: Attempt; queuePosition: number | null };
type Ws = { id: string; root: string; parentId: string | null };

const attempts = () => invoke<AttemptView[]>("task_attempts_list");
const workspaces = () => invoke<Ws[]>("workspace_list");

async function newIssue(title: string): Promise<string> {
  return (await invoke<{ id: string }>("task_save", { input: { title } })).id;
}

async function execute(issueId: string, workspaceId: string): Promise<AttemptView> {
  return invoke<AttemptView>("task_execute", {
    input: {
      issueId,
      workspaceId,
      engineId: "claude",
      isolation: "lane",
      access: "edits",
      fullConfirmed: false,
      review: true,
    },
  });
}

async function attemptOf(id: string): Promise<Attempt> {
  const found = (await attempts()).find((a) => a.attempt.id === id);
  if (!found) throw new Error(`no attempt ${id}`);
  return found.attempt;
}

async function untilState(id: string, states: string[], timeout = 60_000): Promise<Attempt> {
  return waitFor(
    () => attemptOf(id),
    (a) => states.includes(a.state),
    `attempt never reached ${states.join("/")}`,
    {
      timeout,
      interval: 250,
    },
  );
}

/** Chat-engine spawns so far (Claude in stream-json mode). */
async function chatSpawns(): Promise<number> {
  const log = await invoke<Array<{ argv: string[] }>>("test_spawn_log");
  return log.filter((r) => r.argv.includes("--input-format")).length;
}

// E2E-M11-2 Execute (F-TK-04; free, D-2902): a run makes a lane and a session, the
// agent's progress comments show, the task goes to In review with a result
// and the review posts a verdict; a 5th run queues; Stop during lane
// creation leaves nothing behind; assigning spawns nothing.
describe("E2E-M11-2 execute", () => {
  let ws = "";
  const repo = join(home(), "m11-repo");

  before(async function () {
    this.timeout(180_000);
    await freshUser("m11-2");
    mkdirSync(repo, { recursive: true });
    git(repo, "init", "-q", "-b", "main");
    writeFileSync(join(repo, "README.md"), "hello\n");
    git(repo, "add", "README.md");
    git(repo, "commit", "-q", "-m", "first");
    await addFolder(repo);
    ws = (await workspaces()).find((w) => w.root === repo)!.id;
  });

  it("F-TK-04.1 assigning an issue to a teammate spawns nothing", async function () {
    this.timeout(30_000);
    const before = await chatSpawns();
    const id = await newIssue("Assigned only");
    const lead = (await invoke<Array<{ teammate: { id: string; isLead: boolean } }>>("teammate_list")).find(
      (s) => s.teammate.isLead,
    )!;
    await invoke("tasks_meta");
    await invoke("task_save", { input: { id, assignee: lead.teammate.id, expected_revision: 1 } });
    await browser.pause(1_500);
    expect(await chatSpawns()).toBe(before);
    expect((await attempts()).filter((a) => a.attempt.issueId === id)).toEqual([]);
  });

  it("F-TK-04.4 Execute: a lane, progress comments, In review with a result, then a review verdict", async function () {
    this.timeout(120_000);
    const id = await newIssue("Fix the checkout loop");
    // Through the UI: the Tasks page, the task, Execute…, Start run.
    await browser.keys([mod(), "1"]);
    await $('[data-testid="agent-footer-tasks"]').click();
    await $(`[data-testid="task-row"][data-id="${id}"] button`).click();
    await $('[data-testid="task-execute"]').click();
    await $('[data-testid="task-execute-dialog"]').waitForDisplayed({ timeout: 10_000 });
    await expectNoSeriousA11yViolations();
    await $('[data-testid="task-execute-start"]').click();
    const run = await browser.waitUntil(
      async () => (await attempts()).find((a) => a.attempt.issueId === id),
      {
        timeout: 10_000,
        timeoutMsg: "no run started",
      },
    );
    const done = await untilState(run.attempt.id, ["in_review"], 90_000);
    expect(done.laneWorkspaceId).toBeTruthy();
    const lane = (await workspaces()).find((w) => w.id === done.laneWorkspaceId);
    expect(lane?.root).toContain(join(".tandem", "worktrees"));
    const detail = await invoke<{
      issue: { status: string; result: string | null };
      comments: Array<{ body: string }>;
    }>("task_get", { id });
    expect(detail.comments.some((c) => c.body.startsWith("Progress:"))).toBe(true);
    expect(detail.issue.status).toBe("in_review");
    expect(detail.issue.result).toContain(`Fixed ${id}`);
    expect(done.reviewVerdict).toBe("approve");
    expect(detail.comments.some((c) => c.body.startsWith("Verdict: Approve"))).toBe(true);
    // The page shows the run and its verdict.
    await browser.waitUntil(
      async () =>
        (
          await browser.execute(() => document.querySelector('[data-testid="task-runs"]')?.textContent ?? "")
        ).includes("Verdict: Approve"),
      { timeout: 10_000, timeoutMsg: "the verdict never showed" },
    );
  });

  it("F-TK-04.2 a 5th attempt says Queued (position 1) and starts when one of the 4 finishes", async function () {
    this.timeout(180_000);
    const ids: string[] = [];
    for (let i = 0; i < 4; i++)
      ids.push((await execute(await newIssue(`Hold ${i} [hold 8]`), ws)).attempt.id);
    const fifth = await execute(await newIssue("Fifth [hold 1]"), ws);
    expect(fifth.attempt.state).toBe("queued");
    expect(fifth.queuePosition).toBe(1);
    // While the four run, it waits.
    await browser.pause(2_000);
    expect((await attemptOf(fifth.attempt.id)).state).toBe("queued");
    await untilState(fifth.attempt.id, ["setup", "running", "reviewing", "in_review"], 120_000);
    const states = await Promise.all(ids.map(async (i) => (await attemptOf(i)).state));
    expect(states.some((s) => s === "in_review" || s === "reviewing")).toBe(true);
    await untilState(fifth.attempt.id, ["in_review"], 120_000);
  });

  it("F-TK-04.3 Stop during lane creation leaves no process and no lane", async function () {
    this.timeout(60_000);
    const lanesBefore = (await workspaces()).filter((w) => w.parentId).length;
    const spawnsBefore = await chatSpawns();
    await invoke("test_task_lane_delay", { ms: 3_000 });
    const run = await execute(await newIssue("Stopped early"), ws);
    await untilState(run.attempt.id, ["setup"], 10_000);
    const stopped = await invoke<AttemptView>("task_attempt_stop", { attemptId: run.attempt.id });
    expect(stopped.attempt.state).toBe("stopped");
    // Past the delay and the lane's own creation: rolled back.
    await browser.pause(6_000);
    await invoke("test_task_lane_delay", { ms: 0 });
    expect((await workspaces()).filter((w) => w.parentId).length).toBe(lanesBefore);
    expect(await chatSpawns()).toBe(spawnsBefore);
    const a = await attemptOf(run.attempt.id);
    expect(a.state).toBe("stopped");
    expect(a.threadId).toBeNull();
    expect(
      git(repo, "worktree", "list")
        .split("\n")
        .filter((l) => l.includes("stopped-early")),
    ).toEqual([]);
  });
});
