import { expect, test } from "@playwright/test";

// F-WB-02 (D-1817, D-1843): the docs are static pages served at slash URLs
// with no JavaScript but the inline theme script (D-1871). A link without its slash answered with a redirect
// (D-1817); every docs link carries its slash, and moving between pages is
// a plain navigation (the site's cross-document view transition), with no
// failed request or redirect.

test("F-WB-02 docs links open the next page with no failed request or redirect, and the docs run no script but the theme's (D-1817, D-1843, D-1871)", async ({
  page,
}) => {
  const bad: string[] = [];
  const scripts: string[] = [];
  page.on("response", (r) => {
    if (r.status() >= 300) bad.push(`${r.status()} ${new URL(r.url()).pathname}`);
  });
  page.on("request", (r) => {
    if (r.resourceType() === "script") scripts.push(new URL(r.url()).pathname);
  });
  page.on("requestfailed", (r) => bad.push(`failed ${new URL(r.url()).pathname}`));
  await page.goto("/docs/");
  await page.waitForLoadState("networkidle");
  // Only the inline theme script (D-1871): nothing is downloaded to run.
  expect(await page.locator("script").count()).toBe(1);
  expect(await page.locator("head > script[data-theme-boot]:not([src])").count()).toBe(1);

  const link = page.locator("#nd-sidebar").getByRole("link", { name: "Code mode", exact: true });
  await expect(link).toHaveAttribute("href", "/docs/code-mode/");
  await link.click();
  await expect(page).toHaveURL(/\/docs\/code-mode\/$/);
  await expect(page.getByRole("heading", { level: 1, name: "Code mode" })).toBeVisible();
  await expect(page.locator("#nd-sidebar").getByRole("link", { name: "Code mode", exact: true })).toHaveAttribute(
    "data-active",
    "true",
  );

  await page.waitForLoadState("networkidle");
  expect(bad).toEqual([]);
  expect(scripts).toEqual([]);
});
