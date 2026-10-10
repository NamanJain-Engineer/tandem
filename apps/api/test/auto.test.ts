import { eq } from "drizzle-orm";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { DEFAULT_POLICY } from "@tandem/protocol/team";
import { aiRequests, dailyUsage } from "../src/db/schema";
import { logger } from "../src/lib/logger";
import type { MockAi } from "../src/providers/ai/mock";
import { parseAutoNext } from "../src/services/ai";
import { balance, grantMonthly } from "../src/services/ledger";
import { membershipOf } from "../src/services/membership";
import { bearerFor, json, makeTestApp, setSubscription, signUpVerified, type TestEnv } from "./helpers";

let t: TestEnv;
beforeEach(async () => {
  t = await makeTestApp();
  await t.deps.db.delete(aiRequests);
  await t.deps.db.delete(dailyUsage);
});
afterEach(async () => {
  vi.restoreAllMocks();
  await t.close();
});

const mock = () => t.deps.ai as MockAi;

async function proUser(email: string, credits = true) {
  const { userId } = await signUpVerified(t, email);
  await setSubscription(t.deps, userId);
  if (credits) {
    await t.deps.db.transaction((tx) =>
      grantMonthly(tx, { kind: "user", id: userId }, 1, new Date(Date.now() + 30 * 86_400_000), "inv_test"),
    );
  }
  return { userId, bearer: await bearerFor(t.deps, userId) };
}

const step = (over: Record<string, unknown> = {}) => ({
  mode: "finish",
  goal: "make the login tests pass",
  turn: 0,
  ceiling: 20,
  scrollback: "$ pnpm test\n3 failing\n> ",
  ...over,
});

describe("F-AU-01 Auto on the AI relay (/v1/auto/next)", () => {
  it("F-AU-01 a turn answers prompt for 6 credits, with the scrollback sent as data", async () => {
    const { userId, bearer } = await proUser("auto@tandem.test");
    const res = await json(t, "POST", "/v1/auto/next", { bearer, body: step({ turn: 3 }) });
    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({ action: "prompt", text: "Continue with the next step." });
    expect((await balance(t.deps.db, { kind: "user", id: userId })).monthly).toBe(9_994);
    const call = mock().calls.at(-1);
    expect(call?.feature).toBe("auto_next");
    expect(call?.system).toContain("Turn 4 of 20");
    expect(call?.system).toContain("complete and verified");
    expect(call?.messages[0]?.content).toContain("<text>\n$ pnpm test");
    expect(call?.messages[0]?.content).toContain("make the login tests pass");
    const [row] = await t.deps.db.select().from(aiRequests).where(eq(aiRequests.userId, userId));
    expect(row).toMatchObject({ feature: "auto_next", credits: 6, status: 200 });
  });

  it("F-AU-01 the mock says done once the scrollback shows ALL TESTS PASSED", async () => {
    const { bearer } = await proUser("autodone@tandem.test");
    const res = await json(t, "POST", "/v1/auto/next", {
      bearer,
      body: step({ scrollback: "Tests: 12 passed\nALL TESTS PASSED\n> " }),
    });
    expect(await res.json()).toMatchObject({ action: "done" });
  });

  it("F-AU-01.4 with 0 credits the first call is 402", async () => {
    const { bearer } = await proUser("autobroke@tandem.test", false);
    const res = await json(t, "POST", "/v1/auto/next", { bearer, body: step() });
    expect(res.status).toBe(402);
    expect(mock().calls).toHaveLength(0);
  });

  it("a turn at or past the ceiling is refused before any model call", async () => {
    const { bearer } = await proUser("autoceil@tandem.test");
    const res = await json(t, "POST", "/v1/auto/next", { bearer, body: step({ turn: 20, ceiling: 20 }) });
    expect(res.status).toBe(422);
    expect(await res.json()).toMatchObject({ code: "VALIDATION" });
    const big = await json(t, "POST", "/v1/auto/next", { bearer, body: step({ scrollback: "x".repeat(16_385) }) });
    expect(big.status).toBe(422);
    const custom = await json(t, "POST", "/v1/auto/next", { bearer, body: step({ mode: "custom", goal: undefined }) });
    expect(custom.status).toBe(422);
    expect(mock().calls).toHaveLength(0);
  });

  it("is Pro only: Free gets 403 FORBIDDEN_PLAN", async () => {
    const { userId } = await signUpVerified(t, "autofree@tandem.test");
    const res = await json(t, "POST", "/v1/auto/next", { bearer: await bearerFor(t.deps, userId), body: step() });
    expect(res.status).toBe(403);
    expect(await res.json()).toMatchObject({ code: "FORBIDDEN_PLAN" });
  });

  it("a Team whose policy turns Auto off gets 403 FORBIDDEN_POLICY", async () => {
    const owner = await signUpVerified(t, "autoteam@tandem.test");
    const res = await json(t, "POST", "/v1/billing/checkout", {
      jar: owner.jar,
      body: { plan: "team", interval: "month", seats: 2 },
    });
    const sessionId = new URL(((await res.json()) as { url: string }).url).searchParams.get("session") ?? "";
    await json(t, "POST", "/v1/billing/mock/complete", { jar: owner.jar, body: { sessionId } });
    const orgId = (await membershipOf(t.deps.db, owner.userId))?.orgId ?? "";
    const bearer = await bearerFor(t.deps, owner.userId);
    expect((await json(t, "POST", "/v1/auto/next", { bearer, body: step() })).status).toBe(200);
    const put = await json(t, "PUT", `/v1/orgs/${orgId}/policy`, {
      jar: owner.jar,
      body: { ...DEFAULT_POLICY, disableAuto: true },
    });
    expect(put.status).toBe(200);
    const off = await json(t, "POST", "/v1/auto/next", { bearer, body: step() });
    expect(off.status).toBe(403);
    expect(await off.json()).toMatchObject({ code: "FORBIDDEN_POLICY" });
  });

  it("F-AU-01 an unusable answer refunds the 6 credits", async () => {
    const { userId, bearer } = await proUser("autofail@tandem.test");
    mock().failNext = true;
    const res = await json(t, "POST", "/v1/auto/next", { bearer, body: step() });
    expect(res.status).toBe(503);
    expect((await balance(t.deps.db, { kind: "user", id: userId })).monthly).toBe(10_000);
  });

  it("the scrollback and goal never reach the logs", async () => {
    const { bearer } = await proUser("autolog@tandem.test");
    const lines: string[] = [];
    for (const level of ["trace", "debug", "info", "warn", "error", "fatal"] as const) {
      vi.spyOn(logger, level).mockImplementation(((...args: unknown[]) => {
        lines.push(JSON.stringify(args));
      }) as never);
    }
    const res = await json(t, "POST", "/v1/auto/next", {
      bearer,
      body: step({ goal: "ship the walrus-ember-42 flow", scrollback: "walrus-ember-42 failing" }),
    });
    expect(res.status).toBe(200);
    expect(lines.join("\n")).not.toContain("walrus-ember-42");
  });

  it("the model's answer is held to the contract", () => {
    expect(parseAutoNext('Sure: {"action":"prompt","text":"  Run the tests again.  "}')).toEqual({
      action: "prompt",
      text: "Run the tests again.",
    });
    expect(parseAutoNext('{"action":"done","reason":"All 12 tests pass."}')).toEqual({
      action: "done",
      reason: "All 12 tests pass.",
    });
    expect(parseAutoNext('{"action":"stuck"}')).toEqual({ action: "stuck", reason: "The supervisor gave no reason." });
    expect(parseAutoNext('{"action":"prompt","text":""}')).toBeNull();
    expect(parseAutoNext('{"action":"delete everything"}')).toBeNull();
    expect(parseAutoNext("no json")).toBeNull();
    const long = parseAutoNext(JSON.stringify({ action: "prompt", text: "a".repeat(5000) }));
    expect(long?.text?.length).toBe(2000);
  });
});
