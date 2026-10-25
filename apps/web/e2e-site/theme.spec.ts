import { expect, test, type Browser, type Page } from "@playwright/test";
import { createRequire } from "node:module";
import { DOCS_PAGES, docsPath } from "../lib/docs/pages";
import { blogPages } from "../lib/blog/posts";
import { SITE_PAGES } from "../lib/site";
import { THEME_KEY } from "../lib/theme";

// The theme choice (F-WB-01 AC4–AC7, D-1870, D-1871) in a real browser,
// against the exported site.

const AXE = createRequire(import.meta.url).resolve("axe-core/axe.min.js");
const PAGES = [...SITE_PAGES, ...blogPages(), "/changelog/0.5.0/", "/404.html", ...DOCS_PAGES.slice(0, 3).map((p) => docsPath(p.slug))];
// The test server answers a missing page with its own 404, so the site's is
// opened by its file.
// The page backgrounds (--site-page) of each theme.
const BG = { dark: "rgb(9, 9, 11)", light: "rgb(250, 250, 250)" }; // ink-950 and ink-25 (T8, D-1878, D-1883)

async function context(browser: Browser, os: "dark" | "light", stored?: "dark" | "light", javaScriptEnabled = true) {
  const ctx = await browser.newContext({ colorScheme: os, reducedMotion: "reduce", javaScriptEnabled });
  await ctx.route(/\.(webm|mp4)$/, (route) => route.fulfill({ status: 204, body: "" }));
  await ctx.route(/\/demo\/app\//, (route) => route.fulfill({ status: 204, body: "" }));
  if (stored)
    await ctx.addInitScript(([k, v]) => {
      try {
        if (!sessionStorage.getItem("seeded")) {
          localStorage.setItem(k as string, v as string);
          sessionStorage.setItem("seeded", "1");
        }
      } catch {
        /* about:blank */
      }
    }, [THEME_KEY, stored]);
  return ctx;
}

const bodyBg = (page: Page) => page.evaluate(() => getComputedStyle(document.body).backgroundColor);

test("F-WB-01 AC4 a stored choice is on <html> before the first frame, so nothing paints in the OS's theme", async ({ browser }) => {
  for (const [os, stored] of [["light", "dark"], ["dark", "light"]] as const) {
    const ctx = await context(browser, os, stored);
    // The first animation frame runs before the first paint.
    await ctx.addInitScript(() => {
      requestAnimationFrame(() => {
        const w = window as unknown as { firstFrame: unknown };
        w.firstFrame = {
          choice: document.documentElement.getAttribute("data-theme-choice"),
          bg: document.body ? getComputedStyle(document.body).backgroundColor : getComputedStyle(document.documentElement).backgroundColor,
          scheme: getComputedStyle(document.documentElement).colorScheme,
        };
      });
    });
    const page = await ctx.newPage();
    for (const path of ["/", "/download/", "/docs/", "/blog/"]) {
      await page.goto(path);
      const first = await page.evaluate(() => (window as unknown as { firstFrame: unknown }).firstFrame);
      expect(first, `${path} OS ${os}, stored ${stored}`).toEqual({ choice: stored, bg: BG[stored], scheme: stored });
    }
    await ctx.close();
  }
});

test("F-WB-01 AC4 F-WB-01 AC7 the header's control switches the page at once and keeps the choice, under one key", async ({ browser }) => {
  const ctx = await context(browser, "dark");
  const page = await ctx.newPage();
  await page.goto("/");
  expect(await page.evaluate(() => document.documentElement.getAttribute("data-theme-choice"))).toBe("system");
  expect(await bodyBg(page)).toBe(BG.dark);
  const header = page.locator(".site-header fieldset.theme-switch").first();
  await expect(header).toBeVisible();
  await header.locator('label[data-choice="light"]').click();
  expect(await bodyBg(page)).toBe(BG.light);
  await expect(header.locator('input[value="light"]')).toBeChecked();
  expect(await page.evaluate(() => ({ ...localStorage }))).toEqual({ [THEME_KEY]: "light" });
  // Another page and a reload keep it.
  await page.goto("/about/");
  expect(await bodyBg(page)).toBe(BG.light);
  await page.reload();
  expect(await bodyBg(page)).toBe(BG.light);
  await expect(page.locator('.site-header input[name="theme"][value="light"]')).toBeChecked();
  // The keyboard works it too: arrows move between the radios.
  await page.locator('.site-header input[name="theme"][value="light"]').focus();
  await page.keyboard.press("ArrowRight");
  expect(await bodyBg(page)).toBe(BG.dark);
  expect(await page.evaluate(() => document.documentElement.getAttribute("data-theme"))).toBe("dark");
  // System forgets the choice and follows the OS again.
  await header.locator('label[data-choice="system"]').click();
  expect(await page.evaluate(() => localStorage.length)).toBe(0);
  expect(await page.evaluate(() => document.documentElement.hasAttribute("data-theme"))).toBe(false);
  await ctx.close();
});

test("F-WB-01 AC4 keyboard focus shows on the theme control's cell, not only on its hidden radio (WCAG 2.4.7)", async ({ browser }) => {
  const ctx = await context(browser, "dark");
  const page = await ctx.newPage();
  await page.goto("/");
  const cell = page.locator('.site-header > nav > fieldset label[data-choice="system"]');
  const look = () => cell.evaluate((el) => { const s = getComputedStyle(el); return `${s.outlineStyle} ${s.outlineWidth} ${s.outlineColor}`; });
  const before = await look();
  // Tab from the element just before the control lands on its checked radio.
  await page.locator(".site-header > nav > ul a").last().focus();
  await page.keyboard.press("Tab");
  await expect(page.locator('.site-header > nav > fieldset input[value="system"]')).toBeFocused();
  const after = await look();
  expect(after).not.toBe(before);
  expect(after).toMatch(/^solid 2px/);
  // The theme cells are icon-only in the header: each says what it is.
  for (const [v, t] of [["system", "System theme"], ["light", "Light theme"], ["dark", "Dark theme"]])
    await expect(page.locator(`.site-header > nav > fieldset label[data-choice="${v}"]`)).toHaveAttribute("title", t);
  await ctx.close();
});

test("F-WB-01 AC4 phones pick the theme from the menu", async ({ browser }) => {
  const ctx = await context(browser, "light");
  const page = await ctx.newPage();
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/");
  await expect(page.locator(".site-header > nav > fieldset.theme-switch")).toBeHidden();
  await page.getByRole("button", { name: "Menu" }).click();
  const menu = page.locator("#site-menu");
  await menu.locator('label[data-choice="dark"]').click();
  expect(await bodyBg(page)).toBe(BG.dark);
  await expect(menu.locator('input[value="dark"]')).toBeChecked();
  await ctx.close();
});

test("F-WB-01 AC4 without JavaScript the page follows the OS and shows no control", async ({ browser }) => {
  for (const os of ["dark", "light"] as const) {
    const ctx = await context(browser, os, undefined, false);
    const page = await ctx.newPage();
    await page.goto("/");
    expect(await bodyBg(page)).toBe(BG[os]);
    await expect(page.locator("fieldset.theme-switch").first()).toBeHidden();
    await ctx.close();
  }
});

test("F-WB-01 AC6 a forced theme reaches the docs' and the blog's code blocks", async ({ browser }) => {
  const span = (page: Page) => page.evaluate(() => getComputedStyle(document.querySelector("pre span[style]") as Element).color);
  for (const path of [docsPath("code-mode"), "/blog/whats-new-in-0-8-0/"]) {
    const colours: Record<string, string> = {};
    for (const [os, stored] of [["dark", undefined], ["dark", "light"], ["light", undefined], ["light", "dark"]] as const) {
      const ctx = await context(browser, os, stored);
      const page = await ctx.newPage();
      await page.goto(path);
      colours[`${os}/${stored ?? "system"}`] = await span(page);
      await ctx.close();
    }
    expect(colours["dark/light"], path).toBe(colours["light/system"]);
    expect(colours["light/dark"], path).toBe(colours["dark/system"]);
    expect(colours["dark/system"], path).not.toBe(colours["light/system"]);
  }
});

type Violation = { id: string; impact: string | null; nodes: { target: string[] }[] };

for (const [os, stored] of [["dark", "light"], ["light", "dark"]] as const) {
  test(`F-WB-01 AC5 every page passes axe (WCAG 2.2 AA) in ${stored}, chosen over an OS in ${os}`, async ({ browser }) => {
    test.setTimeout(240_000);
    const ctx = await context(browser, os, stored);
    const page = await ctx.newPage();
    const bad: Record<string, string[]> = {};
    for (const path of PAGES) {
      await page.goto(path);
      expect(await page.evaluate(() => document.documentElement.getAttribute("data-theme")), path).toBe(stored);
      await page.addScriptTag({ path: AXE });
      const found = await page.evaluate(async () => {
        const w = window as unknown as { axe: { run: (o: object) => Promise<{ violations: Violation[] }> } };
        const r = await w.axe.run({ runOnly: { type: "tag", values: ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa"] } });
        return r.violations;
      });
      const serious = found
        .filter((v) => v.impact === "serious" || v.impact === "critical")
        .map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).slice(0, 3).join(", ")}`);
      if (serious.length) bad[path] = serious;
    }
    expect(bad).toEqual({});
    await ctx.close();
  });
}
