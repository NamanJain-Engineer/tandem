import { readFileSync } from "node:fs";
import { afterEach, describe, expect, it, vi } from "vitest";
import { logger } from "../src/lib/logger";
import { MockPush } from "../src/providers/push";
import { bearerFor, json, makeTestApp, setSubscription, signUpVerified, type TestEnv } from "./helpers";

let t: TestEnv | undefined;
afterEach(async () => {
  vi.restoreAllMocks();
  await t?.close();
  t = undefined;
});

async function setup() {
  const push = new MockPush();
  const env = await makeTestApp({}, { push });
  t = env;
  const { userId } = await signUpVerified(env, `push${Date.now()}@tandem.test`);
  await setSubscription(env.deps, userId);
  const bearer = await bearerFor(env.deps, userId);
  const pairingId = `pr_push${Date.now().toString(36)}`;
  const res = await json(env, "POST", "/v1/pairings", { bearer, body: { pairing_id: pairingId } });
  expect(res.status).toBe(200);
  return { env, push, bearer, pairingId };
}

const SEALED = "AwEAAAAAAAAAAQ".padEnd(120, "x");

describe("F-RM-06 phone push forwarding", () => {
  it("F-RM-06.2 forwards the sealed bytes and logs no payload or token", async () => {
    const { env, push, bearer, pairingId } = await setup();
    const lines: string[] = [];
    vi.spyOn(logger, "info").mockImplementation(((obj: unknown, msg?: string) => {
      lines.push(JSON.stringify(obj) + (msg ?? ""));
    }) as never);
    const res = await json(env, "POST", `/v1/pairings/${pairingId}/push`, {
      bearer,
      body: { platform: "fcm", token: "fcm-token-123456", payload: SEALED },
    });
    expect(res.status).toBe(202);
    expect(push.sent).toEqual([{ platform: "fcm", token: "fcm-token-123456", payload: SEALED }]);
    expect(lines.join("\n")).not.toContain(SEALED);
    expect(lines.join("\n")).not.toContain("fcm-token-123456");
  });

  it("F-RM-06.3 refuses another user's or a revoked pairing; a dead token answers 410", async () => {
    const { env, bearer, pairingId } = await setup();
    const { userId } = await signUpVerified(env, `other${Date.now()}@tandem.test`);
    await setSubscription(env.deps, userId);
    const other = await bearerFor(env.deps, userId);
    const body = { platform: "fcm", token: "fcm-token-123456", payload: SEALED };
    expect((await json(env, "POST", `/v1/pairings/${pairingId}/push`, { bearer: other, body })).status).toBe(404);
    const dead = await json(env, "POST", `/v1/pairings/${pairingId}/push`, { bearer, body: { ...body, token: "dead-token-1" } });
    expect(dead.status).toBe(410);
    expect((await json(env, "DELETE", `/v1/pairings/${pairingId}`, { bearer })).status).toBe(204);
    expect((await json(env, "POST", `/v1/pairings/${pairingId}/push`, { bearer, body })).status).toBe(404);
  });

  it("F-RM-06.2 rejects payloads that aren't sealed-looking base64url or are too big", async () => {
    const { env, bearer, pairingId } = await setup();
    for (const payload of ["has spaces", "x".repeat(3001)]) {
      const res = await json(env, "POST", `/v1/pairings/${pairingId}/push`, {
        bearer,
        body: { platform: "fcm", token: "fcm-token-123456", payload },
      });
      expect(res.status).toBe(422);
    }
  });

  it("F-RM-06.2 the push path imports no crypto", () => {
    for (const f of ["../src/services/push.ts", "../src/routes/pairings.ts"]) {
      const src = readFileSync(new URL(f, import.meta.url), "utf8");
      expect(src, f).not.toMatch(/from "(node:)?crypto"|@noble|from "jose"/);
    }
  });
});
