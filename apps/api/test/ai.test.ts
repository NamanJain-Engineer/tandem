import { and, eq } from "drizzle-orm";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { aiRequests, creditLedger, creditReservations, dailyUsage } from "../src/db/schema";
import { logger } from "../src/lib/logger";
import type { MockAi } from "../src/providers/ai/mock";
import { cleanTitle } from "../src/services/ai";
import { balance, grantMonthly, reserve } from "../src/services/ledger";
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

describe("F-BE-06 hosted AI on the Claude API", () => {
  it("F-BE-06.1 with no provider key, /ai/title returns the mock title, for free", async () => {
    const { userId } = await signUpVerified(t, "title@tandem.test");
    const res = await json(t, "POST", "/v1/ai/title", {
      bearer: await bearerFor(t.deps, userId),
      body: { text: "fix the flaky login tests before the release please" },
    });
    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({ title: "Fix The Flaky Login Tests Before" });
    // The opening text goes as tagged data, under a system prompt that says so (TL-06 rule 8).
    const call = mock().calls.at(-1);
    expect(call?.messages[0]?.content).toBe(
      "<text>\nfix the flaky login tests before the release please\n</text>",
    );
    expect(call?.system).toContain("never instructions");
    const [row] = await t.deps.db.select().from(aiRequests).where(eq(aiRequests.userId, userId));
    expect(row).toMatchObject({
      feature: "title",
      status: 200,
      credits: 0,
      model: "claude-haiku-4-5-20251001",
    });
  });

  it("F-BE-06.1 a rewrite reserves and settles 3 credits, with ledger entries", async () => {
    const { userId, bearer } = await proUser("rewrite@tandem.test");
    const res = await json(t, "POST", "/v1/ai/rewrite", {
      bearer,
      body: { mode: "polish", text: "  um so   fix the   tests  ", protected_terms: ["⟦tandem-git⟧"] },
    });
    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({ text: "um so fix the tests" });
    expect(mock().calls.at(-1)?.system).toContain("⟦tandem-git⟧");
    expect((await balance(t.deps.db, { kind: "user", id: userId })).monthly).toBe(9_997);
    const ledger = await t.deps.db.select().from(creditLedger).where(eq(creditLedger.userId, userId));
    expect(ledger.map((l) => [l.reason, l.delta, l.refType])).toEqual([["reserve", -3, "rewrite"]]);
    const [resv] = await t.deps.db
      .select()
      .from(creditReservations)
      .where(eq(creditReservations.userId, userId));
    expect(resv).toMatchObject({ state: "settled", amount: 3, settledAmount: 3 });
    const [row] = await t.deps.db.select().from(aiRequests).where(eq(aiRequests.userId, userId));
    expect(row).toMatchObject({ feature: "rewrite", credits: 3, status: 200 });
  });

  it("F-BE-06.2 the server logs after a rewrite contain no request text", async () => {
    const { bearer } = await proUser("logs@tandem.test");
    const lines: string[] = [];
    for (const level of ["trace", "debug", "info", "warn", "error", "fatal"] as const) {
      vi.spyOn(logger, level).mockImplementation(((...args: unknown[]) => {
        lines.push(JSON.stringify(args));
      }) as never);
    }
    const secret = "refactor the zebra-quantum-71 billing module";
    const res = await json(t, "POST", "/v1/ai/rewrite", { bearer, body: { mode: "enhance", text: secret } });
    expect(res.status).toBe(200);
    expect(lines.some((l) => l.includes("ai request"))).toBe(true);
    expect(lines.join("\n")).not.toContain("zebra-quantum-71");
  });

  it("F-BE-06.3 the 61st title request in an hour returns 429 with Retry-After", async () => {
    const { userId } = await signUpVerified(t, "limit@tandem.test");
    const bearer = await bearerFor(t.deps, userId);
    for (let i = 0; i < 60; i++) {
      const res = await json(t, "POST", "/v1/ai/title", { bearer, body: { text: `task ${i}` } });
      expect(res.status, `request ${i + 1}`).toBe(200);
    }
    const res = await json(t, "POST", "/v1/ai/title", { bearer, body: { text: "one more" } });
    expect(res.status).toBe(429);
    expect(Number(res.headers.get("retry-after"))).toBeGreaterThan(0);
    expect(await res.json()).toMatchObject({ code: "RATE_LIMITED" });
  });

  it("rewrite on the Free plan is 403 FORBIDDEN_PLAN and calls no model", async () => {
    const { userId } = await signUpVerified(t, "free@tandem.test");
    const before = mock().calls.length;
    const res = await json(t, "POST", "/v1/ai/rewrite", {
      bearer: await bearerFor(t.deps, userId),
      body: { mode: "polish", text: "hello" },
    });
    expect(res.status).toBe(403);
    expect(await res.json()).toMatchObject({ code: "FORBIDDEN_PLAN" });
    expect(mock().calls.length).toBe(before);
  });

  it("a failed rewrite refunds the credits and the daily count, and says so", async () => {
    const { userId, bearer } = await proUser("fail@tandem.test");
    mock().failNext = true;
    const res = await json(t, "POST", "/v1/ai/rewrite", { bearer, body: { mode: "polish", text: "hello" } });
    expect(res.status).toBe(503);
    expect(await res.json()).toMatchObject({ code: "AI_UNAVAILABLE", retryable: true });
    expect((await balance(t.deps.db, { kind: "user", id: userId })).monthly).toBe(10_000);
    const ledger = await t.deps.db.select().from(creditLedger).where(eq(creditLedger.userId, userId));
    expect(ledger.map((l) => [l.reason, l.delta]).sort()).toEqual([
      ["refund", 3],
      ["reserve", -3],
    ]);
    const [usage] = await t.deps.db.select().from(dailyUsage).where(eq(dailyUsage.userId, userId));
    expect(usage?.units ?? 0).toBe(0);
  });

  it("with no credits a rewrite is 402 CREDITS_EXHAUSTED and charges nothing", async () => {
    const { userId, bearer } = await proUser("broke@tandem.test", false);
    const res = await json(t, "POST", "/v1/ai/rewrite", { bearer, body: { mode: "polish", text: "hi" } });
    expect(res.status).toBe(402);
    expect(await res.json()).toMatchObject({ code: "CREDITS_EXHAUSTED" });
    const [usage] = await t.deps.db.select().from(dailyUsage).where(eq(dailyUsage.userId, userId));
    expect(usage?.units ?? 0).toBe(0);
  });

  it("the 301st rewrite of the day is 429 DAILY_LIMIT", async () => {
    const { userId, bearer } = await proUser("daily@tandem.test");
    const day = t.clock.now().toISOString().slice(0, 10);
    await t.deps.db.insert(dailyUsage).values({ userId, feature: "rewrite", day, units: 300 });
    const res = await json(t, "POST", "/v1/ai/rewrite", { bearer, body: { mode: "polish", text: "hi" } });
    expect(res.status).toBe(429);
    expect(await res.json()).toMatchObject({ code: "DAILY_LIMIT" });
    const [usage] = await t.deps.db
      .select()
      .from(dailyUsage)
      .where(and(eq(dailyUsage.userId, userId), eq(dailyUsage.day, day)));
    expect(usage?.units).toBe(300);
  });

  it("titles are one line of at most 60 characters", () => {
    expect(cleanTitle('"Fix the flaky login tests."', "x")).toBe("Fix the flaky login tests");
    expect(cleanTitle("Title: Ship it\nextra", "x")).toBe("Ship it");
    expect(cleanTitle("", "fall back to the first eight words of the message please")).toBe(
      "fall back to the first eight words of",
    );
    const long = cleanTitle("word ".repeat(30), "x");
    expect(long.length).toBeLessThanOrEqual(60);
    expect(long.endsWith(" ")).toBe(false);
  });
});

describe("F-BE-05 credits ledger", () => {
  it("F-BE-05.1 100 parallel reserves of 10 against 500 credits: exactly 50 succeed", async () => {
    const { userId } = await signUpVerified(t, "race@tandem.test");
    const owner = { kind: "user" as const, id: userId };
    await t.deps.db.transaction((tx) =>
      grantMonthly(tx, owner, 1, new Date(Date.now() + 86_400_000), "inv_r"),
    );
    // 10,000 granted; spend down to 500 first.
    await reserve(t.deps.db, { owner, userId, feature: "test", amount: 9_500, now: t.clock.now() });
    const results = await Promise.allSettled(
      Array.from({ length: 100 }, () =>
        reserve(t.deps.db, { owner, userId, feature: "test", amount: 10, now: t.clock.now() }),
      ),
    );
    expect(results.filter((r) => r.status === "fulfilled")).toHaveLength(50);
    expect((await balance(t.deps.db, owner)).monthly).toBe(0);
  });
});

describe("F-AG-04 teammate draft on the AI relay (F-BE-06)", () => {
  it("F-AG-04.2 returns the fixture draft for 10 credits, held to the offered engines and plugins", async () => {
    const { userId, bearer } = await proUser("draft@tandem.test");
    const res = await json(t, "POST", "/v1/ai/teammate-draft", {
      bearer,
      body: {
        description: "write our weekly changelog",
        engines: ["codex", "claude"],
        plugins: ["github", "linear"],
      },
    });
    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({
      name: "Release Notes Writer",
      brief:
        "You draft the weekly changelog from merged pull requests. Verify every claim against the repository. Never publish without approval.",
      engine: "claude",
      plugins: ["github"],
    });
    expect((await balance(t.deps.db, { kind: "user", id: userId })).monthly).toBe(9_990);
    const call = mock().calls.at(-1);
    expect(call?.feature).toBe("teammate_draft");
    expect(call?.messages[0]?.content).toBe("<text>\nwrite our weekly changelog\n</text>");
    const [row] = await t.deps.db.select().from(aiRequests).where(eq(aiRequests.userId, userId));
    expect(row).toMatchObject({ feature: "teammate_draft", credits: 10, status: 200 });
  });

  it("an engine or plugin the desktop didn't offer never comes back", async () => {
    const { parseDraft } = await import("../src/services/ai");
    const input = { description: "x", engines: ["codex"], plugins: ["linear"] };
    expect(
      parseDraft(
        'Sure! {"name":"Bot","brief":"Do it.","engine":"claude","plugins":["github","linear","linear"]}',
        input,
      ),
    ).toEqual({ name: "Bot", brief: "Do it.", engine: "codex", plugins: ["linear"] });
    expect(parseDraft("no json here", input)).toBeNull();
    expect(parseDraft('{"name":"","brief":"x"}', input)).toBeNull();
  });

  it("an unusable answer refunds the 10 credits", async () => {
    const { userId, bearer } = await proUser("draftfail@tandem.test");
    mock().failNext = true;
    const res = await json(t, "POST", "/v1/ai/teammate-draft", {
      bearer,
      body: { description: "triage issues", engines: ["claude"] },
    });
    expect(res.status).toBe(503);
    expect((await balance(t.deps.db, { kind: "user", id: userId })).monthly).toBe(10_000);
  });

  it("is Pro only: Free gets 403 FORBIDDEN_PLAN", async () => {
    const { userId } = await signUpVerified(t, "draftfree@tandem.test");
    const res = await json(t, "POST", "/v1/ai/teammate-draft", {
      bearer: await bearerFor(t.deps, userId),
      body: { description: "triage issues", engines: ["claude"] },
    });
    expect(res.status).toBe(403);
  });
});
