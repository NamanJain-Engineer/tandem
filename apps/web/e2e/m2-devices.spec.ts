import { createHash, randomBytes } from "node:crypto";
import { expect, test } from "@playwright/test";
import { API, signIn } from "./helpers";

const b64url = (b: Buffer) => b.toString("base64url");

// F-WB-05 AC1 (MILESTONES M2): revoking a desktop session on /devices revokes
// its refresh family, so that desktop's next refresh fails.
test.describe("devices", () => {
  test("E2E-M2-6w revoking a desktop on /devices ends its session @m2", async ({ page, request }) => {
    await signIn(page, "free@tandem.test", undefined, "/account");

    // A desktop signs in through the PKCE endpoints (as the app does).
    const verifier = b64url(randomBytes(48));
    const challenge = b64url(createHash("sha256").update(verifier).digest());
    const device = `E2E Laptop ${Date.now()}`;
    const fingerprint = createHash("sha256").update(device).digest("hex");
    const csrf = (await page.context().cookies()).find((c) => c.name === "tandem_csrf")?.value ?? "";
    const authorize = await page.request.post("/v1/auth/desktop/authorize", {
      headers: { "x-csrf-token": csrf },
      data: {
        challenge,
        state: b64url(randomBytes(32)),
        redirect_uri: "http://127.0.0.1:43123/callback",
        device_fingerprint: fingerprint,
        device_name: device,
        platform: "linux",
      },
    });
    expect(authorize.ok(), await authorize.text()).toBeTruthy();
    const { code } = (await authorize.json()) as { code: string };
    const exchange = await request.post(`${API}/v1/auth/desktop/exchange`, { data: { code, verifier, device_fingerprint: fingerprint } });
    expect(exchange.ok()).toBeTruthy();
    const tokens = (await exchange.json()) as { refresh_token: string };

    await page.goto("/devices");
    const row = page.getByRole("listitem").filter({ hasText: device });
    await expect(row).toBeVisible();
    await row.getByRole("button", { name: "Revoke" }).click();
    await expect(row).toHaveCount(0);

    const refresh = await request.post(`${API}/v1/auth/desktop/refresh`, { data: { refresh_token: tokens.refresh_token } });
    expect(refresh.status()).toBe(401);
  });
});
