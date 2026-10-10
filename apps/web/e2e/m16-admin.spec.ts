import { expect, test } from "@playwright/test";
import { signIn } from "./helpers";

test.describe("@m16 admin", () => {
  test("E2E F-BE-12 an admin finds a user and grants credits with a reason @m16", async ({ page }) => {
    await signIn(page, "admin@tandem.test", undefined, "/admin");
    await page.getByLabel("Find a user by email").fill("pro@tandem");
    await page.getByRole("button", { name: "Search" }).click();
    await page.getByTestId("admin-results").getByRole("button", { name: "Open" }).first().click();
    await expect(page.getByTestId("admin-user")).toHaveText("pro@tandem.test");
    const reason = `E2E goodwill ${Date.now()}`;
    await page.getByLabel("Reason (required for every action)").fill(reason);
    await page.getByLabel("Credits").fill("250");
    await page.getByRole("button", { name: "Grant credits" }).click();
    await expect(page.getByText("Granted 250 credits.")).toBeVisible();
    await expect(page.getByTestId("admin-ledger")).toContainText(`Added by support · ${reason}`);
    await expect(page.getByTestId("admin-actions")).toContainText(reason);
  });

  test("E2E F-BE-12 a non-admin sees no tools @m16", async ({ page }) => {
    await signIn(page, "pro@tandem.test", undefined, "/admin");
    await expect(page.getByText("This page is for Vibezeo support staff.")).toBeVisible();
    await expect(page.getByRole("button", { name: "Search" })).toHaveCount(0);
  });
});
