import { and, eq } from "drizzle-orm";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { DEFAULT_POLICY } from "@tandem/protocol/team";
import { aiRequests, creditLedger, creditReservations, voiceSessions } from "../src/db/schema";
import { sweepReservations } from "../src/jobs/reservationSweeper";
import { sweepVoiceSessions } from "../src/jobs/voiceSweeper";
import { logger } from "../src/lib/logger";
import type { MockAi } from "../src/providers/ai/mock";
import type { MockRealtime } from "../src/providers/realtime/mock";
import {
  type Minted as ProviderMinted,
  type RealtimeProvider,
  RealtimeUnavailableError,
} from "../src/providers/realtime/types";
import { parseMatches } from "../src/services/ai";
import { addPurchased, balance, grantMonthly } from "../src/services/ledger";
import { membershipOf } from "../src/services/membership";
import { bearerFor, json, makeTestApp, setSubscription, signUpVerified, type TestEnv } from "./helpers";

let t: TestEnv;
beforeEach(async () => {
  t = await makeTestApp();
});
afterEach(async () => {
  vi.restoreAllMocks();
  await t.close();
});

const ai = () => t.deps.ai as MockAi;
const rt = () => t.deps.realtime[0] as MockRealtime;

/** A Pro user with the monthly grant (10,000), or exactly `credits` purchased credits. */
async function proUser(email: string, credits?: number) {
  const { userId } = await signUpVerified(t, email);
  await setSubscription(t.deps, userId);
  const owner = { kind: "user" as const, id: userId };
  await t.deps.db.transaction(async (tx) => {
    if (credits === undefined) {
      await grantMonthly(tx, owner, 1, new Date(Date.now() + 30 * 86_400_000), "inv_test");
    } else if (credits > 0) {
      await addPurchased(tx, owner, credits, "cs_test");
    }
  });
  return { userId, bearer: await bearerFor(t.deps, userId) };
}

async function creditsOf(userId: string): Promise<number> {
  const b = await balance(t.deps.db, { kind: "user", id: userId });
  return b.monthly + b.purchased;
}

type Minted = {
  session_id: string;
  provider: string;
  ephemeral_token: string;
  ws_url: string;
  expires_at: string;
  max_seconds: number;
};
type ToolCall = { id: string; name: string; args: Record<string, unknown> };
type ActResponse = { tool_calls: ToolCall[]; say?: string };

async function mint(bearer: string, body: Record<string, unknown> = {}): Promise<Minted> {
  const res = await json(t, "POST", "/v1/voice/session", { bearer, body });
  expect(res.status).toBe(200);
  return (await res.json()) as Minted;
}

const settle = (bearer: string, id: string, seconds: number) =>
  json(t, "POST", `/v1/voice/session/${id}/settle`, { bearer, body: { seconds } });

async function sessionRow(id: string) {
  const [row] = await t.deps.db.select().from(voiceSessions).where(eq(voiceSessions.id, id));
  return row;
}

const SNAPSHOT = {
  workspaces: [{ id: "w-shop", name: "shop" }],
  panes: [
    { id: "p1", name: "auth tests", engine: "claude", status: "working" },
    { id: "p2", name: "billing UI", engine: "codex", status: "idle" },
  ],
};

const actBody = (sessionId: string, over: Record<string, unknown> = {}) => ({
  session_id: sessionId,
  request: "launch two claude agents in shop",
  snapshot: SNAPSHOT,
  ...over,
});

async function act(bearer: string, body: Record<string, unknown>) {
  return json(t, "POST", "/v1/voice/act", { bearer, body });
}

function captureLogs(): string[] {
  const lines: string[] = [];
  for (const level of ["trace", "debug", "info", "warn", "error", "fatal"] as const) {
    vi.spyOn(logger, level).mockImplementation(((...args: unknown[]) => {
      lines.push(JSON.stringify(args));
    }) as never);
  }
  return lines;
}

const CANDIDATES = [
  { id: "p1", kind: "pane", text: "auth tests" },
  { id: "p2", kind: "pane", text: "billing UI" },
  { id: "p3", kind: "pane", text: "docs" },
];

describe("F-BE-08 voice sessions (/v1/voice/session)", () => {
  it("F-BE-08 minting holds 10 minutes of credits and returns the session contract", async () => {
    const { userId, bearer } = await proUser("voice@tandem.test");
    t.clock.freeze();
    const s = await mint(bearer, { voice: "marin" });
    expect(s).toMatchObject({ provider: "mock", max_seconds: 1800 });
    expect(s.session_id).toMatch(/^vs_/);
    expect(s.ephemeral_token.length).toBeGreaterThan(20);
    expect(s.ws_url).toBe(
      `ws://127.0.0.1:8787/v1/dev/realtime?token=${encodeURIComponent(s.ephemeral_token)}`,
    );
    expect(Date.parse(s.expires_at)).toBe(t.clock.nowMs() + 1_800_000);
    expect(await creditsOf(userId)).toBe(10_000 - 1_200);
    // The talker gets its base instructions and the one act tool at mint; the voice passes through.
    const minted = rt().calls.at(-1);
    expect(minted).toMatchObject({ voice: "marin", maxSeconds: 1800 });
    expect(minted?.instructions).toContain("act");
    const row = await sessionRow(s.session_id);
    expect(row).toMatchObject({ userId, provider: "mock", maxSeconds: 1800, settledAt: null });
    expect(row?.providerSessionId).toBeTruthy();
    // The hold outlives the call, so the 15-minute reservation sweeper never takes it mid-call.
    const [resv] = await t.deps.db
      .select()
      .from(creditReservations)
      .where(eq(creditReservations.id, row?.reservationId ?? ""));
    expect(resv).toMatchObject({ feature: "voice", amount: 1_200, state: "held" });
    expect(resv?.expiresAt.getTime()).toBeGreaterThan(row?.expiresAt.getTime() ?? Infinity);
  });

  it("F-BE-08.1 minting with fewer than 1,200 credits returns 402 and starts nothing", async () => {
    const short = await proUser("voiceshort@tandem.test", 1_199);
    const res = await json(t, "POST", "/v1/voice/session", { bearer: short.bearer, body: {} });
    expect(res.status).toBe(402);
    expect(await res.json()).toMatchObject({ code: "CREDITS_EXHAUSTED", needed: 1_200 });
    expect(await creditsOf(short.userId)).toBe(1_199);
    expect(rt().calls).toHaveLength(0);
    expect(await t.deps.db.select().from(voiceSessions)).toHaveLength(0);

    const exact = await proUser("voiceexact@tandem.test", 1_200);
    await mint(exact.bearer);
    expect(await creditsOf(exact.userId)).toBe(0);
  });

  it("F-BE-08 is Pro only, and a Team whose policy turns Conductor off gets 403 FORBIDDEN_POLICY", async () => {
    const free = await signUpVerified(t, "voicefree@tandem.test");
    const res = await json(t, "POST", "/v1/voice/session", {
      bearer: await bearerFor(t.deps, free.userId),
      body: {},
    });
    expect(res.status).toBe(403);
    expect(await res.json()).toMatchObject({ code: "FORBIDDEN_PLAN" });

    const owner = await signUpVerified(t, "voiceteam@tandem.test");
    const checkout = await json(t, "POST", "/v1/billing/checkout", {
      jar: owner.jar,
      body: { plan: "team", interval: "month", seats: 2 },
    });
    const sessionId =
      new URL(((await checkout.json()) as { url: string }).url).searchParams.get("session") ?? "";
    await json(t, "POST", "/v1/billing/mock/complete", { jar: owner.jar, body: { sessionId } });
    const orgId = (await membershipOf(t.deps.db, owner.userId))?.orgId ?? "";
    const bearer = await bearerFor(t.deps, owner.userId);
    const live = await mint(bearer);
    const put = await json(t, "PUT", `/v1/orgs/${orgId}/policy`, {
      jar: owner.jar,
      body: { ...DEFAULT_POLICY, disableConductor: true },
    });
    expect(put.status).toBe(200);
    const off = await json(t, "POST", "/v1/voice/session", { bearer, body: {} });
    expect(off.status).toBe(403);
    expect(await off.json()).toMatchObject({ code: "FORBIDDEN_POLICY" });
    // A call already running loses its doer too; settling still works, so the hold comes back.
    const refused = await act(bearer, actBody(live.session_id));
    expect(refused.status).toBe(403);
    expect(await refused.json()).toMatchObject({ code: "FORBIDDEN_POLICY" });
    expect((await settle(bearer, live.session_id, 0)).status).toBe(200);
  });

  it("F-BE-08 the 11th call started in an hour is 429 with Retry-After", async () => {
    const { bearer } = await proUser("voicerate@tandem.test");
    for (let i = 0; i < 10; i++) {
      const s = await mint(bearer);
      expect((await settle(bearer, s.session_id, 0)).status).toBe(200);
    }
    const res = await json(t, "POST", "/v1/voice/session", { bearer, body: {} });
    expect(res.status).toBe(429);
    expect(Number(res.headers.get("retry-after"))).toBeGreaterThan(0);
  });

  it("F-BE-08 checks the voice name, and a provider failure gives the hold back", async () => {
    const { userId, bearer } = await proUser("voicefail@tandem.test");
    const bad = await json(t, "POST", "/v1/voice/session", { bearer, body: { voice: "two words" } });
    expect(bad.status).toBe(422);
    rt().failNext = true;
    const res = await json(t, "POST", "/v1/voice/session", { bearer, body: {} });
    expect(res.status).toBe(503);
    expect(await res.json()).toMatchObject({ code: "VOICE_UNAVAILABLE", retryable: true });
    expect(await creditsOf(userId)).toBe(10_000);
    expect(await t.deps.db.select().from(voiceSessions)).toHaveLength(0);
  });
});

/** Another vendor for the chain: answers like a provider, or fails with `fail`. */
class FakeVendor implements RealtimeProvider {
  calls = 0;
  fail?: number;
  constructor(readonly name: "gemini" | "openai") {}
  async mint(): Promise<ProviderMinted> {
    this.calls += 1;
    if (this.fail) throw new RealtimeUnavailableError("down", this.fail);
    return { token: `tok_${this.name}_${this.calls}`, wsUrl: `wss://${this.name}.example/live` };
  }
  async reportedSeconds(): Promise<number | null> {
    return null;
  }
}

describe("F-BE-08 a free primary with a backup (D-1013)", () => {
  let gemini: FakeVendor;
  let backup: FakeVendor;
  beforeEach(async () => {
    await t.close();
    gemini = new FakeVendor("gemini");
    backup = new FakeVendor("openai");
    t = await makeTestApp({}, { realtime: [gemini, backup] });
  });

  it("F-BE-08 an app that names what it speaks gets the first provider in order", async () => {
    const { userId, bearer } = await proUser("chain1@tandem.test");
    const s = await mint(bearer, { providers: ["mock", "openai", "gemini"] });
    expect(s).toMatchObject({ provider: "gemini", ws_url: "wss://gemini.example/live" });
    expect(backup.calls).toBe(0);
    expect((await sessionRow(s.session_id))?.provider).toBe("gemini");
    expect(await creditsOf(userId)).toBe(10_000 - 1_200);
  });

  it("F-BE-08 an app that names nothing never gets a provider it can't speak", async () => {
    // Every app released before D-1013 speaks mock and openai only.
    const { bearer } = await proUser("chain2@tandem.test");
    const s = await mint(bearer);
    expect(s.provider).toBe("openai");
    expect(gemini.calls).toBe(0);
  });

  it("F-BE-08 a primary that can't mint falls back to the next, under one hold", async () => {
    const { userId, bearer } = await proUser("chain3@tandem.test");
    gemini.fail = 429;
    const s = await mint(bearer, { providers: ["openai", "gemini"] });
    expect(s.provider).toBe("openai");
    expect([gemini.calls, backup.calls]).toEqual([1, 1]);
    expect(await creditsOf(userId)).toBe(10_000 - 1_200);
    expect(await t.deps.db.select().from(creditReservations)).toHaveLength(1);
  });

  it("F-BE-08 when no provider mints, the hold comes back and the call is unavailable", async () => {
    const { userId, bearer } = await proUser("chain4@tandem.test");
    gemini.fail = 503;
    backup.fail = 500;
    const res = await json(t, "POST", "/v1/voice/session", {
      bearer,
      body: { providers: ["gemini", "openai"] },
    });
    expect(res.status).toBe(503);
    expect(await res.json()).toMatchObject({ code: "VOICE_UNAVAILABLE", retryable: true });
    expect(await creditsOf(userId)).toBe(10_000);
    expect(await t.deps.db.select().from(voiceSessions)).toHaveLength(0);
  });

  it("F-BE-08 names the API doesn't know are ignored; none it has is unavailable, with nothing held", async () => {
    const { userId, bearer } = await proUser("chain5@tandem.test");
    const s = await mint(bearer, { providers: ["future-vendor", "gemini"] });
    expect(s.provider).toBe("gemini");
    const none = await json(t, "POST", "/v1/voice/session", {
      bearer,
      body: { providers: ["future-vendor"] },
    });
    expect(none.status).toBe(503);
    expect(await none.json()).toMatchObject({ code: "VOICE_UNAVAILABLE" });
    expect(await creditsOf(userId)).toBe(10_000 - 1_200);
    expect([gemini.calls, backup.calls]).toEqual([1, 0]);
  });

  it("F-BE-08 a malformed providers list is a 422", async () => {
    const { bearer } = await proUser("chain6@tandem.test");
    for (const providers of ["gemini", [""], ["Gemini!"], Array.from({ length: 17 }, (_, i) => `p${i}`)]) {
      const res = await json(t, "POST", "/v1/voice/session", { bearer, body: { providers } });
      expect(res.status, JSON.stringify(providers)).toBe(422);
    }
    expect(gemini.calls + backup.calls).toBe(0);
  });
});

describe("F-BE-08 settling (/v1/voice/session/:id/settle)", () => {
  it("F-CN-01.1 a 65 s call settles 7 × 20 = 140 credits and refunds the rest of the hold", async () => {
    const { userId, bearer } = await proUser("voice65@tandem.test");
    t.clock.freeze();
    const s = await mint(bearer);
    t.clock.advance(70_000);
    const res = await settle(bearer, s.session_id, 65);
    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({ charged: 140, refunded: 1_060 });
    expect(await creditsOf(userId)).toBe(10_000 - 140);
    const row = await sessionRow(s.session_id);
    expect(row?.seconds).toBe(65);
    expect(row?.settledAt?.getTime()).toBe(t.clock.nowMs());
  });

  it("F-BE-08.2 settling twice is idempotent, also when the two race", async () => {
    const { userId, bearer } = await proUser("voicetwice@tandem.test");
    t.clock.freeze();
    const s = await mint(bearer);
    t.clock.advance(70_000);
    const first = await (await settle(bearer, s.session_id, 65)).json();
    const again = await settle(bearer, s.session_id, 400);
    expect(again.status).toBe(200);
    expect(await again.json()).toEqual(first);
    expect(await creditsOf(userId)).toBe(10_000 - 140);
    const reservationId = (await sessionRow(s.session_id))?.reservationId ?? "";
    const refunds = await t.deps.db
      .select()
      .from(creditLedger)
      .where(and(eq(creditLedger.reservationId, reservationId), eq(creditLedger.reason, "settle_refund")));
    expect(refunds).toHaveLength(1);

    const raced = await mint(bearer);
    t.clock.advance(70_000);
    const both = await Promise.all([
      settle(bearer, raced.session_id, 65),
      settle(bearer, raced.session_id, 65),
    ]);
    expect(both.map((r) => r.status)).toEqual([200, 200]);
    const [a, b] = await Promise.all(both.map((r) => r.json()));
    expect(a).toEqual({ charged: 140, refunded: 1_060 });
    expect(b).toEqual(a);
    expect(await creditsOf(userId)).toBe(10_000 - 280);
  });

  it("F-BE-08 settle is 404 for an unknown or someone else's call, and checks its seconds", async () => {
    const a = await proUser("voicea@tandem.test");
    const b = await proUser("voiceb@tandem.test");
    const s = await mint(a.bearer);
    expect((await settle(b.bearer, s.session_id, 10)).status).toBe(404);
    expect((await settle(a.bearer, "vs_unknown", 10)).status).toBe(404);
    expect((await settle(a.bearer, s.session_id, 1_801)).status).toBe(422);
    expect((await settle(a.bearer, s.session_id, -1)).status).toBe(422);
    expect((await settle(a.bearer, s.session_id, 1.5)).status).toBe(422);
    expect((await sessionRow(s.session_id))?.settledAt).toBeNull();
  });

  it("F-BE-08 a call is never charged for more time than it has run", async () => {
    const { userId, bearer } = await proUser("voiceclock@tandem.test");
    t.clock.freeze();
    const s = await mint(bearer);
    t.clock.advance(30_000);
    // 600 s claimed 30 s in: the charge stops at the elapsed time plus 5 s of skew (35 s, 4 units).
    expect(await (await settle(bearer, s.session_id, 600)).json()).toEqual({ charged: 80, refunded: 1_120 });
    expect(await creditsOf(userId)).toBe(10_000 - 80);
  });

  it("F-BE-08 a settle can't claim less time than the call's last doer round", async () => {
    const { userId, bearer } = await proUser("voicefloor@tandem.test");
    t.clock.freeze();
    const s = await mint(bearer);
    t.clock.advance(300_000);
    expect((await act(bearer, actBody(s.session_id))).status).toBe(200);
    t.clock.advance(5_000);
    // The doer ran 300 s in, so the call lasted at least that long, less 15 s to connect: 285 s, 29 units.
    expect(await (await settle(bearer, s.session_id, 10)).json()).toEqual({ charged: 580, refunded: 620 });
    expect(await creditsOf(userId)).toBe(10_000 - 580);
  });

  it("F-BE-08 a call longer than the hold is charged in full while credits last", async () => {
    const rich = await proUser("voicelong@tandem.test");
    const poor = await proUser("voicepoor@tandem.test", 2_000);
    t.clock.freeze();
    const s = await mint(rich.bearer);
    const p = await mint(poor.bearer);
    t.clock.advance(25 * 60_000);
    expect(await (await settle(rich.bearer, s.session_id, 1_500)).json()).toEqual({
      charged: 3_000,
      refunded: 0,
    });
    expect(await creditsOf(rich.userId)).toBe(7_000);
    expect(await (await settle(poor.bearer, p.session_id, 1_500)).json()).toEqual({
      charged: 2_000,
      refunded: 0,
    });
    expect(await creditsOf(poor.userId)).toBe(0);
  });
});

describe("F-BE-08 voiceSweeper", () => {
  it("F-BE-08 a call nobody settled is settled at max_seconds, within its hold", async () => {
    const { userId, bearer } = await proUser("voicesweep@tandem.test");
    t.clock.freeze();
    const s = await mint(bearer);
    t.clock.advance(31 * 60_000);
    // Past the 30-minute cap, but the app still has 2 minutes to settle on its own.
    expect(await sweepVoiceSessions(t.deps)).toBe(0);
    t.clock.advance(2 * 60_000);
    expect(await sweepVoiceSessions(t.deps)).toBe(1);
    expect((await sessionRow(s.session_id))?.seconds).toBe(1_800);
    expect(await creditsOf(userId)).toBe(10_000 - 1_200);
    // A late settle from the app gets the sweeper's answer, and nothing is swept twice.
    expect(await (await settle(bearer, s.session_id, 65)).json()).toEqual({ charged: 1_200, refunded: 0 });
    expect(await sweepVoiceSessions(t.deps)).toBe(0);
    expect(await creditsOf(userId)).toBe(10_000 - 1_200);
  });

  it("F-BE-08 the sweeper uses the provider's usage report when there is one", async () => {
    const { userId, bearer } = await proUser("voiceusage@tandem.test");
    t.clock.freeze();
    const s = await mint(bearer);
    rt().usage.set((await sessionRow(s.session_id))?.providerSessionId ?? "", 65);
    t.clock.advance(33 * 60_000);
    expect(await sweepVoiceSessions(t.deps)).toBe(1);
    expect((await sessionRow(s.session_id))?.seconds).toBe(65);
    expect(await creditsOf(userId)).toBe(10_000 - 140);
  });

  it("F-BE-08 the sweeper leaves live calls alone and runs on one replica at a time", async () => {
    const { bearer } = await proUser("voicelive@tandem.test");
    t.clock.freeze();
    await mint(bearer);
    t.clock.advance(10 * 60_000);
    expect(await sweepVoiceSessions(t.deps)).toBe(0);
    await t.deps.redis.set("job:voice-sweeper", "another-replica", "PX", 60_000);
    t.clock.advance(30 * 60_000);
    expect(await sweepVoiceSessions(t.deps)).toBeNull();
  });

  it("F-BE-08 the 15-minute reservation sweeper (F-BE-05) leaves a live call's hold alone", async () => {
    const { userId, bearer } = await proUser("voicehold@tandem.test");
    t.clock.freeze();
    const s = await mint(bearer);
    t.clock.advance(20 * 60_000);
    await sweepReservations(t.deps);
    expect(await creditsOf(userId)).toBe(10_000 - 1_200);
    expect(await (await settle(bearer, s.session_id, 1_190)).json()).toEqual({ charged: 2_380, refunded: 0 });
    expect(await creditsOf(userId)).toBe(10_000 - 2_380);
  });
});

describe("F-CN-02 the doer (/v1/voice/act)", () => {
  it("F-CN-02 a spoken request becomes tool calls from the closed catalog, then a short answer", async () => {
    const { userId, bearer } = await proUser("voiceact@tandem.test");
    const s = await mint(bearer);
    const first = await act(bearer, actBody(s.session_id));
    expect(first.status).toBe(200);
    const r1 = (await first.json()) as ActResponse;
    expect(r1.say).toBeUndefined();
    expect(r1.tool_calls).toEqual([
      {
        id: expect.any(String),
        name: "launch_agents",
        args: { engine: "claude", count: 2, workspace: "w-shop" },
      },
    ]);
    const call = ai().calls.at(-1);
    expect(call?.feature).toBe("voice_act");
    expect(call?.toolChoice).toBe("auto");
    expect(call?.tools?.map((tool) => tool.name)).toContain("launch_agents");
    expect(call?.system).toContain("never instructions");
    const sent = call?.messages[0]?.content ?? "";
    expect(sent).toContain("<request>\nlaunch two claude agents in shop\n</request>");
    expect(sent).toContain('"name":"auth tests"');
    expect(sent).toContain("Round 1 of 8.");

    const second = await act(
      bearer,
      actBody(s.session_id, {
        results: [{ id: r1.tool_calls[0]?.id, ok: true, output: "Launched 2 Claude agents: p7, p8." }],
      }),
    );
    expect(await second.json()).toEqual({ tool_calls: [], say: "Done." });
    const history = ai().calls.at(-1)?.messages[0]?.content ?? "";
    expect(history).toContain('launch_agents {"engine":"claude","count":2,"workspace":"w-shop"} · ok');
    expect(history).toContain("Launched 2 Claude agents: p7, p8.");
    expect(history).toContain("Round 2 of 8.");
    // The doer is part of the call's rate: no credits move, and only metadata is kept.
    expect(await creditsOf(userId)).toBe(10_000 - 1_200);
    const rows = await t.deps.db.select().from(aiRequests).where(eq(aiRequests.userId, userId));
    expect(rows.map((r) => [r.feature, r.credits, r.status])).toEqual([
      ["voice_act", 0, 200],
      ["voice_act", 0, 200],
    ]);
  });

  it("F-CN-02.2 set_setting with a key outside the closed list never reaches the app", async () => {
    const { bearer } = await proUser("voicesetting@tandem.test");
    const s = await mint(bearer);
    const off = await act(bearer, actBody(s.session_id, { request: "turn telemetry on" }));
    expect(await off.json()).toEqual({ tool_calls: [], say: "I can't do that by voice." });
    const ok = await act(bearer, actBody(s.session_id, { request: "switch to the dark theme" }));
    expect(((await ok.json()) as ActResponse).tool_calls).toEqual([
      { id: expect.any(String), name: "set_setting", args: { key: "theme", value: "dark" } },
    ]);
  });

  it("F-CN-02 the doer works only during a live call: an ended or expired call is 409", async () => {
    const { bearer } = await proUser("voiceended@tandem.test");
    const other = await proUser("voiceother@tandem.test");
    t.clock.freeze();
    const s = await mint(bearer);
    expect((await settle(bearer, s.session_id, 5)).status).toBe(200);
    const ended = await act(bearer, actBody(s.session_id));
    expect(ended.status).toBe(409);
    expect(await ended.json()).toMatchObject({ code: "SESSION_ENDED" });
    const late = await mint(bearer);
    t.clock.advance(30 * 60_000 + 1_000);
    expect((await act(bearer, actBody(late.session_id))).status).toBe(409);
    expect((await act(other.bearer, actBody(late.session_id))).status).toBe(404);
    expect(ai().calls).toHaveLength(0);
  });

  it("F-CN-02 act checks its body before any model call", async () => {
    const { bearer } = await proUser("voicebody@tandem.test");
    const s = await mint(bearer);
    const bad = [
      actBody(s.session_id, { request: "" }),
      actBody(s.session_id, { request: "x".repeat(2_001) }),
      actBody(s.session_id, { snapshot: { blob: "x".repeat(65_536) } }),
      actBody(s.session_id, { snapshot: ["not", "an", "object"] }),
      actBody(s.session_id, { results: [{ id: "a", ok: true, output: "x".repeat(8_193) }] }),
      actBody(s.session_id, {
        results: Array.from({ length: 65 }, (_, i) => ({ id: `r${i}`, ok: true, output: "" })),
      }),
    ];
    for (const body of bad) expect((await act(bearer, body)).status).toBe(422);
    expect(ai().calls).toHaveLength(0);
  });

  it("F-CN-02 the eighth round has to answer: no more tool calls", async () => {
    const { bearer } = await proUser("voicerounds@tandem.test");
    const s = await mint(bearer);
    let results: Array<{ id: string; ok: boolean; output: string }> = [];
    for (let round = 1; round <= 8; round++) {
      const res = await act(
        bearer,
        actBody(s.session_id, { request: "keep checking what's waiting", results }),
      );
      const out = (await res.json()) as ActResponse;
      const call = ai().calls.at(-1);
      expect(call?.messages[0]?.content).toContain(`Round ${round} of 8`);
      if (round < 8) {
        expect(call?.toolChoice).toBe("auto");
        expect(out.say).toBeUndefined();
        expect(out.tool_calls.map((c) => c.name)).toEqual(["get_waiting"]);
        results = [
          ...results,
          ...out.tool_calls.map((c) => ({ id: c.id, ok: true, output: "Nothing is waiting." })),
        ];
      } else {
        expect(call?.toolChoice).toBe("none");
        expect(out).toEqual({ tool_calls: [], say: "Done." });
      }
    }
  });

  it("F-CN-02 act keeps the hold ahead of a long call, and 402 ends a call that has run out", async () => {
    const rich = await proUser("voicetopup@tandem.test");
    const poor = await proUser("voicebroke@tandem.test", 1_200);
    t.clock.freeze();
    const s = await mint(rich.bearer);
    const p = await mint(poor.bearer);
    // 9 minutes in, within 2 minutes of the 10 the hold pays for: the doer takes 10 more.
    t.clock.advance(9 * 60_000);
    expect((await act(rich.bearer, actBody(s.session_id))).status).toBe(200);
    expect(await creditsOf(rich.userId)).toBe(10_000 - 2_400);
    // Nothing more to take, but the call is still inside what it paid for.
    expect((await act(poor.bearer, actBody(p.session_id))).status).toBe(200);
    t.clock.advance(60_000);
    expect(await (await settle(rich.bearer, s.session_id, 590)).json()).toEqual({
      charged: 1_180,
      refunded: 1_220,
    });
    expect(await creditsOf(rich.userId)).toBe(10_000 - 1_180);
    // 11 minutes in with no credits left: the doer refuses, and the app ends the call.
    t.clock.advance(60_000);
    const calls = ai().calls.length;
    const out = await act(poor.bearer, actBody(p.session_id));
    expect(out.status).toBe(402);
    expect(await out.json()).toMatchObject({ code: "CREDITS_EXHAUSTED" });
    expect(ai().calls).toHaveLength(calls);
    expect(await (await settle(poor.bearer, p.session_id, 660)).json()).toEqual({
      charged: 1_200,
      refunded: 0,
    });
  });

  it("F-CN-02 a doer failure is 503 and costs nothing", async () => {
    const { userId, bearer } = await proUser("voicedoerfail@tandem.test");
    const s = await mint(bearer);
    ai().failNext = true;
    const res = await act(bearer, actBody(s.session_id));
    expect(res.status).toBe(503);
    expect(await res.json()).toMatchObject({ code: "AI_UNAVAILABLE", retryable: true });
    expect(await creditsOf(userId)).toBe(10_000 - 1_200);
  });

  it("F-CN-03 requests, snapshots, results and tokens never reach the logs, and secrets never reach the doer", async () => {
    const { bearer } = await proUser("voicelog@tandem.test");
    const lines = captureLogs();
    const s = await mint(bearer);
    const first = await act(
      bearer,
      actBody(s.session_id, {
        request: "launch two claude agents for walrus-ember-42",
        snapshot: { ...SNAPSHOT, note: "walrus-ember-42" },
      }),
    );
    const r1 = (await first.json()) as ActResponse;
    const key = `sk-ant-api03-${"A".repeat(40)}`;
    const second = await act(
      bearer,
      actBody(s.session_id, {
        request: "launch two claude agents for walrus-ember-42",
        results: [
          { id: r1.tool_calls[0]?.id ?? "", ok: false, output: `walrus-ember-42 failed with ${key}` },
        ],
      }),
    );
    expect(second.status).toBe(200);
    await json(t, "POST", "/v1/voice/resolve", {
      bearer,
      body: { query: "walrus-ember-42", candidates: [{ id: "p1", kind: "pane", text: "walrus-ember-42" }] },
    });
    await settle(bearer, s.session_id, 30);
    const logged = lines.join("\n");
    expect(logged).not.toContain("walrus-ember-42");
    expect(logged).not.toContain(s.ephemeral_token);
    const sent = ai()
      .calls.map((c) => c.messages.map((m) => m.content).join("\n"))
      .join("\n");
    expect(sent).not.toContain(key);
    expect(sent).toContain("[REDACTED]");
  });
});

describe("F-CN-05 find work (/v1/voice/resolve)", () => {
  it("F-CN-05.1 'login testing' resolves to the 'auth tests' pane for 1 credit", async () => {
    const { userId, bearer } = await proUser("resolve@tandem.test");
    const res = await json(t, "POST", "/v1/voice/resolve", {
      bearer,
      body: { query: "login testing", candidates: CANDIDATES },
    });
    expect(res.status).toBe(200);
    const { matches } = (await res.json()) as { matches: Array<{ id: string; score: number }> };
    expect(matches[0]?.id).toBe("p1");
    expect(matches.every((m) => m.score > 0 && m.score <= 1)).toBe(true);
    expect(matches.map((m) => m.id)).not.toContain("p3");
    expect(await creditsOf(userId)).toBe(10_000 - 1);
    const call = ai().calls.at(-1);
    expect(call?.feature).toBe("voice_resolve");
    expect(call?.jsonSchema).toBeDefined();
    expect(call?.messages[0]?.content).toContain("<query>\nlogin testing\n</query>");
    const [row] = await t.deps.db.select().from(aiRequests).where(eq(aiRequests.userId, userId));
    expect(row).toMatchObject({ feature: "voice_resolve", credits: 1, status: 200 });
  });

  it("F-CN-05 resolve checks its body; nothing to rank is free; a failure refunds the credit", async () => {
    const { userId, bearer } = await proUser("resolvebad@tandem.test");
    const bad = [
      { query: "", candidates: CANDIDATES },
      { query: "x".repeat(501), candidates: CANDIDATES },
      {
        query: "docs",
        candidates: Array.from({ length: 201 }, (_, i) => ({ id: `c${i}`, kind: "pane", text: "x" })),
      },
      { query: "docs", candidates: [{ id: "c1", kind: "pane", text: "x".repeat(2_001) }] },
      { query: "docs", candidates: [{ id: "", kind: "pane", text: "x" }] },
    ];
    for (const body of bad)
      expect((await json(t, "POST", "/v1/voice/resolve", { bearer, body })).status).toBe(422);
    const empty = await json(t, "POST", "/v1/voice/resolve", {
      bearer,
      body: { query: "docs", candidates: [] },
    });
    expect(await empty.json()).toEqual({ matches: [] });
    expect(ai().calls).toHaveLength(0);
    ai().failNext = true;
    const res = await json(t, "POST", "/v1/voice/resolve", {
      bearer,
      body: { query: "login testing", candidates: CANDIDATES },
    });
    expect(res.status).toBe(503);
    expect(await creditsOf(userId)).toBe(10_000);
  });

  it("F-CN-05 is Pro only and costs a credit", async () => {
    const free = await signUpVerified(t, "resolvefree@tandem.test");
    const res = await json(t, "POST", "/v1/voice/resolve", {
      bearer: await bearerFor(t.deps, free.userId),
      body: { query: "docs", candidates: CANDIDATES },
    });
    expect(res.status).toBe(403);
    const broke = await proUser("resolvebroke@tandem.test", 0);
    const out = await json(t, "POST", "/v1/voice/resolve", {
      bearer: broke.bearer,
      body: { query: "docs", candidates: CANDIDATES },
    });
    expect(out.status).toBe(402);
    expect(ai().calls).toHaveLength(0);
  });

  it("F-CN-05 the model's ranking is held to the candidates", () => {
    const ids = new Set(["p1", "p2"]);
    expect(
      parseMatches(
        '{"matches":[{"id":"p2","score":0.4},{"id":"zz","score":0.9},{"id":"p1","score":1.7},{"id":"p2","score":0.1}]}',
        ids,
      ),
    ).toEqual([
      { id: "p1", score: 1 },
      { id: "p2", score: 0.4 },
    ]);
    expect(parseMatches('{"matches":[{"id":"p1","score":-2}]}', ids)).toEqual([]);
    expect(parseMatches('{"matches":[]}', ids)).toEqual([]);
    expect(parseMatches('{"matches":"p1"}', ids)).toBeNull();
    expect(parseMatches("not json", ids)).toBeNull();
  });
});
