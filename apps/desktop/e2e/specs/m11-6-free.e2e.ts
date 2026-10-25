import { $, browser } from "@wdio/globals";
import { freshOffline, invoke, mod } from "../support/app";
import { expectNoSeriousA11yViolations } from "../support/axe";

// E2E-M11-6 Free offline (F-RT-04, F-TK-04): with no account, the Routines
// page offers New routine and the core saves a routine; Execute… on a task
// isn't blocked, and the core takes a run without asking for sign-in or a
// plan (D-869, D-2900, D-2902). It was the Free upsell.
describe("E2E-M11-6 free offline", () => {
  before(async function () {
    this.timeout(180_000);
    await freshOffline();
  });

  it("F-RT-04 the Routines page offers New routine and the core saves one", async function () {
    this.timeout(60_000);
    await browser.keys([mod(), "1"]);
    await $('[data-testid="agent-footer-routines"]').click();
    await $('[data-testid="routines-page"]').waitForDisplayed({ timeout: 10_000 });
    await $('[data-testid="routine-new"]').waitForDisplayed({ timeout: 10_000 });
    expect(await $('[data-testid="routines-upgrade"]').isExisting()).toBe(false);
    await expectNoSeriousA11yViolations();
    const lead = (await invoke<Array<{ teammate: { id: string; isLead: boolean } }>>("teammate_list")).find(
      (t) => t.teammate.isLead,
    );
    expect(lead).toBeTruthy();
    const saved = await invoke<{ id: string }>("routine_save", {
      input: {
        teammateId: lead?.teammate.id,
        name: "Morning summary",
        instruction: "Summarize what changed overnight in three bullets.",
        schedule: { schedule: { kind: "cron", expr: "0 9 * * *" }, timezone: "UTC" },
        access: "edits",
        fullConfirmed: false,
        runMode: "in_app",
      },
    });
    expect(saved.id).toBeTruthy();
    await invoke("routine_delete", { id: saved.id });
  });

  it("F-TK-04 Execute isn't blocked, and the core takes a run with no account", async function () {
    this.timeout(60_000);
    const id = (await invoke<{ id: string }>("task_save", { input: { title: "Try running me" } })).id;
    await $('[data-testid="agent-footer-tasks"]').click();
    await $(`[data-testid="task-row"][data-id="${id}"] button`).click();
    const execute = await $('[data-testid="task-execute"]');
    await execute.waitForDisplayed({ timeout: 10_000 });
    await execute.click();
    await $('[data-testid="task-execute-dialog"]').waitForDisplayed({ timeout: 10_000 });
    expect(await $('[data-testid="task-execute-blocked"]').isExisting()).toBe(false);
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
    // Past the limits and with no sign-in check, it reaches the workspace
    // lookup: the unknown workspace is the one thing refused (D-2914).
    expect(refused).toContain('"code":"not_found"');
    expect(refused).not.toContain("forbidden_plan");
    expect(refused).not.toContain("unauthenticated");
    await browser.keys("Escape");
  });
});
