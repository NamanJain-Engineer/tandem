import { mkdtempSync, readdirSync, rmSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { eq } from "drizzle-orm";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { aiRequests, creditLedger, creditReservations, dailyUsage } from "../src/db/schema";
import { logger } from "../src/lib/logger";
import type { MockStt } from "../src/providers/stt/mock";
import { AudioMeter, sttCost } from "../src/services/audio";
import { balance, grantMonthly } from "../src/services/ledger";
import { form, opus, wav } from "./audio";
import { bearerFor, makeTestApp, setSubscription, signUpVerified, type TestEnv } from "./helpers";

let t: TestEnv;
beforeEach(async () => {
  t = await makeTestApp();
});
afterEach(async () => {
  vi.restoreAllMocks();
  await t.close();
});

const stt = () => t.deps.stt as MockStt;

async function proUser(email: string) {
  const { userId } = await signUpVerified(t, email);
  await setSubscription(t.deps, userId);
  await t.deps.db.transaction((tx) =>
    grantMonthly(tx, { kind: "user", id: userId }, 1, new Date(t.clock.nowMs() + 30 * 86_400_000), "inv_stt"),
  );
  return { userId, bearer: await bearerFor(t.deps, userId) };
}

function upload(
  bearer: string,
  audio: { file: Buffer; type: string; filename: string },
  fields: Record<string, string | string[]> = {},
  headers: Record<string, string> = {},
) {
  const parts = [
    ...Object.entries(fields).flatMap(([name, v]) =>
      (Array.isArray(v) ? v : [v]).map((value) => ({ name, value })),
    ),
    { name: "audio", ...audio },
  ];
  const { body, type } = form(parts);
  return t.app.request("/v1/stt/transcribe", {
    method: "POST",
    headers: {
      authorization: `Bearer ${bearer}`,
      "content-type": type,
      "content-length": String(body.length),
      ...headers,
    },
    body,
  });
}

const ogg = (s: number) => ({ file: opus(s), type: "audio/ogg", filename: "take.opus" });

describe("F-BE-07 STT relay", () => {
  it("transcribes with the mock, meters per started 15 s and records only metadata", async () => {
    const { userId, bearer } = await proUser("stt@tandem.test");
    const res = await upload(
      bearer,
      ogg(31),
      { language: "de", hints: ["Vibezeo", "pnpm"] },
      {
        "x-mock-transcript": "Hallo Welt",
      },
    );
    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({ text: "Hallo Welt", language: "de", duration: 31 });
    expect(stt().calls.at(-1)).toMatchObject({ language: "de", hints: ["Vibezeo", "pnpm"] });
    // 31 s = 3 started blocks of 15 s at 2 credits.
    expect((await balance(t.deps.db, { kind: "user", id: userId })).monthly).toBe(10_000 - 6);
    const [resv] = await t.deps.db
      .select()
      .from(creditReservations)
      .where(eq(creditReservations.userId, userId));
    expect(resv).toMatchObject({ feature: "stt", state: "settled", settledAmount: 6 });
    const ledger = await t.deps.db.select().from(creditLedger).where(eq(creditLedger.userId, userId));
    expect(ledger.map((l) => l.reason).sort()).toEqual(["reserve", "settle_refund"]);
    const [row] = await t.deps.db.select().from(aiRequests).where(eq(aiRequests.userId, userId));
    expect(row).toMatchObject({ feature: "stt", model: "mock", status: 200, credits: 6, tokensIn: 31 });
    const [usage] = await t.deps.db.select().from(dailyUsage).where(eq(dailyUsage.userId, userId));
    expect(usage?.units).toBe(31);
  });

  it("F-BE-07.1 an 11-minute upload is rejected with 413 and costs nothing", async () => {
    const { userId, bearer } = await proUser("long@tandem.test");
    for (const audio of [
      ogg(11 * 60),
      { file: wav(11 * 60), type: "audio/wav", filename: "take.wav" },
      { file: wav(11 * 60, false), type: "audio/wav", filename: "take.wav" },
    ]) {
      const res = await upload(bearer, audio);
      expect(res.status).toBe(413);
      expect(await res.json()).toMatchObject({ code: "PAYLOAD_TOO_LARGE", limit: "take_minutes" });
    }
    expect((await balance(t.deps.db, { kind: "user", id: userId })).monthly).toBe(10_000);
    const [usage] = await t.deps.db.select().from(dailyUsage).where(eq(dailyUsage.userId, userId));
    expect(usage?.units ?? 0).toBe(0);
    // A declared length over 10 minutes is refused before any audio is read.
    const declared = await upload(bearer, ogg(5), { duration_ms: String(11 * 60_000) });
    expect(declared.status).toBe(413);
  });

  it("F-BE-07.1 a body over 25 MB is 413 before anything is read", async () => {
    const { bearer } = await proUser("big@tandem.test");
    const res = await t.app.request("/v1/stt/transcribe", {
      method: "POST",
      headers: {
        authorization: `Bearer ${bearer}`,
        "content-type": "multipart/form-data; boundary=x",
        "content-length": String(26 * 1024 * 1024),
      },
      body: "--x--",
    });
    expect(res.status).toBe(413);
    expect(await res.json()).toMatchObject({ limit: "take_bytes" });
  });

  it("F-BE-07.2 the 120-minute daily cap returns DAILY_LIMIT, and resets the next day", async () => {
    t.clock.set(new Date("2026-10-09T23:50:00Z"));
    const { userId, bearer } = await proUser("cap@tandem.test");
    await t.deps.db
      .insert(dailyUsage)
      .values({ userId, feature: "stt", day: "2026-10-09", units: 120 * 60 - 20 });
    // 20 seconds left: a declared 30 s take is refused at once…
    const declared = await upload(bearer, ogg(30), { duration_ms: "30000" });
    expect(declared.status).toBe(429);
    expect(await declared.json()).toMatchObject({ code: "DAILY_LIMIT", limit: "stt_minutes" });
    // …and an undeclared one is stopped when it passes the cap, uncharged.
    const streamed = await upload(bearer, ogg(30));
    expect(streamed.status).toBe(429);
    expect((await balance(t.deps.db, { kind: "user", id: userId })).monthly).toBe(10_000);
    const short = await upload(bearer, ogg(10));
    expect(short.status).toBe(200);
    t.clock.advance(15 * 60_000);
    const tomorrow = await upload(bearer, ogg(30), { duration_ms: "30000" });
    expect(tomorrow.status).toBe(200);
  });

  it("F-BE-07.3 no audio file remains on the server filesystem after a request", async () => {
    const { bearer } = await proUser("disk@tandem.test");
    // A private temp dir: anything the relay or busboy spooled would land here.
    const dir = mkdtempSync(join(tmpdir(), "stt-"));
    const saved = process.env.TMPDIR;
    process.env.TMPDIR = dir;
    try {
      expect(tmpdir()).toBe(dir);
      const res = await upload(bearer, { file: wav(20), type: "audio/wav", filename: "take.wav" });
      expect(res.status).toBe(200);
      expect(readdirSync(dir)).toEqual([]);
    } finally {
      process.env.TMPDIR = saved;
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("logs no transcript, hints or audio", async () => {
    const { bearer } = await proUser("sttlogs@tandem.test");
    const lines: string[] = [];
    for (const level of ["trace", "debug", "info", "warn", "error", "fatal"] as const) {
      vi.spyOn(logger, level).mockImplementation(((...args: unknown[]) => {
        lines.push(JSON.stringify(args));
      }) as never);
    }
    const res = await upload(
      bearer,
      ogg(3),
      { hints: "zebra-hint-41" },
      { "x-mock-transcript": "quokka-text-77" },
    );
    expect(res.status).toBe(200);
    expect(lines.some((l) => l.includes("stt request"))).toBe(true);
    expect(lines.join("\n")).not.toMatch(/quokka-text-77|zebra-hint-41/);
  });

  it("a provider failure refunds the credits and the day's seconds", async () => {
    const { userId, bearer } = await proUser("sttfail@tandem.test");
    const res = await upload(bearer, ogg(20), {}, { "x-mock-stt-fail": "1" });
    expect(res.status).toBe(503);
    expect(await res.json()).toMatchObject({ code: "STT_UNAVAILABLE", retryable: true });
    expect((await balance(t.deps.db, { kind: "user", id: userId })).monthly).toBe(10_000);
    const [usage] = await t.deps.db.select().from(dailyUsage).where(eq(dailyUsage.userId, userId));
    expect(usage?.units ?? 0).toBe(0);
  });

  it("audio longer than its declared length is refused and refunded", async () => {
    const { userId, bearer } = await proUser("liar@tandem.test");
    const res = await upload(bearer, ogg(60), { duration_ms: "5000" });
    expect(res.status).toBe(422);
    expect((await balance(t.deps.db, { kind: "user", id: userId })).monthly).toBe(10_000);
  });

  it("is Pro only, and a short balance is 402 CREDITS_EXHAUSTED", async () => {
    const { userId } = await signUpVerified(t, "sttfree@tandem.test");
    const free = await upload(await bearerFor(t.deps, userId), ogg(3));
    expect(free.status).toBe(403);
    expect(await free.json()).toMatchObject({ code: "FORBIDDEN_PLAN" });
    const { userId: broke } = await signUpVerified(t, "sttbroke@tandem.test");
    await setSubscription(t.deps, broke);
    const res = await upload(await bearerFor(t.deps, broke), ogg(3), { duration_ms: "3000" });
    expect(res.status).toBe(402);
    expect(await res.json()).toMatchObject({ code: "CREDITS_EXHAUSTED" });
  });

  it("reads WAV and Opus lengths from the stream", async () => {
    for (const [buf, kind, secs] of [
      [wav(12.5), "wav", 12.5],
      [wav(3, false), "wav", 3],
      [opus(7.25), "ogg", 7.25],
    ] as const) {
      const m = new AudioMeter(kind, 600);
      m.resume();
      // Feed in uneven chunks, as a socket would.
      for (let i = 0; i < buf.length; i += 997) m.write(buf.subarray(i, i + 997));
      m.end();
      await new Promise((resolve) => m.on("end", resolve));
      expect(m.seconds).toBeCloseTo(secs, 2);
    }
    expect(sttCost(0.1, 2)).toBe(2);
    expect(sttCost(15, 2)).toBe(2);
    expect(sttCost(15.01, 2)).toBe(4);
  });
});
