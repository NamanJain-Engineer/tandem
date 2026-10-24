import { decodeJwt, decodeProtectedHeader, jwtVerify } from "jose";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { bearerFor, json, makeTestApp, setSubscription, signUpVerified, type TestEnv } from "./helpers";

let t: TestEnv;
beforeEach(async () => {
  t = await makeTestApp();
});
afterEach(async () => {
  await t.close();
});

describe("F-BE-04 entitlements", () => {
  it("/me returns plan, limits, credits and an Ed25519 entitlement token valid for 24 h", async () => {
    const { userId } = await signUpVerified(t, "ent@tandem.test");
    await setSubscription(t.deps, userId);
    const me = (await (
      await json(t, "GET", "/v1/me", { bearer: await bearerFor(t.deps, userId) })
    ).json()) as {
      plan: string;
      limits: { panes: number | null; conductor: boolean };
      entitlement_token: string;
      credits: object;
    };
    expect(me).toMatchObject({ plan: "pro", limits: { panes: null, conductor: true } });
    expect(me.credits).toMatchObject({ monthly: 0, purchased: 0 });
    const header = decodeProtectedHeader(me.entitlement_token);
    expect(header).toMatchObject({ alg: "EdDSA", kid: "ek-1" });
    const key = t.deps.keys.entitlement[0]?.publicKey;
    if (!key) throw new Error("no key");
    const { payload } = await jwtVerify(me.entitlement_token, key, { audience: "tandem-desktop" });
    expect(payload).toMatchObject({ sub: userId, plan: "pro" });
    const claims = decodeJwt(me.entitlement_token);
    expect((claims.exp ?? 0) - (claims.iat ?? 0)).toBe(24 * 3600);
  });

  it("F-BE-04.4 a hosted endpoint with a Free account returns 403 FORBIDDEN_PLAN despite a forged 'pro' header", async () => {
    const { userId } = await signUpVerified(t, "forge@tandem.test");
    const res = await json(t, "POST", "/v1/billing/credit-packs/checkout", {
      bearer: await bearerFor(t.deps, userId),
      body: { pack: "5k" },
      headers: { "x-tandem-plan": "pro", "x-entitlement": "pro" },
    });
    expect(res.status).toBe(403);
    expect(await res.json()).toMatchObject({ code: "FORBIDDEN_PLAN" });
  });

  it("an invalid bearer is 401 UNAUTHENTICATED as problem+json", async () => {
    const res = await json(t, "GET", "/v1/me", { bearer: "not-a-jwt" });
    expect(res.status).toBe(401);
    expect(res.headers.get("content-type")).toContain("application/problem+json");
  });
});
