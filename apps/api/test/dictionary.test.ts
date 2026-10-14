import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { bearerFor, json, makeTestApp, setSubscription, signUpVerified, type TestEnv } from "./helpers";

let t: TestEnv;
beforeEach(async () => {
  t = await makeTestApp();
});
afterEach(async () => {
  await t.close();
});

type Change = { rule_id: string; payload: Record<string, unknown>; deleted: boolean; updated_at: string };
type Sync = {
  rev: number;
  changes: Array<Change & { rev: number }>;
  conflicts: Array<{ rule_id: string; local: Change; remote: Change & { rev: number } }>;
};

async function proUser(email: string) {
  const { userId } = await signUpVerified(t, email);
  await setSubscription(t.deps, userId);
  return bearerFor(t.deps, userId);
}

const at = (minutes: number) => new Date(Date.UTC(2026, 9, 14, 12, minutes)).toISOString();
const rule = (id: string, to: string, minutes: number, deleted = false): Change => ({
  rule_id: id,
  payload: { kind: "replace", from: id, to },
  deleted,
  updated_at: at(minutes),
});

async function sync(bearer: string, since_rev: number, changes: Change[]) {
  const res = await json(t, "POST", "/v1/dictionary/sync", { bearer, body: { since_rev, changes } });
  return { status: res.status, body: (await res.json()) as Sync };
}

describe("F-DI-09 dictionary sync (POST /v1/dictionary/sync)", () => {
  it("F-DI-09 is Pro: Free gets FORBIDDEN_PLAN and nothing is stored", async () => {
    const { userId } = await signUpVerified(t, "free@tandem.test");
    const res = await json(t, "POST", "/v1/dictionary/sync", {
      bearer: await bearerFor(t.deps, userId),
      body: { since_rev: 0, changes: [rule("tauri", "Tauri", 1)] },
    });
    expect(res.status).toBe(403);
    expect(await res.json()).toMatchObject({ code: "FORBIDDEN_PLAN" });
  });

  it("F-DI-09 one device's rules reach another, with tombstones", async () => {
    const bearer = await proUser("sync@tandem.test");
    const a = await sync(bearer, 0, [rule("tauri", "Tauri", 1), rule("pnpm", "pnpm", 2)]);
    expect(a.status).toBe(200);
    expect(a.body.changes).toEqual([]);
    expect(a.body.conflicts).toEqual([]);
    expect(a.body.rev).toBe(2);
    // Another device, from nothing: both rules.
    const b = await sync(bearer, 0, []);
    expect(b.body.changes.map((c) => c.rule_id).sort()).toEqual(["pnpm", "tauri"]);
    expect(b.body.changes.find((c) => c.rule_id === "tauri")?.payload).toEqual({ kind: "replace", from: "tauri", to: "Tauri" });
    // A deletion travels as a tombstone.
    const del = await sync(bearer, b.body.rev, [rule("pnpm", "pnpm", 3, true)]);
    expect(del.body.rev).toBe(3);
    const a2 = await sync(bearer, a.body.rev, []);
    expect(a2.body.changes).toEqual([expect.objectContaining({ rule_id: "pnpm", deleted: true, rev: 3 })]);
  });

  it("F-DI-09 the last writer wins per rule; an older change than the server's newer one is a conflict", async () => {
    const bearer = await proUser("lww@tandem.test");
    const first = await sync(bearer, 0, [rule("tauri", "Tauri", 1)]);
    // Device B edits it later.
    await sync(bearer, first.body.rev, [rule("tauri", "TAURI", 10)]);
    // Device A, still at the first rev, sends an edit made before B's: the
    // server keeps B's and returns the conflict for review.
    const a = await sync(bearer, first.body.rev, [rule("tauri", "Tauri.", 5)]);
    expect(a.body.conflicts).toEqual([
      expect.objectContaining({
        rule_id: "tauri",
        local: expect.objectContaining({ payload: { kind: "replace", from: "tauri", to: "Tauri." } }),
        remote: expect.objectContaining({ payload: { kind: "replace", from: "tauri", to: "TAURI" } }),
      }),
    ]);
    const now = await sync(bearer, 0, []);
    expect(now.body.changes.find((c) => c.rule_id === "tauri")?.payload).toMatchObject({ to: "TAURI" });
    // "Keep mine": A sends it again, now the newest write: it wins.
    const keep = await sync(bearer, a.body.rev, [rule("tauri", "Tauri.", 20)]);
    expect(keep.body.conflicts).toEqual([]);
    const after = await sync(bearer, 0, []);
    expect(after.body.changes.find((c) => c.rule_id === "tauri")?.payload).toMatchObject({ to: "Tauri." });
  });

  it("F-DI-09 transcript evidence never syncs, users are separate, and batches are bounded", async () => {
    const bearer = await proUser("one@tandem.test");
    const other = await proUser("two@tandem.test");
    await sync(bearer, 0, [
      { ...rule("tauri", "Tauri", 1), payload: { kind: "replace", from: "tory", to: "Tauri", evidence: ["I use tory daily"] } },
    ]);
    const pulled = await sync(bearer, 0, []);
    expect(pulled.body.changes[0]?.payload).toEqual({ kind: "replace", from: "tory", to: "Tauri" });
    expect((await sync(other, 0, [])).body.changes).toEqual([]);
    const many = Array.from({ length: 501 }, (_, i) => rule(`r${i}`, "x", 1));
    expect((await sync(bearer, 0, many)).status).toBe(422);
  });
});
