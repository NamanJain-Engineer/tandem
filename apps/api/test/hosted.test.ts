import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { grantMonthly } from "../src/services/ledger";
import { form, opus } from "./audio";
import { bearerFor, json, makeTestApp, setSubscription, signUpVerified, type TestEnv } from "./helpers";

let t: TestEnv;
beforeEach(async () => {
  t = await makeTestApp();
});
afterEach(async () => {
  vi.restoreAllMocks();
  await t.close();
});

/** Every hosted (model- or provider-backed) route and the plan feature that guards it. */
const GUARDED: Array<{
  method: string;
  path: string;
  feature: string;
  send: (bearer: string) => Promise<Response>;
}> = [
  {
    method: "POST",
    path: "/v1/ai/rewrite",
    feature: "enhance",
    send: (bearer) => json(t, "POST", "/v1/ai/rewrite", { bearer, body: { mode: "polish", text: "hi" } }),
  },
  {
    method: "POST",
    path: "/v1/ai/teammate-draft",
    feature: "enhance",
    send: (bearer) =>
      json(t, "POST", "/v1/ai/teammate-draft", { bearer, body: { description: "x", engines: ["claude"] } }),
  },
  {
    method: "POST",
    path: "/v1/auto/next",
    feature: "auto",
    send: (bearer) =>
      json(t, "POST", "/v1/auto/next", {
        bearer,
        body: { mode: "finish", turn: 0, ceiling: 20, scrollback: "> " },
      }),
  },
  {
    method: "POST",
    path: "/v1/stt/transcribe",
    feature: "cloudStt",
    send: async (bearer) => {
      const { body, type } = form([{ name: "audio", file: opus(2), type: "audio/ogg", filename: "a.opus" }]);
      return t.app.request("/v1/stt/transcribe", {
        method: "POST",
        headers: { authorization: `Bearer ${bearer}`, "content-type": type },
        body,
      });
    },
  },
];
/** Hosted routes that are free for everyone signed in (rate-limited instead). */
const FREE = ["POST /v1/ai/title"];
const HOSTED_PREFIXES = ["/v1/ai/", "/v1/stt/", "/v1/auto/", "/v1/voice/"];

describe("M13 exit: every hosted route is in the requirePlan table", () => {
  it("the table covers every hosted route the app mounts", () => {
    const mounted = t.app.routes
      .filter((r) => HOSTED_PREFIXES.some((p) => r.path.startsWith(p)) && r.method !== "ALL")
      .map((r) => `${r.method} ${r.path}`);
    const listed = [...GUARDED.map((g) => `${g.method} ${g.path}`), ...FREE];
    expect([...new Set(mounted)].sort()).toEqual(listed.sort());
  });

  it.each(GUARDED.map((g) => [g.path, g] as const))(
    "%s is 403 FORBIDDEN_PLAN on Free, even claiming Pro",
    async (_p, g) => {
      const { userId } = await signUpVerified(
        t,
        `free-${g.feature.toLowerCase()}-${g.path.length}@tandem.test`,
      );
      const bearer = await bearerFor(t.deps, userId);
      const res = await g.send(bearer);
      expect(res.status).toBe(403);
      expect(await res.json()).toMatchObject({ code: "FORBIDDEN_PLAN", feature: g.feature });
    },
  );

  it("a settled call tells the desktop its balance changed (SSE `credits`)", async () => {
    const { userId } = await signUpVerified(t, "sse@tandem.test");
    await setSubscription(t.deps, userId);
    await t.deps.db.transaction((tx) =>
      grantMonthly(tx, { kind: "user", id: userId }, 1, new Date(t.clock.nowMs() + 86_400_000), "inv_sse"),
    );
    const publish = vi.spyOn(t.deps.events, "publish");
    const res = await json(t, "POST", "/v1/ai/rewrite", {
      bearer: await bearerFor(t.deps, userId),
      body: { mode: "polish", text: "hi" },
    });
    expect(res.status).toBe(200);
    expect(publish).toHaveBeenCalledWith(userId, "credits");
  });
});
