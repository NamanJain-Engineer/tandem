import { expect, test } from "@playwright/test";
import { API, clearMail, latestMail, me, signIn, testRoute } from "./helpers";

test.describe("@m1 accounts and billing", () => {
  test.beforeEach(async ({ request }) => {
    await testRoute(request, "clock", { reset: true });
  });

  test("E2E-M1-1 sign-up and verify @m1", async ({ page, request }) => {
    const email = "new@tandem.test";
    await testRoute(request, "user/delete", { email });
    await clearMail(request, email);

    await page.goto("/signup");
    await page.getByLabel("Name").fill("New User");
    await page.getByLabel("Email").fill(email);
    await page.getByLabel("Password (at least 10 characters)").fill("Correct-horse-42");
    await page.getByRole("button", { name: "Create account" }).click();
    await expect(page.getByText("Check your email")).toBeVisible();

    const mail = await latestMail(request, email);
    expect(mail.Subject).toBe("Verify your email for Tandem");
    const link = mail.Text.match(/https?:\/\/\S+/)?.[0];
    expect(link).toBeTruthy();
    await page.goto(link ?? "");
    await expect(page.getByRole("heading", { name: "Your email is verified" })).toBeVisible();

    await signIn(page, email, "Correct-horse-42");
    await expect(page.getByTestId("account-email")).toHaveText(email);

    // A breached password is rejected.
    await testRoute(request, "user/delete", { email: "breached@tandem.test" });
    await page.goto("/signup");
    await page.getByLabel("Name").fill("B");
    await page.getByLabel("Email").fill("breached@tandem.test");
    await page.getByLabel("Password (at least 10 characters)").fill("password123");
    await page.getByRole("button", { name: "Create account" }).click();
    await expect(page.getByRole("alert").filter({ hasText: "data breach" })).toBeVisible();

    // A cookie-authenticated write without the CSRF header is refused.
    await signIn(page, email, "Correct-horse-42");
    const status = await page.evaluate(async () => {
      const res = await fetch("/v1/auth/change-password", {
        method: "POST",
        credentials: "include",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ currentPassword: "Correct-horse-42", newPassword: "Another-horse-43" }),
      });
      return res.status;
    });
    expect(status).toBe(403);
  });

  test("E2E-M1-2 mock upgrade @m1", async ({ page, request }) => {
    await testRoute(request, "plan", { email: "free@tandem.test", plan: "free" });
    await signIn(page, "free@tandem.test", undefined, "/billing");
    await page.evaluate(() => {
      const w = window as unknown as { __events: string[]; __started: number };
      w.__events = [];
      const source = new EventSource("/v1/me/events", { withCredentials: true });
      source.addEventListener("entitlement", () => w.__events.push(`entitlement@${Date.now()}`));
      w.__started = Date.now();
    });
    await page.getByRole("button", { name: /Upgrade to Pro/ }).click();
    await page.waitForURL("**/billing/mock?session=*");
    const clicked = Date.now();
    await page.getByRole("button", { name: "Complete purchase" }).click();
    await expect(page.getByRole("heading", { name: "You're on Pro" })).toBeVisible();
    await expect(page.getByRole("link", { name: "Open Tandem" })).toHaveAttribute("href", "tandem://");
    expect((await me(page)).plan).toBe("pro");
    expect(Date.now() - clicked).toBeLessThan(5000);
    await testRoute(request, "plan", { email: "free@tandem.test", plan: "free" });
  });

  test("E2E-M1-2b the SSE invalidation arrives within 5 s @m1", async ({ page, request }) => {
    await testRoute(request, "plan", { email: "free@tandem.test", plan: "free" });
    await signIn(page, "free@tandem.test", undefined, "/billing");
    await page.evaluate(
      () =>
        new Promise<void>((resolve) => {
          const w = window as unknown as { __got: number | null };
          w.__got = null;
          const source = new EventSource("/v1/me/events", { withCredentials: true });
          source.addEventListener("ready", () => resolve());
          source.addEventListener("entitlement", () => {
            w.__got = Date.now();
          });
        }),
    );
    const start = Date.now();
    await testRoute(request, "plan", { email: "free@tandem.test", plan: "pro" });
    await expect
      .poll(() => page.evaluate(() => (window as unknown as { __got: number | null }).__got), {
        timeout: 5000,
      })
      .not.toBeNull();
    expect(Date.now() - start).toBeLessThan(5000);
    await testRoute(request, "plan", { email: "free@tandem.test", plan: "free" });
  });

  test("E2E-M1-3 cancel at period end @m1", async ({ page, request }) => {
    await page.request.post(`${API}/__test/clock`, { data: { reset: true } });
    await signIn(page, "pro@tandem.test", undefined, "/billing");
    await page.goto("/billing");
    await page.getByRole("button", { name: "Manage billing" }).click();
    await page.waitForURL("**/billing/mock-portal**");
    await page.getByRole("button", { name: "Cancel at period end" }).click();
    await page.waitForURL("**/billing");
    await expect(page.getByTestId("plan-line")).toContainText("Pro until");
    await testRoute(request, "clock", { advanceMs: 40 * 86_400_000 });
    const after = await me(page);
    expect(after.plan).toBe("free");
    expect(after.limits.panes).toBe(4);
    await testRoute(request, "clock", { reset: true });
  });

  test("E2E-M1-4 dunning @m1", async ({ page, request }) => {
    const email = "pastdue@tandem.test";
    await clearMail(request, email);
    const { execFileSync } = await import("node:child_process");
    execFileSync(
      "bash",
      ["../../scripts/stripe-fixture.sh", "invoice.payment_failed", email, String(Date.now())],
      { stdio: "inherit" },
    );
    const mail = await latestMail(request, email);
    expect(mail.Subject).toBe("Payment failed — update your card");
    await signIn(page, email);
    expect((await me(page)).plan).toBe("pro");
    await testRoute(request, "clock", { advanceMs: 8 * 86_400_000 });
    const after = await me(page);
    expect(after.plan).toBe("free");
    await expect(page.getByTestId("account-email")).toHaveText(email);
    await testRoute(request, "clock", { reset: true });
  });

  test("E2E-M1-5 webhook idempotency @m1", async ({ page }) => {
    const { execFileSync } = await import("node:child_process");
    const nonce = String(Date.now());
    const run = (event: string) =>
      execFileSync("bash", ["../../scripts/stripe-fixture.sh", event, "pro@tandem.test", nonce], {
        stdio: "inherit",
      });
    await signIn(page, "pro@tandem.test");
    const before = await me(page);
    run("checkout.session.completed");
    run("checkout.session.completed");
    const after = await me(page);
    expect(after.credits.purchased - before.credits.purchased).toBe(5000);
    run("customer.subscription.created");
    run("customer.subscription.created");
    const ledger = await page.evaluate(
      async () =>
        (await (await fetch("/v1/credits")).json()).ledger as Array<{ reason: string; createdAt: string }>,
    );
    const sinceStart = ledger.filter(
      (l) => l.reason === "pack_purchase" && Date.parse(l.createdAt) >= Number(nonce) - 1000,
    );
    expect(sinceStart).toHaveLength(1);
    const subs = await page.request.get("http://127.0.0.1:8787/__test/subscriptions?email=pro@tandem.test");
    expect((await subs.json()).count).toBe(1);
  });
});
