import type { Redis } from "ioredis";
import { describe, expect, it } from "vitest";
import { EventHub, type MeEventKind } from "./events";

type PMessage = (pattern: string, channel: string, message: string) => void;

/** Enough of ioredis's pub/sub for the hub: a publish reaches every pmessage handler. */
function fakeRedis(): { pub: Redis; sub: Redis } {
  const handlers: PMessage[] = [];
  const pub = {
    publish: async (channel: string, message: string) => {
      for (const h of handlers) h("me:*", channel, message);
      return handlers.length;
    },
  };
  const sub = {
    psubscribe: async () => 1,
    punsubscribe: async () => 1,
    on: (_event: string, h: PMessage) => {
      handlers.push(h);
      return sub;
    },
  };
  return { pub: pub as unknown as Redis, sub: sub as unknown as Redis };
}

describe("F-BE-04 plan and credit invalidations", () => {
  it("F-BE-04 a closed stream's late cleanup never silences the stream that replaced it (D-865)", async () => {
    const { pub, sub } = fakeRedis();
    const hub = new EventHub(pub, sub);
    const old: MeEventKind[] = [];
    const live: MeEventKind[] = [];
    // The desktop reconnects: its old stream is aborted, then a new one opens.
    const stopOld = await hub.listen("u1", (kind) => old.push(kind));
    stopOld();
    const stopLive = await hub.listen("u1", (kind) => live.push(kind));
    // Up to 25 s later the old stream's heartbeat loop wakes, sees the abort
    // and stops again.
    stopOld();
    await hub.publish("u1", "credits");
    expect(live).toEqual(["credits"]);
    expect(old).toEqual([]);
    stopLive();
    await hub.publish("u1", "entitlement");
    expect(live).toEqual(["credits"]);
  });
});
