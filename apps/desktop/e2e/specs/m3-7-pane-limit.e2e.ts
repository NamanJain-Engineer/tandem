import { $, browser } from "@wdio/globals";
import { addFolder, freshOffline, invoke, mod, paneIds, split } from "../support/app";

// E2E-M3-7 No pane limit (F-CD-03 AC7): offline, with no account, a fifth
// pane opens the picker and starts (D-869, D-2900). It was the Free limit of
// four live panes.
describe("E2E-M3-7 no pane limit", () => {
  it("F-CD-03.7 offline a fifth pane opens the picker and starts its shell", async () => {
    await freshOffline();
    await addFolder(`/tmp/tandem-e2e-m3-7-${Date.now()}`);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    for (let i = 0; i < 3; i++) await split(i % 2 ? "down" : "right");
    await browser.waitUntil(
      async () => (await invoke<Array<{ liveCount: number }>>("workspace_list"))[0]?.liveCount === 4,
      { timeout: 15_000 },
    );
    await split("right");
    expect(await $('[data-testid="pane-limit"]').isExisting()).toBe(false);
    await browser.waitUntil(
      async () => (await invoke<Array<{ liveCount: number }>>("workspace_list"))[0]?.liveCount === 5,
      { timeout: 15_000, timeoutMsg: "the fifth pane didn't start" },
    );
    expect((await paneIds()).length).toBe(5);
    // A sixth through the shortcut's picker, too.
    await browser.execute(() => (document.activeElement as HTMLElement | null)?.blur());
    await browser.keys([mod(), "Shift", "d"]);
    await $('[data-testid="tile-shell"]').waitForDisplayed({ timeout: 10_000 });
    await browser.keys("Escape");
  });
});
