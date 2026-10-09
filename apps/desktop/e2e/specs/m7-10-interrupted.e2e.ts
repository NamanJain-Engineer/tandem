import { $, browser } from "@wdio/globals";
import { execFileSync } from "node:child_process";
import { crash, freshOffline, invoke, relaunch } from "../support/app";
import { composer, sendMessage, threadMode } from "../support/thread";

type Detail = { thread: { state: string }; turns: Array<{ state: string }> };

async function bannerText(): Promise<string> {
  return browser.execute(
    () => document.querySelector('[data-testid="thread-interrupted"]')?.textContent ?? "",
  );
}

// E2E-M7-10 Interrupted (F-TH-01 AC6, F-SH-07): kill -9 while a thread is
// Running; after the relaunch it says "Interrupted — Resume?" and isn't done.
describe("E2E-M7-10 interrupted", () => {
  let threadId = "";

  before(async function () {
    this.timeout(60_000);
    await freshOffline();
    await threadMode();
    threadId = (
      await invoke<{ id: string }>("thread_create", { args: { kind: "thread", workspaceId: null } })
    ).id;
    const row = await $(`[data-testid="thread-row"][data-thread-id="${threadId}"]`);
    await row.waitForExist({ timeout: 10_000 });
    await row.click();
    await composer().waitForDisplayed({ timeout: 10_000 });
  });

  after(() => {
    // The hung fake engine outlives the crashed app.
    try {
      execFileSync("pkill", ["-9", "-f", "input-format stream-json"]);
    } catch {
      // None left.
    }
  });

  it("F-TH-01.6 a thread running at a crash shows Interrupted — Resume? after relaunch, never done", async function () {
    this.timeout(120_000);
    await sendMessage("/hang");
    await browser.waitUntil(
      async () => (await invoke<Detail>("thread_get", { threadId })).thread.state === "working",
      { timeout: 15_000, timeoutMsg: "the thread never started working" },
    );

    crash();
    await relaunch();
    await threadMode();
    const row = await $(`[data-testid="thread-row"][data-thread-id="${threadId}"]`);
    await row.waitForExist({ timeout: 15_000 });
    await row.click();
    await browser.waitUntil(async () => (await bannerText()).includes("Interrupted — Resume?"), {
      timeout: 15_000,
      timeoutMsg: `banner: ${await bannerText()}`,
    });
    const detail = await invoke<Detail>("thread_get", { threadId });
    expect(detail.thread.state).toBe("interrupted");
    expect(detail.turns.map((t) => t.state)).toEqual(["interrupted"]);
    const state = await browser.execute(
      () => document.querySelector('[data-testid="thread-state"]')?.textContent ?? "",
    );
    expect(state).toBe("Interrupted");
  });
});
