import { $, browser } from "@wdio/globals";
import { expectNoSeriousA11yViolationsInBothThemes } from "../support/axe";
import { addFolder, freshUser, workspaceRoots } from "../support/app";

// E2E-M2-2 No workspace limit (MILESTONES M2, F-CD-01): Vibezeo is free
// (D-869), so a free account adds a fourth workspace and more, and nothing
// offers a plan (D-2900). It was the Free gate of three workspaces.
describe("E2E-M2-2 no workspace limit", () => {
  const base = `/tmp/tandem-e2e-gate-${Date.now()}`;

  it("F-CD-01 a free account adds five workspaces, the fourth and fifth included", async () => {
    await freshUser("gate");
    for (const n of [1, 2, 3, 4, 5]) {
      await addFolder(`${base}/w${n}`);
      await browser.waitUntil(async () => (await workspaceRoots()).includes(`${base}/w${n}`), {
        timeout: 10_000,
      });
    }
    expect(await $('[data-testid="upgrade-gate"]').isExisting()).toBe(false);
    await expectNoSeriousA11yViolationsInBothThemes();
  });
});
