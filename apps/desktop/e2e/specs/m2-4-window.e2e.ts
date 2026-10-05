import { spawnSync } from "node:child_process";
import { $, browser } from "@wdio/globals";
import { freshOffline, invoke, relaunch } from "../support/app";

const isMaximized = () => invoke<boolean>("plugin:window|is_maximized", { label: "main" });

// E2E-M2-4 Single instance and window state (MILESTONES M2, F-SH-01 AC1–5).
describe("E2E-M2-4 single instance and window state", () => {
  it("F-SH-01.1 a second launch exits 0 in under 1 s", async () => {
    await freshOffline();
    const started = Date.now();
    const second = spawnSync(process.env.TANDEM_APP_BINARY ?? "", [], { env: process.env, timeout: 5_000 });
    expect(second.status).toBe(0);
    expect(Date.now() - started).toBeLessThan(1_000);
    await $('header[role="banner"]').waitForExist();
  });

  it("F-SH-01.4 the maximize label toggles Maximize / Restore", async () => {
    const button = $('[data-testid="window-maximize"]');
    if ((await button.getAttribute("aria-label")) === "Restore") await button.click();
    await browser.waitUntil(async () => (await button.getAttribute("aria-label")) === "Maximize", { timeout: 5_000 });
    await button.click();
    await browser.waitUntil(async () => (await button.getAttribute("aria-label")) === "Restore", { timeout: 5_000 });
    expect(await isMaximized()).toBe(true);
  });

  it("F-SH-01.2 relaunching restores the maximized state", async () => {
    // The app saves window state 400 ms after a change; give it a moment
    // before the driver kills the process.
    await browser.pause(1_000);
    await relaunch();
    await browser.waitUntil(isMaximized, { timeout: 10_000, timeoutMsg: "not maximized after relaunch" });
    expect(await $('[data-testid="window-maximize"]').getAttribute("aria-label")).toBe("Restore");
  });

  it("F-SH-01.3 an off-screen saved position comes back fully on-screen", async () => {
    if (await isMaximized()) await $('[data-testid="window-maximize"]').click();
    await browser.waitUntil(async () => !(await isMaximized()), { timeout: 5_000 });
    // Park the window where no display is; the app saves the position.
    await invoke("plugin:window|set_position", { label: "main", value: { Physical: { x: 20000, y: 20000 } } });
    await browser.pause(1_000);
    await relaunch();
    const rect = await browser.getWindowRect();
    const screen = (await browser.execute(() => [window.screen.width, window.screen.height])) as [number, number];
    expect(rect.x).toBeGreaterThanOrEqual(0);
    expect(rect.y).toBeGreaterThanOrEqual(0);
    expect(rect.x + rect.width).toBeLessThanOrEqual(screen[0]);
    expect(rect.y + rect.height).toBeLessThanOrEqual(screen[1]);
  });

  it("F-SH-01.5 tabbing through the title bar follows visual order with every control named", async () => {
    const order = (await browser.execute(() => {
      const bar = document.querySelector('header[role="banner"]');
      if (!bar) return [];
      const focusable = Array.from(bar.querySelectorAll<HTMLElement>("button, [href], [tabindex]:not([tabindex='-1']), [role=radio]")).filter(
        (el) => el.offsetParent !== null && el.tabIndex >= 0,
      );
      return focusable.map((el) => ({ left: Math.round(el.getBoundingClientRect().left), name: el.getAttribute("aria-label") ?? el.textContent?.trim() ?? "" }));
    })) as Array<{ left: number; name: string }>;
    expect(order.length).toBeGreaterThan(5);
    for (const c of order) expect(c.name).not.toBe("");
    const lefts = order.map((c) => c.left);
    expect([...lefts].sort((a, b) => a - b)).toEqual(lefts);
  });
});
