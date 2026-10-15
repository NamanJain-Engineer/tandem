import { expect, test } from "@playwright/test";
import { DOCS_PAGES, docsPath } from "../lib/docs/pages";
import { VIDEO } from "../lib/media";
import { blogPages } from "../lib/blog/posts";
import { SITE_PAGES } from "../lib/site";

// F-WB-01 (M15): no analytics, ads or cookies on any page, so no consent is
// asked for (AC2, D-132); /download names the visitor's OS.

test.beforeEach(async ({ context }) => {
  // The test server streams the product video in full (no Range support).
  await context.route(/\.(webm|mp4)$/, (route) => route.fulfill({ status: 204, body: "" }));
});

test("F-WB-01 AC2 F-WB-02 F-WB-06 no page (docs included) requests another origin, sets a cookie or stores anything", async ({
  page,
  context,
}) => {
  const origins = new Set<string>();
  page.on("request", (r) => origins.add(new URL(r.url()).origin));
  for (const path of [...SITE_PAGES, ...blogPages(), ...DOCS_PAGES.map((p) => docsPath(p.slug))]) {
    await page.goto(path);
    await page.waitForLoadState("networkidle");
    const stored = await page.evaluate(() => [localStorage.length, sessionStorage.length, document.cookie]);
    expect(stored, path).toEqual([0, 0, ""]);
  }
  expect([...origins].filter((o) => o !== "null" && !o.startsWith("data:"))).toEqual([
    new URL(page.url()).origin,
  ]);
  expect(await context.cookies()).toEqual([]);
});

test("F-WB-01 AC1 the home page loads no video or poster, and /demo/ loads none of the video until it's played (D-1816, D-1841)", async ({
  page,
}) => {
  const asked: string[] = [];
  page.on("request", (r) => asked.push(new URL(r.url()).pathname));
  await page.goto("/");
  await page.waitForLoadState("networkidle");
  expect(await page.locator("video").count()).toBe(0);
  expect(asked).not.toContain(VIDEO.poster);
  await page.goto("/demo/");
  await page.waitForLoadState("networkidle");
  const video = page.locator("#video video");
  await expect(video).toHaveAttribute("preload", "none");
  await expect(video).toHaveAttribute("poster", VIDEO.poster);
  expect(asked.filter((p) => /\.(webm|mp4)$/.test(p))).toEqual([]);
});

for (const [os, ua, says] of [
  ["Linux", "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 Chrome/126 Safari/537.36", "You're on Linux"],
  [
    "macOS",
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 14_5) AppleWebKit/537.36 Chrome/126 Safari/537.36",
    "There's no macOS build yet",
  ],
  [
    "Windows",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/126 Safari/537.36",
    "There's no Windows build yet",
  ],
] as const) {
  test(`F-WB-01 /download names ${os} and keeps the Linux build the only button`, async ({ browser }) => {
    const context = await browser.newContext({ userAgent: ua });
    const page = await context.newPage();
    await page.addInitScript(() => {
      // Chromium reports its own platform through these; the UA under test decides.
      Object.defineProperty(navigator, "userAgentData", { get: () => undefined });
      Object.defineProperty(navigator, "platform", { get: () => "" });
    });
    await page.goto("/download/");
    await expect(page.locator("[data-os-note]")).toContainText(says);
    await expect(page.getByRole("link", { name: "Download for Linux (.deb)" })).toBeVisible();
    await context.close();
  });
}
