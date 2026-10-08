import { $, browser } from "@wdio/globals";
import { freshOffline, invoke } from "../support/app";
import { composer, threadMode, userTexts, waitForAssistant, sendMessage } from "../support/thread";

type Detail = { thread: { mode: string }; planRevisions: Array<{ version: number; text: string }> };

async function checkedMode(): Promise<string> {
  return browser.execute(
    () =>
      document
        .querySelector<HTMLElement>('[data-testid="composer"] [role="radio"][aria-checked="true"]')
        ?.innerText.trim() ?? "",
  );
}

// E2E-M7-6 Plan → implement (F-TH-10): two turns in Plan mode make plan
// versions 1 and 2; Implement on version 1 sends version 1's text (not
// version 2's) and the composer switches to Act.
describe("E2E-M7-6 plan → implement", () => {
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
    await invoke("thread_update", { threadId, mode: "plan" });
    await browser.waitUntil(async () => (await checkedMode()) === "Plan", {
      timeout: 5_000,
      timeoutMsg: `mode: ${await checkedMode()}`,
    });
  });

  it("F-TH-10.1 two plan turns make v1 and v2; Implement v1 sends v1's text and switches to Act", async function () {
    this.timeout(90_000);
    await sendMessage("plan alpha");
    await waitForAssistant("You said: plan alpha");
    await sendMessage("plan beta");
    await waitForAssistant("You said: plan beta");
    const card = (v: number) => `[data-testid="thread-plan"][data-version="${v}"]`;
    await $(card(2)).waitForExist({ timeout: 10_000 });
    expect(await $(card(1)).getText()).toContain("You said: plan alpha");
    expect(await $(card(2)).getText()).toContain("You said: plan beta");
    const versions = await browser.execute(
      (id: string) =>
        document.querySelector(
          `[data-testid="thread-main"][data-thread-id="${id}"] [data-testid="plan-versions"]`,
        )?.textContent ?? "",
      threadId,
    );
    expect(versions).toBe("Plan versions (2)");

    // Something typed but not sent stays in the composer (D-121).
    // (addValue: keys() drops a repeated letter in this driver.)
    await composer().addValue("keep me");
    await $(card(1)).$('[data-testid="plan-implement"]').click();
    await waitForAssistant("You said: Implement plan version 1 exactly as written:");
    const sent = (await userTexts()).at(-1) ?? "";
    expect(sent).toContain("Implement plan version 1 exactly as written:");
    expect(sent).toContain("You said: plan alpha");
    expect(sent).not.toContain("plan beta");
    await browser.waitUntil(async () => (await checkedMode()) === "Act", {
      timeout: 5_000,
      timeoutMsg: `mode: ${await checkedMode()}`,
    });
    const detail = await invoke<Detail>("thread_get", { threadId });
    expect(detail.thread.mode).toBe("act");
    // The Act turn made no plan.
    expect(detail.planRevisions.map((p) => p.version)).toEqual([1, 2]);
    expect(String(await composer().getProperty("value"))).toBe("keep me");
  });
});
