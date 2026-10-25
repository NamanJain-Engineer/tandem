import { expect, test, type Page } from "@playwright/test";

// The product scenes in a real browser (F-WB-01 AC8, AC9; D-1880, D-1881).

test.beforeEach(async ({ context }) => {
  await context.route(/\.(webm|mp4)$/, (route) => route.fulfill({ status: 204, body: "" }));
  await context.route(/\/demo\/app\//, (route) => route.fulfill({ status: 204, body: "" }));
});

/** The play states of a scene's own effects (its `sc-*` animations; a
 * working agent's dots pulse for as long as it works). */
const states = (page: Page, id: string) =>
  page.evaluate((id) => {
    const scene = document.querySelector(`[data-scene="${id}"]`);
    if (!scene) return ["missing"];
    const own = scene.getAnimations({ subtree: true }).filter((a) => String((a as CSSAnimation).animationName).startsWith("sc-"));
    return [...new Set(own.map((a) => a.playState))].sort();
  }, id);

const opacity = (page: Page, selector: string) =>
  page.evaluate((s) => Number(getComputedStyle(document.querySelector(s) as Element).opacity), selector);

for (const [w, h] of [[1440, 900], [1366, 768], [1280, 800]] as const) {
  test(`F-WB-01 AC9 the product is in the first viewport at ${w}×${h} (DESIGN §5.1)`, async ({ page }) => {
    await page.setViewportSize({ width: w, height: h });
    await page.goto("/");
    const box = await page.locator('[data-scene="workspace"] .frame').boundingBox();
    expect(box).not.toBeNull();
    if (!box) return;
    const visible = h - box.y;
    expect(visible / box.height, `${visible}px of ${box.height}px`).toBeGreaterThanOrEqual(0.15);
  });
}

// Screen sizes, and the viewports a laptop's browser actually gets once its
// tabs, address bar and the taskbar take 100–140 px (D-1883).
for (const [w, h] of [[1366, 768], [1280, 800], [1536, 864], [1100, 768], [1366, 650], [1536, 740], [1280, 700]] as const) {
  test(`F-WB-01 AC9 the hero's scene plays in a ${w}×${h} first screen, without scrolling`, async ({ page }) => {
    await page.setViewportSize({ width: w, height: h });
    await page.goto("/");
    await expect.poll(() => states(page, "workspace"), { timeout: 10_000 }).toContain("running");
  });
}

test("F-WB-01 AC9 with the scene player blocked, every scene shows its final frame, not its first (D-1880)", async ({ browser }) => {
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
  await context.route(/\/demo\/embed\.js$/, (route) => route.abort());
  await context.route(/\/demo\/app\//, (route) => route.fulfill({ status: 204, body: "" }));
  const page = await context.newPage();
  await page.goto("/");
  await page.waitForLoadState("load");
  expect(await page.evaluate(() => document.documentElement.hasAttribute("data-fx"))).toBe(false);
  for (const id of ["workspace", "dashboard", "review"]) {
    await page.locator(`[data-scene="${id}"]`).scrollIntoViewIfNeeded();
    expect(await states(page, id), id).toEqual([]);
  }
  expect(await opacity(page, '[data-scene="workspace"] .sc-toast-bottom')).toBe(1);
  expect(await opacity(page, '[data-scene="workspace"] .sc-ask')).toBe(0);
  expect(await opacity(page, '[data-scene="review"] .sc-diff-foot .sc-btn')).toBe(1);
  await context.close();
});

test("F-WB-01 AC9 a scene waits off screen, plays on screen, and stops in a hidden tab", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto("/");
  // Below the fold: waiting at its first frame.
  expect(await states(page, "review")).toEqual(["paused"]);
  await page.locator('[data-scene="review"]').scrollIntoViewIfNeeded();
  await expect.poll(() => states(page, "review")).toContain("running");
  // The tab goes into the background: everything pauses where it is.
  await page.evaluate(() => {
    Object.defineProperty(document, "hidden", { configurable: true, get: () => true });
    document.dispatchEvent(new Event("visibilitychange"));
  });
  await expect.poll(() => states(page, "review")).not.toContain("running");
  await page.evaluate(() => {
    Object.defineProperty(document, "hidden", { configurable: true, get: () => false });
    document.dispatchEvent(new Event("visibilitychange"));
  });
  // It plays once and rests on its final frame.
  await expect.poll(() => states(page, "review"), { timeout: 20_000 }).toEqual(["finished"]);
  expect(await opacity(page, '[data-scene="review"] .sc-diff-foot .sc-btn')).toBe(1);
});

test("F-WB-01 AC8 AC9 under reduced motion every scene shows its final frame and nothing moves", async ({ browser }) => {
  const context = await browser.newContext({ reducedMotion: "reduce", viewport: { width: 1440, height: 900 } });
  await context.route(/\/demo\/app\//, (route) => route.fulfill({ status: 204, body: "" }));
  const page = await context.newPage();
  await page.goto("/");
  for (const id of ["workspace", "dashboard", "review"]) {
    await page.locator(`[data-scene="${id}"]`).scrollIntoViewIfNeeded();
    const moving = await page.evaluate(
      (id) =>
        (document.querySelector(`[data-scene="${id}"]`) as Element)
          .getAnimations({ subtree: true })
          .filter((a) => a.playState === "running" && (a.effect?.getComputedTiming().duration ?? 0) > 1).length,
      id,
    );
    expect(moving, id).toBe(0);
  }
  // The final frame: the review item has arrived, the answered approval has gone.
  expect(await opacity(page, '[data-scene="workspace"] .sc-toast-bottom')).toBe(1);
  expect(await opacity(page, '[data-scene="workspace"] .sc-ask')).toBe(0);
  expect(await opacity(page, '[data-scene="dashboard"] .sc-card-actions')).toBe(1);
  await context.close();
});

test("F-WB-01 AC9 the hero's scene plays without moving anything else on the page", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.addInitScript(() => {
    const w = window as unknown as { shift: number };
    w.shift = 0;
    new PerformanceObserver((list) => {
      for (const e of list.getEntries() as Array<PerformanceEntry & { value: number; hadRecentInput: boolean }>)
        if (!e.hadRecentInput) w.shift += e.value;
    }).observe({ type: "layout-shift", buffered: true });
  });
  await page.goto("/");
  await expect.poll(() => states(page, "workspace"), { timeout: 20_000 }).toEqual(["finished"]);
  const shift = await page.evaluate(() => (window as unknown as { shift: number }).shift);
  expect(shift).toBeLessThan(0.01);
});
