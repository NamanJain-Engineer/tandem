import canonicalize from "canonicalize";
import { verify } from "node:crypto";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { makeTestApp, type TestEnv } from "./helpers";

let t: TestEnv;
beforeEach(async () => {
  t = await makeTestApp();
});
afterEach(async () => {
  await t.close();
});

describe("F-BE-09 remote config", () => {
  it("serves a signed canonical payload that verifies with the config key", async () => {
    const res = await t.app.request("/v1/config");
    expect(res.status).toBe(200);
    expect(res.headers.get("cache-control")).toContain("max-age=600");
    const body = (await res.json()) as { payload: string; sig: string; kid: string };
    expect(body.kid).toBe("ck-1");
    const parsed = JSON.parse(body.payload) as { prices: Record<string, number> };
    expect(canonicalize(parsed)).toBe(body.payload);
    expect(parsed.prices).toMatchObject({ stt_15s: 2, rewrite: 3, voice_10s: 20 });
    const key = t.deps.keys.config[0]?.publicKey;
    if (!key) throw new Error("no key");
    expect(verify(null, Buffer.from(body.payload), key, Buffer.from(body.sig, "base64"))).toBe(true);
    expect(
      verify(null, Buffer.from(body.payload.replace("20", "2")), key, Buffer.from(body.sig, "base64")),
    ).toBe(false);
  });

  it("F-BE-09.2 an ETag revalidation returns 304", async () => {
    const first = await t.app.request("/v1/config");
    const etag = first.headers.get("etag");
    expect(etag).toMatch(/^"[0-9a-f]{64}"$/);
    const second = await t.app.request("/v1/config", { headers: { "if-none-match": etag ?? "" } });
    expect(second.status).toBe(304);
  });
});
