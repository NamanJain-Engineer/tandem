import { $, browser } from "@wdio/globals";
import { addFolder, freshOffline, invoke, paneIds } from "../support/app";
import { sendMessage, threadMode, waitForAssistant } from "../support/thread";

type Item = { key: string; status: string; pendingApproval: { summary: string } | null };

const pinned = '[data-testid="thread-pending"]';

async function openDashboard(): Promise<void> {
  if (
    !(await $('[data-testid="sidebar"]')
      .isDisplayed()
      .catch(() => false))
  )
    await $('[data-testid="sidebar-toggle"]').click();
  await $('[data-testid="sidebar-pill-dashboard"]').click();
  await $('[data-testid="dashboard"]').waitForExist({ timeout: 10_000 });
}

async function pendingCount(): Promise<number> {
  return (await invoke<unknown[]>("approvals_pending")).length;
}

// E2E-M7-2 Approvals (F-TH-11, D-6): the fake Claude asks through the
// gateway's approvals tool. The card shows inline and the thread is Waiting
// on the Dashboard; Allow for this thread covers `npm test …` but not
// `npm install`; Deny with a reason blocks the action; the Dashboard card
// answers for the thread too.
describe("E2E-M7-2 approvals", () => {
  let threadId = "";

  before(async function () {
    this.timeout(60_000);
    await freshOffline();
    await addFolder(`/tmp/tandem-e2e-m7-2-${Date.now()}`);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    await threadMode();
    const [ws] = await invoke<Array<{ id: string }>>("workspace_list");
    const thread = await invoke<{ id: string }>("thread_create", {
      args: { kind: "thread", workspaceId: ws?.id },
    });
    threadId = thread.id;
    const row = await $(`[data-testid="thread-row"][data-thread-id="${threadId}"]`);
    await row.waitForExist({ timeout: 10_000 });
    await row.click();
    await $('[data-testid="thread-main"]').waitForExist({ timeout: 10_000 });
  });

  it("F-TH-11.1 D-6 a command asks inline, and the Dashboard shows Waiting within 500 ms", async function () {
    this.timeout(60_000);
    await sendMessage("run npm test");
    await $(pinned).waitForExist({ timeout: 20_000 });
    const shown = Date.now();
    await browser.waitUntil(
      async () => {
        const items = await invoke<Item[]>("status_snapshot");
        const it = items.find((i) => i.key === `thread:${threadId}`);
        return it?.status === "waiting" && it.pendingApproval !== null;
      },
      { timeout: 2_000, interval: 25, timeoutMsg: "the thread never showed Waiting on the Dashboard" },
    );
    expect(Date.now() - shown).toBeLessThan(500);
    expect(await $(`${pinned}`).getText()).toContain("npm test");
  });

  it("F-TH-11.2 Allow for this thread covers npm test with arguments, not npm install", async function () {
    this.timeout(60_000);
    await $(`${pinned} [data-testid="approval-allow-thread"]`).click();
    await waitForAssistant("Ran npm test.");
    await $(pinned).waitForExist({ reverse: true, timeout: 5_000 });

    await sendMessage("run npm test -- --watch");
    await waitForAssistant("Ran npm test -- --watch.");
    expect(await pendingCount()).toBe(0);

    await sendMessage("run npm install");
    await $(pinned).waitForExist({ timeout: 20_000 });
    expect(await $(pinned).getText()).toContain("npm install");
  });

  it("F-TH-11.3 Deny with a reason shows the denied row and Completed with blocked actions", async function () {
    this.timeout(60_000);
    await $(pinned).$("button=Deny and explain…").click();
    const reason = await $(pinned).$("textarea");
    await reason.waitForDisplayed({ timeout: 5_000 });
    await reason.setValue("use pnpm here");
    await $(pinned).$("button=Deny").click();
    await waitForAssistant("Okay, I won't run it.");
    await $('[data-testid="thread-end"]').waitForExist({ timeout: 10_000 });
    expect(await $('[data-testid="thread-end"]').getText()).toContain("Completed with blocked actions");
    expect(await $('[data-tool-status="denied"]').isExisting()).toBe(true);
  });

  it("D-6 Allow on the Dashboard card lets the thread proceed", async function () {
    this.timeout(60_000);
    await sendMessage("run npm ci");
    await $(pinned).waitForExist({ timeout: 20_000 });
    await openDashboard();
    const card = `[data-testid="agent-card"][data-key="thread:${threadId}"]`;
    await $(card).waitForExist({ timeout: 5_000 });
    // Allow takes clicks once the question has been shown 500 ms.
    await $(`${card} [data-testid="card-allow"]`).waitForEnabled();
    await $(`${card} [data-testid="card-allow"]`).click();
    await waitForAssistant("Ran npm ci.");
    expect(await pendingCount()).toBe(0);
  });
});
