import { $, browser } from "@wdio/globals";
import { freshUser, invoke, mod, planFooter } from "../support/app";
import { expectNoSeriousA11yViolations } from "../support/axe";

// E2E-M11-6 Free (F-RT-04, F-TK-04): a Free account sees the Routines
// upsell, and Execute… on a task is disabled with the plan copy.
describe("E2E-M11-6 Free plan", () => {
  before(async function () {
    this.timeout(180_000);
    await freshUser("m11-6");
    await browser.waitUntil(async () => (await planFooter()).startsWith("FREE"), { timeout: 20_000 });
  });

  it("F-RT-04 the Routines page is the Pro upsell", async function () {
    this.timeout(60_000);
    await browser.keys([mod(), "1"]);
    await $('[data-testid="agent-footer-routines"]').click();
    await $('[data-testid="routines-page"]').waitForDisplayed({ timeout: 10_000 });
    await $("h2=Routines are part of Pro").waitForDisplayed({ timeout: 10_000 });
    expect(await $('[data-testid="routines-upgrade"]').isDisplayed()).toBe(true);
    expect(await $('[data-testid="routine-new"]').isExisting()).toBe(false);
    await expectNoSeriousA11yViolations();
    // The core agrees.
    const refused = await invoke("routine_save", {
      input: {
        teammateId: "tm_x",
        name: "Nope",
        instruction: "Summarize what changed overnight in three bullets.",
        schedule: { schedule: { kind: "cron", expr: "0 9 * * *" }, timezone: "UTC" },
        access: "edits",
        fullConfirmed: false,
        runMode: "in_app",
      },
    }).then(
      () => "",
      (e: Error) => e.message,
    );
    expect(refused).toContain('"code":"forbidden_plan"');
  });

  it("F-TK-04 Execute is disabled with the plan copy", async function () {
    this.timeout(60_000);
    const id = (await invoke<{ id: string }>("task_save", { input: { title: "Try running me" } })).id;
    await $('[data-testid="agent-footer-tasks"]').click();
    await $(`[data-testid="task-row"][data-id="${id}"] button`).click();
    const execute = await $('[data-testid="task-execute"]');
    await execute.waitForDisplayed({ timeout: 10_000 });
    await execute.click();
    await $('[data-testid="task-execute-dialog"]').waitForDisplayed({ timeout: 10_000 });
    expect(await $('[data-testid="task-execute-blocked"]').getText()).toBe(
      "Running tasks is part of Pro. You can still track tasks for free.",
    );
    expect(await $('[data-testid="task-execute-start"]').isEnabled()).toBe(false);
    const refused = await invoke("task_execute", {
      input: {
        issueId: id,
        workspaceId: "ws_x",
        engineId: "claude",
        isolation: "lane",
        access: "edits",
        fullConfirmed: false,
        review: false,
      },
    }).then(
      () => "",
      (e: Error) => e.message,
    );
    expect(refused).toContain('"code":"forbidden_plan"');
    await browser.keys("Escape");
  });
});
