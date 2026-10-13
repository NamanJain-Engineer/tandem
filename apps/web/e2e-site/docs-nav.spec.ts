import { expect, test } from "@playwright/test";

// F-WB-02 (D-1817): the docs are a client app in a static export served at
// slash URLs. A link without its slash made Next fetch /docs/x.txt, which
// doesn't exist: every prefetch failed, and every click fell back to a full
// page load through a redirect.

test("F-WB-02 docs links prefetch and open in the page, with no failed request or redirect (D-1817)", async ({
  page,
}) => {
  const bad: string[] = [];
  page.on("response", (r) => {
    if (r.status() >= 300) bad.push(`${r.status()} ${new URL(r.url()).pathname}`);
  });
  await page.goto("/docs/");
  await page.waitForLoadState("networkidle");
  // Survives a navigation in the page; a full load drops it.
  await page.evaluate(() => Object.assign(window, { stayed: true }));

  const link = page.locator("#nd-sidebar").getByRole("link", { name: "Code mode", exact: true });
  await expect(link).toHaveAttribute("href", "/docs/code-mode/");
  await link.click();
  await expect(page).toHaveURL(/\/docs\/code-mode\/$/);
  await expect(page.getByRole("heading", { level: 1, name: "Code mode" })).toBeVisible();
  await expect(link).toHaveAttribute("data-active", "true");
  expect(await page.evaluate(() => (window as { stayed?: boolean }).stayed)).toBe(true);

  await page.waitForLoadState("networkidle");
  expect(bad).toEqual([]);
});
