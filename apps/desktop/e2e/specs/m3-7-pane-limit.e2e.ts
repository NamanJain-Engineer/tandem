import { $, browser } from "@wdio/globals";
import { addFolder, freshOffline, invoke, mod, paneIds, split } from "../support/app";

// E2E-M3-7 Free pane limit (F-CD-03 AC7): offline mode has the Free limits.
describe("E2E-M3-7 free pane limit", () => {
  it("F-CD-03.7 a fifth pane shows the upgrade card and starts nothing", async () => {
    await freshOffline();
    await addFolder(`/tmp/tandem-e2e-m3-7-${Date.now()}`);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    for (let i = 0; i < 3; i++) await split(i % 2 ? "down" : "right");
    await browser.waitUntil(async () => (await invoke<Array<{ liveCount: number }>>("workspace_list"))[0]?.liveCount === 4, { timeout: 15_000 });
    const spawned = (await invoke<unknown[]>("test_spawn_log")).length;
    await browser.execute(() => (document.activeElement as HTMLElement | null)?.blur());
    await browser.keys([mod(), "Shift", "d"]);
    await $('[data-testid="pane-limit"]').waitForDisplayed({ timeout: 10_000 });
    expect(await browser.execute(() => document.body.innerText.includes("Free includes 4 live panes"))).toBe(true);
    expect(await $('[data-testid="tile-shell"]').isExisting()).toBe(false);
    await browser.pause(500);
    expect((await invoke<unknown[]>("test_spawn_log")).length).toBe(spawned);
    expect((await paneIds()).length).toBe(4);
    await browser.keys("Escape");
  });
});
