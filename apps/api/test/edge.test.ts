import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { CaptchaVerifier } from "../src/providers/captcha";
import { bearerFor, json, makeTestApp, signUpVerified, signed, type TestEnv } from "./helpers";

let t: TestEnv;
beforeEach(async () => {
  t = await makeTestApp();
});
afterEach(async () => {
  await t.close();
});

/** A strict "edge rule": every unauthenticated challenge fails. */
const alwaysChallenge: CaptchaVerifier = { verify: async () => false };

describe("F-BE-10 rate limiting, abuse and edge", () => {
  it("F-BE-10.1 an authenticated desktop request is never served a challenge", async () => {
    const { userId } = await signUpVerified(t, "edge@tandem.test");
    t.deps.captcha = alwaysChallenge;
    const bearer = await bearerFor(t.deps, userId);
    const desktop = await json(t, "POST", "/v1/auth/sign-in/email", {
      bearer,
      body: { email: "edge@tandem.test", password: "Correct-horse-42" },
    });
    expect(desktop.status).not.toBe(403);
    const anonymous = await json(t, "POST", "/v1/auth/sign-in/email", {
      body: { email: "edge@tandem.test", password: "Correct-horse-42" },
    });
    expect(anonymous.status).toBe(403);
    expect(await anonymous.json()).toMatchObject({ code: "CAPTCHA_FAILED" });
    expect((await json(t, "GET", "/v1/me", { bearer })).status).toBe(200);
  });

  it("F-BE-10.2 a Stripe webhook with a valid signature passes from any IP", async () => {
    for (const ip of ["198.51.100.1", "10.0.0.9", "2001:db8::1"]) {
      const { body, signature } = signed(t, {
        id: `evt_ip_${ip}`,
        type: "ping",
        created: t.clock.nowSec(),
        data: { object: {} },
      });
      const res = await t.app.request("/v1/webhooks/stripe", {
        method: "POST",
        headers: { "stripe-signature": signature, "x-forwarded-for": ip },
        body,
      });
      expect(res.status, ip).toBe(200);
    }
  });

  it("per-IP limit is 300 requests per 60 s with Retry-After", async () => {
    t.clock.freeze();
    let last: Response | undefined;
    for (let i = 0; i < 301; i++) {
      last = await t.app.request("/v1/config", { headers: { "x-forwarded-for": "203.0.113.9" } });
    }
    expect(last?.status).toBe(429);
    expect(last?.headers.get("retry-after")).toBeTruthy();
    const other = await t.app.request("/v1/config", { headers: { "x-forwarded-for": "203.0.113.10" } });
    expect(other.status).toBe(200);
    t.clock.advance(1000);
    const refilled = await t.app.request("/v1/config", { headers: { "x-forwarded-for": "203.0.113.9" } });
    expect(refilled.status).toBe(200);
  });

  it("a clock that moves backwards doesn't lock clients out", async () => {
    t.clock.freeze();
    t.clock.advance(40 * 86_400_000);
    await t.app.request("/v1/config", { headers: { "x-forwarded-for": "203.0.113.20" } });
    t.clock.reset();
    const res = await t.app.request("/v1/config", { headers: { "x-forwarded-for": "203.0.113.20" } });
    expect(res.status).toBe(200);
  });

  it("every response carries x-tandem-min-version", async () => {
    const res = await t.app.request("/v1/health");
    expect(res.headers.get("x-tandem-min-version")).toBe("0.1.0");
  });

  it("/__test routes are absent without the flag", async () => {
    const plain = await makeTestApp({ TANDEM_API_TEST_ROUTES: "0" });
    expect((await plain.app.request("/__test/clock", { method: "POST", body: "{}" })).status).toBe(404);
    await plain.close();
    const prod = await makeTestApp({
      NODE_ENV: "production",
      TANDEM_API_TEST_ROUTES: "1",
      JWT_SIGNING_KEYS: "{}",
    });
    expect((await prod.app.request("/__test/clock", { method: "POST", body: "{}" })).status).toBe(404);
    await prod.close();
  });
});
