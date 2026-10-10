import { expect, test } from "@playwright/test";
import { API, signIn } from "./helpers";

test.describe("@m16 device-code sign-in", () => {
  test("E2E F-BE-03 a CLI's code is approved on /device and the CLI gets tokens @m16", async ({ page, request }) => {
    const start = await request.post(`${API}/v1/auth/device/start`, {
      data: { client: "Vibezeo CLI", version: "0.3.0" },
      headers: { "x-forwarded-for": `198.51.100.${Math.floor(Math.random() * 200) + 1}` },
    });
    expect(start.ok()).toBeTruthy();
    const s = (await start.json()) as { device_code: string; user_code: string; verification_uri_complete: string };

    const pending = await request.post(`${API}/v1/auth/device/token`, { data: { device_code: s.device_code } });
    expect((await pending.json()).error).toBe("authorization_pending");

    await signIn(page, "pro@tandem.test", undefined, "/account");
    const url = new URL(s.verification_uri_complete);
    await page.goto(url.pathname + url.search);
    await expect(page.getByTestId("device-client")).toHaveText("Vibezeo CLI 0.3.0 wants to sign in as pro@tandem.test.");
    await page.getByRole("button", { name: "Approve" }).click();
    await expect(page.getByRole("status").filter({ hasText: "Approved" })).toBeVisible();

    // The CLI waits out its 5 s interval (polling sooner is slow_down) and gets tokens.
    await page.waitForTimeout(5_500);
    const token = await request.post(`${API}/v1/auth/device/token`, { data: { device_code: s.device_code } });
    expect(token.status()).toBe(200);
    expect((await token.json()).token_type).toBe("Bearer");
  });
});
