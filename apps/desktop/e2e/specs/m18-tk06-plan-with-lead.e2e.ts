import { $, $$, browser } from "@wdio/globals";
import { execFileSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { addFolder, freshOffline, freshUser, home, invoke, mod, planFooter, setPlan } from "../support/app";

const GIT_ENV = {
  ...process.env,
  GIT_AUTHOR_NAME: "E2E",
  GIT_AUTHOR_EMAIL: "e2e@vibezeo.test",
  GIT_COMMITTER_NAME: "E2E",
  GIT_COMMITTER_EMAIL: "e2e@vibezeo.test",
};
const git = (cwd: string, ...args: string[]) =>
  execFileSync("git", ["-C", cwd, ...args], { encoding: "utf8", env: GIT_ENV }).trim();

type Ws = { id: string; root: string };
type Thread = { id: string; title: string; workspaceId: string | null; kind: string };
type Issue = { id: string; title: string; status: string; project: string | null; suggested_engine?: string };
type AttemptView = { attempt: { issueId: string; engineId: string; laneWorkspaceId: string | null } };

const GOAL = "Add OAuth sign-in with Google";

function repo(name: string): string {
  const dir = join(home(), name);
  mkdirSync(dir, { recursive: true });
  git(dir, "init", "-q", "-b", "main");
  writeFileSync(join(dir, "README.md"), "# shop\n");
  git(dir, "add", "README.md");
  git(dir, "commit", "-q", "-m", "first");
  return dir;
}

async function issues(): Promise<Issue[]> {
  const page = await invoke<{ items: Array<{ id: string }> }>("tasks_list", { filter: {}, cursor: null });
  return Promise.all(
    page.items.map(async (i) => (await invoke<{ issue: Issue }>("task_get", { id: i.id })).issue),
  );
}

/** Agent mode's Tasks page, Plan with Lead…, the goal, Start planning, and
 * Lead's draft. */
async function planToDraft(): Promise<void> {
  await browser.keys([mod(), "1"]);
  await $('[data-testid="agent-footer-tasks"]').click();
  await $('[data-testid="plan-with-lead"]').click();
  await $('[data-testid="plan-goal"]').setValue(GOAL);
  await $('[data-testid="plan-start"]').click();
  // A new plan: the goal form gives way to its dialog, then Lead's draft.
  await $('[data-testid="plan-goal-form"]').waitForExist({ reverse: true, timeout: 15_000 });
  await $('[data-testid="plan-dialog"][data-state="ready"]').waitForExist({ timeout: 60_000 });
}

// E2E-M18-TK-06 Plan with Lead (F-TK-06): the fake Claude is Lead; it
// answers the plan request with two tasks, the second on an engine that
// wasn't offered.
describe("E2E-M18-TK-06 plan with Lead", function () {
  this.timeout(300_000);

  it("F-TK-06.1-4 Free: Lead plans in its thread, the edited draft becomes tasks, and Execute all isn't offered", async () => {
    await freshOffline();
    const dir = repo("tk06-free");
    await addFolder(dir);
    const ws = (await invoke<Ws[]>("workspace_list")).find((w) => w.root === dir)!;
    await planToDraft();

    // Lead's chat is in the workspace, and its first message has the goal
    // and the markers.
    const chats = await invoke<Thread[]>("thread_list", { kind: "teammate_chat", includeArchived: false });
    const chat = chats.find((c) => c.title === `Plan: ${GOAL}`);
    expect(chat?.workspaceId).toBe(ws.id);
    const page = await invoke<{ items: Array<{ event: { type: string; text?: string } }> }>("thread_events", {
      threadId: chat!.id,
      limit: 50,
    });
    const first = page.items.map((e) => e.event).find((e) => e.type === "user")?.text ?? "";
    expect(first).toContain(GOAL);
    expect(first).toContain("Engines a task can run on");
    expect(first).toMatch(/PLAN-BEGIN-[0-9a-f]{8}/);

    // The draft: two rows; the second fell back to Lead's engine. Nothing
    // is in the tracker yet.
    expect(await $$('[data-testid="plan-row"]').length).toBe(2);
    expect(await $('[data-testid="plan-row-fell-back"]').isExisting()).toBe(true);
    expect(await issues()).toEqual([]);
    const title = $$('[data-testid="plan-row-title"]')[0]!;
    await title.clearValue();
    await title.setValue("OAuth routes");
    await $('[data-testid="plan-create"]').click();
    await $('[data-testid="plan-created"]').waitForExist({ timeout: 10_000 });

    const made = await issues();
    expect(made.map((i) => i.title).sort()).toEqual(["OAuth routes", "Write the sign-in page"]);
    expect(made.every((i) => i.status === "todo" && i.project && i.suggested_engine)).toBe(true);
    expect(new Set(made.map((i) => i.project)).size).toBe(1);

    // Signed out on Free: no Execute all; the plan says why where it would be.
    expect(await $('[data-testid="plan-execute"]').isExisting()).toBe(false);
    expect(String(await $('[data-testid="plan-execute-blocked"]').getProperty("textContent"))).toBe(
      "Sign in to run tasks. Your tasks stay available offline. Your tasks are saved; run them later or one at a time.",
    );
    expect(await invoke<AttemptView[]>("task_attempts_list")).toEqual([]);
    // Closed, the plan waits in the header.
    await $('[data-testid="plan-close"]').click();
    await $('[data-testid="plan-dialog"]').waitForExist({ reverse: true, timeout: 5_000 });
  });

  it("F-TK-06.4 Pro: Execute all starts one run per task on its suggested engine", async () => {
    const { email } = await freshUser("tk06");
    await setPlan(email, "pro");
    await browser.waitUntil(async () => (await planFooter()).startsWith("PRO"), { timeout: 20_000 });
    const dir = repo("tk06-pro");
    await addFolder(dir);
    await planToDraft();
    await $('[data-testid="plan-create"]').click();
    await $('[data-testid="plan-execute"]').waitForClickable({ timeout: 10_000 });
    await $('[data-testid="plan-execute"]').click();
    await $('[data-testid="plan-dialog"][data-state="started"]').waitForExist({ timeout: 15_000 });
    // This plan's tasks (the tracker keeps the Free run's too).
    const plan = (await invoke<Array<{ state: string; issueIds: string[] }>>("plan_list")).find(
      (p) => p.state === "started",
    )!;
    const made = (await issues()).filter((i) => plan.issueIds.includes(i.id));
    expect(made).toHaveLength(2);
    const runs = await browser.waitUntil(
      async () => {
        const all = (await invoke<AttemptView[]>("task_attempts_list")).filter((r) =>
          plan.issueIds.includes(r.attempt.issueId),
        );
        return all.length === made.length ? all : false;
      },
      { timeout: 15_000, timeoutMsg: "not every task got a run" },
    );
    for (const issue of made) {
      const run = runs.find((r) => r.attempt.issueId === issue.id);
      expect(run?.attempt.engineId).toBe(issue.suggested_engine);
    }
  });
});
