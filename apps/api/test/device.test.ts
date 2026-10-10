import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { normalizeUserCode, USER_CODE_ALPHABET } from "../src/routes/deviceAuth";
import { json, makeTestApp, signUpVerified, type TestEnv } from "./helpers";

let t: TestEnv;
beforeEach(async () => {
  t = await makeTestApp();
});
afterEach(async () => {
  await t.close();
});

type Start = {
  device_code: string;
  user_code: string;
  verification_uri: string;
  verification_uri_complete: string;
  interval: number;
  expires_in: number;
};

async function start(ip = "198.51.100.7"): Promise<Start> {
  const res = await json(t, "POST", "/v1/auth/device/start", {
    body: { client: "Vibezeo CLI", version: "0.3.0" },
    headers: { "x-forwarded-for": ip },
  });
  expect(res.status).toBe(200);
  return (await res.json()) as Start;
}

const poll = (deviceCode: string) => json(t, "POST", "/v1/auth/device/token", { body: { device_code: deviceCode } });

describe("F-BE-03 CLI device-code sign-in", () => {
  it("F-BE-03.1 start → approve → token returns tokens that work, once", async () => {
    const s = await start();
    expect(s.user_code).toMatch(new RegExp(`^[${USER_CODE_ALPHABET}]{4}-[${USER_CODE_ALPHABET}]{4}$`));
    expect(s).toMatchObject({ interval: 5, expires_in: 600, verification_uri: "http://localhost:3000/device" });
    expect(s.verification_uri_complete).toBe(`http://localhost:3000/device?code=${s.user_code}`);

    const pending = await poll(s.device_code);
    expect(pending.status).toBe(400);
    expect(await pending.json()).toMatchObject({ error: "authorization_pending" });

    const { jar } = await signUpVerified(t, "cli@tandem.test");
    const info = await json(t, "GET", `/v1/auth/device/info?user_code=${s.user_code.toLowerCase().replace("-", " ")}`, {
      jar,
    });
    expect(await info.json()).toMatchObject({ user_code: s.user_code, client: "Vibezeo CLI", version: "0.3.0" });
    const approve = await json(t, "POST", "/v1/auth/device/approve", {
      jar,
      body: { user_code: s.user_code, approve: true },
    });
    expect(approve.status).toBe(204);

    t.clock.advance(6_000);
    const res = await poll(s.device_code);
    expect(res.status).toBe(200);
    const tokens = (await res.json()) as { access_token: string; refresh_token: string; token_type: string };
    expect(tokens.token_type).toBe("Bearer");
    const me = await json(t, "GET", "/v1/me", { bearer: tokens.access_token });
    expect(me.status).toBe(200);
    expect(((await me.json()) as { user: { email: string } }).user.email).toBe("cli@tandem.test");
    // The session shows on the Devices page under the client's name.
    const devices = (await (await json(t, "GET", "/v1/me/sessions", { jar })).json()) as Array<{
      deviceName: string;
      platform: string;
    }>;
    expect(devices).toContainEqual(expect.objectContaining({ deviceName: "Vibezeo CLI", platform: "cli" }));

    t.clock.advance(6_000);
    const again = await poll(s.device_code);
    expect(await again.json()).toMatchObject({ error: "invalid_grant" });
  });

  it("F-BE-03.1 polling faster than the interval returns slow_down and widens it", async () => {
    const s = await start();
    expect(await (await poll(s.device_code)).json()).toMatchObject({ error: "authorization_pending" });
    t.clock.advance(2_000);
    expect(await (await poll(s.device_code)).json()).toMatchObject({ error: "slow_down", interval: 10 });
    t.clock.advance(6_000);
    expect(await (await poll(s.device_code)).json()).toMatchObject({ error: "slow_down", interval: 15 });
    t.clock.advance(16_000);
    expect(await (await poll(s.device_code)).json()).toMatchObject({ error: "authorization_pending" });
  });

  it("F-BE-03.1 an expired code returns expired_token, and can't be approved", async () => {
    const s = await start();
    t.clock.advance(10 * 60_000 + 1_000);
    expect(await (await poll(s.device_code)).json()).toMatchObject({ error: "expired_token" });
    const { jar } = await signUpVerified(t, "late@tandem.test");
    const approve = await json(t, "POST", "/v1/auth/device/approve", {
      jar,
      body: { user_code: s.user_code, approve: true },
    });
    expect(approve.status).toBe(404);
  });

  it("a denied code returns access_denied", async () => {
    const s = await start();
    const { jar } = await signUpVerified(t, "deny@tandem.test");
    await json(t, "POST", "/v1/auth/device/approve", { jar, body: { user_code: s.user_code, approve: false } });
    t.clock.advance(6_000);
    expect(await (await poll(s.device_code)).json()).toMatchObject({ error: "access_denied" });
  });

  it("approving needs a signed-in browser (and its CSRF token)", async () => {
    const s = await start();
    const anon = await json(t, "POST", "/v1/auth/device/approve", { body: { user_code: s.user_code, approve: true } });
    expect(anon.status).toBe(401);
    const { jar } = await signUpVerified(t, "csrf@tandem.test");
    const noCsrf = await json(t, "POST", "/v1/auth/device/approve", {
      jar,
      csrf: false,
      body: { user_code: s.user_code, approve: true },
    });
    expect(noCsrf.status).toBe(403);
  });

  it("the 11th code in an hour from one address is 429", async () => {
    for (let i = 0; i < 10; i++) await start("203.0.113.9");
    const res = await json(t, "POST", "/v1/auth/device/start", {
      body: { client: "x", version: "1" },
      headers: { "x-forwarded-for": "203.0.113.9" },
    });
    expect(res.status).toBe(429);
    expect(Number(res.headers.get("retry-after"))).toBeGreaterThan(0);
  });

  it("user codes read the same however they're typed", () => {
    expect(normalizeUserCode("bcdf ghjk")).toBe("BCDF-GHJK");
    expect(normalizeUserCode("BCDF-GHJK")).toBe("BCDF-GHJK");
    expect(normalizeUserCode("BCDF-GHJA")).toBeNull();
    expect(normalizeUserCode("BCD")).toBeNull();
  });
});
