import { expect, test, type Frame, type Page } from "@playwright/test";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

// The interactive demo on the home page (docs/FEATURES_TO_BUILD.md, its
// acceptance tests 1–6; D-128).

const slot = '[data-demo-embed]';

// The test server has no Range support, so the product video would stream in
// full on every load and starve the demo's own requests. It isn't under test.
test.beforeEach(async ({ context }) => {
  await context.route(/\.(webm|mp4)$/, (route) => route.fulfill({ status: 204, body: "" }));
});

async function demoFrame(page: Page): Promise<Frame> {
  await expect(page.locator("[data-demo-stage]")).toHaveAttribute("data-demo-state", "live");
  const handle = await page.locator("[data-demo-frame]").elementHandle();
  const frame = await handle?.contentFrame();
  if (!frame) throw new Error("no demo frame");
  await frame.waitForSelector('[data-testid="demo-bar"]');
  return frame;
}

/** The demo opens in Code mode (D-2605, DESIGN §5.2.1); the ready message
 * waits in Thread mode. */
async function toThread(frame: Frame): Promise<void> {
  await frame.locator('[role="radio"][value="thread"]').first().click();
}

async function sendReady(frame: Frame): Promise<void> {
  await toThread(frame);
  const box = frame.locator('[data-testid="composer"] textarea[aria-label="Message"]').first();
  await expect(box).toHaveValue(/checkout sends some people back/);
  await box.focus();
  await box.press("Enter");
}

test("demo AC1 the frame shows its label and line, and nothing of the demo loads until the section is near", async ({ page }) => {
  const asked: string[] = [];
  page.on("request", (r) => asked.push(new URL(r.url()).pathname));
  await page.setViewportSize({ width: 1280, height: 400 });
  await page.goto("/");
  await expect(page.getByText("Interactive demo · sample data")).toBeVisible();
  await expect(page.getByText("This is Vibezeo itself, running in your browser with sample data. Click around.")).toBeVisible();
  await expect(page.locator(`${slot} img[data-demo-still]`)).toBeVisible();
  await page.waitForTimeout(1500);
  expect(asked.filter((p) => p.startsWith("/demo/app/"))).toEqual([]);
  expect(await page.locator("[data-demo-frame]").count()).toBe(0);
  await page.locator(slot).scrollIntoViewIfNeeded();
  await demoFrame(page);
  expect(asked.some((p) => p.startsWith("/demo/app/"))).toBe(true);
  await expect(page.locator(`${slot} img[data-demo-still]`)).toBeHidden();
});

test("demo the page stays where the visitor is when the demo loads, and keeps the keyboard (D-1821)", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto("/");
  // Just above the frame's load margin, so it loads off screen; at 0 when
  // the frame already loads from the top of the page.
  const top = await page.locator(slot).evaluate((el) => el.getBoundingClientRect().top + window.scrollY);
  const at = Math.max(0, Math.round(top - 900 - 100));
  await page.evaluate((y) => window.scrollTo({ top: y, behavior: "instant" }), at);
  await demoFrame(page);
  await page.waitForTimeout(1500);
  expect(await page.evaluate(() => Math.round(window.scrollY))).toBe(at);
  expect(await page.evaluate(() => document.activeElement?.tagName)).not.toBe("IFRAME");
});

test("demo AC2 AC3 the ready message runs the scripted turn; full screen opens over the page and Esc returns with the same state", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto("/");
  await page.locator(slot).scrollIntoViewIfNeeded();
  const frame = await demoFrame(page);
  await frame.evaluate(() => ((window as unknown as { __mark: number }).__mark = 42));
  await sendReady(frame);
  await frame.getByRole("button", { name: "Allow once" }).first().click();
  await expect(frame.getByText("Fixed the redirect loop.")).toBeVisible();
  await frame.locator('[data-testid="changes-toggle"]').click();
  await expect(frame.locator('[data-testid="change-row"]')).toHaveCount(2);
  const rows = await frame.locator('[data-testid="change-row"]').allTextContents();
  expect(rows.join(" ")).toContain("redirect.ts");
  expect(rows.join(" ")).toContain("redirect.test.ts");

  await page.getByRole("link", { name: "Explore full screen" }).click();
  const stage = page.locator("[data-demo-stage]");
  await expect(stage).toHaveCSS("position", "fixed");
  const box = await stage.boundingBox();
  expect(box?.width).toBe(1440);
  expect(box?.height).toBe(900);
  await page.keyboard.press("Escape");
  await expect(stage).toHaveCSS("position", "absolute");
  // The same page in the frame: nothing reloaded.
  expect(await frame.evaluate(() => (window as unknown as { __mark?: number }).__mark)).toBe(42);
  await expect(frame.getByText("Fixed the redirect loop.")).toBeVisible();

  // Esc from inside the demo closes full screen too.
  await page.getByRole("link", { name: "Explore full screen" }).click();
  await expect(stage).toHaveCSS("position", "fixed");
  await frame.locator('[data-testid="demo-bar"]').click({ position: { x: 5, y: 5 } });
  await frame.locator("body").press("Escape");
  await expect(stage).toHaveCSS("position", "absolute");
});

test("demo AC4 on a phone the frame says Tap to explore, which opens a usable full-screen demo", async ({ browser }) => {
  const context = await browser.newContext({ viewport: { width: 390, height: 844 }, hasTouch: true, isMobile: true });
  await context.route(/\.(webm|mp4)$/, (route) => route.fulfill({ status: 204, body: "" }));
  const page = await context.newPage();
  await page.goto("/");
  await page.locator(slot).scrollIntoViewIfNeeded();
  await page.waitForTimeout(1000);
  expect(await page.locator("[data-demo-frame]").count()).toBe(0);
  await page.getByText("Tap to explore").tap();
  const frame = await demoFrame(page);
  const stage = await page.locator("[data-demo-stage]").boundingBox();
  expect(stage?.width).toBe(390);
  await toThread(frame);
  await expect(frame.locator('[data-testid="composer"]').first()).toBeVisible();
  await expect(frame.getByRole("button", { name: "Reset the demo" })).toBeVisible();
  await page.getByRole("button", { name: "Close (Esc)" }).tap();
  await expect(page.locator("[data-demo-stage]")).toHaveCSS("position", "absolute");
  await context.close();
});

test("demo AC5 the 13th message says Demo limit reached; Reset the demo brings the sample workspace back", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto("/demo/app/");
  const frame = page.mainFrame();
  await frame.waitForSelector('[data-testid="demo-bar"]');
  await sendReady(frame);
  await frame.getByRole("button", { name: "Deny" }).first().click();
  const box = frame.locator('[data-testid="composer"] textarea[aria-label="Message"]').first();
  for (let i = 2; i <= 12; i++) {
    await expect(box).toBeEnabled();
    await box.fill(`question ${i}`);
    await box.press("Enter");
    await expect(frame.getByText(`question ${i}`, { exact: true })).toBeVisible();
    await expect(page.locator('[data-testid="thread-working"]')).toHaveCount(0, { timeout: 15_000 });
  }
  await expect(page.locator('[data-testid="demo-limit"]')).toHaveCount(0);
  await box.fill("question 13");
  await box.press("Enter");
  await expect(page.getByText(/Demo limit reached/).first()).toBeVisible();
  await expect(page.locator('[data-testid="demo-limit"]')).toBeVisible();
  await expect(page.locator('[data-testid="demo-bar"]').getByRole("link", { name: "Download for Linux" })).toBeVisible();
  await page.getByRole("button", { name: "Reset the demo" }).click();
  await page.waitForSelector('[data-testid="demo-bar"]');
  await expect(page.locator('[data-testid="demo-limit"]')).toHaveCount(0);
  await toThread(frame);
  await expect(page.locator('[data-testid="composer"] textarea[aria-label="Message"]').first()).toHaveValue(/checkout sends some people back/);
  await expect(page.getByText("question 2", { exact: true })).toHaveCount(0);
});

test("demo AC6 the page and the demo request nothing beyond their own files and ask for no microphone or camera", async ({ page }) => {
  const origins = new Set<string>();
  page.on("request", (r) => origins.add(new URL(r.url()).origin));
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto("/");
  await page.locator(slot).scrollIntoViewIfNeeded();
  const frame = await demoFrame(page);
  await sendReady(frame);
  await frame.getByRole("button", { name: "Allow once" }).first().click();
  await expect(frame.getByText("Fixed the redirect loop.")).toBeVisible();
  await frame.locator("body").press("Control+2");
  await page.waitForTimeout(1500);
  expect([...origins].filter((o) => !o.startsWith("data:") && o !== "null")).toEqual([new URL(page.url()).origin]);
  // Nothing in the demo's code can open a microphone or camera.
  const assets = join(import.meta.dirname, "../out/demo/app/assets");
  for (const f of readdirSync(assets).filter((n) => n.endsWith(".js")))
    expect(readFileSync(join(assets, f), "utf8"), f).not.toMatch(/getUserMedia|getDisplayMedia/);
});
