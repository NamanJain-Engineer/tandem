import { $, $$, browser } from "@wdio/globals";
import { execFileSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { addFolder, freshOffline, home, invoke, mod } from "../support/app";

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
type Plan = {
  state: string;
  issueIds: string[];
  queued: string[];
  skipped: Array<{ issueId: string; reason: string }>;
};

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

/** Text a WebKitWebDriver `getText` would drop. */
async function text(selector: string): Promise<string> {
  return String(await $(selector).getProperty("textContent"));
}

// E2E-M18-TK-06 Plan with Lead (F-TK-06): the fake Claude is Lead; it
// answers the plan request with two tasks, the second on an engine that
// wasn't offered. With no account (D-869): runs need none.
describe("E2E-M18-TK-06 plan with Lead", function () {
  this.timeout(300_000);

  after(async () => {
    await invoke("test_task_lane_delay", { ms: 0 });
  });

  it("F-TK-06.1-4 Lead plans in its thread, the edited draft becomes tasks, and Execute all shows its runs, then starts each once", async () => {
    await freshOffline();
    const dir = repo("tk06");
    await addFolder(dir);
    const ws = (await invoke<Ws[]>("workspace_list")).find((w) => w.root === dir)!;
    // Tasks outlive specs (resetState leaves the tracker alone): only the
    // ones made after this point are the plan's.
    const before = new Set((await issues()).map((i) => i.id));
    const added = async () => (await issues()).filter((i) => !before.has(i.id));
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
    expect(await added()).toEqual([]);
    const title = $$('[data-testid="plan-row-title"]')[0]!;
    await title.clearValue();
    await title.setValue("OAuth routes");
    await $('[data-testid="plan-create"]').click();
    await $('[data-testid="plan-created"]').waitForExist({ timeout: 10_000 });

    const made = await added();
    expect(made.map((i) => i.title).sort()).toEqual(["OAuth routes", "Write the sign-in page"]);
    expect(made.every((i) => i.status === "todo" && i.project && i.suggested_engine)).toBe(true);
    expect(new Set(made.map((i) => i.project)).size).toBe(1);
    const routes = made.find((i) => i.title === "OAuth routes")!;
    const page2 = made.find((i) => i.title === "Write the sign-in page")!;

    // The routes task already runs from its own Execute… (held in setup).
    await invoke("test_task_lane_delay", { ms: 60_000 });
    await invoke("task_execute", {
      input: {
        issueId: routes.id,
        workspaceId: ws.id,
        engineId: routes.suggested_engine,
        isolation: "lane",
        access: "edits",
        review: false,
      },
    });

    // Execute all… shows each run first: nothing starts until Run.
    await $('[data-testid="plan-execute"]').click();
    await $('[data-testid="plan-launch-run"]').waitForExist({ timeout: 10_000 });
    const rows = await $$('[data-testid="plan-launch-run"]');
    expect(rows.length).toBe(2);
    const shown: string[] = [];
    const skips: string[] = [];
    for (const r of rows) {
      shown.push(String(await r.getProperty("textContent")));
      skips.push((await r.getAttribute("data-skip")) ?? "");
    }
    // The rows follow the plan's order: find each task's by its id.
    const at = (id: string) => shown.findIndex((row) => row.startsWith(`${id} · `));
    expect([at(routes.id), at(page2.id)].sort()).toEqual([0, 1]);
    expect(skips[at(routes.id)]).toBe("running");
    expect(shown[at(routes.id)]).toContain("Left alone: already running");
    expect(shown[at(page2.id)]).toContain(`${page2.id} · Write the sign-in page`);
    expect(shown[at(page2.id)]).toContain("Edits in its own worktree · reviewed by ");
    const runsOf = async (id: string) =>
      (await invoke<AttemptView[]>("task_attempts_list")).filter((r) => r.attempt.issueId === id);
    expect(await runsOf(page2.id)).toEqual([]);
    await $('[data-testid="plan-confirm-back"]').click();
    await $('[data-testid="plan-created"]').waitForExist({ timeout: 5_000 });
    expect(await runsOf(page2.id)).toEqual([]);

    await $('[data-testid="plan-execute"]').click();
    await $('[data-testid="plan-confirm-run"]').waitForClickable({ timeout: 10_000 });
    expect(await text('[data-testid="plan-confirm-run"]')).toBe("Run 1 task");
    await $('[data-testid="plan-confirm-run"]').click();
    await $('[data-testid="plan-dialog"][data-state="started"]').waitForExist({ timeout: 15_000 });
    expect(await text('[data-testid="plan-skipped"]')).toBe(`Left alone: ${routes.id} (already running).`);

    // One run per task: the page's on its suggested engine; the routes
    // task keeps only its own.
    const plan = (await invoke<Plan[]>("plan_list")).find((p) => p.state === "started")!;
    expect(plan.queued).toEqual([page2.id]);
    expect(plan.skipped).toEqual([{ issueId: routes.id, reason: "running" }]);
    const run = await browser.waitUntil(
      async () => {
        const r = await runsOf(page2.id);
        return r.length === 1 ? r[0]! : false;
      },
      { timeout: 15_000, timeoutMsg: "the page task got no run" },
    );
    expect(run.attempt.engineId).toBe(page2.suggested_engine);
    expect(await runsOf(routes.id)).toHaveLength(1);
    expect(await $('[data-testid="plan-execute"]').isExisting()).toBe(false);
    await $('[data-testid="plan-close"]').click();
    await $('[data-testid="plan-dialog"]').waitForExist({ reverse: true, timeout: 5_000 });
  });
});
