import { expect, test } from "@playwright/test";
import { API, clearMail, latestMail, me, signIn, testRoute } from "./helpers";

type Credits = { balance: { monthly: number; purchased: number }; ledger: Array<{ reason: string }> };

async function credits(page: import("@playwright/test").Page): Promise<Credits> {
  return page.evaluate(async () => (await fetch("/v1/credits", { credentials: "include" })).json());
}

test.describe("@m13 hosted services, credits and Team", () => {
  test.beforeEach(async ({ request }) => {
    await testRoute(request, "clock", { reset: true });
  });

  test("E2E-M13-3 packs: buy 5,000 on /credits, replay the webhook, +5,000 exactly once @m13", async ({ page, request }) => {
    await testRoute(request, "plan", { email: "pro@tandem.test", plan: "pro" });
    await signIn(page, "pro@tandem.test", undefined, "/credits");
    const before = await credits(page);
    await page.getByTestId("pack-5k").click();
    await page.waitForURL("**/billing/mock?session=*");
    await page.getByRole("button", { name: "Complete purchase" }).click();
    await page.waitForURL("**/credits?purchased=5k");
    await expect(page.getByRole("status").filter({ hasText: "5,000 credits" })).toBeVisible();
    await expect(page.getByTestId("credits-purchased")).toHaveText(
      new Intl.NumberFormat("en-US").format(before.balance.purchased + 5_000),
    );
    // A replayed checkout.session.completed (the Stripe fixture, twice) changes nothing more.
    const { execFileSync } = await import("node:child_process");
    const nonce = String(Date.now());
    for (let i = 0; i < 2; i++) {
      execFileSync("bash", ["../../scripts/stripe-fixture.sh", "checkout.session.completed", "pro@tandem.test", nonce], {
        stdio: "inherit",
      });
    }
    const after = await credits(page);
    expect(after.balance.purchased - before.balance.purchased).toBe(10_000);
    await expect(page.getByTestId("credits-ledger")).toContainText("Credit pack");
  });

  test("E2E-M13-3b Free accounts are sent to plans; Team members to their admin @m13", async ({ page, request }) => {
    await testRoute(request, "plan", { email: "free@tandem.test", plan: "free" });
    await testRoute(request, "team", { owner: "owner@tandem.test", members: ["member@tandem.test"] });
    await signIn(page, "free@tandem.test", undefined, "/credits");
    await expect(page.getByText("Credit packs come with Pro and Team.")).toBeVisible();
    await signIn(page, "member@tandem.test", undefined, "/credits");
    await expect(page.getByTestId("packs-ask-admin")).toBeVisible();
    await expect(page.getByText("Your organization's pool")).toBeVisible();
  });

  test("E2E-M13-4 Team: invite through mailpit, policy Edits, a 1,000 cap, a seat change @m13", async ({ browser, page, request }) => {
    const invitee = "invitee@tandem.test";
    await testRoute(request, "user/delete", { email: invitee });
    await clearMail(request, invitee);
    await testRoute(request, "team", { owner: "owner@tandem.test", members: [], seats: 3 });

    await signIn(page, "owner@tandem.test", undefined, "/team");
    await expect(page.getByTestId("org-summary")).toContainText("1 of 3 seats used");
    await page.getByLabel("Email").fill(invitee);
    await page.getByRole("button", { name: "Send invite" }).click();
    await expect(page.getByText(`Invite sent to ${invitee}.`)).toBeVisible();
    await expect(page.getByTestId("invites")).toContainText(invitee);

    const mail = await latestMail(request, invitee);
    expect(mail.Subject).toContain("invited");
    const link = mail.Text.match(/https?:\/\/\S+token=[\w-]+/)?.[0] ?? "";
    expect(link).toContain("/team/join?token=");

    // The invitee signs up (verified by the test route), signs in and follows the link.
    const other = await (await browser.newContext()).newPage();
    await other.goto("/signup");
    await other.getByLabel("Name").fill("Invitee");
    await other.getByLabel("Email").fill(invitee);
    await other.getByLabel("Password (at least 10 characters)").fill("Correct-horse-42");
    await other.getByRole("button", { name: "Create account" }).click();
    await expect(other.getByText("Check your email")).toBeVisible();
    const verify = (await latestMail(request, invitee)).Text.match(/https?:\/\/\S+/)?.[0] ?? "";
    await other.goto(verify);
    await signIn(other, invitee, "Correct-horse-42", "/account");
    await other.goto(new URL(link).pathname + new URL(link).search);
    await expect(other.getByTestId("joined")).toBeVisible();
    expect((await me(other)).plan).toBe("team");

    // The owner sets max access Edits and a 1,000 cap for the new member.
    await page.reload();
    await page.getByRole("combobox", { name: "Highest access level" }).click();
    await page.getByRole("option", { name: "Edits" }).click();
    await page.getByRole("button", { name: "Save policy" }).click();
    await expect(page.getByText("Policy saved.")).toBeVisible();
    const row = page.getByTestId(`member-${invitee}`);
    await row.getByLabel(`Monthly cap for ${invitee}`).fill("1000");
    await row.getByRole("button", { name: "Save" }).click();
    await expect(page.getByText("Cap saved.")).toBeVisible();

    const view = (await other.evaluate(async () => (await fetch("/v1/me", { credentials: "include" })).json())) as {
      policy: { maxAccess: string };
      credits: { cap: number };
    };
    expect(view.policy.maxAccess).toBe("edits");
    expect(view.credits.cap).toBe(1000);
    await other.goto("/team");
    await expect(other.getByTestId("policy-summary")).toContainText("Highest access level: Edits");
    await expect(other.getByTestId("my-cap")).toContainText("0 of 1,000");

    // Seats: 3 → 4 updates the subscription's quantity at the billing provider.
    await page.getByLabel("Seats").fill("4");
    await page.getByRole("button", { name: "Change seats" }).click();
    await expect(page.getByText("Seats updated.")).toBeVisible();
    const calls = (await (await request.get(`${API}/__test/billing-calls`)).json()) as { calls: string[] };
    expect(calls.calls.some((c) => /^quantity:sub_test_.*:4$/.test(c))).toBe(true);
    await expect(page.getByTestId("audit-log")).toContainText("Seats changed: 3 → 4");
    await expect(page.getByTestId("audit-log")).toContainText("Policy changed");
  });

  test("E2E-M13-7 refund: an admin refund gives Free limits and a monthly bucket of 0 @m13", async ({ page, request }) => {
    const email = "lapsed@tandem.test";
    await testRoute(request, "plan", { email, plan: "pro", paid: true });
    await testRoute(request, "credits", { email, monthly: 10_000 });
    const id = ((await (await request.get(`${API}/__test/user?email=${email}`)).json()) as { id: string }).id;
    await signIn(page, "admin@tandem.test");
    const status = await page.evaluate(async (target) => {
      await fetch("/v1/health", { credentials: "include" });
      const csrf = document.cookie.split("; ").find((c) => c.startsWith("tandem_csrf="))?.split("=")[1] ?? "";
      const res = await fetch(`/v1/admin/users/${target}/refund`, {
        method: "POST",
        credentials: "include",
        headers: { "content-type": "application/json", "x-csrf-token": csrf },
        body: JSON.stringify({ reason: "E2E refund within 14 days" }),
      });
      return res.status;
    }, id);
    expect(status).toBe(200);
    await signIn(page, email);
    const view = await me(page);
    expect(view.plan).toBe("free");
    expect(view.limits.panes).toBe(4);
    expect(view.credits.monthly).toBe(0);
    // Back to the seed's lapsed subscription (ended 10 days ago).
    await testRoute(request, "plan", { email, plan: "pro", status: "canceled", periodEndDays: -10 });
  });

  test("F-TL-07 the transparency page lists every hosted feature and the subprocessors @m13", async ({ page }) => {
    await page.goto("/transparency");
    const table = page.getByTestId("hosted-features");
    await expect(table).toContainText("Cloud dictation");
    await expect(table).toContainText("Audio of each take");
    await expect(table).toContainText("Vibezeo API → speech-to-text provider");
    await expect(page.getByTestId("subprocessors")).toContainText("Anthropic (Claude API)");
  });
});
