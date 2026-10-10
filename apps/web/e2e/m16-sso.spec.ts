import { expect, test } from "@playwright/test";
import { testRoute } from "./helpers";

const DOMAIN = "sso-e2e.test";
const ADA = `ada@${DOMAIN}`;

// E2E-M16-8 Team SSO (F-BL-06 P2): an org's members sign in through its
// identity provider (the test one), join on first sign-in, and can't use a
// password once SSO is required.
test.describe("@m16 Team single sign-on", () => {
  test.beforeEach(async ({ request }) => {
    await testRoute(request, "clock", { reset: true });
    await testRoute(request, "user/delete", { email: ADA });
    await testRoute(request, "team", { owner: "owner@tandem.test", members: [], seats: 3 });
    await testRoute(request, "sso", { email: "owner@tandem.test", domain: DOMAIN, enforce: true });
  });

  test("F-BL-06 SSO sign-in creates the account, joins the team and lands signed in @m16", async ({ page, request }) => {
    await testRoute(request, "idp/next", { email: ADA, name: "Ada Lovelace" });
    await page.goto("/signin?next=/team");
    await page.getByLabel("Email").fill(ADA);
    await page.getByTestId("sso-sign-in").click();
    await page.waitForURL("**/team");
    await expect(page.getByTestId("org-name")).toHaveText("Test team");
    await expect(page.getByTestId("org-summary")).toContainText("you're member");
    // The owner sees the connection, the verified domain and the sign-in in the log.
    await page.context().clearCookies();
    await page.goto("/signin?next=/team");
    await page.getByLabel("Email").fill("owner@tandem.test");
    await page.getByLabel("Password").fill("Tandem-dev-pass-1");
    await page.getByRole("button", { name: "Sign in", exact: true }).click();
    await page.waitForURL("**/team");
    await expect(page.getByTestId("sso-status")).toContainText(`required for ${DOMAIN}`);
    await expect(page.getByTestId("sso-domains")).toContainText("Verified");
    await expect(page.getByText("Signed in with SSO").first()).toBeVisible();
  });

  test("F-BL-06 with SSO required, a password sign-in for the domain is refused with the reason @m16", async ({ page }) => {
    await page.goto("/signin");
    await page.getByLabel("Email").fill(ADA);
    await page.getByLabel("Password").fill("Tandem-dev-pass-1");
    await page.getByRole("button", { name: "Sign in", exact: true }).click();
    await expect(page.getByText("Your organization signs in with single sign-on.")).toBeVisible();
  });

  test("F-BL-06 a failed SSO comes back to sign-in with an explanation @m16", async ({ page }) => {
    await page.goto("/signin?sso_error=SEATS_FULL");
    await expect(page.getByText("Your organization has no free seat. Ask an admin to add one.")).toBeVisible();
    await page.getByLabel("Email").fill("someone@nosso-e2e.test");
    await page.getByTestId("sso-sign-in").click();
    await expect(page.getByText("Single sign-on isn't set up for that address.")).toBeVisible();
  });
});
