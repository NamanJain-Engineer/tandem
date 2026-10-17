import { $, browser } from "@wdio/globals";
import { execFileSync } from "node:child_process";
import { mkdirSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import {
  addFolder,
  freshOffline,
  invoke,
  mod,
  paneIds,
  sendToPane,
  waitForPaneText,
  waitForStatus,
} from "../support/app";

const GIT_ENV = {
  ...process.env,
  GIT_AUTHOR_NAME: "E2E",
  GIT_AUTHOR_EMAIL: "e2e@vibezeo.test",
  GIT_COMMITTER_NAME: "E2E",
  GIT_COMMITTER_EMAIL: "e2e@vibezeo.test",
};
const git = (cwd: string, ...args: string[]) =>
  execFileSync("git", ["-C", cwd, ...args], { encoding: "utf8", env: GIT_ENV }).trim();

type Ws = { id: string; laneBranch: string | null; root: string };
type Item = {
  key: string;
  state: string;
  diffHash: string;
  laneId: string | null;
  risks: Array<{ rule: string; files: string[] }>;
  check: { state: string; exit: number | null; output: string } | null;
};

const queue = async (): Promise<Item[]> => (await invoke<{ items: Item[] }>("review_queue", {})).items;

async function waitItem(
  key: string,
  timeout: number,
  pred: (i: Item) => boolean = () => true,
): Promise<Item> {
  let found: Item | undefined;
  await browser.waitUntil(
    async () => {
      found = (await queue()).find((i) => i.key === key && pred(i));
      return found !== undefined;
    },
    { timeout, interval: 100, timeoutMsg: `${key} not in the queue: ${JSON.stringify(await queue())}` },
  );
  return found as Item;
}

/** Shows a worktree (its tab mounts, so its agent's terminal runs). */
async function show(branch: string): Promise<void> {
  await browser.execute((b: string) => {
    const row = Array.from(document.querySelectorAll<HTMLElement>('[data-testid="worktree-row"]')).find((r) =>
      r.querySelector(`button[aria-label="Actions for ${b}"]`),
    );
    row?.click();
  }, branch);
}

/** The agent pane running in a worktree (the launch sheet's seats). */
async function agentIn(laneId: string): Promise<string> {
  let pane = "";
  await browser.waitUntil(
    async () => {
      const ws = (await invoke<Array<Ws & { tabs: Array<{ id: string }> }>>("workspace_list", {})).find(
        (w) => w.id === laneId,
      );
      for (const tab of ws?.tabs ?? []) {
        const panes = await invoke<Array<{ id: string; engineId: string | null }>>("pane_list", {
          tabId: tab.id,
        });
        pane = panes.find((p) => p.engineId !== null)?.id ?? "";
        if (pane) return true;
      }
      return false;
    },
    { timeout: 30_000, timeoutMsg: `no agent in ${laneId}` },
  );
  return pane;
}

// E2E-M18-DB-04 Review queue (F-DB-04).
describe("E2E-M18-DB-04 review queue", function () {
  this.timeout(300_000);
  const repo = `/tmp/tandem-e2e-db-04-${Date.now()}`;
  let lane1: Ws;
  let lane2: Ws;
  let agent1 = "";
  let agent2 = "";

  before(async () => {
    mkdirSync(join(repo, "db/migrations"), { recursive: true });
    mkdirSync(join(repo, "tests"), { recursive: true });
    git(repo, "init", "-q", "-b", "main");
    writeFileSync(join(repo, "db/migrations/0001.sql"), "create table a (id int);\n");
    writeFileSync(join(repo, "tests/auth.test.ts"), "test('auth', () => {});\n");
    writeFileSync(join(repo, "README.md"), "# app\n");
    git(repo, "add", ".");
    git(repo, "commit", "-q", "-m", "first");
    await freshOffline();
    await addFolder(repo);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    await browser.keys([mod(), "Shift", "l"]);
    await $('[data-testid="launch-sheet"]').waitForExist({ timeout: 10_000 });
    await $('//label[normalize-space()="How many of seat 1"]/following::input[1]').setValue("2");
    await $('[data-testid="launch-task"]').setValue("tidy");
    await $('[data-testid="launch-go"]').click();
    await $('[data-testid="launch-sheet"]').waitForExist({ reverse: true, timeout: 30_000 });
    const lanes = await browser.waitUntil(
      async () => {
        const all = (await invoke<Ws[]>("workspace_list", {})).filter((w) => w.laneBranch);
        return all.length === 2 ? all : false;
      },
      { timeout: 20_000 },
    );
    [lane1, lane2] = [
      lanes.find((w) => w.laneBranch === "tandem/tidy") ?? lanes[0],
      lanes.find((w) => w.laneBranch !== "tandem/tidy") ?? lanes[1],
    ] as [Ws, Ws];
    agent1 = await agentIn(lane1.id);
    agent2 = await agentIn(lane2.id);
  });

  it("F-DB-04.1-2 a done agent with changes joins within 1 s with its rules; one without changes doesn't", async () => {
    await show(lane1.laneBranch ?? "");
    await waitForPaneText(agent1, "Fake Claude Code", 30_000);
    writeFileSync(join(lane1.root, "db/migrations/0042.sql"), "alter table a add b int;\n");
    rmSync(join(lane1.root, "tests/auth.test.ts"));
    await sendToPane(agent1, "finished\r");
    await waitForStatus(agent1, "done", 15_000);
    const item = await waitItem(`pane:${agent1}`, 1_500);
    expect(item.laneId).toBe(lane1.id);
    // The deleted file also has "auth" in its name, so that rule fires too.
    expect(item.risks).toEqual(
      expect.arrayContaining([
        { rule: "migrations", files: ["db/migrations/0042.sql"] },
        { rule: "deleted_tests", files: ["tests/auth.test.ts"] },
      ]),
    );
    await show(lane2.laneBranch ?? "");
    await waitForPaneText(agent2, "Fake Claude Code", 30_000);
    await sendToPane(agent2, "nothing to do\r");
    await waitForStatus(agent2, "done", 15_000);
    await browser.pause(1_500);
    expect((await queue()).some((i) => i.key === `pane:${agent2}`)).toBe(false);
  });

  it("F-DB-04.1 a thread that finishes with changes joins too", async () => {
    const thread = await invoke<{ id: string }>("thread_create", {
      args: { kind: "thread", workspaceId: lane2.id, engineId: "claude", access: "edits" },
    });
    await invoke("thread_send", { threadId: thread.id, text: "write notes.md=hello" });
    await waitItem(`thread:${thread.id}`, 30_000);
  });

  it("F-DB-04.3 F-DB-04.6 the Review view: approve, then a new change brings it back as changed", async () => {
    await browser.keys([mod(), "Alt", "Shift", "d"]);
    const review = $('//*[@role="radio"][starts-with(normalize-space(), "Review")]');
    await review.waitForExist({ timeout: 10_000 });
    await review.click();
    const card = $(`[data-testid="review-item"][data-key="pane:${agent1}"]`);
    await card.waitForExist({ timeout: 10_000 });
    expect(await card.$('[data-testid="review-risk"][data-rule="migrations"]').isExisting()).toBe(true);
    await card.$('[data-testid="review-approve"]').click();
    await browser.waitUntil(async () => (await card.getAttribute("data-state")) === "approved", {
      timeout: 5_000,
    });
    await show(lane1.laneBranch ?? "");
    writeFileSync(join(lane1.root, "README.md"), "# app, edited\n");
    await sendToPane(agent1, "edited again\r");
    await waitForStatus(agent1, "done", 15_000);
    await waitItem(`pane:${agent1}`, 5_000, (i) => i.state === "changed");
    await $(
      `[data-testid="review-item"][data-key="pane:${agent1}"] [data-testid="review-changed"]`,
    ).waitForExist({
      timeout: 5_000,
    });
  });

  it("F-DB-04.4 Send back opens the worktree's diff; Merge opens the confirm with the forecast", async () => {
    const card = $(`[data-testid="review-item"][data-key="pane:${agent1}"]`);
    await card.$('[data-testid="review-send-back"]').click();
    await $('[data-testid="worktree-diff"] [data-testid="diff-view"]').waitForExist({ timeout: 15_000 });
    await browser.keys("Escape");
    await $('[data-testid="worktree-diff"]').waitForExist({ reverse: true, timeout: 5_000 });
    await card.$('[data-testid="review-merge"]').click();
    await $('[data-testid="merge-forecast"]').waitForExist({ timeout: 15_000 });
    await browser.keys("Escape");
  });

  it("F-DB-04.5 a check command passes for true and fails with exit 1 for false", async () => {
    const key = `pane:${agent1}`;
    await invoke("review_repo_set", { workspaceId: lane1.id, check: "true", apiPaths: [] });
    await invoke("review_run_check", { key });
    let item = await waitItem(key, 15_000, (i) => i.check?.state === "passed");
    expect(item.check?.exit).toBe(0);
    await invoke("review_repo_set", { workspaceId: lane1.id, check: "echo nope; false", apiPaths: [] });
    await invoke("review_run_check", { key });
    item = await waitItem(key, 15_000, (i) => i.check?.state === "failed");
    expect(item.check?.exit).toBe(1);
    expect(item.check?.output).toContain("nope");
    await $(
      `[data-testid="review-item"][data-key="${key}"] [data-testid="review-check"][data-state="failed"]`,
    ).waitForExist({
      timeout: 5_000,
    });
  });
});
