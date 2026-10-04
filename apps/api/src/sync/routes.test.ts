import { describe, expect, it } from "vitest";
import { createApp } from "../app";
import { MemorySyncRepo } from "./memory";

const workspaceId = "ws-1";

function event(id: string, kind = "session.created") {
  return {
    id,
    workspaceId,
    kind,
    payload: { ok: true },
    createdAt: "2026-10-05T00:00:00.000Z",
  };
}

describe("sync", () => {
  it("appends idempotently and pages by cursor", async () => {
    const app = createApp({ sync: new MemorySyncRepo() });
    const firstId = "6b1f3c0e-1c2d-4b8a-9f0e-123456789abc";
    const secondId = "7c2a4d1f-2d3e-4c9b-8a1f-223456789abc";
    const created = await app.request("/v1/sync/events", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ events: [event(firstId), event(secondId, "pane.opened")] }),
    });
    expect(created.status).toBe(200);
    const createdBody = await created.json();
    expect(createdBody.items.map((item: { seq: number }) => item.seq)).toEqual([1, 2]);

    const replay = await app.request("/v1/sync/events", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ events: [event(firstId)] }),
    });
    const replayBody = await replay.json();
    expect(replayBody.items[0].seq).toBe(1);

    const page = await app.request(
      `/v1/sync/events?workspaceId=${workspaceId}&after=1&limit=1`,
    );
    expect(page.status).toBe(200);
    const body = await page.json();
    expect(body.items).toHaveLength(1);
    expect(body.items[0].id).toBe(secondId);
    expect(body.nextCursor).toBeNull();
  });

  it("rejects an invalid event and an oversized body", async () => {
    const app = createApp();
    const bad = await app.request("/v1/sync/events", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ events: [{ id: "nope" }] }),
    });
    expect(bad.status).toBe(400);
    const problem = await bad.json();
    expect(problem.title).toBe("Bad Request");

    const huge = await app.request("/v1/sync/events", {
      method: "POST",
      headers: { "content-type": "application/json", "content-length": String(1_100_000) },
      body: "x".repeat(1_100_000),
    });
    expect(huge.status).toBe(413);
  });
});
