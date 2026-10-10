import { $, browser } from "@wdio/globals";
import { expectNoSeriousA11yViolationsInBothThemes } from "../support/axe";
import { addFolder, browserLog, freshUser, planFooter, textOf, workspaceRoots } from "../support/app";

// E2E-M2-2 Free gate (MILESTONES M2, F-BL-04, F-BL-01 AC2).
describe("E2E-M2-2 free gate", () => {
  const base = `/tmp/tandem-e2e-gate-${Date.now()}`;

  it("adds three workspaces on Free", async () => {
    await freshUser("gate");
    for (const n of [1, 2, 3]) {
      await addFolder(`${base}/w${n}`);
      await browser.waitUntil(async () => (await workspaceRoots()).includes(`${base}/w${n}`), {
        timeout: 10_000,
      });
    }
    expect(await planFooter()).toBe("FREE");
  });

  it("the fourth opens the upgrade sheet naming the limit", async () => {
    await addFolder(`${base}/w4`, { added: false });
    await $('[data-testid="upgrade-gate"]').waitForExist({ timeout: 10_000 });
    expect(await textOf('[data-testid="upgrade-gate"]')).toBe("You hit the Free limit of 3 workspaces.");
    expect(await workspaceRoots()).not.toContain(`${base}/w4`);
    await expectNoSeriousA11yViolationsInBothThemes();
  });

  it("mock checkout turns the footer to PRO within 5 s", async () => {
    await $('[data-testid="upgrade-pro"]').click();
    await browser.waitUntil(() => browserLog().includes("checkout:complete"), {
      timeout: 60_000,
      timeoutMsg: `checkout: ${browserLog()}`,
    });
    const completed = Date.now();
    await browser.waitUntil(async () => (await planFooter()) === "PRO · 10,000 credits", {
      timeout: 5_000,
      timeoutMsg: `footer: ${await planFooter()}`,
    });
    expect(Date.now() - completed).toBeLessThan(5_000);
  });

  it("the fourth workspace can be added", async () => {
    await $("button=Add the folder you tried to add").waitForClickable({ timeout: 10_000 });
    await $("button=Add the folder you tried to add").click();
    await browser.waitUntil(async () => (await workspaceRoots()).includes(`${base}/w4`), { timeout: 10_000 });
  });
});
