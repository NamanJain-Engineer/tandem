import { expect, test, type Page } from "@playwright/test";
import { createRequire } from "node:module";
import { DOCS_PAGES, docsPath } from "../lib/docs/pages";
import { SITE_PAGES } from "../lib/site";

// The redesign's quality bar (M15, D-1800–D-1899): WCAG 2.2 AA in both
// themes (axe), no sideways scrolling on a 320 px phone, and nothing moving
// for visitors who ask for reduced motion.

const AXE = createRequire(import.meta.url).resolve("axe-core/axe.min.js");
const PAGES = [...SITE_PAGES, "/changelog/0.5.0/", "/no-such-page/", ...DOCS_PAGES.slice(0, 3).map((p) => docsPath(p.slug))];

test.beforeEach(async ({ context }) => {
  // The test server streams the product video in full (no Range support), and
  // the demo is checked by demo-embed.spec.ts.
  await context.route(/\.(webm|mp4)$/, (route) => route.fulfill({ status: 204, body: "" }));
  await context.route(/\/demo\/app\//, (route) => route.fulfill({ status: 204, body: "" }));
});

type Violation = { id: string; impact: string | null; nodes: { target: string[] }[] };

async function axe(page: Page): Promise<string[]> {
  await page.addScriptTag({ path: AXE });
  const found = await page.evaluate(async () => {
    const w = window as unknown as { axe: { run: (o: object) => Promise<{ violations: Violation[] }> } };
    const r = await w.axe.run({ runOnly: { type: "tag", values: ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa"] } });
    return r.violations;
  });
  return found
    .filter((v) => v.impact === "serious" || v.impact === "critical")
    .map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).slice(0, 3).join(", ")}`);
}

for (const scheme of ["dark", "light"] as const) {
  test(`F-WB-01 every page passes axe (WCAG 2.2 AA) in the ${scheme} theme`, async ({ browser }) => {
    test.setTimeout(240_000);
    const context = await browser.newContext({ colorScheme: scheme, reducedMotion: "reduce" });
    await context.route(/\.(webm|mp4)$/, (route) => route.fulfill({ status: 204, body: "" }));
    await context.route(/\/demo\/app\//, (route) => route.fulfill({ status: 204, body: "" }));
    const page = await context.newPage();
    const bad: Record<string, string[]> = {};
    for (const path of PAGES) {
      await page.goto(path);
      const found = await axe(page);
      if (found.length) bad[path] = found;
    }
    expect(bad).toEqual({});
    await context.close();
  });
}

test("F-WB-01 no page scrolls sideways on a 320 px phone", async ({ page }) => {
  await page.setViewportSize({ width: 320, height: 640 });
  const wide: string[] = [];
  for (const path of PAGES) {
    await page.goto(path);
    const [scroll, width] = await page.evaluate(() => [document.documentElement.scrollWidth, window.innerWidth]);
    if (scroll > width) wide.push(`${path}: ${scroll} > ${width}`);
  }
  expect(wide).toEqual([]);
});

test("F-WB-01 with reduced motion nothing on the home page animates", async ({ browser }) => {
  const context = await browser.newContext({ reducedMotion: "reduce" });
  await context.route(/\.(webm|mp4)$/, (route) => route.fulfill({ status: 204, body: "" }));
  await context.route(/\/demo\/app\//, (route) => route.fulfill({ status: 204, body: "" }));
  const page = await context.newPage();
  await page.goto("/");
  await page.mouse.wheel(0, 3000);
  await page.waitForTimeout(300);
  const running = await page.evaluate(() =>
    document
      .getAnimations()
      .filter((a) => a.playState === "running" && (a.effect?.getComputedTiming().duration ?? 0) > 1)
      .map((a) => (a as CSSAnimation).animationName ?? "transition"),
  );
  expect(running).toEqual([]);
  await context.close();
});

test("F-WB-01 AC2 marketing pages load no framework JavaScript (D-1813), and still work", async ({ page }) => {
  const chunks: string[] = [];
  page.on("request", (r) => {
    const path = new URL(r.url()).pathname;
    if (path.startsWith("/_next/static/chunks/")) chunks.push(path);
  });
  for (const path of SITE_PAGES) {
    await page.goto(path);
    await page.waitForLoadState("networkidle");
  }
  expect(chunks).toEqual([]);
  // What used to need a script works without one: the phone menu and the FAQ.
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/");
  await page.getByRole("button", { name: "Menu" }).click();
  await expect(page.locator("#site-menu")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.locator("#site-menu")).toBeHidden();
  const second = page.locator("details[name=faq]").nth(1);
  await second.locator("summary").click();
  await expect(second).toHaveAttribute("open", "");
  await expect(page.locator("details[name=faq]").first()).not.toHaveAttribute("open", "");
});
