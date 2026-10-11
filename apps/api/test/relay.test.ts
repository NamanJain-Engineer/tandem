import { readFileSync } from "node:fs";
import type { AddressInfo } from "node:net";
import { serve, type ServerType } from "@hono/node-server";
import { eq } from "drizzle-orm";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import WebSocket from "ws";
import { DEFAULT_POLICY } from "@tandem/protocol/team";
import { pairings, subscriptions } from "../src/db/schema";
import { logger } from "../src/lib/logger";
import { Relay } from "../src/routes/relay";
import { membershipOf } from "../src/services/membership";
import { relayAuthenticator } from "../src/services/pairings";
import { bearerFor, json, makeTestApp, setSubscription, signUpVerified, type TestEnv } from "./helpers";

let t: TestEnv;
let server: ServerType;
let relay: Relay;
let port = 0;
const open: WebSocket[] = [];

beforeEach(async () => {
  t = await makeTestApp();
  await t.deps.db.delete(pairings);
  relay = new Relay({ authenticate: relayAuthenticator(t.deps), control: t.deps.relay, now: () => Date.now() });
  server = serve({ fetch: t.app.fetch, port: 0, hostname: "127.0.0.1" });
  relay.attach(server);
  await new Promise<void>((resolve) => server.once("listening", () => resolve()));
  port = (server.address() as AddressInfo).port;
});
afterEach(async () => {
  vi.restoreAllMocks();
  for (const ws of open.splice(0)) ws.terminate();
  relay.close();
  await new Promise<void>((resolve) => server.close(() => resolve()));
  await t.close();
});

async function proUser(email: string) {
  const { userId } = await signUpVerified(t, email);
  await setSubscription(t.deps, userId);
  return { userId, bearer: await bearerFor(t.deps, userId) };
}

let seq = 0;
async function pair(bearer: string) {
  const pairingId = `pr_test${++seq}${Date.now().toString(36)}`;
  const res = await json(t, "POST", "/v1/pairings", { bearer, body: { pairing_id: pairingId, device_label: "Phone" } });
  expect(res.status).toBe(200);
  const body = (await res.json()) as { pairing_token: string; relay_url: string; expires_at: string };
  return { pairingId, ...body };
}

type Conn = { ws: WebSocket; binary: Buffer[]; text: unknown[]; closed: Promise<{ code: number; reason: string }> };

function connect(pairingId: string, auth: { bearer?: string; token?: string }): Promise<Conn> {
  const headers: Record<string, string> = {};
  if (auth.bearer) headers.authorization = `Bearer ${auth.bearer}`;
  const protocols = auth.token ? [`tandem-pair.${auth.token}`] : [];
  const ws = new WebSocket(`ws://127.0.0.1:${port}/v1/relay/${pairingId}`, protocols, { headers });
  open.push(ws);
  const binary: Buffer[] = [];
  const text: unknown[] = [];
  ws.on("message", (data, isBinary) => {
    if (isBinary) binary.push(Buffer.from(data as Buffer));
    else text.push(JSON.parse(String(data)));
  });
  const closed = new Promise<{ code: number; reason: string }>((resolve) =>
    ws.on("close", (code, reason) => resolve({ code, reason: String(reason) })),
  );
  return new Promise((resolve, reject) => {
    ws.once("open", () => resolve({ ws, binary, text, closed }));
    ws.once("error", reject);
  });
}

async function until(cond: () => boolean, ms = 3000) {
  const end = Date.now() + ms;
  while (!cond()) {
    if (Date.now() > end) throw new Error("timed out");
    await new Promise((r) => setTimeout(r, 10));
  }
}

async function bothEnds(email: string) {
  const { userId, bearer } = await proUser(email);
  const p = await pair(bearer);
  const desktop = await connect(p.pairingId, { bearer });
  const phone = await connect(p.pairingId, { token: p.pairing_token });
  await until(() => desktop.text.length > 0 && phone.text.length > 0);
  return { userId, bearer, p, desktop, phone };
}

describe("F-BE-11 pairings (/v1/pairings)", () => {
  it("F-BE-11 a Pro desktop registers a pairing and gets a token and the relay URL", async () => {
    const { userId, bearer } = await proUser("pair@tandem.test");
    const p = await pair(bearer);
    expect(p.pairing_token).toMatch(/^[A-Za-z0-9_-]{43}$/);
    expect(p.relay_url).toBe(`ws://127.0.0.1:8787/v1/relay/${p.pairingId}`);
    const ttl = new Date(p.expires_at).getTime() - Date.now();
    expect(ttl).toBeGreaterThan(9 * 60_000);
    expect(ttl).toBeLessThanOrEqual(10 * 60_000);
    const [row] = await t.deps.db.select().from(pairings).where(eq(pairings.id, p.pairingId));
    expect(row?.userId).toBe(userId);
    // Only a hash of the token is stored.
    expect(row?.pairingTokenHash).not.toContain(p.pairing_token);
  });

  it("F-BE-11 the Free plan gets 403 FORBIDDEN_PLAN", async () => {
    const { userId } = await signUpVerified(t, "pairfree@tandem.test");
    const bearer = await bearerFor(t.deps, userId);
    const res = await json(t, "POST", "/v1/pairings", { bearer, body: { pairing_id: "pr_free1" } });
    expect(res.status).toBe(403);
    expect(await res.json()).toMatchObject({ code: "FORBIDDEN_PLAN" });
  });

  it("F-BE-11 a Team admin's disablePhone policy refuses new pairings", async () => {
    const owner = await signUpVerified(t, "pairteam@tandem.test");
    const res = await json(t, "POST", "/v1/billing/checkout", {
      jar: owner.jar,
      body: { plan: "team", interval: "month", seats: 2 },
    });
    const sessionId = new URL(((await res.json()) as { url: string }).url).searchParams.get("session") ?? "";
    await json(t, "POST", "/v1/billing/mock/complete", { jar: owner.jar, body: { sessionId } });
    const orgId = (await membershipOf(t.deps.db, owner.userId))?.orgId ?? "";
    const bearer = await bearerFor(t.deps, owner.userId);
    const put = await json(t, "PUT", `/v1/orgs/${orgId}/policy`, {
      jar: owner.jar,
      body: { ...DEFAULT_POLICY, disablePhone: true },
    });
    expect(put.status).toBe(200);
    const refused = await json(t, "POST", "/v1/pairings", { bearer, body: { pairing_id: "pr_teamoff" } });
    expect(refused.status).toBe(403);
    expect(await refused.json()).toMatchObject({ code: "FORBIDDEN_POLICY" });
  });

  it("F-BE-11 a malformed pairing id is 422 and a taken one is 409", async () => {
    const { bearer } = await proUser("pairbad@tandem.test");
    const bad = await json(t, "POST", "/v1/pairings", { bearer, body: { pairing_id: "not-a-pairing" } });
    expect(bad.status).toBe(422);
    const p = await pair(bearer);
    const again = await json(t, "POST", "/v1/pairings", { bearer, body: { pairing_id: p.pairingId } });
    expect(again.status).toBe(409);
  });

  it("F-BE-11 only the owner can delete a pairing; deleting is 204 and idempotent", async () => {
    const a = await proUser("pairowner@tandem.test");
    const b = await proUser("pairother@tandem.test");
    const p = await pair(a.bearer);
    expect((await json(t, "DELETE", `/v1/pairings/${p.pairingId}`, { bearer: b.bearer })).status).toBe(404);
    expect((await json(t, "DELETE", `/v1/pairings/${p.pairingId}`, { bearer: a.bearer })).status).toBe(204);
    expect((await json(t, "DELETE", `/v1/pairings/${p.pairingId}`, { bearer: a.bearer })).status).toBe(204);
    const [row] = await t.deps.db.select().from(pairings).where(eq(pairings.id, p.pairingId));
    expect(row?.revokedAt).toBeInstanceOf(Date);
  });
});

describe("F-BE-11 the relay (/v1/relay/:pairing_id)", () => {
  it("F-BE-11.1 frames are forwarded unchanged in both directions", async () => {
    const { desktop, phone } = await bothEnds("relayfwd@tandem.test");
    expect(phone.text).toContainEqual({ type: "peer", up: true });
    const toPhone = Buffer.from(Array.from({ length: 4096 }, (_, i) => (i * 7) % 256));
    const toDesktop = Buffer.from([0, 1, 2, 255, 254, 0]);
    desktop.ws.send(toPhone);
    phone.ws.send(toDesktop);
    await until(() => phone.binary.length === 1 && desktop.binary.length === 1);
    expect(phone.binary[0]?.equals(toPhone)).toBe(true);
    expect(desktop.binary[0]?.equals(toDesktop)).toBe(true);
  });

  it("F-BE-11.1 a 64 KB frame passes and a 65 KB frame closes the socket with 1009", async () => {
    const { desktop, phone } = await bothEnds("relaybig@tandem.test");
    desktop.ws.send(Buffer.alloc(64 * 1024, 1));
    await until(() => phone.binary.length === 1);
    expect(phone.binary[0]?.length).toBe(64 * 1024);
    phone.ws.send(Buffer.alloc(65 * 1024, 2));
    expect((await phone.closed).code).toBe(1009);
    expect(desktop.binary).toHaveLength(0);
  });

  it("F-BE-11 text frames from either end are refused: the relay carries only opaque binary", async () => {
    const { desktop, phone } = await bothEnds("relaytext@tandem.test");
    phone.ws.send("hello in the clear");
    expect((await phone.closed).code).toBe(1003);
    expect(desktop.binary).toHaveLength(0);
  });

  it("F-BE-11 both ends authenticate: no credentials, a wrong token or another user's bearer close with 1008", async () => {
    const owner = await proUser("relayauth@tandem.test");
    const other = await proUser("relayintruder@tandem.test");
    const p = await pair(owner.bearer);
    for (const auth of [{}, { token: "x".repeat(43) }, { bearer: other.bearer }, { bearer: "not-a-jwt" }]) {
      const c = await connect(p.pairingId, auth);
      expect((await c.closed).code).toBe(1008);
    }
    const unknown = await connect("pr_doesnotexist", { bearer: owner.bearer });
    expect((await unknown.closed).code).toBe(1008);
  });

  it("F-BE-11 the phone's subprotocol is echoed so browsers accept the socket", async () => {
    const { phone, p } = await bothEnds("relayproto@tandem.test");
    expect(phone.ws.protocol).toBe(`tandem-pair.${p.pairing_token}`);
  });

  it("F-BE-11 at most one desktop and one phone: a second phone replaces the first", async () => {
    const { p, phone, desktop } = await bothEnds("relayone@tandem.test");
    const phone2 = await connect(p.pairingId, { token: p.pairing_token });
    expect((await phone.closed).code).toBe(4000);
    desktop.ws.send(Buffer.from([9]));
    await until(() => phone2.binary.length === 1);
  });

  it("F-BE-11 the other end hears when its peer leaves, and frames with no peer are dropped", async () => {
    const { desktop, phone } = await bothEnds("relaypeer@tandem.test");
    desktop.ws.close();
    await until(() => phone.text.some((m) => JSON.stringify(m) === JSON.stringify({ type: "peer", up: false })));
    phone.ws.send(Buffer.from([1, 2, 3]));
    await new Promise((r) => setTimeout(r, 50));
    expect(phone.ws.readyState).toBe(WebSocket.OPEN);
  });

  it("F-BE-11 deleting the pairing drops both sockets, and later connections are refused", async () => {
    const { bearer, p, desktop, phone } = await bothEnds("relayrevoke@tandem.test");
    expect((await json(t, "DELETE", `/v1/pairings/${p.pairingId}`, { bearer })).status).toBe(204);
    expect((await desktop.closed).code).toBe(1008);
    expect((await phone.closed).code).toBe(1008);
    const again = await connect(p.pairingId, { token: p.pairing_token });
    expect((await again.closed).code).toBe(1008);
  });

  it("F-BE-11 a lapsed plan can't connect", async () => {
    const { userId, bearer } = await proUser("relaylapse@tandem.test");
    const p = await pair(bearer);
    await t.deps.db
      .update(subscriptions)
      .set({ status: "canceled", currentPeriodEnd: new Date(Date.now() - 86_400_000) })
      .where(eq(subscriptions.ownerId, userId));
    const c = await connect(p.pairingId, { token: p.pairing_token });
    expect((await c.closed).code).toBe(1008);
  });

  it("F-BE-11 per-pairing rate limit: a burst over 50 frames a second is cut down", async () => {
    const { desktop, phone } = await bothEnds("relayrate@tandem.test");
    for (let i = 0; i < 120; i++) desktop.ws.send(Buffer.from([i]));
    await until(() => desktop.text.some((m) => (m as { type?: string }).type === "rate_limited"));
    await new Promise((r) => setTimeout(r, 100));
    expect(phone.binary.length).toBeGreaterThanOrEqual(50);
    expect(phone.binary.length).toBeLessThan(120);
  });

  it("F-BE-11 the relay logs connection metadata only, never frame bytes", async () => {
    const lines: string[] = [];
    for (const level of ["info", "warn", "debug", "error"] as const) {
      vi.spyOn(logger, level).mockImplementation(((...args: unknown[]) => {
        lines.push(JSON.stringify(args));
      }) as never);
    }
    const sentence = "The quick brown fox pairs with the phone at dawn";
    const { desktop, phone } = await bothEnds("relaylog@tandem.test");
    desktop.ws.send(Buffer.from(sentence));
    await until(() => phone.binary.length === 1);
    desktop.ws.close();
    await until(() => lines.some((l) => l.includes("relay disconnect")));
    expect(lines.some((l) => l.includes("relay connect"))).toBe(true);
    expect(lines.join("\n")).not.toContain(sentence);
    expect(lines.join("\n")).not.toContain(Buffer.from(sentence).toString("base64"));
  });

  it("F-BE-11.2 the relay module holds no key material: it imports no crypto", () => {
    const source = readFileSync(new URL("../src/routes/relay.ts", import.meta.url), "utf8");
    const imports = [...source.matchAll(/^\s*import[^"']*["']([^"']+)["']/gm)].map((m) => m[1] ?? "");
    expect(imports.length).toBeGreaterThan(0);
    for (const spec of imports) {
      expect(spec).not.toMatch(/^(node:)?crypto$|^@noble\/|^jose$|libsodium|tweetnacl|^\.\.\/lib\/ids$/);
    }
    expect(source).not.toMatch(/require\(|import\(|createHash|subtle|createCipher/);
  });
});
